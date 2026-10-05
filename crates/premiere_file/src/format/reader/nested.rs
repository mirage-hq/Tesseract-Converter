//! Nested-sequence placements: a video track item whose source is another sequence.
//!
//! A nest is read by the placement rules shared with media
//! (`video::read_placement`), clip Enable and track output included. It
//! converts as a plain forward placement or bounded unit reverse; reverse
//! reflects the saved window about OriginalDuration, never the current end.
//! Forward placements use one constant speed that their
//! saved input window confirms by the media rule ([`source_span_matches`]: the
//! speed is source time over placement time, whatever the inner frame rate),
//! optionally with a bounded forward source curve on a top-level placement,
//! intrinsic Motion (static or keyed, without a Track Matte Key), Opacity
//! (static, or keyed unless the nest is retimed), default or standalone static
//! zero-feather Crop, any Blend Mode, and bounded occurrence effects under the
//! [nested occurrence effect rules](../../../README.md#nested-occurrence-effects).
//! Any canvas and frame rate are admitted, though a Track Matte Key only on the
//! outer canvas. The ordinary sequence reader reads its inner timeline, at most
//! [`MAX_NEST_DEPTH`] levels deep.
//! Repeated placements share their media records and reuse one read of
//! their inner sequence per nesting depth that remains below them, so a
//! top-level read reads each sequence at most once per depth. A placement of
//! a timeline on a nesting cycle is omitted, and so is one that closes a cycle
//! the project topology missed.
//!
//! A placement is checked against that one read and stays a [`NestPlacement`]
//! until its track order keeps it; only then is the read copied.
//!
//! A copy keeps the inner sound. Premiere plays it only through the nest's own
//! audio item, which [`pair_sounds`] finds once the outer tracks are read. An
//! audio item that no nest's group carries, one with Level keys included,
//! plays that sound alone, as sound of the sequence that holds it
//! ([`play_alone`]).

use super::{
    animation::chain_components,
    effects, required_integer, sequence,
    video::{read_placement, report_unknown_children, Placement},
};
use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{graph::Element, Graph, Located, Record},
    omit,
    schema::{
        native::VideoClipTrackItem, occurrence_edits, records, source_span_matches, ClipEdits,
        FrameRate, MediaId, OccurrenceEdit, PrAudioFade, PrAudioOccurrence, PrBlendMode,
        PrKeyframeEasing, PrMedia, PrNestOccurrence, PrPropertyAnimation, PrScalarKeyframe,
        PrSequence, PrStaticCrop, PrStaticTransform, PrTrackMatte, PrVideoItem, PrVideoTrack,
        PrVolumeKeys, MAX_NEST_DEPTH, SOURCE_CHAIN_NOT_CONVERTED,
    },
    Omission, OmissionScope,
};
use serde::de::IgnoredAny;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

/// The video track that holds a placement, with its track group's format.
pub(super) struct Parent<'a> {
    pub(super) frame_rate: FrameRate,
    pub(super) dimensions: [u32; 2],
    /// Whether the track's video output is on.
    pub(super) track_output: bool,
    /// Native video track `Index`. Premiere labels index 0 as V1.
    pub(super) track_index: i64,
    /// The position among the group's kept video tracks (the sequence's
    /// track index, not the native `Index`) of each track by its persistent
    /// `Track/ID`, which a Track Matte Key names; `None` for an ID that more
    /// than one track carries.
    pub(super) track_ids: &'a BTreeMap<usize, Option<usize>>,
    /// Whether the sequence is the inner timeline of a nest placement, not
    /// the sequence that the read selected.
    pub(super) nested: bool,
}

impl Parent<'_> {
    /// The kept-track position of the video track whose persistent `Track/ID`
    /// a Track Matte Key of `item` names.
    pub(super) fn matte_track_index(&self, matte_track_id: usize, item: &str) -> Result<usize> {
        match self.track_ids.get(&matte_track_id) {
            Some(Some(index)) => Ok(*index),
            Some(None) => Err(unsupported(format!(
                "{item}: Track Matte Key Matte {matte_track_id} names a video track ID that more than one track carries"
            ))),
            None => Err(unsupported(format!(
                "{item}: Track Matte Key Matte {matte_track_id} names no video track of the sequence"
            ))),
        }
    }
}

/// A finished inner read, shared by every placement at its remaining depth.
struct InnerRead {
    /// The read timeline as a placement. Each placement is checked by writing
    /// its ranges here, and each kept one copies it.
    nest: PrNestOccurrence,
}

/// The audio item of a nested placement: an audio track item that plays the
/// stereo mix of the sequence `sequence`, read with its gain and Level keys
/// (`audio::read_tracks`).
pub(super) struct NestSound {
    pub(super) id: String,
    pub(super) sequence: String,
    pub(super) timeline: Range<i64>,
    /// The inner range that the item plays: from its In, at normal speed,
    /// for the length of `timeline`, before its Out when its tail is
    /// shortened.
    pub(super) source: Range<i64>,
    /// Every static stage (clip Level, Clip Gain, source, track and master),
    /// linear with 1.0 at 0 dB; a muted track or Volume makes it 0. With
    /// Level keys, it holds the Level's value before its keys.
    pub(super) gain: f64,
    /// Keyed clip Volume `Level`, with key times on the item's source clock,
    /// the clock of `source`; its gain holds the other stages.
    pub(super) volume_keys: Option<PrVolumeKeys>,
    /// The item's own clip Enable. A nest's group carries the item's sound
    /// only when its picture has the same Enable; otherwise the item plays
    /// alone ([`play_alone`]), silent when this is off.
    pub(super) enabled: bool,
}

/// A checked nested placement whose inner timeline is not yet copied.
pub(super) struct NestPlacement {
    id: String,
    timeline: Range<i64>,
    source: Range<i64>,
    playback_rate: f64,
    reverse_source_duration: Option<i64>,
    time_remap: Option<crate::schema::PrTimeRemap>,
    /// Effective picture output, from clip Enable and track output.
    enabled: bool,
    /// The placement's Motion and Opacity, with their keys, which its group
    /// carries.
    transform: PrStaticTransform,
    crop: PrStaticCrop,
    linear_wipe: Option<crate::schema::PrLinearWipe>,
    opacity_mask: Option<crate::schema::PrMask>,
    opacity: f64,
    animations: Vec<PrPropertyAnimation>,
    effects: Vec<crate::schema::PrEffect>,
    effects_above_mask: usize,
    track_matte: Option<PrTrackMatte>,
    blend_mode: PrBlendMode,
    read: ReadKey,
}

/// A sequence GUID, remaining nesting depth and optional multicam video track.
/// Camera reads must not reuse a different cut's camera or the composite view.
type ReadKey = (String, usize, Option<usize>);

/// A nested composite or a resolved ordinary multicam picture.
pub(super) enum NestedVideo {
    Nest(Box<NestPlacement>),
    Media(Box<crate::schema::PrVideoOccurrence>),
}

/// Recursion state for one top-level sequence read.
pub(super) struct Nesting<'a> {
    /// Timelines on a nesting cycle in the project topology.
    cyclic: &'a BTreeSet<String>,
    /// GUIDs of the sequences being read, outermost first.
    open: Vec<String>,
    /// Nesting levels open below the top-level sequence.
    depth: usize,
    /// Finished inner reads by GUID and by the levels still allowed below the
    /// read sequence, so a cached copy never nests deeper than the place that
    /// reuses it, whatever the track order. A failure keeps its reason. A read
    /// that met a cycle is kept too: its closing placement is already omitted
    /// and reported.
    finished: BTreeMap<ReadKey, std::result::Result<InnerRead, String>>,
}

impl<'a> Nesting<'a> {
    pub(super) fn new(root: Option<&str>, cyclic: &'a BTreeSet<String>) -> Self {
        Self {
            cyclic,
            open: root.map(str::to_owned).into_iter().collect(),
            depth: 0,
            finished: BTreeMap::new(),
        }
    }

    /// Whether the timeline being read is the inner timeline of a nest placement.
    pub(super) fn reads_inner_timeline(&self) -> bool {
        self.depth > 0
    }

    /// Reads `guid` once per remaining depth; returns the cache key and the
    /// cached read.
    fn read(
        &mut self,
        graph: &Graph<'_>,
        guid: &str,
        media: &mut BTreeMap<MediaId, PrMedia>,
        omissions: &mut Vec<Omission>,
        selected_video_track: Option<usize>,
    ) -> Result<(ReadKey, &mut InnerRead)> {
        ensure!(
            !self.cyclic.contains(guid),
            "nested sequence {guid} is on a nesting cycle; cyclic nesting is not converted"
        );
        // A cycle that the project topology could not see closes here.
        ensure!(
            !self.open.iter().any(|open| open == guid),
            "nested sequence {guid} contains itself; cyclic nesting is not converted"
        );
        let level = self.depth + 1;
        ensure!(
            level <= MAX_NEST_DEPTH,
            "nested sequence {guid} is deeper than {MAX_NEST_DEPTH} levels"
        );
        let key = (
            guid.to_owned(),
            MAX_NEST_DEPTH - level,
            selected_video_track,
        );
        if !self.finished.contains_key(&key) {
            self.open.push(guid.to_owned());
            self.depth = level;
            let read = sequence::read_sequence_tracks_at(
                graph,
                Some(guid),
                media,
                self,
                omissions,
                selected_video_track,
            );
            self.depth = level - 1;
            self.open.pop();
            let read = read.map_err(reason).map(Self::keep);
            self.finished.insert(key.clone(), read);
        }
        match self.finished.get_mut(&key).expect("each read is cached") {
            Ok(read) => Ok((key, read)),
            Err(reason) => Err(unsupported(format!("nested sequence {guid}: {reason}"))),
        }
    }

    /// Prepares a finished inner read for the cache.
    fn keep(sequence: PrSequence) -> InnerRead {
        let nest = PrNestOccurrence {
            id: None,
            reverse_source_duration: None,
            playback_rate: 1.0,
            time_remap: None,
            start_ticks: 0,
            end_ticks: 0,
            in_ticks: 0,
            out_ticks: 0,
            transform: Default::default(),
            opacity: 100.0,
            blend_mode: PrBlendMode::Normal,
            animations: Vec::new(),
            crop: Default::default(),
            linear_wipe: None,
            opacity_mask: None,
            track_matte: None,
            effects: Vec::new(),
            effects_above_mask: 0,
            enabled: true,
            sequence,
        };
        InnerRead { nest }
    }

    /// Copies the inner timeline of a kept placement.
    fn copy(&self, placement: &NestPlacement) -> PrSequence {
        self.finished
            .get(&placement.read)
            .and_then(|read| read.as_ref().ok())
            .expect("a placement is made only from a successful cached read")
            .nest
            .sequence
            .clone()
    }
}

fn reason(error: BuildError) -> String {
    match error {
        BuildError::Unsupported(reason) => reason,
        other => other.to_string(),
    }
}

