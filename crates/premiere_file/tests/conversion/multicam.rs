#![cfg(feature = "ffmpeg-library")]

use super::support::{
    edit_record, first_project, premiere_to_tesseract, read_xml, tesseract_to_premiere,
    write_prproj,
};
use serde_json::{json, Value};
use std::path::Path;
use tesseract_file::TesseractFile;

fn clips(file: &TesseractFile) -> Vec<Value> {
    let document = file.project_json().unwrap();
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| matches!(layer["type"].as_str(), Some("Video" | "Audio")))
        .map(|layer| {
            let asset = &file.metadata().assets[layer["source"]["assetId"].as_str().unwrap()];
            json!({
                "type": layer["type"],
                "file": Path::new(&asset.path).file_name().unwrap().to_str().unwrap(),
                "range": crate::test_support::layer_range(layer),
                "source": layer["sourceRange"],
                "volume": layer["volume"],
            })
        })
        .collect()
}

#[test]
fn adobe_multicam_cuts_export_as_ordinary_clips_with_independent_sound() {
    let temp = tempfile::tempdir().unwrap();
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_multicam_cuts.prproj");
    let output = temp.path().join("import");
    let omissions = premiere_to_tesseract(
        input,
        &output,
        Some("9266cdc4-92a3-447f-8383-ed600a3e7b8c"),
        false,
    )
    .unwrap();
    assert!(
        omissions.iter().any(|item| item.record == "355"
            && item
                .reason
                .contains("only ordinary mono/stereo channel layouts")),
        "{omissions:?}"
    );
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    let expected = vec![
        json!({"type":"Video", "file":"feature_two_tracks_gap_clip_a.mp4",
            "range":{"start":0,"duration":3500}, "source":{"start":0,"duration":3500}, "volume":0.0}),
        json!({"type":"Video", "file":"feature_two_tracks_gap_clip_b.mp4",
            "range":{"start":3500,"duration":3733}, "source":{"start":3500,"duration":3733}, "volume":0.0}),
        json!({"type":"Video", "file":"feature_two_tracks_gap_clip_a.mp4",
            "range":{"start":7233,"duration":2767}, "source":{"start":7233,"duration":2767}, "volume":0.0}),
        json!({"type":"Audio", "file":"nest_tone_stereo_8s.wav",
            "range":{"start":0,"duration":7000}, "source":{"start":1000,"duration":7000}, "volume":1.0}),
    ];
    assert_eq!(clips(&file), expected, "{omissions:?}");
    let native = temp.path().join("native");
    tesseract_to_premiere(&archive, &native, false).unwrap();
    let exported_xml = read_xml(&native.join("project.prproj"));
    assert!(!exported_xml.contains("<IsMulticam>"));
    assert!(!exported_xml.contains("<SelectedTrackIndex>"));
    let again = temp.path().join("again");
    premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    assert_eq!(
        clips(&TesseractFile::open(first_project(&again)).unwrap()),
        expected
    );
}

#[test]
fn missing_multicam_camera_omits_only_its_cut_without_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = read_xml(&fixtures.join("feature_multicam_cuts.prproj"));
    edit_record(
        &mut xml,
        "<VideoClip ObjectID=\"628\"",
        "</VideoClip>",
        |record| {
            record.replace(
                "<SelectedTrackIndex>0</SelectedTrackIndex>",
                "<SelectedTrackIndex>9</SelectedTrackIndex>",
            )
        },
    );
    let input = temp.path().join("missing-camera.prproj");
    write_prproj(&input, &xml);
    for name in [
        "feature_two_tracks_gap_clip_a.mp4",
        "feature_two_tracks_gap_clip_b.mp4",
        "nest_tone_stereo_8s.wav",
    ] {
        std::fs::copy(fixtures.join(name), temp.path().join(name)).unwrap();
    }
    let output = temp.path().join("import");
    let omissions = premiere_to_tesseract(
        input,
        &output,
        Some("9266cdc4-92a3-447f-8383-ed600a3e7b8c"),
        false,
    )
    .unwrap();
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "354" && item.reason.contains("SelectedTrackIndex 9")),
        "{omissions:?}"
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let kept = clips(&file);
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[0]["file"], "feature_two_tracks_gap_clip_b.mp4");
    assert_eq!(kept[0]["range"]["start"], 3500);
    assert_eq!(kept[1]["file"], "feature_two_tracks_gap_clip_a.mp4");
    assert_eq!(kept[1]["range"]["start"], 7233);
    assert_eq!(kept[2]["type"], "Audio");
}

