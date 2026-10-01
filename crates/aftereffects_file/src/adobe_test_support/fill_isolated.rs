use serde_json::Value;

use crate::{structure::read_project, structure_document::to_structural_fx_document};

const SOURCE_PATH: &str = "crates/aftereffects_file/tests/fixtures/effects/fill_isolated.aep";
const SOURCE: &[u8] = include_bytes!("../../tests/fixtures/effects/fill_isolated.aep");
const READBACK: &str = include_str!("../../tests/fixtures/effects/fill_isolated.readback.json");

fn find_effect<'a>(value: &'a Value, kind: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some(kind) {
                return Some(value);
            }
            object.values().find_map(|value| find_effect(value, kind))
        }
        Value::Array(values) => values.iter().find_map(|value| find_effect(value, kind)),
        _ => None,
    }
}

fn contains_rgb(value: &Value, expected: [f64; 3]) -> bool {
    match value {
        Value::Array(values) => {
            let matches = values.len() >= 3
                && values
                    .iter()
                    .take(3)
                    .zip(expected)
                    .all(|(actual, expected)| {
                        actual
                            .as_f64()
                            .is_some_and(|actual| (actual - expected).abs() < 1e-6)
                    });
            matches || values.iter().any(|value| contains_rgb(value, expected))
        }
        Value::Object(object) => object.values().any(|value| contains_rgb(value, expected)),
        _ => false,
    }
}

#[test]
#[ignore = "Adobe-native feature proof runs only through explicit adobe-test selection"]
fn adobe_fill_imports_tint_endpoints_and_percentage_amount() {
    let mut cases = super::CaseBatch::new();
    cases.run(SOURCE_PATH, 1, || {
        let readback: Value = serde_json::from_str(READBACK).expect("native authoring readback");
        assert_eq!(readback["authored"]["opacityPercent"], 65);
        let native_color = readback["authored"]["colorRgba"]
            .as_array()
            .expect("native Fill color");

        let project = read_project(SOURCE).expect("independently Adobe-authored Fill source");
        let imported = to_structural_fx_document(&project, Some(1)).expect("fresh Fill import");
        let diagnostics: Vec<_> = imported
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect();
        let document = imported.document.to_json_value().expect("editable FX JSON");
        let effect = find_effect(&document, "tintTritone").expect("editable Tint approximation");
        for (name, component) in [
            ("blackR", 0), ("blackG", 1), ("blackB", 2),
            ("whiteR", 0), ("whiteG", 1), ("whiteB", 2),
        ] {
            let expected = native_color[component].as_f64().expect("native RGB component");
            let actual = effect[name].as_f64().expect("editable Tint component");
            assert!((actual - expected).abs() < 1e-6, "{name}: expected {expected}, got {actual}");
        }
        // AE stores 0.65 as f32 (readback: 0.64999997615814), not exact f64.
        let expected_amount = f64::from(0.65_f32) * 100.0;
        let actual_amount = effect["amount"].as_f64().expect("editable Tint amount");
        let f32_percent_error = f64::from(f32::EPSILON) * 100.0;
        assert!(
            (actual_amount - expected_amount).abs() <= f32_percent_error,
            "native f32 Fill opacity should map to Tint percentage: expected {expected_amount}, got {actual_amount}"
        );
        assert!(
            (actual_amount - 65.0).abs() <= f32_percent_error,
            "authored Fill 65% should survive f32 storage: got {actual_amount}"
        );
        assert!(contains_rgb(&document, [0.05, 0.15, 0.9]), "authored blue owner must survive Fill approximation");
        assert!(
            diagnostics.iter().any(|message| message.contains("Fill") && message.contains("Tint")),
            "Fill approximation must be diagnosed: {diagnostics:?}"
        );
        assert!(!document.to_string().to_ascii_lowercase().contains("jsscript"));
    });
    cases.finish();
}