/// Reads a track item whose clip plays the sequence `guid`, then rejects what
/// a nest cannot carry. The inner timeline is checked but not yet copied.
pub(super) fn read_nest(
    graph: &Graph<'_>,
    item: Located<VideoClipTrackItem>,
    guid: &str,
    parent: &Parent<'_>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    nesting: &mut Nesting<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<NestedVideo> {
    let placement = read_placement(graph, &item, parent, false, omissions)?;
    for (owner, chain) in placement
        .chain
        .iter()
        .map(|chain| ("placement", chain))
        .chain(
            placement
                .source_chain
                .iter()
                .map(|source| ("source", &source.chain)),
        )
    {
        super::effects::split_chain(
            graph,
            super::animation::chain_components(chain)?,
            &chain.identity,
        )?
        .reject_unconverted_coverage(owner)?;
    }

    if let Some(index) = placement
        .clip
        .value
        .clip
        .as_ref()
        .and_then(|clip| clip.selected_track_index)
    {
        return read_multicam(
            graph,
            &item.identity,
            guid,
            index,
            placement,
            parent,
            media,
            nesting,
            omissions,
        )
        .map(|camera| NestedVideo::Media(Box::new(camera)));
    }
    // A nest placement carries no source effects; its picture converts
    // without them, as without an admitted chain.
    if let Some(source) = &placement.source_chain {
        omit(
            omissions,
            OmissionScope::Feature,
            &source.master,
            SOURCE_CHAIN_NOT_CONVERTED,
        );
    }
    let identity = &item.identity;
    let mut occurrence_effect_omissions = Vec::new();
    let (occurrence_effects, effects_above_mask, transform_stage, geometry2) = if placement
        .has_effects
    {
        let chain = placement.chain.as_ref().ok_or_else(|| {
            unsupported(format!(
                "{identity}: nested effects have no component chain"
            ))
        })?;
        let split = effects::split_chain(graph, chain_components(chain)?, &chain.identity)?;
        if split.has_active_nest_transform() {
            // Keep the existing bounded Transform admission; ordinary effects
            // do not establish a new Transform coordinate basis or stack order.
            let effect = split
                .read_nest_transform(graph)
                .map_err(|error| unsupported(format!("{identity}: {}", effects::reason(error))))?;
            (vec![effect], 0, true, split.has_active_nest_geometry2())
        } else {
            ensure!(
                placement.crop.is_default() && placement.track_matte.is_none(),
                "{identity}: effects on a nested sequence occurrence with masks are not converted"
            );
            ensure!(
                !split.has_active_nest_corner_pin(),
                "{identity}: active Corner Pin on a nested sequence occurrence is not converted: the picture Group has no fixed native canvas bounds; occurrence omitted to preserve coverage"
            );
            let owner = effects::EffectOwner {
                occurrence: identity,
                source: None,
                stroke_geometry: false,
                clip_name: placement.sub.value.name.as_deref(),
                track_index: parent.track_index,
                timeline_ticks: placement.start..placement.end,
                // The canvas is read later. No point-frame mapping is admitted
                // by claiming it matches the parent's canvas here.
                source_is_canvas: false,
                adjustment: false,
            };
            let (effects, above_mask) = split.read_effects(
                graph,
                &owner,
                placement.linear_wipe.is_some(),
                &mut occurrence_effect_omissions,
            );
            (effects, above_mask, false, false)
        }
    } else {
        (Vec::new(), 0, false, false)
    };
    ensure!(
        !placement.scale_to_frame,
        "{identity}: Scale to Frame Size on a nested sequence occurrence is not converted"
    );
    let clip = &placement.clip.identity;
    let edits = occurrence_edits(ClipEdits {
        linear_wipe: placement.linear_wipe.as_ref(),
        animations: &placement.animations,
        transform: placement.transform,
        crop: placement.crop,
        opacity_mask: placement.opacity_mask.as_ref(),
        track_matte: placement.track_matte.as_ref(),
        opacity: placement.opacity,
        playback_rate: placement.playback_rate,
        time_remap: placement.time_remap.as_ref(),
    });
    // A Track Matte Key keys the nest's canvas-sized picture, a blend mode
    // blends it and its Opacity, static or keyed, fades it, which its group
    // carries; Opacity scales the keyed picture's alpha in either order. So
    // is its Motion, static or keyed, as a clip's is its layer's, though not
    // beside a Track Matte Key, whose order against it no nest case
    // measures. A standalone static Crop clips its own canvas before Motion;
    // feather, Motion Crop and Crop beside a Track Matte Key remain excluded.
    let reason = edits.iter().find_map(|&edit| match edit {
        OccurrenceEdit::TrackMatte | OccurrenceEdit::Opacity | OccurrenceEdit::OpacityKeys => None,
        OccurrenceEdit::MotionKeys
        | OccurrenceEdit::Position
        | OccurrenceEdit::AnchorPoint
        | OccurrenceEdit::Scale
        | OccurrenceEdit::Rotation => placement.track_matte.is_some().then(|| {
            format!("{identity}: Motion with a Track Matte Key on a nested sequence occurrence is not converted")
        }),
        OccurrenceEdit::LinearWipe => (!placement.crop.is_default()
            || placement.track_matte.is_some()
            || placement.opacity_mask.is_some()
            || placement.playback_rate != 1.0
            || placement.time_remap.is_some())
            .then(|| format!(
                "{identity}: nested Linear Wipe requires a unit-forward clock without other masks"
            )),
        OccurrenceEdit::OpacityMask => (!placement.crop.is_default()
            || placement.linear_wipe.is_some() || placement.track_matte.is_some()
            || placement.playback_rate != 1.0 || placement.time_remap.is_some()
            || placement.opacity_mask.as_ref().is_some_and(|mask|
                mask.raster.is_some() || !mask.path_keys.is_empty()))
            .then(|| format!("{identity}: nested Opacity mask requires a static vector outline, unit-forward playback and no other masks")),
        OccurrenceEdit::Crop => {
            let reason = if placement.crop_from_motion {
                Some("Motion Crop")
            } else if placement.crop.edge_feather != 0.0 {
                Some("feathered Crop")
            } else if placement.track_matte.is_some() {
                Some("Crop with a Track Matte Key")
            } else {
                None
            };
            reason.map(|reason| format!(
                "{identity}: {reason} on a nested sequence occurrence is not converted"
            ))
        },
        // Check the input span and reverse OriginalDuration reflection against
        // the actual inner timeline, not a forward-only admission predicate.
        OccurrenceEdit::PlaybackRate => None,
        OccurrenceEdit::TimeRemap => parent.nested.then(|| format!(
            "{clip}: TimeRemapping on a nested sequence occurrence inside another nest is not converted"
        )),
    });
    if let Some(reason) = reason {
        return Err(unsupported(reason));
    }
    let source_duration = read_sequence_source(graph, placement.source_record, omissions)?;
    let (key, read) = nesting.read(graph, guid, media, omissions, None)?;
    let id = item.identity;
    let nest = &mut read.nest;
    if transform_stage {
        let old_envelope = crate::schema::nested_transform_canvas_reason(
            nest.sequence.dimensions(),
            parent.dimensions,
            &placement.transform,
            placement.opacity,
            !placement.animations.is_empty(),
            &occurrence_effects[0],
        );
        if let Some(reason) = crate::schema::nested_transform_import_canvas_reason(
            nest.sequence.dimensions(),
            parent.dimensions,
            &placement.transform,
            placement.opacity,
            &placement.animations,
            &occurrence_effects[0],
        ) {
            return Err(unsupported(format!(
                "{id}: {reason}; source {:?}, placement {:?}",
                nest.sequence.dimensions(),
                parent.dimensions
            )));
        }
        ensure!(
            old_envelope.is_none() || geometry2,
            "{id}: differing-canvas keyed affine import requires native Geometry2; ordinary Transform point basis is unmeasured"
        );
        ensure!(
            placement.playback_rate == 1.0
                && placement.time_remap.is_none()
                && placement.source_out - placement.source_in == placement.end - placement.start
                && nest.sequence.frame_rate == parent.frame_rate,
            "{id}: nested Transform requires unit-forward matching clocks"
        );
        ensure!(
            placement.track_matte.is_none() && placement.opacity_mask.is_none(),
            "{id}: nested Transform with Track Matte Key or Opacity mask is not converted"
        );
    }
    (nest.start_ticks, nest.end_ticks) = (placement.start, placement.end);
    (nest.in_ticks, nest.out_ticks) = (placement.source_in, placement.source_out);
    nest.playback_rate = placement.playback_rate;
    nest.reverse_source_duration = (placement.playback_rate < 0.0).then_some(source_duration);
    nest.time_remap = placement.time_remap.clone();
    (nest.transform, nest.opacity, nest.animations) =
        (placement.transform, placement.opacity, placement.animations);
    nest.crop = placement.crop;
    nest.linear_wipe = placement.linear_wipe;
    nest.opacity_mask = placement.opacity_mask;
    ensure!(
        nest.opacity_mask.is_none()
            || (nest.sequence.dimensions() == parent.dimensions
                && nest.sequence.frame_rate == parent.frame_rate),
        "{id}: nested Opacity mask requires equal canvases and matching clocks"
    );
    // Before a later first key Premiere shows that key's value, as AME renders
    // effect parameters. So a keyed property's static value is
    // its first key's, as an effect parameter's is.
    for animation in &nest.animations {
        match animation {
            PrPropertyAnimation::Position(keys) => {
                if let Some(key) = keys.first() {
                    nest.transform.position = key.value;
                }
            }
            PrPropertyAnimation::AnchorPoint(keys) => {
                if let Some(key) = keys.first() {
                    nest.transform.anchor_point = key.value;
                }
            }
            PrPropertyAnimation::Rotation(keys) => {
                if let Some(key) = keys.first() {
                    nest.transform.rotation = key.value;
                }
            }
            PrPropertyAnimation::UniformScale(keys) => {
                if let Some(key) = keys.first() {
                    nest.transform.scale = [key.value; 2];
                }
            }
            // Scale Width keys scale the horizontal axis alone, so the static
            // Scale Height stays.
            PrPropertyAnimation::ScaleWidth(keys) => {
                if let Some(key) = keys.first() {
                    nest.transform.scale[0] = key.value;
                }
            }
            PrPropertyAnimation::Opacity(keys) => {
                if let Some(key) = keys.first() {
                    nest.opacity = key.value;
                }
            }
        }
    }
    nest.validate(parent.frame_rate, media)?;
    // Motion places a nest of another canvas, and Premiere keys the picture
    // before it moves it, so its matte would move with its group; that
    // order is unmeasured, as for a moved nest.
    ensure!(
        placement.track_matte.is_none() || nest.sequence.dimensions() == parent.dimensions,
        "{id}: a Track Matte Key on a nested sequence occurrence of another canvas is not converted"
    );
    // The saved input window must agree with the speed even with a source curve.
    ensure!(
        source_span_matches(
            nest.end_ticks - nest.start_ticks,
            nest.out_ticks - nest.in_ticks,
            placement.playback_rate
        ),
        "{clip}: In {} to Out {} does not match PlaybackSpeed {} over the {}-tick placement of a nested sequence occurrence",
        nest.in_ticks,
        nest.out_ticks,
        placement.playback_rate,
        nest.end_ticks - nest.start_ticks
    );
    // A retimed nest's keys are not converted: its group keeps their static
    // values. Dropping Opacity keys could show what they hide, so they omit a
    // retimed nest.
    if nest.is_retimed() && nest.playback_rate > 0.0 && edits.contains(&OccurrenceEdit::OpacityKeys)
    {
        return Err(unsupported(format!(
            "{id}: Opacity keyframes on a retimed nested sequence occurrence are not converted"
        )));
    }
    // Frame Blending or Optical Flow interpolates the retimed picture of the
    // whole nest; an FX group has no such field, and each inner video's own
    // blending would interpolate its media instead.
    if let Some(mode) = placement.frame_blending.filter(|_| nest.is_retimed()) {
        let mode = match mode {
            fx_schema::FrameBlendingMode::Simple => "Frame Blending",
            fx_schema::FrameBlendingMode::OpticalFlow => "Optical Flow",
        };
        if nest.time_remap.is_some() {
            crate::approximate(
                omissions,
                &id,
                format!("{mode} composite time interpolation is unsupported; sampling fallback evaluates editable children once at each mapped time, without interpolating composite frames or moving interpolation onto a leaf"),
            );
        } else {
            omit(
                omissions,
                OmissionScope::Feature,
                clip,
                format!(
                    "{mode} time interpolation of a retimed nested sequence occurrence is not converted; its group shows the nested frame at each mapped time"
                ),
            );
        }
    }
    // Only admitted occurrences may describe converted or omitted effects.
    omissions.extend(occurrence_effect_omissions);
    Ok(NestedVideo::Nest(Box::new(NestPlacement {
        id,
        timeline: placement.start..placement.end,
        source: placement.source_in..placement.source_out,
        playback_rate: placement.playback_rate,
        reverse_source_duration: nest.reverse_source_duration,
        time_remap: placement.time_remap,
        enabled: placement.enabled && parent.track_output,
        transform: nest.transform,
        crop: nest.crop,
        linear_wipe: nest.linear_wipe.clone(),
        opacity_mask: nest.opacity_mask.clone(),
        opacity: nest.opacity,
        animations: std::mem::take(&mut nest.animations),
        effects: occurrence_effects,
        effects_above_mask,
        track_matte: placement.track_matte,
        blend_mode: placement.blend_mode,
        read: key,
    })))
}

