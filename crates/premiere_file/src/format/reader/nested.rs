//! Nested-sequence placements: a video track item whose source is another sequence.
//!
//! A nest is read by the placement rules shared with media
//! (`video::read_placement`), clip Enable and track output included. It
//! converts only as a plain placement: one constant forward speed that its
//! saved window confirms ([`speed_matches_window`]), intrinsic Motion (static
//! or keyed, without a Track Matte Key), default Crop and Opacity value
//! without keys, any Blend Mode, no effects but a Track Matte Key, and the
//! outer canvas, with the outer frame rate unless it is retimed. The ordinary
//! sequence reader reads its inner timeline, at most [`MAX_NEST_DEPTH`] levels
//! deep. Repeated placements share their media records and reuse one read of
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
    required_integer, sequence,
    video::{read_placement, report_unknown_children},
};
use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{graph::Element, Graph, Located, Record},
    omit,
    schema::{
        native::VideoClipTrackItem, occurrence_edits, records, ClipEdits, FrameRate, MediaId,
        OccurrenceEdit, PrAudioOccurrence, PrBlendMode, PrKeyframeEasing, PrMedia,
        PrNestOccurrence, PrPropertyAnimation, PrScalarKeyframe, PrSequence, PrStaticTransform,
        PrTrackMatte, PrVideoItem, PrVideoTrack, PrVolumeKeys, MAX_NEST_DEPTH,
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
    /// Effective picture output, from clip Enable and track output.
    enabled: bool,
    /// The placement's Motion, which its group carries.
    transform: PrStaticTransform,
    animations: Vec<PrPropertyAnimation>,
    track_matte: Option<PrTrackMatte>,
    blend_mode: PrBlendMode,
    read: ReadKey,
}

