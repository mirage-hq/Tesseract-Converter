#![cfg(feature = "ffmpeg-library")]

//! Premiere caption tracks convert to timed editable text layers and export as
//! Type-tool graphics.

use super::support::*;
use premiere_file::{Omission, OmissionKind, OmissionScope, PrProjectFile, PrVideoItem};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tesseract_file::TesseractFile;

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";

fn captions_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_captions_strict.prproj")
}

fn text_layers(document: &Value) -> Vec<&Value> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Text")
        .collect()
}

/// The names of the caption layers active at `millis`, top first.
fn active_at(document: &Value, millis: i64) -> Vec<&str> {
    text_layers(document)
        .into_iter()
        .filter(|layer| {
            let start = (*crate::test_support::layer_range(layer))["start"]
                .as_i64()
                .unwrap();
            start <= millis
                && millis
                    < start
                        + (*crate::test_support::layer_range(layer))["duration"]
                            .as_i64()
                            .unwrap()
        })
        .map(|layer| layer["name"].as_str().unwrap())
        .collect()
}

#[test]
fn pinned_captions_stay_timed_editable_text_through_edit_and_graphic_export() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions =
        premiere_to_tesseract(captions_fixture(), &output, Some(SEQUENCE), false).unwrap();
    assert_eq!(omissions, [Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Omitted,
        record: "CaptionDataClipTrackItem:200 (\"C1 caption 1\") and 2 more text layers".into(),
        reason: "font \"Arial-BoldMT\" is not packaged in this document; import it with tsrct project import-font before preview or export.".into(),
    }]);
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert_eq!(file.metadata().assets.len(), 1);
    let mut document = file.project_json().unwrap();
    let types: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect();
    // Captions paint above the video; the black canvas stays bottommost.
    assert_eq!(types, ["Text", "Text", "Text", "Video", "Rect"]);
    let cues: Vec<_> = text_layers(&document)
        .into_iter()
        .map(|layer| {
            (
                layer["name"].as_str().unwrap(),
                (*crate::test_support::layer_range(layer)).clone(),
                layer["sourceText"]["text"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        cues,
        [
            (
                "C2 caption 1",
                json!({"start": 2500, "duration": 2000}),
                "Second cue overlaps"
            ),
            (
                "C1 caption 1",
                json!({"start": 1000, "duration": 2000}),
                "First cue"
            ),
            (
                "C1 caption 2",
                json!({"start": 6000, "duration": 2000}),
                "Third after gap\nwith a second line"
            ),
        ]
    );
    for layer in text_layers(&document) {
        // Bottom-centre box text pivoted on its last baseline, which Premiere
        // draws at 95% of the frame height.
        assert_eq!(layer["transform"]["position"], json!([960.0, 1026.0]));
        assert_eq!(layer["transform"]["anchorPoint"], json!([768.0, 969.6]));
        assert_eq!(layer["transform"]["scale"], json!([100.0, 100.0]));
        let text = &layer["sourceText"];
        assert_eq!(text["boxText"], true);
        assert_eq!(text["boxSize"], json!([1536.0, 998.0]));
        assert_eq!(text["boxPosition"], json!([0.0, 0.0]));
        assert_eq!(text["verticalAlign"], "bottom");
        assert_eq!(text["justification"], "center");
        assert_eq!(text["fontFamily"], "Arial-BoldMT");
        assert_eq!(text["fontStyle"], "");
        assert_eq!(text["fontSize"], 48.0);
        assert_eq!(text["fillColor"], json!([1.0, 1.0, 1.0, 1.0]));
    }
    // Overlapping cues are two layers; gaps have no caption.
    for (millis, active) in [
        (500, &[][..]),
        (1500, &["C1 caption 1"][..]),
        (2750, &["C2 caption 1", "C1 caption 1"][..]),
        (4000, &["C2 caption 1"][..]),
        (5000, &[][..]),
        (7000, &["C1 caption 2"][..]),
        (8500, &[][..]),
    ] {
        assert_eq!(active_at(&document, millis), active, "{millis} ms");
    }

    // Reword, retime and move one cue; export must use the current values.
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let third = layers
        .iter_mut()
        .find(|layer| layer["name"] == "C1 caption 2")
        .unwrap();
    third["sourceText"]["text"] = json!("Edited third\ncue");
    third["activeRange"] = json!({"start": 6500, "duration": 1000});
    third["transform"]["position"] = json!([960.0, 900.0]);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps-10s.mp4");
    let edited = archive(root, &document, &source);
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&edited, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let xml = read_xml(&native.join("project.prproj"));
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Text</MatchName>").count(),
        3
    );
    assert!(!xml.contains("CaptionDataClipTrack"));
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Tracks bottom to top: the video, then C1's cues, then C2's cue above the
    // C1 cue it overlaps.
    let lanes: Vec<Vec<_>> = project
        .sequences()
        .next()
        .unwrap()
        .video_tracks()
        .map(|track| {
            track
                .iter()
                .filter_map(|item| match item {
                    PrVideoItem::Graphic(graphic) => {
                        let ticks = graphic.timeline_ticks();
                        Some((ticks.start, ticks.end))
                    }
                    PrVideoItem::Media(_) => None,
                })
                .collect()
        })
        .collect();
    assert_eq!(
        lanes,
        [
            vec![],
            vec![(TICKS, 3 * TICKS), (13 * TICKS / 2, 15 * TICKS / 2)],
            vec![(5 * TICKS / 2, 9 * TICKS / 2)],
        ]
    );
    // The exported graphics reimport with the edited placement and box layout.
    let again = root.join("again");
    premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    let restored = TesseractFile::open(first_project(&again))
        .unwrap()
        .project_json()
        .unwrap();
    // The second import keeps the paint order, top first.
    let names: Vec<_> = text_layers(&restored)
        .into_iter()
        .map(|layer| layer["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["C2 caption 1", "C1 caption 1", "C1 caption 2"]);
    let words: Vec<_> = text_layers(&restored)
        .into_iter()
        .map(|layer| layer["sourceText"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        words,
        ["Second cue overlaps", "First cue", "Edited third\ncue"]
    );
    let mut placements: Vec<_> = text_layers(&restored)
        .into_iter()
        .map(|layer| {
            (
                (*crate::test_support::layer_range(layer))["start"]
                    .as_i64()
                    .unwrap(),
                layer["transform"]["position"].clone(),
                layer["sourceText"]["boxSize"].clone(),
                layer["sourceText"]["verticalAlign"].clone(),
            )
        })
        .collect();
    placements.sort_by_key(|(start, ..)| *start);
    let bottom_centre = |start, position| {
        (
            start,
            json!(position),
            json!([1536.0, 998.0]),
            json!("bottom"),
        )
    };
    assert_eq!(
        placements,
        [
            bottom_centre(1000, [960.0, 1026.0]),
            bottom_centre(2500, [960.0, 1026.0]),
            bottom_centre(6500, [960.0, 900.0]),
        ]
    );
}

const STYLES_SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";

fn caption_styles_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_caption_styles_26_5_strict.prproj")
}

/// One cue as `(layer name, start ms, duration ms, text)`, top first: a
/// root text layer, or the text layer of a graphic group on the group's range.
fn cues(document: &Value) -> Vec<(String, i64, i64, String)> {
    let range = |layer: &Value| {
        (
            (*crate::test_support::layer_range(layer))["start"]
                .as_i64()
                .unwrap(),
            (*crate::test_support::layer_range(layer))["duration"]
                .as_i64()
                .unwrap(),
        )
    };
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|layer| {
            let (start, duration) = range(layer);
            let text = match layer["type"].as_str().unwrap() {
                "Text" => layer,
                "Group" => {
                    let [text] = layer["layers"].as_array().unwrap().as_slice() else {
                        return None;
                    };
                    assert_eq!(text["type"], "Text");
                    assert_eq!(range(text), (0, duration), "{}", text["name"]);
                    text
                }
                _ => return None,
            };
            Some((
                text["name"].as_str().unwrap().to_owned(),
                start,
                duration,
                text["sourceText"]["text"].as_str().unwrap().to_owned(),
            ))
        })
        .collect()
}

/// The layer named `name`, a root text layer or one inside a graphic group.
fn cue_layer<'a>(document: &'a Value, name: &str) -> &'a Value {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|layer| {
            std::iter::once(layer).chain(layer["layers"].as_array().into_iter().flatten())
        })
        .find(|layer| layer["name"] == name)
        .unwrap_or_else(|| panic!("no cue {name}"))
}