/// Resolves a plain saved camera cut to the underlying editable media occurrence.
/// Sound remains owned by the outer audio items, never by the chosen picture.
#[expect(
    clippy::too_many_arguments,
    reason = "shares the native reader's graph, placement and recursion context"
)]
fn read_multicam(
    graph: &Graph<'_>,
    identity: &str,
    guid: &str,
    index: usize,
    placement: Placement<'_>,
    parent: &Parent<'_>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    nesting: &mut Nesting<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<crate::schema::PrVideoOccurrence> {
    let edits = occurrence_edits(ClipEdits {
        linear_wipe: placement.linear_wipe.as_ref(),
        animations: &placement.animations,
        transform: placement.transform,
        crop: placement.crop,
        opacity_mask: placement.opacity_mask.as_ref(),
        track_matte: placement.track_matte.as_ref(),
        opacity: placement.opacity,
        playback_rate: placement.playback_rate,
        time_remap: placement.time_remap.as_ref(),
    });
    ensure!(
        edits.is_empty(),
        "{identity}: multicam placement edits are not converted: {edits:?}"
    );
    ensure!(
        !placement.has_effects
            && placement.source_chain.is_none()
            && !placement.scale_to_frame
            && placement.blend_mode == PrBlendMode::Normal
            && placement.end > placement.start
            && placement.source_out.checked_sub(placement.source_in)
                == placement.end.checked_sub(placement.start),
        "{identity}: only plain unit-speed multicam cuts are converted"
    );
    read_sequence_source(graph, placement.source_record, omissions)?;
    let (_, read) = nesting.read(graph, guid, media, omissions, Some(index))?;
    let sequence = &read.nest.sequence;
    ensure!(
        sequence.frame_rate == parent.frame_rate
            && [sequence.width, sequence.height] == parent.dimensions,
        "{identity}: multicam camera canvas and rate must match the outer sequence"
    );
    let track = &sequence.video_tracks[0];
    ensure!(
        track.nests.is_empty() && track.transitions.is_empty() && track.items.len() == 1,
        "{identity}: the selected multicam track must contain one ordinary camera clip"
    );
    let camera = track.items[0].media().ok_or_else(|| {
        unsupported(format!(
            "{identity}: the selected multicam track is not ordinary media"
        ))
    })?;
    let source = &media[&camera.media];
    ensure!(
        camera.enabled && camera.active_transforms == 0
            && camera.edits().is_empty()
            && camera.effects.is_empty()
            && camera.source_effects.is_none()
            && camera.stroke.is_none()
            && camera.blend_mode == PrBlendMode::Normal
            && source.video.as_ref().is_some_and(|stream| {
                matches!(stream.kind, crate::schema::PrMediaKind::Video { .. })
                    && [stream.width, stream.height] == parent.dimensions
            }),
        "{identity}: the selected multicam camera must be enabled, full-canvas and have no retained edits"
    );
    ensure!(
        camera.start_ticks <= placement.source_in && placement.source_out <= camera.end_ticks,
        "{identity}: the selected multicam camera does not cover the cut's source window"
    );
    let mut camera = camera.clone();
    camera.in_ticks = camera
        .in_ticks
        .checked_add(placement.source_in - camera.start_ticks)
        .ok_or_else(|| unsupported(format!("{identity}: multicam source In exceeds tick range")))?;
    camera.out_ticks = camera
        .in_ticks
        .checked_add(placement.end - placement.start)
        .ok_or_else(|| {
            unsupported(format!(
                "{identity}: multicam source Out exceeds tick range"
            ))
        })?;
    camera.start_ticks = placement.start;
    camera.end_ticks = placement.end;
    camera.id = Some(identity.to_owned());
    camera.enabled &= placement.enabled && parent.track_output;
    camera.validate(parent.frame_rate, source)?;
    Ok(camera)
}

/// Keeps the nested placements that overlap neither a kept media or graphic
/// item nor an earlier nested placement on the same track.
pub(super) fn keep_non_overlapping(
    mut nests: Vec<NestPlacement>,
    items: &[PrVideoItem],
    omissions: &mut Vec<Omission>,
) -> Vec<NestPlacement> {
    // Ordinary media-only tracks need no overlap index.
    if nests.is_empty() {
        return nests;
    }
    let mut item_ranges: Vec<_> = items.iter().map(PrVideoItem::timeline_ticks).collect();
    item_ranges.sort_by_key(|range| range.start);
    let mut item_index = IntervalIndex::default();
    for range in item_ranges {
        item_index.push(&range);
    }
    nests.sort_by_key(|nest| nest.timeline.start);
    let mut kept: Vec<NestPlacement> = Vec::with_capacity(nests.len());
    let mut kept_index = IntervalIndex::default();
    for nest in nests {
        if item_index.overlaps(&nest.timeline) || kept_index.overlaps(&nest.timeline) {
            omit(
                omissions,
                OmissionScope::Occurrence,
                nest.id,
                "overlaps another occurrence on this track",
            );
        } else {
            kept_index.push(&nest.timeline);
            kept.push(nest);
        }
    }
    kept
}

/// Sorted starts and prefix maximum ends implement the exact strict overlap
/// predicate, even for empty or reversed ranges. No endpoint arithmetic is needed.
#[derive(Default)]
struct IntervalIndex {
    starts: Vec<i64>,
    max_ends: Vec<i64>,
}

impl IntervalIndex {
    // Callers append in nondecreasing start order.
    fn push(&mut self, range: &Range<i64>) {
        debug_assert!(self.starts.last().is_none_or(|start| *start <= range.start));
        self.starts.push(range.start);
        self.max_ends.push(
            self.max_ends
                .last()
                .map_or(range.end, |end| (*end).max(range.end)),
        );
    }

    fn overlaps(&self, range: &Range<i64>) -> bool {
        let count = self.starts.partition_point(|start| *start < range.end);
        count > 0 && self.max_ends[count - 1] > range.start
    }
}

/// Copies the nested placements of one sequence into `tracks`.
pub(super) fn copy_placements(
    tracks: &mut [PrVideoTrack],
    placements: Vec<Vec<NestPlacement>>,
    nesting: &Nesting<'_>,
) {
    for (track, placements) in tracks.iter_mut().zip(placements) {
        for placement in placements {
            let sequence = nesting.copy(&placement);
            track.nests.push(PrNestOccurrence {
                id: Some(placement.id),
                playback_rate: placement.playback_rate,
                reverse_source_duration: placement.reverse_source_duration,
                time_remap: placement.time_remap,
                start_ticks: placement.timeline.start,
                end_ticks: placement.timeline.end,
                in_ticks: placement.source.start,
                out_ticks: placement.source.end,
                transform: placement.transform,
                opacity: placement.opacity,
                blend_mode: placement.blend_mode,
                animations: placement.animations,
                crop: placement.crop,
                linear_wipe: placement.linear_wipe,
                opacity_mask: placement.opacity_mask,
                track_matte: placement.track_matte,
                effects: placement.effects,
                effects_above_mask: placement.effects_above_mask,
                enabled: placement.enabled,
                sequence,
            });
        }
    }
}

/// Gives each nest on `tracks` the sound that Premiere plays for it: the
/// inner sound of its one audio item with the same sequence, timeline range,
/// source range and clip Enable, whose gain multiplies every sound of the
/// copy and of the nests inside it. The mix of the inner sequence is heard
/// only through that item (`premiere_isolated_images_nests_26_5`, G6/G7), so
/// a nest without one is silent. Returns the other audio items, which play
/// alone ([`play_alone`]): each item of no kept nest, an item whose clip
/// Enable differs from its video item's, an item with Level keys, whose keys
/// reach each inner sound on that sound's own clock only as it plays alone
/// ([`apply_level_keys`]), and each of several items for one nest. Such a
/// nest keeps only its picture. Each item is a clip of its own track, so
/// several items play the sound together (inferred: no Premiere save has
/// two). An item plays at normal speed (`audio::read_tracks`), so a retimed
/// nest, whose window has another length than its placement, has none and
/// keeps only its picture too. A nest left with nothing to show or play is
/// omitted, reported unless its items play alone.
pub(super) fn pair_sounds(
    tracks: &mut [PrVideoTrack],
    sounds: Vec<NestSound>,
    omissions: &mut Vec<Omission>,
) -> Result<Vec<NestSound>> {
    let mut buckets: BTreeMap<_, Vec<usize>> = BTreeMap::new();
    for (index, sound) in sounds.iter().enumerate() {
        buckets
            .entry((
                sound.sequence.clone(),
                sound.timeline.start,
                sound.timeline.end,
                sound.source.start,
                sound.source.end,
            ))
            .or_default()
            .push(index);
    }
    let mut unpaired: Vec<Option<NestSound>> = sounds.into_iter().map(Some).collect();
    let mut alone = Vec::new();
    for track in tracks.iter_mut() {
        let mut kept = Vec::with_capacity(track.nests.len());
        for mut nest in std::mem::take(&mut track.nests) {
            // Removing the whole bucket gives the first nest every duplicate,
            // in sound order; later nests cannot claim any of those items.
            // A reversed picture does not establish a reverse sound clock.
            // Leave its independently saved forward sound items to play_alone.
            let indices = nest
                .sequence
                .id
                .as_ref()
                .filter(|_| nest.playback_rate > 0.0)
                .and_then(|sequence| {
                    buckets.remove(&(
                        sequence.clone(),
                        nest.start_ticks,
                        nest.end_ticks,
                        nest.in_ticks,
                        nest.out_ticks,
                    ))
                });
            let mut matching: Vec<NestSound> = indices
                .into_iter()
                .flatten()
                .filter_map(|index| unpaired[index].take())
                .collect();
            let plays_alone = match matching.as_slice() {
                [] => false,
                [sound] if sound.enabled == nest.enabled && sound.volume_keys.is_none() => {
                    fold_gain(&mut nest.sequence, sound.gain)?;
                    kept.push(nest);
                    continue;
                }
                _ => {
                    alone.append(&mut matching);
                    true
                }
            };
            silence(&mut nest.sequence);
            if nest.sequence.expanded_occurrence_count() > 0 {
                kept.push(nest);
            } else if !plays_alone {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    nest.record(),
                    "nested sequence has only sound that no audio item of it plays",
                );
            }
        }
        track.nests = kept;
    }
    alone.extend(unpaired.into_iter().flatten());
    Ok(alone)
}

/// Plays each audio item of `sounds`, which no nest's group carries, as sound
/// of `sequence`, the sequence that holds it: every sound of the item's
/// sequence and of the nests inside it that the item's source range shows,
/// at its timeline range and gain ([`played_sounds`]) and under its Level
/// keys ([`apply_level_keys`]). An item whose Enable is off plays them at zero
/// gain, as a disabled clip does (`audio::read_occurrence`). The sounds join
/// `sequence.audio` in start order. An item that plays nothing adds nothing.
/// An item whose sequence cannot be read is omitted with the reason; no
/// other sound depends on it. A sound that cannot take the item's gain or
/// keys, or that does not validate, is omitted with the reason, reported on
/// the item, and the item's other sounds still play.
pub(super) fn play_alone(
    graph: &Graph<'_>,
    sounds: Vec<NestSound>,
    sequence: &mut PrSequence,
    media: &mut BTreeMap<MediaId, PrMedia>,
    nesting: &mut Nesting<'_>,
    omissions: &mut Vec<Omission>,
) {
    for sound in sounds {
        match heard_alone(graph, &sound, media, nesting, omissions) {
            Ok(played) => sequence.audio.extend(played),
            Err(reason) => omit(
                omissions,
                OmissionScope::Occurrence,
                sound.id,
                format!("nested sequence audio item not converted: {reason}"),
            ),
        }
    }
    sequence.audio.sort_by_key(|clip| clip.start_ticks);
}

/// The sounds that `sound` plays alone ([`played_sounds`], at its gain or
/// under its Level keys by [`apply_level_keys`]), each checked against its
/// stream as its sequence will check it, or why it plays none. A sound that
/// cannot take the gain or the keys, or that does not validate, is reported
/// and left out, alone.
fn heard_alone(
    graph: &Graph<'_>,
    sound: &NestSound,
    media: &mut BTreeMap<MediaId, PrMedia>,
    nesting: &mut Nesting<'_>,
    omissions: &mut Vec<Omission>,
) -> std::result::Result<Vec<PrAudioOccurrence>, String> {
    let (_, read) = nesting
        .read(graph, &sound.sequence, media, omissions, None)
        .map_err(reason)?;
    let shift = sound
        .timeline
        .start
        .checked_sub(sound.source.start)
        .ok_or_else(|| TICK_RANGE.to_owned())?;
    // At unit gain: the item's gain or keys reach each sound below.
    let mut played = Vec::new();
    played_sounds(
        &read.nest.sequence,
        &sound.source,
        shift,
        1.0,
        &mut played,
        omissions,
    )
    .map_err(reason)?;
    let gain = if sound.enabled { sound.gain } else { 0.0 };
    let mut heard = Vec::with_capacity(played.len());
    for mut clip in played {
        // Why this sound is not heard, if it is not; each of the item's
        // sounds is heard or not on its own.
        let failure = match &sound.volume_keys {
            None => scale(&mut clip, gain).err().map(reason),
            Some(keys) => match apply_level_keys(&mut clip, sound, keys, shift) {
                Ok(true) => None,
                Ok(false) => Some(LINEAR_PRODUCT.to_owned()),
                Err(error) => Some(reason(error)),
            },
        }
        .or_else(|| {
            let stream = media
                .get(&clip.media)
                .and_then(|source| source.audio.as_ref());
            match stream {
                Some(stream) => clip.validate(stream).err().map(|error| error.to_string()),
                None => Some(format!("{}: unknown audio media", clip.media)),
            }
        });
        match failure {
            None => {
                for fade in unheld_fades(&mut clip) {
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        fade.id.as_deref().unwrap_or(clip.record()),
                        "audio fade not converted: the Level keys of the nested sequence's audio item change during it",
                    );
                }
                heard.push(clip);
            }
            Some(failure) => omit(
                omissions,
                OmissionScope::Occurrence,
                &sound.id,
                format!("nested sound {} not converted: {failure}", clip.record()),
            ),
        }
    }
    Ok(heard)
}

/// Drops and returns the fades of `clip` over which its Level, which the
/// keys of an audio item may have replaced ([`apply_level_keys`]), no longer
/// holds one value as a fade requires (`audio::level_holds_over_fade`).
fn unheld_fades(clip: &mut PrAudioOccurrence) -> Vec<PrAudioFade> {
    let Some(keys) = &clip.volume_keys else {
        return Vec::new();
    };
    let spans = [
        clip.fade_in.as_ref().map(|fade| {
            clip.source_part(&(clip.start_ticks..clip.start_ticks + fade.duration_ticks))
                .expect("validated fade lies inside its audio source span")
        }),
        clip.fade_out.as_ref().map(|fade| {
            clip.source_part(&(clip.end_ticks - fade.duration_ticks..clip.end_ticks))
                .expect("validated fade lies inside its audio source span")
        }),
    ];
    let mut unheld = Vec::new();
    for (fade, fade_in, span) in [
        (&mut clip.fade_in, true, &spans[0]),
        (&mut clip.fade_out, false, &spans[1]),
    ] {
        let holds = span.as_ref().is_none_or(|span| {
            super::audio::level_holds_over_fade(&keys.keys, span.clone(), fade_in)
        });
        if !holds {
            unheld.extend(fade.take());
        }
    }
    unheld
}

