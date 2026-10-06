//! Native-backed controls plus supplementary edited-FX domain exclusions.
//! Fresh AEP structure is not independent native-render or edit-propagation proof.
use super::*;
use crate::effects::native::read_effects;

fn imported_mosaic(value: &Value) -> Option<&Value> {
    if value.get("type").and_then(Value::as_str) == Some("mosaic") {
        return Some(value);
    }
    match value {
        Value::Object(object) => object.values().find_map(imported_mosaic),
        Value::Array(array) => array.iter().find_map(imported_mosaic),
        _ => None,
    }
}

fn input() -> Value {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/effects/catalog.aep"
    ))
    .unwrap();
    let mut input = to_structural_fx_document(&native, Some(58))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    // Keep the independently authored Mosaic controls; move them onto an
    // explicit root Adjustment to exercise the bounded export profile.
    let mosaic = imported_mosaic(&input)
        .expect("native-backed imported Mosaic")
        .clone();
    assert_eq!(mosaic["sharpColors"], false);
    assert_eq!(mosaic["horizontalBlocks"], 10.0);
    assert_eq!(mosaic["verticalBlocks"], 10.0);
    let effects = json!([{"id":904,"enabled":true,"effect":mosaic}]);
    let mut leaf = rect(&imported(), 901);
    leaf["rect"]["size"] = json!([320, 180]);
    leaf["rect"]["position"] = json!([0, 0]);
    leaf["rect"]["roundness"] = json!(0);
    leaf["rect"]["strokeEnabled"] = json!(false);
    leaf["effects"] = json!([]);
    leaf["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    input["composition"]["layers"] = json!([
        {"type":"Adjustment", "id":900, "name":"root mosaic", "parent":null,
         "activeRange":{"start":0,"duration":2000}, "effects":effects,
         "transform":identity_fx_transform()},
        leaf
    ]);
    input["composition"]["dynamics"] = json!({"entries":[]});
    input["dimensions"] = json!({"width":320,"height":180});
    input.as_object_mut().unwrap().remove("backgroundColor");
    input
}

fn checkbox(output: &ExportedDocument) -> f64 {
    let project = read_project(&output.bytes).unwrap();
    let owner = layers(&project)
        .iter()
        .find(|layer| layer.name.as_ref() == "root mosaic")
        .unwrap();
    let (effects, _) = read_effects(&owner.content, [320.0, 180.0]);
    let mosaic = effects
        .iter()
        .find(|effect| effect.match_name == "ADBE Mosaic")
        .unwrap();
    mosaic
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == "ADBE Mosaic-0003")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap()
        .values[0]
}

fn count_keys(output: &ExportedDocument, match_name: &str) -> Vec<f64> {
    let project = read_project(&output.bytes).unwrap();
    let owner = project
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some(&composition.layers),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == "root mosaic")
        .unwrap();
    let (effects, _) = read_effects(&owner.content, [320.0, 180.0]);
    effects
        .iter()
        .find(|effect| effect.match_name == "ADBE Mosaic")
        .unwrap()
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == match_name)
        .unwrap()
        .numeric
        .as_ref()
        .unwrap()
        .keyframes
        .iter()
        .map(|key| key.values[0])
        .collect()
}

fn assert_preserved(value: Value) {
    let output = export(value);
    assert_eq!(checkbox(&output), 0.0);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic.message.contains("Mosaic")
            && diagnostic.message.contains("domain")
            && diagnostic.message.contains("retained")
    }));
}

