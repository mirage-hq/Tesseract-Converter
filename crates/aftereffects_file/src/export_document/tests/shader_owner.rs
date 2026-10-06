use super::*;

fn shader() -> Value {
    json!({"id": 7400, "enabled": true, "effect": {
        "type": "customShader", "name": "Unsupported rendering replacement",
        "wgsl": "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }"
    }})
}

/// Public synthetic control of P005's owner profile, not a copy of private WGSL.
#[test]
fn p005_custom_shader_filled_shape_owners_are_omitted() {
    use fx_schema::layer::{ShapePath, ShapePathCommand};
    let mut value = imported();
    let sibling = rect(&value, 2402);
    let owners: Vec<_> = [2400, 2401].into_iter().map(|id| {
        let mut shape = rect(&value, id);
        shape["type"] = json!("Shape");
        shape.as_object_mut().unwrap().remove("rect");
        shape["shape"] = json!({
            "path": ShapePath { commands: vec![
                ShapePathCommand::MoveTo { x: 0.0, y: 0.0, mirror: None, corner_radius: None },
                ShapePathCommand::LineTo { x: 1080.0, y: 0.0, mirror: None, corner_radius: None },
                ShapePathCommand::LineTo { x: 1080.0, y: 1350.0, mirror: None, corner_radius: None },
                ShapePathCommand::LineTo { x: 0.0, y: 1350.0, mirror: None, corner_radius: None },
                ShapePathCommand::Close,
            ] },
            "fills": [{"paint": {"type": "solid", "color": [1.0, 1.0, 1.0, 1.0]}, "opacity": 1.0}]
        });
        shape["transform"]["opacity"] = json!(100.0);
        shape["blendMode"] = json!("screen");
        let mut effect = shader();
        effect["id"] = json!(id + 5000);
        shape["effects"] = json!([effect]);
        shape
    }).collect();
    value["composition"]["layers"] = json!([owners[0], owners[1], sibling]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native).len(),
        1,
        "bare shader-owner fills must not survive"
    );
    for id in [2400, 2401] {
        assert!(output.omitted_layer_ids.contains(&LayerId::new(id)));
        assert!(output.diagnostics.iter().any(|d| {
            d.layer_id == Some(LayerId::new(id))
                && d.message
                    .contains("owner omitted: CustomShader adjustment or plain white shader canvas")
        }));
    }
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(2402)));
}

#[test]
fn custom_shader_owner_policy_covers_mixed_disabled_and_legacy_records() {
    for effect in [shader(), shader()["effect"].clone(), {
        let mut disabled = shader();
        disabled["enabled"] = json!(false);
        disabled
    }] {
        let mut value = imported();
        let mut owner = rect(&value, 2400);
        owner["rect"]["fillColor"] = json!([0.2, 0.3, 0.4, 1.0]);
        owner["effects"] =
            json!([effect, {"type": "gaussianBlur", "blurriness": 2.0, "repeatEdgePixels": true}]);
        value["composition"]["layers"] = json!([owner, rect(&value, 2402)]);
        value["composition"]["dynamics"] = json!({"entries": []});
        let output = export(value);
        assert!(!output.omitted_layer_ids.contains(&LayerId::new(2400)));
        assert!(!output.omitted_layer_ids.contains(&LayerId::new(2402)));
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Unsupported rendering replacement")
                    && d.message.contains("dropped"))
        );
    }
}

#[test]
fn shader_container_owner_and_children_are_retained() {
    let mut value = imported();
    let mut child = rect(&value, 2402);
    child["parent"] = json!(2400);
    let sibling = rect(&value, 2403);
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(2400);
    group["parent"] = Value::Null;
    group["name"] = json!("omitted shader container");
    group["effects"] = json!([shader()]);
    group["layers"] = json!([child]);
    value["composition"]["layers"] = json!([group, sibling]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value.clone());
    let native = read_project(&output.bytes).unwrap();
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(2400)));
    assert!(layers(&native).len() >= 2);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(2400))
                && d.message.contains("Unsupported rendering replacement"))
    );
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(2402)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(2403)));
    value["composition"]["layers"][0]["effects"] = json!([]);
    assert_eq!(
        output.bytes,
        export(value).bytes,
        "owner and child controls match ordinary unshaded export"
    );
}

#[test]
fn shader_adjustment_is_dropped_with_named_effect_and_owner_diagnostics() {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/adjustment/fx_export/adjustment-stack-order.fx.json"
    ))
    .unwrap();
    value["composition"]["layers"][1]["effects"] = json!([shader()]);
    let output = export(value);
    assert!(output.omitted_layer_ids.contains(&LayerId::new(224)));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(224))
                && d.message.contains("Unsupported rendering replacement")
                && d.message.contains("dropped"))
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(224)) && d.message.contains("owner omitted"))
    );
}

#[test]
fn white_rect_shader_canvas_is_dropped_with_named_effect_and_owner_diagnostics() {
    let mut value = imported();
    let mut canvas = rect(&value, 2400);
    canvas["rect"]["fillColor"] = json!([1.0, 1.0, 1.0, 1.0]);
    canvas["effects"] = json!([shader()]);
    value["composition"]["layers"] = json!([canvas, rect(&value, 2402)]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value);
    assert!(output.omitted_layer_ids.contains(&LayerId::new(2400)));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(2400))
                && d.message.contains("Unsupported rendering replacement")
                && d.message.contains("dropped"))
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(2400)) && d.message.contains("owner omitted"))
    );
}

#[test]
fn nonwhite_shape_shader_keeps_exact_unshaded_content() {
    use fx_schema::layer::{ShapePath, ShapePathCommand};
    let mut value = imported();
    let mut shape = rect(&value, 2400);
    shape["type"] = json!("Shape");
    shape.as_object_mut().unwrap().remove("rect");
    shape["shape"] = json!({"path": ShapePath { commands: vec![
        ShapePathCommand::MoveTo { x: 0.0, y: 0.0, mirror: None, corner_radius: None },
        ShapePathCommand::LineTo { x: 100.0, y: 0.0, mirror: None, corner_radius: None },
        ShapePathCommand::LineTo { x: 100.0, y: 100.0, mirror: None, corner_radius: None },
        ShapePathCommand::Close,
    ] }, "fills": [{"paint": {"type":"solid","color":[0.2,0.3,0.4,1.0]},"opacity":1.0}]});
    shape["effects"] = json!([shader()]);
    value["composition"]["layers"] = json!([shape]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value.clone());
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(2400)));
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Unsupported rendering replacement")
                && d.message.contains("dropped"))
    );
    value["composition"]["layers"][0]["effects"] = json!([]);
    assert_eq!(output.bytes, export(value).bytes);
}

#[test]
fn shader_child_cannot_be_flattened_into_a_shader_free_vector_owner() {
    let mut value = imported();
    let mut shader_child = rect(&value, 2400);
    shader_child["parent"] = json!(2403);
    shader_child["rect"]["fillColor"] = json!([1.0, 1.0, 1.0, 1.0]);
    shader_child["effects"] = json!([shader()]);
    let mut sibling = rect(&value, 2402);
    sibling["parent"] = json!(2403);
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(2403);
    group["parent"] = Value::Null;
    group["effects"] = json!([]);
    group["layers"] = json!([shader_child, sibling]);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let output = export(value);
    assert!(output.omitted_layer_ids.contains(&LayerId::new(2400)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(2402)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(2403)));
}
