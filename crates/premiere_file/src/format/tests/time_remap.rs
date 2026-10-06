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
    let remap = read_time_remapping(&graph, reference, &clip.identity).unwrap();
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
    let remap = read_time_remapping(&graph, reference, &clip.identity).unwrap();

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

#[test]
fn trimmed_variable_speed_ramp_is_omitted_with_diagnostic() {
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
    let trimmed = xml
        .replace("508032000000", "2540160000000")
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
        );
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("trimmed-time-remap.prproj");
    let mut zip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    zip.write_all(trimmed.as_bytes()).unwrap();
    std::fs::write(&path, zip.finish().unwrap()).unwrap();

    let error = crate::format::PrProjectFile::load_selected(&path, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no convertible timelines"), "{error}");
    assert!(
        error.contains("TimeRemapping keys do not cover the placement"),
        "{error}"
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
    let remap = read_time_remapping(&graph, reference, &clip.identity).unwrap();

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
    read_time_remapping(&Graph::parse(xml).unwrap(), &reference, "test clip")
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
fn time_remap_flags_are_optional_only_in_version_9_and_still_checked_when_saved() {
    // Supplementary synthetic edits of the native Version 9 records and of the
    // pinned Version 8 ramp, which saves every flag.
    let version_9 = version_9_ramp();
    for (xml, mapping, reason) in [
        (
            version_8_ramp().replace("<IsTimeVarying>true</IsTimeVarying>", ""),
            "146",
            "TimeComponentParam:147: missing IsTimeVarying",
        ),
        (
            version_9.replace(r#"Version="9""#, r#"Version="10""#),
            "102",
            "TimeComponentParam:103: unsupported TimeRemapping parameter",
        ),
        (
            version_9.replace(
                "<Name>Speed</Name>",
                "<Name>Speed</Name><IsTimeVarying>false</IsTimeVarying>",
            ),
            "102",
            r#"TimeComponentParam:103: unsupported TimeRemapping parameter IsTimeVarying "false""#,
        ),
    ] {
        let error = read_remap(&xml, mapping).unwrap_err().to_string();
        assert!(error.contains(reason), "{reason}: {error}");
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