#[test]
fn multicam_source_offsets_and_native_track_index_compose_without_extending_camera_coverage() {
    // Supplementary edits distinguish the two source clocks from timeline Start.
    // The native fixture above remains unchanged and is the camera-cut witness.
    let temp = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = read_xml(&fixtures.join("feature_multicam_cuts.prproj"));
    edit_record(
        &mut xml,
        "<VideoClip ObjectID=\"628\"",
        "</VideoClip>",
        |record| {
            record
                .replace("<InPoint>0</InPoint>", "<InPoint>254016000000</InPoint>")
                .replace(
                    "<OutPoint>889056000000</OutPoint>",
                    "<OutPoint>1143072000000</OutPoint>",
                )
        },
    );
    edit_record(
        &mut xml,
        "<VideoClipTrackItem ObjectID=\"456\"",
        "</VideoClipTrackItem>",
        |record| {
            record.replace(
                "<End>2540160000000</End>",
                "<Start>127008000000</Start><End>2413152000000</End>",
            )
        },
    );
    edit_record(
        &mut xml,
        "<VideoClip ObjectID=\"977\"",
        "</VideoClip>",
        |record| record.replace("<InPoint>0</InPoint>", "<InPoint>254016000000</InPoint>"),
    );
    edit_record(
        &mut xml,
        "<VideoTrackGroup ObjectID=\"295\"",
        "</VideoTrackGroup>",
        |record| record.replace("<Track Index=\"0\"", "<Track Index=\"4\""),
    );
    edit_record(
        &mut xml,
        "<VideoClipTrack ObjectUID=\"20a4e492-fb79-45fc-9858-a7bb11788b2c\"",
        "</VideoClipTrack>",
        |record| record.replace("<Index>0</Index>", "<Index>4</Index>"),
    );
    xml = xml.replace(
        "<SelectedTrackIndex>0</SelectedTrackIndex>",
        "<SelectedTrackIndex>4</SelectedTrackIndex>",
    );
    edit_record(
        &mut xml,
        "<AudioClip ObjectID=\"629\"",
        "</AudioClip>",
        |record| {
            record.replace("<InPoint>0</InPoint>", "<IsMulticam>true</IsMulticam><SelectedTrackIndex>0</SelectedTrackIndex><InPoint>0</InPoint>")
        },
    );
    let input = temp.path().join("offset-cameras.prproj");
    write_prproj(&input, &xml);
    for name in [
        "feature_two_tracks_gap_clip_a.mp4",
        "feature_two_tracks_gap_clip_b.mp4",
        "nest_tone_stereo_8s.wav",
    ] {
        std::fs::copy(fixtures.join(name), temp.path().join(name)).unwrap();
    }
    let output = temp.path().join("import");
    let omissions = premiere_to_tesseract(
        input,
        &output,
        Some("9266cdc4-92a3-447f-8383-ed600a3e7b8c"),
        false,
    )
    .unwrap();
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "468" && item.reason.contains("does not cover")),
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "355"
                && item.reason.contains("multicam audio channel selection")),
        "{omissions:?}"
    );
    let kept = clips(&TesseractFile::open(first_project(&output)).unwrap());
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[0]["file"], "feature_two_tracks_gap_clip_a.mp4");
    assert_eq!(kept[0]["range"], json!({"start":0,"duration":3500}));
    assert_eq!(kept[0]["source"], json!({"start":1500,"duration":3500}));
    assert_eq!(kept[1]["file"], "feature_two_tracks_gap_clip_b.mp4");
    assert_eq!(kept[1]["source"], json!({"start":3500,"duration":3733}));
    assert_eq!(kept[2]["type"], "Audio");
}
