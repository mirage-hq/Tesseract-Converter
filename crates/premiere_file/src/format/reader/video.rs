//! Video-track, occurrence, and media-reference parsing.

use super::{
    adjustment,
    animation::{chain_components, read_video_animations, read_video_compositing, MotionAndMasks},
    color_matte, effects, frame_dimensions, graphic, integer, require_zero_subclip_time_offset,
    required, required_integer, stream_dimensions,
    time_remap::{read_frame_hold, read_time_remapping},
    visibility,
};
use crate::error::{ensure, unsupported, BuildError, Result};
use crate::format::{
    graph::{self, Element},
    Graph, Located, Record,
};
use crate::schema::{
    caption, check_track_matte,
    native::{
        Clip, Markers, MasterClip, Media, Reference, SubClip, VideoClip, VideoClipTrack,
        VideoClipTrackItem, VideoComponentChain, VideoMediaSource, VideoStream, VideoTrackGroup,
        VideoTransitionTrackItem,
    },
    records::{self, MediaPathField},
    ColorSpace, FrameRate, MediaId, PrBlendMode, PrLinearWipe, PrMask, PrMedia,
    PrPropertyAnimation, PrStaticCrop, PrStaticTransform, PrTimeRemap, PrTrackMatte, PrVideoItem,
    PrVideoOccurrence, PrVideoStream, PrVideoTrack, PrVideoTransition, PrVideoTransitionKind,
    ToneMapSettings, VIDEO_MEDIA,
};
use crate::{omit, Omission, OmissionScope};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
};

pub(super) fn playback_rate(clip: &Clip, identity: &str) -> Result<f64> {
    let speed = match clip.playback_speed.as_deref() {
        Some(value) => value
            .parse::<f64>()
            .map_err(|_| unsupported(format!("{identity}: invalid PlaybackSpeed")))?,
        None => 1.0,
    };
    ensure!(
        speed.is_finite() && speed > 0.0,
        "{identity}: PlaybackSpeed must be finite and positive"
    );
    let backwards = match clip.play_backwards.as_deref() {
        Some("true") => true,
        Some("false") | None => false,
        Some(_) => return Err(unsupported(format!("{identity}: invalid PlayBackwards"))),
    };
    Ok(if backwards { -speed } else { speed })
}

fn frame_blending(
    clip: &VideoClip,
    identity: &str,
    omissions: &mut Vec<Omission>,
) -> Option<fx_schema::FrameBlendingMode> {
    use fx_schema::FrameBlendingMode;

    match clip.time_interpolation_type.as_deref() {
        // Premiere CS6 to CC 2015 save Frame Blending as the `FrameBlend` flag;
        // `legacy_clip_settings` has already rejected any value but true/false.
        None if clip.frame_blend.as_deref() == Some("true") => Some(FrameBlendingMode::Simple),
        None | Some("0") => None,
        Some("1") => Some(FrameBlendingMode::Simple),
        Some("2") => Some(FrameBlendingMode::OpticalFlow),
        Some(value) => {
            omit(
                omissions,
                OmissionScope::Feature,
                identity,
                format!("unsupported TimeInterpolationType {value:?}; using frame sampling"),
            );
            None
        }
    }
}

/// Whether the placement's `clip` has Scale to Frame Size on:
/// `<ScaleToFramePolicy>1</ScaleToFramePolicy>`, the form of all six corpus
/// clips that use it; absent is off. Premiere then draws the source fitted
/// inside the sequence frame at Motion Scale 100 (measured on a still in
/// `premiere_isolated_images_nests_26_5`). No host converts it, so each
/// rejects `true` with its own reason. Premiere CS6 to CC 2015 write the off
/// state as policy `0` (360 corpus clips) or as `ScaleToFrameSize`; other
/// values are unobserved and reject.
pub(super) fn scale_to_frame_size(clip: &VideoClip, identity: &str) -> Result<bool> {
    legacy_clip_settings(clip, identity)?;
    match (
        clip.scale_to_frame_policy.as_deref(),
        clip.scale_to_frame_size.as_deref(),
    ) {
        (None | Some("0"), None | Some("false")) => Ok(false),
        (Some("1"), None) | (None, Some("true")) => Ok(true),
        (Some(policy), _) => Err(unsupported(format!(
            "{identity}: unknown Scale to Frame Size policy {policy:?}"
        ))),
        (None, Some(legacy)) => Err(unsupported(format!(
            "{identity}: invalid ScaleToFrameSize {legacy:?}"
        ))),
    }
}

/// Legacy interlace options are accepted only in the inert form that every
/// corpus clip saves; the converter has no field processing to map them to.
/// `FrameBlend` must be a boolean so that `frame_blending` reads it exactly.
fn legacy_clip_settings(clip: &VideoClip, identity: &str) -> Result<()> {
    for (name, value, inert) in [
        ("FieldProcessing", &clip.field_processing, "0"),
        ("HoldFilters", &clip.hold_filters, "false"),
        ("DeinterlaceOnHold", &clip.deinterlace_on_hold, "false"),
        (
            "ReverseFieldDominance",
            &clip.reverse_field_dominance,
            "false",
        ),
    ] {
        if let Some(value) = value.as_deref().filter(|value| *value != inert) {
            return Err(unsupported(format!(
                "{identity}: {name} {value:?} is not converted"
            )));
        }
    }
    ensure!(
        matches!(clip.frame_blend.as_deref(), None | Some("true" | "false")),
        "{identity}: invalid FrameBlend {:?}",
        clip.frame_blend.as_deref().unwrap_or_default()
    );
    Ok(())
}

pub(super) fn report_markers(
    graph: &Graph<'_>,
    clip: &Clip,
    identity: &str,
    omissions: &mut Vec<Omission>,
) {
    if let Some(owner) = &clip.marker_owner {
        if let Some(reference) = &owner.markers {
            if let Err(error) = graph.follow::<Markers>(reference, identity) {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    identity,
                    format!("clip markers not converted: {error}"),
                );
            }
        } else {
            omit(
                omissions,
                OmissionScope::Feature,
                identity,
                "clip marker owner has no markers reference",
            );
        }
    }
}

pub(super) fn report_unknown_children(
    element: Element<'_>,
    allowed: &[&str],
    identity: &str,
    path: &str,
    omissions: &mut Vec<Omission>,
) {
    for attribute in element.attributes().filter(|name| {
        ![
            "Version",
            "ClassID",
            "ObjectID",
            "ObjectUID",
            "ObjectRef",
            "ObjectURef",
            "Index",
            "nil",
        ]
        .contains(name)
    }) {
        omit(
            omissions,
            OmissionScope::Feature,
            identity,
            format!("{path}@{attribute} not converted"),
        );
    }
    if element.text().is_some_and(|text| !text.trim().is_empty()) {
        omit(
            omissions,
            OmissionScope::Feature,
            identity,
            format!("{path}$text not converted"),
        );
    }
    for child in element
        .children()
        .filter(|child| !allowed.contains(&child.tag()))
    {
        omit(
            omissions,
            OmissionScope::Feature,
            identity,
            format!("{path}{} not converted", child.tag()),
        );
    }
}

fn report_track_structure(record: Record<'_>, omissions: &mut Vec<Omission>) -> Result<()> {
    let identity = record.identity();
    let root = record.element();
    report_unknown_children(root, &["ClipTrack"], &identity, "", omissions);
    if let Some(clip_track) = root.child("ClipTrack") {
        report_unknown_children(
            clip_track,
            &["Track", "ClipItems", "TransitionItems"],
            &identity,
            "ClipTrack/",
            omissions,
        );
        if let Some(track) = clip_track.child("Track") {
            report_unknown_children(
                track,
                &["Node", "ID", "MediaType", "Index", "IsMuted", "Name"],
                &identity,
                "ClipTrack/Track/",
                omissions,
            );
        }
        if let Some(items) = clip_track.child("ClipItems") {
            report_unknown_children(
                items,
                &["MediaType", "Index", "TrackItems"],
                &identity,
                "ClipTrack/ClipItems/",
                omissions,
            );
        }
        if let Some(transitions) = clip_track.child("TransitionItems") {
            report_unknown_children(
                transitions,
                &["TrackItems", "MediaType", "Index"],
                &identity,
                "ClipTrack/TransitionItems/",
                omissions,
            );
        }
    }
    Ok(())
}

fn report_occurrence_structure(record: Record<'_>, omissions: &mut Vec<Omission>) -> Result<()> {
    let identity = record.identity();
    let root = record.element();
    report_unknown_children(
        root,
        &[
            "ClipTrackItem",
            "PixelAspectRatio",
            "ToneMapSettings",
            "FrameRect",
        ],
        &identity,
        "",
        omissions,
    );
    if let Some(item) = root.child("ClipTrackItem") {
        report_unknown_children(
            item,
            &[
                "ComponentOwner",
                "TrackItem",
                "SubClip",
                "HeadTransition",
                "TailTransition",
                "IsMuted",
            ],
            &identity,
            "ClipTrackItem/",
            omissions,
        );
        if let Some(owner) = item.child("ComponentOwner") {
            report_unknown_children(
                owner,
                &["Components"],
                &identity,
                "ClipTrackItem/ComponentOwner/",
                omissions,
            );
        }
        if let Some(range) = item.child("TrackItem") {
            // The legacy Version 3 fields repeat the item class and the track
            // structure (`TrackItemRange`), so they lose nothing.
            report_unknown_children(
                range,
                &[
                    "Start",
                    "End",
                    "Type",
                    "MediaType",
                    "TrackIndex",
                    "TrackRefCount",
                ],
                &identity,
                "ClipTrackItem/TrackItem/",
                omissions,
            );
        }
    }
    Ok(())
}

