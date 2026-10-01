use crate::{
    format::{read_xml, reader::time_remap::read_time_remapping, Graph},
    schema::{native::VideoClip, PrKeyframeEasing},
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
    // range to 5..15 seconds. The importer cannot retain those trim boundaries.
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
        error.contains("trimmed TimeRemapping clips are not supported"),
        "{error}"
    );
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
