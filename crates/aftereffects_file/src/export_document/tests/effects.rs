//! Explicit edited FX -> fresh native AEP checks. Own-reader structural proof only;
//! Adobe-open and independently rendered export comparisons remain unmeasured.
use super::*;
use crate::effects::native::{DecodedEffect, read_effects};
use std::collections::BTreeMap;

fn effect_input(effects: Value) -> Value {
    let mut document = imported();
    let mut leaf = rect(&document, 900);
    leaf["rect"]["size"] = json!([120.0, 80.0]);
    leaf["effects"] = effects;
    document["composition"]["layers"] = json!([leaf]);
    document
}

fn fresh_effects(output: &ExportedDocument) -> Vec<DecodedEffect> {
    let project = read_project(&output.bytes).expect("fresh native AEP parse");
    let layer = layers(&project).first().expect("retained edited rectangle");
    let (effects, warnings) = read_effects(&layer.content, [120.0, 80.0]);
    assert!(
        warnings.iter().all(|warning| !warning.contains("missing")),
        "{warnings:?}"
    );
    effects
}

fn value(effect: &DecodedEffect, name: &str) -> Vec<f64> {
    effect
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == name)
        .unwrap_or_else(|| panic!("{name} missing from {}", effect.match_name))
        .numeric
        .as_ref()
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .values
        .clone()
}

#[test]
fn edited_static_effects_are_freshly_authored_in_order_with_native_units() {
    let input = effect_input(json!([
        {"id":9001,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":37,"repeatEdgePixels":true}},
        {"id":9002,"enabled":false,"effect":{"type":"glow","glowThreshold":60,"glowRadius":17,"glowIntensity":2}},
        {"id":9003,"enabled":true,"effect":{"type":"cornerPin","upperLeftX":0,"upperLeftY":0,"upperRightX":1,"upperRightY":0,"lowerLeftX":0,"lowerLeftY":1,"lowerRightX":1,"lowerRightY":1}},
        {"id":9004,"enabled":true,"effect":{"type":"levels","inputBlack":0,"inputWhite":255,"gamma":1,"outputBlack":0,"outputWhite":255}}
    ]));
    let output = export(input);
    let effects = fresh_effects(&output);
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        [
            "ADBE Gaussian Blur 2",
            "ADBE Glo2",
            "ADBE Corner Pin",
            "ADBE Pro Levels2"
        ]
    );
    assert_eq!(value(&effects[0], "ADBE Gaussian Blur 2-0001"), [37.0]);
    assert_eq!(value(&effects[0], "ADBE Gaussian Blur 2-0003"), [1.0]);
    assert!(!effects[1].enabled, "disabled effect occurrence persists");
    assert!((value(&effects[1], "ADBE Glo2-0002")[0] - 153.0).abs() < 0.01);
    assert_eq!(value(&effects[1], "ADBE Glo2-0006"), [2.0]);
    assert_eq!(value(&effects[2], "ADBE Corner Pin-0002"), [120.0, 0.0]);
    assert_eq!(value(&effects[3], "ADBE Pro Levels2-0005"), [1.0]);
}

