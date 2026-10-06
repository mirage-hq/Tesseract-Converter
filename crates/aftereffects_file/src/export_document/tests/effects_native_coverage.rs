//! Explicit nondefault inputs/oracles, not values generated from the mapping table.
//! Own-reader assertions are supplementary; Adobe execution is recorded separately.
use super::*;
use std::collections::BTreeSet;

const TEMPLATE: &str = include_str!("../../../tests/fixtures/effects_coverage/template.fx.json");
const CASES: &str = include_str!("../../../tests/fixtures/effects_coverage/cases.json");

#[test]
fn aligned_color_cubic_full_export_retains_keys_and_endpoint_edit() {
    for (input_json, red) in [
        (
            include_str!(
                "../../../tests/fixtures/effects_coverage/aligned_color_cubic_original.fx.json"
            ),
            0.8,
        ),
        (
            include_str!(
                "../../../tests/fixtures/effects_coverage/aligned_color_cubic_edited.fx.json"
            ),
            0.9,
        ),
    ] {
        let input: Value = serde_json::from_str(input_json).unwrap();
        let oracle = json!({"effect":"ADBE Tint", "enabled":true, "controls":[
            {"name":"ADBE Tint-0001", "keys":[[0,0.1,0.2,0.3,0],[1,red,0.7,0.6,0]], "segments":["cubic"]},
            {"name":"ADBE Tint-0002", "value":[0.52,1,0.7,0]},
            {"name":"ADBE Tint-0003", "value":[73]}
        ]});
        super::effects_native_panel::check_case(
            input["composition"]["name"].as_str().unwrap(),
            input_json,
            &oracle.to_string(),
        );
    }
}

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(CASES).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

fn input_and_oracle(case: &Value, animated: bool) -> (Value, Value) {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    let name = format!(
        "{}-{}",
        case["id"].as_str().unwrap(),
        if animated { "animated" } else { "static" }
    );
    input["composition"]["name"] = json!(name);
    input["composition"]["layers"][0]["effects"] = json!([{
        "id": 9001, "enabled": true, "effect": case["effect"]
    }]);
    if animated {
        let tracks = case["tracks"].as_array().unwrap();
        assert!(
            !tracks.is_empty(),
            "static-only case has no animation proof"
        );
        input["composition"]["dynamics"]["entries"] = json!(tracks.iter().map(|track| {
            let param = track["param"].as_str().unwrap();
            json!({
                "target": {"kind":"effectProperty", "effectId":9001, "paramName":param},
                "animator": {"type":"keyframes", "enabled":true, "keyframes":[
                    {"id":format!("{param}-start"), "layerTime":0,
                     "value":{"type":"float", "value":track["values"][0]}, "easing":{"type":"linear"}},
                    {"id":format!("{param}-end"), "layerTime":1000,
                     "value":{"type":"float", "value":track["values"][1]}, "easing":{"type":track["easing"]}}
                ]}, "dependencies":[], "layerRefs":{}
            })
        }).collect::<Vec<_>>());
    }
    if let Some(keys) = case["rect_size_keys"].as_array() {
        let values: [(i64, PropertyValue); 2] = std::array::from_fn(|index| {
            (
                keys[index][0].as_i64().unwrap() * 1000,
                PropertyValue::Vector2([
                    keys[index][1].as_f64().unwrap(),
                    keys[index][2].as_f64().unwrap(),
                ]),
            )
        });
        input["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(
                serde_json::to_value(keyed_entry(LayerId::new(900), PropType::RectSize, values))
                    .unwrap(),
            );
    }
    let controls: Vec<_> = case["controls"].as_array().unwrap().iter().map(|control| {
        if animated && let Some(end) = control.get("end") {
            let start = control["value"].as_array().unwrap();
            let end = end.as_array().unwrap();
            let mut first = vec![json!(0)]; first.extend_from_slice(start);
            let mut last = vec![json!(1)]; last.extend_from_slice(end);
            let middle: Vec<_> = start.iter().zip(end).map(|(a,b)| {
                if control["segment"] == "hold" { a.clone() }
                else { json!((a.as_f64().unwrap()+b.as_f64().unwrap())/2.0) }
            }).collect();
            let mut midpoint = vec![json!(0.5)]; midpoint.extend(middle);
            json!({"name":control["name"], "keys":[first,last],
                   "segments":[control["segment"]], "valueAtTime":control.get("samples").cloned().unwrap_or_else(|| json!([midpoint]))})
        } else { json!({"name":control["name"], "value":control["value"]}) }
    }).collect();
    (
        input,
        json!({"effect":case["native_effect"], "enabled":true, "controls":controls}),
    )
}

