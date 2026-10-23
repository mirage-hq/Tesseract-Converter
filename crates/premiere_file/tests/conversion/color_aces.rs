//! Premiere ACEScct Color Setup ↔ FX ACES `colorManagement`, on the natively
//! authored `color_aces_pr_sequence.prproj` (Premiere 26.5.2: ACEScct working
//! colour space, the chart still with Lumetri Exposure +1, and an in-house
//! S-Log3/S-Gamut3.Cine clip with an Interpret Footage Color Space Override).

use super::support::*;
use fx_conv::{ConversionMode, ExportFromTesseract};
use premiere_file::{Premiere, PremiereExportOptions};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tesseract_file::TesseractFile;

const ACES_PROJECT: &str = "color_aces_pr_sequence.prproj";
const ACES_SEQUENCE: &str = "866909df-d4ee-478f-8494-d437dd4ebf6d";
const ACES_PROJECT_SHA256: &str =
    "aba684731e624f88d3dad081d6932ccdccd6f70b9dcdea3999d5e650a5879520";
const SLOG3_CINE: &str = "camera:sony-slog3-sgamut3-cine:v1";

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn sha256(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

/// Every layer of `layers` and its descendants whose type is `kind`.
fn layers_of_type<'a>(layers: &'a Value, kind: &str, found: &mut Vec<&'a Value>) {
    for layer in layers.as_array().into_iter().flatten() {
        if layer["type"] == kind {
            found.push(layer);
        }
        layers_of_type(&layer["layers"], kind, found);
    }
}

/// The texts of the children of the one `VideoTrackGroup` that declares a
/// working colour space.
fn sequence_colour_fields(xml: &str) -> std::collections::BTreeMap<String, String> {
    let document = roxmltree::Document::parse(xml).unwrap();
    let groups: Vec<_> = document
        .descendants()
        .filter(|node| {
            node.has_tag_name("VideoTrackGroup")
                && node
                    .children()
                    .any(|child| child.has_tag_name("WorkingColorSpace"))
        })
        .collect();
    let [group] = groups[..] else {
        panic!("expected one working colour space declaration");
    };
    group
        .children()
        .filter(roxmltree::Node::is_element)
        .filter(|child| {
            !["TrackGroup", "FrameRect", "ComponentOwner"].contains(&child.tag_name().name())
        })
        .map(|child| {
            (
                child.tag_name().name().to_owned(),
                child.text().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// The imported document: ACES policy, the log clip's `cameraInput`, and no
/// legacy override or HDR-target report.
fn assert_aces_import(omissions: &[premiere_file::Omission], document: &Value) {
    assert_eq!(
        document["colorManagement"],
        json!({"workingSpace": "acescct", "outputTransform": "aces2.0", "compositing": "footageOnly"})
    );
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .starts_with("Premiere ACEScct working colour space imported as FX colorManagement acescct / aces2.0 / footageOnly")),
        "{omissions:?}"
    );
    assert!(
        !omissions.iter().any(|omission| omission
            .reason
            .contains("HDR sequence target not admitted")
            || omission.reason.contains("Color Space Override")),
        "{omissions:?}"
    );
    let mut videos = Vec::new();
    layers_of_type(&document["composition"]["layers"], "Video", &mut videos);
    let [video] = videos[..] else {
        panic!("{document}");
    };
    assert_eq!(
        video["source"]["inputTransform"],
        json!({"semanticVersion": 1, "transform": {"type": "cameraInput", "inputEncoding": SLOG3_CINE}})
    );
    let mut images = Vec::new();
    layers_of_type(&document["composition"]["layers"], "Image", &mut images);
    let [image] = images[..] else {
        panic!("{document}");
    };
    assert!(image["source"].get("inputTransform").is_none(), "{image}");
    assert!(
        image["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["effect"]["type"] == "primaryGrade"),
        "{image}"
    );
}

#[test]
fn native_aces_sequence_imports_as_an_fx_aces_document_with_a_camera_input() {
    assert_eq!(sha256(&fixture_path(ACES_PROJECT)), ACES_PROJECT_SHA256);
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(
        fixture_path(ACES_PROJECT),
        &output,
        Some(ACES_SEQUENCE),
        false,
    )
    .unwrap();
    let document = TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap();
    assert_aces_import(&omissions, &document);
}

#[test]
fn fx_aces_document_exports_the_native_acescct_sequence_and_camera_override() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("converted");
    premiere_to_tesseract(
        fixture_path(ACES_PROJECT),
        &output,
        Some(ACES_SEQUENCE),
        false,
    )
    .unwrap();
    let archive = first_project(&output);
    let package = dir.path().join("native");
    Premiere
        .export_from_tesseract(
            &archive,
            &package,
            &PremiereExportOptions::default(),
            ConversionMode::Write,
        )
        .unwrap();
    let ours_xml = read_xml(&package.join("project.prproj"));
    let native_xml = read_xml(&fixture_path(ACES_PROJECT));
    let ours = sequence_colour_fields(&ours_xml);
    let native = sequence_colour_fields(&native_xml);
    // Every natively saved ACEScct field is written with the native text;
    // the immersive-video declaration is the writer's usual extra field.
    for (field, value) in &native {
        assert_eq!(ours.get(field), Some(value), "{field}");
    }
    let extra: Vec<_> = ours
        .keys()
        .filter(|field| !native.contains_key(*field))
        .map(String::as_str)
        .collect();
    assert_eq!(extra, ["ImmersiveVideoVRConfiguration"]);
    let override_text = r#"<OverriddenColorSpace>{"baseColorProfile":{"colorProfileName":"Sony S-Log3/S-Gamut3.Cine"},"baseProfileType":1}</OverriddenColorSpace>"#;
    assert!(native_xml.contains(override_text));
    assert!(
        ours_xml.contains(override_text),
        "camera override not written"
    );
    assert!(ours_xml.contains("<IsColorSpacOverridden>true</IsColorSpacOverridden>"));

    // Supplementary: our own project reimports with the same policy and input.
    let reimported = dir.path().join("reimported");
    let omissions =
        premiere_to_tesseract(package.join("project.prproj"), &reimported, None, false).unwrap();
    let document = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    assert_aces_import(&omissions, &document);
}