#[test]
fn glow_export_uses_normal_without_changing_keyed_controls_or_native_source() {
    // The independent Adobe-authored static source uses Add (3); the FX
    // approximation deliberately writes Normal (2) only on fresh export.
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/effects_coverage/native_static_controls.aep"
    ))
    .expect("pinned native Effects source");
    let ItemKind::Composition(composition) = &source.item(183).expect("glow target 183").kind
    else {
        panic!("glow target must be a composition");
    };
    let native_glow = composition
        .layers
        .iter()
        .flat_map(|layer| crate::effects::native::read_effects(&layer.content, [320.0, 180.0]).0)
        .find(|effect| effect.match_name == "ADBE Glo2")
        .expect("independently authored Glow occurrence");
    assert_eq!(value(&native_glow, "ADBE Glo2-0006"), [3.0]);

    let mut input = effect_input(json!([
        {"id":9001,"enabled":true,"effect":{"type":"glow","glowThreshold":60,"glowRadius":17,"glowIntensity":2}}
    ]));
    let mut threshold = serde_json::to_value(keyed_entry(
        LayerId::new(900),
        PropType::Rotation,
        [
            (0, PropertyValue::Float(40.0)),
            (1_000, PropertyValue::Float(80.0)),
        ],
    ))
    .expect("FX animator JSON");
    threshold["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(9001),
        "glowThreshold",
    ))
    .expect("effect target JSON");
    input["composition"]["dynamics"]["entries"] = json!([threshold]);
    let output = export(input);
    let effects = fresh_effects(&output);
    let glow = &effects[0];
    assert!(glow.enabled);
    assert_eq!(value(glow, "ADBE Glo2-0006"), [2.0]);
    assert_eq!(value(glow, "ADBE Glo2-0003"), [17.0]);
    assert_eq!(value(glow, "ADBE Glo2-0004"), [2.0]);
    let threshold = glow
        .parameters
        .iter()
        .find(|property| property.match_name == "ADBE Glo2-0002")
        .expect("editable threshold");
    let keys = &threshold
        .numeric
        .as_ref()
        .expect("numeric threshold")
        .keyframes;
    assert_eq!(keys.len(), 2);
    assert_eq!(
        keys.iter().map(|key| key.values[0]).collect::<Vec<_>>(),
        [102.0, 204.0]
    );
    assert!((keys[1].time_secs - 1.0).abs() < 0.002);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("Glow Operation")
            && diagnostic.message.contains("Normal")
            && diagnostic.message.contains("halo")
    }));
}

#[test]
fn unsupported_effect_does_not_discard_owner_or_supported_siblings() {
    let input = effect_input(json!([
        {"id":9001,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":43}},
        {"id":9002,"enabled":true,"effect":{"type":"futureAdobePlugin","mystery":12}},
        {"id":9003,"enabled":true,"effect":{"type":"posterize","levels":8}}
    ]));
    let output = export(input);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("omitted")),
        "{:?}",
        output.diagnostics
    );
    let effects = fresh_effects(&output);
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        ["ADBE Gaussian Blur 2", "ADBE Posterize"]
    );
    assert_eq!(value(&effects[0], "ADBE Gaussian Blur 2-0001"), [43.0]);
    assert_eq!(value(&effects[1], "ADBE Posterize-0001"), [8.0]);
}

#[test]
fn shadow_payload_canonicalizes_to_disabled_native_layer_style() {
    let mut payload = crate::effects::mapping::default_effect("dropShadow");
    payload["enabled"] = json!(false);
    payload["blendMode"] = json!("multiply");
    let output = export(effect_input(json!([
        {"id":9001,"enabled":true,"effect":payload}
    ])));
    assert!(fresh_effects(&output).is_empty());
    let project = read_project(&output.bytes).unwrap();
    let shadow = layers(&project)
        .iter()
        .find_map(|layer| {
            crate::layer_styles::read(&layer.content, [320.0, 180.0])
                .styles
                .into_iter()
                .find_map(|style| match style {
                    crate::layer_styles::NativeLayerStyle::DropShadow(shadow) => Some(shadow),
                    _ => None,
                })
        })
        .expect("native Drop Shadow Layer Style");
    assert!(!shadow.enabled);
    assert_eq!(shadow.blend_mode, fx_schema::BlendMode::Multiply);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("canonicalized to AE's native Layer Style")
    }));
}

#[test]
fn edited_animated_scalar_exports_authored_keys_not_frame_samples() {
    let mut input = effect_input(json!([
        {"id":9001,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":25}}
    ]));
    let entry = keyed_entry(
        LayerId::new(900),
        PropType::Rotation,
        [
            (0, PropertyValue::Float(25.0)),
            (1_000, PropertyValue::Float(30.0)),
        ],
    );
    let mut entry = serde_json::to_value(entry).expect("FX animator JSON");
    entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(9001),
        "blurriness",
    ))
    .expect("effect target JSON");
    input["composition"]["dynamics"]["entries"] = json!([entry]);
    let output = export(input);
    let effects = fresh_effects(&output);
    let blur = effects.first().expect("edited blur");
    let numeric = blur
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == "ADBE Gaussian Blur 2-0001")
        .expect("blur control")
        .numeric
        .as_ref()
        .expect("numeric blur keys");
    assert_eq!(numeric.keyframes.len(), 2);
    assert_eq!(
        numeric
            .keyframes
            .iter()
            .map(|key| key.values[0])
            .collect::<Vec<_>>(),
        [25.0, 30.0]
    );
    assert!((numeric.keyframes[1].time_secs - 1.0).abs() < 0.002);
}

