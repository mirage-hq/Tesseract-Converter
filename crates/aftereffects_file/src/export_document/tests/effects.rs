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
fn mosaic_unproven_leaf_retains_checkbox_counts_clocks_and_scales() {
    // The pinned Adobe source has Sharp Colors off. Import keeps that control;
    // the current FX renderer nevertheless always samples each block's center.
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/effects/catalog.aep"
    ))
    .expect("pinned native Mosaic source");
    let ItemKind::Composition(composition) = &source.item(58).unwrap().kind else {
        panic!("native Mosaic target must be a composition");
    };
    let source_effect = read_effects(&composition.layers[0].content, [320.0, 180.0])
        .0
        .into_iter()
        .find(|effect| effect.match_name == "ADBE Mosaic")
        .expect("independently authored Mosaic");
    assert_eq!(value(&source_effect, "ADBE Mosaic-0001"), [10.0]);
    assert_eq!(value(&source_effect, "ADBE Mosaic-0002"), [10.0]);
    assert_eq!(value(&source_effect, "ADBE Mosaic-0003"), [0.0]);

    // Supplementary edited FX controls exercise scale/bounds, bypass, and the
    // short Hold count change. They are not a new Adobe-authored oracle.
    for (sharp_colors, enabled, scale, size) in [
        (false, true, [100.0, 100.0], [120.0, 80.0]),
        (false, false, [200.0, 50.0], [240.0, 160.0]),
        (true, true, [200.0, 50.0], [240.0, 160.0]),
    ] {
        let mut input = effect_input(json!([
            {"id":9001,"enabled":enabled,"effect":{"type":"mosaic","horizontalBlocks":48,"verticalBlocks":27,"sharpColors":sharp_colors}},
            {"id":9002,"enabled":false,"effect":{"type":"gaussianBlur","blurriness":7,"repeatEdgePixels":true}}
        ]));
        input["composition"]["layers"][0]["transform"]["scale"] = json!(scale);
        input["composition"]["layers"][0]["rect"]["size"] = json!(size);
        let entries: Vec<_> = [
            ("horizontalBlocks", [48.0, 120.0]),
            ("verticalBlocks", [27.0, 68.0]),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (parameter, values))| {
            let mut entry = serde_json::to_value(keyed_entry(
                LayerId::new(900 + index as u64),
                PropType::Rotation,
                [
                    (0, PropertyValue::Float(values[0])),
                    (36, PropertyValue::Float(values[1])),
                ],
            ))
            .unwrap();
            entry["target"] =
                json!({"kind":"effectProperty","effectId":9001,"paramName":parameter});
            entry["animator"]["keyframes"][1]["easing"] = json!({"type":"hold"});
            entry
        })
        .collect();
        input["composition"]["dynamics"]["entries"] = json!(entries);
        let output = export(input);
        let effects = fresh_effects(&output);
        let project = read_project(&output.bytes).unwrap();
        let transform = crate::properties::read_transform(&layers(&project)[0].content).unwrap();
        let native_scale = transform
            .iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(
            native_scale.values,
            [scale[0] / 100.0, scale[1] / 100.0, 1.0]
        );
        assert_eq!(effects.len(), 2);
        let mosaic = &effects[0];
        assert_eq!(mosaic.match_name, "ADBE Mosaic");
        assert_eq!(mosaic.enabled, enabled);
        assert_eq!(value(mosaic, "ADBE Mosaic-0003"), [f64::from(sharp_colors)]);
        for (name, expected) in [
            ("ADBE Mosaic-0001", [48.0, 120.0]),
            ("ADBE Mosaic-0002", [27.0, 68.0]),
        ] {
            let numeric = mosaic
                .parameters
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert_eq!(numeric.keyframes.len(), 2);
            for (key, (time, count)) in numeric
                .keyframes
                .iter()
                .zip([(0.0, expected[0]), (0.036, expected[1])])
            {
                assert!((key.time_secs - time).abs() < 0.002);
                assert_eq!(key.values, [count]);
            }
            assert_eq!(numeric.keyframes[0].out_interpolation, 3);
            assert_eq!(numeric.keyframes[1].in_interpolation, 3);
        }
        assert_eq!(effects[1].match_name, "ADBE Gaussian Blur 2");
        assert!(!effects[1].enabled);
        assert_eq!(value(&effects[1], "ADBE Gaussian Blur 2-0001"), [7.0]);
        assert_eq!(value(&effects[1], "ADBE Gaussian Blur 2-0003"), [1.0]);
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("Mosaic")
                && diagnostic.message.contains("domain")
                && diagnostic.message.contains("retained")
        }));
    }
}

