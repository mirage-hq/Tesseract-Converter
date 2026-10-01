//! Nondefault independently Adobe-authored controls complement the default catalog.
use super::*;

const BRIGHTNESS: &[u8] =
    include_bytes!("../../../tests/fixtures/effects_coverage/brightness_contrast_controls.aep");

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_brightness_contrast_static_keeps_signed_editable_controls() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/effects_coverage/brightness_contrast_controls.aep",
        1,
        || {
            let native = read_project(BRIGHTNESS).unwrap();
            let (document, diagnostics) = imported_case(&native, 1);
            let record = effect_payload(&document, "brightnessContrast")
                .expect("editable Brightness/Contrast");
            assert_eq!(record["enabled"], true);
            assert_editable_fields(
                &record["effect"],
                &serde_json::json!({"brightness":20,"contrast":-25}),
                "independent static Brightness/Contrast",
            );
            assert!(!document.to_string().contains("JsScript"));
            assert!(
                !diagnostics
                    .iter()
                    .any(|d| d.contains("ADBE Brightness & Contrast 2")
                        && d.contains("entire effect omitted"))
            );
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_brightness_contrast_animation_keeps_distinct_linear_tracks() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/effects_coverage/brightness_contrast_controls.aep",
        14,
        || {
            let native = read_project(BRIGHTNESS).unwrap();
            let (document, _) = imported_case(&native, 14);
            let record = effect_payload(&document, "brightnessContrast")
                .expect("editable Brightness/Contrast");
            let entries = document["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap();
            for (param, values) in [("brightness", [20.0, 50.0]), ("contrast", [-25.0, 25.0])] {
                let tracks: Vec<_> = entries
                    .iter()
                    .filter(|e| {
                        e["target"]["kind"] == "effectProperty"
                            && e["target"]["effectId"] == record["id"]
                            && e["target"]["paramName"] == param
                    })
                    .collect();
                assert_eq!(
                    tracks.len(),
                    1,
                    "{param}: one independently editable occurrence target"
                );
                let keys = tracks[0]["animator"]["keyframes"].as_array().unwrap();
                assert_eq!(
                    keys.len(),
                    2,
                    "{param}: preserve authored knots, not frame baking"
                );
                for (index, key) in keys.iter().enumerate() {
                    assert_eq!(key["layerTime"], index * 1000);
                    assert_eq!(key["value"]["type"], "float");
                    close(
                        &[key["value"]["value"].as_f64().unwrap()],
                        &[values[index]],
                        param,
                    );
                    assert_eq!(key["easing"]["type"], "linear");
                }
            }
            assert!(!document.to_string().contains("JsScript"));
        },
    );
    cases.finish();
}
