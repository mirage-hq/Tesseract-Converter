use super::*;

const TEMPLATE: &str = include_str!("../../../tests/fixtures/effects_coverage/template.fx.json");

fn object_with_type<'a>(
    value: &'a Value,
    kind: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some(kind) {
                return Some(object);
            }
            object
                .values()
                .find_map(|value| object_with_type(value, kind))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| object_with_type(value, kind)),
        _ => None,
    }
}

fn objects_with_type<'a>(
    value: &'a Value,
    kind: &str,
    output: &mut Vec<&'a serde_json::Map<String, Value>>,
) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some(kind) {
                output.push(object);
            }
            for child in object.values() {
                objects_with_type(child, kind, output);
            }
        }
        Value::Array(values) => {
            for child in values {
                objects_with_type(child, kind, output);
            }
        }
        _ => {}
    }
}

fn assert_point(value: &[Value], expected: [f64; 2], case: &str) {
    for (actual, expected) in value.iter().zip(expected) {
        assert!(
            (actual.as_f64().unwrap() - expected).abs() < 1e-9,
            "{case}: {value:?}"
        );
    }
}

fn effect_animation(effect_id: u64, param: &str, values: [(i64, PropertyValue); 2]) -> Value {
    let mut entry =
        serde_json::to_value(keyed_entry(LayerId::new(900), PropType::Rotation, values)).unwrap();
    entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(effect_id),
        param,
    ))
    .unwrap();
    for (index, key) in entry["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        key["id"] = json!(format!("layer-style-{effect_id}-{param}-{index}"));
    }
    entry
}

fn disabled_effect_animation(
    effect_id: u64,
    param: &str,
    values: [(i64, PropertyValue); 2],
    disabled_value: PropertyValue,
) -> Value {
    let mut entry: AnimationGraphEntry =
        serde_json::from_value(effect_animation(effect_id, param, values)).unwrap();
    let mut data = entry.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value: stored_disabled_value,
        ..
    } = &mut data
    else {
        panic!("Layer Style fixture is keyed")
    };
    *enabled = false;
    *stored_disabled_value = Some(disabled_value);
    entry.animator = fx_schema::PropertyAnimator::from_data(&data).unwrap();
    serde_json::to_value(entry).unwrap()
}

fn clocked_drop_shadow_group(playback: Value, include_sibling: bool) -> Value {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    let mut child = input["composition"]["layers"][0].clone();
    child["id"] = json!(901);
    child["activeRange"] = json!({"start":0,"duration":2000});
    let transform = child["transform"].clone();
    let group = json!({
        "type":"Group", "id":900, "name":"Clocked style owner", "parent":null,
        "transform":transform,
        "playback":playback, "layers":[child],
        "effects":[{
            "id":9250, "enabled":true,
            "effect":{"type":"dropShadow","enabled":true,"color":[0.1,0.2,0.3,0.7],"offset":[3.0,4.0],"blurRadius":3.0,"spreadRadius":0.0,"blendMode":"normal"}
        }]
    });
    let mut layers = vec![group];
    if include_sibling {
        let mut sibling = input["composition"]["layers"][0].clone();
        sibling["id"] = json!(902);
        sibling["name"] = json!("Clock sibling");
        layers.push(sibling);
    }
    input["composition"]["layers"] = Value::Array(layers);
    input["composition"]["dynamics"]["entries"] = json!([effect_animation(
        9250,
        "blurRadius",
        [
            (500, PropertyValue::Float(3.0)),
            (1_500, PropertyValue::Float(7.0)),
        ],
    )]);
    input
}