#[test]
fn radial_zoom_export_preserves_static_and_keyed_controls_and_siblings() {
    // Explicit FX input, not an independent Adobe Zoom oracle. Inspect fresh
    // native bytes: the canonical donor's default Type 1 (Spin) is wrong here.
    for animated in [false, true] {
        let mut input = effect_input(json!([
            {"id":9001,"enabled":false,"effect":{"type":"radialBlur","amount":12,"centerX":0.25,"centerY":0.75}},
            {"id":9002,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":7}}
        ]));
        if animated {
            let entries: Vec<_> = [
                ("amount", [12.0, 24.0]),
                ("centerX", [0.25, 0.5]),
                ("centerY", [0.75, 0.25]),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (parameter, values))| {
                // The helper derives key IDs from this seed; each control needs
                // distinct IDs. The actual effect target is replaced below.
                let mut entry = serde_json::to_value(keyed_entry(
                    LayerId::new(900 + index as u64),
                    PropType::Rotation,
                    [
                        (0, PropertyValue::Float(values[0])),
                        (1_000, PropertyValue::Float(values[1])),
                    ],
                ))
                .expect("FX animator JSON");
                entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
                    fx_schema::EffectId::new(9001),
                    parameter,
                ))
                .expect("effect target JSON");
                entry
            })
            .collect();
            input["composition"]["dynamics"]["entries"] = json!(entries);
        }
        let output = export(input);
        let effects = fresh_effects(&output);
        assert_eq!(effects.len(), 2);
        let radial = &effects[0];
        assert_eq!(radial.match_name, "ADBE Radial Blur");
        assert!(!radial.enabled, "disabled radial occurrence persists");
        assert_eq!(effects[1].match_name, "ADBE Gaussian Blur 2");
        assert!(effects[1].enabled);
        assert_eq!(value(&effects[1], "ADBE Gaussian Blur 2-0001"), [7.0]);
        for (name, base, keyed) in [
            (
                "ADBE Radial Blur-0001",
                vec![12.0],
                vec![vec![12.0], vec![24.0]],
            ),
            (
                "ADBE Radial Blur-0002",
                vec![30.0, 60.0],
                vec![vec![30.0, 60.0], vec![60.0, 20.0]],
            ),
        ] {
            let numeric = radial
                .parameters
                .iter()
                .find(|parameter| parameter.match_name == name)
                .expect("editable radial control")
                .numeric
                .as_ref()
                .expect("numeric radial control");
            if animated {
                assert_eq!(
                    numeric
                        .keyframes
                        .iter()
                        .map(|key| key.values.clone())
                        .collect::<Vec<_>>(),
                    keyed
                );
                assert!(numeric.keyframes[0].time_secs.abs() < 0.002);
                assert!((numeric.keyframes[1].time_secs - 1.0).abs() < 0.002);
            } else {
                assert_eq!(value(radial, name), base);
                assert!(numeric.keyframes.is_empty());
            }
        }
        assert_eq!(
            value(radial, "ADBE Radial Blur-0003"),
            [2.0],
            "FX radialBlur is Zoom, not Spin"
        );
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("Zoom") && diagnostic.message.contains("kernel")
        }));
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Only default Spin"))
        );
    }
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
    // A single RGB component with static siblings is now representable. Keep
    // this negative case discriminating: a changing green channel has a different
    // normalized curve on the same original knots.
    let mut green = entry.clone();
    green["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(9001),
        "startG",
    ))
    .unwrap();
    green["animator"]["keyframes"][0]["id"] = json!("color-green-start");
    green["animator"]["keyframes"][1]["id"] = json!("color-green-end");
    green["animator"]["keyframes"][0]["value"]["value"] = json!(0.4);
    green["animator"]["keyframes"][1]["value"]["value"] = json!(0.6);
    green["animator"]["keyframes"][1]["easing"]["y1"] = json!(0.1);
    input["composition"]["dynamics"]["entries"] = json!([entry, green]);
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
fn grain_strength_amount_alias_and_conflicts_preserve_current_native_controls() {
    let entry = |name: &str, values: [f64; 2]| {
        json!({
            "target":{"kind":"effectProperty","effectId":9001,"paramName":name},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                {"id":format!("{name}-a"),"layerTime":0,"value":{"type":"float","value":values[0]},"easing":{"type":"linear"}},
                {"id":format!("{name}-b"),"layerTime":1000,"value":{"type":"float","value":values[1]},"easing":{"type":"hold"}}
            ]}
        })
    };
    for names in [
        vec!["amount"],
        vec!["intensity"],
        vec!["amount", "intensity"],
        vec!["intensity", "amount"],
    ] {
        let mut input = effect_input(json!([
            {"id":9001,"enabled":false,"effect":{"type":"grain","amount":6.0,"seed":13.0,"size":2.5}},
            {"id":9002,"effect":{"type":"gaussianBlur","blurriness":7.0}}
        ]));
        let mut entries: Vec<_> = names
            .iter()
            .map(|name| {
                entry(
                    name,
                    if *name == "amount" {
                        [4.0, 12.0]
                    } else {
                        [8.0, 16.0]
                    },
                )
            })
            .collect();
        entries.push(entry("seed", [13.0, 29.0]));
        input["composition"]["dynamics"]["entries"] = json!(entries);
        let output = export(input);
        let effects = fresh_effects(&output);
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].match_name, "VISINF Grain Implant");
        assert!(!effects[0].enabled);
        assert_eq!(effects[1].match_name, "ADBE Gaussian Blur 2");
        assert_eq!(value(&effects[0], "VISINF Grain Implant-0007"), [2.5]);
        let control = |name: &str| {
            effects[0]
                .parameters
                .iter()
                .find(|parameter| parameter.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
        };
        let strength = control("VISINF Grain Implant-0008");
        if names.len() == 2 {
            assert!(
                strength.keyframes.is_empty(),
                "neither conflicting track may win"
            );
            assert_eq!(strength.values, [6.0]);
            assert!(output.diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .message
                    .contains("competing Grain amount/intensity animator targets")
            }));
        } else {
            let expected = if names[0] == "amount" {
                [4.0, 12.0]
            } else {
                [8.0, 16.0]
            };
            assert_eq!(
                strength
                    .keyframes
                    .iter()
                    .map(|key| (key.time_secs, key.values[0]))
                    .collect::<Vec<_>>(),
                [(0.0, expected[0]), (1.0, expected[1])]
            );
            assert!(
                !output
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("animator omitted")
                        || diagnostic.message.contains("competing Grain"))
            );
        }
        let seed = control("VISINF Grain Implant-0013");
        assert_eq!(
            seed.keyframes
                .iter()
                .map(|key| (key.time_secs, key.values[0]))
                .collect::<Vec<_>>(),
            [(0.0, 13.0), (1.0, 29.0)]
        );
    }
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
        if composition_id == 222 {
            // Add Grain already separates its persisted amount from the runtime target.
            let wire = imported.document.to_json_value().unwrap();
            let entries = wire["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap();
            assert!(
                !entries
                    .iter()
                    .any(|entry| entry["target"]["paramName"] == "amount")
            );
            for (param, expected) in [("intensity", [0.5, 0.8]), ("seed", [9., 13.])] {
                let tracks: Vec<_> = entries
                    .iter()
                    .filter(|entry| entry["target"]["paramName"] == param)
                    .collect();
                assert_eq!(tracks.len(), 1);
                let keys = tracks[0]["animator"]["keyframes"].as_array().unwrap();
                assert_eq!(keys.len(), 2);
                for (index, value) in expected.into_iter().enumerate() {
                    assert_eq!(keys[index]["layerTime"], json!(index * 1000));
                    assert!((keys[index]["value"]["value"].as_f64().unwrap() - value).abs() < 1e-6);
                }
            }
        }
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
fn twirl_plane_legacy_and_identified_static_owners_share_carrier_and_guards() {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/effects_coverage/native_static_controls.aep"
    ))
    .unwrap();
    let original = to_structural_fx_document(&native, Some(625))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn reachable<'a>(
        project: &'a StructuralProject,
        id: u32,
        output: &mut Vec<&'a crate::structure::Layer>,
    ) {
        let Some(item) = project.item(id) else {
            return;
        };
        let ItemKind::Composition(comp) = &item.kind else {
            return;
        };
        for layer in &comp.layers {
            output.push(layer);
            if layer.record.source_id() != 0 {
                reachable(project, layer.record.source_id(), output);
            }
        }
    }
    fn property(layer: &crate::structure::Layer, name: &str) -> Vec<f64> {
        let numeric = crate::properties::read_transform(&layer.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == name)
            .unwrap()
            .numeric
            .unwrap();
        assert!(
            numeric.keyframes.is_empty(),
            "static legacy conversion must not invent animation"
        );
        numeric.values
    }
    for (legacy, enabled) in [(false, true), (true, true), (false, false)] {
        let mut input = original.clone();
        input["composition"]
            .as_object_mut()
            .unwrap()
            .remove("version");
        let owner = &mut input["composition"]["layers"][0]["layers"][0];
        owner["transform"]["position"] = json!([160.0, 90.0]);
        owner["transform"]["anchorPoint"] = json!([60.0, 40.0]);
        owner["transform"]["opacity"] = json!(37.5);
        let controls =
            json!({"type":"twirl","angle":73.0,"radius":0.35,"centerX":0.4,"centerY":0.6});
        let twirl = if legacy {
            controls
        } else {
            json!({"id":9000,"enabled":enabled,"effect":controls})
        };
        let record: fx_schema::EffectRecord = serde_json::from_value(twirl.clone()).unwrap();
        assert_eq!(matches!(record.data(), EffectData::Legacy(_)), legacy);
        owner["effects"] = json!([twirl,
            {"id":9001,"enabled":true,"effect":{"type":"simpleChoker","choke":2.5}},
            {"id":9002,"enabled":false,"effect":{"type":"gaussianBlur","blurriness":7}}
        ]);
        let output = export(input.clone());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|note| note.message.contains("Twirl frame-plane staging:"))
        );
        let project = read_project(&output.bytes).unwrap();
        let mut layers = Vec::new();
        reachable(&project, 1, &mut layers);
        let carriers: Vec<_> = layers
            .iter()
            .filter(|layer| {
                read_effects(&layer.content, [320.0, 180.0])
                    .0
                    .iter()
                    .any(|effect| effect.match_name == "ADBE Twirl")
            })
            .collect();
        assert_eq!(carriers.len(), 1);
        let carrier = *carriers[0];
        let ItemKind::Composition(frame) = &project.item(carrier.record.source_id()).unwrap().kind
        else {
            panic!("reachable carrier frame");
        };
        assert_eq!((frame.width, frame.height), (320, 180));
        assert_eq!(property(carrier, "ADBE Position"), vec![0.0; 3]);
        assert_eq!(property(carrier, "ADBE Anchor Point"), vec![0.0; 3]);
        assert_eq!(property(carrier, "ADBE Opacity"), vec![0.375]);
        assert_eq!(carrier.record.start_time(), Some(0.0));
        assert_eq!(carrier.record.in_point(), Some(0.0));
        assert_eq!(carrier.record.out_point(), Some(2.0));
        let effects = read_effects(&carrier.content, [320.0, 180.0]).0;
        assert_eq!(
            effects
                .iter()
                .map(|effect| (effect.match_name.as_str(), effect.enabled))
                .collect::<Vec<_>>(),
            [
                ("ADBE Twirl", enabled),
                ("ADBE Simple Choker", true),
                ("ADBE Gaussian Blur 2", false)
            ]
        );
        assert_eq!(value(&effects[0], "ADBE Twirl-0001"), vec![73.0]);
        assert_eq!(value(&effects[0], "ADBE Twirl-0002"), vec![35.0]);
        assert_eq!(value(&effects[0], "ADBE Twirl-0003").len(), 2);
        for (actual, expected) in value(&effects[0], "ADBE Twirl-0003")
            .iter()
            .zip([128.0, 108.0])
        {
            assert!((actual - expected).abs() < 1e-6);
        }
        let mut inner = Vec::new();
        reachable(&project, carrier.record.source_id(), &mut inner);
        assert!(inner.iter().any(|layer| layer.record.layer_type() == 4));
        let mut translations = 0;
        for layer in inner {
            assert_eq!(property(layer, "ADBE Opacity"), vec![1.0]);
            let position = property(layer, "ADBE Position");
            let mut anchor = property(layer, "ADBE Anchor Point");
            if let Some(item) = project.item(layer.record.source_id())
                && let ItemKind::Composition(comp) = &item.kind
            {
                anchor[0] *= f64::from(comp.width);
                anchor[1] *= f64::from(comp.height);
            }
            if (position[0] - anchor[0] - 100.0).abs() < 1e-6
                && (position[1] - anchor[1] - 50.0).abs() < 1e-6
            {
                translations += 1;
            } else {
                assert!((position[0] - anchor[0]).abs() < 1e-6);
                assert!((position[1] - anchor[1]).abs() < 1e-6);
            }
        }
        assert_eq!(
            translations, 1,
            "current spatial transform occurs once inside, not on the effect plane"
        );
        input["composition"]["layers"][0]["layers"][0]["playback"] = fixture_linear_playback(
            json!({"start":500,"duration":1500}),
            json!({"start":0,"duration":1500}),
        );
        let unsafe_clock = export(input);
        assert!(unsafe_clock.diagnostics.iter().any(|note| {
            note.message
                .contains("own Group clock/window is not full-span identity")
        }));
        assert!(
            !unsafe_clock
                .diagnostics
                .iter()
                .any(|note| note.message.contains("Twirl frame-plane staging:"))
        );
    }
    assert!(
        serde_json::from_value::<fx_schema::EffectRecord>(
            json!({"type":"twirl","angle":"invalid"})
        )
        .is_err()
    );
    let mut unknown = original;
    unknown["composition"]["layers"][0]["layers"][0]["effects"] = json!([{"type":"futureTwirl"}]);
    let output = export(unknown);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|note| note.message.contains("Twirl frame-plane staging:"))
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|note| note.message.contains("Unknown FX effect payload omitted"))
    );
}