fn check_case(kind: &str, animated: bool) {
    let suffix = if animated { "animated" } else { "static" };
    crate::adobe_test_support::export_case(&format!("{kind}-{suffix}"), || {
        check_case_inner(kind, animated);
    });
}

fn check_case_inner(kind: &str, animated: bool) {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == kind)
        .expect("explicit coverage case");
    let (input, oracle) = input_and_oracle(&case, animated);
    let name = input["composition"]["name"].as_str().unwrap();
    let input_json = serde_json::to_string_pretty(&input).unwrap() + "\n";
    let oracle_json = serde_json::to_string_pretty(&oracle).unwrap() + "\n";
    // Scratch artifacts never belong in the committed fixture directory. The
    // .tsrct is built from exactly this explicit FX input, not a re-imported AEP.
    {
        let directory = crate::adobe_test_support::artifact_directory();
        let path = std::path::Path::new(&directory).join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(path.with_extension("fx.json"), &input_json).unwrap();
        std::fs::write(path.with_extension("expected.json"), &oracle_json).unwrap();
        let document = EditableFxCompositionDocument::from_json_value(input.clone()).unwrap();
        let output = to_aep(&document).unwrap();
        std::fs::write(path.with_extension("aep"), &output.bytes).unwrap();
        tesseract_file::TesseractFileBuilder::try_new(document)
            .unwrap()
            .write(path.with_extension("tsrct"))
            .unwrap();
    }
    if kind == "dropShadow" {
        check_drop_shadow_layer_style(&input);
    } else if kind == "hueSaturation" && animated {
        check_hue_static_only_controls(&input);
        // Keep the original requested-control oracle unchanged on disk. It is
        // not an Adobe pass: Master/toggle animation is explicitly unsupported.
        // The remaining Colorize numeric controls must still retain their keys.
        let mut colorize_oracle = oracle;
        colorize_oracle["controls"]
            .as_array_mut()
            .unwrap()
            .retain(|control| {
                matches!(
                    control["name"].as_str(),
                    Some(
                        "ADBE HUE SATURATION-0008"
                            | "ADBE HUE SATURATION-0009"
                            | "ADBE HUE SATURATION-0010"
                    )
                )
            });
        super::effects_native_panel::check_case(name, &input_json, &colorize_oracle.to_string());
    } else {
        super::effects_native_panel::check_case(name, &input_json, &oracle_json);
    }
}

fn check_drop_shadow_layer_style(input: &Value) {
    let output = export(input.clone());
    let native = read_project(&output.bytes).expect("fresh Drop Shadow export");
    let [layer] = layers(&native) else {
        panic!("Drop Shadow owner must survive canonicalization");
    };
    let (effects, effect_warnings) =
        crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
    assert!(
        effects.is_empty(),
        "shared Drop Shadow payload must not also emit an Effect Parade plugin: {effects:?}; {effect_warnings:?}"
    );

    let decoded = crate::layer_styles::read(&layer.content, [320.0, 180.0]);
    let [crate::layer_styles::NativeLayerStyle::DropShadow(shadow)] = decoded.styles.as_slice()
    else {
        panic!(
            "expected one native Drop Shadow Layer Style: {:?}; {:?}",
            decoded.styles, output.diagnostics
        );
    };
    assert!(shadow.enabled);
    for (index, (actual, expected)) in shadow.color.iter().zip([0.2, 0.4, 0.6, 0.5]).enumerate() {
        assert!(
            (actual - expected).abs() < 1e-6,
            "Drop Shadow color[{index}]: {actual} != {expected}"
        );
    }
    for (index, (actual, expected)) in shadow.offset.iter().zip([6.0, -8.0]).enumerate() {
        assert!(
            (actual - expected).abs() < 1e-6,
            "Drop Shadow offset[{index}]: {actual} != {expected}"
        );
    }
    assert!((shadow.size - 4.0).abs() < 1e-6, "Drop Shadow blur");
    assert!(shadow.spread.abs() < 1e-6, "Drop Shadow spread");
    assert_eq!(shadow.blend_mode, fx_schema::BlendMode::Normal);
    assert!(shadow.animations.is_empty(), "static Drop Shadow keys");
    assert!(
        output.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("canonicalized to AE's native Layer Style")
                && diagnostic.message.contains("Shadow Only")
                && diagnostic.message.contains("spread is retained")
        }),
        "missing Drop Shadow canonicalization diagnostic: {:?}",
        output.diagnostics
    );
}