#[test]
fn outer_glow_exports_as_native_layer_style_and_freshly_imports_as_editable_effect() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([{
        "id": 9001,
        "enabled": true,
        "effect": {
            "type": "outerGlow",
            "enabled": true,
            "color": [0.1, 0.8, 1.0, 0.85],
            "size": 14.0,
            "spread": 0.25,
            "range": 0.75,
            "blendMode": "screen"
        }
    }]);
    let output = export(input);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("solid-color, zero-noise, default-technique")
    }));

    let project = read_project(&output.bytes).unwrap();
    let layer = layers(&project)
        .iter()
        .find(|layer| {
            !crate::layer_styles::read(&layer.content, [320.0, 180.0])
                .styles
                .is_empty()
        })
        .unwrap_or_else(|| {
            panic!(
                "exported Outer Glow owner missing: {:?}; diagnostics: {:?}",
                layers(&project)
                    .iter()
                    .map(|layer| (
                        &layer.name,
                        crate::layer_styles::read(&layer.content, [320.0, 180.0]).warnings
                    ))
                    .collect::<Vec<_>>(),
                output.diagnostics
            )
        });
    let decoded = crate::layer_styles::read(&layer.content, [320.0, 180.0]);
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
    let [crate::layer_styles::NativeLayerStyle::OuterGlow(glow)] = decoded.styles.as_slice() else {
        panic!("one native Outer Glow")
    };
    assert!(glow.enabled);
    assert_eq!(glow.blend_mode, fx_schema::BlendMode::Screen);
    assert!((glow.color[0] - 0.1).abs() < 1e-9);
    assert!((glow.color[1] - 0.8).abs() < 1e-9);
    assert!((glow.color[2] - 1.0).abs() < 1e-9);
    assert!((glow.color[3] - 0.85).abs() < 1e-9);
    assert!((glow.size - 14.0).abs() < 1e-9);
    assert!((glow.spread - 0.25).abs() < 1e-9);
    assert!((glow.range - 0.75).abs() < 1e-9);
    let root_names: Vec<_> = crate::properties::root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(root_names.contains(&"ADBE Layer Styles"));
    assert!(!root_names.contains(&"ADBE Effect Parade"));

    let imported = to_structural_fx_document(&project, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let glow = object_with_type(&imported, "outerGlow").expect("editable Outer Glow effect");
    assert_eq!(glow["enabled"], json!(true));
    assert_eq!(glow["blendMode"], json!("screen"));
    assert!((glow["size"].as_f64().unwrap() - 14.0).abs() < 1e-9);
    assert!((glow["spread"].as_f64().unwrap() - 0.25).abs() < 1e-9);
    assert!((glow["range"].as_f64().unwrap() - 0.75).abs() < 1e-9);
}