const TICK_RANGE: &str = "nested sequence audio item exceeds Premiere's tick range";
const GAIN_OVERFLOW: &str = "nested sequence audio gain overflows";
/// Why [`apply_level_keys`] leaves a keyed sound unchanged.
const LINEAR_PRODUCT: &str = "its Volume and the Level keys of this audio item both change over it, and one key track cannot hold their product because one of them changes its Level along a Linear segment";

/// Appends to `played` what an audio item that plays `sequence` over the
/// inner range `window` hears: each sound of `sequence` and of the nests
/// inside it that `window` shows, moved by `shift` ticks and multiplied by
/// `gain`. A nest's group carries only the sound of an item with its
/// picture's Enable ([`pair_sounds`]), so a hidden group's sound is a
/// disabled item's and plays at zero gain. A fade that `window` shows only
/// part of is reported and dropped ([`PrAudioOccurrence::play_part`]).
fn played_sounds(
    sequence: &PrSequence,
    window: &Range<i64>,
    shift: i64,
    gain: f64,
    played: &mut Vec<PrAudioOccurrence>,
    omissions: &mut Vec<Omission>,
) -> Result<()> {
    for clip in &sequence.audio {
        let range = clip.start_ticks..clip.end_ticks;
        if let Some((timeline, source)) = shown(range.clone(), clip.in_ticks, window, shift)? {
            let source = if clip.playback_rate == 1.0 {
                source
            } else {
                clip.source_part(&(range.start.max(window.start)..range.end.min(window.end)))?
            };
            let mut clip = clip.clone();
            for fade in clip.play_part(timeline, source)? {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    fade.id.as_deref().unwrap_or(clip.record()),
                    PrAudioFade::PARTLY_PLAYED,
                );
            }
            scale(&mut clip, gain)?;
            played.push(clip);
        }
    }
    for nest in sequence.nest_occurrences() {
        if let Some((timeline, source)) =
            shown(nest.timeline_ticks(), nest.in_ticks, window, shift)?
        {
            let shift = timeline
                .start
                .checked_sub(source.start)
                .ok_or_else(|| unsupported(TICK_RANGE))?;
            let gain = if nest.enabled { gain } else { 0.0 };
            played_sounds(&nest.sequence, &source, shift, gain, played, omissions)?;
        }
    }
    Ok(())
}

/// The part of a placement over `range` from `source_in` that the inner
/// range `window` shows: that part moved by `shift` ticks and its source
/// range, or `None` when `window` shows none of it.
pub(super) fn shown(
    range: Range<i64>,
    source_in: i64,
    window: &Range<i64>,
    shift: i64,
) -> Result<Option<(Range<i64>, Range<i64>)>> {
    let (start, end) = (range.start.max(window.start), range.end.min(window.end));
    if start >= end {
        return Ok(None);
    }
    let ticks = |value: i128| i64::try_from(value).map_err(|_| unsupported(TICK_RANGE));
    let source_start = ticks(i128::from(source_in) + i128::from(start) - i128::from(range.start))?;
    Ok(Some((
        ticks(i128::from(start) + i128::from(shift))?..ticks(i128::from(end) + i128::from(shift))?,
        source_start..ticks(i128::from(source_start) + i128::from(end) - i128::from(start))?,
    )))
}