pub(super) fn default_chain(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    omissions: &mut Vec<Omission>,
) {
    let record = match graph.locate(reference, from) {
        Ok(record) => record,
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Feature,
                from,
                format!("component chain not converted: {error}"),
            );
            return;
        }
    };
    let identity = record.identity();
    report_unknown_children(
        record.element(),
        &[
            "DefaultMotion",
            "DefaultOpacity",
            "DefaultMotionComponentID",
            "DefaultOpacityComponentID",
            "ComponentChain",
        ],
        &identity,
        "",
        omissions,
    );
    if let Some(component_chain) = record.element().child("ComponentChain") {
        report_unknown_children(
            component_chain,
            &["Node", "Components"],
            &identity,
            "ComponentChain/",
            omissions,
        );
    }
    let chain = match graph.decode_as::<VideoComponentChain>(record, from) {
        Ok(chain) => chain,
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Feature,
                from,
                format!("component chain not converted: {error}"),
            );
            return;
        }
    };
    for (name, value) in [
        ("DefaultMotion", chain.value.default_motion.as_deref()),
        ("DefaultOpacity", chain.value.default_opacity.as_deref()),
    ] {
        if let Some(value) = value {
            if value != "true" {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    &chain.identity,
                    format!("nondefault {name} not converted"),
                );
            }
        }
    }
    if chain.value.component_chain.is_none() {
        omit(
            omissions,
            OmissionScope::Feature,
            &chain.identity,
            "missing ComponentChain; effects not converted",
        );
    }
}

/// Reads the video track group: its frame rate, its canvas size in pixels and
/// its tracks, bottom first.
pub(super) fn read_tracks(
    graph: &Graph<'_>,
    video: Located<VideoTrackGroup>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    nesting: &mut super::nested::Nesting<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<(FrameRate, [u32; 2], Vec<PrVideoTrack>)> {
    let frame_rect = required(
        video.value.frame_rect.as_deref(),
        &video.identity,
        "FrameRect",
    )?;
    let [width, height] = frame_dimensions(frame_rect, &video.identity)?;
    if let Some(pixel_aspect_ratio) = &video.value.pixel_aspect_ratio {
        let ratio = records::PixelAspectRatio::parse(pixel_aspect_ratio, &video.identity)?;
        ensure!(
            ratio.is_square(),
            "non-square sequence pixels ({ratio}) are unsupported"
        );
    }
    for (name, actual, expected) in [
        (
            "ColorManagementSettings",
            video.value.color_management_settings.as_deref(),
            records::COLOR_MANAGEMENT_SETTINGS,
        ),
        (
            "ImmersiveVideoVRConfiguration",
            video.value.immersive_video_vr_configuration.as_deref(),
            records::IMMERSIVE_VIDEO_VR_CONFIGURATION,
        ),
        (
            "AutoInputGamutCompressionEnabled",
            video.value.auto_input_gamut_compression_enabled.as_deref(),
            "true",
        ),
        (
            "IsGraphicsWhiteSameAsProject",
            video.value.is_graphics_white_same_as_project.as_deref(),
            "false",
        ),
        (
            "IsColorAwareEffectsEnabledSameAsProject",
            video
                .value
                .is_color_aware_effects_enabled_same_as_project
                .as_deref(),
            "false",
        ),
    ] {
        if actual.is_some_and(|actual| actual != expected) {
            omit(
                omissions,
                OmissionScope::Feature,
                &video.identity,
                format!("nondefault {name} not converted"),
            );
        }
    }
    if let Some(components) = video
        .value
        .component_owner
        .as_ref()
        .and_then(|owner| owner.components.as_ref())
    {
        default_chain(graph, components, &video.identity, omissions);
        let chain = graph.follow::<VideoComponentChain>(components, &video.identity)?;
        let components: Vec<_> = chain_components(&chain)?.iter().collect();
        let MotionAndMasks {
            crop,
            linear_wipe,
            track_matte,
            ..
        } = read_video_animations(graph, &chain, &components, false)?;
        ensure!(
            crop.is_default() && linear_wipe.is_none() && track_matte.is_none(),
            "{}: sequence-level Crop, Linear Wipe or Track Matte Key is unsupported",
            video.identity
        );
    } else {
        omit(
            omissions,
            OmissionScope::Feature,
            &video.identity,
            "missing component chain; effects not converted",
        );
    }

    let track_group = required(
        video.value.track_group.as_ref(),
        &video.identity,
        "TrackGroup",
    )?;
    let frame_rate_ticks = required_integer(
        track_group.frame_rate.as_deref(),
        &video.identity,
        "FrameRate",
    )?;
    let frame_rate = super::frame_rate(frame_rate_ticks, &video.identity)?;

    let mut video_tracks = Vec::new();
    let mut seen_clips = BTreeSet::new();
    let mut seen_indices = BTreeSet::new();
    if let Some(tracks) = &track_group.tracks {
        for reference in &tracks.tracks {
            let record = match graph.locate(reference, &video.identity) {
                Ok(record) => record,
                Err(error) => {
                    omit(
                        omissions,
                        OmissionScope::Track,
                        &video.identity,
                        error.to_string(),
                    );
                    continue;
                }
            };
            let track_identity = record.identity();
            match read_track(
                graph,
                record,
                reference,
                &video.identity,
                &seen_indices,
                omissions,
            ) {
                Ok(track) => {
                    seen_indices.insert(track.index);
                    video_tracks.push(track);
                }
                Err(error) => omit(
                    omissions,
                    OmissionScope::Track,
                    track_identity,
                    error.to_string(),
                ),
            }
        }
    }
    video_tracks.sort_by_key(|track| track.index);
    // A Track Matte Key names its matte track by the persistent `Track/ID`;
    // an ID that two tracks carry names neither. The ID resolves to the
    // track's position among the kept tracks, which `parsed_tracks` and
    // `PrTrackMatte::track_index` count: an omitted track leaves no gap
    // there, so the native `Index` would name the track above the matte.
    let mut track_ids: BTreeMap<usize, Option<usize>> = BTreeMap::new();
    for (position, track) in video_tracks.iter().enumerate() {
        if let Some(id) = track.id {
            track_ids
                .entry(id)
                .and_modify(|resolved| *resolved = None)
                .or_insert(Some(position));
        }
    }
    let mut parsed_tracks = Vec::with_capacity(video_tracks.len());
    // Nested placements wait here, uncopied, until every direct item is known.
    let mut track_nests = Vec::with_capacity(video_tracks.len());
    let mut claims = Vec::new();
    let mut seen_transitions = BTreeSet::new();
    for NativeTrack {
        index: track_index,
        output_enabled,
        clips,
        transitions: transition_references,
        ..
    } in video_tracks
    {
        let parent = super::nested::Parent {
            frame_rate,
            dimensions: [width, height],
            track_output: output_enabled,
            track_index,
            track_ids: &track_ids,
        };
        let mut items = Vec::with_capacity(clips.len());
        let mut nests = Vec::new();
        let mut transition_links = Vec::with_capacity(clips.len());
        for reference in clips {
            let identity = reference
                .id
                .as_deref()
                .or(reference.uid.as_deref())
                .unwrap_or("unidentified occurrence")
                .to_owned();
            let decoded = (|| -> Result<(
                Located<VideoClipTrackItem>,
                Option<String>,
                TransitionClipLink,
            )> {
                let record = graph.locate(&reference, &video.identity)?;
                report_occurrence_structure(record, omissions)?;
                let item = graph.decode_as::<VideoClipTrackItem>(record, &video.identity)?;
                ensure!(
                    seen_clips.insert(item.identity.clone()),
                    "{}: duplicate video occurrence reference",
                    item.identity
                );
                let clip_item = required(
                    item.value.clip_track_item.as_ref(),
                    &item.identity,
                    "ClipTrackItem",
                )?;
                let range = required(clip_item.track_item.as_ref(), &item.identity, "TrackItem")?;
                let start_ticks = match range.start.as_deref() {
                    Some(value) => integer(value, &format!("{}: invalid Start", item.identity))?,
                    None => 0,
                };
                let end_ticks = integer(&range.end, &format!("{}: invalid End", item.identity))?;
                let link = TransitionClipLink {
                    id: item.identity.clone(),
                    start_ticks,
                    end_ticks,
                    head_transition: transition_identity(
                        graph,
                        clip_item.head_transition.as_ref(),
                        &item.identity,
                    )?,
                    tail_transition: transition_identity(
                        graph,
                        clip_item.tail_transition.as_ref(),
                        &item.identity,
                    )?,
                    occurrence: None,
                };
                // A broken link here is reported by the media reader.
                let nested = graph::nested_sequence(graph, record).ok().flatten();
                Ok((item, nested, link))
            })();
            let (item, nested, mut link) = match decoded {
                Ok(decoded) => decoded,
                Err(error) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        identity,
                        error.to_string(),
                    );
                    continue;
                }
            };
            // Whatever becomes of the placement, an active key consumes its
            // matte track over the placement's range.
            claims.extend(claimed_matte_tracks(graph, &item, &parent).into_iter().map(
                |matte_track| MatteClaim {
                    keyed: item.identity.clone(),
                    track: parsed_tracks.len(),
                    range: link.start_ticks..link.end_ticks,
                    matte_track,
                },
            ));
            // A nest or graphic has no source media, so its transition link keeps
            // no occurrence.
            if let Some(guid) = nested {
                match super::nested::read_nest(
                    graph, item, &guid, &parent, media, nesting, omissions,
                ) {
                    Ok(nest) => nests.push(nest),
                    Err(error) => omit(
                        omissions,
                        OmissionScope::Occurrence,
                        identity,
                        error.to_string(),
                    ),
                }
                transition_links.push(link);
                continue;
            }
            // An adjustment layer plays generator media like a graphic; its
            // flag decides before the graphic reader claims the media.
            let converted = match graphic::graphic_clip(graph, &item)
                .filter(|_| !adjustment::is_flagged(graph, &item))
            {
                Some(source) => graphic::read_graphic(
                    graph,
                    item,
                    source,
                    [width, height],
                    frame_rate,
                    omissions,
                )
                .map(PrVideoItem::Graphic),
                None => read_occurrence(graph, &video, item, media, frame_rate, &parent, omissions)
                    .map(PrVideoItem::Media),
            };
            // A kept media occurrence also links its source window.
            let kept = match converted {
                Ok(PrVideoItem::Media(mut occurrence)) => {
                    occurrence.enabled &= output_enabled;
                    let stream = media[&occurrence.media]
                        .video
                        .as_ref()
                        .expect("video occurrence has a picture stream");
                    let source = LinkedSource {
                        in_ticks: occurrence.in_ticks,
                        out_ticks: occurrence.out_ticks,
                        intrinsic_ticks: stream.intrinsic_ticks,
                    };
                    (super::still::keep_occurrence(
                        &occurrence,
                        stream.kind,
                        track_index,
                        omissions,
                    ) && keep_mask_boundary(
                        &occurrence,
                        [stream.width, stream.height],
                        [width, height],
                        track_index,
                        omissions,
                    ))
                    .then_some((PrVideoItem::Media(occurrence), Some(source)))
                }
                Ok(PrVideoItem::Graphic(mut graphic)) => {
                    graphic.enabled &= output_enabled;
                    Some((PrVideoItem::Graphic(graphic), None))
                }
                Err(error) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        &identity,
                        error.to_string(),
                    );
                    None
                }
            };
            if let Some((item, source)) = kept {
                link.occurrence = source;
                items.push(item);
            }
            transition_links.push(link);
        }
        items.sort_by_key(|item| item.timeline_ticks().start);
        let mut kept: Vec<PrVideoItem> = Vec::new();
        for item in items {
            if kept
                .last()
                .is_some_and(|last| last.timeline_ticks().end > item.timeline_ticks().start)
            {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    item.id().unwrap_or_default(),
                    "overlaps another occurrence on this track",
                );
            } else {
                kept.push(item);
            }
        }
        let mut transition_memberships = BTreeSet::new();
        for reference in &transition_references {
            let identity = reference
                .id
                .as_deref()
                .or(reference.uid.as_deref())
                .unwrap_or("unidentified transition");
            match graph.locate(reference, &video.identity) {
                Ok(record) => {
                    transition_memberships.insert(record.identity());
                }
                Err(error) => omit(
                    omissions,
                    OmissionScope::Feature,
                    identity,
                    error.to_string(),
                ),
            }
        }
        let mut unlisted_transitions = BTreeSet::new();
        for identity in transition_links.iter().flat_map(|clip| {
            [
                clip.head_transition.as_deref(),
                clip.tail_transition.as_deref(),
            ]
            .into_iter()
            .flatten()
        }) {
            if !transition_memberships.contains(identity)
                && unlisted_transitions.insert(identity.to_owned())
            {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    identity,
                    "clip-linked video transition is missing from TransitionItems; transition not converted",
                );
            }
        }

        let mut transitions = Vec::with_capacity(transition_references.len());
        for reference in transition_references {
            let identity = reference
                .id
                .as_deref()
                .or(reference.uid.as_deref())
                .unwrap_or("unidentified transition")
                .to_owned();
            match read_transition(graph, &reference, &video.identity, &transition_links) {
                Ok(transition) => {
                    ensure!(
                        seen_transitions.insert(transition.id.clone()),
                        "duplicate video transition reference"
                    );
                    transitions.push(transition);
                }
                Err(error) => omit(
                    omissions,
                    OmissionScope::Feature,
                    identity,
                    error.to_string(),
                ),
            }
        }
        track_nests.push(super::nested::keep_non_overlapping(nests, &kept, omissions));
        parsed_tracks.push(PrVideoTrack {
            transitions,
            nests: Vec::new(),
            items: kept,
        });
    }
    super::nested::copy_placements(&mut parsed_tracks, track_nests, nesting);
    resolve_track_mattes(graph, &mut parsed_tracks, [width, height], omissions);
    consume_claimed_mattes(&mut parsed_tracks, &claims, omissions);
    Ok((frame_rate, [width, height], parsed_tracks))
}