#[test]
fn every_fx_representable_layer_style_exports_and_freshly_imports_edited_values() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([
        {"id":9101,"enabled":true,"effect":{"type":"dropShadow","enabled":false,"color":[0.1,0.2,0.3,0.4],"offset":[-6.0,8.0],"blurRadius":3.0,"spreadRadius":4.0,"blendMode":"multiply"}},
        {"id":9102,"enabled":true,"effect":{"type":"innerShadow","enabled":true,"color":[0.2,0.3,0.4,0.5],"offset":[-4.0,3.0],"size":9.0,"choke":0.2,"blendMode":"multiply"}},
        {"id":9103,"enabled":true,"effect":{"type":"outerGlow","enabled":true,"color":[0.3,0.4,0.5,0.6],"size":10.0,"spread":0.3,"range":0.7,"blendMode":"screen"}},
        {"id":9104,"enabled":true,"effect":{"type":"innerGlow","enabled":true,"color":[0.4,0.5,0.6,0.7],"size":11.0,"choke":0.25,"range":0.65,"source":"center","blendMode":"screen"}},
        {"id":9105,"enabled":true,"effect":{"type":"bevelEmboss","enabled":true,"style":"outerBevel","technique":"chiselHard","depth":1.8,"direction":"down","size":12.0,"soften":2.0,"angle":35.0,"altitude":50.0,"highlightColor":[0.8,0.7,0.6,0.5],"shadowColor":[0.2,0.1,0.3,0.4]}},
        {"id":9106,"enabled":true,"effect":{"type":"satin","enabled":true,"color":[0.1,0.3,0.5,0.7],"offset":[-9.0,12.0],"size":13.0,"invert":false,"blendMode":"multiply"}},
        {"id":9107,"enabled":true,"effect":{"type":"gradientOverlay","enabled":true,"opacity":0.8,"gradientType":"linear","start":[0.0,0.0],"end":[1.0,0.0],"stops":[{"offset":0.0,"color":[0.9,0.1,0.2,0.75]},{"offset":1.0,"color":[0.9,0.1,0.2,0.75]}],"blendMode":"normal"}},
        {"id":9108,"enabled":true,"effect":{"type":"gradientOverlay","enabled":true,"opacity":0.55,"gradientType":"radial","start":[10.0,20.0],"end":[70.0,80.0],"stops":[{"offset":0.0,"color":[1.0,0.0,0.0,1.0]},{"offset":1.0,"color":[0.0,0.0,1.0,0.5]}],"blendMode":"screen"}},
        {"id":9109,"enabled":true,"effect":{"type":"stroke","enabled":true,"color":[0.2,0.8,0.4,0.65],"width":7.0,"position":"inside","blendMode":"normal"}}
    ]);
    let output = export(input);
    let project = read_project(&output.bytes).unwrap();
    let decoded = layers(&project)
        .iter()
        .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
        .find(|decoded| decoded.styles.len() == 9)
        .unwrap_or_else(|| {
            panic!(
                "nine native styles missing: {:?}; decoded={:?}",
                output.diagnostics,
                layers(&project)
                    .iter()
                    .map(|layer| {
                        let decoded = crate::layer_styles::read(&layer.content, [320.0, 180.0]);
                        (layer.name.clone(), decoded.styles.len(), decoded.warnings)
                    })
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        decoded.warnings.iter().all(|warning| {
            !warning.contains("malformed") && !warning.contains("unknown native control")
        }),
        "{:?}",
        decoded.warnings
    );
    assert!(
        decoded
            .warnings
            .iter()
            .any(|warning| warning.contains("Inner Shadow"))
    );
    assert!(
        decoded
            .warnings
            .iter()
            .any(|warning| warning.contains("Inner Glow"))
    );
    assert!(
        decoded
            .warnings
            .iter()
            .any(|warning| warning.contains("Bevel and Emboss"))
    );
    assert!(
        decoded
            .warnings
            .iter()
            .any(|warning| warning.contains("Satin"))
    );
    assert!(matches!(
        decoded.styles[0],
        crate::layer_styles::NativeLayerStyle::DropShadow(_)
    ));
    assert!(matches!(
        decoded.styles[1],
        crate::layer_styles::NativeLayerStyle::InnerShadow(_)
    ));
    assert!(matches!(
        decoded.styles[2],
        crate::layer_styles::NativeLayerStyle::OuterGlow(_)
    ));
    assert!(matches!(
        decoded.styles[3],
        crate::layer_styles::NativeLayerStyle::InnerGlow(_)
    ));
    assert!(matches!(
        decoded.styles[4],
        crate::layer_styles::NativeLayerStyle::BevelEmboss(_)
    ));
    assert!(matches!(
        decoded.styles[5],
        crate::layer_styles::NativeLayerStyle::Satin(_)
    ));
    assert!(matches!(
        decoded.styles[6],
        crate::layer_styles::NativeLayerStyle::ColorOverlay(_)
    ));
    assert!(matches!(
        decoded.styles[7],
        crate::layer_styles::NativeLayerStyle::GradientOverlay(_)
    ));
    assert!(matches!(
        decoded.styles[8],
        crate::layer_styles::NativeLayerStyle::Stroke(_)
    ));

    let imported = to_structural_fx_document(&project, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut shadows = Vec::new();
    objects_with_type(&imported, "dropShadow", &mut shadows);
    assert_eq!(shadows.len(), 1);
    assert_eq!(shadows[0]["enabled"], json!(false));
    assert_eq!(shadows[0]["blendMode"], json!("multiply"));
    assert!((shadows[0]["blurRadius"].as_f64().unwrap() - 3.0).abs() < 1e-9);
    assert!((shadows[0]["spreadRadius"].as_f64().unwrap() - 4.0).abs() < 1e-9);
    let shadow_offset = shadows[0]["offset"].as_array().unwrap();
    assert!((shadow_offset[0].as_f64().unwrap() + 6.0).abs() < 1e-9);
    assert!((shadow_offset[1].as_f64().unwrap() - 8.0).abs() < 1e-9);

    let mut inner_shadows = Vec::new();
    objects_with_type(&imported, "innerShadow", &mut inner_shadows);
    assert_eq!(inner_shadows.len(), 1);
    assert!((inner_shadows[0]["size"].as_f64().unwrap() - 9.0).abs() < 1e-9);
    assert!((inner_shadows[0]["choke"].as_f64().unwrap() - 0.2).abs() < 1e-9);
    let inner_offset = inner_shadows[0]["offset"].as_array().unwrap();
    assert!((inner_offset[0].as_f64().unwrap() + 4.0).abs() < 1e-9);
    assert!((inner_offset[1].as_f64().unwrap() - 3.0).abs() < 1e-9);

    let mut outer_glows = Vec::new();
    objects_with_type(&imported, "outerGlow", &mut outer_glows);
    assert_eq!(outer_glows.len(), 1);
    assert!((outer_glows[0]["size"].as_f64().unwrap() - 10.0).abs() < 1e-9);
    assert!((outer_glows[0]["spread"].as_f64().unwrap() - 0.3).abs() < 1e-9);

    let mut inner_glows = Vec::new();
    objects_with_type(&imported, "innerGlow", &mut inner_glows);
    assert_eq!(inner_glows.len(), 1);
    assert_eq!(inner_glows[0]["source"], json!("center"));
    assert!((inner_glows[0]["range"].as_f64().unwrap() - 0.65).abs() < 1e-9);

    let mut bevels = Vec::new();
    objects_with_type(&imported, "bevelEmboss", &mut bevels);
    assert_eq!(bevels.len(), 1);
    assert_eq!(bevels[0]["style"], json!("outerBevel"));
    assert_eq!(bevels[0]["technique"], json!("chiselHard"));
    assert_eq!(bevels[0]["direction"], json!("down"));
    assert!((bevels[0]["depth"].as_f64().unwrap() - 1.8).abs() < 1e-9);
    assert!((bevels[0]["angle"].as_f64().unwrap() - 35.0).abs() < 1e-9);

    let mut satins = Vec::new();
    objects_with_type(&imported, "satin", &mut satins);
    assert_eq!(satins.len(), 1);
    assert_eq!(satins[0]["invert"], json!(false));
    let satin_offset = satins[0]["offset"].as_array().unwrap();
    assert!((satin_offset[0].as_f64().unwrap() + 9.0).abs() < 1e-9);
    assert!((satin_offset[1].as_f64().unwrap() - 12.0).abs() < 1e-9);
    assert!((satins[0]["size"].as_f64().unwrap() - 13.0).abs() < 1e-9);

    let mut overlays = Vec::new();
    objects_with_type(&imported, "gradientOverlay", &mut overlays);
    assert_eq!(overlays.len(), 2);
    assert!(overlays.iter().any(|overlay| {
        let color = overlay["stops"][0]["color"].as_array().unwrap();
        (color[0].as_f64().unwrap() - 0.9).abs() < 1e-9
            && (color[1].as_f64().unwrap() - 0.1).abs() < 1e-9
            && (color[2].as_f64().unwrap() - 0.2).abs() < 1e-9
            && (color[3].as_f64().unwrap() - 0.6).abs() < 1e-9
    }));
    assert!(overlays.iter().any(|overlay| {
        let start = overlay["start"].as_array().unwrap();
        let end = overlay["end"].as_array().unwrap();
        overlay["gradientType"] == json!("radial")
            && (start[0].as_f64().unwrap() - 10.0).abs() < 1e-9
            && (start[1].as_f64().unwrap() - 20.0).abs() < 1e-9
            && (end[0].as_f64().unwrap() - 70.0).abs() < 1e-9
            && (end[1].as_f64().unwrap() - 80.0).abs() < 1e-9
            && (overlay["opacity"].as_f64().unwrap() - 0.55).abs() < 1e-9
    }));

    let mut strokes = Vec::new();
    objects_with_type(&imported, "stroke", &mut strokes);
    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0]["position"], json!("inside"));
    assert!((strokes[0]["width"].as_f64().unwrap() - 7.0).abs() < 1e-9);
}

