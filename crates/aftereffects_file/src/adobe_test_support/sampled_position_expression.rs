use serde_json::Value;

use crate::{
    expression_samples::ExpressionSamples, structure::read_project,
    structure_document::to_structural_fx_document_with_assets_and_expressions,
};

const SOURCE_PATH: &str =
    "crates/aftereffects_file/tests/fixtures/expression_samples/sampled_position_expression.aep";
const SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/expression_samples/sampled_position_expression.aep");
const SAMPLES: &[u8] =
    include_bytes!("../../tests/fixtures/expression_samples/sampled_position_expression.v2.json");
const READBACK: &str = include_str!(
    "../../tests/fixtures/expression_samples/sampled_position_expression.readback.json"
);

fn position_track<'a>(document: &'a Value, property: &str) -> &'a [Value] {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .expect("editable dynamics")
        .iter()
        .find(|entry| {
            entry["target"]["kind"] == "layer" && entry["target"]["propertyType"] == property
        })
        .unwrap_or_else(|| panic!("missing editable {property} track"))["animator"]["keyframes"]
        .as_array()
        .expect("editable keyframe track")
}

fn cubic(coordinate1: f64, coordinate2: f64, t: f64) -> f64 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * t * coordinate1 + 3.0 * inverse * t * t * coordinate2 + t * t * t
}

fn easing_progress(easing: &Value, progress: f64) -> f64 {
    match easing["type"].as_str().expect("easing type") {
        "hold" => 0.0,
        "linear" => progress,
        "cubicBezier" => {
            let x1 = easing["x1"].as_f64().expect("cubic x1");
            let x2 = easing["x2"].as_f64().expect("cubic x2");
            let mut low = 0.0;
            let mut high = 1.0;
            for _ in 0..60 {
                let middle = (low + high) / 2.0;
                if cubic(x1, x2, middle) < progress {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let parameter = (low + high) / 2.0;
            cubic(
                easing["y1"].as_f64().expect("cubic y1"),
                easing["y2"].as_f64().expect("cubic y2"),
                parameter,
            )
        }
        kind => panic!("unsupported editable easing {kind}"),
    }
}

fn track_value(keys: &[Value], time_ms: f64) -> f64 {
    let next = keys
        .iter()
        .position(|key| key["layerTime"].as_f64().expect("key time") >= time_ms)
        .unwrap_or(keys.len() - 1);
    if next == 0 {
        return keys[0]["value"]["value"].as_f64().expect("float key");
    }
    let previous = &keys[next - 1];
    let current = &keys[next];
    let start = previous["layerTime"].as_f64().expect("previous time");
    let end = current["layerTime"].as_f64().expect("current time");
    let progress = easing_progress(&current["easing"], (time_ms - start) / (end - start));
    let from = previous["value"]["value"].as_f64().expect("previous float");
    let to = current["value"]["value"].as_f64().expect("current float");
    from + (to - from) * progress
}

fn contains_hidden_controller(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            (object.get("name") == Some(&Value::String("Controller".to_owned()))
                && object.get("isHidden") == Some(&Value::Bool(true)))
                || object.values().any(contains_hidden_controller)
        }
        Value::Array(values) => values.iter().any(contains_hidden_controller),
        _ => false,
    }
}

#[test]
#[ignore = "Adobe-native feature proof runs only through explicit adobe-test selection"]
fn adobe_sampled_position_expression_imports_sparse_editable_xy_tracks() {
    let mut cases = super::CaseBatch::new();
    cases.run(SOURCE_PATH, 1, || {
        let readback: Value = serde_json::from_str(READBACK).expect("native authoring readback");
        assert_eq!(readback["subject"]["position"]["storedBase"], serde_json::json!([23, 41, 0]));
        assert_eq!(readback["controller"]["effect"]["control"]["value"], 35);

        let samples = ExpressionSamples::from_json_for_source(SAMPLES, SOURCE)
            .expect("exact-source-bound Adobe expression samples");
        assert_eq!(samples.properties().len(), 1);
        assert!(samples.errors().is_empty());
        let actual_times = samples.properties()[0].sample_times_seconds();
        assert!((actual_times[1] - 25.0 / 24_576.0).abs() < 1.0e-12);
        assert!((actual_times[2] - 49.0 / 24_576.0).abs() < 1.0e-12);
        let project = read_project(SOURCE).expect("independently Adobe-authored source");
        let imported = to_structural_fx_document_with_assets_and_expressions(
            &project,
            Some(1),
            &mut |_| false,
            &samples,
        )
        .expect("fresh expression-sampled import");
        let diagnostics: Vec<_> = imported
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect();
        let document = imported.document.to_json_value().expect("editable FX JSON");
        let x = position_track(&document, "positionX");
        let y = position_track(&document, "positionY");
        assert!(x.len() < 128 && y.len() < 128, "sparse fitted tracks");

        for sample in readback["samples"].as_array().expect("native samples") {
            let seconds = sample["timeSeconds"].as_f64().expect("sample time");
            let expected = sample["evaluated"].as_array().expect("native vector");
            for (actual, expected) in [track_value(x, seconds * 1000.0), track_value(y, seconds * 1000.0)]
                .into_iter()
                .zip(expected.iter().take(2).map(|value| value.as_f64().expect("native scalar")))
            {
                assert!((actual - expected).abs() <= 0.001, "{seconds}s: expected {expected}, got {actual}");
            }
        }
        assert!(!document.to_string().to_ascii_lowercase().contains("jsscript"));
        assert!(
            contains_hidden_controller(&document)
                || diagnostics.iter().any(|message| message.contains("Controller")),
            "hidden source controller must remain hidden or be explicitly diagnosed: {diagnostics:?}"
        );
    });
    cases.finish();
}
