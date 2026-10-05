#![cfg(feature = "ffmpeg-library")]

//! Text shadow and stroke on the Adobe-scaffold fixture: editable import, an
//! edit in the Tesseract document, and export read back by the crate reader.

use super::support::*;
use premiere_file::PrProjectFile;
use serde_json::{json, Value};
use std::path::Path;
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

const SEQUENCE: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn assert_near(actual: &Value, expected: &[f64]) {
    let actual: Vec<f64> = actual
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_f64().unwrap())
        .collect();
    assert!(
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1e-6),
        "{actual:?} != {expected:?}"
    );
}

/// The payload of a text layer's one effect, which must be a drop shadow.
fn drop_shadow(layer: &Value) -> &Value {
    let [effect] = layer["effects"].as_array().unwrap().as_slice() else {
        panic!("expected one effect: {layer}");
    };
    assert_eq!(effect["enabled"], true);
    assert_eq!(effect["effect"]["type"], "dropShadow");
    assert_eq!(effect["effect"]["blendMode"], "normal");
    &effect["effect"]
}

fn text_layers(document: &Value) -> Vec<&Value> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Text")
        .collect()
}

#[test]
fn adobe_scaffold_text_shadow_and_stroke_stay_editable_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        fixture("feature_text_shadow_stroke_strict.prproj"),
        &output,
        Some(SEQUENCE),
        false,
    )
    .unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].reason, "font \"Arial-BoldMT\" is not packaged in this document; import it with tsrct project import-font before preview or export.");
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let mut document = file.project_json().unwrap();
    let types: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["Text", "Text", "Text", "Video", "Rect"]);
    let texts = text_layers(&document);
    let names: Vec<_> = texts.iter().map(|layer| &layer["name"]).collect();
    assert_eq!(names, ["Shadow", "Stroke", "Both"]);

    // Shadow only, with the Adobe-calibrated units: 24 px at 45° clockwise
    // from up, 80% linear-light black as alpha 0.4886, blur 8 as σ 0.7728 px
    // and size 2 as a 0.978 px spread.
    let [shadow_only, stroke_only, both] = texts.as_slice() else {
        unreachable!("three text layers")
    };
    assert_eq!(
        (*crate::test_support::layer_range(shadow_only)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(shadow_only["sourceText"]["applyStroke"], false);
    let shadow = drop_shadow(shadow_only);
    let leg = 24.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert_near(&shadow["offset"], &[leg, -leg]);
    assert_near(&shadow["color"], &[0.0, 0.0, 0.0, 0.488_597_91]);
    assert_near(
        &json!([&shadow["blurRadius"], &shadow["spreadRadius"]]),
        &[0.7728, 0.978],
    );

    // Stroke only: Premiere's 8 px outside stroke is the text's own 16 px
    // centered stroke under the fill; there is no effect.
    let source = &stroke_only["sourceText"];
    assert_eq!(
        (
            &source["applyStroke"],
            &source["strokeWidth"],
            &source["strokeOverFill"]
        ),
        (&json!(true), &json!(16.0), &json!(false))
    );
    assert_near(&source["strokeColor"], &[0.0, 0.0, 1.0, 1.0]);
    assert!(stroke_only.get("effects").is_none());

    // Both: the shadow is cast from the stroked text below the fill.
    assert_eq!(both["sourceText"]["strokeWidth"], json!(12.0));
    let shadow = drop_shadow(both);
    let leg = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert_near(&shadow["offset"], &[leg, leg]);
    assert_near(&shadow["color"], &[0.0, 0.0, 0.0, 1.0]);
    assert_near(&json!([&shadow["blurRadius"]]), &[0.5796]);

    // Edit the current document; export must read these values, not the XML.
    let asset_id = document["composition"]["layers"][3]["source"]["assetId"]
        .as_str()
        .unwrap()
        .to_owned();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let effect = &mut layers[0]["effects"][0]["effect"];
    effect["offset"] = json!([0.0, 10.0]);
    effect["color"] = json!([0.2, 0.4, 0.6, 0.5]);
    effect["blurRadius"] = json!(2.0);
    effect["spreadRadius"] = json!(1.0);
    layers[1]["sourceText"]["strokeWidth"] = json!(20.0);
    layers[2].as_object_mut().unwrap().remove("effects");
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(&asset_id, fixture("video-30fps-10s.mp4"), AssetKind::Video)
        .unwrap()
        .write(&edited)
        .unwrap();
    let native = root.join("native");
    tesseract_to_premiere(&edited, &native, false).unwrap();
    let (project, omissions) = PrProjectFile::load(native.join("project.prproj")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(project.sequences().count(), 1);

    // Reading the exported project again recovers the edited styles.
    let reimported = root.join("reimported");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &reimported, None, false).unwrap();
    // The edited shadows are translucent and non-black, which the black-shadow
    // opacity calibration only approximates; that is reported, not hidden.
    let (shadow_approximations, other): (Vec<_>, Vec<_>) = omissions
        .iter()
        .partition(|omission| omission.reason.contains("translucent non-black shadow"));
    assert!(!shadow_approximations.is_empty(), "{omissions:?}");
    assert_eq!(other.len(), 1, "{omissions:?}");
    assert_eq!(other[0].reason, "font \"Arial-BoldMT\" is not packaged in this document; import it with tsrct project import-font before preview or export.");
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let texts = text_layers(&document);
    let [shadow_only, stroke_only, both] = texts.as_slice() else {
        panic!("expected three text layers: {texts:?}")
    };
    let shadow = drop_shadow(shadow_only);
    assert_near(&shadow["offset"], &[0.0, 10.0]);
    assert_near(&shadow["color"], &[0.2, 0.4, 0.6, 0.5]);
    assert_near(
        &json!([&shadow["blurRadius"], &shadow["spreadRadius"]]),
        &[2.0, 1.0],
    );
    assert_eq!(stroke_only["sourceText"]["strokeWidth"], json!(20.0));
    assert!(both.get("effects").is_none());
    assert_eq!(both["sourceText"]["applyStroke"], true);
}
