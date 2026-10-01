use crate::{
    format::{
        inspect_project, inspect_project_with_media, inspect_project_with_omissions, FrameRate,
    },
    schema::TICKS,
    test_support::{one_clip_xml, OneClip},
    tests::support::project_document_with_media,
    OmissionScope,
};
use fx_schema::FrameBlendingMode;
use serde_json::json;
use std::{io::Read, path::Path};

const SOURCE: &str = include_str!("../../../tests/fixtures/one-clip.xml");

fn two_cuts_xml() -> String {
    SOURCE
        .replace("<TrackItem ObjectRef=\"3\"/>", "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"9\"/>")
        .replace("<End>1270080000000</End>", "<End>508032000000</End>")
        .replace("<OutPoint>1270080000000</OutPoint>", "<OutPoint>508032000000</OutPoint>")
        .replace("<RelativePath>media/source.mp4</RelativePath>",
            "<RelativePath>../01-one-clip/media/source.mp4</RelativePath><RelativePath>media/source.mp4</RelativePath>")
        .replace("</PremiereData>", r#"
  <VideoClipTrackItem ObjectID="9"><ClipTrackItem><ComponentOwner><Components ObjectRef="10"/></ComponentOwner><TrackItem><Start>508032000000</Start><End>1278547200000</End></TrackItem><SubClip ObjectRef="11"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
  <VideoComponentChain ObjectID="10"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>
  <SubClip ObjectID="11"><Clip ObjectRef="12"/><MasterClip ObjectURef="master-2"/><Name>clip-b.mp4</Name></SubClip>
  <VideoClip ObjectID="12"><Clip><Source ObjectRef="13"/><InPoint>762048000000</InPoint><OutPoint>1532563200000</OutPoint></Clip></VideoClip>
  <VideoMediaSource ObjectID="13"><MediaSource><Media ObjectURef="media-2"/></MediaSource><OriginalDuration>2540160000000</OriginalDuration></VideoMediaSource>
  <Media ObjectUID="media-2"><VideoStream ObjectRef="14"/><RelativePath>media/clip-b.mp4</RelativePath></Media>
  <VideoStream ObjectID="14"><Duration>2540160000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect></VideoStream>
  <MasterClip ObjectUID="master-2"><Node><Properties><monitor.edit.time>1524096000000</monitor.edit.time><monitor.take.audio>false</monitor.take.audio><monitor.take.video>true</monitor.take.video><monitor.zoom.in.time>0</monitor.zoom.in.time><monitor.zoom.out.time>2540160000000</monitor.zoom.out.time></Properties></Node><Clips><Clip ObjectRef="15"/></Clips><Name>clip-b.mp4</Name><MasterClipChangeVersion>5</MasterClipChangeVersion></MasterClip>
  <VideoClip ObjectID="15"><Clip><Source ObjectRef="13"/><ClipID>master-clip-b</ClipID><InUse>false</InUse><InPoint>762048000000</InPoint><OutPoint>1532563200000</OutPoint></Clip></VideoClip>
</PremiereData>"#)
}

fn cross_dissolve_xml() -> String {
    // Exact native field names and match name come from the pinned Adobe-authored
    // `slideshow` corpus source (SHA-256
    // 9b44d3330c8789f6d5775257ff1b15ddcc0ab3c5353b289a81e9bb1e1df168a7).
    two_cuts_xml()
        .replace(
            "</ClipItems></ClipTrack>",
            "</ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef=\"16\"/></TrackItems></TransitionItems></ClipTrack>",
        )
        .replacen(
            "<SubClip ObjectRef=\"5\"/></ClipTrackItem>",
            "<SubClip ObjectRef=\"5\"/><TailTransition ObjectRef=\"16\"/></ClipTrackItem>",
            1,
        )
        .replace(
            "<SubClip ObjectRef=\"11\"/></ClipTrackItem>",
            "<SubClip ObjectRef=\"11\"/><HeadTransition ObjectRef=\"16\"/></ClipTrackItem>",
        )
        .replace(
            "</PremiereData>",
            r#"<VideoTransitionTrackItem ObjectID="16">
  <TransitionTrackItem>
    <TrackItem><Start>381024000000</Start><End>635040000000</End></TrackItem>
    <Alignment>127008000000</Alignment>
    <DisplayName>Cross Dissolve</DisplayName>
    <MatchName>AE.ADBE Cross Dissolve New</MatchName>
    <HasOutgoingClip>true</HasOutgoingClip>
    <HasIncomingClip>true</HasIncomingClip>
  </TransitionTrackItem>
  <StartPercent>0</StartPercent>
  <EndPercent>1</EndPercent>
  <SwitchSources>false</SwitchSources>
  <Reverse>false</Reverse>
</VideoTransitionTrackItem>
</PremiereData>"#,
        )
}

#[test]
fn adobe_cross_dissolve_fields_preserve_cut_topology_and_handles() {
    let (project, omissions) = inspect_project_with_omissions(&cross_dissolve_xml(), None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let track = &project.single_sequence().unwrap().video_tracks[0];
    let transition = &track.transitions[0];
    assert_eq!(
        transition.kind,
        crate::schema::PrVideoTransitionKind::CrossDissolve
    );
    assert_eq!(transition.start_ticks, 381_024_000_000);
    assert_eq!(transition.cut_ticks, 508_032_000_000);
    assert_eq!(transition.end_ticks, 635_040_000_000);
    assert!(transition.outgoing_clip.is_some());
    assert!(transition.incoming_clip.is_some());
}

#[test]
fn legacy_version_3_track_items_read_as_their_ranges() {
    // Premiere CS6 to CC 2015 save Version 3 track items that also carry the
    // item kind (`Type`), the `MediaType` GUID, `TrackIndex` and
    // `TrackRefCount`, in varying order, on clip and transition items. They
    // read as the current ranges, without a report.
    let video = "228cda18-3625-4d2d-951e-348879e4ed93";
    let clip = |range: &str| {
        format!(
            r#"<TrackItem Version="3"><TrackRefCount>1</TrackRefCount><TrackIndex>0</TrackIndex><MediaType>{video}</MediaType><Type>1</Type>{range}</TrackItem>"#
        )
    };
    let transition = |range: &str| {
        format!(
            r#"<TrackItem Version="3"><Node Version="1"></Node><TrackRefCount>1</TrackRefCount><TrackIndex>0</TrackIndex><Type>2</Type>{range}<MediaType>{video}</MediaType></TrackItem>"#
        )
    };
    let current = cross_dissolve_xml();
    let mut legacy = current.clone();
    for (item, saved) in [
        (
            "<TrackItem><End>508032000000</End></TrackItem>",
            clip("<End>508032000000</End>"),
        ),
        (
            "<TrackItem><Start>508032000000</Start><End>1278547200000</End></TrackItem>",
            clip("<End>1278547200000</End><Start>508032000000</Start>"),
        ),
        (
            "<TrackItem><Start>381024000000</Start><End>635040000000</End></TrackItem>",
            transition("<End>635040000000</End><Start>381024000000</Start>"),
        ),
    ] {
        assert_eq!(legacy.matches(item).count(), 1, "{item}");
        legacy = legacy.replace(item, &saved);
    }
    let ranges = |xml: &str| {
        let (project, omissions) = inspect_project_with_omissions(xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let sequence = project.single_sequence().unwrap();
        let clips: Vec<_> = sequence
            .video_occurrences()
            .map(|clip| clip.timeline_ticks())
            .collect();
        let transitions: Vec<_> = sequence.video_tracks[0]
            .transitions
            .iter()
            .map(|transition| {
                (
                    transition.start_ticks,
                    transition.cut_ticks,
                    transition.end_ticks,
                )
            })
            .collect();
        (clips, transitions)
    };
    assert_eq!(ranges(&legacy), ranges(&current));
    assert_eq!(
        ranges(&legacy),
        (
            vec![0..508_032_000_000, 508_032_000_000..1_278_547_200_000],
            vec![(381_024_000_000, 508_032_000_000, 635_040_000_000)]
        )
    );
}

/// A synthetic incoming-only Cross Dissolve (Legacy),
/// `VideoTransitionTrackItem:60`, over `start..end` with its cut at `start`.
/// As a Version 6 record, it saves no StartPercent, EndPercent, SwitchSources
/// or Reverse.
pub(super) fn version_6_dissolve(start: i64, end: i64) -> String {
    format!(
        r#"<VideoTransitionTrackItem ObjectID="60" Version="6"><TransitionTrackItem><TrackItem><Start>{start}</Start><End>{end}</End></TrackItem><Alignment>0</Alignment><DisplayName>Cross Dissolve (Legacy)</DisplayName><MatchName>AE.ADBE Cross Dissolve New</MatchName><HasOutgoingClip>false</HasOutgoingClip><HasIncomingClip>true</HasIncomingClip></TransitionTrackItem></VideoTransitionTrackItem>"#
    )
}

/// [`two_cuts_xml`] with [`version_6_dissolve`] over the first 15 frames of
/// its second clip.
fn version_6_dissolve_xml() -> String {
    two_cuts_xml()
        .replace(
            "</ClipItems></ClipTrack>",
            "</ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef=\"60\"/></TrackItems></TransitionItems></ClipTrack>",
        )
        .replace(
            "<SubClip ObjectRef=\"11\"/></ClipTrackItem>",
            "<SubClip ObjectRef=\"11\"/><HeadTransition ObjectRef=\"60\"/></ClipTrackItem>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                "{}</PremiereData>",
                version_6_dissolve(508_032_000_000, 635_040_000_000)
            ),
        )
}

#[test]
fn a_version_6_cross_dissolve_reads_its_omitted_controls_as_defaults() {
    let (project, omissions) =
        inspect_project_with_omissions(&version_6_dissolve_xml(), None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let transition = &sequence.video_tracks[0].transitions[0];
    assert_eq!(
        (
            transition.kind,
            transition.start_ticks,
            transition.cut_ticks,
            transition.end_ticks
        ),
        (
            crate::schema::PrVideoTransitionKind::CrossDissolve,
            508_032_000_000,
            508_032_000_000,
            635_040_000_000
        )
    );
    assert!(transition.outgoing_clip.is_none() && transition.incoming_clip.is_some());
    // Only a static Color Matte's head converts: on this video the
    // transition is reported, not dropped.
    let mut omissions = Vec::new();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions).unwrap();
    assert!(
        omissions
            .iter()
            .any(|omission| omission.record == "VideoTransitionTrackItem:60"
                && omission.kind == crate::OmissionKind::Omitted
                && omission.reason.contains("Cross Dissolve detected")
                && omission
                    .reason
                    .contains("only a Color Matte's head converts")),
        "{omissions:?}"
    );
    // A Version 5 record still saves each control, and so must have it; a
    // saved nondefault control keeps its rejection in either version.
    for (xml, field) in [
        (
            cross_dissolve_xml().replace("<StartPercent>0</StartPercent>", ""),
            "missing StartPercent",
        ),
        (
            version_6_dissolve_xml().replace(
                "</TransitionTrackItem></VideoTransitionTrackItem>",
                "</TransitionTrackItem><Reverse>true</Reverse></VideoTransitionTrackItem>",
            ),
            "reversed/switched transition is unsupported",
        ),
    ] {
        let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(field)),
            "{field}: {omissions:?}"
        );
    }
}