/// The matte track that an active Track Matte Key of one placement names,
/// over the placement's timeline range: Premiere does not draw that track's
/// clips there, whether or not the placement converts (fixture G1b).
struct MatteClaim {
    /// The keyed placement's record.
    keyed: String,
    /// The keyed placement's kept-track position.
    track: usize,
    range: Range<i64>,
    /// The matte track's kept-track position.
    matte_track: usize,
}

/// The kept-track positions that the active Track Matte Keys of `item`'s
/// chain name ([`effects::claimed_matte_track_ids`]). A chain or a matte track
/// ID that cannot be read names none: the placement's own read then omits the
/// placement with that reason.
fn claimed_matte_tracks(
    graph: &Graph<'_>,
    item: &Located<VideoClipTrackItem>,
    parent: &super::nested::Parent<'_>,
) -> Vec<usize> {
    let Some(components) = item
        .value
        .clip_track_item
        .as_ref()
        .and_then(|clip| clip.component_owner.as_ref())
        .and_then(|owner| owner.components.as_ref())
    else {
        return Vec::new();
    };
    let Ok(chain) = graph.follow::<VideoComponentChain>(components, &item.identity) else {
        return Vec::new();
    };
    let Ok(components) = chain_components(&chain) else {
        return Vec::new();
    };
    effects::claimed_matte_track_ids(graph, components, &chain.identity)
        .into_iter()
        .filter_map(|id| parent.matte_track_index(id, &item.identity).ok())
        .collect()
}

/// Drops each matte clip over the range of a keyed placement that was not
/// kept, unless a kept placement keys it, recording the omitted placement:
/// Premiere does not draw a matte clip while an active key names its track
/// (fixture G1b), and FX hides a matte source only through the layer that
/// consumes it. Claims are in kept-track order, so a matte clip dropped here
/// that keys its own matte drops that one in turn.
fn consume_claimed_mattes(
    tracks: &mut [PrVideoTrack],
    claims: &[MatteClaim],
    omissions: &mut Vec<Omission>,
) {
    let placed = |tracks: &[PrVideoTrack], track: usize, range: &Range<i64>| {
        tracks.get(track).is_some_and(|track| {
            track
                .items
                .iter()
                .any(|item| item.timeline_ticks() == *range)
                || track
                    .nests
                    .iter()
                    .any(|nest| nest.timeline_ticks() == *range)
        })
    };
    let overlaps = |a: &Range<i64>, b: &Range<i64>| a.start < b.end && b.start < a.end;
    for claim in claims {
        if placed(tracks, claim.track, &claim.range) {
            continue;
        }
        let Some(matte_track) = tracks.get(claim.matte_track) else {
            continue;
        };
        // A kept placement's key names its matte clip over exactly its range.
        let consumed: Vec<Range<i64>> = tracks[..claim.matte_track]
            .iter()
            .flat_map(|track| {
                track
                    .items
                    .iter()
                    .filter_map(PrVideoItem::media)
                    .filter_map(|clip| Some((clip.track_matte?, clip.timeline_ticks())))
                    .chain(
                        track
                            .nests
                            .iter()
                            .filter_map(|nest| Some((nest.track_matte?, nest.timeline_ticks()))),
                    )
            })
            .filter(|(matte, _)| matte.track_index == claim.matte_track)
            .map(|(_, range)| range)
            .collect();
        let dropped: Vec<(String, Range<i64>)> = matte_track
            .items
            .iter()
            .map(|item| {
                (
                    item.id().unwrap_or_default().to_owned(),
                    item.timeline_ticks(),
                )
            })
            .chain(
                matte_track
                    .nests
                    .iter()
                    .map(|nest| (nest.record(), nest.timeline_ticks())),
            )
            .filter(|(_, range)| overlaps(range, &claim.range) && !consumed.contains(range))
            .collect();
        let matte_track = &mut tracks[claim.matte_track];
        for (record, range) in dropped {
            matte_track
                .items
                .retain(|item| item.timeline_ticks() != range);
            matte_track
                .nests
                .retain(|nest| nest.timeline_ticks() != range);
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                format!(
                    "matte source of the omitted clip {} was not converted: Premiere does not draw a track-matte source",
                    claim.keyed
                ),
            );
        }
    }
}

