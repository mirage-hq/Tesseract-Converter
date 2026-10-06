use crate::schema::TICKS;
use crate::{
    format::{read_xml, reader::time_remap::read_time_remapping, Graph},
    schema::{
        native::{Reference, VideoClip},
        PrKeyframeEasing, PrTimeRemap,
    },
    tests::support::{project_document_with_media, video_media, video_sequence},
};
use serde_json::json;
use std::{io::Write, path::Path};

#[test]
fn time_remap_keys_cross_the_former_count_quota() {
    use crate::schema::{
        native::{TimeComponentParam, TimeRemapping},
        TICKS,
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_time_remap_variable_speed_strict.prproj");
    let xml = read_xml(&path).unwrap();
    let graph = Graph::parse(&xml).unwrap();
    let record = graph
        .records()
        .find(|record| record.identity() == "VideoClip:142")
        .unwrap();
    let clip = graph.decode::<VideoClip>(record).unwrap();
    let reference = clip
        .value
        .clip
        .as_ref()
        .unwrap()
        .time_remapping
        .as_ref()
        .unwrap();
    let mapping = graph
        .follow::<TimeRemapping>(reference, &clip.identity)
        .unwrap();
    let param = graph
        .follow::<TimeComponentParam>(&mapping.value.keyframes, &mapping.identity)
        .unwrap();
    let keys: String = (0..4097_i64)
        .map(|index| {
            let mode = match index {
                1 => 7,
                2 => 8,
                _ => 6,
            };
            format!("{},{index},{mode},0,0,0,0,0;", index * TICKS)
        })
        .collect();
    let expanded = xml.replacen(&param.value.keyframes, &keys, 1);
    let graph = Graph::parse(&expanded).unwrap();
    let remap = read_time_remapping(&graph, reference, &clip.identity, &mut Vec::new()).unwrap();
    assert_eq!(remap.keys.len(), 4097);
    assert_eq!(remap.keys.last().unwrap().source_ticks, 4096 * TICKS);
}

#[test]
fn adobe_native_variable_speed_ramp_preserves_source_clock_curve() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/adobe_native_variable_speed_ramp_ppro25.prproj");
    let xml = read_xml(&path).unwrap();
    let graph = Graph::parse(&xml).unwrap();
    let record = graph
        .records()
        .find(|record| record.identity() == "VideoClip:1565")
        .unwrap();
    let clip = graph.decode::<VideoClip>(record).unwrap();
    let reference = clip
        .value
        .clip
        .as_ref()
        .and_then(|clip| clip.time_remapping.as_ref())
        .unwrap();
    let remap = read_time_remapping(&graph, reference, &clip.identity, &mut Vec::new()).unwrap();

    assert_eq!(
        remap
            .keys
            .iter()
            .map(|key| (key.timeline_ticks, key.source_ticks))
            .collect::<Vec<_>>(),
        vec![
            (0, 0),
            (3_342_850_560_000, 3_342_850_560_000),
            (4_267_468_800_000, 4_886_963_020_800),
            (6_669_070_670_769, 10_506_711_398_400),
            (9_309_628_019_947, 14_916_442_171_529),
            (11_698_053_994_415, 17_304_868_145_998),
            (15_823_273_834_415, 19_986_261_041_998),
            (38_446_691_694_421, 26_773_286_400_000),
        ]
    );
    assert!(matches!(
        remap.keys[2].easing,
        PrKeyframeEasing::CubicBezier { .. }
    ));
    assert!(matches!(
        remap.keys[4].easing,
        PrKeyframeEasing::CubicBezier { .. }
    ));
    assert!(matches!(
        remap.keys[6].easing,
        PrKeyframeEasing::CubicBezier { .. }
    ));

    let mut sequence = video_sequence();
    let occurrence = sequence.video_tracks[0].clip_mut(0);
    occurrence.end_ticks = remap.keys.last().unwrap().timeline_ticks;
    occurrence.out_ticks = occurrence.end_ticks;
    occurrence.time_remap = Some(remap);
    let mut media = video_media();
    let video = media
        .get_mut(&occurrence.media)
        .unwrap()
        .video
        .as_mut()
        .unwrap();
    video.intrinsic_ticks = 26_773_286_400_000;

    let document = project_document_with_media(&sequence, &media);
    let layer = &document["composition"]["layers"][0];
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 151355})
    );
    assert_eq!(
        layer["sourceRange"],
        json!({"start": 0, "duration": 105400})
    );
    let playback = &layer["playback"]["mapping"]["property"];
    assert_eq!(playback["before"], "continue");
    assert_eq!(playback["after"], "continue");
    assert_eq!(
        playback["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| (
                key["time"].as_u64().unwrap(),
                key["value"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, 0),
            (13_160, 13_160),
            (16_800, 19_239),
            (26_255, 41_362),
            (36_650, 58_722),
            (46_052, 68_125),
            (62_292, 78_681),
            (151_355, 105_400),
        ]
    );
    assert_eq!(playback["keyframes"][2]["easing"]["type"], "cubicBezier");
    assert_eq!(playback["keyframes"][4]["easing"]["type"], "cubicBezier");
    assert_eq!(playback["keyframes"][6]["easing"]["type"], "cubicBezier");
}

#[test]
fn nonlinear_native_ramp_writer_uses_saved_speed_modes_and_seconds() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_time_remap_variable_speed_strict.prproj");
    let (mut project, omissions) = crate::PrProjectFile::load(&source).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    for media in project.media.values_mut() {
        if let Some(video) = &mut media.video {
            if let crate::schema::PrMediaKind::Video { codec, .. } = &mut video.kind {
                *codec = Some(crate::schema::VideoCodec::H264);
            }
        }
        media.name = "source.mp4".into();
        media.relative_path = Some("./media/source.mp4".into());
        media.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            "/media/source.mp4".into(),
        )];
    }
    let xml = crate::format::writer::project_xml(&project).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let parameter = document
        .descendants()
        .find(|node| node.has_tag_name("TimeComponentParam"))
        .unwrap();
    assert_eq!(parameter.attribute("Version"), Some("9"));
    assert!(parameter
        .children()
        .all(|node| !node.has_tag_name("IsTimeVarying")));
    let keys = parameter
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap();
    let fields: Vec<_> = keys
        .split_terminator(';')
        .map(|key| key.split(',').collect::<Vec<_>>())
        .collect();
    assert_eq!(
        fields.iter().map(|key| key[2]).collect::<Vec<_>>(),
        ["6", "7", "8", "7", "8", "7", "8", "6"]
    );
    let original = crate::tests::support::first_clip(&project);
    for (fields, key) in fields
        .iter()
        .zip(&original.time_remap.as_ref().unwrap().keys)
    {
        assert_eq!(fields.len(), 8);
        assert_eq!(
            fields[0].parse::<i64>().unwrap(),
            key.timeline_ticks + original.in_ticks
        );
        assert!(
            (fields[1].parse::<f64>().unwrap() * crate::schema::TICKS as f64
                - key.source_ticks as f64)
                .abs()
                < 1.0
        );
        assert!(fields[3..].iter().all(|field| *field == "0"));
    }
    let mapping = document
        .descendants()
        .find(|node| node.has_tag_name("TimeRemapping") && node.attribute("ObjectID").is_some())
        .unwrap();
    let reference = mapping
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap();
    assert_eq!(
        reference.attribute("ObjectRef"),
        parameter.attribute("ObjectID")
    );
    assert!(document
        .descendants()
        .any(|node| node.has_tag_name("TimeRemapping")
            && node.attribute("ObjectRef") == mapping.attribute("ObjectID")));
}