#[test]
fn mosaic_root_canvas_normalizes_native_backed_controls_without_changing_keys_or_bypass() {
    for (sharp_colors, enabled) in [(false, true), (false, false), (true, true)] {
        let mut input = input();
        let effect_id = input["composition"]["layers"][0]["effects"][0]["id"]
            .as_u64()
            .unwrap();
        input["composition"]["layers"][0]["effects"][0]["effect"] = json!({
            "type":"mosaic", "horizontalBlocks":48, "verticalBlocks":27,
            "sharpColors":sharp_colors
        });
        input["composition"]["layers"][0]["effects"][0]["enabled"] = json!(enabled);
        input["composition"]["layers"][0]["effects"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id":902,"enabled":false,"effect":{"type":"gaussianBlur","blurriness":7}
            }));
        input["composition"]["dynamics"]["entries"] = json!([
            {"target":{"kind":"effectProperty","effectId":effect_id,"paramName":"horizontalBlocks"},
             "animator":{"type":"keyframes","enabled":true,"keyframes":[
                 {"id":"h0","layerTime":0,"value":{"type":"float","value":48},"easing":{"type":"linear"}},
                 {"id":"h1","layerTime":36,"value":{"type":"float","value":120},"easing":{"type":"hold"}}
             ]}},
            {"target":{"kind":"effectProperty","effectId":effect_id,"paramName":"verticalBlocks"},
             "animator":{"type":"keyframes","enabled":true,"keyframes":[
                 {"id":"v0","layerTime":0,"value":{"type":"float","value":27},"easing":{"type":"linear"}},
                 {"id":"v1","layerTime":36,"value":{"type":"float","value":68},"easing":{"type":"hold"}}
             ]}}
        ]);
        let output = export(input);
        assert_eq!(checkbox(&output), 1.0);
        let project = read_project(&output.bytes).unwrap();
        let (effects, _) = read_effects(&layers(&project)[0].content, [320.0, 180.0]);
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].match_name, "ADBE Mosaic");
        assert_eq!(effects[0].enabled, enabled);
        assert_eq!(effects[1].match_name, "ADBE Gaussian Blur 2");
        assert!(!effects[1].enabled);
        for (name, counts) in [
            ("ADBE Mosaic-0001", [48.0, 120.0]),
            ("ADBE Mosaic-0002", [27.0, 68.0]),
        ] {
            let track = effects[0]
                .parameters
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert_eq!(track.keyframes.len(), 2);
            assert_eq!(track.keyframes[0].values, [counts[0]]);
            assert_eq!(track.keyframes[1].values, [counts[1]]);
            assert!((track.keyframes[1].time_secs - 0.036).abs() < 0.002);
            assert_eq!(track.keyframes[0].out_interpolation, 3);
            assert_eq!(track.keyframes[1].in_interpolation, 3);
        }
        assert_eq!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("Sharp Colors set to on") }),
            !sharp_colors
        );
    }
}

#[test]
fn mosaic_constant_escaped_geometry_scales_count_keys_but_retains_authored_kernel() {
    let output = export(constant_escaped_input());
    assert_eq!(checkbox(&output), 0.0);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0001"), [43.0, 107.0]);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0002"), [24.0, 61.0]);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic.message.contains("approximate cell size")
            && diagnostic.message.contains("no grid-origin control")
            && diagnostic.message.contains("Sharp Colors retained")
    }));
}

fn constant_escaped_input() -> Value {
    let mut input = input();
    input["composition"]["layers"][0]["activeRange"]["duration"] = json!(100);
    input["composition"]["layers"][1]["rect"]["position"] = json!([-20, -10]);
    input["composition"]["layers"][1]["rect"]["size"] = json!([360, 200]);
    let effect_id = input["composition"]["layers"][0]["effects"][0]["id"]
        .as_u64()
        .unwrap();
    input["composition"]["layers"][0]["effects"][0]["effect"] = json!({
        "type":"mosaic", "horizontalBlocks":48, "verticalBlocks":27,
        "sharpColors":false
    });
    input["composition"]["dynamics"]["entries"] = json!([
        {"target":{"kind":"effectProperty","effectId":effect_id,"paramName":"horizontalBlocks"},
         "animator":{"type":"keyframes","enabled":true,"keyframes":[
             {"id":"h0","layerTime":0,"value":{"type":"float","value":48},"easing":{"type":"linear"}},
             {"id":"h1","layerTime":36,"value":{"type":"float","value":120},"easing":{"type":"hold"}}
         ]}},
        {"target":{"kind":"effectProperty","effectId":effect_id,"paramName":"verticalBlocks"},
         "animator":{"type":"keyframes","enabled":true,"keyframes":[
             {"id":"v0","layerTime":0,"value":{"type":"float","value":27},"easing":{"type":"linear"}},
             {"id":"v1","layerTime":36,"value":{"type":"float","value":68},"easing":{"type":"hold"}}
         ]}}
    ]);

    input
}

