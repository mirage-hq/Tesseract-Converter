//! Independent Adobe-native wiggle-coverage fixture (AE 26.5x89, managed
//! `run_jsx`): eased spatial Position, separated Position, 3D X Rotation and
//! Orientation, and a wiggle frequency read from a Bezier-keyed Slider.

use super::*;
use crate::expression_samples::PropertyIdentity;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] = include_bytes!("../../tests/fixtures/expression_samples/wiggle_coverage.aep");
const READBACK: &str =
    include_str!("../../tests/fixtures/expression_samples/wiggle_coverage.readback.json");
const SOURCE_SHA256: &str = "270686adc7d6de144f2a651f4e1e62a686b9620c8867046fdbcd3dc7660c185a";

fn project() -> crate::structure::StructuralProject {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    crate::structure::read_project(SOURCE).unwrap()
}

fn identity(match_name: &str) -> PropertyIdentity {
    if match_name.starts_with("ADBE Slider Control") {
        PropertyIdentity::Effect {
            index: 1,
            match_name: match_name.into(),
        }
    } else {
        PropertyIdentity::Transform {
            match_name: match_name.into(),
        }
    }
}

fn components(value: &serde_json::Value) -> Vec<f64> {
    match value {
        serde_json::Value::Array(values) => values.iter().map(|v| v.as_f64().unwrap()).collect(),
        value => vec![value.as_f64().unwrap()],
    }
}

/// The pre-expression `value` the evaluator feeds to wiggle comes from the
/// converter's editable key mapping. Adobe's own pre-expression samples bound
/// how far that mapping is from native interpolation at every output frame.
#[test]
fn wiggle_coverage_base_values_track_adobe_pre_expression_samples() {
    let project = project();
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
        let mut model = Model::new(&items, comp_id, comp, true);
        let property = identity(match_name);
        let slot = model
            .properties
            .iter()
            .position(|p| p.comp_id == comp_id && p.layer_id == layer_id && p.identity == property)
            .unwrap_or_else(|| panic!("{match_name} not modelled"));
        assert_eq!(model.properties[slot].error, None, "{match_name}");
        model.properties[slot].expression = Some(syntax::compile("value;").unwrap());
        let samples = case["samples"].as_array().unwrap();
        let times: Vec<f64> = samples
            .iter()
            .map(|s| s["time"].as_f64().unwrap())
            .collect();
        let actual = evaluate_grid(&mut prepare(&model).unwrap(), slot, &times).unwrap();
        let mut max_error = 0.0_f64;
        for (sample, actual) in samples.iter().zip(&actual) {
            let expected = components(&sample["pre"]);
            assert_eq!(actual.len(), expected.len(), "{match_name} dimensions");
            for (a, e) in actual.iter().zip(&expected) {
                max_error = max_error.max((a - e).abs());
            }
        }
        // Spatial Position uses the keyed import's 0.25 source-unit path
        // refinement; every other eased property maps exactly to FX cubic easing.
        let tolerance = if match_name == "ADBE Position" {
            0.25
        } else {
            1e-3
        };
        eprintln!(
            "{} {match_name}: max pre-expression error {max_error:e}",
            case["composition"]
        );
        assert!(
            max_error <= tolerance,
            "{} {match_name}: max error {max_error} exceeds {tolerance}",
            case["composition"]
        );
    }
}

fn dynamics_for(comp_id: u32) -> (Vec<String>, serde_json::Value) {
    let project = project();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(comp_id)).unwrap();
    let json = String::from_utf8(converted.document.to_json_vec().unwrap()).unwrap();
    assert!(!json.to_ascii_lowercase().contains("jsscript"));
    let warnings: Vec<String> = converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    let tracks = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter_map(|entry| {
            let property = entry.target.as_property()?;
            let track = entry.animator.keyframe_track()?;
            track.validate_for_target(&entry.target).unwrap();
            let varying = track
                .keyframes()
                .windows(2)
                .any(|k| k[0].value() != k[1].value());
            Some((format!("{:?}", property.property_type()), varying))
        })
        .collect::<Vec<_>>();
    (warnings, serde_json::json!(tracks))
}

#[test]
fn wiggle_coverage_native_wiggle_lowers_to_editable_varying_tracks() {
    for (comp_id, layer, expected) in [
        (16, "ADBE Position", &["PositionX", "PositionY"][..]),
        (30, "ADBE Position_0", &["PositionX"][..]),
        (44, "ADBE Rotate X", &["RotationX"][..]),
        (
            44,
            "ADBE Orientation",
            &["OrientationX", "OrientationY", "OrientationZ"][..],
        ),
        (58, "ADBE Rotate Z", &["Rotation"][..]),
    ] {
        let (warnings, tracks) = dynamics_for(comp_id);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(layer) && w.contains("AE expression wiggle approximated")),
            "comp {comp_id} {layer}: missing wiggle approximation diagnostic: {warnings:?}"
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.contains(layer)
                    && w.contains("converter expression evaluation unsupported")),
            "comp {comp_id} {layer}: evaluation was denied: {warnings:?}"
        );
        for property in expected {
            assert!(
                tracks
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t[0] == *property && t[1] == true),
                "comp {comp_id}: no varying editable {property} track in {tracks}"
            );
        }
    }
}