/// Structural-only derivative: a 10 s active span, saved source 5..15 s,
/// declared intrinsic duration 20 s, and a curve covering only input 0..10 s.
fn trimmed_native_ramp_xml() -> String {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_time_remap_variable_speed_strict.prproj");
    let xml = read_xml(&fixture).unwrap();
    assert_eq!(xml.matches("508032000000").count(), 3);
    assert_eq!(xml.matches("<Duration>2540160000000</Duration>").count(), 1);
    assert_eq!(
        xml.matches("<OriginalDuration>2540160000000</OriginalDuration>")
            .count(),
        1
    );

    // Preserve equal 10-second active/source durations while moving the source
    // range to 5..15 seconds, past the curve's last key at input 10 s.
    xml.replace("508032000000", "2540160000000")
        .replace("<InPoint>0</InPoint>", "<InPoint>1270080000000</InPoint>")
        .replace(
            "<OutPoint>2540160000000</OutPoint>",
            "<OutPoint>3810240000000</OutPoint>",
        )
        .replace(
            "<Duration>2540160000000</Duration>",
            "<Duration>5080320000000</Duration>",
        )
        .replace(
            "<OriginalDuration>2540160000000</OriginalDuration>",
            "<OriginalDuration>5080320000000</OriginalDuration>",
        )
}

fn read_native_ramp_xml(
    xml: &str,
) -> crate::error::Result<(crate::format::PrProjectFile, Vec<crate::Omission>)> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("time-remap.prproj");
    let mut zip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    zip.write_all(xml.as_bytes()).unwrap();
    std::fs::write(&path, zip.finish().unwrap()).unwrap();
    crate::format::PrProjectFile::load_selected(&path, Some("9a10a3b7-a83b-47d9-a68c-91d06d937738"))
}

#[test]
fn trimmed_variable_speed_ramp_retains_valid_saved_base_with_diagnostic() {
    let trimmed = trimmed_native_ramp_xml();
    let (project, omissions) = read_native_ramp_xml(&trimmed).unwrap();
    let clip = crate::tests::support::first_clip(&project);
    assert_eq!(
        (clip.start_ticks, clip.end_ticks),
        (0, 10 * crate::schema::TICKS)
    );
    assert_eq!(
        (clip.in_ticks, clip.out_ticks),
        (5 * crate::schema::TICKS, 15 * crate::schema::TICKS)
    );
    assert_eq!(clip.playback_rate, 1.0);
    assert!(clip.time_remap.is_none());
    assert_eq!(
        project.media[&clip.media]
            .video
            .as_ref()
            .unwrap()
            .intrinsic_ticks,
        20 * crate::schema::TICKS
    );
    clip.validate(
        project.single_sequence().unwrap().frame_rate,
        &project.media[&clip.media],
    )
    .unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].kind, crate::OmissionKind::Approximated);
    assert_eq!(omissions[0].scope, crate::OmissionScope::Feature);
    assert!(
        omissions[0]
            .reason
            .contains("TimeRemapping keys do not cover the placement"),
        "{omissions:?}"
    );
    assert!(
        omissions[0].reason.contains("saved constant-rate playback"),
        "{omissions:?}"
    );
    let document = project_document_with_media(project.single_sequence().unwrap(), &project.media);
    let layer = &document["composition"]["layers"][0];
    assert_eq!(layer["type"], "Video");
    assert_eq!(
        *crate::test_support::layer_range(layer),
        json!({"start": 0, "duration": 10000})
    );
    assert_eq!(
        layer["sourceRange"],
        json!({"start": 5000, "duration": 10000})
    );
    assert_eq!(layer["playback"]["mapping"]["type"], "linear");
    assert!(layer["source"]["assetId"].as_str().is_some());
}

#[test]
fn native_time_remap_optional_parse_failure_retains_saved_trim() {
    let native = trimmed_native_ramp_xml();
    let parsed = roxmltree::Document::parse(&native).unwrap();
    let record = parsed
        .descendants()
        .find(|node| {
            node.has_tag_name("TimeComponentParam") && node.attribute("ObjectID") == Some("147")
        })
        .unwrap();
    let keys = record
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap();
    let xml = native.replace(&native[keys.range()], "<Keyframes></Keyframes>");
    for reverse in [false, true] {
        let xml = if reverse {
            xml.replace(
                "<InPoint>1270080000000</InPoint>",
                "<PlayBackwards>true</PlayBackwards><InPoint>1270080000000</InPoint>",
            )
        } else {
            xml.clone()
        };
        let (project, omissions) = read_native_ramp_xml(&xml).unwrap();
        let clip = crate::tests::support::first_clip(&project);
        assert_eq!(
            (clip.in_ticks, clip.out_ticks),
            (5 * crate::schema::TICKS, 15 * crate::schema::TICKS)
        );
        assert_eq!(
            (clip.start_ticks, clip.end_ticks),
            (0, 10 * crate::schema::TICKS)
        );
        assert_eq!(clip.playback_rate, if reverse { -1.0 } else { 1.0 });
        assert!(clip.time_remap.is_none());
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(omissions[0].kind, crate::OmissionKind::Approximated);
        assert!(
            omissions[0].reason.contains("fewer than two usable keys"),
            "{omissions:?}"
        );
        let wire = project_document_with_media(project.single_sequence().unwrap(), &project.media);
        let layer = &wire["composition"]["layers"][0];
        assert_eq!(layer["type"], "Video");
        assert_eq!(
            layer["sourceRange"],
            json!({"start": 5000, "duration": 10000})
        );
        assert!(layer["source"]["assetId"].as_str().is_some());
        if reverse {
            let keys = crate::tests::support::playback_keys(layer);
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0]["time"], 0);
            assert_eq!(keys[0]["value"], 15000);
            assert_eq!(keys[1]["time"], 10000);
            assert_eq!(keys[1]["value"], 5000);
        }
    }
}

