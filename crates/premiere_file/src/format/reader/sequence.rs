//! Sequence selection and track-group coordination.

use super::{audio, non_empty, required, timeline_end, video};
use crate::{
    error::{ensure, unsupported, BuildError, Result},
    format::{Graph, Record},
    omit,
    schema::{
        native::{AudioTrackGroup, Sequence, VideoTrackGroup},
        records, MediaId, PrMedia, PrSequence,
    },
    Omission, OmissionScope,
};
use std::collections::{BTreeMap, BTreeSet};

// Classify native membership, not retained output: a failed ordinary picture
// must never be mistaken for an intentionally adjustment-only mix template.
fn adjustment_only_template(graph: &Graph<'_>, groups: &[Record<'_>]) -> bool {
    let mut adjustments = 0;
    for group in groups {
        let Ok(references) = group.track_references() else {
            return false;
        };
        for reference in references {
            let Ok(track) = graph.locate(&reference, "template membership") else {
                return false;
            };
            if !track.has_referenced_track_items() {
                continue;
            }
            if group.tag() != records::VIDEO_TRACK_GROUP.tag
                || track.tag() != records::VIDEO_CLIP_TRACK.tag
            {
                return false;
            }
            let items = track.track_item_references();
            if items.is_empty() {
                return false;
            }
            for reference in items {
                let Ok(record) = graph.locate(&reference, "template item") else {
                    return false;
                };
                let Ok(item) = graph.decode::<crate::schema::native::VideoClipTrackItem>(record)
                else {
                    return false;
                };
                if !super::adjustment::is_flagged(graph, &item) {
                    return false;
                }
                adjustments += 1;
            }
        }
    }
    adjustments > 0
}

// Only native membership can distinguish an empty source from failed conversion.
fn empty_native_sequence(graph: &Graph<'_>, groups: &[Record<'_>]) -> bool {
    groups.iter().all(|group| {
        if group.tag() == records::VIDEO_TRACK_GROUP.tag {
            let Ok(video) = graph.decode::<VideoTrackGroup>(*group) else {
                return false;
            };
            if let Some(reference) = video
                .value
                .component_owner
                .and_then(|owner| owner.components)
            {
                let Ok(chain) = graph.follow::<crate::schema::native::VideoComponentChain>(
                    &reference,
                    &video.identity,
                ) else {
                    return false;
                };
                if !super::animation::chain_components(&chain)
                    .is_ok_and(|components| components.is_empty())
                {
                    return false;
                }
            }
        } else if group.tag() != records::AUDIO_TRACK_GROUP.tag
            && group.tag() != records::DATA_TRACK_GROUP.tag
        {
            return false;
        }
        group.track_references().is_ok_and(|references| {
            references.iter().all(|reference| {
                graph
                    .locate(reference, "empty sequence membership")
                    .is_ok_and(|track| !track.has_referenced_track_items())
            })
        })
    })
}

fn report_non_video_tracks(graph: &Graph<'_>, group: Record<'_>, omissions: &mut Vec<Omission>) {
    let identity = group.identity();
    let references = match group.track_references() {
        Ok(references) => references,
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Feature,
                identity,
                format!("non-video group not converted: {error}"),
            );
            return;
        }
    };
    for reference in references {
        match graph.locate(&reference, &identity) {
            Ok(track) if track.tag() == records::VIDEO_CLIP_TRACK.tag => {
                omit(
                    omissions,
                    OmissionScope::Track,
                    track.identity(),
                    "video track outside video group not converted",
                );
            }
            // Caption tracks are read once the video tracks fix frame and cadence.
            Ok(track)
                if group.tag() == records::DATA_TRACK_GROUP.tag
                    && track.tag() == crate::schema::caption::CAPTION_DATA_CLIP_TRACK.tag => {}
            Ok(track) if track.has_referenced_track_items() => {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    track.identity(),
                    "non-video track items not converted",
                );
            }
            Ok(_) => {}
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                &identity,
                format!("non-video track not converted: {error}"),
            ),
        }
    }
}

pub(super) fn read_sequence(
    graph: &Graph<'_>,
    sequence_id: Option<&str>,
    cyclic: &BTreeSet<String>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    omissions: &mut Vec<Omission>,
) -> Result<PrSequence> {
    let mut nesting = super::nested::Nesting::new(sequence_id, cyclic);
    read_sequence_at(graph, sequence_id, media, &mut nesting, omissions)
}

