//! Explicit edited-FX vector input and manually specified native-control expectation.
//! Native source/reference evidence is pending; Adobe control and render proof are unrun.
use super::*;
use crate::{properties, rifx::Chunk};

fn close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "{label}: expected {expected}, got {actual}"
    );
}

fn numeric(chunks: &[Chunk], target: &str) -> Option<properties::NumericProperty> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                let storage = properties::unique_list(run, *b"tdbs").ok()?;
                return properties::read_numeric(storage).ok();
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| numeric(children, target))
}

fn check_values(actual: &[f64], expected: &[Value], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label} component count");
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        close(
            *actual,
            expected.as_f64().expect("manual numeric oracle"),
            &format!("{label}[{index}]"),
        );
    }
}

fn check_case(name: &str, input_json: &str, expected_json: &str) {
    let input: Value = serde_json::from_str(input_json).expect("committed FX panel input");
    let expected: Value =
        serde_json::from_str(expected_json).expect("committed native-control oracle");
    assert_eq!(input["composition"]["name"], name, "fixture case name");
    assert!(
        !input_json.contains("jsScript"),
        "native editable geometry only"
    );

    let directory = std::env::var_os("AEP_EFFECTS_FX_PANEL_DIR");
    if let Some(ref directory) = directory {
        std::fs::create_dir_all(directory).expect("panel output directory");
    }
    let output = export(input);
    if let Some(ref directory) = directory {
        let path = std::path::Path::new(directory).join(name);
        std::fs::write(path.with_extension("fx.json"), input_json).expect("FX panel input");
        std::fs::write(path.with_extension("aep"), &output.bytes).expect("FX panel native output");
        std::fs::write(path.with_extension("expected.json"), expected_json)
            .expect("native-control oracle");
    }

    let native = read_project(&output.bytes).expect("fresh native AEP");
    let ItemKind::Composition(composition) = &native.item(1).expect("root composition").kind else {
        panic!("{name}: root item is not a composition");
    };
    assert_eq!((composition.width, composition.height), (320, 180));
    close(composition.duration_secs, 2.0, "composition duration");
    close(composition.frame_rate, 24.0, "composition frame rate");
    let [layer] = composition.layers.as_slice() else {
        panic!(
            "{name}: expected one native Shape layer: {:?}",
            output.diagnostics
        );
    };
    assert_eq!(layer.name.as_ref(), expected["layerName"].as_str().unwrap());
    assert_eq!(layer.record.layer_type(), 4, "editable native Shape layer");
    assert_eq!(layer.record.source_id(), 0, "no flattened footage source");
    assert_eq!(expected["properties"].as_array().unwrap().len(), 4);

    for property in expected["properties"].as_array().expect("property oracle") {
        let property_name = property["name"].as_str().expect("property name");
        let label = format!("{name}/{property_name}");
        let actual = numeric(&layer.content, property_name)
            .unwrap_or_else(|| panic!("{label}: native property missing"));
        if let Some(values) = property["value"].as_array() {
            assert!(!actual.animated, "{label}: unexpected native keys");
            check_values(&actual.values, values, &label);
        }
        if let Some(keys) = property["keys"].as_array() {
            assert!(actual.animated, "{label}: missing native animation");
            assert_eq!(actual.keyframes.len(), keys.len(), "{label}: key count");
            for (index, (native_key, oracle)) in actual.keyframes.iter().zip(keys).enumerate() {
                let oracle = oracle.as_array().expect("manual key [time, values...]");
                close(
                    native_key.time_secs,
                    oracle[0].as_f64().unwrap(),
                    &format!("{label} key {index} time"),
                );
                check_values(
                    &native_key.values,
                    &oracle[1..],
                    &format!("{label} key {index}"),
                );
                assert_eq!(
                    (native_key.in_interpolation, native_key.out_interpolation),
                    (1, 1),
                    "{label}: linear interpolation"
                );
            }
        }
    }
}

macro_rules! panel_case {
    ($test:ident, $name:literal, $input:expr, $expected:expr) => {
        #[test]
        fn $test() {
            crate::adobe_test_support::export_case($name, || {
                check_case($name, $input, $expected);
            });
        }
    };
}

panel_case!(
    vector_rect_size,
    "vector-rect-size",
    include_str!("../../../tests/fixtures/vectors/fx_export_panel/vector-rect-size.fx.json"),
    include_str!("../../../tests/fixtures/vectors/fx_export_panel/vector-rect-size.expected.json")
);
