#![cfg(feature = "ffmpeg-library")]

use super::support::*;
use crate::test_support::{one_clip_xml, OneClip};
use premiere_file::PrProjectFile;
use serde_json::{json, Value};
use std::{fs, path::Path};
use tesseract_file::TesseractFile;

const FRAME_30: i64 = TICKS / 30;
const MILLISECOND: i64 = TICKS / 1000;

#[test]
fn native_constant_rate_residue_keeps_exact_ticks_and_editable_playback() {
    // Timing copied unchanged from Co-Editor's Adobe-native occurrence 1159,
    // SubClip 1349 / VideoClip 1643. Source SHA-256:
    // 332c86eb7927e19cc4177aa94fe29f4c62a2d3978b8c4de4c894f8f8ac69bae8.
    // Only the timing is native-derived: unrelated metadata/paths/Motion are
    // removed and the existing 30 fps / 10 s fixture media replaces the footage.
    // Its 6.066644689510400-tick residue is already in the saved fields, not
    // caused by f64 parsing. This is public structural proof, not a render oracle.
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(
        temp.path(),
        include_str!("../fixtures/native-constant-rate-timing.xml"),
    );
    fs::write(
        temp.path().join("media/source.mp4"),
        include_bytes!("../fixtures/video-30fps-10s.mp4"),
    )
    .unwrap();
    let (project, omissions) = PrProjectFile::load(&source).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.sequences().next().unwrap();
    let clips: Vec<_> = sequence.video_occurrences().collect();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id(), Some("VideoClipTrackItem:1159"));
    assert_eq!(
        clips[0].timeline_ticks(),
        5_526_135_014_400..5_661_745_689_600
    );
    assert_eq!(clips[0].source_ticks(), 600_830_630_396..999_052_454_391);

    let output = temp.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert_eq!(file.metadata().assets.len(), 1);
    let document = file.project_json().unwrap();
    let layers = video_layers(&document);
    assert_eq!(layers.len(), 1);
    let video = layers[0];
    // The established output clock rounds each authored endpoint once to ms.
    assert_eq!(
        (*crate::test_support::layer_range(video)),
        json!({"start": 21755, "duration": 534})
    );
    assert_eq!(
        video["sourceRange"],
        json!({"start": 2365, "duration": 1568})
    );
    let mapping = &video["playback"]["mapping"];
    assert_eq!(mapping["type"], "timeRemap");
    let keys = mapping["property"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    for (key, time, value) in [(&keys[0], 21755, 2365), (&keys[1], 22289, 3933)] {
        assert_eq!(key["time"], time);
        assert_eq!(key["value"], value);
        assert_eq!(key["easing"]["type"], "linear");
    }
}

