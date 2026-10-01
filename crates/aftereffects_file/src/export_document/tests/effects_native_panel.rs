//! Explicit edited-FX inputs and independently specified native-control expectations.
//! This is own-reader structural evidence, not Adobe acceptance or pixel fidelity.
use super::*;
use crate::effects::native::read_effects;

fn close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < 0.002,
        "{label}: expected {expected}, got {actual}"
    );
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

pub(super) fn check_case(name: &str, input_json: &str, expected_json: &str) {
    // The committed input and oracle are also consumed by the independent Adobe
    // inspection workflow. Reading our own native output here is not independent
    // acceptance or valueAtTime proof.
    let input: Value = serde_json::from_str(input_json).expect("committed FX panel input");
    let expected: Value =
        serde_json::from_str(expected_json).expect("committed Adobe readback oracle");
    assert_eq!(input["composition"]["name"], name, "fixture case name");
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
            .expect("independent Adobe readback oracle");
    }
    let native = read_project(&output.bytes).expect("fresh native AEP");
    let [layer] = layers(&native) else {
        panic!(
            "{name}: expected one editable owner: {:?}",
            output.diagnostics
        );
    };
    // Shape Point keys use the containing composition as their native source
    // space; FX effect controls were independently lowered from the 120×80 rect.
    let (effects, warnings) = read_effects(&layer.content, [320.0, 180.0]);
    // Some plugins expose unrelated, unsupported default-only controls. The
    // explicitly asserted controls must still decode without a warning.
    assert!(
        warnings.iter().all(|warning| expected["controls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|control| !warning.contains(control["name"].as_str().unwrap()))),
        "{name}: asserted native control could not be decoded: {warnings:?}"
    );
    let [effect] = effects.as_slice() else {
        panic!(
            "{name}: expected one editable effect: {effects:?}; {:?}",
            output.diagnostics
        );
    };
    assert_eq!(effect.match_name, expected["effect"], "{name}");
    assert_eq!(effect.enabled, expected["enabled"], "{name}");
    for control in expected["controls"]
        .as_array()
        .expect("manual control oracle")
    {
        let label = format!("{name}/{}", control["name"]);
        let property = effect
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == control["name"].as_str().unwrap())
            .unwrap_or_else(|| panic!("{label}: native control missing"));
        let numeric = property
            .numeric
            .as_ref()
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        if let Some(values) = control["value"].as_array() {
            assert!(!numeric.animated, "{label}: unexpected native keys");
            check_values(&numeric.values, values, &label);
        }
        if let Some(keys) = control["keys"].as_array() {
            assert!(numeric.animated, "{label}: missing native animation");
            assert_eq!(numeric.keyframes.len(), keys.len(), "{label}: key count");
            for (index, (native_key, oracle)) in numeric.keyframes.iter().zip(keys).enumerate() {
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
                if index > 0 {
                    let interpolation = match control["segments"][index - 1].as_str().unwrap() {
                        "linear" => 1,
                        "cubic" => 2,
                        "hold" => 3,
                        other => panic!("{label}: unknown manual interpolation {other}"),
                    };
                    assert_eq!(
                        numeric.keyframes[index - 1].out_interpolation,
                        interpolation,
                        "{label}: outgoing interpolation"
                    );
                    assert_eq!(
                        native_key.in_interpolation, interpolation,
                        "{label}: incoming interpolation"
                    );
                }
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
    scalar_cubic,
    "scalar-cubic",
    include_str!("../../../tests/fixtures/effects/fx_export_panel/scalar-cubic.fx.json"),
    include_str!("../../../tests/fixtures/effects/fx_export_panel/scalar-cubic.expected.json")
);
panel_case!(
    scalar_hold,
    "scalar-hold",
    include_str!("../../../tests/fixtures/effects/fx_export_panel/scalar-hold.fx.json"),
    include_str!("../../../tests/fixtures/effects/fx_export_panel/scalar-hold.expected.json")
);
panel_case!(
    radial_point_offset_knots,
    "radial-point-offset-knots",
    include_str!(
        "../../../tests/fixtures/effects/fx_export_panel/radial-point-offset-knots.fx.json"
    ),
    include_str!(
        "../../../tests/fixtures/effects/fx_export_panel/radial-point-offset-knots.expected.json"
    )
);
panel_case!(
    ramp_rgba_split_knots,
    "ramp-rgba-split-knots",
    include_str!("../../../tests/fixtures/effects/fx_export_panel/ramp-rgba-split-knots.fx.json"),
    include_str!(
        "../../../tests/fixtures/effects/fx_export_panel/ramp-rgba-split-knots.expected.json"
    )
);
panel_case!(
    disabled_glow,
    "disabled-glow",
    include_str!("../../../tests/fixtures/effects/fx_export_panel/disabled-glow.fx.json"),
    include_str!("../../../tests/fixtures/effects/fx_export_panel/disabled-glow.expected.json")
);
panel_case!(
    levels_normalized,
    "levels-normalized",
    include_str!("../../../tests/fixtures/effects/fx_export_panel/levels-normalized.fx.json"),
    include_str!("../../../tests/fixtures/effects/fx_export_panel/levels-normalized.expected.json")
);
panel_case!(
    twirl_center_and_angle,
    "twirl-center-and-angle",
    include_str!("../../../tests/fixtures/effects/fx_export_panel/twirl-center-and-angle.fx.json"),
    include_str!(
        "../../../tests/fixtures/effects/fx_export_panel/twirl-center-and-angle.expected.json"
    )
);
panel_case!(
    exposure_master,
    "exposure-master",
    include_str!("../../../tests/fixtures/effects/fx_export_panel/exposure-master.fx.json"),
    include_str!("../../../tests/fixtures/effects/fx_export_panel/exposure-master.expected.json")
);