#[test]
fn clip_linked_transition_missing_from_track_membership_is_reported() {
    const MEMBERSHIP: &str =
        "<TransitionItems><TrackItems><TrackItem ObjectRef=\"16\"/></TrackItems></TransitionItems>";
    for replacement in [
        "",
        "<TransitionItems><TrackItems><TrackItem/></TrackItems></TransitionItems>",
    ] {
        let xml = cross_dissolve_xml().replace(MEMBERSHIP, replacement);
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let track = &project.single_sequence().unwrap().video_tracks[0];

        assert_eq!(track.items.len(), 2);
        assert!(track.transitions.is_empty());
        assert!(
            omissions.iter().any(|item| {
                item.scope == OmissionScope::Feature
                    && item.record == "VideoTransitionTrackItem:16"
                    && item.reason.contains("missing from TransitionItems")
            }),
            "{omissions:?}"
        );
    }
}

#[test]
fn pinned_cross_dissolve_project_preserves_native_links_and_source_handles() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_cross_dissolve_strict.prproj");
    let (project, omissions) = crate::PrProjectFile::load(source).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        sequence.id.as_deref(),
        Some("c8acf9c1-34b2-4086-9f55-d528950a7059")
    );
    assert_eq!(sequence.name, "Cross Dissolve");
    let track = &sequence.video_tracks[0];
    assert_eq!(track.items.len(), 2);
    assert_eq!(
        (
            track.clip(0).start_ticks,
            track.clip(0).end_ticks,
            track.clip(0).in_ticks,
            track.clip(0).out_ticks,
        ),
        (0, 508_032_000_000, 0, 508_032_000_000)
    );
    assert_eq!(
        (
            track.clip(1).start_ticks,
            track.clip(1).end_ticks,
            track.clip(1).in_ticks,
            track.clip(1).out_ticks,
        ),
        (
            508_032_000_000,
            1_016_064_000_000,
            127_008_000_000,
            635_040_000_000,
        )
    );
    assert_eq!(track.transitions.len(), 1);
    let transition = &track.transitions[0];
    assert_eq!(transition.start_ticks, 381_024_000_000);
    assert_eq!(transition.cut_ticks, 508_032_000_000);
    assert_eq!(transition.end_ticks, 635_040_000_000);
    assert!(transition.outgoing_clip.is_some());
    assert!(transition.incoming_clip.is_some());
}

#[test]
fn cross_dissolve_rejects_contradictory_native_topology() {
    let xml = cross_dissolve_xml().replace(
        "<HasIncomingClip>true</HasIncomingClip>",
        "<HasIncomingClip>false</HasIncomingClip>",
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(project.single_sequence().unwrap().video_tracks[0]
        .transitions
        .is_empty());
    assert!(
        omissions.iter().any(|item| {
            item.scope == OmissionScope::Feature
                && item
                    .reason
                    .contains("links conflict with HasOutgoingClip/HasIncomingClip")
        }),
        "{omissions:?}"
    );
}

#[test]
fn cross_dissolve_rejects_missing_source_handles() {
    let xml = cross_dissolve_xml()
        .replace("<InPoint>762048000000</InPoint>", "<InPoint>0</InPoint>")
        .replace(
            "<OutPoint>1532563200000</OutPoint>",
            "<OutPoint>770515200000</OutPoint>",
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(project.single_sequence().unwrap().video_tracks[0]
        .transitions
        .is_empty());
    assert!(
        omissions.iter().any(|item| {
            item.scope == OmissionScope::Feature
                && item
                    .reason
                    .contains("incoming clip lacks the required source handle")
        }),
        "{omissions:?}"
    );
}

#[test]
fn media_id_and_uid_namespaces_do_not_alias() {
    let xml = two_cuts_xml()
        .replace(
            "<Media ObjectUID=\"media-2\">",
            "<Media ObjectID=\"media-1\">",
        )
        .replace(
            "<Media ObjectURef=\"media-2\"/>",
            "<Media ObjectRef=\"media-1\"/>",
        );
    let project = crate::format::inspect_project_with_media(&xml, None).unwrap();
    assert_eq!(project.media.len(), 2);
    let sequence = project.single_sequence().unwrap();
    let sources: Vec<_> = sequence
        .video_occurrences()
        .map(|clip| project.media(clip).unwrap().name())
        .collect();
    assert_eq!(sources, ["source.mp4", "clip-b.mp4"]);
}

#[test]
fn sequence_and_media_frame_rates_come_from_their_own_records() {
    let (sequence_rate, media_rate) = (FrameRate::Fps24000Over1001, FrameRate::Fps25);
    let clip = 120 * sequence_rate.ticks_per_frame();
    let xml = one_clip_xml(OneClip {
        sequence_frame: sequence_rate.ticks_per_frame(),
        media_frame: media_rate.ticks_per_frame(),
        media_duration: 240 * media_rate.ticks_per_frame(),
        end: clip,
        out_point: clip,
        ..OneClip::default()
    });
    let project = crate::format::inspect_project_with_media(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.frame_rate, sequence_rate);
    let occurrence = sequence.video_occurrences().next().unwrap();
    let video = project.media(occurrence).unwrap().video.as_ref().unwrap();
    assert_eq!(video.frame_rate, media_rate.into());

    // An unlisted sequence rate fails its timeline. A nonpositive source
    // duration omits that media; positive physical source durations are checked
    // against exact container timing at import preparation.
    for (timing, reason, omitted) in [
        (
            OneClip {
                sequence_frame: 123,
                ..OneClip::default()
            },
            "VideoTrackGroup:1: unsupported video frame rate",
            false,
        ),
        (
            OneClip {
                media_frame: 0,
                ..OneClip::default()
            },
            "source frame duration must be positive",
            true,
        ),
    ] {
        let error = inspect_project(&one_clip_xml(timing), None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(reason)
                && error.contains("no convertible video or audio occurrences") == omitted,
            "{error}"
        );
    }
}

#[test]
fn reader_preserves_exact_cut_ranges_and_sorts_track_items() {
    let xml = two_cuts_xml();
    let reversed = xml.replace(
        "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"9\"/>",
        "<TrackItem ObjectRef=\"9\"/><TrackItem ObjectRef=\"3\"/>",
    );
    assert_ne!(xml, reversed);
    for source in [xml, reversed] {
        let sequence = inspect_project(&source, None).unwrap();
        assert_eq!(sequence.id.as_deref(), Some("sequence-1"));
        let ranges: Vec<_> = sequence
            .video_occurrences()
            .map(|clip| {
                (
                    clip.start_ticks,
                    clip.end_ticks,
                    clip.in_ticks,
                    clip.out_ticks,
                )
            })
            .collect();
        assert_eq!(
            ranges,
            [
                (0, 508_032_000_000, 0, 508_032_000_000),
                (
                    508_032_000_000,
                    1_278_547_200_000,
                    762_048_000_000,
                    1_532_563_200_000
                ),
            ]
        );
    }
    let overlapping = two_cuts_xml()
        .replace("<Start>508032000000</Start>", "<Start>499564800000</Start>")
        .replace("<End>1278547200000</End>", "<End>1270080000000</End>");
    let (partial, omissions) = inspect_project_with_omissions(&overlapping, None).unwrap();
    assert_eq!(
        partial
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert!(omissions
        .iter()
        .any(|item| item.scope == OmissionScope::Occurrence && item.reason.contains("overlaps")));
}

#[test]
fn broken_occurrence_link_does_not_discard_its_track() {
    let xml = two_cuts_xml().replace(
        "<TrackItem ObjectRef=\"9\"/>",
        "<TrackItem ObjectRef=\"999\"/>",
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.scope == OmissionScope::Occurrence
                && item.reason.contains("missing reference")),
        "{omissions:?}"
    );
}

#[test]
fn unknown_feature_and_bad_occurrence_leave_valid_cut() {
    let xml = two_cuts_xml()
        .replacen(
            "<ClipTrackItem><ComponentOwner>",
            "<ClipTrackItem><UnknownEffect/><ComponentOwner>",
            1,
        )
        .replace(
            "<VideoClip ObjectID=\"12\"><Clip><Source",
            "<VideoClip ObjectID=\"12\"><Clip><PlaybackSpeed>garbage</PlaybackSpeed><Source",
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let clips: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].timeline_ticks(), 0..508_032_000_000);
    assert!(
        omissions
            .iter()
            .any(|item| item.scope == OmissionScope::Feature
                && item.reason.contains("UnknownEffect")),
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.scope == OmissionScope::Occurrence
                && item.reason.contains("PlaybackSpeed")),
        "{omissions:?}"
    );
}