#[test]
fn twirl_plane_carrier_applies_fractional_and_keyed_opacity_after_alpha_sensitive_effects() {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/effects_coverage/native_animated_controls.aep"
    ))
    .unwrap();
    let original = to_structural_fx_document(&native, Some(612))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn reachable<'a>(
        project: &'a StructuralProject,
        id: u32,
        out: &mut Vec<&'a crate::structure::Layer>,
    ) {
        let Some(item) = project.item(id) else {
            return;
        };
        let ItemKind::Composition(comp) = &item.kind else {
            return;
        };
        for layer in &comp.layers {
            out.push(layer);
            if layer.record.source_id() != 0 {
                reachable(project, layer.record.source_id(), out);
            }
        }
    }
    fn property(layer: &crate::structure::Layer, name: &str) -> crate::properties::NumericProperty {
        crate::properties::read_transform(&layer.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == name)
            .unwrap()
            .numeric
            .unwrap()
    }
    for animated_opacity in [false, true] {
        let mut input = original.clone();
        let owner = &mut input["composition"]["layers"][0]["layers"][0];
        let owner_id = LayerId::new(owner["id"].as_u64().unwrap());
        owner["transform"]["opacity"] = json!(37.5);
        owner["transform"]["position"] = json!([160.0, 90.0]);
        owner["transform"]["anchorPoint"] = json!([60.0, 40.0]);
        owner["effects"].as_array_mut().unwrap().truncate(1);
        owner["effects"].as_array_mut().unwrap().extend([
            json!({"id":9001,"enabled":true,"effect":{"type":"simpleChoker","choke":2.5}}),
            json!({"id":9002,"enabled":false,"effect":{"type":"gaussianBlur","blurriness":7}}),
        ]);
        let entries = input["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap();
        // Position must stay inside and remain keyed while only opacity moves.
        entries.push(
            serde_json::to_value(keyed_entry(
                owner_id,
                PropType::PositionX,
                [
                    (0, PropertyValue::Float(160.0)),
                    (1000, PropertyValue::Float(200.0)),
                ],
            ))
            .unwrap(),
        );
        if animated_opacity {
            entries.push(
                serde_json::to_value(keyed_entry(
                    owner_id,
                    PropType::Opacity,
                    [
                        (0, PropertyValue::Float(37.5)),
                        (1000, PropertyValue::Float(81.25)),
                    ],
                ))
                .unwrap(),
            );
        }
        let output = export(input);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|note| note.message.contains("Twirl frame-plane staging:"))
        );
        let project = read_project(&output.bytes).unwrap();
        let mut all = Vec::new();
        reachable(&project, 1, &mut all);
        let owners: Vec<_> = all
            .iter()
            .filter(|layer| {
                read_effects(&layer.content, [320.0, 180.0])
                    .0
                    .iter()
                    .any(|effect| effect.match_name == "ADBE Twirl")
            })
            .collect();
        assert_eq!(owners.len(), 1);
        let outer = *owners[0];
        let opacity = property(outer, "ADBE Opacity");
        assert_eq!(
            opacity.keyframes.len(),
            if animated_opacity { 2 } else { 0 }
        );
        if animated_opacity {
            for (key, (time, value)) in opacity.keyframes.iter().zip([(0.0, 0.375), (1.0, 0.8125)])
            {
                assert!((key.time_secs - time).abs() < 0.002);
                assert_eq!(key.values, vec![value]);
            }
        } else {
            assert_eq!(opacity.values, vec![0.375]);
        }
        let position = property(outer, "ADBE Position");
        assert!(position.keyframes.is_empty());
        assert_eq!(position.values, vec![0.0; 3]);
        assert_eq!(outer.record.start_time(), Some(0.0));
        assert_eq!(outer.record.in_point(), Some(0.0));
        assert_eq!(outer.record.out_point(), Some(2.0));
        let ItemKind::Composition(frame) = &project.item(outer.record.source_id()).unwrap().kind
        else {
            panic!("frame carrier");
        };
        assert_eq!((frame.width, frame.height), (320, 180));
        let mut inner = Vec::new();
        reachable(&project, outer.record.source_id(), &mut inner);
        let spatial: Vec<_> = inner
            .iter()
            .filter(|layer| !property(layer, "ADBE Position").keyframes.is_empty())
            .collect();
        assert_eq!(
            spatial.len(),
            1,
            "spatial transform is authored exactly once inside"
        );
        let ItemKind::Composition(source) =
            &project.item(spatial[0].record.source_id()).unwrap().kind
        else {
            panic!("spatial source");
        };
        let anchor = property(spatial[0], "ADBE Anchor Point");
        assert!((anchor.values[0] * f64::from(source.width) - 60.0).abs() < 1e-6);
        assert!((anchor.values[1] * f64::from(source.height) - 40.0).abs() < 1e-6);
        let position = property(spatial[0], "ADBE Position");
        assert_eq!(position.keyframes.len(), 2);
        for (key, (time, x)) in position.keyframes.iter().zip([(0.0, 160.0), (1.0, 200.0)]) {
            assert!((key.time_secs - time).abs() < 0.002);
            assert_eq!(key.values, vec![x, 90.0, 0.0]);
        }
        for layer in inner {
            let opacity = property(layer, "ADBE Opacity");
            assert_eq!(
                opacity.values,
                vec![1.0],
                "inner source opacity is identity"
            );
            assert!(
                opacity.keyframes.is_empty(),
                "owner opacity track must not be copied into source"
            );
            assert_eq!(layer.record.start_time(), Some(0.0));
            assert_eq!(layer.record.in_point(), Some(0.0));
            assert_eq!(layer.record.out_point(), Some(2.0));
        }
        let effects = read_effects(&outer.content, [320.0, 180.0]).0;
        assert_eq!(
            effects
                .iter()
                .map(|effect| (effect.match_name.as_str(), effect.enabled))
                .collect::<Vec<_>>(),
            [
                ("ADBE Twirl", true),
                ("ADBE Simple Choker", true),
                ("ADBE Gaussian Blur 2", false)
            ]
        );
        assert_eq!(value(&effects[1], "ADBE Simple Choker-0002"), vec![2.5]);
        assert_eq!(value(&effects[2], "ADBE Gaussian Blur 2-0001"), vec![7.0]);
        let angle = effects[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Twirl-0001")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(angle.keyframes.len(), 2);
        for (key, (time, value)) in angle.keyframes.iter().zip([(0.0, 45.0), (1.0, 90.0)]) {
            assert!((key.time_secs - time).abs() < 0.002);
            assert_eq!(key.values, vec![value]);
        }
    }
}