#[test]
fn chromatic_candidate_failure_keeps_editable_owner_and_diagnoses_omission() {
    // An explicit FX source exercises the nine-occurrence show's built-in
    // effect shape; the separate Adobe chart in color-worker/assessment.md
    // demonstrates why the channel-assembly candidate is not a mapping.
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/effects_coverage/hue_master_static.fx.json"
    ))
    .unwrap();
    input["composition"]["layers"][0]["effects"][0]["effect"] =
        json!({"type":"chromaticAberration", "amount":0.3, "direction":90});
    let output = export(input);
    let native = read_project(&output.bytes).unwrap();
    let [layer] = layers(&native) else {
        panic!("Chromatic omission must not discard its owner");
    };
    let (effects, _) = crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
    assert!(
        effects.is_empty(),
        "unproved candidate must not emit an AE plugin"
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Chromatic channel/alpha assembly failed native RGBA checks")
    }));
}

#[test]
fn hue_master_transfer_and_animation_are_diagnosed_from_explicit_fx_sources() {
    // The Adobe-authored native Hue chart (AE 26.5x89, composite probe v3,
    // composition 1) renders a changed maximum channel for Master Saturation
    // +25; the FX HSV implementation keeps that channel fixed. This CPU test
    // checks honest export diagnostics, not native color or alpha fidelity.
    let static_input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/effects_coverage/hue_master_static.fx.json"
    ))
    .unwrap();
    let static_output = export(static_input);
    assert!(static_output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("FX HSV Master transfer differs from Adobe's native Hue/Saturation")
    }));

    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "hueSaturation")
        .unwrap();
    let (animated_input, _) = input_and_oracle(&case, true);
    let animated_output = export(animated_input);
    for param in ["hue", "saturation", "lightness", "colorize"] {
        assert!(animated_output.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains(&format!("Effect hueSaturation / {param}:"))
                && diagnostic.message.contains("Channel Range")
                && diagnostic.message.contains("animation omitted")
        }));
    }
}