#[test]
fn pinned_adobe_vhsvertical_selected_sequence_defines_native_reverse_fields() {
    // Byte identity is pinned in tests/manifest.json (SHA-256
    // f16803da67c3e18eca9f102b7410a62a9609cf723ae8b1351c4b99169a5a1ef0).
    let bytes = include_bytes!("../../../tests/fixtures/vhsvertical.prproj");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut xml)
        .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let record = |tag, attribute, value| {
        document
            .descendants()
            .find(|node| node.has_tag_name(tag) && node.attribute(attribute) == Some(value))
            .unwrap()
    };
    let reference = |node: roxmltree::Node<'_, '_>, tag, attribute| {
        node.descendants()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.attribute(attribute))
            .unwrap()
            .to_owned()
    };

    let sequence = record(
        "Sequence",
        "ObjectUID",
        "8ac6ccdd-108a-4624-ba23-f17754c13e5b",
    );
    assert_eq!(reference(sequence, "Second", "ObjectRef"), "313");
    assert!(sequence
        .descendants()
        .filter(|node| node.has_tag_name("Second"))
        .any(|node| node.attribute("ObjectRef") == Some("314")));
    let video_group = record("VideoTrackGroup", "ObjectID", "314");
    assert!(video_group.descendants().any(|node| {
        node.has_tag_name("Track")
            && node.attribute("ObjectURef") == Some("63cc7b3e-8509-47cb-a8b8-eb54bffcd6a6")
    }));
    let track = record(
        "VideoClipTrack",
        "ObjectUID",
        "63cc7b3e-8509-47cb-a8b8-eb54bffcd6a6",
    );
    assert_eq!(reference(track, "TrackItem", "ObjectRef"), "482");
    let item = record("VideoClipTrackItem", "ObjectID", "482");
    assert_eq!(reference(item, "SubClip", "ObjectRef"), "858");
    let subclip = record("SubClip", "ObjectID", "858");
    assert_eq!(reference(subclip, "Clip", "ObjectRef"), "3871");
    let video_clip = record("VideoClip", "ObjectID", "3871");
    let clip = video_clip
        .children()
        .find(|node| node.has_tag_name("Clip"))
        .unwrap();
    let value = |name| {
        clip.children()
            .find(|child| child.has_tag_name(name))
            .and_then(|child| child.text())
    };
    assert_eq!(
        value("ClipID"),
        Some("61912011-c78a-4c0a-a3ec-85de7393e39f")
    );
    assert_eq!(value("PlaybackSpeed"), Some("0.90500000000000003"));
    assert_eq!(value("PlayBackwards"), Some("true"));
    assert_eq!(value("InPoint"), Some("1678182105600"));
    assert_eq!(value("OutPoint"), Some("3381028402749"));
    assert_eq!(reference(clip, "Source", "ObjectRef"), "268");
    let source = record("VideoMediaSource", "ObjectID", "268");
    assert_eq!(
        reference(source, "Media", "ObjectURef"),
        "7119ff4d-362b-4d32-8264-d46a9459a5a2"
    );
    let media = record("Media", "ObjectUID", "7119ff4d-362b-4d32-8264-d46a9459a5a2");
    assert_eq!(
        media
            .children()
            .find(|node| node.has_tag_name("RelativePath"))
            .and_then(|node| node.text()),
        Some("./media/vhsvertical_vhs_bg.mp4")
    );
}

#[test]
fn isolated_reverse_fixture_has_only_the_reviewed_native_element_diff() {
    fn decode(bytes: &[u8]) -> String {
        let mut xml = String::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_string(&mut xml)
            .unwrap();
        xml
    }

    let base = decode(include_bytes!(
        "../../../tests/fixtures/feature_nonzero_source_trim_strict.prproj"
    ));
    let reverse = decode(include_bytes!(
        "../../../tests/fixtures/feature_constant_reverse_0_905_strict.prproj"
    ));
    let base_name = "<Name>Nonzero source trim</Name>";
    let base_clip = concat!(
        "<OutPoint>1270080000000</OutPoint>\n",
        "\t\t\t<InPoint>762048000000</InPoint>"
    );
    assert_eq!(base.matches(base_name).count(), 3);
    assert_eq!(base.matches("<End>508032000000</End>").count(), 1);
    assert_eq!(base.matches(base_clip).count(), 1);

    let expected = base
        .replace(base_name, "<Name>Constant reverse 0.905x</Name>")
        .replace("<End>508032000000</End>", "<End>1879718400000</End>")
        .replace(
            base_clip,
            concat!(
                "<PlaybackSpeed>0.90500000000000003</PlaybackSpeed>\n",
                "\t\t\t<PlayBackwards>true</PlayBackwards>\n",
                "\t\t\t<OutPoint>2463193152000</OutPoint>\n",
                "\t\t\t<InPoint>762048000000</InPoint>"
            ),
        );
    assert_eq!(reverse, expected);
}

#[test]
fn native_constant_speed_reverse_keeps_the_video_occurrence() {
    // `PlaybackSpeed` plus `PlayBackwards` is the native shape observed on the
    // SHA-pinned Adobe `vhsvertical` project. Source bounds stay ascending.
    let xml = SOURCE
        .replace(
            "<Source ObjectRef=\"7\"/>",
            "<PlaybackSpeed>0.5</PlaybackSpeed><PlayBackwards>true</PlayBackwards><Source ObjectRef=\"7\"/>",
        )
        .replace(
            "<OutPoint>1270080000000</OutPoint>",
            "<OutPoint>635040000000</OutPoint>",
        );

    let sequence = inspect_project(&xml, None).unwrap();
    let clips: Vec<_> = sequence.video_occurrences().collect();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].timeline_ticks(), 0..1_270_080_000_000);
    assert_eq!(clips[0].source_ticks(), 0..635_040_000_000);
}

#[test]
fn unsupported_semantics_fail_with_native_context() {
    for (xml, expected) in [
        (
            // 29.97 fps is supported, but this source still has 30 fps-aligned
            // clip boundaries; reject the malformed timeline, not the rate.
            SOURCE.replace(
                "<FrameRate>8467200000</FrameRate></TrackGroup>",
                "<FrameRate>8475667200</FrameRate></TrackGroup>",
            ),
            "timeline end must align",
        ),
        (
            SOURCE.replace("<End>1270080000000</End>", "<End>1016064000000</End>"),
            "source span",
        ),
        (
            SOURCE.replace("<InPoint>0</InPoint>", "<InPoint>-1</InPoint>"),
            "ranges",
        ),
        (
            SOURCE.replace("<SubClip ObjectRef=\"5\"/>", "<SubClip ObjectRef=\"3\"/>"),
            "cyclic",
        ),
        (
            SOURCE.replace("<PixelAspectRatio>1,1", "<PixelAspectRatio>2,1"),
            "non-square occurrence pixels (2:1) are unsupported",
        ),
        (
            SOURCE.replace(
                "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
                "<FrameRect>garbage</FrameRect></VideoStream>",
            ),
            "invalid FrameRect",
        ),
        (
            SOURCE.replace(
                "<FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>",
                "<FrameRect>0,0,1280,720</FrameRect><PixelAspectRatio>",
            ),
            "occurrence FrameRect must match the sequence canvas",
        ),
    ] {
        let error = inspect_project(&xml, None).unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "expected {expected}: {error}"
        );
    }

    let larger_source = SOURCE.replace(
        "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
        "<FrameRect>0,0,3840,2160</FrameRect></VideoStream>",
    );
    let project = crate::format::inspect_project_with_media(&larger_source, None).unwrap();
    let video = project
        .media
        .values()
        .next()
        .unwrap()
        .video
        .as_ref()
        .unwrap();
    assert_eq!((video.width, video.height), (3840, 2160));
}

/// `xml` with its sequence frame and the frame of its first occurrence, the
/// clip `SubClip:5` plays, set to `frame`; each source keeps its own frame.
fn on_canvas(xml: &str, frame: &str) -> String {
    [
        "</TrackGroup><FrameRect>0,0,1920,1080</FrameRect>",
        "<SubClip ObjectRef=\"5\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect>",
    ]
    .into_iter()
    .fold(xml.to_owned(), |xml, stored| {
        assert_eq!(xml.matches(stored).count(), 1, "{stored}");
        xml.replacen(stored, &stored.replace("0,0,1920,1080", frame), 1)
    })
}

#[test]
fn custom_canvases_keep_their_size_and_place_the_source_by_its_own_frame() {
    // Portrait, 720p, square, ultrawide and odd canvases around the 1920x1080
    // source, and the 1080p control. Default Motion anchors the source at its
    // own centre and places it at the canvas centre; nothing rescales it.
    for [width, height] in [
        [1080, 1920],
        [1280, 720],
        [1000, 1000],
        [2560, 1080],
        [1001, 777],
        [1920, 1080],
    ] {
        let xml = on_canvas(SOURCE, &format!("0,0,{width},{height}"));
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(omissions.is_empty(), "{width}x{height}: {omissions:?}");
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.dimensions(), [width, height]);
        let document = project_document_with_media(sequence, &project.media);
        assert_eq!(
            document["dimensions"],
            json!({"width": width, "height": height})
        );
        let layers = document["composition"]["layers"].as_array().unwrap();
        let (video, canvas) = (&layers[0], &layers[1]);
        assert_eq!(
            video["source"]["sourceRect"],
            json!({"x": 0.0, "y": 0.0, "width": 1920.0, "height": 1080.0})
        );
        assert_eq!(video["transform"]["anchorPoint"], json!([960.0, 540.0]));
        assert_eq!(
            video["transform"]["position"],
            json!([f64::from(width) / 2.0, f64::from(height) / 2.0]),
            "{width}x{height}"
        );
        assert_eq!(video["transform"]["scale"], json!([100.0, 100.0]));
        assert_eq!(
            canvas["rect"]["size"],
            json!([f64::from(width), f64::from(height)])
        );
    }
}