/// Why a keyed placement that is disabled or on a muted track is omitted: FX
/// draws the track matte source of a hidden layer as content, and whether
/// Premiere draws the matte clip of a disabled keyed clip is unmeasured
/// (fixture G1b measured an enabled one), so the placement goes and its matte
/// clips over its range with it (`consume_claimed_mattes`).
const DISABLED_KEYED_PLACEMENT: &str = "a Track Matte Key on a disabled clip or on a muted track is not converted: whether Premiere draws its matte clip is unmeasured, and FX draws a hidden clip's matte source as content";

/// Drops each media or nest placement whose Track Matte Key names no matte
/// that converts ([`check_track_matte`]), or that is disabled or on a muted
/// track ([`DISABLED_KEYED_PLACEMENT`]), recording why. Runs once every
/// item and nest of every track is kept, from the top track down, because a
/// matte lies on a track above its clip.
fn resolve_track_mattes(
    graph: &Graph<'_>,
    tracks: &mut [PrVideoTrack],
    canvas: [u32; 2],
    omissions: &mut Vec<Omission>,
) {
    for index in (0..tracks.len()).rev() {
        let track = &tracks[index];
        let keyed = track
            .items
            .iter()
            .filter_map(PrVideoItem::media)
            .filter_map(|clip| {
                let matte = clip.track_matte?;
                let measured_transform = clip
                    .transform_stage(canvas, canvas)
                    .ok()
                    .flatten()
                    .filter(|(_, _, owner)| *owner == crate::schema::TransformOwner::KeyedPicture)
                    .map(|(effect, _, _)| effect);
                Some((
                    clip.id.clone(),
                    clip.timeline_ticks(),
                    clip.enabled,
                    matte,
                    measured_transform,
                ))
            })
            .chain(track.nests.iter().filter_map(|nest| {
                let matte = nest.track_matte?;
                Some((
                    nest.id.clone(),
                    nest.timeline_ticks(),
                    nest.enabled,
                    matte,
                    None,
                ))
            }));
        let rejected: Vec<_> = keyed
            .filter_map(|(id, range, enabled, matte, measured_transform)| {
                let (reason, transform) = if enabled {
                    match check_track_matte(tracks, index, range.clone(), matte) {
                        Err(reason) => (reason, None),
                        Ok(()) if measured_transform.is_some() && native_matte_has_effects(graph, tracks, matte, &range).unwrap_or(true) => (
                            "Transform with Track Matte Key requires a static matte with no active native effects, including effects without a mapping (measured A4)".to_owned(),
                            measured_transform,
                        ),
                        Ok(()) => return None,
                    }
                } else {
                    (DISABLED_KEYED_PLACEMENT.to_owned(), None)
                };
                Some((id.unwrap_or_default(), range, reason, transform))
            })
            .collect();
        let track = &mut tracks[index];
        for (record, range, reason, transform) in rejected {
            if let Some(effect) = transform {
                let clip = track
                    .items
                    .iter_mut()
                    .find_map(|item| match item {
                        PrVideoItem::Media(clip) if clip.timeline_ticks() == range => Some(clip),
                        _ => None,
                    })
                    .expect("identified keyed media occurrence remains on its track");
                clip.remove_effect(effect);
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!("{reason}; Transform omitted, existing Track Matte Key retained"),
                );
                continue;
            }
            // A track holds one placement per range.
            track.items.retain(|item| item.timeline_ticks() != range);
            track.nests.retain(|nest| nest.timeline_ticks() != range);
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                format!(
                    "track {index}, range {}..{} ticks: {reason}; occurrence omitted",
                    range.start, range.end
                ),
            );
        }
    }
}

/// Inspect the matte's native stack before unsupported effects are dropped.
fn native_matte_has_effects(
    graph: &Graph<'_>,
    tracks: &[PrVideoTrack],
    matte: PrTrackMatte,
    range: &Range<i64>,
) -> Result<bool> {
    let source = tracks[matte.track_index]
        .items
        .iter()
        .filter_map(PrVideoItem::media)
        .find(|source| source.timeline_ticks() == *range)
        .ok_or_else(|| unsupported("A4 requires a media matte"))?;
    let record = graph
        .records()
        .find(|record| Some(record.identity().as_str()) == source.id.as_deref())
        .ok_or_else(|| unsupported("A4 matte has no native record"))?;
    let item = graph.decode_as::<VideoClipTrackItem>(record, "A4 matte")?;
    let components = item
        .value
        .clip_track_item
        .as_ref()
        .and_then(|item| item.component_owner.as_ref())
        .and_then(|owner| owner.components.as_ref())
        .ok_or_else(|| unsupported("A4 matte has no component chain"))?;
    let chain = graph.follow::<VideoComponentChain>(components, &item.identity)?;
    Ok(
        effects::split_chain(graph, chain_components(&chain)?, &chain.identity)?
            .has_active_standard_effects(),
    )
}

/// Whether an occurrence's Crop, Linear Wipe or Track Matte Key converts
/// ([`PrVideoOccurrence::mask_boundary`]), recording why not.
fn keep_mask_boundary(
    clip: &PrVideoOccurrence,
    source: [u32; 2],
    canvas: [u32; 2],
    track_index: i64,
    omissions: &mut Vec<Omission>,
) -> bool {
    let Err(reason) = clip.mask_boundary(source, canvas) else {
        return true;
    };
    omit(
        omissions,
        OmissionScope::Occurrence,
        clip.id.clone().unwrap_or_default(),
        format!(
            "track {track_index}, range {}..{} ticks: {reason}; occurrence omitted",
            clip.start_ticks, clip.end_ticks
        ),
    );
    false
}

struct TransitionClipLink {
    id: String,
    start_ticks: i64,
    end_ticks: i64,
    head_transition: Option<String>,
    tail_transition: Option<String>,
    occurrence: Option<LinkedSource>,
}

/// The source window of a kept media occurrence that a transition checks for
/// handles; the occurrence itself stays on its track.
#[derive(Debug, Clone, Copy)]
struct LinkedSource {
    in_ticks: i64,
    out_ticks: i64,
    /// The source media duration.
    intrinsic_ticks: i64,
}

fn transition_identity(
    graph: &Graph<'_>,
    reference: Option<&Reference>,
    owner: &str,
) -> Result<Option<String>> {
    Ok(reference
        .map(|reference| {
            graph
                .locate(reference, owner)
                .map(|record| record.identity())
        })
        .transpose()?)
}

pub(super) fn native_bool(value: &str, identity: &str, field: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(unsupported(format!("{identity}: invalid {field}"))),
    }
}