#[test]
fn native_time_remap_unsupported_headers_retain_saved_base_and_sibling_curve() {
    // Structural edits of the native two-placement source. Only the first
    // placement's mapping/parameter header changes; its saved base is valid.
    use sha2::{Digest, Sha256};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixture = fixtures.join("feature_time_remap_trimmed_speed_26_5_strict.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&fixture).unwrap())),
        "b370546ee11c8cf81395a0fb9acbe7bb0c469aae8733a9f85de96fa6729cd58a"
    );
    // Provenance only: this reader test consumes saved XML media facts, not
    // decoded MP4 samples or public package/media-admission operations.
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(std::fs::read(fixtures.join("feature_timecoded_source.mp4")).unwrap())
        ),
        "4256ae026cb923ee0498374a1def4dd8c0e51078099a9f415726935e198ac0fe"
    );
    let xml = read_xml(&fixture).unwrap();
    let (control, _) = read_native_ramp_xml(&xml).unwrap();
    let clips: Vec<_> = control
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].start_ticks, 0);
    assert_eq!(clips[1].start_ticks, 2 * crate::schema::TICKS);
    assert!(clips.iter().all(|clip| clip.time_remap.is_some()));
    let control_document =
        project_document_with_media(control.single_sequence().unwrap(), &control.media);
    let document = roxmltree::Document::parse(&xml).unwrap();
    for (tag, id, field, value, reason) in [
        (
            "TimeRemapping",
            "106",
            "Version",
            "99",
            "incompatible TimeRemapping binding",
        ),
        (
            "TimeRemapping",
            "106",
            "ClassID",
            "00000000-0000-0000-0000-000000000000",
            "incompatible TimeRemapping binding",
        ),
        (
            "TimeComponentParam",
            "108",
            "Version",
            "10",
            "incompatible TimeRemapping binding",
        ),
        (
            "TimeComponentParam",
            "108",
            "ClassID",
            "00000000-0000-0000-0000-000000000000",
            "incompatible TimeRemapping binding",
        ),
        (
            "TimeComponentParam",
            "108",
            "ParameterID",
            "42",
            "incompatible TimeRemapping binding",
        ),
    ] {
        let node = document
            .descendants()
            .find(|node| node.has_tag_name(tag) && node.attribute("ObjectID") == Some(id))
            .unwrap();
        let record = &xml[node.range()];
        let (from, to) = if field == "ParameterID" {
            (
                "<ParameterID>-1</ParameterID>".to_owned(),
                format!("<ParameterID>{value}</ParameterID>"),
            )
        } else {
            (
                format!("{field}=\"{}\"", node.attribute(field).unwrap()),
                format!("{field}=\"{value}\""),
            )
        };
        assert_eq!(record.matches(&from).count(), 1);
        let mut edited_record = record.replacen(&from, &to, 1);
        if field == "ClassID" {
            // An unknown class need not match the known typed payload grammar.
            let closing = format!("</{tag}>");
            edited_record = edited_record.replace(
                &closing,
                &format!("<UnknownCurveLayout>opaque</UnknownCurveLayout>{closing}"),
            );
        }
        let edited = xml.replacen(record, &edited_record, 1);
        if field == "Version" {
            let (project, notes) = read_native_ramp_xml(&edited).unwrap();
            let kept = project
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .collect::<Vec<_>>();
            assert_eq!(kept.len(), 2, "{tag}:{id} {notes:?}");
            for (current, original) in kept.iter().zip(&clips) {
                assert_eq!(
                    (
                        current.start_ticks,
                        current.end_ticks,
                        current.in_ticks,
                        current.out_ticks
                    ),
                    (
                        original.start_ticks,
                        original.end_ticks,
                        original.in_ticks,
                        original.out_ticks
                    )
                );
                let keys = |clip: &crate::schema::PrVideoOccurrence| {
                    clip.time_remap
                        .as_ref()
                        .unwrap()
                        .keys
                        .iter()
                        .map(|key| (key.timeline_ticks, key.source_ticks, key.easing))
                        .collect::<Vec<_>>()
                };
                assert_eq!(keys(current), keys(original));
            }
            assert!(read_remap(&edited, "106").is_ok());
            continue;
        }
        // Unknown optional layouts must not be decoded as known source-time
        // curves. Their independently bound physical source and base survive.
        let (project, notes) = read_native_ramp_xml(&edited).unwrap();
        let kept: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .collect();
        assert_eq!(kept.len(), 2, "{tag}:{id} {field}: {notes:?}");
        for (current, original) in kept.iter().zip(&clips) {
            assert_eq!(
                (
                    &current.media,
                    current.start_ticks,
                    current.end_ticks,
                    current.in_ticks,
                    current.out_ticks,
                    current.playback_rate,
                    current.opacity,
                    current.enabled,
                ),
                (
                    &original.media,
                    original.start_ticks,
                    original.end_ticks,
                    original.in_ticks,
                    original.out_ticks,
                    original.playback_rate,
                    original.opacity,
                    original.enabled,
                )
            );
            assert_eq!(current.transform, original.transform);
            assert_eq!(current.blend_mode, original.blend_mode);
            current
                .validate(
                    project.single_sequence().unwrap().frame_rate,
                    &project.media[&current.media],
                )
                .unwrap();
        }
        assert!(kept[0].time_remap.is_none());
        let wire = project_document_with_media(project.single_sequence().unwrap(), &project.media);
        fn at(document: &serde_json::Value, start: u64) -> &serde_json::Value {
            document["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|layer| {
                    layer["type"] == "Video" && layer["playback"]["inputRange"]["start"] == start
                })
                .unwrap()
        }
        assert_eq!(at(&wire, 2000), at(&control_document, 2000));
        let retained = at(&wire, 0);
        let original = at(&control_document, 0);
        for field in [
            "source",
            "sourceIntrinsicDuration",
            "transform",
            "blendMode",
        ] {
            assert_eq!(retained[field], original[field], "{field}");
        }
        assert_eq!(retained["sourceIntrinsicDuration"], 10000);
        assert!(retained["source"]["assetId"].as_str().is_some());
        assert_eq!(
            retained["sourceRange"],
            json!({"start": 400, "duration": 1600})
        );
        assert_eq!(
            retained["playback"]["inputRange"],
            json!({"start": 0, "duration": 2000})
        );
        let keys = crate::tests::support::playback_keys(retained);
        assert_eq!(keys.len(), 2);
        for (key, (time, value)) in keys.iter().zip([(0, 400), (2000, 2000)]) {
            assert_eq!(key["time"], time);
            assert_eq!(key["value"], value);
            assert_eq!(key["easing"], json!({"type": "linear"}));
        }
        assert!(
            notes.iter().any(|note| {
                note.kind == crate::OmissionKind::Approximated
                    && note.scope == crate::OmissionScope::Feature
                    && note.reason.contains(&format!("{tag}:{id}"))
                    && note.reason.contains(reason)
                    && note.reason.contains("saved constant-rate playback")
            }),
            "{tag}:{id} {field}: {notes:?}"
        );
        assert!(!notes
            .iter()
            .any(|note| note.scope == crate::OmissionScope::Occurrence));
        let error = read_remap(&edited, "106").unwrap_err();
        assert!(
            matches!(error, crate::error::BuildError::Unsupported(_)),
            "{error:?}"
        );
    }
}

