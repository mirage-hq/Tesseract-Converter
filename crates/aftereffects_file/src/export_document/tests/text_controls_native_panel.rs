//! Explicit edited-FX Text inputs and independently specified native values.
//! All cases are unrun; our native reader is supplementary, not Adobe inspection.
use super::*;
use crate::{properties, rifx::Chunk};

fn numeric(chunks: &[Chunk], target: &str) -> Option<properties::NumericProperty> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                return properties::read_numeric(properties::unique_list(run, *b"tdbs").ok()?).ok();
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| numeric(children, target))
}

fn close(actual: f64, expected: &Value, label: &str) {
    let expected = expected.as_f64().expect("manual numeric value");
    assert!(
        (actual - expected).abs() < 1e-6,
        "{label}: expected {expected}, got {actual}"
    );
}

fn check_case(name: &str, input_json: &str, expected_json: &str) {
    let input: Value = serde_json::from_str(input_json).expect("explicit authored FX fixture");
    let expected: Value = serde_json::from_str(expected_json).expect("manual native controls");
    assert_eq!(expected["status"], "AUTHORED_UNRUN_UNMEASURED");
    assert_eq!(
        expected["native_authoring"],
        format!("native-specs.json#fx-export-{name}")
    );
    assert_eq!(input["composition"]["name"], name);
    assert!(
        !input_json.contains("jsScript"),
        "Text export must remain native and editable"
    );
    let output = export(input);
    if let Some(directory) = std::env::var_os("AEP_EFFECTS_FX_PANEL_DIR") {
        std::fs::create_dir_all(&directory).expect("artifact directory");
        let base = std::path::Path::new(&directory).join(name);
        std::fs::write(base.with_extension("fx.json"), input_json).expect("explicit FX artifact");
        std::fs::write(base.with_extension("aep"), &output.bytes).expect("fresh native artifact");
        std::fs::write(base.with_extension("expected.json"), expected_json)
            .expect("manual oracle artifact");
    }
    let native = read_project(&output.bytes).expect("supplementary fresh native parse");
    let ItemKind::Composition(composition) = &native.item(1).expect("root composition").kind else {
        panic!("{name}: root is not a composition");
    };
    assert_eq!((composition.width, composition.height), (320, 180));
    close(composition.duration_secs, &json!(2.0), "duration");
    close(composition.frame_rate, &json!(24.0), "source fps");
    let text_layers = composition
        .layers
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .collect::<Vec<_>>();
    let owner = text_layers
        .iter()
        .copied()
        .find(|layer| layer.name.as_ref() == expected["layerName"].as_str().unwrap())
        .unwrap_or_else(|| {
            panic!(
                "{name}: native editable Text owner missing: {:?}",
                output.diagnostics
            )
        });
    assert_eq!(owner.record.source_id(), 0, "not flattened to footage");
    assert!(!output.bytes.windows(8).any(|bytes| bytes == b"JsScript"));

    if name == "panel-text-unsupported-layout" {
        assert_eq!(text_layers.len(), 2, "convertible sibling must survive");
        assert!(
            text_layers
                .iter()
                .any(|layer| layer.name.as_ref() == expected["siblingName"].as_str().unwrap())
        );
        for diagnostic in expected["diagnostics"].as_array().unwrap() {
            let needle = diagnostic.as_str().unwrap();
            assert!(
                output
                    .diagnostics
                    .iter()
                    .any(|message| message.message.contains(needle)),
                "missing contextual omission {needle}: {:?}",
                output.diagnostics
            );
        }
        // Native mixed character-run style has no current FX input shape. This
        // case asserts diagnosed FX extensions + sibling retention, NOT run parity.
        return;
    }

    if name == "panel-text-style-hold" {
        // Source Text keys are complete document snapshots; check authored text,
        // font-size/style values, not merely a nonempty text layer.
        let converted =
            to_structural_fx_document(&native, Some(1)).expect("editable text re-import");
        let graph = converted.document.composition().dynamics().entries();
        let text_keys = graph
            .iter()
            .find(|entry| {
                matches!(&entry.target,
            fx_schema::PropertyTarget::LayerProperty(target)
                if target.property_type() == fx_schema::PropType::TextContent)
            })
            .expect("native Source Text Hold keys must be editable")
            .animator
            .keyframe_track()
            .expect("Hold document track")
            .keyframes();
        for (key, oracle) in text_keys
            .iter()
            .zip(expected["documentKeys"].as_array().unwrap())
        {
            assert_eq!(
                key.layer_time().as_millis(),
                oracle[0].as_i64().unwrap() * 1000
            );
            assert_eq!(
                serde_json::to_value(key.value()).unwrap()["value"],
                oracle[1]
            );
            assert_eq!(key.easing(), fx_schema::PropertyKeyframeEasing::Hold);
        }
        assert_eq!(
            text_keys.len(),
            expected["documentKeys"].as_array().unwrap().len()
        );
        assert!(
            properties::runs(&owner.content).is_ok(),
            "ADBE Text Document must have native group records"
        );
        return;
    }

    if let Some(tracks) = expected["nativeTracks"].as_array() {
        assert_eq!(
            input_json.matches("\"propertyName\"").count(),
            tracks.len(),
            "one independently specified editable FX target per expected native track"
        );
        for track in tracks {
            let property = track[0].as_str().expect("native match name");
            let native_track = numeric(&owner.content, property)
                .unwrap_or_else(|| panic!("{name}: missing native {property}"));
            assert!(
                native_track.animated,
                "{property} must be editable keyframes"
            );
            assert_eq!(native_track.keyframes.len(), 2, "{property} key count");
            for (key, index) in native_track.keyframes.iter().zip([1usize, 2]) {
                close(
                    key.time_secs,
                    &json!(index - 1),
                    &format!("{property} time"),
                );
                let values = track[index]
                    .as_array()
                    .expect("manual native key components");
                assert_eq!(key.values.len(), values.len(), "{property} components");
                for (actual, expected) in key.values.iter().zip(values) {
                    close(*actual, expected, property);
                }
                assert_eq!(
                    (key.in_interpolation, key.out_interpolation),
                    (1, 1),
                    "{property} linear interpolation"
                );
            }
        }
    }
    if let Some(path) = expected.get("path") {
        for (property, value) in [
            ("ADBE Text Path", &path["maskIndex"]),
            ("ADBE Text Reverse Path", &path["reversePath"]),
            (
                "ADBE Text Perpendicular To Path",
                &path["perpendicularToPath"],
            ),
            ("ADBE Text Force Align Path", &path["forceAlignment"]),
        ] {
            let actual = numeric(&owner.content, property)
                .unwrap_or_else(|| panic!("missing editable Path Options {property}"));
            let expected = if let Some(value) = value.as_bool() {
                f64::from(u8::from(value))
            } else {
                value.as_f64().unwrap()
            };
            assert_eq!(actual.values, vec![expected], "{property}");
        }
        assert_eq!(text_layers.len(), 1, "guide consumed as native Text Mask 1");
    }
    // Animator and selector names are intentionally specified by the native
    // authoring request. A raw property check cannot independently verify Adobe
    // readback or editable display names; that remains an explicit proof blocker.
}

macro_rules! panel_case {
    ($test:ident, $name:literal, $fixture:expr, $expected:expr) => {
        #[test]
        #[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
        fn $test() {
            crate::adobe_test_support::export_case($name, || {
                check_case($name, $fixture, $expected);
            });
        }
    };
}

panel_case!(
    panel_text_style_hold,
    "panel-text-style-hold",
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-style-hold.fx.json"
    ),
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-style-hold.expected.json"
    )
);
panel_case!(
    panel_text_animator_controls,
    "panel-text-animator-controls",
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-animator-controls.fx.json"
    ),
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-animator-controls.expected.json"
    )
);
panel_case!(
    panel_text_selectors_more,
    "panel-text-selectors-more",
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-selectors-more.fx.json"
    ),
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-selectors-more.expected.json"
    )
);
panel_case!(
    panel_text_path_controls,
    "panel-text-path-controls",
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-path-controls.fx.json"
    ),
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-path-controls.expected.json"
    )
);
panel_case!(
    panel_text_unsupported_layout,
    "panel-text-unsupported-layout",
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-unsupported-layout.fx.json"
    ),
    include_str!(
        "../../../tests/fixtures/text_controls_native_panel/panel-text-unsupported-layout.expected.json"
    )
);