#[test]
fn all_gradient_overlay_kinds_roundtrip_through_native_ordinals() {
    for (kind, ordinal) in [
        ("linear", 1.0),
        ("radial", 2.0),
        ("conic", 3.0),
        ("reflected", 4.0),
    ] {
        let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
        input["composition"]["layers"][0]["effects"] = json!([{
            "id": 9240,
            "enabled": true,
            "effect": {
                "type":"gradientOverlay", "enabled":true, "opacity":0.65,
                "gradientType":kind, "start":[10.0,20.0], "end":[70.0,80.0],
                "stops":[
                    {"offset":0.0,"color":[1.0,0.0,0.0,1.0]},
                    {"offset":1.0,"color":[0.0,0.0,1.0,0.5]}
                ],
                "blendMode":"screen"
            }
        }]);

        let output = export(input);
        let project = read_project(&output.bytes).expect("fresh Gradient Overlay export");
        let decoded = layers(&project)
            .iter()
            .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
            .find(|decoded| !decoded.styles.is_empty())
            .expect("native Gradient Overlay");
        let native_kind = decoded.source_properties[0]
            .iter()
            .find(|(name, _)| name == "gradientFill/type")
            .and_then(|(_, property)| property.values.first())
            .copied();
        assert_eq!(native_kind, Some(ordinal), "{kind}");
        let [crate::layer_styles::NativeLayerStyle::GradientOverlay(overlay)] =
            decoded.styles.as_slice()
        else {
            panic!("one native Gradient Overlay for {kind}")
        };
        for (actual, expected) in overlay.start.into_iter().zip([10.0, 20.0]) {
            assert!((actual - expected).abs() < 1e-9, "{kind}");
        }
        for (actual, expected) in overlay.end.into_iter().zip([70.0, 80.0]) {
            assert!((actual - expected).abs() < 1e-9, "{kind}");
        }

        let imported = to_structural_fx_document(&project, Some(1))
            .expect("fresh Gradient Overlay import")
            .document
            .to_json_value()
            .unwrap();
        let overlay = object_with_type(&imported, "gradientOverlay")
            .unwrap_or_else(|| panic!("editable Gradient Overlay for {kind}"));
        assert_eq!(overlay["gradientType"], json!(kind));
        assert_point(overlay["start"].as_array().unwrap(), [10.0, 20.0], kind);
        assert_point(overlay["end"].as_array().unwrap(), [70.0, 80.0], kind);
    }
}