#[test]
fn twirl_plane_nonidentity_own_clock_keeps_ordinary_occurrence_and_window() {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/effects_coverage/native_animated_controls.aep"
    ))
    .unwrap();
    let original = to_structural_fx_document(&native, Some(612))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    for (playback, window) in [
        (
            fixture_linear_playback(
                json!({"start":500,"duration":1500}),
                json!({"start":0,"duration":1500}),
            ),
            [0.5, 2.0],
        ),
        (
            fixture_remapped_playback(
                json!({"start":0,"duration":2000}),
                json!({
                "before":"inactive","after":"inactive","keyframes":[
                    {"id":"clock0","time":0,"value":0,"easing":{"type":"linear"}},
                    {"id":"clock1","time":1000,"value":500,"easing":{"type":"linear"}},
                    {"id":"clock2","time":2000,"value":2000,"easing":{"type":"linear"}}
                ]}),
            ),
            [0.0, 2.0],
        ),
    ] {
        let mut input = original.clone();
        input["composition"]["layers"][0]["layers"][0]["playback"] = playback;
        let output = export(input);
        assert!(output.diagnostics.iter().any(|note| {
            note.message
                .contains("own Group clock/window is not full-span identity")
        }));
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|note| note.message.contains("Twirl frame-plane staging:"))
        );
        let project = read_project(&output.bytes).unwrap();
        let owners: Vec<_> = project
            .items
            .iter()
            .filter_map(|item| {
                let ItemKind::Composition(comp) = &item.kind else {
                    return None;
                };
                Some(&comp.layers)
            })
            .flatten()
            .filter(|layer| {
                read_effects(&layer.content, [320.0, 180.0])
                    .0
                    .iter()
                    .any(|effect| effect.match_name == "ADBE Twirl")
            })
            .collect();
        assert_eq!(
            owners.len(),
            1,
            "fallback retains editable Twirl, not an omitted owner"
        );
        let owner = owners[0];
        let start = owner.record.start_time().unwrap();
        assert!((start + owner.record.in_point().unwrap() - window[0]).abs() < 0.002);
        assert!((start + owner.record.out_point().unwrap() - window[1]).abs() < 0.002);
        assert_ne!(
            owner.record.source_id(),
            0,
            "content source remains reachable from the effect occurrence"
        );
    }
}