/// A sequence GUID and the nesting levels still allowed below it.
type ReadKey = (String, usize);

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

    /// Reads `guid` once per remaining depth; returns the cache key and the
    /// cached read.
    fn read(
        &mut self,
        graph: &Graph<'_>,
        guid: &str,
        media: &mut BTreeMap<MediaId, PrMedia>,
        omissions: &mut Vec<Omission>,
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
        let key = (guid.to_owned(), MAX_NEST_DEPTH - level);
        if !self.finished.contains_key(&key) {
            self.open.push(guid.to_owned());
            self.depth = level;
            let read = sequence::read_sequence_at(graph, Some(guid), media, self, omissions);
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
            track_matte: None,
            effects: Vec::new(),
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

/// How far, in ticks, a nest's window may lie from the length that its speed
/// gives, as a media clip's source span may (`PrVideoOccurrence::validate`):
/// Adobe truncates the floating-point product to integer ticks.
const SPEED_SPAN_TOLERANCE_TICKS: f64 = 4.0;

/// Whether the window from In to Out of the checked `nest` is the one that
/// the native forward `speed` plays over its placement on the `outer`
/// sequence. Premiere counts a nest's speed in inner frames per outer frame, so
/// a mixed-rate nest saves Out − In = duration × speed × the inner frame
/// duration ÷ the outer frame duration. At one frame rate this is the rule of
/// a media clip's speed. The saved window, not the speed, then defines the
/// nest's clock, so the frame-rate ratio enters only this check.
fn speed_matches_window(nest: &PrNestOccurrence, speed: f64, outer: FrameRate) -> bool {
    let frame = |rate: FrameRate| rate.ticks_per_frame() as f64;
    let played =
        (nest.end_ticks - nest.start_ticks) as f64 * speed * frame(nest.sequence.frame_rate)
            / frame(outer);
    ((nest.out_ticks - nest.in_ticks) as f64 - played).abs() <= SPEED_SPAN_TOLERANCE_TICKS
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
) -> Result<NestPlacement> {
    let placement = read_placement(graph, &item, parent, omissions)?;
    let identity = &item.identity;
    ensure!(
        !placement.has_effects,
        "{identity}: effects on a nested sequence occurrence are not converted"
    );
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
    // A Track Matte Key keys the nest's canvas-sized picture, and a blend
    // mode blends it, which its group carries. So is its Motion, static or
    // keyed, as a clip's is its layer's, though not beside a Track Matte Key,
    // whose order against it no nest case measures. Every other edit omits
    // the nest.
    let reason = edits.iter().find_map(|&edit| match edit {
        OccurrenceEdit::TrackMatte => None,
        OccurrenceEdit::MotionKeys
        | OccurrenceEdit::Position
        | OccurrenceEdit::AnchorPoint
        | OccurrenceEdit::Scale
        | OccurrenceEdit::Rotation => placement.track_matte.is_some().then(|| {
            format!("{identity}: Motion with a Track Matte Key on a nested sequence occurrence is not converted")
        }),
        OccurrenceEdit::LinearWipe => Some(format!(
            "{identity}: Linear Wipe on a nested sequence occurrence is not converted"
        )),
        OccurrenceEdit::OpacityKeys => Some(format!(
            "{identity}: Opacity keyframes on a nested sequence occurrence are not converted"
        )),
        OccurrenceEdit::OpacityMask => Some(format!(
            "{identity}: an Opacity mask on a nested sequence occurrence is not converted"
        )),
        OccurrenceEdit::Opacity => Some(format!(
            "{identity}: nondefault Opacity on a nested sequence occurrence is not converted"
        )),
        OccurrenceEdit::Crop => Some(format!(
            "{identity}: Crop on a nested sequence occurrence is not converted"
        )),
        // A forward speed is checked against the window once the inner
        // frame rate is read.
        OccurrenceEdit::PlaybackRate => (placement.playback_rate < 0.0).then(|| {
            format!("{clip}: reverse playback of a nested sequence occurrence is not converted")
        }),
        OccurrenceEdit::TimeRemap => Some(format!(
            "{clip}: TimeRemapping on a nested sequence occurrence is not converted"
        )),
    });
    if let Some(reason) = reason {
        return Err(unsupported(reason));
    }
    read_sequence_source(graph, placement.source_record, omissions)?;
    let (key, read) = nesting.read(graph, guid, media, omissions)?;
    let id = item.identity;
    let nest = &mut read.nest;
    (nest.start_ticks, nest.end_ticks) = (placement.start, placement.end);
    (nest.in_ticks, nest.out_ticks) = (placement.source_in, placement.source_out);
    (nest.transform, nest.animations) = (placement.transform, placement.animations);
    // Before a later first key Premiere shows that key's value, as AME renders
    // effect parameters (Oracle run D). So a keyed property's static value is
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
            // A nest's Opacity keys omit it above.
            PrPropertyAnimation::Opacity(_) => {}
        }
    }
    nest.validate(parent.frame_rate, parent.dimensions, media)?;
    ensure!(
        speed_matches_window(nest, placement.playback_rate, parent.frame_rate),
        "{clip}: In {} to Out {} does not match PlaybackSpeed {} over the {}-tick placement of a nested sequence occurrence",
        nest.in_ticks,
        nest.out_ticks,
        placement.playback_rate,
        nest.end_ticks - nest.start_ticks
    );
    // Frame Blending or Optical Flow interpolates the retimed picture of the
    // whole nest; an FX group has no such field, and each inner video's own
    // blending would interpolate its media instead.
    if let Some(mode) = placement.frame_blending.filter(|_| nest.is_retimed()) {
        let mode = match mode {
            fx_schema::FrameBlendingMode::Simple => "Frame Blending",
            fx_schema::FrameBlendingMode::OpticalFlow => "Optical Flow",
        };
        omit(
            omissions,
            OmissionScope::Feature,
            clip,
            format!(
                "{mode} time interpolation of a retimed nested sequence occurrence is not converted; its group shows the nested frame at each mapped time"
            ),
        );
    }
    Ok(NestPlacement {
        id,
        timeline: placement.start..placement.end,
        source: placement.source_in..placement.source_out,
        enabled: placement.enabled && parent.track_output,
        transform: nest.transform,
        animations: std::mem::take(&mut nest.animations),
        track_matte: placement.track_matte,
        blend_mode: placement.blend_mode,
        read: key,
    })
}