fn read_transition(
    graph: &Graph<'_>,
    reference: &Reference,
    owner: &str,
    clips: &[TransitionClipLink],
) -> Result<PrVideoTransition> {
    let record = graph.locate(reference, owner)?;
    let transition = graph.decode_as::<VideoTransitionTrackItem>(record, owner)?;
    let item = required(
        transition.value.transition_track_item.as_ref(),
        &transition.identity,
        "TransitionTrackItem",
    )?;
    let range = required(item.track_item.as_ref(), &transition.identity, "TrackItem")?;
    let start = required_integer(range.start.as_deref(), &transition.identity, "Start")?;
    let end = integer(&range.end, &format!("{}: invalid End", transition.identity))?;
    ensure!(
        start < end,
        "{}: empty video transition",
        transition.identity
    );
    let duration = end.checked_sub(start).ok_or_else(|| {
        unsupported(format!(
            "{}: video transition duration overflow",
            transition.identity
        ))
    })?;
    let alignment = required_integer(item.alignment.as_deref(), &transition.identity, "Alignment")?;
    ensure!(
        (0..=duration).contains(&alignment),
        "{}: transition Alignment lies outside its range",
        transition.identity
    );
    let cut = start.checked_add(alignment).ok_or_else(|| {
        unsupported(format!(
            "{}: video transition cut overflow",
            transition.identity
        ))
    })?;
    let has_outgoing_clip = native_bool(
        required(
            item.has_outgoing_clip.as_deref(),
            &transition.identity,
            "HasOutgoingClip",
        )?,
        &transition.identity,
        "HasOutgoingClip",
    )?;
    let has_incoming_clip = native_bool(
        required(
            item.has_incoming_clip.as_deref(),
            &transition.identity,
            "HasIncomingClip",
        )?,
        &transition.identity,
        "HasIncomingClip",
    )?;
    ensure!(
        has_outgoing_clip || has_incoming_clip,
        "{}: transition has no adjacent clip",
        transition.identity
    );
    let kind = match item.match_name.as_deref() {
        Some("AE.ADBE Cross Dissolve New") => PrVideoTransitionKind::CrossDissolve,
        Some(name @ ("AE.AE_Impact_Dissolve" | "AE.AE_Impact_Pop")) => {
            super::effects::only_children(
                record.element(),
                &["TransitionTrackItem", "VideoFilterComponent"],
                "Film Impact transition/",
            )?;
            let reference = required(
                transition.value.video_filter_component.as_ref(),
                &transition.identity,
                "VideoFilterComponent",
            )?;
            let component = graph.locate(reference, &transition.identity)?;
            if name == "AE.AE_Impact_Pop" {
                super::pop::read_profile(graph, component)?;
            } else {
                super::dissolve::read_profile(graph, component)?;
            }
            ensure!(
                transition.value.start_percent.is_none()
                    && transition.value.end_percent.is_none()
                    && transition.value.switch_sources.is_none()
                    && transition.value.reverse.is_none(),
                "Film Impact dissolve has conflicting built-in controls"
            );
            if name == "AE.AE_Impact_Pop" {
                PrVideoTransitionKind::FilmImpactPop
            } else {
                PrVideoTransitionKind::FilmImpactDissolve
            }
        }
        Some(name) => {
            return Err(unsupported(format!(
                "{} ({name}) video transition not converted",
                item.display_name.as_deref().unwrap_or("unnamed")
            )));
        }
        None => return Err(unsupported("video transition has no MatchName")),
    };
    if kind == PrVideoTransitionKind::CrossDissolve {
        // A Version 6 record omits these controls at their defaults, while a
        // Version 5 record saves all four.
        let defaults_omitted = record.element().attribute("Version") == Some("6");
        let percent = |value: Option<&str>, field: &str, expected: f64| -> Result<()> {
            if value.is_none() && defaults_omitted {
                return Ok(());
            }
            let value = required(value, &transition.identity, field)?;
            ensure!(
                value.parse::<f64>().ok() == Some(expected),
                "{}: nondefault {field} is unsupported",
                transition.identity
            );
            Ok(())
        };
        let flag = |value: Option<&str>, field: &str| -> Result<bool> {
            match value {
                None if defaults_omitted => Ok(false),
                value => native_bool(
                    required(value, &transition.identity, field)?,
                    &transition.identity,
                    field,
                ),
            }
        };
        percent(
            transition.value.start_percent.as_deref(),
            "StartPercent",
            0.0,
        )?;
        percent(transition.value.end_percent.as_deref(), "EndPercent", 1.0)?;
        ensure!(
            !flag(transition.value.switch_sources.as_deref(), "SwitchSources")?
                && !flag(transition.value.reverse.as_deref(), "Reverse")?,
            "{}: reversed/switched transition is unsupported",
            transition.identity
        );
    }

    let outgoing: Vec<_> = clips
        .iter()
        .filter(|clip| clip.tail_transition.as_deref() == Some(&transition.identity))
        .collect();
    let incoming: Vec<_> = clips
        .iter()
        .filter(|clip| clip.head_transition.as_deref() == Some(&transition.identity))
        .collect();
    ensure!(
        outgoing.len() == usize::from(has_outgoing_clip)
            && incoming.len() == usize::from(has_incoming_clip),
        "{}: transition clip links conflict with HasOutgoingClip/HasIncomingClip",
        transition.identity
    );
    if let Some(clip) = outgoing.first() {
        ensure!(
            clip.end_ticks == cut,
            "{}: outgoing clip does not end at the transition cut",
            transition.identity
        );
        if let Some(source) = &clip.occurrence {
            ensure!(
                source
                    .out_ticks
                    .checked_add(end - cut)
                    .is_some_and(|out| out <= source.intrinsic_ticks),
                "{}: outgoing clip lacks the required source handle",
                transition.identity
            );
        }
    }
    if let Some(clip) = incoming.first() {
        ensure!(
            clip.start_ticks == cut,
            "{}: incoming clip does not start at the transition cut",
            transition.identity
        );
        if let Some(source) = &clip.occurrence {
            ensure!(
                source.in_ticks >= cut - start,
                "{}: incoming clip lacks the required source handle",
                transition.identity
            );
        }
    }

    Ok(PrVideoTransition {
        id: transition.identity,
        kind,
        start_ticks: start,
        cut_ticks: cut,
        end_ticks: end,
        outgoing_clip: outgoing.first().map(|clip| clip.id.clone()),
        incoming_clip: incoming.first().map(|clip| clip.id.clone()),
    })
}

/// Read one video track's index, output state, occurrences and transitions.
///
/// An error omits the whole track.
/// One video track's native facts, before its items are read.
struct NativeTrack {
    index: i64,
    /// The persistent `Track/ID`, which a Track Matte Key's Matte names.
    id: Option<usize>,
    /// Whether the track's video output is on.
    output_enabled: bool,
    clips: Vec<Reference>,
    transitions: Vec<Reference>,
}

fn read_track(
    graph: &Graph<'_>,
    record: Record<'_>,
    reference: &Reference,
    group_identity: &str,
    seen_indices: &BTreeSet<i64>,
    omissions: &mut Vec<Omission>,
) -> Result<NativeTrack> {
    report_track_structure(record, omissions)?;
    let track = graph.decode_as::<VideoClipTrack>(record, group_identity)?;
    let clip_track = required(track.value.clip_track, &track.identity, "ClipTrack")?;
    let track_fields = required(clip_track.track.as_ref(), &track.identity, "Track")?;
    let index = match track_fields.index.as_deref() {
        Some(value) => integer(value, &format!("{}: invalid track Index", track.identity))?,
        None => 0,
    };
    // Only a Track Matte Key reads the ID; a track whose ID is not a number
    // keeps its items and can be no matte track.
    let id = track_fields
        .id
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok());
    ensure!(
        index >= 0,
        "{}: video track Index must be nonnegative",
        track.identity
    );
    ensure!(
        !seen_indices.contains(&index),
        "{}: duplicate video track Index {index}",
        track.identity
    );
    if let Some(value) = &reference.index {
        ensure!(
            value.parse::<i64>().ok() == Some(index),
            "{}: conflicting track reference Index",
            track.identity
        );
    }
    validate_track_holder(
        &track.identity,
        "Track",
        track_fields.index.as_deref(),
        track_fields.media_type.as_deref(),
        index,
    )?;
    let output_enabled = !visibility::is_muted(track_fields.is_muted.as_deref(), &track.identity)?;
    let clip_items = required(clip_track.clip_items.as_ref(), &track.identity, "ClipItems")?;
    validate_track_holder(
        &track.identity,
        "ClipItems",
        clip_items.index.as_deref(),
        clip_items.media_type.as_deref(),
        index,
    )?;
    if let Some(transitions) = &clip_track.transition_items {
        validate_track_holder(
            &track.identity,
            "TransitionItems",
            transitions.index.as_deref(),
            transitions.media_type.as_deref(),
            index,
        )?;
    }
    let clips = clip_track
        .clip_items
        .and_then(|items| items.track_items)
        .map_or_else(Vec::new, |items| items.items);
    let transitions = clip_track
        .transition_items
        .and_then(|items| items.track_items)
        .map_or_else(Vec::new, |items| items.items);
    Ok(NativeTrack {
        index,
        id,
        output_enabled,
        clips,
        transitions,
    })
}

fn validate_track_holder(
    identity: &str,
    holder: &str,
    index_value: Option<&str>,
    media_type: Option<&str>,
    index: i64,
) -> Result<()> {
    if let Some(value) = index_value {
        ensure!(
            value.parse::<i64>().ok() == Some(index),
            "{identity}: conflicting {holder} Index"
        );
    }
    if let Some(value) = media_type {
        ensure!(
            value == VIDEO_MEDIA,
            "{identity}: unsupported video track MediaType"
        );
    }
    Ok(())
}