#[test]
fn native_time_remap_unknown_binding_does_not_silently_admit_nonphysical_hosts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_images_nests_26_5.prproj");
    let xml = read_xml(&path).unwrap();
    let sequence = Some("f3c651e6-0302-4499-b6f5-814b7b22c207");
    let (base, _) = crate::format::inspect_project_with_omissions(&xml, sequence).unwrap();
    let ramp = version_8_ramp();
    let start = ramp.find(r#"<TimeRemapping ObjectID="146""#).unwrap();
    let end = start
        + ramp[start..].find("</TimeComponentParam>").unwrap()
        + "</TimeComponentParam>".len();
    let records = ramp[start..end]
        .replace("\"146\"", "\"10001\"")
        .replace("\"147\"", "\"10002\"")
        .replace(
            crate::schema::records::TIME_REMAPPING.class_id,
            "00000000-0000-0000-0000-000000000000",
        );
    let document = roxmltree::Document::parse(&xml).unwrap();
    for (clip_id, nest) in [("179", true), ("262", false)] {
        let clip = document
            .descendants()
            .find(|node| {
                node.has_tag_name("VideoClip") && node.attribute("ObjectID") == Some(clip_id)
            })
            .unwrap();
        let native = &xml[clip.range()];
        let edited = native.replace("</Clip>", "<TimeRemapping ObjectRef=\"10001\" /></Clip>");
        let changed = xml
            .replacen(native, &edited, 1)
            .replace("</PremiereData>", &format!("{records}</PremiereData>"));
        let (project, notes) =
            crate::format::inspect_project_with_omissions(&changed, sequence).unwrap();
        let original = base.single_sequence().unwrap();
        let changed = project.single_sequence().unwrap();
        assert_eq!(
            changed.nest_occurrences().count() + usize::from(nest),
            original.nest_occurrences().count(),
            "{clip_id}: {notes:?}"
        );
        assert_eq!(
            changed.video_occurrences().count() + usize::from(!nest),
            original.video_occurrences().count(),
            "{clip_id}: {notes:?}"
        );
        assert!(
            notes.iter().any(|note| {
                note.scope == crate::OmissionScope::Occurrence
                    && note.reason.contains("incompatible TimeRemapping binding")
            }),
            "{clip_id}: {notes:?}"
        );
        assert!(
            !notes
                .iter()
                .any(|note| note.reason.contains("saved constant-rate playback")),
            "{clip_id}: {notes:?}"
        );
    }
}

#[test]
fn native_time_remap_recovers_non_rotation_motion_and_keeps_sibling_curve() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_time_remap_rotation_26_5_strict.prproj");
    let native = read_xml(&fixture).unwrap();
    // Give the first physical placement a valid saved unit-rate base and
    // Position keys. The other placement remains the native healthy sibling.
    let xml = native.replace(
        "<PlaybackSpeed>0.8</PlaybackSpeed>\n\t\t\t<InPoint>101606400000</InPoint>",
        "<PlaybackSpeed>1</PlaybackSpeed>\n\t\t\t<InPoint>0</InPoint>",
    ).replace(
        "<Name>Position</Name>",
        "<Name>Position</Name><Keyframes>0,0.5:0.5,0,0,0,0,0,0,0,0,0,0,0,0;508032000000,0.75:0.5,0,0,0,0,0,0,0,0,0,0,0,0;</Keyframes>",
    );
    assert_ne!(xml, native);
    let absent = xml.replace("<TimeRemapping ObjectRef=\"118\"/>", "");
    assert_ne!(absent, xml);
    let (control, _) = read_native_ramp_xml(&absent).unwrap();
    let first = control
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .find(|clip| clip.start_ticks == 0)
        .unwrap();
    assert_eq!(first.playback_rate, 1.0);
    assert_eq!(
        (first.in_ticks, first.out_ticks),
        (0, 2 * crate::schema::TICKS)
    );
    assert!(first.animations.iter().any(
        |animation| matches!(animation, crate::schema::PrPropertyAnimation::Position(keys)
            if keys.len() == 2 && keys[1].value == [0.75, 0.5])
    ));
    let document = roxmltree::Document::parse(&xml).unwrap();
    let parameter = document
        .descendants()
        .find(|node| {
            node.has_tag_name("TimeComponentParam") && node.attribute("ObjectID") == Some("120")
        })
        .unwrap();
    let rejected = xml.replace(
        &xml[parameter.range()],
        &xml[parameter.range()].replace(
            "<Name>Speed</Name>",
            "<Name>Speed</Name><ParameterControlType>99</ParameterControlType>",
        ),
    );
    assert_ne!(rejected, xml);
    for curve in [&xml, &rejected] {
        let (project, omissions) = read_native_ramp_xml(curve).unwrap();
        let clips: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .collect();
        assert_eq!(clips.len(), 2, "{omissions:?}");
        assert_eq!(clips[0].start_ticks, 0);
        assert!(clips[0].time_remap.is_none());
        assert_eq!(clips[0].playback_rate, 1.0);
        assert_eq!((clips[0].in_ticks, clips[0].out_ticks), (0, 2 * TICKS));
        assert!(clips[0].animations.iter().any(|animation|matches!(animation,crate::schema::PrPropertyAnimation::Position(keys) if keys.len()==2 && keys[1].value==[0.75,0.5])));
        assert_eq!(clips[1].start_ticks, 2 * TICKS);
        assert!(clips[1].time_remap.is_some());
        assert!(
            omissions.iter().any(
                |omission| omission.kind == crate::OmissionKind::Approximated
                    && omission
                        .reason
                        .contains("picture and Motion controls retained")
            ),
            "{omissions:?}"
        );
        assert!(
            !omissions
                .iter()
                .any(|omission| omission.scope == crate::OmissionScope::Occurrence),
            "{omissions:?}"
        );
    }
}