#[test]
fn mosaic_escaped_rect_stroke_includes_renderer_half_width_in_count_keys() {
    let mut input = constant_escaped_input();
    let rect = &mut input["composition"]["layers"][1]["rect"];
    rect["strokeEnabled"] = json!(true);
    rect["strokeColor"] = json!([0.2, 0.3, 0.4, 1.0]);
    rect["strokeWidth"] = json!(10);
    let output = export(input);
    // A 10px stroke makes the raw domain 370x210, not the fill's 360x200.
    assert_eq!(count_keys(&output, "ADBE Mosaic-0001"), [42.0, 104.0]);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0002"), [23.0, 58.0]);
    assert_eq!(checkbox(&output), 0.0);
}

#[test]
fn mosaic_root_canvas_rejects_three_d_owner_even_at_zero_depth() {
    for z in [0, 200] {
        let mut input = input();
        input["composition"]["layers"][0]["transform"]["position"] = json!([0, 0, z]);
        let mut above = input["composition"]["layers"][1].clone();
        above["id"] = json!(902);
        above["name"] = json!("escaped above");
        above["rect"]["position"] = json!([-160, 12]);
        above["rect"]["size"] = json!([30, 40]);
        above["transform"]["position"] = json!([0, 0, 100]);
        input["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .insert(0, above);
        // At z=0, FX depth sorting moves the declared-above Rect into the
        // backdrop. The declared lower suffix cannot certify that domain.
        // z=200 also stays outside the conservative TwoD-owner profile.
        assert_preserved(input);
    }
}

#[test]
fn mosaic_root_canvas_accepts_inert_nonidentity_two_d_owner_geometry() {
    let mut input = input();
    let transform = &mut input["composition"]["layers"][0]["transform"];
    transform["position"] = json!([40, -20]);
    transform["scale"] = json!([150, 75]);
    transform["rotation"] = json!(30);
    transform["skew"] = json!(12);
    transform["skewAxis"] = json!(25);
    transform["anchorPoint"] = json!([17, 9]);
    let mut above = input["composition"]["layers"][1].clone();
    above["id"] = json!(902);
    above["name"] = json!("escaped above");
    above["rect"]["position"] = json!([-160, 12]);
    above["rect"]["size"] = json!([30, 40]);
    above["transform"]["position"] = json!([0, 0, 100]);
    input["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, above);
    // A TwoD owner breaks the depth-sort bin; its local spatial geometry
    // does not transform the FX Adjustment's effect node or native solid.
    let output = export(input);
    assert_eq!(checkbox(&output), 1.0);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic.message.contains("Sharp Colors set to on")
    }));
}