/// One video track item's clip placement, read by the same rules whatever its
/// clip plays: media (`read_occurrence`) or another sequence
/// (`nested::read_nest`). The clip and its master clip play `source_record`,
/// which the caller reads by kind.
pub(super) struct Placement<'g> {
    pub(super) enabled: bool,
    pub(super) transform: PrStaticTransform,
    pub(super) animations: Vec<PrPropertyAnimation>,
    pub(super) crop: PrStaticCrop,
    pub(super) linear_wipe: Option<PrLinearWipe>,
    pub(super) opacity_mask: Option<PrMask>,
    /// The Track Matte Key with its matte track resolved to the track's
    /// index; whether that track holds the matte is checked once every item
    /// is read (`resolve_track_mattes`).
    pub(super) track_matte: Option<PrTrackMatte>,
    pub(super) opacity: f64,
    pub(super) blend_mode: PrBlendMode,
    /// The component chain; the media reader reads its standard effects once
    /// the occurrence validates.
    pub(super) chain: Option<Located<VideoComponentChain>>,
    /// Whether the chain holds a standard effect other than an active Crop,
    /// Linear Wipe or Track Matte Key, which the Motion reader owns.
    pub(super) has_effects: bool,
    pub(super) start: i64,
    pub(super) end: i64,
    pub(super) sub: Located<SubClip>,
    pub(super) clip: Located<VideoClip>,
    /// Scale to Frame Size on `clip` ([`scale_to_frame_size`]); each host
    /// rejects it with its own reason.
    pub(super) scale_to_frame: bool,
    pub(super) playback_rate: f64,
    pub(super) frame_blending: Option<fx_schema::FrameBlendingMode>,
    pub(super) time_remap: Option<PrTimeRemap>,
    pub(super) source_in: i64,
    pub(super) source_out: i64,
    pub(super) source_record: Record<'g>,
    /// Whether the clip carries `AdjustmentLayer` true, as its master clip
    /// then carries `IsAdjustmentLayer` true (`reader/adjustment.rs`).
    pub(super) adjustment: bool,
}

/// The one `VideoClip` of `master`, which draws its picture.
///
/// A media master also lists the linked `AudioClip` that its sound placements
/// play and, once Premiere has transcribed the media, a `TranscriptClip` of
/// speech-to-text data (as the video masters of a real Premiere 26.3 project
/// do); neither draws. A second `VideoClip` is ambiguous and any other record
/// may draw, so both are unsupported.
pub(super) fn master_video_clip(
    graph: &Graph<'_>,
    master: &Located<MasterClip>,
) -> Result<Located<VideoClip>> {
    let clips = required(master.value.clips.as_ref(), &master.identity, "Clips")?;
    let mut video = None;
    for reference in &clips.items {
        let record = graph.locate(reference, &master.identity)?;
        match record.tag() {
            tag if tag == records::VIDEO_CLIP.tag => ensure!(
                video.replace(record).is_none(),
                "{}: multiple source clips unsupported",
                master.identity
            ),
            tag if tag == records::AUDIO_CLIP.tag || tag == caption::TRANSCRIPT_CLIP.tag => {}
            tag => {
                return Err(unsupported(format!(
                    "{}: {tag} source clip unsupported",
                    master.identity
                )));
            }
        }
    }
    let video = required(video, &master.identity, "video source clip")?;
    let video = graph.decode_as::<VideoClip>(video, &master.identity)?;
    ensure!(
        !video.value.declares_frame_hold(),
        "{}: FrameHold on a master source is unsupported",
        video.identity
    );
    Ok(video)
}

/// The source end that a forward unit-speed clip plays. Premiere plays such a
/// clip from In for its timeline `duration`, and it can save an OutPoint up to
/// one sequence frame from that end.
/// A larger difference keeps the saved Out, which validation rejects, because
/// it can be a retiming that the reader does not know; so does an empty or
/// reversed saved range, which is malformed rather than stale.
fn unit_source_out(source_in: i64, saved_out: i64, duration: i64, frame_rate: FrameRate) -> i64 {
    source_in
        .checked_add(duration)
        .filter(|played| {
            saved_out > source_in
                && played.abs_diff(saved_out) <= frame_rate.ticks_per_frame().unsigned_abs()
        })
        .unwrap_or(saved_out)
}

pub(super) fn read_placement<'g>(
    graph: &'g Graph<'_>,
    item: &Located<VideoClipTrackItem>,
    parent: &super::nested::Parent<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<Placement<'g>> {
    if let Some(tone_map_settings) = &item.value.tone_map_settings {
        let value: ToneMapSettings = serde_json::from_str(tone_map_settings)?;
        if value != ToneMapSettings::DEFAULT {
            omit(
                omissions,
                OmissionScope::Feature,
                &item.identity,
                "nondefault tone mapping not converted",
            );
        }
    }
    // A placement stores the frame of the sequence that holds it, whatever
    // its source's size (`vhsvertical` places a 1080p nest in a portrait
    // sequence with the portrait frame); another frame has no known mapping.
    let [width, height] = frame_dimensions(
        required(
            item.value.frame_rect.as_deref(),
            &item.identity,
            "FrameRect",
        )?,
        &item.identity,
    )?;
    ensure!(
        [width, height] == parent.dimensions,
        "{}: occurrence FrameRect must match the sequence canvas: {width}x{height} on a {}x{} sequence",
        item.identity,
        parent.dimensions[0],
        parent.dimensions[1]
    );
    let ratio = records::PixelAspectRatio::parse(
        required(
            item.value.pixel_aspect_ratio.as_deref(),
            &item.identity,
            "PixelAspectRatio",
        )?,
        &item.identity,
    )?;
    ensure!(
        ratio.is_square(),
        "{}: non-square occurrence pixels ({ratio}) are unsupported",
        item.identity
    );
    let clip_track_item = required(
        item.value.clip_track_item.as_ref(),
        &item.identity,
        "ClipTrackItem",
    )?;
    require_zero_subclip_time_offset(clip_track_item, &item.identity)?;
    let enabled = !visibility::is_muted(clip_track_item.is_muted.as_deref(), &item.identity)?;
    let chain = if let Some(components) = clip_track_item
        .component_owner
        .as_ref()
        .and_then(|owner| owner.components.as_ref())
    {
        default_chain(graph, components, &item.identity, omissions);
        Some(graph.follow::<VideoComponentChain>(components, &item.identity)?)
    } else {
        omit(
            omissions,
            OmissionScope::Feature,
            &item.identity,
            "missing component chain; effects not converted",
        );
        None
    };
    let (
        transform,
        animations,
        crop,
        linear_wipe,
        track_matte,
        opacity,
        blend_mode,
        opacity_mask,
        has_effects,
    ) = match &chain {
        Some(chain) => {
            let components = chain_components(chain)?;
            let split = effects::split_chain(graph, components, &chain.identity)?;
            split.reject_coverage_effects(graph)?;
            let MotionAndMasks {
                transform,
                mut animations,
                crop,
                linear_wipe,
                track_matte,
            } = read_video_animations(graph, chain, &split.motion_and_masks, true)?;
            let track_matte = match track_matte {
                Some(key) => Some(PrTrackMatte {
                    track_index: parent.matte_track_index(key.matte_track_id, &item.identity)?,
                    channel: key.channel,
                }),
                None => None,
            };
            let (opacity, blend_mode, opacity_animation, opacity_mask) =
                read_video_compositing(graph, chain, &split.motion_and_masks, omissions)?;
            if let Some(animation) = opacity_animation {
                animations.push(animation);
            }
            (
                transform,
                animations,
                crop,
                linear_wipe,
                track_matte,
                opacity,
                blend_mode,
                opacity_mask,
                // The Motion reader takes every component but the effects.
                split.motion_and_masks.len() < components.len(),
            )
        }
        None => (
            Default::default(),
            Vec::new(),
            Default::default(),
            None,
            None,
            100.0,
            crate::schema::PrBlendMode::Normal,
            None,
            false,
        ),
    };

    let range = required(
        clip_track_item.track_item.as_ref(),
        &item.identity,
        "TrackItem",
    )?;
    let start = match range.start.as_deref() {
        Some(value) => integer(value, &format!("{}: invalid Start", item.identity))?,
        None => 0,
    };
    let end = integer(&range.end, &format!("{}: invalid End", item.identity))?;
    let sub_reference = required(
        clip_track_item.sub_clip.as_ref(),
        &item.identity,
        records::SUB_CLIP.tag,
    )?;
    let sub = graph.follow::<SubClip>(sub_reference, &item.identity)?;
    let clip = graph.follow::<VideoClip>(&sub.value.clip, &sub.identity)?;
    let native_clip = required(clip.value.clip.as_ref(), &clip.identity, "Clip")?;
    let is_adjustment = adjustment::flag(
        clip.value.adjustment_layer.as_deref(),
        &clip.identity,
        "AdjustmentLayer",
    )?;
    ensure!(
        !is_adjustment || !clip.value.declares_frame_hold(),
        "{}: FrameHold on an adjustment layer is unsupported",
        clip.identity
    );
    let scale_to_frame = scale_to_frame_size(&clip.value, &clip.identity)?;
    let signed_playback_rate = playback_rate(native_clip, &clip.identity)?;
    let frame_blending = frame_blending(&clip.value, &clip.identity, omissions);
    let duration = end.checked_sub(start).ok_or_else(|| {
        unsupported(format!(
            "{}: occurrence duration exceeds tick range",
            item.identity
        ))
    })?;
    let time_remap = match read_frame_hold(&clip.value, duration, &clip.identity)? {
        Some(hold) => Some(hold),
        None => native_clip
            .time_remapping
            .as_ref()
            .map(|reference| read_time_remapping(graph, reference, &clip.identity))
            .transpose()?,
    };
    ensure!(
        time_remap.is_none() || signed_playback_rate == 1.0,
        "{}: constant playback combined with TimeRemapping is unsupported",
        item.identity
    );
    ensure!(
        time_remap.is_none() || animations.is_empty(),
        "{}: Motion animation combined with TimeRemapping is unsupported",
        item.identity
    );
    report_markers(graph, native_clip, &clip.identity, omissions);
    let source_in = required_integer(native_clip.in_point.as_deref(), &clip.identity, "InPoint")?;
    let saved_out = required_integer(native_clip.out_point.as_deref(), &clip.identity, "OutPoint")?;
    let source_out = match end.checked_sub(start) {
        Some(duration) if signed_playback_rate == 1.0 && time_remap.is_none() => {
            unit_source_out(source_in, saved_out, duration, parent.frame_rate)
        }
        _ => saved_out,
    };
    let source_reference = required(native_clip.source.as_ref(), &clip.identity, "Source")?;
    let source_record = graph.locate(source_reference, &clip.identity)?;

    // Premiere flags the item and each of its placements together, so the
    // item's flag can only be checked on an item the placement names.
    ensure!(
        !is_adjustment || sub.value.master_clip.is_some(),
        "{}: an AdjustmentLayer clip requires its MasterClip",
        sub.identity
    );
    // Source/master effects must not disappear merely because occurrence effects are default.
    if let Some(master_reference) = &sub.value.master_clip {
        let master_record = graph.locate(master_reference, &sub.identity)?;
        report_unknown_children(
            master_record.element(),
            &[
                "Node",
                "LoggingInfo",
                "AudioComponentChains",
                "Clips",
                "AudioClipChannelGroups",
                "Name",
                "IsAdjustmentLayer",
                "MasterClipChangeVersion",
            ],
            &master_record.identity(),
            "",
            omissions,
        );
        let master = graph.decode_as::<MasterClip>(master_record, &sub.identity)?;
        // One flag without the other is a shape this reader has not seen.
        ensure!(
            adjustment::flag(
                master.value.is_adjustment_layer.as_deref(),
                &master.identity,
                "IsAdjustmentLayer",
            )? == is_adjustment,
            "{}: IsAdjustmentLayer disagrees with the placed clip's AdjustmentLayer flag",
            master.identity
        );
        let original = master_video_clip(graph, &master)?;
        let original_clip = required(original.value.clip.as_ref(), &original.identity, "Clip")?;
        ensure!(
            playback_rate(original_clip, &original.identity)? == 1.0,
            "{}: master source playback must remain unit and forward",
            original.identity
        );
        report_markers(graph, original_clip, &original.identity, omissions);
        // Master In/Out marks, including a subclip's, annotate the media; the
        // occurrence keeps its own absolute source range. The caller reads the
        // shared source record.
        let original_source =
            required(original_clip.source.as_ref(), &original.identity, "Source")?;
        ensure!(
            graph.locate(original_source, &original.identity)? == source_record,
            "{}: source identity mismatch",
            master.identity
        );
    }
    Ok(Placement {
        enabled,
        transform,
        animations,
        crop,
        linear_wipe,
        opacity_mask,
        track_matte,
        opacity,
        blend_mode,
        chain,
        has_effects,
        start,
        end,
        sub,
        clip,
        scale_to_frame,
        playback_rate: signed_playback_rate,
        frame_blending,
        time_remap,
        source_in,
        source_out,
        source_record,
        adjustment: is_adjustment,
    })
}