#[test]
fn rejected_native_time_remap_recovers_saved_span_without_normalizing_unit_source_out() {
    // One native 30 fps frame beyond the saved unit-rate span is stale only
    // when remapping was genuinely absent, not when its curve was rejected.
    let xml = trimmed_native_ramp_xml().replace(
        "<OutPoint>3810240000000</OutPoint>",
        "<OutPoint>3818707200000</OutPoint>",
    );
    let rejected = xml.replace(
        "<ParameterControlType>21</ParameterControlType>",
        "<ParameterControlType>99</ParameterControlType>",
    );
    let (project, notes) = read_native_ramp_xml(&rejected).unwrap();
    let clip = crate::tests::support::first_clip(&project);
    assert_eq!(
        (clip.in_ticks, clip.out_ticks),
        (5 * TICKS, 15 * TICKS + TICKS / 30)
    );
    assert!((clip.playback_rate - (10.0 + 1.0 / 30.0) / 10.0).abs() < 1e-12);
    assert!(clip.time_remap.is_none());
    assert!(
        notes
            .iter()
            .any(|note| note.kind == crate::OmissionKind::Approximated
                && note.reason.contains("bounded authored source trim")),
        "{notes:?}"
    );

    let absent = xml.replace("<TimeRemapping ObjectRef=\"146\" />", "");
    assert_ne!(absent, xml);
    let (project, omissions) = read_native_ramp_xml(&absent).unwrap();
    let clip = crate::tests::support::first_clip(&project);
    assert_eq!(
        (clip.start_ticks, clip.end_ticks),
        (0, 10 * crate::schema::TICKS)
    );
    assert_eq!(
        (clip.in_ticks, clip.out_ticks),
        (5 * crate::schema::TICKS, 15 * crate::schema::TICKS)
    );
    assert_eq!(clip.playback_rate, 1.0);
    assert!(clip.time_remap.is_none());
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[test]
fn native_time_remap_recovery_rejects_required_clocks_and_graph_errors() {
    let xml = trimmed_native_ramp_xml();
    // Saved caches/playback metadata can recover a physically bounded source
    // selection; actual frame grids and required graph identity remain fatal.
    for (from,to,expected_out,expected_rate) in [
        ("<OutPoint>3810240000000</OutPoint>","<OutPoint>3556224000000</OutPoint>",14*TICKS,0.9),
        ("<Duration>5080320000000</Duration>","<Duration>2540160000000</Duration>",10*TICKS,0.5),
        ("<OriginalDuration>5080320000000</OriginalDuration>","<OriginalDuration>2540160000000</OriginalDuration>",15*TICKS,1.0),
        ("<InPoint>1270080000000</InPoint>","<PlaybackSpeed>0</PlaybackSpeed><InPoint>1270080000000</InPoint>",15*TICKS,1.0),
        ("<InPoint>1270080000000</InPoint>","<PlaybackSpeed>NaN</PlaybackSpeed><InPoint>1270080000000</InPoint>",15*TICKS,1.0),
        ("<ClipID>0ad61a94-8103-4385-bc5d-86f00a438773</ClipID>\n\t\t</Clip>","<ClipID>0ad61a94-8103-4385-bc5d-86f00a438773</ClipID>\n\t\t</Clip><FrameHold>4</FrameHold><FrameHoldStart>0</FrameHoldStart>",15*TICKS,1.0),
    ] {
        assert!(xml.contains(from),"{from}");
        let (project,notes)=read_native_ramp_xml(&xml.replace(from,to)).unwrap();
        let clip=crate::tests::support::first_clip(&project);
        assert_eq!((clip.in_ticks,clip.out_ticks),(5*TICKS,expected_out));
        assert!((clip.playback_rate-expected_rate).abs()<1e-12,"{notes:?}");
        assert!(clip.time_remap.is_none());
        clip.validate(project.single_sequence().unwrap().frame_rate,&project.media[&clip.media]).unwrap();
        assert!(notes.iter().any(|note|note.kind==crate::OmissionKind::Approximated),"{notes:?}");
    }
    for (from, to, reason) in [
        (
            "<End>2540160000000</End>",
            "<End>2540159999999</End>",
            "frame boundary",
        ),
        (
            "<Source ObjectRef=\"19\"/>",
            "<Source ObjectRef=\"999999\"/>",
            "missing reference",
        ),
        (
            "<TimeRemapping ObjectRef=\"146\"/>",
            "<TimeRemapping ObjectRef=\"999999\"/>",
            "missing reference",
        ),
        (
            "<Keyframes ObjectRef=\"147\"/>",
            "<Keyframes ObjectRef=\"19\"/>",
            "expected TimeComponentParam",
        ),
    ] {
        // Saved native whitespace varies; edit the exact consumed reference.
        let from = from.replace("/>", " />");
        let to = to.replace("/>", " />");
        assert!(xml.contains(&from), "missing mutation {from}");
        let error = read_native_ramp_xml(&xml.replace(&from, &to))
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{from}: {error}");
    }
    // An overlong source selection recovers only its remaining physical interval.
    let out_of_bounds = xml
        .replace(
            "<Duration>5080320000000</Duration>",
            "<Duration>2540160000000</Duration>",
        )
        .replace(
            "<OriginalDuration>5080320000000</OriginalDuration>",
            "<OriginalDuration>2540160000000</OriginalDuration>",
        );
    let (project, notes) = read_native_ramp_xml(&out_of_bounds).unwrap();
    let clip = crate::tests::support::first_clip(&project);
    assert_eq!((clip.in_ticks, clip.out_ticks), (5 * TICKS, 10 * TICKS));
    assert_eq!(clip.playback_rate, 0.5);
    assert!(clip.time_remap.is_none());
    clip.validate(
        project.single_sequence().unwrap().frame_rate,
        &project.media[&clip.media],
    )
    .unwrap();
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("bounded authored source trim")),
        "{notes:?}"
    );
    // Required record decoding/graph identity never becomes optional Unsupported.
    for (xml, decode) in [
        (
            version_8_ramp().replace(
                "<Keyframes ObjectRef=\"147\" />",
                "<Keyframes ObjectRef=\"999999\" />",
            ),
            false,
        ),
        (
            version_8_ramp().replace("<ParameterID>-1</ParameterID>", ""),
            true,
        ),
    ] {
        let error = read_remap(&xml, "146").unwrap_err();
        if decode {
            assert!(
                matches!(
                    error,
                    crate::error::BuildError::Premiere(crate::format::FormatError::Decode { .. })
                ),
                "{error:?}"
            );
        } else {
            assert!(
                matches!(
                    error,
                    crate::error::BuildError::Premiere(crate::format::FormatError::Invalid(_))
                ),
                "{error:?}"
            );
        }
        assert!(read_native_ramp_xml(&xml).is_err());
    }
}

