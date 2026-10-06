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
    video::report_unknown_children(
        sequence_element,
        &[
            "Node",
            "PersistentGroupContainer",
            "TrackGroups",
            "Name",
            "PreviewFormatIdentifier",
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
    for entry in track_groups.groups {
        match graph.locate(&entry.target, &sequence.identity) {
            Ok(group) => groups.push(group),
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                &sequence.identity,
                format!("track group not converted: {error}"),
            ),
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
    let mut audio_occurrences = Vec::new();
    let mut nest_sounds = Vec::new();
    // Camera selection is picture-only. Do not resolve source-sequence sound
    // or captions: only the outer sequence's saved items may play them.
    if selected_video_track.is_none() {
        for group in groups {
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
            "AutoInputGamutCompressionEnabled",
            "IsGraphicsWhiteSameAsProject",
            "IsColorAwareEffectsEnabledSameAsProject",
            "FrameRect",
            "PixelAspectRatio",
            "ComponentOwner",
        ],
        &video_identity,
        "",
        omissions,
    );
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
    let native_frame_ticks = video
        .value
        .track_group
        .as_ref()
        .and_then(|group| group.frame_rate.as_deref())
        .and_then(|ticks| ticks.parse::<i64>().ok());
    let (frame_rate, [width, height], mut video_tracks) = video::read_tracks(
        graph,
        video,
        media,
        nesting,
        omissions,
        selected_video_track,
    )?;
    audio::clamp_short_fades(&mut audio_occurrences, frame_rate, omissions);
    let alone = super::nested::pair_sounds(&mut video_tracks, nest_sounds, omissions)?;
    // Converted audio extends this video end exactly in PrSequence::end_ticks.
    let timeline_end_ticks = timeline_end::read_sequence_end(
        graph,
        video_record,
        native_frame_ticks.unwrap_or(frame_rate.ticks_per_frame()),
    );
    let sequence_id = sequence
        .value
        .object_uid
        .ok_or_else(|| unsupported("sequence GUID missing"))?;
    let mut parsed = PrSequence {
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
    };
    super::nested::play_alone(graph, alone, &mut parsed, media, nesting, omissions);
    if selected_video_track.is_none() {
        super::caption::read_caption_tracks(graph, &data_groups, &mut parsed, nesting, omissions);
    }
    // An adjustment layer has no picture of its own, so it is not content.
    let has_content = parsed.video_items().any(|item| {
        item.media()
            .is_none_or(|clip| !media[&clip.media].is_adjustment())
    });
    if !has_content && parsed.nest_occurrences().next().is_none() && parsed.audio.is_empty() {
        return Err(unsupported(format!(
            "no convertible video or audio occurrences: {}",
            omissions[first_omission..]
                .last()
                .map_or("none", |item| item.reason.as_str())
        )));
    }
    parsed.validate_timeline(media)?;
    Ok(parsed)
}

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
