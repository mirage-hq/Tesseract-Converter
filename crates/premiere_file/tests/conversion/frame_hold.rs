//! The explicit frame hold extracted from a native Crop placement.

use super::support::*;
use serde_json::json;
use std::{fs, path::Path};
use tesseract_file::TesseractFile;

const NATIVE_HOLD: &str = include_str!("../fixtures/cap2-native-frame-hold.xml");
const TEN_SECONDS: &[u8] = include_bytes!("../fixtures/video-30fps-10s.mp4");

fn hold_xml(record: &str) -> String {
    let clip = record
        .replace("ObjectID=\"404\"", "ObjectID=\"6\"")
        .replace("ObjectRef=\"111\"", "ObjectRef=\"7\"");
    let start = XML.find("  <VideoClip ObjectID=\"6\"").unwrap();
    let end = start + XML[start..].find("</VideoClip>").unwrap() + "</VideoClip>".len();
    let mut xml = XML.to_owned();
    xml.replace_range(start..end, &clip);
    xml.replace(
        "<End>1270080000000</End>",
        "<Start>3602158560000</Start><End>3814050240000</End>",
    )
    .replacen(
        "<FrameRate>8467200000</FrameRate>",
        "<FrameRate>10594584000</FrameRate>",
        1,
    )
}

#[test]
fn native_explicit_frame_hold_imports_as_editable_constant_playback() {
    let dir = tempfile::tempdir().unwrap();
    let source = fixture(dir.path(), &hold_xml(NATIVE_HOLD));
    fs::write(dir.path().join("media/source.mp4"), TEN_SECONDS).unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(source, &output, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let archive = first_project(&output);
    let file = TesseractFile::open(&archive).unwrap();
    let document = file.project_json().unwrap();
    let video = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(video["sourceRange"], json!({"start":0,"duration":10000}));
    let playback = &video["playback"];
    assert_eq!(
        playback["inputRange"],
        json!({"start":14181,"duration":834})
    );
    assert_eq!(playback["inputOffsetMs"], 0);
    assert_eq!(playback["mapping"]["type"], "timeRemap");
    let keys = &playback["mapping"]["property"]["keyframes"];
    assert_eq!(keys[0]["time"], 14181);
    assert_eq!(keys[1]["time"], 15015);
    assert_eq!(keys[0]["value"], 3462);
    assert_eq!(keys[1]["value"], 3462);
    assert_eq!(keys.as_array().unwrap().len(), 2);
    // Native export has no measured FrameHold writer; it must reject this
    // imported playback rather than silently emit a moving clip.
    let error = tesseract_to_premiere(archive, dir.path().join("native"), false).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("time remapping was not exported"),
        "{error}"
    );
}

#[test]
fn native_frame_hold_rejects_unknown_incomplete_out_of_bounds_and_combined_forms() {
    for (from, to, reason) in [
        (
            "<FrameHold>4</FrameHold>",
            "<FrameHold>3</FrameHold>",
            "FrameHold",
        ),
        (
            "<FrameHoldStart>879350472000</FrameHoldStart>",
            "",
            "FrameHold",
        ),
        ("<FrameHold>4</FrameHold>", "", "FrameHold"),
        (
            "879350472000</FrameHoldStart>",
            "-1</FrameHoldStart>",
            "FrameHold",
        ),
        (
            "879350472000</FrameHoldStart>",
            "2540160000000</FrameHoldStart>",
            "TimeRemapping",
        ),
        (
            "<InPoint>",
            "<PlaybackSpeed>2</PlaybackSpeed><InPoint>",
            "TimeRemapping",
        ),
        (
            "<InPoint>",
            "<TimeRemapping ObjectRef=\"4\"/><InPoint>",
            "FrameHold",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path(), &hold_xml(&NATIVE_HOLD.replace(from, to)));
        fs::write(dir.path().join("media/source.mp4"), TEN_SECONDS).unwrap();
        let error =
            premiere_to_tesseract(source, dir.path().join("converted"), None, false).unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
    }
}

#[test]
fn native_frame_hold_is_rejected_on_master_sources_graphics_and_adjustments() {
    // Inject holds into independently pinned existing native sources to ensure
    // the newly typed fields cannot bypass unsupported clock owners.
    for (name, master, reason) in [
        (
            "feature_constant_reverse_0_905_strict.prproj",
            true,
            "FrameHold on a master source",
        ),
        ("feature_text_point.prproj", false, "graphic retiming"),
        (
            "feature_adjustment_layer_26_5_strict.prproj",
            false,
            "FrameHold on an adjustment layer",
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let xml = read_xml(&path);
        let document = roxmltree::Document::parse(&xml).unwrap();
        let id = if name == "feature_adjustment_layer_26_5_strict.prproj" {
            document
                .descendants()
                .find(|node| {
                    node.has_tag_name("VideoClip")
                        && node.children().any(|child| {
                            child.has_tag_name("AdjustmentLayer") && child.text() == Some("true")
                        })
                })
                .unwrap()
                .attribute("ObjectID")
                .unwrap()
        } else if master {
            document
                .descendants()
                .find(|node| node.has_tag_name("MasterClip"))
                .unwrap()
                .descendants()
                .find(|node| node.has_tag_name("Clip") && node.attribute("ObjectRef").is_some())
                .unwrap()
                .attribute("ObjectRef")
                .unwrap()
        } else {
            let sub = document
                .descendants()
                .find(|node| node.has_tag_name("VideoClipTrackItem"))
                .unwrap()
                .descendants()
                .find(|node| node.has_tag_name("SubClip"))
                .unwrap()
                .attribute("ObjectRef")
                .unwrap();
            document
                .root_element()
                .children()
                .find(|node| {
                    node.has_tag_name("SubClip") && node.attribute("ObjectID") == Some(sub)
                })
                .unwrap()
                .children()
                .find(|node| node.has_tag_name("Clip"))
                .unwrap()
                .attribute("ObjectRef")
                .unwrap()
        };
        let record = document
            .root_element()
            .children()
            .find(|node| node.has_tag_name("VideoClip") && node.attribute("ObjectID") == Some(id))
            .unwrap();
        let position = record.range().end - "</VideoClip>".len();
        for fields in [
            "<FrameHold>4</FrameHold><FrameHoldStart>0</FrameHoldStart>",
            "<FrameHoldStart>0</FrameHoldStart>",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut edited = xml.clone();
            edited.insert_str(position, fields);
            let input = dir.path().join("held.prproj");
            write_prproj(&input, &edited);
            match premiere_file::PrProjectFile::load(&input) {
                Ok((_, omissions)) => assert!(
                    omissions
                        .iter()
                        .any(|omission| omission.to_string().contains(reason)),
                    "{omissions:?}"
                ),
                Err(error) => assert!(error.to_string().contains(reason), "{error}"),
            }
        }
    }
}