/// Keeps the nested placements that overlap neither a kept media or graphic
/// item nor an earlier nested placement on the same track.
pub(super) fn keep_non_overlapping(
    mut nests: Vec<NestPlacement>,
    items: &[PrVideoItem],
    omissions: &mut Vec<Omission>,
) -> Vec<NestPlacement> {
    let overlap = |a: &Range<i64>, b: &Range<i64>| a.start < b.end && b.start < a.end;
    nests.sort_by_key(|nest| nest.timeline.start);
    let mut kept: Vec<NestPlacement> = Vec::with_capacity(nests.len());
    for nest in nests {
        if items
            .iter()
            .any(|item| overlap(&item.timeline_ticks(), &nest.timeline))
            || kept
                .iter()
                .any(|other| overlap(&other.timeline, &nest.timeline))
        {
            omit(
                omissions,
                OmissionScope::Occurrence,
                nest.id,
                "overlaps another occurrence on this track",
            );
        } else {
            kept.push(nest);
        }
    }
    kept
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
                start_ticks: placement.timeline.start,
                end_ticks: placement.timeline.end,
                in_ticks: placement.source.start,
                out_ticks: placement.source.end,
                transform: placement.transform,
                opacity: 100.0,
                blend_mode: placement.blend_mode,
                animations: placement.animations,
                crop: Default::default(),
                linear_wipe: None,
                track_matte: placement.track_matte,
                effects: Vec::new(),
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
        .read(graph, &sound.sequence, media, omissions)
        .map_err(reason)?;
    let shift = sound
        .timeline
        .start
        .checked_sub(sound.source.start)
        .ok_or_else(|| TICK_RANGE.to_owned())?;
    // At unit gain: the item's gain or keys reach each sound below.
    let mut played = Vec::new();
    played_sounds(&read.nest.sequence, &sound.source, shift, 1.0, &mut played).map_err(reason)?;
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
            None => heard.push(clip),
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

const TICK_RANGE: &str = "nested sequence audio item exceeds Premiere's tick range";
const GAIN_OVERFLOW: &str = "nested sequence audio gain overflows";
/// Why [`apply_level_keys`] leaves a keyed sound unchanged.
const LINEAR_PRODUCT: &str = "its Volume and the Level keys of this audio item both change over it, and one key track cannot hold their product because one of them changes its Level along a Linear segment";

/// Appends to `played` what an audio item that plays `sequence` over the
/// inner range `window` hears: each sound of `sequence` and of the nests
/// inside it that `window` shows, moved by `shift` ticks and multiplied by
/// `gain`. A nest's group carries only the sound of an item with its
/// picture's Enable ([`pair_sounds`]), so a hidden group's sound is a
/// disabled item's and plays at zero gain.
fn played_sounds(
    sequence: &PrSequence,
    window: &Range<i64>,
    shift: i64,
    gain: f64,
    played: &mut Vec<PrAudioOccurrence>,
) -> Result<()> {
    for clip in &sequence.audio {
        let range = clip.start_ticks..clip.end_ticks;
        if let Some((timeline, source)) = shown(range, clip.in_ticks, window, shift)? {
            let mut clip = clip.clone();
            (clip.start_ticks, clip.end_ticks) = (timeline.start, timeline.end);
            (clip.in_ticks, clip.out_ticks) = (source.start, source.end);
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
            played_sounds(&nest.sequence, &source, shift, gain, played)?;
        }
    }
    Ok(())
}

/// The part of a placement over `range` from `source_in` that the inner
/// range `window` shows: that part moved by `shift` ticks and its source
/// range, or `None` when `window` shows none of it.
fn shown(
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
    let offset = i128::from(shift) + i128::from(clip.in_ticks) - i128::from(clip.start_ticks);
    let keys = keys
        .keys
        .iter()
        .map(|key| {
            let source_ticks = i64::try_from(i128::from(key.source_ticks) + offset)
                .map_err(|_| unsupported(TICK_RANGE))?;
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
fn scale(clip: &mut PrAudioOccurrence, gain: f64) -> Result<()> {
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
) -> Result<()> {
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
    )?;
    Ok(())
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
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: 0,
            out_ticks: 4 * TICKS,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: keys,
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
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: 0,
            out_ticks: 4 * TICKS,
            volume: fx_schema::LinearGain::new(1.0).unwrap(),
            volume_keys: None,
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
            id: None,
            media: MediaId("timecoded".into()),
            out_ticks: source_in + timeline.end - timeline.start,
            start_ticks: timeline.start,
            end_ticks: timeline.end,
            in_ticks: source_in,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: None,
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
        played_sounds(&mid, &(at(1.5)..at(7.0)), at(8.5), 0.5, &mut played).unwrap();
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
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 10 * TICKS,
            end_ticks: 12 * TICKS,
            in_ticks: 0,
            out_ticks: 2 * TICKS,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: keys,
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
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 10 * TICKS,
            end_ticks: 12 * TICKS,
            in_ticks: 0,
            out_ticks: 2 * TICKS,
            volume: fx_schema::LinearGain::new(0.25).unwrap(),
            volume_keys: Some(PrVolumeKeys { keys, gain: 0.5 }),
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
        played_sounds(&inner, &top.source, 11 * TICKS, 1.0, &mut played).unwrap();
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
            id: None,
            media: MediaId("timecoded".into()),
            out_ticks: source_in + timeline.end - timeline.start,
            start_ticks: timeline.start,
            end_ticks: timeline.end,
            in_ticks: source_in,
            volume: fx_schema::LinearGain::new(volume).unwrap(),
            volume_keys: None,
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
        played_sounds(&mid, &item.source, shift, 1.0, &mut played).unwrap();
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
