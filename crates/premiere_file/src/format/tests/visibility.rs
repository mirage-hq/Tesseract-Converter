//! Native `IsMuted` decoding/encoding for clip Enable and track video output.
use crate::format::{
    inspect_project, inspect_project_with_media, inspect_project_with_omissions,
    writer::project_xml,
};
use crate::schema::{records::MediaPathField, PrProjectFile, PrVideoTrack, TICKS};
use crate::tests::support::{text_graphic, video_media, video_sequence};
use crate::OmissionScope;

const SOURCE: &str = include_str!("../../../tests/fixtures/one-clip.xml");

/// Two tracks: the fixture clip on track 0 and a copy on track 1.
fn two_tracks() -> String {
    let document = roxmltree::Document::parse(SOURCE).unwrap();
    let clip = document
        .descendants()
        .find(|node| node.has_tag_name("VideoClipTrackItem"))
        .unwrap();
    let upper_clip = SOURCE[clip.range()].replace("ObjectID=\"3\"", "ObjectID=\"13\"");
    let upper_track = r#"<VideoClipTrack ObjectUID="track-2"><ClipTrack>
        <Track><Index>1</Index></Track>
        <ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef="13"/></TrackItems></ClipItems>
        </ClipTrack></VideoClipTrack>"#;
    SOURCE
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
fn disabled_clip_imports_hidden_with_its_source_and_trims_intact() {
    let xml = SOURCE.replace(
        "<SubClip ObjectRef=\"5\"/></ClipTrackItem>",
        "<SubClip ObjectRef=\"5\"/><IsMuted>true</IsMuted></ClipTrackItem>",
    );
    assert_ne!(xml, SOURCE);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert!(!clip.enabled);
    assert_eq!(clip.timeline_ticks(), 0..5 * TICKS);
    assert_eq!(clip.source_ticks(), 0..5 * TICKS);
    assert_eq!(project.media(clip).unwrap().name(), "source.mp4");
    // The picture-free span is a gap even though the sequence still ends at 5 s.
    assert_eq!(sequence.gaps(&project.media), vec![0..5 * TICKS]);
    assert_eq!(sequence.end_ticks(), 5 * TICKS);
}

#[test]
fn absent_and_false_is_muted_import_enabled() {
    let explicit = SOURCE.replace(
        "<SubClip ObjectRef=\"5\"/></ClipTrackItem>",
        "<SubClip ObjectRef=\"5\"/><IsMuted>false</IsMuted></ClipTrackItem>",
    );
    for xml in [SOURCE.to_owned(), explicit] {
        let project = inspect_project_with_media(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert!(sequence.video_occurrences().next().unwrap().enabled);
        assert!(sequence.gaps(&project.media).is_empty());
    }
}

#[test]
fn unknown_is_muted_values_reject_instead_of_guessing() {
    let clip = SOURCE.replace(
        "<SubClip ObjectRef=\"5\"/></ClipTrackItem>",
        "<SubClip ObjectRef=\"5\"/><IsMuted>yes</IsMuted></ClipTrackItem>",
    );
    let error = inspect_project(&clip, None).unwrap_err().to_string();
    assert!(
        error.contains("VideoClipTrackItem:3: invalid IsMuted"),
        "{error}"
    );
    let track = SOURCE.replace(
        "<Track><ID>1</ID></Track>",
        "<Track><ID>1</ID><IsMuted>1</IsMuted></Track>",
    );
    assert_ne!(track, SOURCE);
    let error = inspect_project(&track, None).unwrap_err().to_string();
    assert!(
        error.contains("VideoClipTrack:track-1: invalid IsMuted"),
        "{error}"
    );
}

#[test]
fn track_output_off_hides_every_clip_on_that_track_only() {
    let xml = two_tracks().replace(
        "<Track><Index>1</Index></Track>",
        "<Track><Index>1</Index><IsMuted>true</IsMuted></Track>",
    );
    assert_ne!(xml, two_tracks());
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|item| item.scope == OmissionScope::Track
                || item.scope == OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let sequence = project.single_sequence().unwrap();
    let enabled: Vec<Vec<bool>> = sequence
        .video_tracks()
        .map(|track| {
            track
                .iter()
                .map(|clip| clip.media().unwrap().enabled)
                .collect()
        })
        .collect();
    assert_eq!(enabled, [vec![true], vec![false]]);
    assert!(sequence.gaps(&project.media).is_empty());

    // The clip-level flag composes with the track flag rather than replacing it.
    let lower_disabled = xml.replacen(
        "<SubClip ObjectRef=\"5\"/></ClipTrackItem>",
        "<SubClip ObjectRef=\"5\"/><IsMuted>true</IsMuted></ClipTrackItem>",
        1,
    );
    let project = inspect_project_with_media(&lower_disabled, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert!(sequence.video_occurrences().all(|clip| !clip.enabled));
    assert_eq!(sequence.gaps(&project.media), vec![0..5 * TICKS]);
}

#[test]
fn empty_track_with_output_off_adds_no_omission_or_occurrence() {
    // The only corpus shape (`horror_title`, `vhsvertical`, `corporate_slideshow`):
    // a muted video track whose `ClipItems` has no `TrackItems`.
    let muted_track = r#"<VideoClipTrack ObjectUID="track-2"><ClipTrack>
        <Track><Index>1</Index><IsMuted>true</IsMuted></Track>
        <ClipItems><Index>1</Index></ClipItems>
        </ClipTrack></VideoClipTrack>"#;
    let xml = SOURCE
        .replace(
            "</Tracks>",
            "<Track Index=\"1\" ObjectURef=\"track-2\"/></Tracks>",
        )
        .replace("</PremiereData>", &format!("{muted_track}</PremiereData>"));
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let clips_per_track: Vec<usize> = sequence.video_tracks().map(<[_]>::len).collect();
    assert_eq!(clips_per_track, [1, 0]);
    assert!(sequence.video_occurrences().all(|clip| clip.enabled));
    assert!(sequence.gaps(&project.media).is_empty());
    assert_eq!(sequence.end_ticks(), 5 * TICKS);
}

#[test]
fn gaps_include_spans_covered_only_by_disabled_clips() {
    let mut sequence = video_sequence();
    let lower = sequence.video_tracks[0].clip_mut(0);
    lower.end_ticks = 2 * TICKS;
    lower.out_ticks = 2 * TICKS;
    let mut disabled_middle = lower.clone();
    disabled_middle.start_ticks = 2 * TICKS;
    disabled_middle.end_ticks = 3 * TICKS;
    disabled_middle.out_ticks = TICKS;
    disabled_middle.enabled = false;
    let mut upper = lower.clone();
    upper.start_ticks = 3 * TICKS;
    upper.end_ticks = 4 * TICKS;
    upper.out_ticks = TICKS;
    let mut disabled_tail = lower.clone();
    disabled_tail.start_ticks = 4 * TICKS;
    disabled_tail.end_ticks = 5 * TICKS;
    disabled_tail.out_ticks = TICKS;
    disabled_tail.enabled = false;
    sequence.video_tracks[0].items.extend(
        [disabled_middle, upper, disabled_tail]
            .into_iter()
            .map(crate::schema::PrVideoItem::Media),
    );
    let media = video_media();
    sequence.validate_timeline(&media).unwrap();
    assert_eq!(sequence.end_ticks(), 5 * TICKS);
    assert_eq!(
        sequence.gaps(&media),
        [2 * TICKS..3 * TICKS, 4 * TICKS..5 * TICKS]
    );

    // A disabled upper clip over an enabled lower clip adds no gap.
    let mut covered = video_sequence();
    let mut hidden = covered.video_tracks[0].clip(0).clone();
    hidden.enabled = false;
    covered.video_tracks.push(PrVideoTrack {
        transitions: Vec::new(),
        items: vec![crate::schema::PrVideoItem::Media(hidden)],
        nests: Vec::new(),
    });
    assert!(covered.gaps(&media).is_empty());
}

#[test]
fn writer_encodes_disabled_clips_and_leaves_tracks_enabled() {
    let mut sequence = video_sequence();
    let mut hidden = sequence.video_tracks[0].clip(0).clone();
    hidden.enabled = false;
    sequence.video_tracks.push(PrVideoTrack {
        transitions: Vec::new(),
        items: vec![crate::schema::PrVideoItem::Media(hidden)],
        nests: Vec::new(),
    });
    let mut graphic = text_graphic();
    graphic.enabled = false;
    sequence.video_tracks.push(PrVideoTrack {
        transitions: Vec::new(),
        items: vec![crate::schema::PrVideoItem::Graphic(graphic)],
        nests: Vec::new(),
    });
    let mut media = video_media();
    for facts in media.values_mut() {
        facts.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Video {
            codec: Some(crate::schema::VideoCodec::H264),
            hdr_profile: None,
            color_space_override: None,
        };
        facts.relative_path = Some("./media/source.mp4".into());
        facts.relative_paths = vec!["./media/source.mp4".into()];
        facts.absolute_paths = vec![(
            MediaPathField::ActualMediaFilePath,
            "/tmp/package/media/source.mp4".into(),
        )];
    }
    let xml = project_xml(&PrProjectFile::from_sequences(vec![sequence], media)).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let muted_owners: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("IsMuted"))
        .map(|node| (node.parent().unwrap().tag_name().name(), node.text()))
        .collect();
    assert_eq!(muted_owners, [("ClipTrackItem", Some("true")); 2]);
    let read = inspect_project(&xml, None).unwrap();
    let enabled: Vec<Vec<bool>> = read
        .video_tracks()
        .map(|track| {
            track
                .iter()
                .map(|item| match item {
                    crate::schema::PrVideoItem::Media(clip) => clip.enabled,
                    crate::schema::PrVideoItem::Graphic(graphic) => graphic.enabled,
                    crate::schema::PrVideoItem::Capsule(capsule) => capsule.placement.enabled,
                })
                .collect()
        })
        .collect();
    assert_eq!(enabled, [vec![true], vec![false], vec![false]]);
    let hidden = read.video_occurrences().last().unwrap();
    assert_eq!(hidden.timeline_ticks(), 0..5 * TICKS);
    assert_eq!(hidden.source_ticks(), 0..5 * TICKS);
}

#[test]
fn track_editing_locks_are_admitted_without_reports() {
    for locks in [
        "<IsSyncLocked>true</IsSyncLocked>",
        "<IsSyncLocked>false</IsSyncLocked><IsLocked>true</IsLocked>",
    ] {
        let xml = SOURCE.replace(
            "<Track><ID>1</ID></Track>",
            &format!("<Track><ID>1</ID>{locks}</Track>"),
        );
        assert_ne!(xml, SOURCE);
        let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
        assert!(omissions.is_empty(), "{locks}: {omissions:?}");
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1);
        assert!(sequence.video_occurrences().all(|clip| clip.enabled));
    }
}

/// Older saves (`Sequence` Version 11, `VideoTrackGroup` Version 10) write a
/// sequence `ID`, a progressive `FieldType`, a flat VR configuration with 0 x 0
/// captured angles and no sequence component chain; none has picture
/// semantics.
#[test]
fn older_sequence_fields_without_picture_semantics_are_admitted() {
    const FLAT_VR: &str = r#"{"ambisonicsHRIR":"","ambisonicsMonitoringType":0,"capturedHorizontalView":0,"capturedVerticalView":0,"fieldOfHorizontalView":108,"fieldOfVerticalView":108,"projectionType":0,"stereoscopicEye":0,"stereoscopicType":0,"version":3}"#;
    let older = |field_type: &str, vr: &str| {
        SOURCE
            .replace(
                "<Name>Main</Name>",
                "<Name>Main</Name><ID>5c3b8f2e-0000-4000-8000-000000000001</ID>",
            )
            .replace(
                "<FrameRect>0,0,1920,1080</FrameRect><ComponentOwner><Components ObjectRef=\"2\"/></ComponentOwner></VideoTrackGroup>",
                &format!("<FrameRect>0,0,1920,1080</FrameRect><FieldType>{field_type}</FieldType><ImmersiveVideoVRConfiguration>{vr}</ImmersiveVideoVRConfiguration></VideoTrackGroup>"),
            )
    };
    let xml = older("0", FLAT_VR);
    assert_ne!(xml, SOURCE);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );

    // Interlaced fields and a VR projection change the picture: reported.
    let xml = older(
        "1",
        &FLAT_VR.replace("\"projectionType\":0", "\"projectionType\":1"),
    );
    let (_, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        [
            "nondefault FieldType not converted",
            "nondefault ImmersiveVideoVRConfiguration not converted"
        ],
        "{omissions:?}"
    );
}