#[test]
fn native_time_remap_recovery_preserves_still_base_and_omits_uncovered_nest() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_images_nests_26_5.prproj");
    let xml = read_xml(&path).unwrap();
    let sequence = Some("f3c651e6-0302-4499-b6f5-814b7b22c207");
    let (base, _) = crate::format::inspect_project_with_omissions(&xml, sequence).unwrap();
    let ramp = version_8_ramp();
    let start = ramp.find(r#"<TimeRemapping ObjectID="146""#).unwrap();
    let end = start
        + ramp[start..].find("</TimeComponentParam>").unwrap()
        + "</TimeComponentParam>".len();
    let records = ramp[start..end]
        .replace("\"146\"", "\"10001\"")
        .replace("\"147\"", "\"10002\"")
        .replace(
            "<ParameterControlType>21</ParameterControlType>",
            "<ParameterControlType>99</ParameterControlType>",
        );
    for (clip_id, kind) in [("179", "nest"), ("262", "still")] {
        let document = roxmltree::Document::parse(&xml).unwrap();
        let clip = document
            .descendants()
            .find(|node| {
                node.has_tag_name("VideoClip") && node.attribute("ObjectID") == Some(clip_id)
            })
            .unwrap();
        let native = &xml[clip.range()];
        let edited = native.replace("</Clip>", "<TimeRemapping ObjectRef=\"10001\" /></Clip>");
        let changed = xml
            .replacen(native, &edited, 1)
            .replace("</PremiereData>", &format!("{records}</PremiereData>"));
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&changed, sequence).unwrap();
        let original = base.single_sequence().unwrap();
        let changed = project.single_sequence().unwrap();
        let lost_picture = 0;
        let lost_nest = usize::from(kind == "nest");
        assert_eq!(
            changed.video_occurrences().count() + lost_picture,
            original.video_occurrences().count(),
            "{kind}: {omissions:?}"
        );
        assert_eq!(
            changed.nest_occurrences().count() + lost_nest,
            original.nest_occurrences().count(),
            "{kind}: {omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains("TimeRemapping")),
            "{kind}: {omissions:?}"
        );
        if kind == "still" {
            let original = original
                .video_occurrences()
                .find(|clip| clip.id.as_deref() == Some("VideoClipTrackItem:141"))
                .unwrap();
            let retained = changed
                .video_occurrences()
                .find(|clip| clip.id == original.id)
                .unwrap();
            assert_eq!(
                (
                    &retained.media,
                    retained.start_ticks,
                    retained.end_ticks,
                    retained.in_ticks,
                    retained.out_ticks
                ),
                (
                    &original.media,
                    original.start_ticks,
                    original.end_ticks,
                    original.in_ticks,
                    original.out_ticks
                )
            );
            assert_eq!(retained.transform, original.transform);
            assert_eq!(retained.opacity, original.opacity);
            assert!(retained.time_remap.is_none());
            retained
                .validate(changed.frame_rate, &project.media[&retained.media])
                .unwrap();
            assert!(
                omissions
                    .iter()
                    .any(|note| note.kind == crate::OmissionKind::Approximated
                        && note.reason.contains("bounded authored source trim")),
                "{omissions:?}"
            );
        } else {
            assert!(
                omissions
                    .iter()
                    .any(|note| note.scope == crate::OmissionScope::Occurrence
                        && note.reason.contains(
                            "invalid, uncovered or out-of-bounds nested TimeRemapping curve"
                        )),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn trimmed_variable_speed_ramp_recovers_picture_and_source_trim_with_diagnostic() {
    let (project, omissions) = read_native_ramp_xml(&trimmed_native_ramp_xml()).unwrap();
    let clip = project.single_sequence().unwrap().video_tracks[0].clip(0);
    assert_eq!(clip.in_ticks, 5 * TICKS);
    assert_eq!(clip.out_ticks, 15 * TICKS);
    assert_eq!(clip.playback_rate, 1.0);
    assert!(clip.time_remap.is_none());
    assert!(
        omissions
            .iter()
            .any(|loss| loss.kind == crate::OmissionKind::Approximated
                && loss.reason.contains("TimeRemapping")),
        "{omissions:?}"
    );
}

#[test]
fn time_remapping_from_another_in_or_speed_validates_only_on_physical_video() {
    use crate::{
        format::FrameRate,
        schema::{MediaId, PrMediaKind, PrTimeRemapKeyframe, TICKS},
        tests::support::clip_of,
    };
    // A 2 s placement that plays a curve from In 1 s to Out 2 s at 0.5x: its
    // keys, at input 0 and 2 s, are 1 s before and after In.
    let mut clip = clip_of("source", 0..2 * TICKS, TICKS);
    clip.playback_rate = 0.5;
    clip.out_ticks = 2 * TICKS;
    clip.time_remap = Some(PrTimeRemap {
        keys: [(-TICKS, 0), (TICKS, 2 * TICKS)]
            .map(|(timeline_ticks, source_ticks)| PrTimeRemapKeyframe {
                timeline_ticks,
                source_ticks,
                easing: PrKeyframeEasing::Linear,
            })
            .to_vec(),
    });
    let mut media = video_media();
    let facts = media.get_mut(&MediaId("source".into())).unwrap();
    clip.validate(FrameRate::Fps30, facts).unwrap();
    // A still, like a Color Matte or a linked composition, is not measured.
    facts.video.as_mut().unwrap().kind = PrMediaKind::Still { alpha: false };
    let error = clip
        .validate(FrameRate::Fps30, facts)
        .unwrap_err()
        .to_string();
    let reason =
        "TimeRemapping from a source In or at another speed is converted only on physical video";
    assert!(error.contains(reason), "{error}");
}

#[test]
fn unit_speed_time_remapping_requires_its_exact_span() {
    use crate::{
        format::FrameRate,
        schema::{MediaId, PrTimeRemapKeyframe, TICKS},
        tests::support::clip_of,
    };
    // An untrimmed curve at unit speed: an Out one tick from the end of its
    // 2 s placement omits it, though another speed admits Adobe's truncated
    // product.
    let media = video_media();
    let facts = &media[&MediaId("source".into())];
    for (out_ticks, admitted) in [
        (2 * TICKS, true),
        (2 * TICKS - 1, false),
        (2 * TICKS + 1, false),
    ] {
        let mut clip = clip_of("source", 0..2 * TICKS, 0);
        clip.out_ticks = out_ticks;
        clip.time_remap = Some(PrTimeRemap {
            keys: [(0, 0), (3 * TICKS, 3 * TICKS)]
                .map(|(timeline_ticks, source_ticks)| PrTimeRemapKeyframe {
                    timeline_ticks,
                    source_ticks,
                    easing: PrKeyframeEasing::Linear,
                })
                .to_vec(),
        });
        let error = clip
            .validate(FrameRate::Fps30, facts)
            .err()
            .map(|error| error.to_string());
        let reason = "TimeRemapping In to Out must match the clip's forward playback rate";
        assert_eq!(error.is_none(), admitted, "{out_ticks}: {error:?}");
        assert!(
            error
                .as_deref()
                .is_none_or(|message| message.contains(reason)),
            "{error:?}"
        );
    }
}

#[test]
fn minimal_variable_speed_ramp_preserves_scaled_native_curve() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_time_remap_variable_speed_strict.prproj");
    let xml = read_xml(&path).unwrap();
    let graph = Graph::parse(&xml).unwrap();
    let record = graph
        .records()
        .find(|record| record.identity() == "VideoClip:142")
        .unwrap();
    let clip = graph.decode::<VideoClip>(record).unwrap();
    let reference = clip
        .value
        .clip
        .as_ref()
        .and_then(|clip| clip.time_remapping.as_ref())
        .unwrap();
    let remap = read_time_remapping(&graph, reference, &clip.identity, &mut Vec::new()).unwrap();

    assert_eq!(
        remap
            .keys
            .iter()
            .map(|key| (key.timeline_ticks, key.source_ticks))
            .collect::<Vec<_>>(),
        vec![
            (0, 0),
            (44_172_202_623, 44_172_202_623),
            (56_390_045_901, 64_576_000_898),
            (88_124_651_607, 138_834_978_249),
            (123_016_798_944, 197_104_864_302),
            (154_577_299_241, 228_665_364_599),
            (209_087_676_946, 264_097_110_107),
            (508_032_000_000, 353_780_407_024),
        ]
    );
    for index in [2, 4, 6] {
        assert!(matches!(
            remap.keys[index].easing,
            PrKeyframeEasing::CubicBezier { .. }
        ));
    }

    let mut sequence = video_sequence();
    let occurrence = sequence.video_tracks[0].clip_mut(0);
    occurrence.end_ticks = remap.keys.last().unwrap().timeline_ticks;
    occurrence.out_ticks = occurrence.end_ticks;
    occurrence.time_remap = Some(remap);
    let media = video_media();

    let document = project_document_with_media(&sequence, &media);
    let layer = &document["composition"]["layers"][0];
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(layer["sourceRange"], json!({"start": 0, "duration": 10000}));
    let playback = &layer["playback"]["mapping"]["property"];
    assert_eq!(playback["before"], "continue");
    assert_eq!(playback["after"], "continue");
    assert_eq!(
        playback["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| (
                key["time"].as_u64().unwrap(),
                key["value"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, 0),
            (174, 174),
            (222, 254),
            (347, 547),
            (484, 776),
            (609, 900),
            (823, 1040),
            (2000, 1393),
        ]
    );
}

/// The unchanged `TimeRemapping:102` and its Version 9 Speed parameter
/// `TimeComponentParam:103` of a Premiere 26.5.1 save, in a document of their
/// own.
fn version_9_ramp() -> String {
    format!(
        "<PremiereData Version=\"3\">\n\t{}</PremiereData>\n",
        include_str!("../../../tests/fixtures/native-time-remapping-version-9.xml")
    )
}

/// The pinned project whose Version 8 ramp, `TimeRemapping:146` and its Speed
/// parameter `TimeComponentParam:147`, the Version 9 save re-saved.
fn version_8_ramp() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_time_remap_variable_speed_strict.prproj");
    read_xml(&path).unwrap()
}

/// Reads `TimeRemapping:{id}` of `xml` as a clip's source-time curve.
fn read_remap(xml: &str, id: &str) -> crate::error::Result<PrTimeRemap> {
    let reference = Reference {
        id: Some(id.to_owned()),
        uid: None,
        index: None,
    };
    read_time_remapping(
        &Graph::parse(xml).unwrap(),
        &reference,
        "test clip",
        &mut Vec::new(),
    )
}

#[test]
fn native_version_9_time_remap_reads_all_nine_saved_keys() {
    use crate::schema::TICKS;

    let decode = |xml: &str, mapping: &str| -> Vec<_> {
        read_remap(xml, mapping)
            .unwrap()
            .keys
            .iter()
            .map(|key| (key.timeline_ticks, key.source_ticks, key.easing))
            .collect()
    };
    let keys = decode(&version_9_ramp(), "102");
    assert_eq!(
        keys.iter()
            .map(|&(timeline, source, _)| (timeline, source))
            .collect::<Vec<_>>(),
        vec![
            (0, 0),
            (44_172_202_623, 44_172_202_623),
            (56_390_045_901, 64_576_000_898),
            (88_124_651_607, 138_834_978_249),
            (123_016_798_944, 197_104_864_302),
            (154_577_299_241, 228_665_364_599),
            (209_087_676_946, 264_097_110_107),
            (508_032_000_000, 353_780_407_024),
            // The key that Premiere appended: the 10 s media end.
            (2_694_411_592_977, 10 * TICKS),
        ]
    );
    for (index, (_, _, easing)) in keys.iter().enumerate() {
        if matches!(index, 2 | 4 | 6) {
            assert!(
                matches!(easing, PrKeyframeEasing::CubicBezier { .. }),
                "{index}: {easing:?}"
            );
        } else {
            assert_eq!(*easing, PrKeyframeEasing::Linear, "{index}");
        }
    }
    // The Version 8 source of this save decodes to the same first eight keys,
    // ramp easing included: Premiere rewrote only the decimals of their values.
    assert_eq!(keys[..8], decode(&version_8_ramp(), "146")[..]);
}

#[test]
fn time_remap_metadata_differences_retain_all_source_time_keys() {
    let version_9 = version_9_ramp();
    for (xml, mapping) in [
        (
            version_8_ramp().replace("<IsTimeVarying>true</IsTimeVarying>", ""),
            "146",
        ),
        (
            version_9.replace(r#"Version="9""#, r#"Version="10""#),
            "102",
        ),
        (
            version_9.replace(
                "<Name>Speed</Name>",
                "<Name>Localized speed</Name><IsTimeVarying>false</IsTimeVarying>",
            ),
            "102",
        ),
    ] {
        let remap = read_remap(&xml, mapping).unwrap();
        assert!(remap.keys.len() >= 8);
        assert_eq!(remap.keys[0].source_ticks, 0);
        assert!(remap
            .keys
            .windows(2)
            .all(|keys| keys[0].timeline_ticks < keys[1].timeline_ticks));
    }
}

#[test]
fn native_time_remap_preserves_unused_linear_tail_beyond_media() {
    let xml = include_str!("../../../tests/fixtures/native-time-remapping-outside-media.xml");
    for (mapping, end, out, speed, expected) in [
        (
            "1",
            2_317_896_000_000,
            222_761_448_000,
            0.8095,
            vec![
                (8042, 0),
                (8243, 163),
                (8263, 289),
                (8401, 1975),
                (8493, 2636),
                (10078, 6228),
            ],
        ),
        (
            "3",
            2_434_320_000_000,
            391_608_000_000,
            1.0,
            vec![
                (8042, 0),
                (8205, 163),
                (8220, 289),
                (8333, 1975),
                (8407, 2636),
                (8750, 3279),
                (8929, 3488),
                (14922, 6228),
            ],
        ),
    ] {
        let remap = read_remap(xml, mapping).unwrap();
        let mut sequence = video_sequence();
        sequence.frame_rate = crate::schema::FrameRate::Fps24;
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.start_ticks = 2_042_712_000_000;
        clip.end_ticks = end;
        clip.in_ticks = 0;
        clip.out_ticks = out;
        clip.playback_rate = speed;
        clip.time_remap = Some(remap);
        let mut media = video_media();
        media
            .get_mut(&clip.media)
            .unwrap()
            .video
            .as_mut()
            .unwrap()
            .intrinsic_ticks = 1_280_664_000_000;
        clip.validate(crate::schema::FrameRate::Fps24, &media[&clip.media])
            .unwrap();
        let document = project_document_with_media(&sequence, &media);
        let layer = &document["composition"]["layers"][0];
        assert_eq!(layer["sourceRange"], json!({"start": 0, "duration": 5042}));
        let keys = layer["playback"]["mapping"]["property"]["keyframes"]
            .as_array()
            .unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| (
                    key["time"].as_u64().unwrap(),
                    key["value"].as_u64().unwrap()
                ))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(keys[2]["easing"]["type"], "cubicBezier");
        assert_eq!(keys[4]["easing"]["type"], "cubicBezier");
        assert_eq!(keys.last().unwrap()["easing"]["type"], "linear");
    }
}

#[test]
fn native_time_remap_unused_tail_exception_rejects_unsafe_curves() {
    use crate::schema::{FrameRate, TICKS};
    let xml = include_str!("../../../tests/fixtures/native-time-remapping-outside-media.xml");
    for case in [
        "played tail",
        "interior key",
        "nonlinear tail",
        "negative key",
        "reverse tail",
    ] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.in_ticks = 0;
        clip.out_ticks = TICKS;
        clip.end_ticks = TICKS;
        let mut remap = read_remap(xml, "1").unwrap();
        match case {
            "played tail" => {
                clip.out_ticks = 3 * TICKS / 2;
                clip.end_ticks = clip.out_ticks;
            }
            "interior key" => remap.keys[4].source_ticks = 6 * TICKS,
            "nonlinear tail" => {
                remap.keys[5].easing = PrKeyframeEasing::CubicBezier {
                    x1: 1.0 / 3.0,
                    y1: 0.0,
                    x2: 2.0 / 3.0,
                    y2: 1.0,
                }
            }
            "negative key" => remap.keys[0].source_ticks = -1,
            "reverse tail" => remap.keys[5].source_ticks = TICKS,
            _ => unreachable!(),
        }
        clip.time_remap = Some(remap);
        let mut media = video_media();
        media
            .get_mut(&clip.media)
            .unwrap()
            .video
            .as_mut()
            .unwrap()
            .intrinsic_ticks = 1_280_664_000_000;
        let error = clip
            .validate(FrameRate::Fps30, &media[&clip.media])
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid or unsupported TimeRemapping curve"),
            "{case}: {error}"
        );
    }
}