#[test]
fn mosaic_escaped_raw_geometry_approximates_only_without_a_mask() {
    for masked in [false, true] {
        let mut input = input();
        input["composition"]["layers"][1]["rect"]["position"] = json!([-20, 0]);
        input["composition"]["layers"][1]["rect"]["size"] = json!([360, 180]);
        if masked {
            let masks: Value = serde_json::from_str(include_str!(
                "../../../tests/fixtures/adjustment/fx_export/adjustment-masks.fx.json"
            ))
            .unwrap();
            input["composition"]["layers"][1]["masks"] = masks["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|layer| layer.get("masks").is_some())
                .unwrap()["masks"]
                .clone();
            // Retain the real mask guide: its contained paint support must
            // not certify the escaped unmasked Rect's raw sampling geometry.
            let guide = masks["composition"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|layer| layer["type"] == "Shape")
                .unwrap()
                .clone();
            input["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .push(guide);
            assert_preserved(input);
        } else {
            let output = export(input);
            assert_eq!(checkbox(&output), 0.0);
            assert!(output.diagnostics.iter().any(|diagnostic| {
                diagnostic.layer_id == Some(LayerId::new(900))
                    && diagnostic.message.contains("block counts scaled")
            }));
        }
    }
}

#[test]
fn mosaic_domain_rejects_tilt_text_and_animated_geometry() {
    let mut tilted = input();
    tilted["composition"]["layers"][1]["transform"]["rotationY"] = json!(15);
    assert_preserved(tilted);

    let mut unknown = input();
    let mixed = mixed_scene_with_text_branch();
    let mut text = mixed["composition"]["layers"][0]["layers"][1]["layers"][0].clone();
    text["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    unknown["composition"]["layers"][1] = text;
    assert_preserved(unknown);

    let mut animated = input();
    animated["composition"]["layers"][1]["rect"]["size"] = json!([360, 180]);
    animated["composition"]["dynamics"]["entries"] = json!([keyed_entry(
        LayerId::new(901),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(0.0)),
            (1000, PropertyValue::Float(-20.0))
        ]
    )]);
    let output = export(animated);
    assert_eq!(checkbox(&output), 0.0);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic
                .message
                .contains("changes during the effect interval")
            && diagnostic.message.contains("authored counts")
    }));
}

#[test]
fn mosaic_nested_canvas_preserves_source_true_and_false() {
    for sharp_colors in [false, true] {
        let mut input = input();
        input["composition"]["layers"][0]["effects"][0]["effect"]["sharpColors"] =
            json!(sharp_colors);
        let mut group = imported()["composition"]["layers"][0].clone();
        group["id"] = json!(903);
        group["parent"] = Value::Null;
        group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
        group["layers"] = input["composition"]["layers"].clone();
        for child in group["layers"].as_array_mut().unwrap() {
            child["parent"] = json!(903);
        }
        input["composition"]["layers"] = json!([group]);
        let output = export(input);
        let project = read_project(&output.bytes).unwrap();
        let effects: Vec<_> = project
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(composition) => Some(&composition.layers),
                _ => None,
            })
            .flatten()
            .flat_map(|layer| read_effects(&layer.content, [320.0, 180.0]).0)
            .collect();
        let mosaic = effects
            .iter()
            .find(|effect| effect.match_name == "ADBE Mosaic")
            .unwrap();
        let value = mosaic
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Mosaic-0003")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values[0];
        assert_eq!(value, f64::from(sharp_colors));
    }
}

#[test]
fn mosaic_domain_rejects_gates_and_background_but_approximates_static_nested_planes() {
    let mut gated = input();
    gated["composition"]["layers"][0]["transform"]["opacity"] = json!(50);
    gated["composition"]["layers"][1]["rect"]["position"] = json!([-20, 0]);
    gated["composition"]["layers"][1]["rect"]["size"] = json!([360, 180]);
    assert_preserved(gated);
    let mut background = input();
    background["backgroundColor"] = json!([0.2, 0.3, 0.4, 1.0]);
    background["composition"]["layers"][1]["rect"]["position"] = json!([-20, 0]);
    background["composition"]["layers"][1]["rect"]["size"] = json!([360, 180]);
    assert_preserved(background);
    let mut nested = input();
    let mut group = imported()["composition"]["layers"][0].clone();
    group["id"] = json!(905);
    group["parent"] = Value::Null;
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    let mut child = nested["composition"]["layers"][1].clone();
    child["parent"] = json!(905);
    child["rect"]["position"] = json!([-20, 0]);
    child["rect"]["size"] = json!([360, 180]);
    group["layers"] = json!([child]);
    nested["composition"]["layers"][1] = group;
    let output = export(nested);
    assert_eq!(checkbox(&output), 0.0);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic.message.contains("block counts scaled")
    }));
}