#[test]
fn native_twirl_translated_owner_stages_image_and_frame_on_import_and_edited_export() {
    use sha2::{Digest, Sha256};

    for (bytes, hash, composition_id, owner_id, animated) in [
        (
            include_bytes!("../../../tests/fixtures/effects_coverage/native_static_controls.aep")
                .as_slice(),
            "7c65ebe724fc8403399979cc1354adca3abd737b6ad7a76ba8eafc5b61ccf95b",
            625,
            637,
            false,
        ),
        (
            include_bytes!("../../../tests/fixtures/effects_coverage/native_animated_controls.aep")
                .as_slice(),
            "4e38ded65e44f6a1adfe22713ddb36557afd312959a612ee601b75849d3f73ce",
            612,
            624,
            true,
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), hash);
        let native = read_project(bytes).unwrap();
        let ItemKind::Composition(comp) = &native.item(composition_id).unwrap().kind else {
            panic!("pinned native composition");
        };
        assert_eq!((comp.width, comp.height), (320, 180));
        assert_eq!(comp.layers.len(), 1);
        assert_eq!(comp.layers[0].record.id(), owner_id);
        assert_eq!(comp.layers[0].record.source_id(), 0);
        let imported = to_structural_fx_document(&native, Some(composition_id)).unwrap();
        assert!(
            imported.diagnostics.iter().any(|diagnostic| {
                diagnostic.layer_id == Some(owner_id)
                    && diagnostic.message.contains("Twirl")
                    && diagnostic
                        .message
                        .contains("late full-composition CornerPin")
            }),
            "{:?}",
            imported.diagnostics
        );
        let original = imported.document.to_json_value().unwrap();
        let owner = &original["composition"]["layers"][0]["layers"][0];
        assert_eq!(owner["transform"]["position"], json!([0.0, 0.0]));
        assert_eq!(owner["transform"]["anchorPoint"], json!([0.0, 0.0]));
        assert_eq!(owner["transform"]["scale"], json!([100.0, 100.0]));
        assert_eq!(owner["transform"]["rotation"].as_f64(), Some(0.0));
        assert_eq!(owner["effects"].as_array().unwrap().len(), 2);
        let transport = &owner["effects"][1]["effect"];
        assert_eq!(transport["type"], "cornerPin");
        for (name, expected) in [
            ("upperLeftX", 100.0 / 320.0),
            ("upperLeftY", 50.0 / 180.0),
            ("upperRightX", 420.0 / 320.0),
            ("upperRightY", 50.0 / 180.0),
            ("lowerLeftX", 100.0 / 320.0),
            ("lowerLeftY", 230.0 / 180.0),
            ("lowerRightX", 420.0 / 320.0),
            ("lowerRightY", 230.0 / 180.0),
        ] {
            assert!((transport[name].as_f64().unwrap() - expected).abs() < 1e-9);
        }
        // Runtime Group CornerPin uses the full composition as its reference.
        for (point, expected) in [
            ([30.0, 60.0], [130.0, 110.0]),
            ([90.0, 20.0], [190.0, 70.0]),
        ] {
            for axis in 0..2 {
                let (lo, hi, size) = if axis == 0 {
                    ("upperLeftX", "upperRightX", 320.0)
                } else {
                    ("upperLeftY", "lowerLeftY", 180.0)
                };
                let mapped = size * transport[lo].as_f64().unwrap()
                    + point[axis]
                        * (transport[hi].as_f64().unwrap() - transport[lo].as_f64().unwrap());
                assert!((mapped - expected[axis]).abs() < 1e-9);
            }
        }
        let effect = &owner["effects"][0];
        assert_eq!(effect["enabled"], true);
        assert_eq!(effect["effect"]["type"], "twirl");
        let entries = original["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let tracks: Vec<_> = entries
            .iter()
            .filter(|entry| entry["target"]["effectId"] == effect["id"])
            .collect();
        assert_eq!(tracks.len(), if animated { 4 } else { 0 });
        for (param, start, end) in [
            ("angle", 45.0, 90.0),
            ("radius", 0.3, 0.5),
            ("centerX", 0.09375, 0.28125),
            ("centerY", 1.0 / 3.0, 1.0 / 9.0),
        ] {
            assert!((effect["effect"][param].as_f64().unwrap() - start).abs() < 1e-6);
            if animated {
                let track = tracks
                    .iter()
                    .find(|entry| entry["target"]["paramName"] == param)
                    .unwrap();
                let keys = track["animator"]["keyframes"].as_array().unwrap();
                assert_eq!(keys.len(), 2);
                for (index, expected) in [start, end].into_iter().enumerate() {
                    assert_eq!(keys[index]["layerTime"], index * 1000);
                    assert!(
                        (keys[index]["value"]["value"].as_f64().unwrap() - expected).abs() < 1e-6
                    );
                }
                assert_eq!(keys[1]["easing"]["type"], "linear");
            }
        }
        let content = &owner["layers"][0];
        assert_eq!(content["name"], "Source content clock");
        let clock = &content["playback"];
        assert_eq!(clock["inputRange"], json!({"start":0,"duration":2000}));
        assert_eq!(clock["inputOffsetMs"], 0);
        let keys = clock["mapping"]["property"]["keyframes"]
            .as_array()
            .unwrap();
        assert_eq!(keys.len(), 2);
        for (key, time) in keys.iter().zip([0, 2000]) {
            assert_eq!(key["time"], time);
            assert_eq!(key["value"], time);
            assert_eq!(key["easing"]["type"], "linear");
        }

        assert_eq!(content["layers"][0]["rect"]["size"], json!([120.0, 80.0]));

        // These edits are explicit FX inputs, not reconstructed native coordinates.
        // Keep keys/clock/content intact; edit all centerX knots independently.
        for edited in [false, true] {
            let mut input = original.clone();
            if edited {
                // An independently edited ordinary FX Transform must move content
                // inside the carrier, not move the Twirl frame or its controls.
                let edited_owner = &mut input["composition"]["layers"][0]["layers"][0];
                edited_owner["effects"].as_array_mut().unwrap().pop();
                edited_owner["transform"]["position"] = json!([160.0, 90.0]);
                edited_owner["transform"]["anchorPoint"] = json!([60.0, 40.0]);
                input["composition"]["layers"][0]["layers"][0]["effects"][0]["effect"]["centerX"] =
                    json!(0.4);
                for entry in input["composition"]["dynamics"]["entries"]
                    .as_array_mut()
                    .unwrap()
                {
                    if entry["target"]["effectId"] == effect["id"]
                        && entry["target"]["paramName"] == "centerX"
                    {
                        entry["animator"]["keyframes"][0]["value"]["value"] = json!(0.4);
                        entry["animator"]["keyframes"][1]["value"]["value"] = json!(0.7);
                    }
                }
            }
            input["composition"]["layers"][0]["layers"][0]["effects"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "id":9001,"enabled":false,
                    "effect":{"type":"gaussianBlur","blurriness":7}
                }));
            let output = export(input);
            assert!(
                output.diagnostics.iter().any(|diagnostic| {
                    diagnostic.message.contains("Twirl frame-plane staging:")
                }),
                "{:?}",
                output.diagnostics
            );
            let generated = read_project(&output.bytes).unwrap();
            let owners: Vec<_> = generated
                .items
                .iter()
                .filter_map(|item| {
                    let ItemKind::Composition(comp) = &item.kind else {
                        return None;
                    };
                    Some(&comp.layers)
                })
                .flatten()
                .filter_map(|layer| {
                    let (effects, _) = read_effects(&layer.content, [320.0, 180.0]);
                    effects
                        .iter()
                        .any(|effect| effect.match_name == "ADBE Twirl")
                        .then_some((layer, effects))
                })
                .collect();
            assert_eq!(owners.len(), 1);
            let (layer, effects) = &owners[0];
            assert_eq!(effects.len(), if edited { 2 } else { 3 });
            assert_eq!(effects[0].match_name, "ADBE Twirl");
            assert!(effects[0].enabled);
            let blur = effects.last().unwrap();
            assert_eq!(blur.match_name, "ADBE Gaussian Blur 2");
            assert!(!blur.enabled);
            assert_eq!(value(blur, "ADBE Gaussian Blur 2-0001"), [7.0]);
            if !edited {
                assert_eq!(effects[1].match_name, "ADBE Corner Pin");
                for (name, expected) in [
                    ("ADBE Corner Pin-0001", [100.0, 50.0]),
                    ("ADBE Corner Pin-0002", [420.0, 50.0]),
                    ("ADBE Corner Pin-0003", [100.0, 230.0]),
                    ("ADBE Corner Pin-0004", [420.0, 230.0]),
                ] {
                    let actual = value(&effects[1], name);
                    assert_eq!(actual.len(), 2);
                    for (actual, expected) in actual.iter().zip(expected) {
                        assert!((actual - expected).abs() < 1e-6);
                    }
                }
            }
            let ItemKind::Composition(source) =
                &generated.item(layer.record.source_id()).unwrap().kind
            else {
                panic!("bounded Twirl source");
            };
            assert_eq!((source.width, source.height), (320, 180));
            // Follow only reachable source edges from the published root, not
            // disconnected items that happen to carry correct control bytes.
            fn reachable<'a>(
                project: &'a StructuralProject,
                id: u32,
                out: &mut Vec<&'a crate::structure::Layer>,
            ) {
                let ItemKind::Composition(comp) = &project.item(id).unwrap().kind else {
                    return;
                };
                for layer in &comp.layers {
                    out.push(layer);
                    if layer.record.source_id() != 0 {
                        reachable(project, layer.record.source_id(), out);
                    }
                }
            }
            let mut reachable_layers = Vec::new();
            reachable(&generated, 1, &mut reachable_layers);
            assert!(
                reachable_layers
                    .iter()
                    .any(|candidate| candidate.record.id() == layer.record.id())
            );
            assert!(
                reachable_layers
                    .iter()
                    .any(|candidate| candidate.record.layer_type() == 4)
            );
            let properties = crate::properties::read_transform(&layer.content).unwrap();
            for name in ["ADBE Anchor Point", "ADBE Position"] {
                let numeric = properties
                    .iter()
                    .find(|property| property.match_name == name)
                    .unwrap()
                    .numeric
                    .as_ref()
                    .unwrap();
                assert!(
                    numeric.values.iter().all(|value| value.abs() < 1e-9),
                    "identity carrier {name}: {numeric:?}"
                );
                assert!(numeric.keyframes.is_empty());
            }
            let mut inside = Vec::new();
            reachable(&generated, layer.record.source_id(), &mut inside);
            let translations: Vec<_> = inside
                .iter()
                .filter_map(|child| {
                    let properties = crate::properties::read_transform(&child.content).unwrap();
                    let get = |name| {
                        properties
                            .iter()
                            .find(|property| property.match_name == name)
                            .and_then(|property| property.numeric.as_ref().ok())
                            .map(|numeric| numeric.values.clone())
                    };
                    let position = get("ADBE Position")?;
                    let mut anchor = get("ADBE Anchor Point")?;
                    if let Some(item) = generated.item(child.record.source_id())
                        && let ItemKind::Composition(comp) = &item.kind
                    {
                        anchor[0] *= f64::from(comp.width);
                        anchor[1] *= f64::from(comp.height);
                    }
                    Some([position[0] - anchor[0], position[1] - anchor[1]])
                })
                .collect();
            let transported = translations
                .iter()
                .filter(|delta| (delta[0] - 100.0).abs() < 1e-6 && (delta[1] - 50.0).abs() < 1e-6)
                .count();
            assert_eq!(
                transported,
                usize::from(edited),
                "transport exactly once: {translations:?}"
            );
            assert_eq!(layer.record.start_time(), Some(0.0));
            assert_eq!(layer.record.in_point(), Some(0.0));
            assert_eq!(layer.record.out_point(), Some(2.0));
            for child in inside {
                assert_eq!(child.record.start_time(), Some(0.0));
                assert_eq!(child.record.in_point(), Some(0.0));
                assert_eq!(child.record.out_point(), Some(2.0));
            }
            let point = effects[0]
                .parameters
                .iter()
                .find(|parameter| parameter.match_name == "ADBE Twirl-0003")
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert_eq!(point.keyframes.len(), if animated { 2 } else { 0 });
            // Animated native leaves carry a key list instead of static cdat values.
            let initial_point = if animated {
                assert!(point.values.is_empty());
                assert!(point.keyframes[0].time_secs.abs() < 0.002);
                &point.keyframes[0].values
            } else {
                &point.values
            };
            assert_eq!(initial_point.len(), 2);
            let x = if edited { 0.4 } else { 0.09375 };
            assert!((initial_point[0] - 320.0 * x).abs() < 1e-5);
            assert!((initial_point[1] - 180.0 / 3.0).abs() < 1e-5);

            for (name, start, end) in [
                ("ADBE Twirl-0001", 45.0, 90.0),
                ("ADBE Twirl-0002", 30.0, 50.0),
            ] {
                let numeric = effects[0]
                    .parameters
                    .iter()
                    .find(|p| p.match_name == name)
                    .unwrap()
                    .numeric
                    .as_ref()
                    .unwrap();
                assert_eq!(numeric.keyframes.len(), if animated { 2 } else { 0 });
                if animated {
                    assert!(numeric.values.is_empty());
                    for (key, (time, expected)) in
                        numeric.keyframes.iter().zip([(0.0, start), (1.0, end)])
                    {
                        assert_eq!(key.values.len(), 1);
                        assert!((key.values[0] - expected).abs() < 1e-5);
                        assert!((key.time_secs - time).abs() < 0.002);
                    }
                } else {
                    assert_eq!(numeric.values.len(), 1);
                    assert!((numeric.values[0] - start).abs() < 1e-5);
                }
            }
            if animated {
                assert_eq!(point.keyframes[1].values.len(), 2);
                let end_x = if edited { 0.7 } else { 0.28125 };
                assert!((point.keyframes[1].values[0] - 320.0 * end_x).abs() < 1e-5);
                assert!((point.keyframes[1].values[1] - 180.0 / 9.0).abs() < 1e-5);
                assert!((point.keyframes[1].time_secs - 1.0).abs() < 0.002);
            }
        }
    }
}