#[test]
fn time_remap_recovery_skips_unusable_key_and_retains_editable_picture() {
    let xml = version_9_ramp().replace("56390045901,0.254220210136270885481480", "56390045901,NaN");
    let graph = Graph::parse(&xml).unwrap();
    let reference = Reference {
        id: Some("102".into()),
        uid: None,
        index: None,
    };
    let mut omissions = Vec::new();
    let remap =
        read_time_remapping(&graph, &reference, "retained picture", &mut omissions).unwrap();
    assert_eq!(remap.keys.len(), 8);
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.end_ticks = 2 * TICKS;
    clip.out_ticks = 2 * TICKS;
    clip.transform.rotation = 25.0;
    clip.time_remap = Some(remap);
    let media = video_media();
    let wire = project_document_with_media(&sequence, &media);
    let layer = &wire["composition"]["layers"][0];
    assert!(layer["source"]["assetId"].is_string());
    assert_eq!(
        crate::test_support::layer_range(layer),
        &json!({"start": 0, "duration": 2000})
    );
    assert_eq!(layer["transform"]["rotation"], 25.0);
    assert!(
        layer["playback"]["mapping"]["property"]["keyframes"]
            .as_array()
            .unwrap()
            .len()
            >= 5
    );
    assert!(
        omissions
            .iter()
            .any(|loss| loss.reason.contains("unusable TimeRemapping key")
                && loss.reason.contains("retained")),
        "{omissions:?}"
    );
}