#[test]
fn group_effect_points_use_generated_source_dimensions_not_outer_canvas() {
    let mut input = effect_input(json!([]));
    input["dimensions"] = json!({"width":320,"height":180});
    input["duration"] = json!(2.0);
    let mut leaf = input["composition"]["layers"][0].take();
    let mut transform = leaf["transform"].clone();
    transform["anchorPoint"] = json!([0, 0]);
    transform["position"] = json!([0, 0]);
    transform["scale"] = json!([100, 100]);
    transform["rotation"] = json!(0);
    transform["opacity"] = json!(100);
    leaf["transform"] = transform.clone();
    leaf["activeRange"] = json!({"start":0,"duration":2000});
    input["composition"]["layers"] = json!([{
        "type":"Group","id":800,"name":"Effect owner","parent":null,
        "transform":transform,"playback":fixture_linear_playback(json!({"start":0,"duration":2000}), json!({"start":0,"duration":2000})),"layers":[leaf],
        "effects":[{"id":9001,"enabled":true,"effect":{"type":"radialBlur","amount":12,"centerX":0.25,"centerY":0.75}}]
    }]);
    let output = export(input);
    let native = read_project(&output.bytes).unwrap();
    let owner = &layers(&native)[0];
    let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind else {
        panic!("effect owner must reference a bounded precomposition");
    };
    assert!(source.width < 320 && source.height < 180);
    let size = [f64::from(source.width), f64::from(source.height)];
    let (effects, _) = read_effects(&owner.content, size);
    assert_eq!(
        value(&effects[0], "ADBE Radial Blur-0002"),
        [size[0] * 0.25, size[1] * 0.75]
    );
}

#[test]
fn incompatible_color_easing_retains_base_owner_and_effect_sibling() {
    let mut input = effect_input(json!([
        {"id":9001,"enabled":true,"effect":{"type":"gradientRamp","startR":0.2,"startG":0.4,"startB":0.1}},
        {"id":9002,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":4}}
    ]));
    let mut entry = serde_json::to_value(keyed_entry(
        LayerId::new(900),
        PropType::Rotation,
        [
            (0, PropertyValue::Float(0.2)),
            (1000, PropertyValue::Float(0.8)),
        ],
    ))
    .unwrap();
    entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(9001),
        "startR",
    ))
    .unwrap();
    entry["animator"]["keyframes"][1]["easing"] =
        serde_json::to_value(PropertyKeyframeEasing::CubicBezier {
            x1: 0.25,
            y1: 0.0,
            x2: 0.75,
            y2: 1.0,
        })
        .unwrap();
    input["composition"]["dynamics"]["entries"] = json!([entry]);
    let output = export(input);
    let effects = fresh_effects(&output);
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[1].match_name, "ADBE Gaussian Blur 2");
    assert_eq!(value(&effects[0], "ADBE Ramp-0002"), [0.2, 0.4, 0.1, 1.0]);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("cubic color animation omitted"))
    );
}