#[test]
fn drop_shadow_zero_spread_preserves_blur_radius_animation_bidirectionally() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([{
        "id": 9250,
        "enabled": true,
        "effect": {"type":"dropShadow","enabled":true,"color":[0.1,0.2,0.3,0.7],"offset":[3.0,4.0],"blurRadius":3.0,"spreadRadius":0.0,"blendMode":"normal"}
    }]);
    let mut entry = serde_json::to_value(keyed_entry(
        LayerId::new(900),
        PropType::Rotation,
        [
            (0, PropertyValue::Float(3.0)),
            (1_000, PropertyValue::Float(7.0)),
        ],
    ))
    .unwrap();
    entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(9250),
        "blurRadius",
    ))
    .unwrap();
    input["composition"]["dynamics"]["entries"] = json!([entry]);

    let output = export(input);
    let project = read_project(&output.bytes).expect("fresh Drop Shadow export");
    let decoded = layers(&project)
        .iter()
        .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
        .find(|decoded| !decoded.styles.is_empty())
        .expect("native Drop Shadow");
    let blur = decoded.source_properties[0]
        .iter()
        .find(|(name, _)| name == "dropShadow/blur")
        .map(|(_, value)| value)
        .expect("native Size property");
    assert!(blur.animated);
    assert_eq!(blur.keyframes.len(), 2);
    assert_eq!(blur.keyframes[0].values, vec![6.0]);
    assert_eq!(blur.keyframes[1].values, vec![14.0]);

    let imported = to_structural_fx_document(&project, Some(1))
        .expect("fresh Drop Shadow import")
        .document
        .to_json_value()
        .unwrap();
    let entry = imported["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["paramName"] == "blurRadius")
        .expect("editable blurRadius animation");
    let keys = entry["animator"]["keyframes"]
        .as_array()
        .expect("blurRadius keys");
    assert_eq!(keys[0]["value"], json!({"type":"float","value":3.0}));
    assert_eq!(keys[1]["value"], json!({"type":"float","value":7.0}));
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[1]["layerTime"], 1_000);
}

#[test]
fn drop_shadow_spread_animation_omits_both_coupled_native_tracks() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([{
        "id": 9250,
        "enabled": true,
        "effect": {"type":"dropShadow","enabled":true,"color":[0.1,0.2,0.3,0.7],"offset":[3.0,4.0],"blurRadius":3.0,"spreadRadius":0.0,"blendMode":"normal"}
    }]);
    input["composition"]["dynamics"]["entries"] = json!([
        effect_animation(
            9250,
            "blurRadius",
            [
                (0, PropertyValue::Float(3.0)),
                (1_000, PropertyValue::Float(7.0)),
            ],
        ),
        effect_animation(
            9250,
            "spreadRadius",
            [
                (0, PropertyValue::Float(0.0)),
                (1_000, PropertyValue::Float(4.0)),
            ],
        ),
    ]);

    let output = export(input);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("blurRadius + spreadRadius")
            && diagnostic.message.contains("both animations omitted")
    }));
    let project = read_project(&output.bytes).expect("fresh Drop Shadow export");
    let decoded = layers(&project)
        .iter()
        .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
        .find(|decoded| !decoded.styles.is_empty())
        .expect("native Drop Shadow");
    let blur = decoded.source_properties[0]
        .iter()
        .find(|(name, _)| name == "dropShadow/blur")
        .map(|(_, value)| value)
        .expect("native Size property");
    assert!(!blur.animated);
    assert_eq!(blur.values, vec![6.0]);
}

#[test]
fn disabled_zero_spread_preserves_coupled_drop_shadow_blur_animation() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([{
        "id": 9250,
        "enabled": true,
        "effect": {"type":"dropShadow","enabled":true,"color":[0.1,0.2,0.3,0.7],"offset":[3.0,4.0],"blurRadius":3.0,"spreadRadius":5.0,"blendMode":"normal"}
    }]);
    input["composition"]["dynamics"]["entries"] = json!([
        effect_animation(
            9250,
            "blurRadius",
            [
                (0, PropertyValue::Float(3.0)),
                (1_000, PropertyValue::Float(7.0)),
            ],
        ),
        disabled_effect_animation(
            9250,
            "spreadRadius",
            [
                (0, PropertyValue::Float(2.0)),
                (1_000, PropertyValue::Float(4.0)),
            ],
            PropertyValue::Float(0.0),
        ),
    ]);

    let output = export(input);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("runtime-visible disabledValue is normalized to a native constant")
    }));
    let project = read_project(&output.bytes).expect("fresh Drop Shadow export");
    let decoded = layers(&project)
        .iter()
        .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
        .find(|decoded| !decoded.styles.is_empty())
        .expect("native Drop Shadow");
    let [crate::layer_styles::NativeLayerStyle::DropShadow(shadow)] = decoded.styles.as_slice()
    else {
        panic!("one native Drop Shadow")
    };
    assert_eq!(shadow.spread, 0.0);
    let blur = decoded.source_properties[0]
        .iter()
        .find(|(name, _)| name == "dropShadow/blur")
        .map(|(_, value)| value)
        .expect("native Size property");
    assert!(blur.animated);
    assert_eq!(blur.keyframes[0].values, [6.0]);
    assert_eq!(blur.keyframes[1].values, [14.0]);
}