/// Reads one sequence; its nested placements read their sequences through `nesting`.
pub(super) fn read_sequence_at(
    graph: &Graph<'_>,
    sequence_id: Option<&str>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    nesting: &mut super::nested::Nesting<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<PrSequence> {
    read_sequence_tracks_at(graph, sequence_id, media, nesting, omissions, None)
}

/// A multicam cut reads only its selected native video track. Sound keeps its
/// independent saved routing, rather than following picture selection.
pub(super) fn read_sequence_tracks_at(
    graph: &Graph<'_>,
    sequence_id: Option<&str>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    nesting: &mut super::nested::Nesting<'_>,
    omissions: &mut Vec<Omission>,
    selected_video_track: Option<usize>,
) -> Result<PrSequence> {
    // The list is shared by every top-level sequence read into one project;
    // a nested read fills one of its own (`nested::Nesting::read`).
    let first_omission = omissions.len();
    let sequences: Vec<_> = if let Some(sequence_id) = sequence_id {
        graph
            .locate_uid(sequence_id, "sequence selection")
            .ok()
            .filter(|record| record.tag() == records::SEQUENCE.tag)
            .into_iter()
            .collect()
    } else {
        graph
            .records()
            .filter(|record| record.tag() == records::SEQUENCE.tag)
            .collect()
    };
    ensure!(
        sequences.len() == 1,
        "sequence selection must identify exactly one sequence; supply an exact sequence GUID"
    );
    let sequence_record = sequences[0];
    let sequence_identity = sequence_record.identity();
    let sequence_element = sequence_record.element();
    // Older saves (`Sequence` Version 11) also write `ID`, an editing
    // identity with no picture semantics; it is admitted and not retained.
    video::report_unknown_children(
        sequence_element,
        &[
            "Node",
            "PersistentGroupContainer",
            "TrackGroups",
            "Name",
            "PreviewFormatIdentifier",
            "ID",
        ],
        &sequence_identity,
        "",
        omissions,
    );
    let sequence = graph.decode::<Sequence>(sequence_record)?;
    let track_groups = required(
        sequence.value.track_groups,
        &sequence.identity,
        "TrackGroups",
    )?;
    let mut groups = Vec::with_capacity(track_groups.groups.len());
    let mut all_groups_resolved = true;
    for entry in track_groups.groups {
        match graph.locate(&entry.target, &sequence.identity) {
            Ok(group) => groups.push(group),
            Err(error) => {
                all_groups_resolved = false;
                omit(
                    omissions,
                    OmissionScope::Feature,
                    &sequence.identity,
                    format!("track group not converted: {error}"),
                );
            }
        }
    }
    let data_groups: Vec<_> = groups
        .iter()
        .copied()
        .filter(|record| record.tag() == records::DATA_TRACK_GROUP.tag)
        .collect();
    let videos: Vec<_> = groups
        .iter()
        .copied()
        .filter(|record| record.tag() == records::VIDEO_TRACK_GROUP.tag)
        .collect();
    ensure!(
        videos.len() == 1,
        "{}: expected one video track group",
        sequence.identity
    );
    let adjustment_only_template = all_groups_resolved
        && selected_video_track.is_none()
        && adjustment_only_template(graph, &groups);
    let mut audio_occurrences = Vec::new();
    let mut nest_sounds = Vec::new();
    // Camera selection is picture-only. Do not resolve source-sequence sound
    // or captions: only the outer sequence's saved items may play them.
    if selected_video_track.is_none() {
        for group in groups.iter().copied() {
            match group.tag() {
                tag if tag == records::VIDEO_TRACK_GROUP.tag => {}
                tag if tag == records::AUDIO_TRACK_GROUP.tag => {
                    let identity = group.identity();
                    match graph
                        .decode::<AudioTrackGroup>(group)
                        .map_err(BuildError::from)
                        .and_then(|group| {
                            audio::read_tracks(graph, group, media, nesting, omissions)
                        }) {
                        Ok((occurrences, sounds)) => {
                            audio_occurrences.extend(occurrences);
                            nest_sounds.extend(sounds);
                        }
                        Err(error) => omit(
                            omissions,
                            OmissionScope::Feature,
                            identity,
                            format!("audio not converted: {error}"),
                        ),
                    }
                }
                tag if tag == records::DATA_TRACK_GROUP.tag => {
                    report_non_video_tracks(graph, group, omissions);
                }
                _ => omit(
                    omissions,
                    OmissionScope::Feature,
                    group.identity(),
                    "track group not converted",
                ),
            }
        }
    }
    let video_record = videos[0];
    let video_identity = video_record.identity();
    video::report_unknown_children(
        video_record.element(),
        &[
            "TrackGroup",
            "ColorManagementSettings",
            "ImmersiveVideoVRConfiguration",
            "OutputColorSpace",
            "WorkingColorSpace",
            "WorkingColorSpaceConfiguration",
            "AllowLinearCompositing",
            "ToneMappingAlgorithm",
            "GamutCompressionAlgorithm",
            "ToneMappingDesaturation",
            "ToneMappingKnee",
            "AutoInputGamutCompressionEnabled",
            "IsGraphicsWhiteSameAsProject",
            "IsColorAwareEffectsEnabledSameAsProject",
            "FrameRect",
            "PixelAspectRatio",
            "ComponentOwner",
            "FieldType",
        ],
        &video_identity,
        "",
        omissions,
    );
    // Older saves write the sequence field order; 0 is progressive, which is
    // how every converted picture is drawn.
    if let Some(field_type) = video_record.element().child("FieldType") {
        if field_type.text().map(str::trim) != Some("0") {
            omit(
                omissions,
                OmissionScope::Feature,
                &video_identity,
                "nondefault FieldType not converted",
            );
        }
    }
    if let Some(track_group) = video_record.element().child("TrackGroup") {
        video::report_unknown_children(
            track_group,
            &["Tracks", "FrameRate", "NextTrackID"],
            &video_identity,
            "TrackGroup/",
            omissions,
        );
    }
    if let Some(owner) = video_record.element().child("ComponentOwner") {
        video::report_unknown_children(
            owner,
            &["Components"],
            &video_identity,
            "ComponentOwner/",
            omissions,
        );
    }
    let video = graph.decode::<VideoTrackGroup>(video_record)?;
    let aces = super::color_setup::sequence(&video, omissions);
    // An ACEScct working space is not an HDR working space.
    let hdr = aces
        .is_none()
        .then(|| super::hdr::sequence(graph, &video, omissions))
        .flatten();
    let native_frame_ticks = video
        .value
        .track_group
        .as_ref()
        .and_then(|group| group.frame_rate.as_deref())
        .and_then(|ticks| ticks.parse::<i64>().ok());
    // A valid audio placement still sets the sequence end when its sound is
    // omitted. Camera selection remains picture-only, as above.
    let sequence_end = |video_frame_ticks| {
        std::iter::once(video_record)
            .chain(groups.iter().copied().filter(|group| {
                selected_video_track.is_none() && group.tag() == records::AUDIO_TRACK_GROUP.tag
            }))
            .map(|group| {
                timeline_end::read_sequence_end(
                    graph,
                    group,
                    if group.tag() == records::AUDIO_TRACK_GROUP.tag {
                        // Audio placements keep their exact sample-clock end;
                        // snapping upward would lengthen otherwise retained sound.
                        1
                    } else {
                        video_frame_ticks
                    },
                )
            })
            .max()
            .unwrap_or(0)
    };
    let (frame_rate, [width, height], mut video_tracks) = video::read_tracks(
        graph,
        video,
        media,
        nesting,
        omissions,
        selected_video_track,
        sequence_end(1),
    )?;
    let timeline_end_ticks =
        sequence_end(native_frame_ticks.unwrap_or(frame_rate.ticks_per_frame()));
    audio::clamp_short_fades(&mut audio_occurrences, frame_rate, omissions);
    let alone = super::nested::pair_sounds(&mut video_tracks, nest_sounds, omissions)?;
    let sequence_id = sequence
        .value
        .object_uid
        .ok_or_else(|| unsupported("sequence GUID missing"))?;
    let mut parsed = PrSequence {
        hdr,
        aces,
        native_frame_ticks: native_frame_ticks
            .filter(|ticks| *ticks != frame_rate.ticks_per_frame()),
        id: Some(sequence_id),
        name: non_empty(sequence.value.name, &sequence.identity, records::NAME)?,
        top_level: None,
        video_tracks,
        audio: audio_occurrences,
        frame_rate,
        width,
        height,
        timeline_end_ticks,
        adjustment_only_template,
        empty_template_placeholder: false,
    };
    super::nested::play_alone(graph, alone, &mut parsed, media, nesting, omissions);
    if selected_video_track.is_none() {
        super::caption::read_caption_tracks(graph, &data_groups, &mut parsed, nesting, omissions);
    }
    // Graphic placements retain exact tick endpoints, unlike sampled media.
    // Nearest-frame timeline inference must not end before retained text/shape
    // content and turn an otherwise valid nested sequence into an omission.
    let graphic_end = parsed
        .video_items()
        .filter_map(crate::schema::PrVideoItem::graphic)
        .map(|graphic| graphic.end_ticks)
        .max()
        .unwrap_or(0);
    if graphic_end > parsed.timeline_end_ticks {
        parsed.timeline_end_ticks = graphic_end;
        crate::approximate(
            omissions,
            &sequence_identity,
            "sequence end retains the exact graphic endpoint instead of rounding before editable content; native partial-frame sampling is unverified",
        );
    }
    // An adjustment layer has no picture of its own, so it is not content.
    let has_content = parsed.video_items().any(|item| {
        item.media()
            .is_none_or(|clip| !media[&clip.media].is_adjustment())
    });
    if !has_content
        && parsed.nest_occurrences().next().is_none()
        && parsed.audio.is_empty()
        && !(parsed.adjustment_only_template && parsed.timeline_end_ticks > 0)
    {
        if all_groups_resolved && empty_native_sequence(graph, &groups) {
            return Ok(empty_template_placeholder(
                parsed,
                &sequence_identity,
                nesting.reads_inner_timeline(),
                omissions,
            ));
        }
        return Err(unsupported(format!(
            "no convertible video or audio occurrences: {}",
            omissions[first_omission..]
                .last()
                .map_or("none", |item| item.reason.as_str())
        )));
    }
    if !has_content && parsed.adjustment_only_template {
        crate::approximate(omissions, &sequence_identity,
            "adjustment-only sequence retains its timed editable black canvas; omitted adjustment processing is not restored and RGB/alpha/audio fidelity is unverified");
    }
    parsed.validate_timeline(media)?;
    Ok(parsed)
}

/// Native membership of `sequence` is empty: a template drop zone ("Media"
/// sequences that a template nests under its looks, for the user to fill
/// later). Premiere renders an empty sequence as nothing, so it converts to
/// no content and its placements keep their Motion, Opacity and effects on an
/// empty group, with a caveat naming the placeholder. An empty top-level
/// sequence has no duration, which an FX document cannot have: it becomes a
/// one-frame empty canvas, reported as an approximation.
fn empty_template_placeholder(
    mut sequence: PrSequence,
    identity: &str,
    nested: bool,
    omissions: &mut Vec<Omission>,
) -> PrSequence {
    sequence.empty_template_placeholder = true;
    omit(
        omissions,
        OmissionScope::Feature,
        identity,
        format!(
            "{EMPTY_TEMPLATE_PLACEHOLDER} {:?} has no source occurrences; Premiere renders it as nothing, so it converts to an empty group that keeps its placements' Motion, Opacity and effects",
            sequence.name
        ),
    );
    if !nested && sequence.timeline_end_ticks == 0 {
        sequence.timeline_end_ticks = sequence.native_frame_rate().ticks_per_frame();
        crate::approximate(
            omissions,
            identity,
            "empty template placeholder sequence has no duration; converted as a one-frame empty canvas",
        );
    }
    sequence
}

/// Opens the caveat on an empty template placeholder sequence and on the
/// audio items that play one.
pub(super) const EMPTY_TEMPLATE_PLACEHOLDER: &str = "empty template placeholder sequence";

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn multicam_camera_read_does_not_import_source_sequence_sound() {
        let mut xml = String::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../../tests/fixtures/feature_multicam_cuts.prproj")[..],
        )
        .read_to_string(&mut xml)
        .unwrap();
        // Supplementary native-record edit: put the existing WAV item on the
        // camera sequence's empty audio track. The source fixture stays intact.
        let track = xml
            .find("<AudioClipTrack ObjectUID=\"276c725f-2544-4c8c-8951-ffac0e09d723\"")
            .unwrap();
        let marker = "<ClipItems Version=\"3\">";
        let at = track + xml[track..].find(marker).unwrap() + marker.len();
        xml.insert_str(
            at,
            "<TrackItems Version=\"1\"><TrackItem Index=\"0\" ObjectRef=\"471\" /></TrackItems>",
        );
        let graph = Graph::parse(&xml).unwrap();
        let guid = "c3262259-8ea8-41b5-ae37-a43eef3f5a2c";
        let cyclic = BTreeSet::new();
        let read = |selected| {
            let mut media = BTreeMap::new();
            let sequence = read_sequence_tracks_at(
                &graph,
                Some(guid),
                &mut media,
                &mut super::super::nested::Nesting::new(Some(guid), &cyclic),
                &mut Vec::new(),
                selected,
            )
            .unwrap();
            (sequence, media)
        };
        assert_eq!(read(None).0.audio.len(), 1);
        let (camera, media) = read(Some(0));
        assert_eq!(camera.video_tracks.len(), 1);
        assert!(camera.audio.is_empty());
        assert!(media.values().all(|source| source.audio.is_none()));
    }
}