#[test]
fn malformed_or_non_square_frames_are_rejected() {
    let invalid = [
        "0,0,0,1920",
        "0,0,1080,-1920",
        "0,0,4294967296,1920",
        "8,0,1088,1920",
        "0,8,1080,1928",
        "0,0,1080",
        "0,0,1080,1920,0",
        "0,0,1080.5,1920",
    ];
    for frame in invalid {
        let error = inspect_project(&on_canvas(SOURCE, frame), None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("VideoTrackGroup:1: invalid FrameRect"),
            "{frame}: {error}"
        );
        let source = SOURCE.replace(
            "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
            &format!("<FrameRect>{frame}</FrameRect></VideoStream>"),
        );
        let error = inspect_project(&source, None).unwrap_err().to_string();
        assert!(
            error.contains("VideoStream:8: invalid FrameRect"),
            "{frame}: {error}"
        );
    }
    for (xml, expected) in [
        (
            SOURCE.replace(
                "</TrackGroup><FrameRect>0,0,1920,1080</FrameRect>",
                "</TrackGroup><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>2,1</PixelAspectRatio>",
            ),
            "non-square sequence pixels (2:1) are unsupported",
        ),
        (
            SOURCE.replace(
                "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
                "<FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>2,1</PixelAspectRatio></VideoStream>",
            ),
            "VideoStream:8: non-square source pixels (2:1) are unsupported",
        ),
    ] {
        assert_ne!(xml, SOURCE);
        let error = inspect_project(&xml, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn equal_pixel_aspect_pairs_read_as_square_pixels() {
    // Premiere saves square pixels as any equal pair on the sequence, the
    // source stream and the occurrence. An anamorphic HDV pair names its
    // ratio, and a malformed one names the record.
    let declared = |ratio: &str| {
        SOURCE
            .replace(
                "</TrackGroup><FrameRect>0,0,1920,1080</FrameRect>",
                &format!("</TrackGroup><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>{ratio}</PixelAspectRatio>"),
            )
            .replace(
                "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
                &format!("<FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>{ratio}</PixelAspectRatio></VideoStream>"),
            )
            .replace(
                "<PixelAspectRatio>1,1</PixelAspectRatio>",
                &format!("<PixelAspectRatio>{ratio}</PixelAspectRatio>"),
            )
    };
    for square in ["1,1", "1280,1280", "1920,1920", "1000000,1000000"] {
        let xml = declared(square);
        assert_eq!(xml.matches(&format!(">{square}<")).count(), 3, "{square}");
        let sequence = inspect_project(&xml, None).unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1, "{square}");
    }
    for (ratio, expected) in [
        (
            "1920,1440",
            "non-square sequence pixels (1920:1440) are unsupported",
        ),
        (
            "1920,0",
            "VideoTrackGroup:1: invalid PixelAspectRatio \"1920,0\"",
        ),
    ] {
        let error = inspect_project(&declared(ratio), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{ratio}: {error}");
    }
}

#[test]
fn an_occurrence_frame_other_than_its_sequence_canvas_omits_only_that_occurrence() {
    // A portrait sequence whose first clip carries its frame; the second clip
    // keeps the 1080p frame it would have had in another sequence, or a frame
    // that is no size at the origin.
    let portrait = on_canvas(&two_cuts_xml(), "0,0,1080,1920");
    let second = "<SubClip ObjectRef=\"11\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect>";
    assert_eq!(portrait.matches(second).count(), 1);
    for (frame, reason) in [
        (
            "0,0,1920,1080",
            "VideoClipTrackItem:9: occurrence FrameRect must match the sequence canvas: 1920x1080 on a 1080x1920 sequence",
        ),
        ("8,0,1088,1920", "VideoClipTrackItem:9: invalid FrameRect"),
        ("0,0,1080,0", "VideoClipTrackItem:9: invalid FrameRect"),
    ] {
        let xml = portrait.replacen(
            second,
            &format!("<SubClip ObjectRef=\"11\"/></ClipTrackItem><FrameRect>{frame}</FrameRect>"),
            1,
        );
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.dimensions(), [1080, 1920]);
        let kept: Vec<_> = sequence
            .video_occurrences()
            .map(|clip| clip.id.as_deref().unwrap())
            .collect();
        assert_eq!(kept, ["VideoClipTrackItem:3"], "{frame}");
        assert_eq!(omissions.len(), 1, "{frame}: {omissions:?}");
        assert_eq!(
            (omissions[0].scope, omissions[0].record.as_str()),
            (OmissionScope::Occurrence, "9")
        );
        assert!(
            omissions[0].reason.contains(reason),
            "{frame}: {}",
            omissions[0].reason
        );
    }
}

#[test]
fn load_rejects_sequence_with_no_convertible_occurrences() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.prproj");
    let invalid = SOURCE.replace("<End>1270080000000</End>", "<End>1016064000000</End>");
    use std::io::Write as _;
    let mut zip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    zip.write_all(invalid.as_bytes()).unwrap();
    std::fs::write(&path, zip.finish().unwrap()).unwrap();
    let error = crate::format::PrProjectFile::load_selected(&path, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no convertible timelines"), "{error}");
    assert!(
        error.contains("source span") && error.contains("sequence"),
        "{error}"
    );
}

#[test]
fn sequence_selection_is_explicit_and_unambiguous() {
    let xml = SOURCE.replace(
        "</PremiereData>",
        "<Sequence ObjectUID='sequence-2'><Name>Other</Name></Sequence></PremiereData>",
    );
    assert!(inspect_project(&xml, None).is_err());
    assert!(inspect_project(&xml, Some("missing")).is_err());
    assert!(inspect_project(&xml, Some("sequence-1")).is_ok());

    let prefixed = SOURCE
        .replacen("<PremiereData", "<PremiereData xmlns:p='urn:test'", 1)
        .replace("<Sequence ObjectUID", "<p:Sequence p:ObjectUID")
        .replace("</Sequence>", "</p:Sequence>");
    assert!(inspect_project(&prefixed, None).is_ok());
}

#[test]
fn source_effects_and_ambiguous_optional_defaults_are_not_dropped() {
    let xml = SOURCE.replace("<Clip ObjectRef=\"6\"/>", "<Clip ObjectRef=\"6\"/><MasterClip ObjectURef='master-1'/>")
        .replace("</PremiereData>", "<MasterClip ObjectUID='master-1'><Clips><Clip ObjectRef='9'/></Clips></MasterClip><VideoClip ObjectID='9'><Clip><Source ObjectRef='7'/></Clip></VideoClip></PremiereData>");
    assert!(inspect_project(&xml, None).is_ok());
    let effected = xml.replace(
        "<MasterClip ObjectUID='master-1'>",
        "<MasterClip ObjectUID='master-1'><VideoComponentChain/>",
    );
    let (_, omissions) = inspect_project_with_omissions(&effected, None).unwrap();
    assert!(omissions
        .iter()
        .any(|item| item.record == "MasterClip:master-1"
            && item.reason.contains("VideoComponentChain")));
    let duplicate = SOURCE.replace(
        "<DefaultMotion>true</DefaultMotion>",
        "<DefaultMotion>true</DefaultMotion><DefaultMotion>false</DefaultMotion>",
    );
    let error = inspect_project_with_omissions(&duplicate, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate field `DefaultMotion`"), "{error}");

    let colliding_identity = xml
        .replace(
            "<VideoClip ObjectID='9'><Clip><Source ObjectRef='7'/>",
            "<VideoClip ObjectID='9'><Clip><Source ObjectURef='7'/>",
        )
        .replace(
            "</PremiereData>",
            "<VideoMediaSource ObjectUID='7'><MediaSource><Media ObjectURef='media-1'/></MediaSource><OriginalDuration>2540160000000</OriginalDuration></VideoMediaSource></PremiereData>",
        );
    let error = inspect_project(&colliding_identity, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("source identity mismatch"), "{error}");
}

#[test]
fn populated_clip_markers_are_not_dropped() {
    let xml = SOURCE.replace("<Clip><Source", "<Clip><MarkerOwner><Markers ObjectRef=\"90\"/></MarkerOwner><Source")
        .replace("</PremiereData>", "<Markers ObjectID=\"90\"><ByGUID>byGUID</ByGUID><DVAMarker><Name>must-survive</Name></DVAMarker></Markers></PremiereData>");
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions
        .iter()
        .any(|item| item.reason.contains("clip markers not converted")));

    let wrong_target = SOURCE.replace(
        "<Clip><Source",
        "<Clip><MarkerOwner><Markers ObjectRef=\"8\"/></MarkerOwner><Source",
    );
    let (_, omissions) = inspect_project_with_omissions(&wrong_target, None).unwrap();
    assert!(
        omissions
            .iter()
            .any(|item| item.reason.contains("expected Markers")),
        "{omissions:?}"
    );
}

#[test]
fn xsi_nil_cannot_erase_semantic_content() {
    const XSI: &str = "xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance' xsi:nil='true'";
    for (index, xml) in [
        SOURCE.replace(
            "<VideoStream ObjectRef=\"8\"/>",
            &format!("<VideoStream ObjectRef=\"8\"/><AudioStream {XSI} ObjectRef=\"8\"/>"),
        ),
        SOURCE.replace(
            "<DefaultMotion>true</DefaultMotion>",
            &format!("<DefaultMotion {XSI}>false</DefaultMotion>"),
        ),
        SOURCE.replace(
            "<Clip><Source",
            &format!("<Clip><PlaybackSpeed {XSI}>2</PlaybackSpeed><Source"),
        ),
        SOURCE.replace(
            "<Clip><Source",
            &format!("<Clip><MarkerOwner {XSI}><Markers ObjectRef=\"8\"/></MarkerOwner><Source"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let error = inspect_project(&xml, None).unwrap_err().to_string();
        assert!(
            error.contains("xsi:nil is unsupported"),
            "case {index}: {error}"
        );
    }
}

#[test]
fn source_duration_and_role_specific_color_profiles_are_not_rewritten_silently() {
    for (name, xml, expected) in [
        ("source-duration", SOURCE.replace("<OriginalDuration>2540160000000</OriginalDuration>", "<OriginalDuration>5080320000000</OriginalDuration>"), "source OriginalDuration"),
        ("source-profile", SOURCE.replace("<VideoStream ObjectID=\"8\">", r#"<VideoStream ObjectID="8"><OriginalColorSpace>{"baseColorProfile":{"colorProfileName":"BT.709,8-bit,Display-Referred"},"baseProfileType":1}</OriginalColorSpace>"#), "OriginalColorSpace profile"),
        ("sequence-profile", SOURCE.replace("<VideoTrackGroup ObjectID=\"1\">", r#"<VideoTrackGroup ObjectID="1"><OutputColorSpace>{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"BT.709,32f,Display-Referred"},"baseProfileType":1}</OutputColorSpace>"#), "OutputColorSpace profile"),
        ("profile-payload", SOURCE.replace("<VideoStream ObjectID=\"8\">", r#"<VideoStream ObjectID="8"><OriginalColorSpace>{"baseColorProfile":{"colorProfileData":"different","colorProfileName":"BT.709,32f,Display-Referred"},"baseProfileType":1}</OriginalColorSpace>"#), "OriginalColorSpace profile"),
    ] {
        let error = inspect_project(&xml, None).unwrap_err();
        assert!(error.to_string().contains(expected), "{name}: {error}");
    }
}

fn with_output_color_space(profile: &str) -> String {
    SOURCE.replace(
        "<VideoTrackGroup ObjectID=\"1\">",
        &format!("<VideoTrackGroup ObjectID=\"1\"><OutputColorSpace>{profile}</OutputColorSpace>"),
    )
}

fn with_original_color_space(profile: &str) -> String {
    SOURCE.replace(
        "<VideoStream ObjectID=\"8\">",
        &format!("<VideoStream ObjectID=\"8\"><OriginalColorSpace>{profile}</OriginalColorSpace>"),
    )
}

#[test]
fn premiere_26_5_short_sequence_profile_reads_as_sdr() {
    // Exact OutputColorSpace text of 15 of the 19 evaluation-corpus projects
    // that Premiere 26.5.1 last saved, for example the upgraded
    // prepared/adobe-cinemagraph/native-26.5.1/project.prproj.
    let xml = with_output_color_space(
        r#"{"baseColorProfile":{"colorProfileName":"BT.709,8-bit,Display-Referred"},"baseProfileType":1}"#,
    );
    assert_eq!(
        format!("{:?}", inspect_project(&xml, None).unwrap()),
        format!("{:?}", inspect_project(SOURCE, None).unwrap())
    );
}

#[test]
fn premiere_26_5_short_rgb_full_source_profile_reads_as_sdr() {
    // Exact OriginalColorSpace text of the PNG still in 6 of the 19
    // evaluation-corpus projects that Premiere 26.5.1 last saved, for example
    // the upgraded prepared/adobe-cinemagraph/native-26.5.1/project.prproj.
    let xml = with_original_color_space(
        r#"{"baseColorProfile":{"colorProfileName":"BT.709 RGB Full"},"baseProfileType":1}"#,
    );
    assert_eq!(
        format!("{:?}", inspect_project(&xml, None).unwrap()),
        format!("{:?}", inspect_project(SOURCE, None).unwrap())
    );
}

#[test]
fn short_color_profiles_with_one_changed_value_still_reject() {
    const UNKNOWN_FIELD: &str = "unknown field `unexpected`";
    const SEQUENCE: &str = "unsupported OutputColorSpace profile for its native role";
    for (profile, expected) in [
        (
            r#"{"baseColorProfile":{"colorProfileName":"BT.709,10-bit,Display-Referred"},"baseProfileType":1}"#,
            SEQUENCE,
        ),
        (
            r#"{"baseColorProfile":{"colorProfileName":"BT.709,8-bit,Display-Referred"},"baseProfileType":2}"#,
            SEQUENCE,
        ),
        // Saved by Premiere 25.2 and 25.5 (corpus travel_days, practice_files_transcription_magic).
        (
            r#"{"baseColorProfile":{"colorProfileData":"AQAAAGQAAAA=","colorProfileName":"BT.709,8-bit,Display-Referred"},"baseProfileType":1}"#,
            SEQUENCE,
        ),
        (
            r#"{"baseColorProfile":{"colorProfileName":"BT.709,8-bit,Display-Referred"},"baseProfileType":1,"unexpected":0}"#,
            UNKNOWN_FIELD,
        ),
    ] {
        let error = inspect_project(&with_output_color_space(profile), None).unwrap_err();
        assert!(error.to_string().contains(expected), "{profile}: {error}");
    }
    const MEDIA: &str = "unsupported OriginalColorSpace profile for its native role";
    for (profile, expected) in [
        (
            r#"{"baseColorProfile":{"colorProfileName":"BT.709,10-bit,Display-Referred"},"baseProfileType":1}"#,
            MEDIA,
        ),
        (
            r#"{"baseColorProfile":{"colorProfileName":"BT.709 RGB Full"},"baseProfileType":2}"#,
            MEDIA,
        ),
        (
            r#"{"baseColorProfile":{"colorProfileName":"BT.709 RGB Full","unexpected":0},"baseProfileType":1}"#,
            UNKNOWN_FIELD,
        ),
    ] {
        let error = inspect_project(&with_original_color_space(profile), None).unwrap_err();
        assert!(error.to_string().contains(expected), "{profile}: {error}");
    }
}

/// The `OriginalColorSpace` text that Premiere 26.5.1 saved for 10-bit BT.2020
/// HLG and PQ `hvc1` sources on a Rec. 709 sequence (`oracle/M2/hdr/facts.md`).
const HDR_SOURCE_PROFILES: [&str; 2] = [
    r#"{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"BT.2100 HLG,10-bit,Display-Referred"},"baseProfileType":1}"#,
    r#"{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"BT.2100 PQ,10-bit,Display-Referred"},"baseProfileType":1}"#,
];

#[test]
fn premiere_26_5_hdr_source_profiles_read_as_video_sources() {
    // Inspection of the file, not the saved profile, decides the pass-through
    // report, so the record reads like an SDR one.
    for profile in HDR_SOURCE_PROFILES {
        assert_eq!(
            format!(
                "{:?}",
                inspect_project(&with_original_color_space(profile), None).unwrap()
            ),
            format!("{:?}", inspect_project(SOURCE, None).unwrap()),
            "{profile}"
        );
    }
}

#[test]
fn hdr_profiles_reject_on_a_sequence_or_without_their_saved_data() {
    for profile in HDR_SOURCE_PROFILES {
        // Sequence-level HDR working spaces stay rejected.
        let error = inspect_project(&with_output_color_space(profile), None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported OutputColorSpace profile for its native role"),
            "{profile}: {error}"
        );
        let short = profile.replace(r#""colorProfileData":"AQAAAP////8=","#, "");
        let error = inspect_project(&with_original_color_space(&short), None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported OriginalColorSpace profile for its native role"),
            "{short}: {error}"
        );
    }
}

#[test]
fn pinned_half_speed_frame_blending_fixture_has_exact_native_fields() {
    // The source/media byte identities are pinned in tests/manifest.json.
    let bytes =
        include_bytes!("../../../tests/fixtures/feature_frame_blending_half_speed_strict.prproj");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut xml)
        .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();

    fn child_text<'input>(
        node: roxmltree::Node<'input, 'input>,
        name: &str,
    ) -> Option<&'input str> {
        node.children()
            .find(|child| child.has_tag_name(name))
            .and_then(|child| child.text())
    }
    let sequence = document
        .descendants()
        .find(|node| {
            node.has_tag_name("Sequence")
                && node.attribute("ObjectUID") == Some("c8acf9c1-34b2-4086-9f55-d528950a7059")
        })
        .unwrap();
    assert_eq!(
        child_text(sequence, "Name"),
        Some("Frame blending half speed")
    );

    let media = document
        .descendants()
        .find(|node| {
            node.has_tag_name("Media")
                && node.attribute("ObjectUID") == Some("bb3ebba2-51a3-44a7-b1bb-4d97fcee09b1")
        })
        .unwrap();
    assert_eq!(
        child_text(media, "RelativePath"),
        Some("feature_timecoded_source.mp4")
    );

    let clip = document
        .descendants()
        .find(|node| {
            node.has_tag_name("Clip")
                && child_text(*node, "ClipID") == Some("0ad61a94-8103-4385-bc5d-86f00a438773")
        })
        .unwrap();
    assert_eq!(child_text(clip, "PlaybackSpeed"), Some("0.5"));
    assert_eq!(child_text(clip, "InPoint"), Some("0"));
    assert_eq!(child_text(clip, "OutPoint"), Some("254016000000"));
    assert_eq!(
        child_text(clip.parent().unwrap(), "TimeInterpolationType"),
        Some("1")
    );

    let track_item = document
        .descendants()
        .find(|node| node.has_tag_name("VideoClipTrackItem"))
        .unwrap();
    let timeline = track_item
        .descendants()
        .find(|node| node.has_tag_name("TrackItem"))
        .unwrap();
    assert_eq!(child_text(timeline, "End"), Some("508032000000"));
}

#[test]
fn adobe_time_interpolation_maps_to_editable_frame_blending() {
    for (native, expected) in [
        ("0", None),
        ("1", Some(FrameBlendingMode::Simple)),
        ("2", Some(FrameBlendingMode::OpticalFlow)),
    ] {
        let xml = SOURCE.replace(
            "</Clip></VideoClip>",
            &format!("</Clip><TimeInterpolationType>{native}</TimeInterpolationType></VideoClip>"),
        );
        let sequence = inspect_project(&xml, None).unwrap();
        let occurrence = sequence.video_occurrences().next().unwrap();
        assert_eq!(occurrence.frame_blending, expected);
    }
}

#[test]
fn unsupported_time_interpolation_is_diagnostic_and_uses_sampling() {
    for value in ["-1", "3", "blend"] {
        let xml = SOURCE.replace(
            "</Clip></VideoClip>",
            &format!("</Clip><TimeInterpolationType>{value}</TimeInterpolationType></VideoClip>"),
        );
        let (sequence, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(
            sequence
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .next()
                .unwrap()
                .frame_blending,
            None
        );
        assert!(omissions.iter().any(|item| {
            item.scope == OmissionScope::Feature
                && item.reason.contains("unsupported TimeInterpolationType")
                && item.reason.contains("using frame sampling")
        }));
    }
}

#[test]
fn adobe_playback_speed_must_be_positive_and_match_the_source_span() {
    for value in ["1", "1.0", "0", "-1", "2", "NaN", "Infinity"] {
        let xml = SOURCE.replace(
            "<Clip><Source",
            &format!("<Clip><PlaybackSpeed>{value}</PlaybackSpeed><Source"),
        );
        let result = inspect_project(&xml, None);
        if ["1", "1.0"].contains(&value) {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert!(
                error.to_string().contains(if value == "2" {
                    "source span"
                } else {
                    "PlaybackSpeed must be finite and positive"
                }),
                "{value}: {error}"
            );
        }
    }
}

fn two_tracks() -> String {
    let source = SOURCE;
    let document = roxmltree::Document::parse(source).unwrap();
    let clip = document
        .descendants()
        .find(|node| node.has_tag_name("VideoClipTrackItem"))
        .unwrap();
    let upper_clip = source[clip.range()].replace("ObjectID=\"3\"", "ObjectID=\"13\"");
    let upper_track = r#"<VideoClipTrack ObjectUID="track-2"><ClipTrack>
        <Track><Index>1</Index></Track>
        <ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef="13"/></TrackItems></ClipItems>
        <TransitionItems><Index>1</Index></TransitionItems>
        </ClipTrack></VideoClipTrack>"#;
    source
        .replace(
            "</Tracks>",
            "<Track Index=\"1\" ObjectURef=\"track-2\"/></Tracks>",
        )
        .replace(
            "</PremiereData>",
            &format!("{upper_clip}{upper_track}</PremiereData>"),
        )
}

#[test]
fn sparse_indices_not_record_or_reference_order_determine_tracks() {
    let xml = two_tracks()
        .replace("<Index>1</Index>", "<Index>40</Index>")
        .replace("Index=\"1\"", "Index=\"40\"")
        .replace(
            "<Track ObjectURef=\"track-1\"/><Track Index=\"40\" ObjectURef=\"track-2\"/>",
            "<Track Index=\"40\" ObjectURef=\"track-2\"/><Track ObjectURef=\"track-1\"/>",
        );
    let document = roxmltree::Document::parse(&xml).unwrap();
    let records: String = document
        .root_element()
        .children()
        .rev()
        .filter(roxmltree::Node::is_element)
        .map(|node| &xml[node.range()])
        .collect();
    let project =
        inspect_project_with_media(&format!("<PremiereData>{records}</PremiereData>"), None)
            .unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        sequence
            .video_tracks()
            .map(|track| track[0].id())
            .collect::<Vec<_>>(),
        [Some("VideoClipTrackItem:3"), Some("VideoClipTrackItem:13")]
    );
    assert!(sequence.gaps(&project.media).is_empty());
    assert_eq!(sequence.end_ticks(), 5 * TICKS);
}

#[test]
fn invalid_track_membership_and_unsupported_controls_omit_or_reject() {
    let xml = two_tracks();
    assert_eq!(inspect_project(&xml, None).unwrap().video_tracks().len(), 2);
    for (before, after, reason) in [
        (
            "<Index>1</Index>",
            "<Index>0</Index>",
            "duplicate video track Index",
        ),
        ("<Index>1</Index>", "", "duplicate video track Index"),
        ("<Index>1</Index>", "<Index>-1</Index>", "nonnegative"),
        (
            "<Index>1</Index>",
            "<Index>bad</Index>",
            "invalid track Index",
        ),
        (
            "Index=\"1\"",
            "Index=\"9\"",
            "conflicting track reference Index",
        ),
        (
            "<ClipItems><Index>1",
            "<ClipItems><Index>9",
            "conflicting ClipItems Index",
        ),
        (
            "<TransitionItems><Index>1",
            "<TransitionItems><Index>9",
            "conflicting TransitionItems Index",
        ),
        (
            "ObjectRef=\"13\"",
            "ObjectRef=\"3\"",
            "duplicate video occurrence",
        ),
        (
            "<Track><Index>1",
            "<Track><MediaType>audio</MediaType><Index>1",
            "MediaType",
        ),
        (
            "<TransitionItems>",
            "<TransitionItems><UnsupportedTransitionControl/>",
            "TransitionItems/UnsupportedTransitionControl",
        ),
        (
            "VideoMediaSource",
            "VideoSequenceSource",
            "VideoSequenceSource",
        ),
    ] {
        let changed = xml.replace(before, after);
        assert_ne!(xml, changed, "{before}");
        match inspect_project_with_omissions(&changed, None) {
            Ok((_, omissions)) => assert!(
                omissions
                    .iter()
                    .any(|item| item.to_string().contains(reason)),
                "{before}: {omissions:?}"
            ),
            Err(error) => assert!(error.to_string().contains(reason), "{before}: {error}"),
        }
    }
}

#[test]
fn non_video_groups_report_omissions() {
    let xml = two_tracks()
        .replace("<Track Index=\"1\" ObjectURef=\"track-2\"/>", "")
        .replace("</TrackGroups>", "<TrackGroup><Second ObjectRef=\"14\"/></TrackGroup></TrackGroups>")
        .replace("</PremiereData>", "<AudioTrackGroup ObjectID=\"14\"><TrackGroup><Tracks><Track ObjectURef=\"track-2\"/></Tracks></TrackGroup></AudioTrackGroup></PremiereData>");
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.iter().any(|item| item
        .reason
        .contains("expected AudioClipTrack, found VideoClipTrack")));
    let audio = xml.replace(
        "<VideoClipTrack ObjectUID=\"track-2\"",
        "<AudioClipTrack ObjectUID=\"track-2\"",
    );
    // Only the last track is changed to audio.
    let end = audio.rfind("</VideoClipTrack>").unwrap();
    let audio = format!(
        "{}</AudioClipTrack>{}",
        &audio[..end],
        &audio[end + "</VideoClipTrack>".len()..]
    );
    let (_, omissions) = inspect_project_with_omissions(&audio, None).unwrap();
    assert!(omissions
        .iter()
        .any(|item| item.record == "AudioClipTrack:track-2"));

    let nested = audio
        .replace("<TrackItems>", "<Extra><TrackItems>")
        .replace("</TrackItems>", "</TrackItems></Extra>");
    assert!(inspect_project(&nested, None)
        .unwrap_err()
        .to_string()
        .contains("no convertible video or audio occurrences"));

    let prefixed = audio
        .replacen("<PremiereData", "<PremiereData xmlns:p='urn:test'", 1)
        .replace("<TrackItems>", "<p:TrackItems>")
        .replace("</TrackItems>", "</p:TrackItems>")
        .replace("<TrackItem ", "<p:TrackItem ");
    let (_, omissions) = inspect_project_with_omissions(&prefixed, None).unwrap();
    assert!(omissions
        .iter()
        .any(|item| item.record == "AudioClipTrack:track-2"));
}

#[test]
fn adobe_sequence_numeric_id_is_non_semantic_but_must_be_valid() {
    let with_id = SOURCE.replace("<Name>Main</Name>", "<ID>22</ID><Name>Main</Name>");
    let original = inspect_project(SOURCE, None).unwrap();
    let parsed = inspect_project(&with_id, None).unwrap();
    assert_eq!(parsed.id, original.id);
    assert_eq!(parsed.name, original.name);
    assert_eq!(
        parsed
            .video_occurrences()
            .map(|clip| (clip.start_ticks, clip.end_ticks))
            .collect::<Vec<_>>(),
        original
            .video_occurrences()
            .map(|clip| (clip.start_ticks, clip.end_ticks))
            .collect::<Vec<_>>()
    );
    let malformed = with_id.replace("<ID>22</ID>", "<ID>not-a-number</ID>");
    assert!(inspect_project(&malformed, None).is_err());
}

#[test]
fn unknown_content_is_reported_but_duplicates_still_reject() {
    for (xml, expected) in [
        (
            SOURCE.replace(
                "<Sequence ObjectUID=\"sequence-1\"",
                "<Sequence ObjectUID=\"sequence-1\" Foo=\"bar\"",
            ),
            "@Foo",
        ),
        (
            SOURCE.replace("<Name>Main</Name>", "<Node/><Node/><Name>Main</Name>"),
            "duplicate field `Node`",
        ),
        (
            SOURCE.replace("<Name>Main</Name>", "unexpected<Name>Main</Name>"),
            "$text",
        ),
    ] {
        if expected.contains("duplicate field") {
            let error = inspect_project(&xml, None).unwrap_err().to_string();
            assert!(error.contains(expected), "expected {expected}: {error}");
        } else {
            let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
            assert!(
                omissions.iter().any(|item| item.reason.contains(expected)),
                "expected {expected}: {omissions:?}"
            );
        }
    }
}

#[test]
fn unknown_effects_on_video_groups_tracks_and_component_owners_are_reported() {
    for (xml, record, path) in [
        (
            SOURCE.replace(
                "<VideoTrackGroup ObjectID=\"1\">",
                "<VideoTrackGroup ObjectID=\"1\"><UnknownEffect/>",
            ),
            "VideoTrackGroup:1",
            "UnknownEffect",
        ),
        (
            SOURCE.replace(
                "<TrackGroup><Tracks>",
                "<TrackGroup><UnknownEffect/><Tracks>",
            ),
            "VideoTrackGroup:1",
            "TrackGroup/UnknownEffect",
        ),
        (
            SOURCE.replacen("<ComponentOwner>", "<ComponentOwner><UnknownEffect/>", 1),
            "VideoTrackGroup:1",
            "ComponentOwner/UnknownEffect",
        ),
        (
            SOURCE.replace(
                "<VideoClipTrack ObjectUID=\"track-1\">",
                "<VideoClipTrack ObjectUID=\"track-1\"><UnknownEffect/>",
            ),
            "VideoClipTrack:track-1",
            "UnknownEffect",
        ),
        (
            SOURCE.replace(
                "<ClipTrackItem><ComponentOwner>",
                "<ClipTrackItem><ComponentOwner><UnknownEffect/>",
            ),
            "VideoClipTrackItem:3",
            "ClipTrackItem/ComponentOwner/UnknownEffect",
        ),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .count(),
            1
        );
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == OmissionScope::Feature
                    && item.record == record
                    && item.reason.contains(path)),
            "{record} {path}: {omissions:?}"
        );
    }
}

#[test]
fn nondefault_video_group_settings_are_reported() {
    let xml = SOURCE.replace(
        "<VideoTrackGroup ObjectID=\"1\">",
        "<VideoTrackGroup ObjectID=\"1\"><AutoInputGamutCompressionEnabled>false</AutoInputGamutCompressionEnabled>",
    );
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions
        .iter()
        .any(|item| item.scope == OmissionScope::Feature
            && item.record == "VideoTrackGroup:1"
            && item
                .reason
                .contains("nondefault AutoInputGamutCompressionEnabled")));
}

#[test]
fn ignored_subtrees_do_not_change_the_result() {
    let baseline = format!("{:?}", inspect_project(SOURCE, None).unwrap());
    let ignored = "<Unrelated Attribute=\"kept\"><Nested/></Unrelated>";
    let with_ignored_nodes = SOURCE
        .replace(
            "<Name>Main</Name>",
            &format!("<Node>{ignored}</Node><PersistentGroupContainer>{ignored}</PersistentGroupContainer><Name>Main</Name>"),
        )
        .replace("<Track><ID>", &format!("<Track><Node>{ignored}</Node><ID>"))
        .replace("<Clip><Source", &format!("<Clip><Node>{ignored}</Node><Source"))
        .replace(
            "<ComponentChain/>",
            &format!("<ComponentChain><Node>{ignored}</Node></ComponentChain>"),
        );
    assert_eq!(
        format!("{:?}", inspect_project(&with_ignored_nodes, None).unwrap()),
        baseline
    );

    // A master clip's Node holds Source Monitor state and its bin node
    // number; 26.5.1 writes a file-media master clip's Node with the number
    // alone.
    for (case, node) in [
        (
            "monitor properties",
            format!("<Properties><monitor.edit.time>{ignored}</monitor.edit.time></Properties>"),
        ),
        ("node number only", "<ID>1000004</ID>".to_owned()),
        (
            "both",
            "<Properties><monitor.take.video>true</monitor.take.video></Properties><ID>1000004</ID>"
                .to_owned(),
        ),
    ] {
        let with_master_node = SOURCE
            .replace(
                "<Clip ObjectRef=\"6\"/>",
                "<Clip ObjectRef=\"6\"/><MasterClip ObjectURef='master-1'/>",
            )
            .replace(
                "</PremiereData>",
                &format!("<MasterClip ObjectUID='master-1'><Node Version=\"1\">{node}</Node><Clips><Clip ObjectRef='9'/></Clips></MasterClip><VideoClip ObjectID='9'><Clip><Source ObjectRef='7'/></Clip></VideoClip></PremiereData>"),
            );
        assert_eq!(
            format!("{:?}", inspect_project(&with_master_node, None).unwrap()),
            baseline,
            "{case}"
        );
    }
}

#[test]
fn interleaved_relative_paths_are_all_read() {
    let xml = SOURCE.replace(
        "<RelativePath>media/source.mp4</RelativePath>",
        "<RelativePath>../source.mp4</RelativePath><Title>Source</Title><RelativePath>media/source.mp4</RelativePath>",
    );
    let project = crate::format::inspect_project_with_media(&xml, None).unwrap();
    let occurrence = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(
        project.media(occurrence).unwrap().relative_paths.as_slice(),
        ["../source.mp4", "media/source.mp4"]
    );
}

#[test]
fn identical_repeated_relative_paths_keep_the_first() {
    // Premiere can write a Media's RelativePath twice, after ModificationState
    // and after ContentAndMetadataState. The copies are identical in the
    // Premiere 26.5.1 re-save of feature_motion_opacity_26_5_strict.prproj
    // (SHA-256 42b41a045db3f6b4c3ef1a4fb7d7cdafb710417acd9bc232a4555e99883fe4c7)
    // and in the Premiere 25.0 user saves of the corpus adobe-pro-audio and
    // adobe-modern-speed projects.
    for (path, package_local) in [
        ("./feature_multi_sequence_blue_10s.mp4", true),
        ("../../../../../../../Macintosh HD/Users/whaley/Downloads/RAW FILES/Interview/Intro  Outro/Outro/Outro-Cam-A.mp4", false),
    ] {
        let xml = SOURCE.replace(
            "<RelativePath>media/source.mp4</RelativePath>",
            &format!("<RelativePath>{path}</RelativePath>").repeat(2),
        );
        let project = crate::format::inspect_project_with_media(&xml, None).unwrap();
        let occurrence = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        let media = project.media(occurrence).unwrap();
        assert_eq!(media.relative_paths, [path]);
        assert_eq!(media.relative_path.as_deref(), package_local.then_some(path));
    }
}

#[test]
fn different_package_local_relative_paths_still_reject() {
    for paths in [
        "<RelativePath>./feature_multi_sequence_blue_10s.mp4</RelativePath><RelativePath>./feature_multi_sequence_red_10s.mp4</RelativePath>",
        // Two spellings of one file are not an identical repeat.
        "<RelativePath>./feature_multi_sequence_blue_10s.mp4</RelativePath><RelativePath>feature_multi_sequence_blue_10s.mp4</RelativePath>",
    ] {
        let xml = SOURCE.replace("<RelativePath>media/source.mp4</RelativePath>", paths);
        let error = inspect_project(&xml, None).unwrap_err().to_string();
        assert!(
            error.contains("expected at most one package-local RelativePath, found 2"),
            "{error}"
        );
    }
}

#[cfg(unix)]
#[test]
fn windows_saved_media_paths_resolve_only_through_package_local_hints() {
    // Path fields of a Media that Premiere saved on Windows: corpus
    // motionarray-83972 `Mini Glitch_Pack Free.prproj` (SHA-256
    // c7fc4484c9c065ba928848e80f61496c691f7e10add4a595c8b7931dd3ada2ac),
    // Lines_02.mov, with the user folder renamed.
    const ALIAS: &str =
        r"C:\Users\editor\Documents\Pack\Premiere\Footages\video_Elem\Lines\Lines_02.mov";
    const HINT: &str = r".\Footages\video_Elem\Lines\Lines_02.mov";
    let paths = |hint: &str, actual: &str, file: &str| {
        format!("<RelativePath>{hint}</RelativePath><FilePath>{file}</FilePath><ActualMediaFilePath>{actual}</ActualMediaFilePath>")
    };
    // Ok: package-local hint, local alias count and name; Err: the reason.
    for (fields, expected) in [
        (
            paths(HINT, ALIAS, ALIAS),
            Ok(("./Footages/video_Elem/Lines/Lines_02.mov", 0, "Lines_02.mov")),
        ),
        // A POSIX save keeps a backslash as a file-name character.
        (
            r"<RelativePath>media/source\clip.mp4</RelativePath><FilePath>/Users/editor/media/source\clip.mp4</FilePath>".to_owned(),
            Ok((r"media/source\clip.mp4", 1, r"source\clip.mp4")),
        ),
        // Corpus motionarray-95522: a `..\` hint is outside the package.
        (
            paths(r"..\Pattern.png", r"D:\Motionarray\Pattern.png", r"D:\Motionarray\Pattern.png"),
            Err("media saved on Windows has no package-local RelativePath"),
        ),
        (
            paths(r"C:\Footages\Lines_02.mov", ALIAS, ALIAS),
            Err("RelativePath names a Windows drive"),
        ),
        (
            paths(r"\Footages\Lines_02.mov", ALIAS, ALIAS),
            Err("empty or absolute RelativePath"),
        ),
        // Drive-relative, device (corpus adobe-edit-videos), UNC and mixed aliases.
        (
            paths(HINT, r"C:Footages\Lines_02.mov", ALIAS),
            Err("ActualMediaFilePath must be a nonempty absolute path"),
        ),
        (
            paths(HINT, &format!(r"\\?\{ALIAS}"), ALIAS),
            Err("ActualMediaFilePath must be a nonempty absolute path"),
        ),
        (
            paths(HINT, r"\\server\share\Lines_02.mov", r"\\server\share\Lines_02.mov"),
            Err("ActualMediaFilePath must be a nonempty absolute path"),
        ),
        (
            paths(HINT, "/Users/editor/Lines_02.mov", ALIAS),
            Err("FilePath must be a nonempty absolute path"),
        ),
    ] {
        let xml = SOURCE.replace("<RelativePath>media/source.mp4</RelativePath>", &fields);
        match (inspect_project_with_media(&xml, None), expected) {
            (Ok(project), Ok((relative_path, aliases, name))) => {
                let occurrence = project
                    .single_sequence()
                    .unwrap()
                    .video_occurrences()
                    .next()
                    .unwrap();
                let media = project.media(occurrence).unwrap();
                assert_eq!(media.relative_path.as_deref(), Some(relative_path), "{fields}");
                assert_eq!(media.relative_paths, [relative_path], "{fields}");
                assert_eq!(media.absolute_paths.len(), aliases, "{fields}");
                assert_eq!(media.name, name, "{fields}");
            }
            (Err(error), Err(reason)) => {
                assert!(error.to_string().contains(reason), "{fields}: {error}");
            }
            (actual, expected) => {
                panic!("{fields}: {:?} instead of {expected:?}", actual.map(|_| ()))
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn windows_saved_paths_keep_the_timelines_of_pinned_fixtures() {
    // Only the path fields change, to the form Premiere writes on Windows. The
    // audio case keeps its gains and trimmed source ranges, and the disabled
    // layers case its disabled clips and track outputs.
    let fixture = |bytes: &[u8]| {
        let mut xml = String::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_string(&mut xml)
            .unwrap();
        xml
    };
    let sound = |name: &str| {
        let alias = format!(r"C:\Users\editor\seed\{name}");
        (
            format!("<RelativePath>{name}</RelativePath>"),
            format!(
                r"<RelativePath>.\{name}</RelativePath><FilePath>{alias}</FilePath><ActualMediaFilePath>{alias}</ActualMediaFilePath>"
            ),
        )
    };
    for (xml, edits) in [
        (
            fixture(include_bytes!(
                "../../../tests/fixtures/feature_audio_clips_strict.prproj"
            )),
            vec![
                sound("feature_audio_click_left.wav"),
                sound("feature_audio_tone_right.wav"),
            ],
        ),
        (
            fixture(include_bytes!(
                "../../../tests/fixtures/feature_export_disabled_layers.prproj"
            )),
            vec![
                (
                    "<RelativePath>./media/feature_still_opaque.jpg</RelativePath>".to_owned(),
                    r"<RelativePath>.\media\feature_still_opaque.jpg</RelativePath>".to_owned(),
                ),
                (
                    "/private/tmp/JRB-1966/evidence/native/media/feature_still_opaque.jpg"
                        .to_owned(),
                    r"C:\Users\editor\native\media\feature_still_opaque.jpg".to_owned(),
                ),
            ],
        ),
    ] {
        let mut saved_on_windows = xml.clone();
        for (from, to) in &edits {
            assert!(saved_on_windows.contains(from.as_str()), "{from}");
            saved_on_windows = saved_on_windows.replace(from.as_str(), to);
        }
        let (expected, expected_omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let (project, omissions) = inspect_project_with_omissions(&saved_on_windows, None).unwrap();
        assert_eq!(
            format!("{:?}", project.sequences),
            format!("{:?}", expected.sequences)
        );
        assert_eq!(omissions, expected_omissions);
        for (id, media) in &project.media {
            assert_eq!(media.name, expected.media[id].name);
            assert!(media.absolute_paths.is_empty(), "{id}");
        }
    }
}

/// Places the `one-clip.xml` occurrence under a master clip on the same media
/// source whose own clip record carries the In/Out marks `in_point..out_point`.
fn with_master_range(xml: &str, in_point: i64, out_point: i64) -> String {
    let subclip = "<SubClip ObjectID=\"5\"><Clip ObjectRef=\"6\"/>";
    assert_eq!(xml.matches(subclip).count(), 1, "one-clip.xml: {subclip}");
    xml.replacen(
        subclip,
        &format!("{subclip}<MasterClip ObjectURef=\"master-1\"/>"),
        1,
    )
    .replace(
        "</PremiereData>",
        &format!(
            "<MasterClip ObjectUID=\"master-1\"><Clips><Clip ObjectRef=\"40\"/></Clips><Name>Subclip</Name></MasterClip>\
             <VideoClip ObjectID=\"40\"><Clip><Source ObjectRef=\"7\"/><InPoint>{in_point}</InPoint><OutPoint>{out_point}</OutPoint></Clip></VideoClip></PremiereData>"
        ),
    )
}

/// Adds a native `OriginalSubClipTimeOffset` to the `one-clip.xml` track item,
/// last in `ClipTrackItem`, where Premiere 9.x/10.x saves write it.
fn with_subclip_time_offset(xml: &str, value: &str) -> String {
    let end = "<SubClip ObjectRef=\"5\"/></ClipTrackItem>";
    assert_eq!(xml.matches(end).count(), 1, "one-clip.xml: {end}");
    xml.replacen(
        end,
        &format!(
            "<SubClip ObjectRef=\"5\"/><OriginalSubClipTimeOffset>{value}</OriginalSubClipTimeOffset></ClipTrackItem>"
        ),
        1,
    )
}

#[test]
fn adobe_marked_master_keeps_absolute_occurrence_source_ranges() {
    // Corpus `2128_aiedit_en_na_multicreators`, master clip
    // 85f59409-461e-478f-aa5e-fa67a48ba381: In/Out 39.960-41.242 s of a 284.156 s
    // screen recording. Track items 293 and 288 keep their exact 30 fps sequence
    // ranges and source In/Out; only the media rate and size are normalized to
    // the supported 30 fps 1080p subset. Item 293 starts 26.6 ms before the
    // master In, item 288 lies wholly outside it, and the old reader omitted
    // both ("master source range conflicts with occurrence"). Read relative to
    // the master In, item 293 would instead start at 79.894 s.
    for (item, start, end, in_point, out_point) in [
        (
            293,
            1_447_891_200_000,
            1_778_112_000_000,
            10_143_705_600_000,
            10_473_926_400_000,
        ),
        (
            288,
            313_286_400_000,
            491_097_600_000,
            804_384_000_000,
            982_195_200_000,
        ),
    ] {
        let xml = with_master_range(
            &one_clip_xml(OneClip {
                media_duration: 72_180_137_981_952,
                start,
                end,
                in_point,
                out_point,
                ..OneClip::default()
            }),
            10_150_464_728_292,
            10_476_204_279_614,
        );
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!(clip.timeline_ticks(), start..end, "item {item}");
        assert_eq!(clip.source_ticks(), in_point..out_point, "item {item}");
        assert!(
            omissions
                .iter()
                .all(|omission| omission.scope != OmissionScope::Occurrence),
            "item {item}: {omissions:?}"
        );
    }
}

#[test]
fn occurrence_outside_its_media_still_rejects_under_a_master_range() {
    for (in_point, out_point, expected) in [
        (10 * TICKS, 15 * TICKS, "ranges"),
        (6 * TICKS, 11 * TICKS, "frame past"),
    ] {
        let xml = with_master_range(
            &one_clip_xml(OneClip {
                in_point,
                out_point,
                ..OneClip::default()
            }),
            2 * TICKS,
            8 * TICKS,
        );
        let error = inspect_project(&xml, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn zero_subclip_time_offset_keeps_the_occurrence_source_range() {
    // Corpus `free_quotes` and `5_ink_transitions` (Premiere 9.x/10.x saves)
    // write `0` on all 31 of their placements.
    let xml = with_subclip_time_offset(&one_clip_xml(OneClip::default()), "0");
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(
        (clip.timeline_ticks(), clip.source_ticks()),
        (0..5 * TICKS, 0..5 * TICKS)
    );
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
}

#[test]
fn nonzero_or_invalid_subclip_time_offset_omits_the_occurrence() {
    // What a nonzero offset encodes is unknown, so converting the placement's
    // own In/Out anyway could select other frames.
    for (value, expected) in [
        ((2 * TICKS).to_string(), "nonzero OriginalSubClipTimeOffset"),
        ("0.5".to_owned(), "invalid OriginalSubClipTimeOffset"),
    ] {
        let xml = with_subclip_time_offset(&one_clip_xml(OneClip::default()), &value);
        let error = inspect_project(&xml, None).unwrap_err().to_string();
        assert!(
            error.contains(expected) && error.contains("VideoClipTrackItem:3"),
            "{value}: {error}"
        );
    }
}

#[test]
fn native_film_impact_default_tail_profile_is_admitted() {
    let (project, omissions) =
        inspect_project_with_omissions(&crate::tests::support::film_impact_tail_xml(), None)
            .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let transitions = &project.single_sequence().unwrap().video_tracks[0].transitions;
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0].start_ticks, 4 * TICKS);
    assert_eq!(transitions[0].end_ticks, 5 * TICKS);
}

#[test]
fn film_impact_tail_rejects_unmeasured_profiles_without_losing_the_picture() {
    let source = crate::tests::support::film_impact_tail_xml();
    for (from, to) in [
        ("<VideoFilterType>2</VideoFilterType>", "<VideoFilterType>1</VideoFilterType>"),
        ("<StartKeyframePosition>-91445760000000000</StartKeyframePosition>", "<StartKeyframePosition>-91445759999999999</StartKeyframePosition>"),
        ("<Component Version=\"7\">", "<Component Version=\"7\"><Bypass>true</Bypass>"),
        ("<Component Version=\"7\">", "<Component Version=\"7\"><Bypass>1</Bypass>"),
        ("<ParameterID>16</ParameterID>", "<ParameterID>999</ParameterID>"),
        ("<ParameterID>17</ParameterID>", "<ParameterID>16</ParameterID>"),
        ("<Param Index=\"29\" ObjectRef=\"2864\" />", ""),
        ("<StartKeyframePosition>-91445760000000000</StartKeyframePosition>", "<StartKeyframePosition>-91445760000000000</StartKeyframePosition><BinaryData BinaryHash=\"custom\">AA==</BinaryData>"),
        ("<ArbVideoComponentParam ObjectID=\"2844\"", "<ArbVideoComponentParam BinaryHash=\"custom\" ObjectID=\"2844\""),
        ("<ParameterID>16</ParameterID>", "<ParameterID>16</ParameterID><IsTimeVarying>true</IsTimeVarying>"),
        ("<ParameterID>16</ParameterID>", "<ParameterID>16</ParameterID><Keyframes>1,2</Keyframes>"),
        ("<ParameterID>16</ParameterID>", "<ParameterID>16</ParameterID><IsTimeVarying>false</IsTimeVarying><Keyframes><Private/></Keyframes>"),
        ("<ParameterID>16</ParameterID>", "<ParameterID>16</ParameterID><CurrentValue>35</CurrentValue>"),
        ("-91445760000000000,34.,", "-91445760000000000,35.,"),
        ("<ParameterID>16</ParameterID>", "<ParameterID>16</ParameterID><IsTimeVarying>false</IsTimeVarying><Keyframes><!-- saved keys -->1,2</Keyframes>"),
        ("-91445760000000000,34.,0,0,0,0,0,0</StartKeyframe>", "-91445760000000000,34.,0,0,0,0,0,0<!-- split -->,extra</StartKeyframe>"),
        ("-91445760000000000,260300.,", "-91445760000000000,260301.,"),
        ("-91445760000000000,0.5:0.5,", "-91445760000000000,0.4:0.5,"),
        ("-91445760000000000,100.,", "-91445760000000000,NaN,"),
        ("<ParameterID>5</ParameterID>\n", "<ParameterID>5</ParameterID><ParameterID>5</ParameterID>\n"),
        ("<Alignment>254016000000</Alignment>", "<Alignment>127008000000</Alignment>"),
        ("<HasIncomingClip>false</HasIncomingClip>", "<HasIncomingClip>true</HasIncomingClip>"),
        ("<VideoFilterComponent ObjectRef=\"1610\" />", "<VideoFilterComponent ObjectRef=\"1610\" /><Reverse>false</Reverse>"),
    ] {
        assert!(source.contains(from), "mutation not applied: {from}");
        let (project, omissions) = inspect_project_with_omissions(&source.replace(from, to), None).unwrap();
        let track = &project.single_sequence().unwrap().video_tracks[0];
        assert_eq!(track.items.len(), 1, "{from}: {omissions:?}");
        assert!(track.transitions.is_empty(), "{from}: {omissions:?}");
        assert!(omissions.iter().any(|item| item.scope == OmissionScope::Feature && item.record == "1009"), "{from}: {omissions:?}");
    }
}

#[test]
fn native_film_impact_curve_expansion_is_ui_without_private_curve_data() {
    let source = crate::tests::support::film_impact_curve_ui_xml();
    let (project, omissions) = inspect_project_with_omissions(&source, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project.single_sequence().unwrap().video_tracks[0]
            .transitions
            .len(),
        1
    );
    for (from, to) in [
        (
            "<ECP.Custom.Expanded>true</ECP.Custom.Expanded>",
            "<ECP.Custom.Expanded>true</ECP.Custom.Expanded><CustomCurve>1</CustomCurve>",
        ),
        (
            "<ECP.Custom.Expanded>true</ECP.Custom.Expanded>",
            "<ECP.Custom.Expanded ObjectRef=\"123\">true</ECP.Custom.Expanded>",
        ),
        (
            "<ECP.Custom.Expanded>true</ECP.Custom.Expanded>",
            "<ECP.Custom.Expanded><!-- hidden -->true</ECP.Custom.Expanded>",
        ),
        (
            "<Properties Version=\"1\">",
            "<Properties Version=\"1\" BinaryHash=\"private\">",
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&source.replace(from, to), None).unwrap();
        let track = &project.single_sequence().unwrap().video_tracks[0];
        assert_eq!(track.items.len(), 1);
        assert!(track.transitions.is_empty(), "{from}: {omissions:?}");
    }
}