#[test]
fn grain_export_normalizes_unmapped_plugin_state_on_source_owner_canvas() {
    fn grain_values(project: &StructuralProject) -> ([u16; 2], BTreeMap<String, Vec<f64>>) {
        for item in &project.items {
            let ItemKind::Composition(composition) = &item.kind else {
                continue;
            };
            for layer in &composition.layers {
                let (effects, _) = crate::effects::native::read_effects(
                    &layer.content,
                    [f64::from(composition.width), f64::from(composition.height)],
                );
                if let Some(effect) = effects
                    .iter()
                    .find(|effect| effect.match_name == "VISINF Grain Implant")
                {
                    let values = effect
                        .parameters
                        .iter()
                        .filter_map(|property| {
                            property.numeric.as_ref().ok().map(|numeric| {
                                (property.match_name.clone(), numeric.values.clone())
                            })
                        })
                        .collect();
                    return ([composition.width, composition.height], values);
                }
            }
        }
        panic!("Grain effect")
    }

    for (source_bytes, composition_id) in [
        (
            &include_bytes!("../../../tests/fixtures/effects_coverage/native_static_controls.aep")
                [..],
            235,
        ),
        (
            &include_bytes!(
                "../../../tests/fixtures/effects_coverage/native_animated_controls.aep"
            )[..],
            222,
        ),
    ] {
        let source = read_project(source_bytes).expect("pinned Adobe-authored Effects source");
        let (source_owner, source_values) = grain_values(&source);
        assert_eq!(source_owner, [320, 180], "native Grain owner canvas");
        assert_eq!(source_values["VISINF Grain Implant-0021"], [0.0]);
        assert_eq!(source_values["VISINF Grain Implant-0028"], [1.0]);

        let imported =
            to_structural_fx_document(&source, Some(composition_id)).expect("fresh Grain import");
        let output = to_aep(&imported.document).expect("fresh Grain export");
        let fresh = read_project(&output.bytes).expect("fresh native AEP parse");
        let (fresh_owner, fresh_values) = grain_values(&fresh);
        assert_eq!(fresh_owner, [320, 180], "fresh Grain owner canvas");

        for (name, fresh_value) in &fresh_values {
            let source_value = source_values
                .get(name)
                .unwrap_or_else(|| panic!("{name}: missing from pinned native Grain source"));
            assert_eq!(
                fresh_value, source_value,
                "{name}: preserve mapped controls and normalize omitted plugin state"
            );
        }
    }
}

#[test]
fn explicit_probe_archive_is_only_emitted_when_requested() {
    let Ok(directory) = std::env::var("AEP_EFFECTS_PROBE_DIR") else {
        return;
    };
    let mut input = effect_input(json!([
        {"id":9001,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":25}}
    ]));
    input["dimensions"] = json!({"width":320,"height":180});
    input["duration"] = json!(2.0);
    input["backgroundColor"] = json!([0.0, 0.0, 0.0, 0.0]);
    input["composition"]["layers"][0]["rect"]["fillColor"] = json!([0.9, 0.2, 0.1, 1.0]);
    input["composition"]["layers"][0]["transform"]["anchorPoint"] = json!([60.0, 40.0]);
    input["composition"]["layers"][0]["transform"]["position"] = json!([160.0, 90.0]);
    input["composition"]["name"] = json!("EditedEffect");
    input["composition"]["dynamics"]["entries"] = json!([]);
    let path = std::path::Path::new(&directory);
    std::fs::create_dir_all(path).expect("probe output directory");
    for animated in [false, true] {
        let name = if animated {
            "edited-gaussian-animated"
        } else {
            "edited-gaussian-static"
        };
        if animated {
            let entry = keyed_entry(
                LayerId::new(900),
                PropType::Rotation,
                [
                    (0, PropertyValue::Float(25.0)),
                    (1_000, PropertyValue::Float(30.0)),
                ],
            );
            let mut entry = serde_json::to_value(entry).unwrap();
            entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
                fx_schema::EffectId::new(9001),
                "blurriness",
            ))
            .unwrap();
            input["composition"]["dynamics"]["entries"] = json!([entry]);
        }
        std::fs::write(
            path.join(format!("{name}.fx.json")),
            serde_json::to_vec_pretty(&input).unwrap(),
        )
        .unwrap();
        let output = export(input.clone());
        std::fs::write(path.join(format!("{name}.aep")), output.bytes).unwrap();
        std::fs::write(
            path.join(format!("{name}.diagnostics.txt")),
            format!("{:?}", output.diagnostics),
        )
        .unwrap();
    }
}