#[test]
fn mixed_rate_import_and_export_preserves_media_timing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    // Three sequence frames at 25 fps; six source frames at 24000/1001 fps.
    let (frame_25, frame_23_976) = (TICKS / 25, TICKS * 1001 / 24_000);
    let source = fixture(
        root,
        &one_clip_xml(OneClip {
            sequence_frame: frame_25,
            media_frame: frame_23_976,
            media_duration: 6 * frame_23_976,
            end: 3 * frame_25,
            out_point: 3 * frame_25,
            ..OneClip::default()
        }),
    );
    let media = include_bytes!("../fixtures/video-23.976fps.mp4");
    fs::write(root.join("media/source.mp4"), media).unwrap();
    let imported = root.join("imported");
    let omissions = premiere_to_tesseract(source, &imported, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let path = first_project(&imported);
    let doc = TesseractFile::open(&path).unwrap().project_json().unwrap();
    assert_eq!(
        (*crate::test_support::layer_range(&doc["composition"]["layers"][0])),
        json!({"start":0,"duration":120})
    );
    assert_eq!(
        doc["composition"]["layers"][0]["sourceRange"],
        json!({"start":0,"duration":120})
    );
    assert_eq!(
        doc["composition"]["layers"][0]["sourceIntrinsicDuration"],
        250
    );

    let native = root.join("native");
    tesseract_to_premiere(&path, &native, false).unwrap();
    assert_eq!(fs::read(native.join("media/source.mp4")).unwrap(), media);
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project
        .sequences()
        .next()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    // 120 ms is 3.6 frames at 30 fps, so the end snaps to frame 4; source-in stays 0.
    assert_eq!(clip.timeline_ticks(), 0..4 * FRAME_30);
    assert_eq!(clip.source_ticks(), 0..4 * FRAME_30);

    // Inspect native media facts separately from the 30 fps export sequence.
    let xml = read_xml(&native.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let stream = parsed
        .descendants()
        .find(|node| node.has_tag_name("VideoStream") && node.has_attribute("ObjectID"))
        .unwrap();
    for (field, expected) in [("FrameRate", frame_23_976), ("Duration", 6 * frame_23_976)] {
        assert_eq!(
            stream
                .children()
                .find(|node| node.has_tag_name(field))
                .unwrap()
                .text(),
            Some(expected.to_string().as_str())
        );
    }
    let group = parsed
        .descendants()
        .find(|node| node.has_tag_name("VideoTrackGroup"))
        .unwrap();
    assert_eq!(
        group
            .descendants()
            .find(|node| node.has_tag_name("FrameRate"))
            .unwrap()
            .text(),
        Some(FRAME_30.to_string().as_str())
    );
}

#[test]
fn rounding_only_source_tail_is_accepted_by_export_and_reader() {
    // 31 frames at 30 fps, placed at frame 1 and ending at the media end, round
    // to a source range 1 ms past sourceIntrinsicDuration. Both operations and
    // the native reader must accept it; an old per-clip media check did not.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let frame = FRAME_30;
    let source = fixture(
        root,
        &one_clip_xml(OneClip {
            start: frame,
            end: 32 * frame,
            in_point: 10 * TICKS - 31 * frame,
            out_point: 10 * TICKS,
            ..OneClip::default()
        }),
    );
    fs::write(
        root.join("media/source.mp4"),
        include_bytes!("../fixtures/video-30fps-10s.mp4"),
    )
    .unwrap();
    let output = root.join("tesseract");
    let omissions = premiere_to_tesseract(source, &output, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let doc = file.project_json().unwrap();
    assert_eq!(
        doc["composition"]["layers"][0]["sourceRange"],
        json!({"start":8967,"duration":1034})
    );
    assert_eq!(
        doc["composition"]["layers"][0]["sourceIntrinsicDuration"],
        10000
    );
    let native = root.join("exported");
    tesseract_to_premiere(first_project(&output), &native, false).unwrap();
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.sequences().next().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.timeline_ticks(), frame..32 * frame);
    assert_eq!(
        clip.source_ticks(),
        8967 * MILLISECOND..8967 * MILLISECOND + 31 * frame
    );
    // The exclusive source end has a 1/3 ms tail, but its final sample is inside
    // the unchanged media. Loading the generated output with this crate's reader
    // must admit the same rule.
    assert!(clip.source_ticks().end > 10 * TICKS);
    assert!(clip.source_ticks().end - frame < 10 * TICKS);
}

/// Sequence UID of the Adobe-derived rate cases in `tests/manifest.json`.
const ISOLATED_SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";

/// Converts one pinned case directly from `tests/fixtures`, where its media lives.
fn convert_pinned_case(root: &Path, fixture: &str) -> TesseractFile {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let output = root.join("tesseract");
    let omissions =
        premiere_to_tesseract(&source, &output, Some(ISOLATED_SEQUENCE), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    TesseractFile::open(first_project(&output)).unwrap()
}

fn layer_for_media<'a>(file: &TesseractFile, document: &'a Value, file_name: &str) -> &'a Value {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer["source"]["assetId"].as_str().is_some_and(|asset_id| {
                Path::new(&file.metadata().assets[asset_id].path).file_name()
                    == Some(file_name.as_ref())
            })
        })
        .unwrap()
}

#[test]
fn adobe_derived_24_fps_cut_rounds_the_shared_boundary_once() {
    // Frame 37 of the 24 fps sequence is 1541.667 ms. Both clips must use the one
    // rounded 1542 ms cut, and the blue source still starts at its first frame.
    let temp = tempfile::tempdir().unwrap();
    let file = convert_pinned_case(temp.path(), "feature_rate_24_cut_strict.prproj");
    let doc = file.project_json().unwrap();
    assert_eq!(doc["duration"], 3.0);
    for (media, active, source) in [
        ("feature_rate_24_red.mp4", (0, 1542), (0, 1542)),
        ("feature_rate_24_blue.mp4", (1542, 1458), (0, 1458)),
    ] {
        let layer = layer_for_media(&file, &doc, media);
        assert_eq!(
            (*crate::test_support::layer_range(layer)),
            json!({"start": active.0, "duration": active.1})
        );
        assert_eq!(
            layer["sourceRange"],
            json!({"start": source.0, "duration": source.1})
        );
        assert_eq!(layer["sourceIntrinsicDuration"], 5000);
    }
}