#[test]
fn disabled_coupled_shadow_blur_uses_effective_static_value() {
    let effect = serde_json::from_value(json!({
        "type":"dropShadow", "blurRadius":3.0, "spreadRadius":4.0
    }))
    .unwrap();
    let dynamics = vec![
        serde_json::from_value(disabled_effect_animation(
            9250,
            "blurRadius",
            [
                (0, PropertyValue::Float(1.0)),
                (1_000, PropertyValue::Float(2.0)),
            ],
            PropertyValue::Float(7.0),
        ))
        .unwrap(),
    ];
    let lowered = crate::export_document::layer_styles::lower(
        &effect,
        true,
        Some(fx_schema::EffectId::new(9250)),
        &dynamics,
        [320.0, 180.0],
    );
    let Some(crate::layer_styles::NativeLayerStyle::DropShadow(shadow)) = lowered.style else {
        panic!("native shadow: {:?}", lowered.warnings);
    };
    assert_eq!(shadow.size, 7.0);
    assert_eq!(shadow.spread, 4.0);
    assert!(shadow.animations.is_empty());
}

#[test]
fn disabled_nonzero_spread_retains_diagnosed_coupled_style_fallback() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([{
        "id": 9250,
        "enabled": true,
        "effect": {"type":"dropShadow","enabled":true,"color":[0.1,0.2,0.3,0.7],"offset":[3.0,4.0],"blurRadius":3.0,"spreadRadius":0.0,"blendMode":"normal"}
    }]);
    input["composition"]["dynamics"]["entries"] = json!([
        effect_animation(
            9250,
            "blurRadius",
            [
                (0, PropertyValue::Float(3.0)),
                (1_000, PropertyValue::Float(7.0)),
            ],
        ),
        disabled_effect_animation(
            9250,
            "spreadRadius",
            [
                (0, PropertyValue::Float(0.0)),
                (1_000, PropertyValue::Float(8.0)),
            ],
            PropertyValue::Float(4.0),
        ),
    ]);

    let output = export(input);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("blur animation with nonzero spread")
            && diagnostic.message.contains("authored bases retained")
    }));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("runtime-visible disabledValue is normalized to a native constant")
    }));
    let project = read_project(&output.bytes).expect("fresh Drop Shadow export");
    let decoded = layers(&project)
        .iter()
        .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
        .find(|decoded| !decoded.styles.is_empty())
        .expect("native Drop Shadow");
    let [crate::layer_styles::NativeLayerStyle::DropShadow(shadow)] = decoded.styles.as_slice()
    else {
        panic!("one native Drop Shadow")
    };
    assert_eq!(shadow.spread, 4.0);
    let blur = decoded.source_properties[0]
        .iter()
        .find(|(name, _)| name == "dropShadow/blur")
        .map(|(_, value)| value)
        .expect("native Size property");
    assert!(!blur.animated);
}