fn check_hue_static_only_controls(input: &Value) {
    let output = export(input.clone());
    let native = read_project(&output.bytes).unwrap();
    let [layer] = layers(&native) else {
        panic!("Hue owner must survive unsupported animation");
    };
    let (effects, _) = crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
    let [effect] = effects.as_slice() else {
        panic!("Hue effect must survive unsupported animation");
    };
    for (suffix, param, base) in [
        ("0004", "hue", 15.0),
        ("0005", "saturation", 20.0),
        ("0006", "lightness", -10.0),
        ("0007", "colorize", 1.0),
    ] {
        let name = format!("ADBE HUE SATURATION-{suffix}");
        let control = effect
            .parameters
            .iter()
            .find(|control| control.match_name == name)
            .unwrap();
        let numeric = control.numeric.as_ref().unwrap();
        assert!(!numeric.animated, "Adobe ignores ordinary keys for {param}");
        assert_eq!(
            numeric.values,
            vec![base],
            "retain authored base for {param}"
        );
        assert!(
            output.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains(&format!("Effect hueSaturation / {param}:"))
                && diagnostic.message.contains("animation omitted")),
            "missing contextual animation diagnostic for {param}: {:?}",
            output.diagnostics
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn explicit_hue_master_static_exports_editable_aep() {
    // Adobe-authored source/independent render: hue_master_static_adobe.aep,
    // composition ID 1. This is a separately authored edited FX input, not an
    // import-then-export round trip. The writer regression checks its parT
    // encoding; own-reader assertions below are only supplementary.
    const INPUT: &str =
        include_str!("../../../tests/fixtures/effects_coverage/hue_master_static.fx.json");
    crate::adobe_test_support::export_case("hueMasterStatic", || {
        let input: Value = serde_json::from_str(INPUT).unwrap();
        let document = EditableFxCompositionDocument::from_json_value(input).unwrap();
        let output = to_aep(&document).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let [layer] = layers(&native) else {
            panic!("fresh output must retain one editable owner");
        };
        let (effects, warnings) =
            crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
        let [effect] = effects.as_slice() else {
            panic!("fresh output must retain one editable effect: {warnings:?}");
        };
        assert_eq!(effect.match_name, "ADBE HUE SATURATION");
        for (suffix, expected) in [("0004", 50.0), ("0005", -60.0), ("0006", 20.0)] {
            let name = format!("ADBE HUE SATURATION-{suffix}");
            let control = effect
                .parameters
                .iter()
                .find(|control| control.match_name == name)
                .unwrap();
            let numeric = control.numeric.as_ref().unwrap();
            assert_eq!(numeric.values, vec![expected]);
            assert!(!numeric.animated, "Master-only source does not key {name}");
        }
        assert!(!INPUT.contains("JsScript"));
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn hue_master_out_of_range_retains_convertible_siblings() {
    // Supplemental boundary test, not an independently authored Adobe oracle.
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/effects_coverage/hue_master_static.fx.json"
    ))
    .unwrap();
    input["composition"]["layers"][0]["effects"][0]["effect"]["hue"] = json!(40_000.0);
    let output = export(input);
    assert!(output.diagnostics.iter().any(|diagnostic| diagnostic.message.contains("Effect hueSaturation / hue: value exceeds native 16:16 range; native default retained")));
    let native = read_project(&output.bytes).unwrap();
    let [layer] = layers(&native) else {
        panic!("owner must survive out-of-range control");
    };
    let (effects, _) = crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
    let [effect] = effects.as_slice() else {
        panic!("effect siblings must survive out-of-range control");
    };
    for (suffix, value) in [("0004", 0.0), ("0005", -60.0), ("0006", 20.0)] {
        let name = format!("ADBE HUE SATURATION-{suffix}");
        let control = effect
            .parameters
            .iter()
            .find(|control| control.match_name == name)
            .unwrap();
        assert_eq!(control.numeric.as_ref().unwrap().values, vec![value]);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn hue_master_fractional_values_have_diagnosed_integer_state() {
    // Supplemental boundary assertion; fractional Adobe fidelity is unproved.
    let mut input: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/effects_coverage/hue_master_static.fx.json"
    ))
    .unwrap();
    let effect = &mut input["composition"]["layers"][0]["effects"][0]["effect"];
    effect["hue"] = json!(50.25);
    effect["saturation"] = json!(-60.5);
    effect["lightness"] = json!(20.75);
    let output = export(input);
    for param in ["hue", "saturation", "lightness"] {
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains(&format!(
                    "Effect hueSaturation / {param}: fractional Master value rounded"
                )))
        );
    }
    let native = read_project(&output.bytes).unwrap();
    let [layer] = layers(&native) else {
        panic!("owner must survive rounding");
    };
    let (effects, _) = crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
    let [effect] = effects.as_slice() else {
        panic!("effect must survive rounding");
    };
    for (suffix, value) in [("0004", 50.0), ("0005", -61.0), ("0006", 21.0)] {
        let name = format!("ADBE HUE SATURATION-{suffix}");
        let control = effect
            .parameters
            .iter()
            .find(|control| control.match_name == name)
            .unwrap();
        assert_eq!(control.numeric.as_ref().unwrap().values, vec![value]);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn every_mapped_type_field_and_animatable_target_has_an_explicit_case() {
    let cases = cases();
    let expected_types: BTreeSet<_> = crate::effects::mapping::mappings()
        .iter()
        .map(|m| m.fx_type)
        .collect();
    let actual_types: BTreeSet<_> = cases.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(
        actual_types, expected_types,
        "mapping additions require explicit test inputs/oracles"
    );
    assert_eq!(actual_types.len(), cases.len(), "duplicate coverage cases");
    for case in &cases {
        let kind = case["id"].as_str().unwrap();
        let mapping = crate::effects::mapping::by_fx(kind).unwrap();
        assert_eq!(
            mapping.native, case["native_effect"],
            "{kind}: native plugin selection"
        );
        for field in mapping.fields {
            assert!(
                case["effect"].get(field.field).is_some(),
                "{kind}: missing {} input",
                field.field
            );
            assert!(
                case["controls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["name"] == field.native),
                "{kind}: missing {} native oracle",
                field.native
            );
            if field.animated {
                let panel_has_track = case["tracks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["param"] == field.param);
                // The published 55-case panel is immutable evidence. The later
                // source-backed Softness fix has its exact 7→12 native-key
                // oracle in `effects_edge_coverage` instead of rewriting that
                // panel's explicit FX input and invalidating its hashes.
                let supplemental_case = kind == "dropShadow" && field.param == "blurRadius";
                assert!(
                    panel_has_track || supplemental_case,
                    "{kind}: missing {} animation",
                    field.param
                );
            }
        }
    }
}

macro_rules! coverage_case {
    ($static_name:ident, $animated_name:ident, $kind:literal) => {
        #[test]
        #[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
        fn $static_name() {
            check_case($kind, false);
        }
        #[test]
        #[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
        fn $animated_name() {
            check_case($kind, true);
        }
    };
    ($static_name:ident, $kind:literal) => {
        #[test]
        #[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
        fn $static_name() {
            check_case($kind, false);
        }
    };
}

coverage_case!(gaussian_static, gaussian_animated, "gaussianBlur");
coverage_case!(glow_static, glow_animated, "glow");
coverage_case!(directional_static, directional_animated, "directionalBlur");
coverage_case!(
    pixel_motion_static,
    pixel_motion_animated,
    "pixelMotionBlur"
);
coverage_case!(mosaic_static, mosaic_animated, "mosaic");
coverage_case!(shift_channels_static, "shiftChannels");
coverage_case!(drop_shadow_static, "dropShadow");
coverage_case!(brightness_static, brightness_animated, "brightnessContrast");
coverage_case!(
    hue_saturation_static,
    hue_saturation_animated,
    "hueSaturation"
);
coverage_case!(radial_static, radial_animated, "radialBlur");
coverage_case!(levels_static, levels_animated, "levels");
coverage_case!(bulge_static, bulge_animated, "bulge");
coverage_case!(corner_pin_static, corner_pin_animated, "cornerPin");
coverage_case!(motion_tile_static, motion_tile_animated, "motionTile");
coverage_case!(posterize_static, posterize_animated, "posterize");
coverage_case!(posterize_time_static, "posterizeTime");
coverage_case!(vignette_static, vignette_animated, "vignette");
coverage_case!(find_edges_static, find_edges_animated, "findEdges");
coverage_case!(exposure_static, exposure_animated, "exposure");
coverage_case!(vibrance_static, vibrance_animated, "vibrance");
coverage_case!(twirl_static, twirl_animated, "twirl");
coverage_case!(ripple_static, ripple_animated, "ripple");
coverage_case!(sharpen_static, sharpen_animated, "sharpen");
coverage_case!(luma_key_static, luma_key_animated, "lumaKey");
coverage_case!(choker_static, choker_animated, "simpleChoker");
coverage_case!(grain_static, grain_animated, "grain");
coverage_case!(wave_warp_static, wave_warp_animated, "waveWarp");
coverage_case!(tint_static, tint_animated, "tintTritone");
coverage_case!(ramp_static, ramp_animated, "gradientRamp");
coverage_case!(noise_static, noise_animated, "turbulentNoise");

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn vignette_export_diagnoses_native_replacements() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "vignette")
        .expect("explicit Vignette coverage case");
    let (input, _) = input_and_oracle(&case, false);
    let document = EditableFxCompositionDocument::from_json_value(input).unwrap();
    let output = to_aep(&document).unwrap();
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("feather")
            && diagnostic.message.contains("Center")
            && diagnostic.message.contains("Pin Highlights")
    }));
}
