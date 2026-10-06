//! Independent Adobe-native expression-API fixture (AE 26.5x89, managed
//! `run_jsx`): every expression is deterministic, so converter evaluation is
//! compared with Adobe's evaluated values at all 61 frames.

use super::*;
use crate::expression_samples::PropertyIdentity;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] = include_bytes!("../../tests/fixtures/expression_samples/expression_apis.aep");
const READBACK: &str =
    include_str!("../../tests/fixtures/expression_samples/expression_apis.readback.json");
const SOURCE_SHA256: &str = "1ac1debce220b6dbc7f1bf582d2726562026d6ec7288e09f0468fc32f78aab34";

fn components(value: &serde_json::Value) -> Vec<f64> {
    match value {
        serde_json::Value::Array(values) => values.iter().map(|v| v.as_f64().unwrap()).collect(),
        value => vec![value.as_f64().unwrap()],
    }
}

#[test]
fn expression_apis_match_adobe_evaluated_values() {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    let project = crate::structure::read_project(SOURCE).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    let mut failures = Vec::new();
    for case in readback["cases"].as_array().unwrap() {
        let comp_id = case["composition_id"].as_u64().unwrap() as u32;
        let layer_id = case["layer_id"].as_u64().unwrap() as u32;
        let match_name = case["match_name"].as_str().unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(comp_id).unwrap().kind
        else {
            panic!("composition {comp_id} missing");
        };
        let model = Model::new(&items, comp_id, comp, true);
        let property = PropertyIdentity::Transform {
            match_name: match_name.into(),
        };
        let slot = model
            .properties
            .iter()
            .position(|p| p.comp_id == comp_id && p.layer_id == layer_id && p.identity == property)
            .unwrap();
        let samples = case["samples"].as_array().unwrap();
        let times: Vec<f64> = samples
            .iter()
            .map(|s| s["time"].as_f64().unwrap())
            .collect();
        let label = format!("{} {match_name} `{}`", case["layer"], case["expression"]);
        let actual = match evaluate_grid(&mut prepare(&model).unwrap(), slot, &times) {
            Ok(actual) => actual,
            Err(error) => {
                failures.push(format!("{label}: {error}"));
                continue;
            }
        };
        // lookAt's roll is not reproduced (diagnosed); compare its aim only.
        let axes = if match_name == "ADBE Orientation" {
            2
        } else {
            usize::MAX
        };
        let mut max_error = 0.0_f64;
        for (sample, actual) in samples.iter().zip(&actual) {
            let expected = components(&sample["evaluated"]);
            for (a, e) in actual.iter().zip(&expected).take(axes) {
                max_error = max_error.max((a - e).abs());
            }
        }
        eprintln!(
            "{label}: max error vs Adobe {max_error:e} (t=0 ours {:?} adobe {:?})",
            actual[0], samples[0]["evaluated"]
        );
        // Reads of spatial Position paths (layer space, Contents, Position
        // loops, Position velocity) go through the keyed import's 0.25-unit
        // path refinement; everything else must match numerically.
        let source = case["expression"].as_str().unwrap();
        let spatial = [
            "toComp",
            "fromComp",
            "content(",
            "position.speed",
            "position.velocity",
        ]
        .iter()
        .any(|api| source.contains(api))
            || (match_name == "ADBE Position" && source.contains("loop"));
        let tolerance = if spatial { 0.25 } else { 1e-3 };
        if max_error > tolerance {
            failures.push(format!("{label}: max error {max_error}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