/// Gives one sound `clip`, which the audio item `sound` with the Level keys
/// `keys` plays at unit gain ([`played_sounds`], moved by `shift` ticks),
/// the item's Volume. At outer time `t` the clip plays its source time
/// `in + t - start` and the item its own `t - shift`, so each key moves by
/// `shift + in - start` to the clip's source clock.
///
/// A static clip takes those keys, which its own gain and the item's other
/// stages multiply. The keys keep the item's Level values, so their Linear
/// segments follow Premiere's fader curve for that Level. One key track holds
/// the product of the item's Level and a keyed clip's exactly when one of the
/// two holds over the clip's source range ([`level_over`]): the clip keeps
/// its own keys when the item's Level holds, and takes the item's when its
/// own Level holds, each held Level folded into the gain. It also holds it
/// when both only step ([`step_product`]). A clip that is silent throughout
/// keeps its own keys at zero gain. Otherwise both Levels change over the
/// clip and one of them changes along a Linear segment: returns `false` and
/// leaves the clip unchanged.
fn apply_level_keys(
    clip: &mut PrAudioOccurrence,
    sound: &NestSound,
    keys: &PrVolumeKeys,
    shift: i64,
) -> Result<bool> {
    // The item's gain with and without its Level's static value; a disabled
    // item plays its sounds at zero gain.
    let (item_gain, stage_gain) = if sound.enabled {
        (sound.gain, keys.gain)
    } else {
        (0.0, 0.0)
    };
    let keys = keys
        .keys
        .iter()
        .map(|key| {
            let source_ticks = if clip.playback_rate == 1.0 {
                // Preserve the established unit clock and its overflow boundary.
                let offset =
                    i128::from(shift) + i128::from(clip.in_ticks) - i128::from(clip.start_ticks);
                i64::try_from(i128::from(key.source_ticks) + offset)
                    .map_err(|_| unsupported(TICK_RANGE))?
            } else {
                let timeline_ticks = key
                    .source_ticks
                    .checked_add(shift)
                    .ok_or_else(|| unsupported(TICK_RANGE))?;
                clip.source_at(timeline_ticks)
                    .map_err(|_| unsupported(TICK_RANGE))?
            };
            Ok(PrScalarKeyframe {
                source_ticks,
                ..key.clone()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let range = clip.in_ticks..clip.out_ticks;
    // The keys that the clip takes, its gain before them, and the gain of
    // its stages that multiplies them.
    let (keys, sound_gain, sound_stages) = match &clip.volume_keys {
        None => (keys, clip.volume.as_f64(), clip.volume.as_f64()),
        Some(own) => match (level_over(&keys, &range), level_over(&own.keys, &range)) {
            (Some(level), _) => {
                scale(clip, stage_gain * level)?;
                return Ok(true);
            }
            (None, Some(level)) => (keys, own.gain * level, own.gain * level),
            (None, None) => match step_product(&keys, &own.keys)? {
                Some(product) => (product, clip.volume.as_f64(), own.gain),
                None if own.gain * stage_gain == 0.0 => {
                    scale(clip, stage_gain)?;
                    return Ok(true);
                }
                None => return Ok(false),
            },
        },
    };
    clip.volume = fx_schema::LinearGain::new(sound_gain * item_gain)
        .map_err(|_| unsupported(GAIN_OVERFLOW))?;
    clip.volume_keys = Some(PrVolumeKeys {
        keys,
        gain: sound_stages * stage_gain,
    });
    Ok(true)
}

/// The product of the Linear or Hold Level keys `keys` and `other`, both on
/// one source clock, when each only steps: every segment holds its Level, as
/// a Hold or a Linear segment between equal Levels does, and a first key's
/// easing shapes no segment. The product then holds between the times at
/// which either has a key, so Hold keys at each of those times, valued at
/// the product of both Levels there, are that product exactly, before the
/// first key and after the last too. The first key is Linear, as the reader
/// gives a first key. `None` when either changes its Level along a Linear
/// segment, even outside the sound: that product follows no fader curve of
/// one track.
fn step_product(
    keys: &[PrScalarKeyframe],
    other: &[PrScalarKeyframe],
) -> Result<Option<Vec<PrScalarKeyframe>>> {
    let steps = |track: &[PrScalarKeyframe]| {
        !track.is_empty()
            && track.windows(2).all(|pair| {
                pair[1].easing == PrKeyframeEasing::Hold || pair[1].value == pair[0].value
            })
    };
    if !(steps(keys) && steps(other)) {
        return Ok(None);
    }
    // The Level at `ticks`: the last key's at or before it, or the first's.
    let level = |track: &[PrScalarKeyframe], ticks: i64| {
        track[track
            .partition_point(|key| key.source_ticks <= ticks)
            .saturating_sub(1)]
        .value
    };
    let mut times: Vec<i64> = keys
        .iter()
        .chain(other)
        .map(|key| key.source_ticks)
        .collect();
    times.sort_unstable();
    times.dedup();
    times
        .into_iter()
        .enumerate()
        .map(|(index, source_ticks)| {
            let value = level(keys, source_ticks) * level(other, source_ticks);
            if !value.is_finite() {
                return Err(unsupported(GAIN_OVERFLOW));
            }
            Ok(PrScalarKeyframe {
                source_ticks,
                value,
                easing: if index == 0 {
                    PrKeyframeEasing::Linear
                } else {
                    PrKeyframeEasing::Hold
                },
            })
        })
        .collect::<Result<_>>()
        .map(Some)
}

/// The one Level that the Linear or Hold `keys` play over the source range
/// `range`, or `None` when it changes there. Before the first key and after
/// the last, the Level holds that key's value.
fn level_over(keys: &[PrScalarKeyframe], range: &Range<i64>) -> Option<f64> {
    // The range hears the keys from the last at or before its start to the
    // first at or after its end; a Hold into that one changes the Level only
    // once the range is over.
    let first = keys
        .iter()
        .rposition(|key| key.source_ticks <= range.start)
        .unwrap_or(0);
    let heard = match keys.iter().position(|key| key.source_ticks >= range.end) {
        Some(end) if end > first && keys[end].easing == PrKeyframeEasing::Hold => &keys[first..end],
        Some(end) => keys.get(first..=end)?,
        None => keys.get(first..)?,
    };
    let level = heard.first()?.value;
    heard.iter().all(|key| key.value == level).then_some(level)
}

/// Multiplies one sound's gain by `gain`, its Volume keys included.
pub(super) fn scale(clip: &mut PrAudioOccurrence, gain: f64) -> Result<()> {
    clip.volume = fx_schema::LinearGain::new(clip.volume.as_f64() * gain)
        .map_err(|_| unsupported(GAIN_OVERFLOW))?;
    if let Some(keys) = &mut clip.volume_keys {
        keys.gain *= gain;
    }
    Ok(())
}

/// Multiplies every sound of `sequence` and of the nests inside it by `gain`.
fn fold_gain(sequence: &mut PrSequence, gain: f64) -> Result<()> {
    for clip in &mut sequence.audio {
        scale(clip, gain)?;
    }
    for track in &mut sequence.video_tracks {
        for nest in &mut track.nests {
            fold_gain(&mut nest.sequence, gain)?;
        }
    }
    Ok(())
}

/// Drops every sound of `sequence` and of the nests inside it, and every
/// inner nest left empty; nothing of it was audible.
fn silence(sequence: &mut PrSequence) {
    sequence.audio.clear();
    for track in &mut sequence.video_tracks {
        for nest in &mut track.nests {
            silence(&mut nest.sequence);
        }
        track
            .nests
            .retain(|nest| nest.sequence.expanded_occurrence_count() > 0);
    }
}

fn read_sequence_source(
    graph: &Graph<'_>,
    source: Record<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<i64> {
    let identity = source.identity();
    ensure!(
        source.tag() == records::VIDEO_SEQUENCE_SOURCE.tag,
        "{identity}: a video placement of a sequence must use a {}",
        records::VIDEO_SEQUENCE_SOURCE.tag
    );
    // No typed shape is retained; decoding still checks references and xsi:nil.
    graph.decode::<IgnoredAny>(source)?;
    let element = source.element();
    report_unknown_children(
        element,
        &["SequenceSource", "OriginalDuration"],
        &identity,
        "",
        omissions,
    );
    if let Some(body) = element.child("SequenceSource") {
        report_unknown_children(
            body,
            &["Content", "Sequence"],
            &identity,
            "SequenceSource/",
            omissions,
        );
    }
    required_integer(
        element.child("OriginalDuration").and_then(Element::text),
        &identity,
        "OriginalDuration",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        format::{
            graph,
            tests::{
                caption::{captions_xml, cue},
                graphic::{graphic_item_record, graphic_source_records},
                nested::{
                    placement_records, sequence_records, sequence_source, with_records, Placement,
                    ONE_CLIP_XML,
                },
            },
            text_payload,
        },
        schema::{PrAudioOccurrence, PrKeyframeEasing, PrScalarKeyframe, PrVolumeKeys, TICKS},
        tests::support::{clip_of, nest_of, sequence_of},
    };
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::io::Read;

    // Frozen pre-index implementation: an independent ordering/error oracle.
    fn old_pair_sounds(
        tracks: &mut [PrVideoTrack],
        sounds: Vec<NestSound>,
        omissions: &mut Vec<Omission>,
    ) -> Result<Vec<NestSound>> {
        let mut unpaired: Vec<Option<NestSound>> = sounds.into_iter().map(Some).collect();
        let mut alone = Vec::new();
        for track in tracks.iter_mut() {
            let mut kept = Vec::with_capacity(track.nests.len());
            for mut nest in std::mem::take(&mut track.nests) {
                let mut matching: Vec<NestSound> = unpaired
                    .iter_mut()
                    .filter(|sound| {
                        sound.as_ref().is_some_and(|sound| {
                            nest.sequence.id.as_deref() == Some(sound.sequence.as_str())
                                && sound.timeline == nest.timeline_ticks()
                                && sound.source == (nest.in_ticks..nest.out_ticks)
                        })
                    })
                    .filter_map(Option::take)
                    .collect();
                let plays_alone = match matching.as_slice() {
                    [] => false,
                    [sound] if sound.enabled == nest.enabled && sound.volume_keys.is_none() => {
                        fold_gain(&mut nest.sequence, sound.gain)?;
                        kept.push(nest);
                        continue;
                    }
                    _ => {
                        alone.append(&mut matching);
                        true
                    }
                };
                silence(&mut nest.sequence);
                if nest.sequence.expanded_occurrence_count() > 0 {
                    kept.push(nest);
                } else if !plays_alone {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        nest.record(),
                        "nested sequence has only sound that no audio item of it plays",
                    );
                }
            }
            track.nests = kept;
        }
        alone.extend(unpaired.into_iter().flatten());
        Ok(alone)
    }

    fn indexed_placement(index: usize, timeline: Range<i64>) -> NestPlacement {
        NestPlacement {
            id: index.to_string(),
            timeline,
            source: 0..1,
            reverse_source_duration: None,
            playback_rate: 1.0,
            time_remap: None,
            enabled: true,
            transform: Default::default(),
            crop: Default::default(),
            linear_wipe: None,
            opacity_mask: None,
            opacity: 100.0,
            animations: Vec::new(),
            effects: Vec::new(),
            effects_above_mask: 0,
            track_matte: None,
            blend_mode: PrBlendMode::Normal,
            read: ("inner".into(), 1, None),
        }
    }

    fn compare_overlap(ranges: &[Range<i64>], item_ranges: &[Range<i64>]) {
        let placements = || {
            ranges
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, range)| indexed_placement(index, range))
                .collect::<Vec<_>>()
        };
        let items: Vec<_> = item_ranges
            .iter()
            .cloned()
            .map(|range| PrVideoItem::Media(clip_of("media", range, 0)))
            .collect();
        let mut ordered = placements();
        ordered.sort_by_key(|nest| nest.timeline.start);
        let mut expected: Vec<NestPlacement> = Vec::new();
        let mut expected_omissions = Vec::new();
        let start = std::time::Instant::now();
        for nest in ordered {
            let overlaps = |range: &Range<i64>| {
                range.start < nest.timeline.end && nest.timeline.start < range.end
            };
            if items.iter().any(|item| overlaps(&item.timeline_ticks()))
                || expected.iter().any(|other| overlaps(&other.timeline))
            {
                omit(
                    &mut expected_omissions,
                    OmissionScope::Occurrence,
                    nest.id,
                    "overlaps another occurrence on this track",
                );
            } else {
                expected.push(nest);
            }
        }
        let old_elapsed = start.elapsed();
        let input = placements();
        let mut omissions = Vec::new();
        let start = std::time::Instant::now();
        let actual = keep_non_overlapping(input, &items, &mut omissions);
        let new_elapsed = start.elapsed();
        if ranges.len() >= 2048 {
            eprintln!(
                "overlap 2048 disjoint nests/items: scan={old_elapsed:?}, index={new_elapsed:?}"
            );
        }
        assert_eq!(
            actual.iter().map(|nest| &nest.id).collect::<Vec<_>>(),
            expected.iter().map(|nest| &nest.id).collect::<Vec<_>>()
        );
        assert_eq!(format!("{omissions:?}"), format!("{expected_omissions:?}"));
    }

    #[test]
    fn indexed_overlap_matches_strict_scan_for_permutations_and_stress() {
        let single = 0..1;
        compare_overlap(&[], std::slice::from_ref(&single));
        compare_overlap(std::slice::from_ref(&single), &[]);
        // These are interval data, not iterators: preserve reversed endpoints too.
        let reversed = Range { start: 3, end: 1 };
        let mut ranges = vec![0..0, 0..2, 0..1, 2..4, 1..3, reversed.clone(), -2..0, 4..4];
        for rotation in 0..ranges.len() {
            ranges.rotate_left(rotation);
            compare_overlap(&ranges, &[]);
            compare_overlap(&ranges, &[0..0, reversed.clone(), 2..4, -3..-1]);
            ranges.reverse();
        }
        // Exhaust all endpoint pairs, including signed extremes without arithmetic.
        let endpoints = [i64::MIN, -1, 0, 1, i64::MAX];
        for start in endpoints {
            for end in endpoints {
                let mut index = IntervalIndex::default();
                index.push(&(start..end));
                for query_start in endpoints {
                    for query_end in endpoints {
                        assert_eq!(
                            index.overlaps(&(query_start..query_end)),
                            start < query_end && query_start < end
                        );
                    }
                }
            }
        }
        let disjoint: Vec<_> = (0..2048).map(|i| 4 * i..4 * i + 1).collect();
        let items: Vec<_> = (0..2048).map(|i| 4 * i + 2..4 * i + 3).collect();
        compare_overlap(&disjoint, &items);
    }

    fn pairing_fixture(order: &[usize], gain: f64) -> (Vec<PrVideoTrack>, Vec<NestSound>) {
        let mut inner = sequence_of(
            "Inner",
            vec![PrVideoTrack::media([clip_of("media", 0..4, 0)])],
        );
        inner.id = Some("inner".into());
        inner.audio = vec![PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("media".into()),
            start_ticks: 0,
            end_ticks: 4,
            in_ticks: 0,
            out_ticks: 4,
            volume: fx_schema::LinearGain::new(0.5).unwrap(),
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        }];
        let mut nests: Vec<_> = (0..12)
            .map(|index| {
                let mut sequence = inner.clone();
                if index % 4 == 3 {
                    sequence.video_tracks.clear();
                }
                if index == 11 {
                    sequence.id = None;
                }
                let mut nest = nest_of(sequence, 4 * (index / 2)..4 * (index / 2) + 4, 0);
                nest.id = Some(index.to_string());
                nest.enabled = index % 3 != 0;
                nest
            })
            .collect();
        // Split a duplicate identity across tracks to exercise first-track claim.
        let later = nests.split_off(5);
        let tracks = vec![
            PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests,
            },
            PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests: later,
            },
        ];
        let sounds = order
            .iter()
            .map(|index| NestSound {
                id: index.to_string(),
                sequence: if *index == 9 { "unmatched" } else { "inner" }.into(),
                timeline: 4 * (*index as i64 / 2)..4 * (*index as i64 / 2) + 4,
                source: if *index == 8 { 1..5 } else { 0..4 },
                gain,
                enabled: index % 3 != 0,
                volume_keys: (index % 4 == 2).then(|| PrVolumeKeys {
                    keys: vec![key(0.0, 1.0, PrKeyframeEasing::Linear)],
                    gain: 1.0,
                }),
            })
            .collect();
        (tracks, sounds)
    }

    #[test]
    fn reverse_nest_never_claims_or_retimes_an_independent_forward_sound_item() {
        let (mut tracks, sounds) = pairing_fixture(&[1], 0.25);
        tracks.truncate(1);
        tracks[0].nests.truncate(1);
        let nest = &mut tracks[0].nests[0];
        nest.enabled = true;
        nest.playback_rate = -1.0;
        nest.reverse_source_duration = Some(4);
        let mut omissions = Vec::new();
        let alone = pair_sounds(&mut tracks, sounds, &mut omissions).unwrap();
        assert!(omissions.is_empty());
        assert!(!tracks[0].nests[0].sequence.has_sound());
        assert_eq!(alone.len(), 1);
        assert_eq!(alone[0].timeline, 0..4);
        assert_eq!(alone[0].source, 0..4);
        assert_eq!(alone[0].gain, 0.25);
        assert!(alone[0].enabled);
    }

    fn compare_pairing(order: &[usize], gain: f64) {
        let (mut actual, sounds) = pairing_fixture(order, gain);
        let (mut expected, old_sounds) = pairing_fixture(order, gain);
        let mut omissions = Vec::new();
        let mut expected_omissions = Vec::new();
        let result = pair_sounds(&mut actual, sounds, &mut omissions);
        let expected_result = old_pair_sounds(&mut expected, old_sounds, &mut expected_omissions);
        let summarize = |result: Result<Vec<NestSound>>| {
            result
                .map(|sounds| sounds.into_iter().map(|sound| sound.id).collect::<Vec<_>>())
                .map_err(|error| error.to_string())
        };
        assert_eq!(summarize(result), summarize(expected_result));
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(format!("{omissions:?}"), format!("{expected_omissions:?}"));
    }

    #[test]
    fn indexed_pairing_matches_scan_order_branches_and_errors() {
        let mut order: Vec<_> = (0..12).collect();
        order.extend([0, 2, 6]);
        for rotation in 0..order.len() {
            order.rotate_left(rotation);
            compare_pairing(&order, 0.5);
            order.reverse();
            compare_pairing(&order, 0.5);
        }
        for index in 0..12 {
            compare_pairing(&[index], 0.5);
            compare_pairing(&[index], f64::MAX);
            compare_pairing(&[index], f64::INFINITY);
            compare_pairing(&[index], f64::NAN);
        }
        compare_pairing(&[], 0.5);
        let stress: Vec<_> = (0..4096).map(|index| index % 12).collect();
        compare_pairing(&stress, 0.5);
    }

    #[test]
    fn indexed_pairing_bounded_cpu_comparison() {
        let (tracks, _) = pairing_fixture(&[], 0.5);
        let template = tracks[0].nests[0].clone();
        let input = || {
            let nests = (0..2048)
                .map(|index| {
                    let mut nest = template.clone();
                    nest.start_ticks = index * 4;
                    nest.end_ticks = index * 4 + 4;
                    nest
                })
                .collect();
            let sounds = (0..2048)
                .map(|index| NestSound {
                    id: index.to_string(),
                    sequence: "inner".into(),
                    timeline: index * 4..index * 4 + 4,
                    source: 0..4,
                    gain: 0.5,
                    volume_keys: None,
                    enabled: template.enabled,
                })
                .collect();
            (
                vec![PrVideoTrack {
                    items: Vec::new(),
                    transitions: Vec::new(),
                    nests,
                }],
                sounds,
            )
        };
        let (mut old_tracks, old_sounds) = input();
        let (mut new_tracks, new_sounds) = input();
        let mut old_omissions = Vec::new();
        let mut new_omissions = Vec::new();
        let start = std::time::Instant::now();
        let old = old_pair_sounds(&mut old_tracks, old_sounds, &mut old_omissions).unwrap();
        let old_elapsed = start.elapsed();
        let start = std::time::Instant::now();
        let new = pair_sounds(&mut new_tracks, new_sounds, &mut new_omissions).unwrap();
        let new_elapsed = start.elapsed();
        assert!(old.is_empty() && new.is_empty());
        assert_eq!(format!("{old_tracks:?}"), format!("{new_tracks:?}"));
        assert_eq!(format!("{old_omissions:?}"), format!("{new_omissions:?}"));
        eprintln!("pairing 2048 disjoint nests: scan={old_elapsed:?}, index={new_elapsed:?}");
    }

    /// A placement from `start` to `end` seconds, from the source's start.
    fn span(start: i64, end: i64) -> Placement {
        Placement {
            start: start * TICKS,
            end: end * TICKS,
            source_in: 0,
        }
    }

    /// Records of placements of `source`, one second each from time zero, with
    /// track items at `base + 10`, `base + 20`, ...
    fn placements(base: u32, source: u32, count: u32) -> (Vec<u32>, String) {
        let items: Vec<u32> = (1..=count).map(|slot| base + 10 * slot).collect();
        let records = items
            .iter()
            .zip(0_i64..)
            .map(|(&id, second)| placement_records(id, source, &span(second, second + 1)))
            .collect();
        (items, records)
    }

    /// "Root": five placements of the sequence source `first`, then one clip of
    /// the `one-clip.xml` media at 5-6 s so that Root converts on its own.
    fn root_records(first: u32) -> String {
        let (mut items, mut records) = placements(900, first, 5);
        items.push(5000);
        records.push_str(&placement_records(5000, 7, &span(5, 6)));
        sequence_records("root", "Root", 900, &items) + &records
    }

    /// Reads Root with `cyclic`, or with the project topology when `None`;
    /// returns Root, the inner reads performed (each leaves one cache entry),
    /// and the omissions.
    fn read_root(
        xml: &str,
        cyclic: Option<&BTreeSet<String>>,
    ) -> (PrSequence, usize, Vec<Omission>) {
        let graph = Graph::parse(xml).unwrap();
        let topology = graph::cyclic_sequences(&graph::sequences(&graph, &mut Vec::new()).unwrap());
        let (root, reads, omissions) = read_within(&graph, "root", cyclic.unwrap_or(&topology));
        (root, reads, omissions)
    }

    /// Reads `guid` and returns the inner read count and omissions.
    fn read_within(
        graph: &Graph<'_>,
        guid: &str,
        cyclic: &BTreeSet<String>,
    ) -> (PrSequence, usize, Vec<Omission>) {
        let mut nesting = Nesting::new(Some(guid), cyclic);
        let mut omissions = Vec::new();
        let sequence = sequence::read_sequence_at(
            graph,
            Some(guid),
            &mut BTreeMap::new(),
            &mut nesting,
            &mut omissions,
        )
        .unwrap();
        (sequence, nesting.finished.len(), omissions)
    }

    fn read_xml(xml: &str, guid: &str) -> (PrSequence, Vec<Omission>) {
        let (sequence, _, omissions) =
            read_within(&Graph::parse(xml).unwrap(), guid, &BTreeSet::new());
        (sequence, omissions)
    }

    fn reasons_of<'a>(omissions: &'a [Omission], records: &[u32]) -> Vec<&'a str> {
        records
            .iter()
            .map(|record| {
                omissions
                    .iter()
                    .find(|item| item.record == record.to_string())
                    .map_or("", |item| item.reason.as_str())
            })
            .collect()
    }

    /// Root places level-0 five times, each level-i places level-(i+1) five
    /// times, and level-6 places level-0: a seven-sequence cycle. Before reads
    /// that met a cycle were cached, this shape read level-6 5^7 times and took
    /// about 17 s.
    #[test]
    fn a_branching_cycle_reads_each_inner_sequence_at_most_once() {
        let level = |index: u32| 1000 + 100 * index;
        let mut records = root_records(level(0) + 2);
        for index in 0..7 {
            let (items, placed) = placements(level(index), level((index + 1) % 7) + 2, 5);
            records.push_str(&sequence_records(
                &format!("level-{index}"),
                &format!("Level {index}"),
                level(index),
                &items,
            ));
            records.push_str(&sequence_source(
                level(index) + 2,
                &format!("level-{index}"),
            ));
            records.push_str(&placed);
        }
        let xml = with_records(ONE_CLIP_XML, &records);
        let root_items = [910, 920, 930, 940, 950];

        // The topology knows every level is on the cycle, so nothing is read.
        let (root, reads, omissions) = read_root(&xml, None);
        assert_eq!(reads, 0);
        assert_eq!(
            reasons_of(&omissions, &root_items),
            ["unsupported conversion: nested sequence level-0 is on a nesting cycle; cyclic nesting is not converted"; 5]
        );
        assert_eq!(
            (
                root.video_occurrences().count(),
                root.nest_occurrences().count()
            ),
            (1, 0)
        );

        // Without the topology the cycle closes where level-6 places level-0,
        // and every finished read is reused: seven reads in all.
        let (root, reads, omissions) = read_root(&xml, Some(&BTreeSet::new()));
        assert_eq!(reads, 7);
        assert_eq!(
            reasons_of(&omissions, &[1610, 1620, 1630, 1640, 1650]),
            ["unsupported conversion: nested sequence level-0 contains itself; cyclic nesting is not converted"; 5]
        );
        assert_eq!(
            (
                root.video_occurrences().count(),
                root.nest_occurrences().count()
            ),
            (1, 0)
        );
    }

    /// Base64 Source Text with `text`, in the style of the graphic fixtures.
    fn source_text(text: &str, font: &str) -> String {
        let payload = STANDARD
            .decode(crate::format::text_payload::tests::BEFORE)
            .unwrap();
        let mut document = text_payload::decode(&payload).unwrap().document;
        document.text = text.into();
        document.font = font.into();
        STANDARD.encode(text_payload::encode(&document).unwrap())
    }

    /// Root, Mid and Leaf each hold four 2 s graphics that share one stored
    /// 4 KiB text; Root and Mid then place the next level at 8-9 s.
    fn shared_text_chain() -> String {
        chain_over(graphic_source_records(&source_text(
            &"x".repeat(4096),
            "Arial",
        )))
    }

    /// [`shared_text_chain`] whose graphics share a stored Source Text with
    /// four keys of 4 KiB text each (16 KiB in keys) beside a short static
    /// document, in the Premiere 26.5.1 saved form.
    fn keyed_text_chain() -> String {
        let keys: String = (0_i64..4)
            .map(|index| {
                format!(
                    "{},{};",
                    914_161_248_000_000 + index * TICKS / 2,
                    source_text(&"k".repeat(4096), "Arial")
                )
            })
            .collect();
        let records = graphic_source_records(&source_text("x", "Arial"))
            .replacen(
                "<Name>Source Text</Name>",
                "<Name>Source Text</Name><IsTimeVarying>true</IsTimeVarying>",
                1,
            )
            .replacen(
                "<StartKeyframeValue ",
                &format!("<Keyframes>{keys}</Keyframes><StartKeyframeValue "),
                1,
            );
        chain_over(records)
    }

    fn chain_over(mut records: String) -> String {
        for (guid, name, base, next) in [
            ("root", "Root", 1000, Some(2002)),
            ("mid", "Mid", 2000, Some(3002)),
            ("leaf", "Leaf", 3000, None),
        ] {
            let mut items: Vec<u32> = (1..=4).map(|slot| base + 10 * slot).collect();
            for (&id, index) in items.iter().zip(0_i64..) {
                records.push_str(&graphic_item_record(id, 2 * index * TICKS));
            }
            if let Some(source) = next {
                items.push(base + 50);
                records.push_str(&placement_records(base + 50, source, &span(8, 9)));
            }
            records.push_str(&sequence_records(guid, name, base, &items));
            if guid != "root" {
                records.push_str(&sequence_source(base + 2, guid));
            }
        }
        with_records(ONE_CLIP_XML, &records)
    }

    #[test]
    fn nested_text_and_keys_survive_copying() {
        let (root, omissions) = read_xml(&keyed_text_chain(), "root");
        assert!(omissions.is_empty(), "{omissions:?}");
        let mid = &root.nest_occurrences().next().unwrap().sequence;
        let leaf = &mid.nest_occurrences().next().unwrap().sequence;
        for timeline in [&root, mid, leaf] {
            assert_eq!(timeline.video_items().count(), 4);
            let text = timeline
                .video_items()
                .next()
                .unwrap()
                .graphic()
                .unwrap()
                .text();
            assert_eq!(text.source_text_keys.len(), 4);
            assert_eq!(text.source_text_keys[0].document.text.len(), 4096);
        }
    }

    #[test]
    fn nested_shared_text_survives_copying() {
        let (root, omissions) = read_xml(&shared_text_chain(), "root");
        assert!(omissions.is_empty(), "{omissions:?}");
        let mid = &root.nest_occurrences().next().unwrap().sequence;
        let leaf = &mid.nest_occurrences().next().unwrap().sequence;
        for timeline in [&root, mid, leaf] {
            assert_eq!(timeline.video_items().count(), 4);
            assert_eq!(
                timeline
                    .video_items()
                    .next()
                    .unwrap()
                    .graphic()
                    .unwrap()
                    .text()
                    .document
                    .text
                    .len(),
                4096
            );
        }
    }

    #[test]
    fn caption_cues_keep_all_text() {
        let text = "c".repeat(2048);
        let cues: Vec<_> = (0..4_u32)
            .zip(0_i64..)
            .map(|(index, second)| cue(1000 + 10 * index, second * 30..(second + 1) * 30, &text))
            .collect();
        let (sequence, omissions) = read_xml(&captions_xml(&[cues]), "sequence-1");
        assert!(omissions.is_empty(), "{omissions:?}");
        let captions: Vec<_> = sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .collect();
        assert_eq!(captions.len(), 4);
        assert!(captions
            .iter()
            .all(|graphic| graphic.text().document.text == text));
    }

    /// `feature_linked_av_strict.prproj` (sequence `80acdd81-…`: one video and
    /// one sound of the same A/V media) with `extra` more sound items, each
    /// `(ObjectID, SubClip)`, listed after the original on its audio track.
    /// With `sound_media_uid`, the sound plays a copy of the media record
    /// under that ObjectUID.
    fn linked_av_xml(extra: &[(u32, u32)], sound_media_uid: Option<&str>) -> String {
        let mut xml = String::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../../tests/fixtures/feature_linked_av_strict.prproj")[..],
        )
        .read_to_string(&mut xml)
        .unwrap();
        let start = xml.find(r#"<AudioClipTrackItem ObjectID="122""#).unwrap();
        let item = xml[start..]
            .split_inclusive("</AudioClipTrackItem>")
            .next()
            .unwrap()
            .to_owned();
        let mut references = String::from(r#"<TrackItem Index="0" ObjectRef="122"/>"#);
        let mut records = String::new();
        for (index, (id, sub_clip)) in (1..).zip(extra) {
            references.push_str(&format!(r#"<TrackItem Index="{index}" ObjectRef="{id}"/>"#));
            records.push_str(
                &item
                    .replacen(r#"ObjectID="122""#, &format!(r#"ObjectID="{id}""#), 1)
                    .replacen(
                        r#"<SubClip ObjectRef="144"/>"#,
                        &format!(r#"<SubClip ObjectRef="{sub_clip}"/>"#),
                        1,
                    ),
            );
        }
        let mut xml = with_records(
            &xml.replacen(r#"<TrackItem Index="0" ObjectRef="122"/>"#, &references, 1),
            &records,
        );
        if let Some(uid) = sound_media_uid {
            let start = xml.find("<Media ObjectUID=\"").unwrap();
            let media = xml[start..]
                .split_inclusive("</Media>")
                .next()
                .unwrap()
                .to_owned();
            let old = media.split('"').nth(1).unwrap().to_owned();
            let source = xml.find("<AudioMediaSource").unwrap();
            let reference = source
                + xml[source..]
                    .find(&format!("ObjectURef=\"{old}\""))
                    .unwrap();
            xml.replace_range(
                reference..reference + old.len() + "ObjectURef=\"\"".len(),
                &format!("ObjectURef=\"{uid}\""),
            );
            xml = with_records(&xml, &media.replacen(&old, uid, 1));
        }
        xml
    }

    const LINKED_AV: &str = "80acdd81-0a96-4677-b17f-b2ffe2dff738";

    /// Outer places the linked-A/V sequence `placements` times at 1 s steps;
    /// its audio track holds 16 sounds.
    fn nested_sound_fanout(placements: u32) -> (Vec<u32>, String) {
        let extra: Vec<_> = (9001..9016).map(|id| (id, 144)).collect();
        let items: Vec<u32> = (0..placements).map(|index| 10010 + 10 * index).collect();
        let mut records = sequence_records("outer", "Outer", 10000, &items);
        records.push_str(&sequence_source(10002, LINKED_AV));
        for (&id, second) in items.iter().zip(0_i64..) {
            records.push_str(&placement_records(id, 10002, &span(second, second + 1)));
        }
        (items, with_records(&linked_av_xml(&extra, None), &records))
    }

    fn read_outer(xml: &str) -> (PrSequence, usize, Vec<Omission>) {
        let (outer, reads, omissions) =
            read_within(&Graph::parse(xml).unwrap(), "outer", &BTreeSet::new());
        (outer, reads, omissions)
    }

    /// 16 placements of the linked-A/V sequence, none with an audio item: one
    /// inner read; each copy keeps its picture and none of the 16 inner
    /// sounds, which Premiere plays only through a nest's audio item, and
    /// nothing audible is reported lost.
    #[test]
    fn a_nest_without_an_audio_item_drops_its_unheard_sound_silently() {
        let (_, xml) = nested_sound_fanout(16);
        let (outer, reads, omissions) = read_outer(&xml);
        assert_eq!(reads, 1);
        assert_eq!(outer.nest_occurrences().count(), 16);
        for nest in outer.nest_occurrences() {
            assert!(nest.sequence.audio.is_empty());
            assert_eq!(nest.sequence.video_occurrences().count(), 1);
        }
        assert!(
            !omissions
                .iter()
                .any(|item| item.record.starts_with("VideoClipTrackItem:100")),
            "{omissions:?}"
        );
    }

    /// Mid holds a sound and a nest of Inner, whose sound is keyed; Outer
    /// nests Mid. Each level's audio item scales every sound below it, as the
    /// reader pairs Inner's item while it reads Mid and Mid's while it reads
    /// Outer: Inner's sound ends at the product of both item gains.
    #[test]
    fn each_nest_level_scales_every_sound_below_it() {
        let sound = |volume: f64, keys: Option<PrVolumeKeys>| PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: 0,
            out_ticks: 4 * TICKS,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: keys,
            fade_in: None,
            fade_out: None,
        };
        let nested = |guid: &str, mut sequence: PrSequence| {
            sequence.id = Some(guid.into());
            vec![PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests: vec![nest_of(sequence, 0..4 * TICKS, 0)],
            }]
        };
        let item = |guid: &str, gain: f64| NestSound {
            id: format!("{guid} item"),
            sequence: guid.into(),
            timeline: 0..4 * TICKS,
            source: 0..4 * TICKS,
            gain,
            volume_keys: None,
            enabled: true,
        };
        let mut inner = sequence_of(
            "Inner",
            vec![PrVideoTrack::media([clip_of("timecoded", 0..4 * TICKS, 0)])],
        );
        let keys = vec![PrScalarKeyframe {
            source_ticks: 0,
            value: 1.0,
            easing: PrKeyframeEasing::Linear,
        }];
        inner.audio = vec![sound(0.5, Some(PrVolumeKeys { keys, gain: 0.5 }))];
        let mut mid = sequence_of("Mid", nested("inner", inner));
        mid.audio = vec![sound(1.0, None)];
        let mut omissions = Vec::new();
        let alone = pair_sounds(
            &mut mid.video_tracks,
            vec![item("inner", 0.25)],
            &mut omissions,
        )
        .unwrap();
        assert!(alone.is_empty());
        let mut outer = nested("mid", mid);
        let alone = pair_sounds(&mut outer, vec![item("mid", 0.5)], &mut omissions).unwrap();
        assert!(alone.is_empty() && omissions.is_empty(), "{omissions:?}");

        let mid = &outer[0].nests[0].sequence;
        let inner = &mid.video_tracks[0].nests[0].sequence;
        assert_eq!(mid.audio[0].volume.as_f64(), 0.5);
        assert_eq!(inner.audio[0].volume.as_f64(), 0.0625);
        assert_eq!(inner.audio[0].volume_keys.as_ref().unwrap().gain, 0.0625);
    }

    /// An audio item plays its nest at normal speed, so even the item over a
    /// retimed nest's range from its In pairs with none: the nest keeps its
    /// picture without sound, and the item plays alone.
    #[test]
    fn a_retimed_nest_pairs_no_audio_item_and_keeps_only_its_picture() {
        let mut inner = sequence_of(
            "Inner",
            vec![PrVideoTrack::media([clip_of("timecoded", 0..4 * TICKS, 0)])],
        );
        inner.id = Some("inner".into());
        inner.audio = vec![PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: 0,
            out_ticks: 4 * TICKS,
            volume: fx_schema::LinearGain::new(1.0).unwrap(),
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        }];
        // Inner 0-2 s over 0-4 s: half speed.
        let mut nest = nest_of(inner, 0..4 * TICKS, 0);
        nest.out_ticks = 2 * TICKS;
        assert!(nest.is_retimed());
        let mut tracks = vec![PrVideoTrack {
            transitions: Vec::new(),
            items: Vec::new(),
            nests: vec![nest],
        }];
        let item = NestSound {
            id: "inner item".into(),
            sequence: "inner".into(),
            timeline: 0..4 * TICKS,
            source: 0..4 * TICKS,
            gain: 0.5,
            volume_keys: None,
            enabled: true,
        };
        let mut omissions = Vec::new();
        let alone = pair_sounds(&mut tracks, vec![item], &mut omissions).unwrap();
        assert_eq!(
            alone
                .iter()
                .map(|sound| sound.id.as_str())
                .collect::<Vec<_>>(),
            ["inner item"]
        );
        assert!(omissions.is_empty(), "{omissions:?}");
        let [nest] = tracks[0].nests.as_slice() else {
            panic!("the nest keeps its picture");
        };
        assert!(nest.sequence.audio.is_empty());
        assert_eq!(nest.sequence.video_occurrences().count(), 1);
    }

    /// Mid holds a keyed sound at 1-3 s from source 5 s, a nest of Inner at
    /// 2-6 s from Inner 1 s and a hidden one at 6-8 s from Inner 0 s; each
    /// nest's group carries Inner's sound at 0-4 s, folded to 0.25. An item
    /// that plays Mid from Mid 1.5 s to 7 s at outer 10 s and gain 0.5 hears
    /// each part of every level that its window shows, on the outer clock,
    /// keys on their source clock; the hidden group carries a disabled item's
    /// sound, which plays at zero gain.
    #[test]
    fn a_sound_that_plays_alone_keeps_every_nested_clock() {
        let at = |seconds: f64| (seconds * TICKS as f64) as i64;
        let sound = |timeline: Range<i64>, source_in: i64, volume: f64| PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            out_ticks: source_in + timeline.end - timeline.start,
            start_ticks: timeline.start,
            end_ticks: timeline.end,
            in_ticks: source_in,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        };
        let mut inner = sequence_of("Inner", Vec::new());
        inner.audio = vec![sound(0..at(4.0), 0, 0.25)];
        let hidden = PrNestOccurrence {
            enabled: false,
            ..nest_of(inner.clone(), at(6.0)..at(8.0), 0)
        };
        let mut mid = sequence_of(
            "Mid",
            vec![PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests: vec![nest_of(inner, at(2.0)..at(6.0), at(1.0)), hidden],
            }],
        );
        let keys = vec![PrScalarKeyframe {
            source_ticks: at(6.0),
            value: 1.0,
            easing: PrKeyframeEasing::Linear,
        }];
        mid.audio = vec![PrAudioOccurrence {
            volume_keys: Some(PrVolumeKeys { keys, gain: 0.8 }),
            ..sound(at(1.0)..at(3.0), at(5.0), 0.8)
        }];
        let mut played = Vec::new();
        played_sounds(
            &mid,
            &(at(1.5)..at(7.0)),
            at(8.5),
            0.5,
            &mut played,
            &mut Vec::new(),
        )
        .unwrap();
        let rows: Vec<_> = played
            .iter()
            .map(|clip| {
                (
                    [
                        clip.start_ticks,
                        clip.end_ticks,
                        clip.in_ticks,
                        clip.out_ticks,
                    ],
                    clip.volume.as_f64(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                // Mid 1.5-3 s from source 5.5 s.
                ([at(10.0), at(11.5), at(5.5), at(7.0)], 0.4),
                // Mid 2-6 s shows Inner 1-5 s, whose sound ends at 4 s.
                ([at(10.5), at(13.5), at(1.0), at(4.0)], 0.125),
                // The window ends at Mid 7 s, Inner 1 s of the hidden nest.
                ([at(14.5), at(15.5), 0, at(1.0)], 0.0),
            ]
        );
        let keys = played[0].volume_keys.as_ref().unwrap();
        assert_eq!((keys.keys[0].source_ticks, keys.gain), (at(6.0), 0.4));
    }

    fn key(seconds: f64, value: f64, easing: PrKeyframeEasing) -> PrScalarKeyframe {
        PrScalarKeyframe {
            source_ticks: (seconds * TICKS as f64) as i64,
            value,
            easing,
        }
    }

    /// Keys at 1 s and 2 s at 1.0, a Hold into 0.5 at 3 s and Linear to 0.25
    /// at 4 s: each source range hears one Level only while no key it hears
    /// changes it, and a Hold into its end changes nothing inside it.
    #[test]
    fn level_over_is_the_one_level_that_a_source_range_hears() {
        use PrKeyframeEasing::{Hold, Linear};
        let keys = [
            key(1.0, 1.0, Linear),
            key(2.0, 1.0, Linear),
            key(3.0, 0.5, Hold),
            key(4.0, 0.25, Linear),
        ];
        let over = |keys: &[PrScalarKeyframe], from: f64, to: f64| {
            let at = |seconds: f64| (seconds * TICKS as f64) as i64;
            level_over(keys, &(at(from)..at(to)))
        };
        for (from, to, level) in [
            (0.0, 0.5, Some(1.0)),
            (0.0, 2.5, Some(1.0)),
            // The Hold into 3 s changes the Level at the range's end.
            (1.5, 3.0, Some(1.0)),
            (2.5, 3.0, Some(1.0)),
            (1.5, 3.5, None),
            (3.0, 3.5, None),
            (4.0, 5.0, Some(0.25)),
            (4.5, 5.0, Some(0.25)),
        ] {
            assert_eq!(over(&keys, from, to), level, "{from}-{to} s");
        }
        // A Linear key at the range's end is approached inside it.
        let fade = [key(1.0, 1.0, Linear), key(3.0, 0.5, Linear)];
        assert_eq!(over(&fade, 0.0, 1.0), Some(1.0));
        assert_eq!(over(&fade, 2.0, 3.0), None);
        assert_eq!(over(&fade[..1], 2.0, 3.0), Some(1.0));
        assert_eq!(over(&[], 2.0, 3.0), None);
    }

    /// One sound at outer 10-12 s from source 0, played by an item from its
    /// source 0 at 10 s, so that the item's keys stay at their times, under
    /// the item's Level keys (0.5 at 0.5 s, Linear to 1.0 at 1.5 s) and other
    /// stages of 0.8.
    #[test]
    fn a_sound_takes_the_item_level_keys_while_one_level_holds_over_it() {
        use PrKeyframeEasing::Linear;
        let clip = |volume: f64, keys: Option<PrVolumeKeys>| PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 10 * TICKS,
            end_ticks: 12 * TICKS,
            in_ticks: 0,
            out_ticks: 2 * TICKS,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: keys,
            fade_in: None,
            fade_out: None,
        };
        let item_keys = vec![key(0.5, 0.5, Linear), key(1.5, 1.0, Linear)];
        let item = |enabled: bool| NestSound {
            id: "item".into(),
            sequence: "inner".into(),
            timeline: 10 * TICKS..12 * TICKS,
            source: 0..2 * TICKS,
            gain: 0.8,
            volume_keys: Some(PrVolumeKeys {
                keys: item_keys.clone(),
                gain: 0.8,
            }),
            enabled,
        };
        // The sound's own keys: holding 0.25 past its range, or changing
        // over it.
        let holding = PrVolumeKeys {
            keys: vec![key(3.0, 0.25, Linear), key(4.0, 1.0, Linear)],
            gain: 0.5,
        };
        let changing = PrVolumeKeys {
            keys: vec![key(1.0, 1.0, Linear), key(2.0, 0.5, Linear)],
            gain: 0.5,
        };
        let silent = PrVolumeKeys {
            gain: 0.0,
            ..changing.clone()
        };
        // The item's Level holds 0.75 over the sound.
        let flat = PrVolumeKeys {
            keys: vec![key(0.0, 0.75, Linear), key(5.0, 0.75, Linear)],
            gain: 0.8,
        };
        let flat_item = NestSound {
            volume_keys: Some(flat.clone()),
            ..item(true)
        };
        // (row, sound, item, converts, volume, keys and their gain)
        let rows = [
            (
                "static",
                clip(0.5, None),
                item(true),
                true,
                0.4,
                Some((item_keys.clone(), 0.4)),
            ),
            (
                "own Level holds",
                clip(0.5, Some(holding.clone())),
                item(true),
                true,
                0.1,
                Some((item_keys.clone(), 0.1)),
            ),
            (
                "item Level holds",
                clip(0.5, Some(changing.clone())),
                flat_item,
                true,
                0.3,
                Some((changing.keys.clone(), 0.3)),
            ),
            (
                "both change",
                clip(0.5, Some(changing.clone())),
                item(true),
                false,
                0.5,
                Some((changing.keys.clone(), 0.5)),
            ),
            (
                "both change, sound silent",
                clip(0.0, Some(silent.clone())),
                item(true),
                true,
                0.0,
                Some((changing.keys.clone(), 0.0)),
            ),
            (
                "both change, item disabled",
                clip(0.5, Some(changing.clone())),
                item(false),
                true,
                0.0,
                Some((changing.keys.clone(), 0.0)),
            ),
        ];
        for (name, mut sound, item, converts, volume, keys) in rows {
            let item_level = item.volume_keys.as_ref().unwrap();
            assert_eq!(
                apply_level_keys(&mut sound, &item, item_level, 10 * TICKS).unwrap(),
                converts,
                "{name}"
            );
            assert!(
                (sound.volume.as_f64() - volume).abs() < 1e-12,
                "{name}: {sound:?}"
            );
            let actual = sound
                .volume_keys
                .as_ref()
                .map(|keys| (keys.keys.clone(), keys.gain));
            let near = actual.as_ref().zip(keys.as_ref()).is_some_and(
                |((actual, actual_gain), (expected, expected_gain))| {
                    actual == expected && (actual_gain - expected_gain).abs() < 1e-12
                },
            );
            assert!(near, "{name}: {actual:?}");
        }
    }

    #[test]
    fn audio_clock_outer_nested_gain_keys_follow_retimed_inner_source() {
        for rate in [2.0, -2.0] {
            let mut clip = PrAudioOccurrence {
                id: None,
                media: MediaId("timecoded".into()),
                source_channel: None,
                preserve_audio_pitch: false,
                playback_rate: rate,
                start_ticks: 10 * TICKS,
                end_ticks: 12 * TICKS,
                in_ticks: 0,
                out_ticks: 4 * TICKS,
                volume: fx_schema::LinearGain::new(0.5).unwrap(),
                volume_keys: None,
                fade_in: Some(PrAudioFade {
                    id: None,
                    duration_ticks: 3 * TICKS / 4,
                    curve: crate::schema::PrFadeCurve::ConstantGain,
                }),
                fade_out: None,
            };
            let keys = PrVolumeKeys {
                gain: 0.8,
                keys: vec![
                    key(0.5, 0.5, PrKeyframeEasing::Linear),
                    key(1.5, 1.0, PrKeyframeEasing::Linear),
                ],
            };
            let item = NestSound {
                id: "outer".into(),
                sequence: "inner".into(),
                timeline: 10 * TICKS..12 * TICKS,
                source: 0..2 * TICKS,
                gain: 0.8,
                volume_keys: Some(keys.clone()),
                enabled: true,
            };
            assert!(apply_level_keys(&mut clip, &item, &keys, 10 * TICKS).unwrap());
            let gain = clip.volume_keys.as_ref().unwrap();
            assert_eq!(
                gain.keys,
                [
                    key(1.0, 0.5, PrKeyframeEasing::Linear),
                    key(3.0, 1.0, PrKeyframeEasing::Linear)
                ]
            );
            assert!((gain.gain - 0.4).abs() < 1e-12);
            assert_eq!(unheld_fades(&mut clip).len(), 1);
            assert!(clip.fade_in.is_none());
        }
    }

    #[test]
    fn audio_clock_retimed_nested_window_trims_source_and_reports_partial_fade() {
        for rate in [2.0, 0.5, -2.0] {
            let mut inner = crate::tests::support::video_sequence();
            inner.video_tracks.clear();
            inner.audio = vec![PrAudioOccurrence {
                source_channel: None,
                preserve_audio_pitch: false,
                playback_rate: rate,
                id: Some("trim sound".into()),
                media: MediaId("source".into()),
                start_ticks: 0,
                end_ticks: 4 * TICKS,
                in_ticks: TICKS,
                out_ticks: TICKS + (4.0 * rate.abs()) as i64 * TICKS,
                volume: fx_schema::LinearGain::UNITY,
                volume_keys: None,
                fade_in: Some(PrAudioFade {
                    id: Some("partial".into()),
                    curve: crate::schema::PrFadeCurve::ConstantGain,
                    duration_ticks: 2 * TICKS,
                }),
                fade_out: Some(PrAudioFade {
                    id: Some("whole".into()),
                    curve: crate::schema::PrFadeCurve::ConstantGain,
                    duration_ticks: TICKS,
                }),
            }];
            let mut played = Vec::new();
            let mut notes = Vec::new();
            played_sounds(
                &inner,
                &(TICKS..4 * TICKS),
                10 * TICKS,
                0.5,
                &mut played,
                &mut notes,
            )
            .unwrap();
            let sound = &played[0];
            assert_eq!(
                (sound.start_ticks, sound.end_ticks),
                (11 * TICKS, 14 * TICKS)
            );
            assert_eq!(sound.in_ticks, TICKS + (rate.abs() * TICKS as f64) as i64);
            assert_eq!(sound.out_ticks, inner.audio[0].out_ticks);
            assert!(sound.fade_in.is_none());
            assert!(sound.fade_out.is_some());
            assert_eq!(sound.volume.as_f64(), 0.5);
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].record, "partial");
        }
    }

    /// The Level at `ticks` of keys that only step: the last key's at or
    /// before it, or the first key's before them all.
    fn stepped(keys: &[PrScalarKeyframe], ticks: i64) -> f64 {
        keys.iter()
            .rfind(|key| key.source_ticks <= ticks)
            .unwrap_or(&keys[0])
            .value
    }

    /// One sound at outer 10-12 s from source 0 whose Level keys only step:
    /// 0.8 from -0.5 s, before its In (a first key, Linear as the reader gives
    /// it), a Hold to 0.25 at 1 s, Linear to the same 0.25 at 1.25 s, and a
    /// Hold to 1.0 at 2 s, its Out. The item plays it from its source 0 at
    /// 10 s, so that its keys stay at their times, and steps too: 1.0 from
    /// 0.5 s, then Holds to 0.5 at 1 s, where the sound's step lands, to
    /// silence at 1.5 s, to 1.0 at 1.75 s and to 0.25 at 3 s, past the sound.
    /// The sound takes Hold keys at every time either has one, valued at the
    /// product of both Levels, which is that product at every tick; a
    /// disabled item keeps them at zero gain, and a stepping item one nest
    /// level up multiplies them again. A product past the largest gain
    /// fails. A Linear change of Level in either, even outside the sound, is
    /// not a step, and the sound is not converted.
    #[test]
    fn step_levels_multiply_into_hold_keys_at_every_key_of_either() {
        use PrKeyframeEasing::{Hold, Linear};
        let own_keys = vec![
            key(-0.5, 0.8, Linear),
            key(1.0, 0.25, Hold),
            key(1.25, 0.25, Linear),
            key(2.0, 1.0, Hold),
        ];
        let item_keys = vec![
            key(0.5, 1.0, Linear),
            key(1.0, 0.5, Hold),
            key(1.5, 0.0, Hold),
            key(1.75, 1.0, Hold),
            key(3.0, 0.25, Hold),
        ];
        // The sound's Level before its keys times its other stages, 0.5.
        let sound = |keys: Vec<PrScalarKeyframe>| PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 10 * TICKS,
            end_ticks: 12 * TICKS,
            in_ticks: 0,
            out_ticks: 2 * TICKS,
            volume: fx_schema::LinearGain::new(0.25).unwrap(),
            volume_keys: Some(PrVolumeKeys { keys, gain: 0.5 }),
            fade_in: None,
            fade_out: None,
        };
        // The item's Level before its keys, 2/3, times its other stages, 0.75.
        let item = |keys: Vec<PrScalarKeyframe>, enabled: bool| NestSound {
            id: "item".into(),
            sequence: "inner".into(),
            timeline: 10 * TICKS..12 * TICKS,
            source: 0..2 * TICKS,
            gain: 0.5,
            volume_keys: Some(PrVolumeKeys { keys, gain: 0.75 }),
            enabled,
        };
        let apply = |clip: &mut PrAudioOccurrence, item: &NestSound, shift: i64| {
            apply_level_keys(clip, item, item.volume_keys.as_ref().unwrap(), shift).unwrap()
        };
        // Every tick at and next to each key of `keys`.
        let near_keys = |keys: &[PrScalarKeyframe]| {
            keys.iter()
                .flat_map(|key| [key.source_ticks - 1, key.source_ticks, key.source_ticks + 1])
                .collect::<Vec<_>>()
        };
        let product = vec![
            key(-0.5, 0.8, Linear),
            key(0.5, 0.8, Hold),
            key(1.0, 0.125, Hold),
            key(1.25, 0.125, Hold),
            key(1.5, 0.0, Hold),
            key(1.75, 0.25, Hold),
            key(2.0, 1.0, Hold),
            key(3.0, 0.25, Hold),
        ];
        // (volume, gain of the keys)
        for (enabled, gains) in [(true, (0.125, 0.375)), (false, (0.0, 0.0))] {
            let mut clip = sound(own_keys.clone());
            assert!(apply(
                &mut clip,
                &item(item_keys.clone(), enabled),
                10 * TICKS
            ));
            let keys = clip.volume_keys.as_ref().unwrap();
            assert_eq!((clip.volume.as_f64(), keys.gain), gains, "{enabled}");
            assert_eq!(keys.keys, product, "{enabled}");
            for ticks in near_keys(&keys.keys) {
                assert_eq!(
                    stepped(&keys.keys, ticks),
                    stepped(&item_keys, ticks) * stepped(&own_keys, ticks),
                    "{ticks}"
                );
            }
        }

        // One level up, an item that plays the sound's sequence from its 9 s
        // at 20 s and steps at 10.5 s (1.0), 11 s (0.5) and 12.5 s (silence).
        let mut clip = sound(own_keys.clone());
        assert!(apply(&mut clip, &item(item_keys.clone(), true), 10 * TICKS));
        let mut inner = sequence_of("Inner", Vec::new());
        inner.audio = vec![clip];
        let top_keys = vec![
            key(10.5, 1.0, Linear),
            key(11.0, 0.5, Hold),
            key(12.5, 0.0, Hold),
        ];
        let top = NestSound {
            timeline: 20 * TICKS..24 * TICKS,
            source: 9 * TICKS..13 * TICKS,
            gain: 1.0,
            volume_keys: Some(PrVolumeKeys {
                keys: top_keys.clone(),
                gain: 1.0,
            }),
            ..item(Vec::new(), true)
        };
        let mut played = Vec::new();
        played_sounds(
            &inner,
            &top.source,
            11 * TICKS,
            1.0,
            &mut played,
            &mut Vec::new(),
        )
        .unwrap();
        let [mut clip] = <[_; 1]>::try_from(played).unwrap();
        assert!(apply(&mut clip, &top, 11 * TICKS));
        let keys = clip.volume_keys.unwrap();
        assert_eq!((clip.volume.as_f64(), keys.gain), (0.125, 0.375));
        // Inner 10 s is the sound's source 0.
        for ticks in near_keys(&keys.keys) {
            assert_eq!(
                stepped(&keys.keys, ticks),
                stepped(&top_keys, ticks + 10 * TICKS)
                    * (stepped(&item_keys, ticks) * stepped(&own_keys, ticks)),
                "{ticks}"
            );
        }

        // A product past the largest gain overflows.
        let mut clip = sound(vec![key(0.0, 1e200, Linear), key(1.5, 1.0, Hold)]);
        let huge = item(vec![key(0.5, 1e200, Linear), key(1.0, 1.0, Hold)], true);
        let keys = huge.volume_keys.as_ref().unwrap();
        let error = apply_level_keys(&mut clip, &huge, keys, 10 * TICKS).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("unsupported conversion: {GAIN_OVERFLOW}")
        );

        // The item fades after the sound, or the sound before its In.
        let mut item_fade = item_keys.clone();
        item_fade.push(key(4.0, 1.0, Linear));
        let own_fade = [vec![key(-1.0, 0.4, Linear)], own_keys.clone()].concat();
        for (name, own, item_keys) in [
            ("item fades", own_keys.clone(), item_fade),
            ("sound fades", own_fade, item_keys),
        ] {
            let mut clip = sound(own.clone());
            assert!(
                !apply(&mut clip, &item(item_keys, true), 10 * TICKS),
                "{name}"
            );
            assert_eq!(clip.volume.as_f64(), 0.25, "{name}");
            assert_eq!(clip.volume_keys.unwrap().keys, own, "{name}");
        }
    }

    /// The item of `a_sound_that_plays_alone_keeps_every_nested_clock`, with
    /// Level keys at Mid 2 s (1.0) and 4 s (0.5): each sound it hears takes
    /// them on its own source clock, through the nest level too.
    #[test]
    fn level_keys_reach_each_nested_sound_on_its_own_clock() {
        use PrKeyframeEasing::Linear;
        let at = |seconds: f64| (seconds * TICKS as f64) as i64;
        let sound = |timeline: Range<i64>, source_in: i64, volume: f64| PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            out_ticks: source_in + timeline.end - timeline.start,
            start_ticks: timeline.start,
            end_ticks: timeline.end,
            in_ticks: source_in,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        };
        let mut inner = sequence_of("Inner", Vec::new());
        inner.audio = vec![sound(0..at(4.0), 0, 0.25)];
        let hidden = PrNestOccurrence {
            enabled: false,
            ..nest_of(inner.clone(), at(6.0)..at(8.0), 0)
        };
        let mut mid = sequence_of(
            "Mid",
            vec![PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests: vec![nest_of(inner, at(2.0)..at(6.0), at(1.0)), hidden],
            }],
        );
        mid.audio = vec![sound(at(1.0)..at(3.0), at(5.0), 0.8)];
        let item = NestSound {
            id: "item".into(),
            sequence: "mid".into(),
            timeline: at(10.0)..at(15.5),
            source: at(1.5)..at(7.0),
            gain: 0.5,
            volume_keys: Some(PrVolumeKeys {
                keys: vec![key(2.0, 1.0, Linear), key(4.0, 0.5, Linear)],
                gain: 0.5,
            }),
            enabled: true,
        };
        let shift = at(8.5);
        let mut played = Vec::new();
        played_sounds(&mid, &item.source, shift, 1.0, &mut played, &mut Vec::new()).unwrap();
        let rows: Vec<_> = played
            .into_iter()
            .map(|mut clip| {
                let keys = item.volume_keys.as_ref().unwrap();
                assert!(apply_level_keys(&mut clip, &item, keys, shift).unwrap());
                let keys = clip.volume_keys.unwrap();
                (
                    [clip.start_ticks, clip.in_ticks],
                    keys.keys
                        .iter()
                        .map(|key| key.source_ticks)
                        .collect::<Vec<_>>(),
                    keys.gain,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                // Mid 2 s plays at outer 10.5 s, source 6 s of Mid's sound.
                ([at(10.0), at(5.5)], vec![at(6.0), at(8.0)], 0.4),
                // Mid 2 s shows Inner 1 s, source 1 s of Inner's sound.
                ([at(10.5), at(1.0)], vec![at(1.0), at(3.0)], 0.125),
                // Mid 6 s shows the hidden nest's Inner 0 s.
                ([at(14.5), 0], vec![at(-4.0), at(-2.0)], 0.0),
            ]
        );
    }
}