#[test]
fn mosaic_domain_rejects_part_time_escaped_contributors() {
    let mut input = input();
    input["composition"]["layers"][0]["activeRange"]["duration"] = json!(100);
    let mut transient = input["composition"]["layers"][1].clone();
    transient["id"] = json!(902);
    transient["name"] = json!("part-time escaped Rect");
    transient["activeRange"] = json!({"start":50,"duration":50});
    transient["rect"]["position"] = json!([-20, 0]);
    transient["rect"]["size"] = json!([360, 180]);
    input["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(transient);

    let output = export(input);
    assert_eq!(checkbox(&output), 0.0);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic
                .message
                .contains("part-time geometry contributor")
            && diagnostic.message.contains("authored counts")
    }));
}

#[test]
fn mosaic_domain_excludes_unpainted_and_hidden_inputs() {
    for excluded in ["unpainted", "uncolored_stroke", "zero_stroke", "hidden"] {
        let mut input = input();
        let mut escaped = input["composition"]["layers"][1].clone();
        escaped["id"] = json!(903);
        escaped["name"] = json!("non-painting escaped Rect");
        escaped["rect"]["position"] = json!([-20, 0]);
        escaped["rect"]["size"] = json!([360, 180]);
        match excluded {
            "unpainted" => {
                escaped["rect"]["fillEnabled"] = json!(false);
                escaped["rect"]["strokeEnabled"] = json!(false);
            }
            "uncolored_stroke" => {
                escaped["rect"]["fillEnabled"] = json!(false);
                escaped["rect"]["strokeEnabled"] = json!(true);
                escaped["rect"]["strokeColor"] = Value::Null;
                escaped["rect"]["strokeWidth"] = json!(10);
            }
            "zero_stroke" => {
                escaped["rect"]["fillEnabled"] = json!(false);
                escaped["rect"]["strokeEnabled"] = json!(true);
                escaped["rect"]["strokeColor"] = json!([0.2, 0.3, 0.4, 1.0]);
                escaped["rect"]["strokeWidth"] = json!(0);
            }
            "hidden" => {
                escaped["isHidden"] = json!(true);
                escaped["activeRange"] = json!({"start":50,"duration":50});
            }
            _ => unreachable!(),
        }
        input["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(escaped);

        let output = export(input);
        assert_eq!(checkbox(&output), 1.0);
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(900))
                && diagnostic.message.contains("Sharp Colors set to on")
        }));
    }
}

fn assert_count_keys_retained(input: Value, reason: &str) {
    let output = export(input);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0001"), [48.0, 120.0]);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0002"), [27.0, 68.0]);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic.message.contains(reason)
            && diagnostic
                .message
                .contains("authored counts and Sharp Colors retained")
    }));
}

#[test]
fn mosaic_generated_empty_path_retains_counts_without_a_complete_domain() {
    let mut input = constant_escaped_input();
    input["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type":"Shape", "id":906, "name":"empty-path generated contributor",
            "parent":null, "activeRange":{"start":0,"duration":2000},
            "transform":{"anchorPoint":[0,0],"position":[800,0],
                         "scale":[100,100],"rotation":0,"opacity":100},
            "shape":{"path":{"commands":[]},
                     "ellipse":{"size":[400,240],"position":[0,90]},
                     "fills":[{"paint":{"type":"solid","color":[0.2,0.3,0.4,1.0]}}]}
        }));

    // Actual no-paint and plain static empty paths must not add an origin point
    // to the escaped Rect's domain or change its established count scaling.
    let mut unpainted = input.clone();
    unpainted["composition"]["layers"][2]["shape"]["fills"] = json!([]);
    let output = export(unpainted);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0001"), [43.0, 107.0]);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0002"), [24.0, 61.0]);

    let mut empty_static = input.clone();
    empty_static["composition"]["layers"][2]["shape"]["ellipse"] = Value::Null;
    let output = export(empty_static);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0001"), [43.0, 107.0]);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0002"), [24.0, 61.0]);

    assert_count_keys_retained(input, "generated or animated Shape geometry");
}