#[test]
fn pinned_caption_styles_convert_from_each_cues_own_payload_and_no_cue_is_lost() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        caption_styles_fixture(),
        &output,
        Some(STYLES_SEQUENCE),
        false,
    )
    .unwrap();
    assert_eq!(omissions, [Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Omitted,
        record: "CaptionDataClipTrackItem:75 (\"C1 caption 1\") and 6 more text layers".into(),
        reason: "font \"Arial-BoldMT\" is not packaged in this document; import it with tsrct project import-font before preview or export.".into(),
    }]);
    let mut document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    // Seven cues on three tracks, 30 fps frames 15-74, 45-104, 120-149,
    // 150-194, 195-227, 228-263 and 264-299; the higher track Index is the
    // higher lane, so C7 (C2 caption 1) paints over C1 where they overlap.
    let cue = |name: &str, start, duration, text: &str| {
        (name.to_owned(), start, duration, text.to_owned())
    };
    let imported = cues(&document);
    assert_eq!(
        imported,
        [
            cue("C3 caption 1", 6500, 1100, "C4 BACKGROUND"),
            cue("C2 caption 1", 1500, 2000, "C7 OVERLAP"),
            cue("C2 caption 2", 5000, 1500, "C3 STROKE AND SHADOW"),
            cue("C1 caption 1", 500, 2000, "C1 PLAIN"),
            cue("C1 caption 2", 4000, 1000, "C2 GREEN FILL"),
            cue("C1 caption 3", 7600, 1200, "C5 FIRST LINE\nSECOND LINE"),
            cue("C1 caption 4", 8800, 1200, "C6 LEFT"),
        ]
    );
    // Each cue's own style: the per-cue green fill against its white
    // template, the stroke and shadow template, the left cue's 75% box.
    let green = cue_layer(&document, "C1 caption 2");
    assert_eq!(
        green["sourceText"]["fillColor"],
        json!([0.0, 1.0, 0.0, 1.0])
    );
    for name in ["C2 caption 1", "C2 caption 2"] {
        let layer = cue_layer(&document, name);
        assert_eq!(
            layer["sourceText"]["strokeColor"],
            json!([0.0, 0.0, 1.0, 1.0])
        );
        assert_eq!(layer["effects"][0]["effect"]["type"], "dropShadow");
    }
    let left = cue_layer(&document, "C1 caption 4");
    assert_eq!(left["sourceText"]["justification"], "left");
    assert_eq!(left["sourceText"]["boxSize"], json!([1440.0, 998.0]));
    assert_eq!(left["transform"]["anchorPoint"], json!([720.0, 969.6]));
    // The background cue is a group whose box has the background's size on
    // every side, its color as one fill and its radius on every corner.
    let boxed = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    for key in ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"] {
        assert_eq!(boxed[key], 10.0, "{key}");
    }
    for key in [
        "cornerRadiusTopLeft",
        "cornerRadiusTopRight",
        "cornerRadiusBottomRight",
        "cornerRadiusBottomLeft",
    ] {
        assert_eq!(boxed[key], 15.0, "{key}");
    }
    assert_eq!(
        boxed["fills"][0]["paint"]["color"],
        json!([0.0, 0.0, 1.0, 1.0])
    );
    assert_eq!(boxed["fills"].as_array().unwrap().len(), 1);

    // Reword one cue and change one fill, then export: every cue becomes an
    // editable Type-tool graphic with the current values.
    let edited_cue = cue_layer(&document, "C1 caption 2").clone();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let edited = layers
        .iter_mut()
        .find(|layer| layer["name"] == edited_cue["name"])
        .unwrap();
    edited["sourceText"]["text"] = json!("C2 EDITED FILL");
    edited["sourceText"]["fillColor"] = json!([1.0, 0.0, 0.0, 1.0]);
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps-10s.mp4");
    let archive = archive(root, &document, &source);
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let xml = read_xml(&native.join("project.prproj"));
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Text</MatchName>").count(),
        7
    );
    assert!(!xml.contains("CaptionDataClipTrack"));

    // Lost-cue check: the reimport has every cue with its name, timing and
    // wording, the edited fill, and C7 still above C1. The exported box is
    // written in the background slots and reported on this Type-tool
    // reimport, which converts no Type-tool background.
    let again = root.join("again");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    assert_eq!(
        omissions.iter().map(|omission| omission.reason.as_str()).collect::<Vec<_>>(),
        [
            "text background not converted",
            "font \"Arial-BoldMT\" is not packaged in this document; import it with tsrct project import-font before preview or export.",
        ]
    );
    let restored = TesseractFile::open(first_project(&again))
        .unwrap()
        .project_json()
        .unwrap();
    let mut expected = imported.clone();
    expected
        .iter_mut()
        .find(|(name, ..)| name == "C1 caption 2")
        .unwrap()
        .3 = "C2 EDITED FILL".to_owned();
    let mut reimported = cues(&restored);
    assert_eq!(reimported.len(), 7);
    assert_eq!(reimported[0].0, "C2 caption 1");
    expected.sort();
    reimported.sort();
    assert_eq!(reimported, expected);
    assert_eq!(
        cue_layer(&restored, "C1 caption 2")["sourceText"]["fillColor"],
        json!([1.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(
        cue_layer(&restored, "C1 caption 4")["sourceText"]["boxSize"],
        json!([1440.0, 998.0])
    );
}