#[test]
fn warp_static_and_keyed_phase_export_never_adds_native_speed() {
    // The native catalog's Wave Speed defaults are 1 in both effects. Our
    // editable FX kernels have only explicit phase, never an implicit clock.
    let mut input = effect_input(json!([
        {"id":9001,"enabled":false,"effect":{"type":"ripple","amplitude":0.02,"frequency":20,"phase":0.3,"centerX":0.4,"centerY":0.6}},
        {"id":9002,"enabled":true,"effect":{"type":"waveWarp","waveHeight":0.1,"waveWidth":6,"direction":30,"phase":0.5}},
        {"id":9003,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":7}}
    ]));
    let entries: Vec<_> = [9001, 9002]
        .into_iter()
        .map(|id| {
            let mut entry = serde_json::to_value(keyed_entry(
                LayerId::new(id),
                PropType::Rotation,
                [
                    (0, PropertyValue::Float(0.25)),
                    (1000, PropertyValue::Float(1.5)),
                ],
            ))
            .unwrap();
            entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
                fx_schema::EffectId::new(id),
                "phase",
            ))
            .unwrap();
            entry
        })
        .collect();
    input["composition"]["dynamics"]["entries"] = json!(entries);
    let output = export(input);
    let effects = fresh_effects(&output);
    assert_eq!(
        effects
            .iter()
            .map(|e| e.match_name.as_str())
            .collect::<Vec<_>>(),
        ["ADBE Ripple", "ADBE Wave Warp", "ADBE Gaussian Blur 2"]
    );
    assert!(!effects[0].enabled);
    for (effect, speed) in [
        (&effects[0], "ADBE Ripple-0004"),
        (&effects[1], "ADBE Wave Warp-0005"),
    ] {
        assert_eq!(value(effect, speed), [0.]);
        let phase = effect
            .parameters
            .iter()
            .find(|p| p.match_name.ends_with("-0007"))
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(phase.keyframes.len(), 2);
        for (key, expected) in phase.keyframes.iter().zip([0.25_f64, 1.5]) {
            assert!((key.values[0] - expected.to_degrees()).abs() < 0.002);
        }
        assert!((phase.keyframes[1].time_secs - 1.).abs() < 0.001);
    }
}