#[test]
fn animated_layer_style_percentages_use_native_percentage_units() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([
        {"id":9301,"enabled":true,"effect":{"type":"innerShadow","enabled":true,"color":[0.2,0.3,0.4,0.5],"offset":[-4.0,3.0],"size":9.0,"choke":0.2,"blendMode":"multiply"}},
        {"id":9302,"enabled":true,"effect":{"type":"outerGlow","enabled":true,"color":[0.3,0.4,0.5,0.6],"size":10.0,"spread":0.3,"range":0.7,"blendMode":"screen"}},
        {"id":9303,"enabled":true,"effect":{"type":"innerGlow","enabled":true,"color":[0.4,0.5,0.6,0.7],"size":11.0,"choke":0.25,"range":0.65,"source":"center","blendMode":"screen"}},
        {"id":9304,"enabled":true,"effect":{"type":"bevelEmboss","enabled":true,"style":"outerBevel","technique":"chiselHard","depth":1.8,"direction":"down","size":12.0,"soften":2.0,"angle":35.0,"altitude":50.0,"highlightColor":[0.8,0.7,0.6,0.5],"shadowColor":[0.2,0.1,0.3,0.4]}},
        {"id":9305,"enabled":true,"effect":{"type":"gradientOverlay","enabled":true,"opacity":0.55,"gradientType":"radial","start":[10.0,20.0],"end":[70.0,80.0],"stops":[{"offset":0.0,"color":[1.0,0.0,0.0,1.0]},{"offset":1.0,"color":[0.0,0.0,1.0,0.5]}],"blendMode":"screen"}}
    ]);
    let cases = [
        (
            9301,
            "choke",
            "innerShadow/chokeMatte",
            [0.2, 0.4],
            [20.0, 40.0],
        ),
        (
            9302,
            "spread",
            "outerGlow/chokeMatte",
            [0.3, 0.6],
            [30.0, 60.0],
        ),
        (
            9302,
            "range",
            "outerGlow/inputRange",
            [0.7, 0.9],
            [70.0, 90.0],
        ),
        (
            9303,
            "choke",
            "innerGlow/chokeMatte",
            [0.25, 0.5],
            [25.0, 50.0],
        ),
        (
            9303,
            "range",
            "innerGlow/inputRange",
            [0.65, 0.85],
            [65.0, 85.0],
        ),
        (
            9304,
            "depth",
            "bevelEmboss/strengthRatio",
            [1.8, 2.2],
            [180.0, 220.0],
        ),
        (
            9305,
            "opacity",
            "gradientFill/opacity",
            [0.55, 0.75],
            [55.0, 75.0],
        ),
    ];
    input["composition"]["dynamics"]["entries"] = Value::Array(
        cases
            .iter()
            .map(|(id, param, _, fx_values, _)| {
                effect_animation(
                    *id,
                    param,
                    [
                        (0, PropertyValue::Float(fx_values[0])),
                        (1_000, PropertyValue::Float(fx_values[1])),
                    ],
                )
            })
            .collect(),
    );

    let output = export(input);
    let project = read_project(&output.bytes).expect("fresh Layer Styles export");
    let decoded = layers(&project)
        .iter()
        .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
        .find(|decoded| decoded.styles.len() == 5)
        .expect("five native Layer Styles");
    for (_, _, property_name, _, native_values) in &cases {
        let property = decoded
            .source_properties
            .iter()
            .flatten()
            .find(|(name, _)| name == property_name)
            .map(|(_, property)| property)
            .unwrap_or_else(|| panic!("missing native {property_name}"));
        assert!(property.animated, "{property_name}");
        assert_eq!(
            property.keyframes[0].values,
            [native_values[0]],
            "{property_name}"
        );
        assert_eq!(
            property.keyframes[1].values,
            [native_values[1]],
            "{property_name}"
        );
    }

    let imported = to_structural_fx_document(&project, Some(1))
        .expect("fresh Layer Styles import")
        .document
        .to_json_value()
        .unwrap();
    let entries = imported["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (_, param, _, fx_values, _) in &cases {
        assert!(
            entries.iter().any(|entry| {
                if entry["target"]["paramName"] != *param {
                    return false;
                }
                let Some(keys) = entry["animator"]["keyframes"].as_array() else {
                    return false;
                };
                keys.len() == 2
                    && keys[0]["value"]["type"] == "float"
                    && keys[1]["value"]["type"] == "float"
                    && keys[0]["value"]["value"]
                        .as_f64()
                        .is_some_and(|value| (value - fx_values[0]).abs() < 1e-9)
                    && keys[1]["value"]["value"]
                        .as_f64()
                        .is_some_and(|value| (value - fx_values[1]).abs() < 1e-9)
            }),
            "missing editable {param} keys {fx_values:?}"
        );
    }
}

#[test]
fn constant_color_overlay_opacity_combines_stop_alpha_and_stays_finite() {
    for (alpha, expected) in [(0.5, [20.0, 40.0]), (0.0, [0.0, 0.0])] {
        let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
        input["composition"]["layers"][0]["effects"] = json!([{
            "id": 9401,
            "enabled": true,
            "effect": {
                "type":"gradientOverlay", "enabled":true, "opacity":0.4,
                "gradientType":"linear", "start":[0.0,0.0], "end":[1.0,0.0],
                "stops":[
                    {"offset":0.0,"color":[0.9,0.1,0.2,alpha]},
                    {"offset":1.0,"color":[0.9,0.1,0.2,alpha]}
                ],
                "blendMode":"normal"
            }
        }]);
        input["composition"]["dynamics"]["entries"] = json!([effect_animation(
            9401,
            "opacity",
            [
                (0, PropertyValue::Float(0.4)),
                (1_000, PropertyValue::Float(0.8)),
            ],
        )]);

        let output = export(input);
        let project = read_project(&output.bytes).expect("fresh Color Overlay export");
        let decoded = layers(&project)
            .iter()
            .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
            .find(|decoded| !decoded.styles.is_empty())
            .expect("native Color Overlay");
        let opacity = decoded.source_properties[0]
            .iter()
            .find(|(name, _)| name == "solidFill/opacity")
            .map(|(_, property)| property)
            .expect("native Color Overlay opacity");
        assert!(opacity.animated, "alpha={alpha}");
        assert_eq!(opacity.keyframes[0].values, [expected[0]], "alpha={alpha}");
        assert_eq!(opacity.keyframes[1].values, [expected[1]], "alpha={alpha}");
        assert!(
            opacity
                .keyframes
                .iter()
                .flat_map(|key| &key.values)
                .all(|value| value.is_finite()),
            "alpha={alpha}"
        );
    }
}