#[test]
fn mosaic_domain_rejects_cross_layer_inputs_including_hidden_referencers() {
    for hidden in [false, true] {
        let mut input = constant_escaped_input();
        let mut consumer = input["composition"]["layers"][1].clone();
        consumer["id"] = json!(902);
        consumer["isHidden"] = json!(hidden);
        consumer["layerMask"] = json!({"mode":"alpha","layer":901});
        input["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .insert(0, consumer);
        assert_count_keys_retained(input, "cross-layer sampling inputs");
    }
}

#[test]
fn mosaic_domain_rejects_temporal_effects_and_group_owned_transform_clocks() {
    let mut temporal = constant_escaped_input();
    temporal["composition"]["layers"][1]["effects"] = json!([
        {"id":907,"enabled":true,"effect":{"type":"posterizeTime","frameRate":12}}
    ]);
    assert_count_keys_retained(temporal, "temporal effects");

    let mut clocked = constant_escaped_input();
    clocked["composition"]["layers"][0]["activeRange"] = json!({"start":1000,"duration":100});
    let mut group = imported()["composition"]["layers"][0].clone();
    group["id"] = json!(905);
    group["parent"] = Value::Null;
    group["playback"] = fixture_linear_playback(
        json!({"start":1000,"duration":1000}),
        json!({"start":0,"duration":1000}),
    );
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    let mut child = clocked["composition"]["layers"][1].clone();
    child["parent"] = json!(905);
    group["layers"] = json!([child]);
    clocked["composition"]["layers"][1] = group;
    let mut entry = serde_json::to_value(keyed_entry(
        LayerId::new(905),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(0.0)),
            (500, PropertyValue::Float(-20.0)),
        ],
    ))
    .unwrap();
    entry["animator"]["keyframes"][0]["easing"] = json!({"type":"hold"});
    clocked["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(entry);
    assert_count_keys_retained(clocked, "changes during the effect interval");
}

#[test]
fn mosaic_domain_rejects_skew_and_non_quarter_turn_reboxing() {
    for (property, value) in [("rotation", 30), ("skew", 12)] {
        let mut input = constant_escaped_input();
        input["composition"]["layers"][1]["transform"][property] = json!(value);
        assert_count_keys_retained(input, "skew or non-quarter-turn rotation");
    }
}

#[test]
fn mosaic_nested_owner_scales_constant_escaped_count_keys_without_cross_layer_inputs() {
    let mut input = constant_escaped_input();
    let mut group = imported()["composition"]["layers"][0].clone();
    group["id"] = json!(905);
    group["parent"] = Value::Null;
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    group["layers"] = input["composition"]["layers"].clone();
    for child in group["layers"].as_array_mut().unwrap() {
        child["parent"] = json!(905);
    }
    input["composition"]["layers"] = json!([group]);
    let output = export(input);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0001"), [43.0, 107.0]);
    assert_eq!(count_keys(&output, "ADBE Mosaic-0002"), [24.0, 61.0]);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(900))
            && diagnostic.message.contains("block counts scaled")
            && diagnostic.message.contains("Sharp Colors retained")
    }));
}

#[test]
fn mosaic_root_canvas_plain_path_uses_raw_control_points_not_curve_or_paint_bounds() {
    for escaped_control in [false, true] {
        let mut input = input();
        let shapes: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-masks.fx.json"
        ))
        .unwrap();
        let mut shape = shapes["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Shape")
            .unwrap()
            .clone();
        shape["id"] = json!(906);
        shape["parent"] = Value::Null;
        shape["effects"] = json!([]);
        shape["shape"]["fills"] = json!([{"paint":{"type":"solid","color":[0.2,0.3,0.4,1.0]}}]);
        shape["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
        shape["shape"]["path"]["commands"] = json!([
            {"type":"moveTo","x":20,"y":20},
            {"type":"cubicTo","c1x":40,"c1y":if escaped_control {-20} else {40},
             "c2x":60,"c2y":40,"x":80,"y":20},
            {"type":"lineTo","x":80,"y":80}, {"type":"close"}
        ]);
        input["composition"]["layers"][1] = shape;
        let output = export(input);
        assert_eq!(checkbox(&output), if escaped_control { 0.0 } else { 1.0 });
    }
}