#[test]
fn warp_invalid_frequency_omits_only_the_unsafe_effect() {
    let output = export(effect_input(json!([
        {"id":9001,"effect":{"type":"ripple","frequency":0}},
        {"id":9002,"effect":{"type":"waveWarp","waveWidth":-1}},
        {"id":9003,"effect":{"type":"gaussianBlur","blurriness":7}}
    ])));
    let effects = fresh_effects(&output);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].match_name, "ADBE Gaussian Blur 2");
}

#[test]
fn warp_omitted_frequency_exports_effective_fx_defaults_not_donor_widths() {
    for (kind, field, default, native, width, speed) in [
        ("ripple", "frequency", 30., "ADBE Ripple", "-0005", "-0004"),
        (
            "waveWarp",
            "waveWidth",
            6.,
            "ADBE Wave Warp",
            "-0003",
            "-0005",
        ),
    ] {
        let mut results = Vec::new();
        for explicit in [false, true] {
            let mut effect = json!({"type":kind});
            if explicit {
                effect[field] = json!(default);
            }
            let mut input = effect_input(json!([{"id":9001,"effect":effect}]));
            input["composition"]["layers"][0]["rect"]["size"] = json!([320., 180.]);
            let output = export(input);
            let effects = fresh_effects(&output);
            assert_eq!(effects.len(), 1);
            assert_eq!(effects[0].match_name, native);
            assert_eq!(value(&effects[0], &format!("{native}{speed}")), [0.]);
            results.push(value(&effects[0], &format!("{native}{width}"))[0]);
        }
        let expected = 320.
            * if kind == "ripple" {
                std::f64::consts::TAU / 30.
            } else {
                1. / 6.
            };
        for actual in &results {
            assert!(
                (actual - expected).abs() < 0.002,
                "{kind}: {results:?}, expected {expected}"
            );
        }
        assert_eq!(results[0], results[1]);
    }
}