#[test]
fn adobe_derived_25_fps_media_keeps_its_rate_in_a_24_fps_sequence() {
    // 48 frames at 24 fps are 2000 ms. Source-in is 13 frames at 25 fps, exactly
    // 520 ms, so the clip plays at normal speed from that media frame.
    let temp = tempfile::tempdir().unwrap();
    let file = convert_pinned_case(temp.path(), "feature_mixed_rate_25_in_24_strict.prproj");
    let doc = file.project_json().unwrap();
    let layer = layer_for_media(&file, &doc, "feature_rate_25_green.mp4");
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        layer["sourceRange"],
        json!({"start": 520, "duration": 2000})
    );
    assert_eq!(layer["sourceIntrinsicDuration"], 5000);

    // Export writes a 30 fps sequence and keeps the 25 fps media record.
    let native = temp.path().join("native");
    tesseract_to_premiere(
        first_project(&temp.path().join("tesseract")),
        &native,
        false,
    )
    .unwrap();
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project
        .sequences()
        .next()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.timeline_ticks(), 0..60 * FRAME_30);
    assert_eq!(
        clip.source_ticks(),
        520 * MILLISECOND..520 * MILLISECOND + 60 * FRAME_30
    );
    let xml = read_xml(&native.join("project.prproj"));
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let stream = parsed
        .descendants()
        .find(|node| node.has_tag_name("VideoStream") && node.has_attribute("ObjectID"))
        .unwrap();
    assert_eq!(
        stream
            .children()
            .find(|node| node.has_tag_name("FrameRate"))
            .unwrap()
            .text(),
        Some((TICKS / 25).to_string().as_str())
    );
}

/// Converts the root sequence "Two tracks overlap" of case
/// `premiere_nested_second_sequence_colors`: its V1 item 85 nests "Two tracks
/// gap" for 0-5 s and is omitted; V2 item 89 is `clip-a.mp4` for 2-4 s.
fn convert_nested_root(root: &Path) -> (std::path::PathBuf, Vec<premiere_file::Omission>) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(root.join("media")).unwrap();
    fs::copy(
        fixtures.join("feature_nested_second_sequence_strict.prproj"),
        root.join("project.prproj"),
    )
    .unwrap();
    for (name, source) in [
        ("clip-a.mp4", "feature_multi_sequence_red_10s.mp4"),
        ("clip-b.mp4", "feature_multi_sequence_blue_10s.mp4"),
    ] {
        fs::copy(fixtures.join(source), root.join("media").join(name)).unwrap();
    }
    let output = root.join("tesseract");
    let omissions = premiere_to_tesseract(
        root.join("project.prproj"),
        &output,
        Some("1592feef-89df-4c40-ba9d-fe9088c8f4a5"),
        false,
    )
    .unwrap();
    (first_project(&output), omissions)
}

fn layer_ranges(document: &Value) -> Vec<(&str, &Value)> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| {
            (
                layer["type"].as_str().unwrap(),
                crate::test_support::layer_range(layer),
            )
        })
        .collect()
}

#[test]
fn omitted_last_item_sets_the_imported_duration() {
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_nested_root(temp.path());
    assert!(
        omissions.iter().any(|item| item.record == "85"),
        "the nested item is reported: {omissions:?}"
    );
    let document = TesseractFile::open(archive)
        .unwrap()
        .project_json()
        .unwrap();
    // The 5 s end is the omitted item's end, not the 4 s converted clip end or
    // the 60 s work area.
    assert_eq!(document["duration"], json!(5.0));
    assert_eq!(
        layer_ranges(&document),
        [
            ("Video", &json!({"start": 2000, "duration": 2000})),
            ("Rect", &json!({"start": 0, "duration": 5000})),
        ]
    );
}

#[test]
fn shortened_clip_keeps_the_document_end_through_save_reopen_and_reports_the_tail_on_export() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (archive, _) = convert_nested_root(root);

    // Shorten the clip to end at 3 s through the normal edit cycle.
    let mut file = TesseractFile::open(&archive).unwrap();
    let checkout = root.join("project.json");
    file.checkout_project_json(&checkout).unwrap();
    let mut document: Value = serde_json::from_slice(&fs::read(&checkout).unwrap()).unwrap();
    document["composition"]["layers"][0]["playback"]["inputRange"]["duration"] = json!(1000);
    document["composition"]["layers"][0]["sourceRange"]["duration"] = json!(1000);
    fs::write(&checkout, serde_json::to_vec(&document).unwrap()).unwrap();
    file.commit_project_json(&checkout).unwrap();
    file.save().unwrap();
    let reopened = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(reopened["duration"], json!(5.0));
    assert_eq!(
        layer_ranges(&reopened),
        [
            ("Video", &json!({"start": 2000, "duration": 1000})),
            ("Rect", &json!({"start": 0, "duration": 5000})),
        ]
    );

    // Export writes the 5 s end as the work area, which Premiere does not render
    // and the reader does not reimport, so the tail past 3 s is reported.
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .filter(|item| item.record == "document.duration")
        .map(|item| item.reason.clone())
        .collect();
    assert_eq!(
        reasons,
        [format!(
            "duration differs from the last occurrence; exported duration is {} ticks",
            3 * TICKS
        )]
    );
    let xml = read_xml(&native.join("project.prproj"));
    let work_out = roxmltree::Document::parse(&xml)
        .unwrap()
        .descendants()
        .filter(|node| node.has_tag_name("MZ.WorkOutPoint"))
        .map(|node| node.text().unwrap().parse::<i64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(work_out, [5 * TICKS]);
    let reimported = root.join("reimported");
    premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(document["duration"], json!(3.0));
}