#[test]
fn delayed_stretched_layer_style_keys_roundtrip_once_through_source_clock() {
    let output = export(clocked_drop_shadow_group(
        fixture_linear_playback(
            json!({"start":500,"duration":1000}),
            json!({"start":0,"duration":500}),
        ),
        false,
    ));
    let project = read_project(&output.bytes).expect("fresh clocked Drop Shadow export");
    let owner = layers(&project)
        .iter()
        .find(|layer| layer.name.as_ref() == "Clocked style owner")
        .expect("clocked style owner");
    assert!((owner.record.start_time().unwrap() - 0.5).abs() < 1e-9);
    assert!((owner.record.stretch().unwrap() - 2.0).abs() < 1e-9);
    let decoded = crate::layer_styles::read(&owner.content, [320.0, 180.0]);
    let blur = decoded.source_properties[0]
        .iter()
        .find(|(name, _)| name == "dropShadow/blur")
        .map(|(_, value)| value)
        .expect("native Size property");
    assert_eq!(
        blur.keyframes
            .iter()
            .map(|key| key.time_secs)
            .collect::<Vec<_>>(),
        [0.0, 0.5]
    );

    let imported = to_structural_fx_document(&project, Some(1))
        .expect("fresh clocked Drop Shadow import")
        .document
        .to_json_value()
        .unwrap();
    let entry = imported["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["paramName"] == "blurRadius")
        .expect("editable blurRadius animation");
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys[0]["layerTime"], 500);
    assert_eq!(keys[1]["layerTime"], 1_500);
}

#[test]
fn nonlinear_time_remap_keeps_style_owner_sibling_and_static_base() {
    let playback = json!({
        "keyframes":[
            {"id":"guard-before","time":0,"value":0,"easing":{"type":"linear"}},
            {"id":"guard-after","time":2000,"value":1000,"easing":{"type":"linear"}}
        ],
        "before":"inactive", "after":"inactive"
    });
    let output = export(clocked_drop_shadow_group(
        fixture_remapped_playback(json!({"start":500,"duration":1000}), playback),
        true,
    ));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("nonlinear Time Remap")
            && diagnostic.message.contains("authored base")
    }));
    let project = read_project(&output.bytes).expect("fresh time-remapped Drop Shadow export");
    assert!(
        layers(&project)
            .iter()
            .any(|layer| layer.name.as_ref() == "Clock sibling")
    );
    let owner = layers(&project)
        .iter()
        .find(|layer| layer.name.as_ref() == "Clocked style owner")
        .expect("time-remapped style owner");
    let decoded = crate::layer_styles::read(&owner.content, [320.0, 180.0]);
    let [crate::layer_styles::NativeLayerStyle::DropShadow(shadow)] = decoded.styles.as_slice()
    else {
        panic!("one static native Drop Shadow")
    };
    assert_eq!(shadow.size, 3.0);
    assert_eq!(shadow.spread, 0.0);
    let blur = decoded.source_properties[0]
        .iter()
        .find(|(name, _)| name == "dropShadow/blur")
        .map(|(_, value)| value)
        .expect("native Size property");
    assert!(!blur.animated);
    assert_eq!(blur.values, vec![6.0]);
}

#[test]
fn duplicate_and_animated_styles_keep_first_static_base_with_diagnostics() {
    let mut input: Value = serde_json::from_str(TEMPLATE).unwrap();
    input["composition"]["layers"][0]["effects"] = json!([
        {"id":9201,"enabled":true,"effect":{"type":"stroke","enabled":true,"color":[1.0,0.0,0.0,1.0],"width":5.0,"position":"outside","blendMode":"normal"}},
        {"id":9202,"enabled":true,"effect":{"type":"stroke","enabled":true,"color":[0.0,1.0,0.0,1.0],"width":40.0,"position":"inside","blendMode":"normal"}}
    ]);
    let entry = keyed_entry(
        LayerId::new(9991),
        PropType::Rotation,
        [
            (0, PropertyValue::Float(5.0)),
            (1_000, PropertyValue::Float(12.0)),
        ],
    );
    let mut entry = serde_json::to_value(entry).unwrap();
    entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(9201),
        "width",
    ))
    .unwrap();
    input["composition"]["dynamics"]["entries"] = json!([entry]);
    let output = export(input);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("duplicate FX occurrence"))
    );
    let project = read_project(&output.bytes).unwrap();
    let decoded = layers(&project)
        .iter()
        .map(|layer| crate::layer_styles::read(&layer.content, [320.0, 180.0]))
        .find(|decoded| !decoded.styles.is_empty())
        .expect("Layer Styles group");
    assert!(decoded.warnings.iter().any(|warning| {
        warning.contains("frameFX/size") && warning.contains("retains the initial value")
    }));
    let stroke = decoded
        .styles
        .into_iter()
        .find_map(|style| match style {
            crate::layer_styles::NativeLayerStyle::Stroke(stroke) => Some(stroke),
            _ => None,
        })
        .expect("first native Stroke");
    assert_eq!(stroke.size, 5.0);
    assert_eq!(stroke.color, [1.0, 0.0, 0.0, 1.0]);
}