#[test]
fn native_fractal_keys_current_export_uses_turbulent_controls() {
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/native-fractal-turbulent-keys.aep"
    ))
    .unwrap();
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    let mut document = imported.document.to_json_value().unwrap();
    for entry in document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
    {
        let value = match entry["target"]["paramName"].as_str() {
            Some("contrast") => 170.,
            Some("brightness") => 20.,
            Some("scale") => 180.,
            Some("offsetX") => 10.,
            Some("offsetY") => -5.,
            Some("evolution") => 90.,
            _ => continue,
        };
        entry["animator"]["keyframes"][1]["value"]["value"] = json!(value);
    }
    let output = export(document);
    let native = read_project(&output.bytes).unwrap();
    let mut found = Vec::new();
    for item in &native.items {
        if let ItemKind::Composition(comp) = &item.kind {
            for layer in &comp.layers {
                found.extend(
                    read_effects(
                        &layer.content,
                        [f64::from(comp.width), f64::from(comp.height)],
                    )
                    .0
                    .into_iter()
                    .filter(|effect| effect.match_name == "ADBE AIF Perlin Noise 3D"),
                );
            }
        }
    }
    assert_eq!(found.len(), 1);
    for (slot, expected) in [
        ("0004", vec![vec![100.], vec![170.]]),
        ("0005", vec![vec![0.], vec![20.]]),
        ("0010", vec![vec![100.], vec![180.]]),
        ("0013", vec![vec![160., 90.], vec![192., 74.]]),
        ("0020", vec![vec![0.], vec![90.]]),
    ] {
        let parameter = found[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == format!("ADBE AIF Perlin Noise 3D-{slot}"))
            .unwrap();
        let numeric = parameter.numeric.as_ref().unwrap();
        assert_eq!(numeric.keyframes.len(), 2);
        for (index, key) in numeric.keyframes.iter().enumerate() {
            assert_eq!(key.time_secs, index as f64);
            for (actual, expected) in key.values.iter().zip(&expected[index]) {
                assert!((actual - expected).abs() < 1e-6, "{slot}: {key:?}");
            }
        }
    }
}
