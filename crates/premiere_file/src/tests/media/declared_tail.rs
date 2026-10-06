//! Supplemental container edits exercising selected-source admission, not an Adobe render oracle.
use super::*;
use crate::media::{SampleClock, VideoUse};
use crate::schema::PrVideoStream;

fn declared_tail_bytes() -> Vec<u8> {
    let bytes = sample_grid_bytes(&[0, 1000, 2000, 2999, 3999, 4999, 5999], 30000);
    let bytes = patch_box(&bytes, MDHD, |table| write_u32(table, 16, 6000));
    let bytes = patch_box(&bytes, MVHD, |table| write_u32(table, 16, 6000));
    let bytes = patch_box(&bytes, TKHD, |table| write_u32(table, 20, 6000));
    patch_box(&bytes, ELST, |table| write_u32(table, 8, 6000))
}

fn source() -> PrVideoStream {
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate = FrameRate::Fps30.into();
    source.intrinsic_ticks = 6 * FrameRate::Fps30.ticks_per_frame();
    source
}

fn interior_use() -> VideoUse {
    VideoUse {
        ranges: std::iter::once(0..4 * FrameRate::Fps30.ticks_per_frame()).collect(),
        uses_audio: false,
    }
}

#[test]
fn declared_tail_selected_picture_keeps_exact_sample_clock() {
    let bytes = declared_tail_bytes();
    let usage = interior_use();
    let file = inspect_selected(&bytes, usage.ranges.clone(), false).unwrap();
    assert_eq!(file.timing.sample_count, 6);
    assert_eq!(file.timing.timescale, 30000);
    assert!(matches!(
        file.timing.clock,
        SampleClock::Irregular { media_end: 5999 }
    ));
    assert!(file.timing.partial_timeline);
    assert_eq!(file.timing.declared_media_end, Some(6000));
    assert!(file.validate_source(&source()).is_err());
    assert!(file
        .validate_source_for_use(&source(), Some(&usage))
        .unwrap());
    assert!(file.timing.source_clock().is_err());
}

#[test]
fn declared_tail_whole_source_audio_and_referenced_tail_stay_rejected() {
    let bytes = declared_tail_bytes();
    assert!(inspect(&bytes).is_err());
    assert!(inspect_selected(&bytes, interior_use().ranges, true).is_err());
    for end in [5999, 6000, 6001] {
        let end = i64::from(end) * (crate::schema::TICKS / 30000);
        assert!(inspect_selected(&bytes, std::iter::once(0..end).collect(), false).is_err());
    }
    // Every selected occurrence is checked, not only the first safe interval.
    let mut ranges = interior_use().ranges;
    ranges.push(0..source().intrinsic_ticks);
    assert!(inspect_selected(&bytes, ranges, false).is_err());
}

#[test]
fn declared_tail_negative_or_full_sample_header_excess_stays_rejected() {
    let bytes = declared_tail_bytes();
    for duration in [0, 5998, 6999, u32::MAX] {
        let bytes = patch_box(&bytes, MDHD, |table| write_u32(table, 16, duration));
        assert!(
            inspect_selected(&bytes, interior_use().ranges, false).is_err(),
            "{duration}"
        );
    }
}

#[test]
fn declared_tail_malformed_tables_edits_and_clocks_stay_rejected() {
    let bytes = declared_tail_bytes();
    for (offset, value) in [(4, 7), (8, 0), (12, 0), (8, u32::MAX), (12, u32::MAX)] {
        let bad = patch_box(&bytes, &stbl(b"stts"), |table| {
            write_u32(table, offset, value)
        });
        assert!(
            inspect_selected(&bad, interior_use().ranges, false).is_err(),
            "stts {offset}/{value}"
        );
    }
    for (path, offset, value) in [
        (MDHD, 12, 0),
        (MVHD, 12, 0),
        (ELST, 12, 1),
        (ELST, 16, 0),
        (ELST, 8, 0),
    ] {
        let bad = patch_box(&bytes, path, |table| write_u32(table, offset, value));
        assert!(
            inspect_selected(&bad, interior_use().ranges, false).is_err(),
            "clock/edit {offset}/{value}"
        );
    }
}

#[test]
fn declared_tail_decoder_clock_repair_is_rejected() {
    // FFmpeg interprets this unsigned STTS delta as a negative correction.
    // Exact declared/header totals do not make the repaired packet clock safe.
    let origin = u32::MAX - 10_000;
    let starts = [
        0,
        origin,
        origin + 1000,
        origin + 2000,
        origin + 3000,
        origin + 4000,
        origin + 5000,
    ];
    let bytes = sample_grid_bytes(&starts, 30000);
    assert!(inspect_selected(&bytes, interior_use().ranges, false)
        .unwrap_err()
        .to_string()
        .contains("sample timing table"));
}

#[test]
fn declared_tail_native_identity_rate_and_count_are_not_inferred() {
    let bytes = declared_tail_bytes();
    let usage = interior_use();
    let file = inspect_selected(&bytes, usage.ranges.clone(), false).unwrap();
    for change in 0..3 {
        let mut native = source();
        match change {
            0 => native.intrinsic_ticks += 1,
            1 => {
                native.frame_rate = FrameRate::Fps24.into();
                native.intrinsic_ticks = 6 * FrameRate::Fps24.ticks_per_frame();
            }
            _ => native.orientation = crate::schema::VideoOrientation::Clockwise,
        }
        assert!(file.validate_source_for_use(&native, Some(&usage)).is_err());
    }
    assert!(file.validate_source_for_use(&source(), None).is_err());
}

#[test]
fn declared_tail_public_import_publishes_original_bytes_and_editable_trim() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    let bytes = declared_tail_bytes();
    fs::write(root.path().join("media/source.mp4"), &bytes).unwrap();
    let duration = 4 * FrameRate::Fps30.ticks_per_frame();
    let xml = include_str!("../../../tests/fixtures/one-clip.xml")
        .replace("2540160000000", &source().intrinsic_ticks.to_string())
        .replace("1270080000000", &duration.to_string());
    let input = root.path().join("source.prproj");
    crate::test_support::write_prproj(&input, &xml);
    let options = crate::PremiereImportOptions {
        sequence: Some("sequence-1".into()),
    };
    let report = crate::Premiere
        .inspect_media(&input, &options, None)
        .unwrap();
    assert_eq!(
        report.media[0].status,
        fx_conv::MediaStatus::Supported,
        "{report:?}"
    );
    let output = root.path().join("import");
    let notes = crate::premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("declared media-header tail")),
        "{notes:?}"
    );
    let archive = tesseract_file::TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document = archive.project_json().unwrap();
    let videos: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(videos.len(), 1);
    let video = videos[0];
    let range = crate::test_support::layer_range(video);
    assert_eq!(range["start"], 0);
    assert_eq!(range["duration"], 133);
    let mapping = &video["playback"]["mapping"];
    assert_eq!(mapping["type"], "linear");
    assert_eq!(mapping["output"], video["sourceRange"]);
    assert_eq!(mapping["input"]["duration"], mapping["output"]["duration"]);
    assert_eq!(
        video["sourceRange"],
        serde_json::json!({"start":0,"duration":133})
    );
    let asset = video["source"]["assetId"].as_str().unwrap();
    assert_eq!(
        archive
            .asset(asset)
            .unwrap()
            .read_verified_bytes(bytes.len() as u64)
            .unwrap(),
        bytes
    );
}