fn read_occurrence(
    graph: &Graph<'_>,
    video: &Located<VideoTrackGroup>,
    item: Located<VideoClipTrackItem>,
    media_table: &mut BTreeMap<MediaId, PrMedia>,
    sequence_rate: FrameRate,
    parent: &super::nested::Parent<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<PrVideoOccurrence> {
    let Placement {
        enabled,
        transform,
        animations,
        crop,
        linear_wipe,
        opacity_mask,
        track_matte,
        opacity,
        blend_mode,
        chain,
        start,
        end,
        sub,
        clip,
        scale_to_frame,
        playback_rate: signed_playback_rate,
        frame_blending,
        time_remap,
        source_in,
        source_out,
        source_record,
        adjustment,
        ..
    } = read_placement(graph, &item, parent, omissions)?;
    let source = graph.decode_as::<VideoMediaSource>(source_record, &clip.identity)?;
    let media_source = required(
        source.value.media_source.as_ref(),
        &source.identity,
        "MediaSource",
    )?;
    if let Some(content) = &media_source.content {
        content.require_unbounded(&source.identity)?;
    }
    let media_reference = required(media_source.media.as_ref(), &source.identity, "Media")?;
    let media_id = read_source_media(
        graph,
        media_reference,
        &source.identity,
        media_table,
        adjustment,
        omissions,
    )?;
    validate_color(
        video.value.output_color_space.as_deref(),
        &video.identity,
        true,
        "OutputColorSpace",
    )?;
    let original_duration = required_integer(
        source.value.original_duration.as_deref(),
        &source.identity,
        "OriginalDuration",
    )?;
    let stream = media_table[&media_id]
        .video
        .as_ref()
        .filter(|video| original_duration == video.intrinsic_ticks)
        .ok_or_else(|| {
            unsupported(format!(
                "{}: source OriginalDuration conflicts with VideoStream.Duration",
                source.identity
            ))
        })?;
    let kind = stream.kind;
    // Import draws a Color Matte as a rectangle of its sequence's canvas and an
    // adjustment layer over the whole composite below it, so generator media of
    // another size would silently grow to the canvas. Shared media is checked
    // against each placement's own sequence.
    let generator = match kind {
        crate::schema::PrMediaKind::ColorMatte(_) => Some("a Color Matte"),
        crate::schema::PrMediaKind::Adjustment => Some("an adjustment layer"),
        crate::schema::PrMediaKind::Video { .. }
        | crate::schema::PrMediaKind::Still { .. }
        | crate::schema::PrMediaKind::AfterEffectsComposition(_) => None,
    };
    if let Some(generator) = generator {
        ensure!(
            [stream.width, stream.height] == parent.dimensions,
            "{}: {generator} of {}x{} on a {}x{} sequence is not converted: import covers the whole canvas with it, and how Premiere draws one of another size is unmeasured",
            item.identity,
            stream.width,
            stream.height,
            parent.dimensions[0],
            parent.dimensions[1]
        );
    }
    ensure!(
        !scale_to_frame,
        "{}: Scale to Frame Size on {} is not converted",
        clip.identity,
        match kind {
            crate::schema::PrMediaKind::Video { .. } => "a video clip",
            crate::schema::PrMediaKind::Still { .. } => "a still",
            crate::schema::PrMediaKind::AfterEffectsComposition(_) =>
                "an After Effects composition",
            crate::schema::PrMediaKind::ColorMatte(_) => "a Color Matte",
            crate::schema::PrMediaKind::Adjustment => "an adjustment layer",
        }
    );
    ensure!(
        animations.is_empty() || !matches!(kind, crate::schema::PrMediaKind::ColorMatte(_)),
        "{}: Motion keyframes on a Color Matte are not converted",
        item.identity
    );
    // Premiere holds a keyed property's first key before that key, also over
    // a trim that starts earlier, and the FX animator holds it the same way,
    // so the static StartKeyframe of a keyed property is never visible.
    let mut occurrence = PrVideoOccurrence {
        id: None,
        media: media_id.clone(),
        start_ticks: start,
        end_ticks: end,
        in_ticks: source_in,
        out_ticks: source_out,
        playback_rate: signed_playback_rate,
        frame_blending,
        opacity,
        blend_mode,
        transform,
        crop,
        animations,
        time_remap,
        linear_wipe,
        opacity_mask,
        track_matte,
        enabled,
        effects: Vec::new(),
        effects_above_mask: 0,
        stroke: None,
        active_transforms: 0,
    };
    occurrence.validate(sequence_rate, &media_table[&media_id])?;
    if let Some(chain) = &chain {
        let split = effects::split_chain(graph, chain_components(chain)?, &chain.identity)?;
        if adjustment && occurrence.transform != crate::schema::PrStaticTransform::default() {
            split.reject_unmeasured_adjustment_coverage(graph)?;
        }
        let owner = effects::EffectOwner {
            occurrence: &item.identity,
            clip_name: sub.value.name.as_deref(),
            track_index: parent.track_index,
            timeline_ticks: start..end,
        };
        let mask = !occurrence.crop.is_default()
            || occurrence.linear_wipe.is_some()
            || occurrence.track_matte.is_some();
        occurrence.stroke = split.read_stroke(graph, omissions);
        occurrence.active_transforms = u8::try_from(split.active_transforms()).unwrap_or(u8::MAX);
        (occurrence.effects, occurrence.effects_above_mask) =
            split.read_effects(graph, &owner, mask, omissions);
        // Premiere applies Opacity, and so its mask, after every standard
        // effect; a clip with both a Crop or wipe and an Opacity mask is
        // omitted by `mask_boundary`.
        if occurrence.opacity_mask.is_some() {
            occurrence.effects_above_mask = occurrence.effects.len();
        }
    }
    occurrence.id = Some(item.identity);
    Ok(occurrence)
}

/// Returns the stable identity of a `Media` record and reads it once. A
/// placement flagged as an `adjustment` layer reads Black Video media as
/// [`PrMediaKind::Adjustment`]; the record must then be that media on every
/// placement, flagged or not.
pub(super) fn read_source_media(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
    media_table: &mut BTreeMap<MediaId, PrMedia>,
    adjustment: bool,
    omissions: &mut Vec<Omission>,
) -> Result<MediaId> {
    let record = graph.locate(reference, from)?;
    let media_id = MediaId(
        if let Some(id) = record.element().attribute(records::OBJECT_ID) {
            format!("Media:ObjectID:{id}")
        } else {
            let uid = required(
                record.element().attribute(records::OBJECT_UID),
                &record.identity(),
                records::OBJECT_UID,
            )?;
            format!("Media:ObjectUID:{uid}")
        },
    );
    if let std::collections::btree_map::Entry::Vacant(entry) = media_table.entry(media_id.clone()) {
        let media = graph.decode_as::<Media>(record, from)?;
        entry.insert(if adjustment {
            adjustment::read_adjustment_media(graph, media)?
        } else if color_matte::is_color_matte_media(record) {
            color_matte::read_color_matte_media(graph, media)?
        } else {
            read_media(graph, media, omissions)?
        });
    }
    ensure!(
        media_table[&media_id].is_adjustment() == adjustment,
        "{from}: media placed both by an AdjustmentLayer clip and by another clip"
    );
    Ok(media_id)
}

fn read_video_stream(
    graph: &Graph<'_>,
    reference: &Reference,
    media: &Located<Media>,
) -> Result<PrVideoStream> {
    let stream = graph.follow::<VideoStream>(reference, &media.identity)?;
    let kind = match super::after_effects::media_kind(graph, media, &stream)? {
        Some(kind) => kind,
        None => super::still::media_kind(&stream, media)?,
    };
    let frame_rate_ticks = required_integer(
        stream.value.frame_rate.as_deref(),
        &stream.identity,
        "FrameRate",
    )?;
    let frame_rate = if matches!(kind, crate::schema::PrMediaKind::Video { .. }) {
        crate::schema::SourceFrameRate::from_ticks_per_frame(frame_rate_ticks)?
    } else {
        super::frame_rate(frame_rate_ticks, &stream.identity)?.into()
    };
    let [width, height] = stream_dimensions(&stream)?;
    let orientation = match stream.value.original_image_orientation_type.as_deref() {
        Some(code) => crate::schema::VideoOrientation::from_native(code)?,
        None => crate::schema::VideoOrientation::Identity,
    };
    ensure!(
        matches!(kind, crate::schema::PrMediaKind::Video { .. })
            || orientation == crate::schema::VideoOrientation::Identity,
        "{}: non-video source orientation unsupported",
        stream.identity
    );
    validate_color(
        stream.value.original_color_space.as_deref(),
        &stream.identity,
        false,
        "OriginalColorSpace",
    )?;
    let intrinsic_ticks = required_integer(
        stream.value.duration.as_deref(),
        &stream.identity,
        "Duration",
    )?;
    ensure!(
        !matches!(kind, crate::schema::PrMediaKind::AfterEffectsComposition(_))
            || intrinsic_ticks > 0,
        "{}: After Effects composition duration must be positive",
        stream.identity
    );
    Ok(PrVideoStream {
        orientation,
        kind,
        intrinsic_ticks,
        frame_rate,
        width,
        height,
    })
}

fn read_media(
    graph: &Graph<'_>,
    media: Located<Media>,
    omissions: &mut Vec<Omission>,
) -> Result<PrMedia> {
    let video = media
        .value
        .video_stream
        .as_ref()
        .map(|reference| read_video_stream(graph, reference, &media))
        .transpose()?;
    let audio = match media
        .value
        .audio_stream
        .as_ref()
        .map(|reference| super::audio::read_stream(graph, reference, &media.identity))
        .transpose()
    {
        Ok(audio) => audio,
        // Video placements are muted. Unsupported native sound must not veto
        // their valid picture; audio placements still require a modeled stream.
        Err(error @ BuildError::Unsupported(_)) if video.is_some() => {
            omit(
                omissions,
                OmissionScope::Feature,
                &media.identity,
                format!("native audio was not imported: {error}"),
            );
            None
        }
        Err(error) => return Err(error),
    };
    ensure!(
        video.is_some() || audio.is_some(),
        "{}: media has no video or audio stream",
        media.identity
    );
    let aliases: Vec<(MediaPathField, String)> = [
        (
            MediaPathField::ActualMediaFilePath,
            media.value.actual_media_file_path,
        ),
        (MediaPathField::FilePath, media.value.file_path),
    ]
    .into_iter()
    .filter_map(|(field, path)| path.map(|path| (field, path)))
    .collect();
    // Premiere on Windows names media by drive-absolute paths, which this host
    // cannot open, and separates RelativePath components with `\` (every such
    // corpus Media with a RelativePath has exactly one `.\` or `..\` hint).
    // Then only the package-local hint can identify the local copy; the aliases
    // are not local candidates.
    let saved_on_windows = !aliases.is_empty()
        && aliases
            .iter()
            .all(|(_, path)| is_foreign_windows_path(path));
    let mut hints = media.value.relative_paths;
    ensure!(
        !hints.is_empty() || !aliases.is_empty(),
        "{}: missing RelativePath and absolute media aliases",
        media.identity
    );
    if saved_on_windows {
        for hint in &mut hints {
            ensure!(
                !has_windows_drive(hint),
                "{}: RelativePath names a Windows drive",
                media.identity
            );
            *hint = hint.replace('\\', "/");
        }
    }
    let (relative_path, relative_paths) = media_relative_paths(hints, &media.identity)?;
    let mut absolute_paths: Vec<(MediaPathField, PathBuf)> = Vec::new();
    if saved_on_windows {
        ensure!(
            relative_path.is_some(),
            "{}: media saved on Windows has no package-local RelativePath",
            media.identity
        );
    } else {
        for (field, path) in &aliases {
            ensure!(
                !path.is_empty() && Path::new(path).is_absolute(),
                "{}: {} must be a nonempty absolute path",
                media.identity,
                field.tag()
            );
            absolute_paths.push((*field, path.into()));
        }
    }
    let name = relative_path
        .as_deref()
        .or_else(|| relative_paths.first().map(String::as_str))
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .or_else(|| {
            absolute_paths.first().and_then(|(_, path)| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
        })
        .ok_or_else(|| unsupported("native media reference has no safe UTF-8 name"))?;
    if video.as_ref().is_some_and(|video| {
        matches!(
            video.kind,
            crate::schema::PrMediaKind::AfterEffectsComposition(_)
        )
    }) {
        ensure!(
            relative_paths
                .iter()
                .map(Path::new)
                .chain(aliases.iter().map(|(_, path)| Path::new(path)))
                .all(|path| path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("aep"))),
            "{}: After Effects linked source must name an AEP file",
            media.identity
        );
    }
    Ok(PrMedia {
        name,
        relative_path,
        relative_paths,
        absolute_paths,
        video,
        audio,
    })
}

fn validate_color(
    encoded: Option<&str>,
    identity: &str,
    sequence: bool,
    field: &str,
) -> Result<()> {
    if let Some(encoded) = encoded {
        let value: ColorSpace = serde_json::from_str(encoded)?;
        ensure!(
            if sequence {
                value.is_sequence_sdr()
            } else {
                value.is_source()
            },
            "{identity}: unsupported {field} profile for its native role"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;

/// Whether `path` starts with a Windows drive designator such as `C:`.
fn has_windows_drive(path: &str) -> bool {
    matches!(path.as_bytes(), [drive, b':', ..] if drive.is_ascii_alphabetic())
}

/// Whether `path` is a Windows drive-absolute path (`C:\…`) that this host
/// does not treat as absolute, so it names a file on the machine that saved the
/// project. Drive-relative (`C:name`), UNC and device paths are not this form.
fn is_foreign_windows_path(path: &str) -> bool {
    has_windows_drive(path)
        && path.as_bytes().get(2) == Some(&b'\\')
        && !Path::new(path).is_absolute()
}

fn media_relative_paths(
    mut candidates: Vec<String>,
    identity: &str,
) -> Result<(Option<String>, Vec<String>)> {
    // Premiere 25.0 and 26.5.1 saves can repeat a RelativePath; keep the first copy.
    let mut seen = BTreeSet::new();
    candidates.retain(|value| seen.insert(value.clone()));
    ensure!(
        candidates
            .iter()
            .all(|value| !value.is_empty() && !Path::new(value).is_absolute()),
        "{identity}: empty or absolute RelativePath"
    );
    let package_local: Vec<_> = candidates
        .iter()
        .filter(|value| {
            !Path::new(value)
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        })
        .collect();
    ensure!(
        package_local.len() <= 1,
        "{identity}: expected at most one package-local RelativePath, found {}",
        package_local.len()
    );
    Ok((
        package_local.first().map(|path| (*path).clone()),
        candidates,
    ))
}
