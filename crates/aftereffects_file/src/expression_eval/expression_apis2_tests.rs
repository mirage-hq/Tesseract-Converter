//! Second Adobe-native expression-API fixture (AE 26.5x89, managed `run_jsx`):
//! Source Text strings, 3D layer space through AE's default camera and Shape
//! `sourceRectAtTime`, compared with Adobe's evaluated values at all frames.

use super::*;
use crate::expression_samples::PropertyIdentity;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/expression_samples/expression_apis2.aep");
const READBACK: &str =
    include_str!("../../tests/fixtures/expression_samples/expression_apis2.readback.json");
const SOURCE_SHA256: &str = "b222051ea8b0aa583e7846383e7a1476f5c9fdd2cd5223f43adfda694befd205";

#[test]
fn expression_apis2_match_adobe_evaluated_values() {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    let project = crate::structure::read_project(SOURCE).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    for case in readback["cases"].as_array().unwrap() {
        let comp_id = case["composition_id"].as_u64().unwrap() as u32;
        let layer_id = case["layer_id"].as_u64().unwrap() as u32;
        let match_name = case["match_name"].as_str().unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(comp_id).unwrap().kind
        else {
            panic!("composition {comp_id} missing");
        };
        let model = Model::new(&items, comp_id, comp, true);
        let identity = if match_name == "ADBE Text Document" {
            PropertyIdentity::SourceText {}
        } else {
            PropertyIdentity::Transform {
                match_name: match_name.into(),
            }
        };
        let slot = model
            .properties
            .iter()
            .position(|p| p.comp_id == comp_id && p.layer_id == layer_id && p.identity == identity)
            .unwrap_or_else(|| panic!("{} {match_name} not modelled", case["layer"]));
        let samples = case["samples"].as_array().unwrap();
        let times: Vec<f64> = samples
            .iter()
            .map(|s| s["time"].as_f64().unwrap())
            .collect();
        let label = format!("{} {match_name}", case["layer"]);
        if matches!(identity, PropertyIdentity::SourceText {}) {
            let texts = evaluate_text_grid(&mut prepare(&model).unwrap(), slot, &times)
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            for (sample, text) in samples.iter().zip(&texts) {
                assert_eq!(sample["evaluated"].as_str().unwrap(), text, "{label}");
            }
            continue;
        }
        let actual = evaluate_grid(&mut prepare(&model).unwrap(), slot, &times)
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        let mut max_error = 0.0_f64;
        for (sample, actual) in samples.iter().zip(&actual) {
            let expected: Vec<f64> = match &sample["evaluated"] {
                serde_json::Value::Array(values) => {
                    values.iter().map(|v| v.as_f64().unwrap()).collect()
                }
                value => vec![value.as_f64().unwrap()],
            };
            for (a, e) in actual.iter().zip(&expected) {
                max_error = max_error.max((a - e).abs());
            }
        }
        eprintln!("{label}: max error vs Adobe {max_error:e}");
        assert!(max_error <= 1e-3, "{label}: max error {max_error}");
    }
}

/// A fresh import turns the evaluated Source Text strings into held text
/// segments: every distinct Adobe string appears as editable text content.
#[test]
fn source_text_expressions_import_as_held_text_segments() {
    let project = crate::structure::read_project(SOURCE).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    let json = converted.document.to_json_value().unwrap().to_string();
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    for case in readback["cases"].as_array().unwrap() {
        if case["match_name"] != "ADBE Text Document" {
            continue;
        }
        for sample in case["samples"].as_array().unwrap() {
            let text = sample["evaluated"].as_str().unwrap();
            assert!(
                json.contains(&format!("\"text\":\"{text}\"")),
                "{}: missing held text {text:?}",
                case["layer"]
            );
        }
    }
    assert!(
        converted.diagnostics.iter().any(|d| d
            .message
            .contains("Source Text expression evaluated by the converter")),
        "missing Source Text evaluation diagnostic"
    );
    assert!(!json.to_ascii_lowercase().contains("jsscript"));
}
