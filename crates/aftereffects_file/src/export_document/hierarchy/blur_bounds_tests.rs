use super::*;

fn shadow(radius: f64, enabled: bool) -> Layer {
    Layer::from_data(&serde_json::from_value::<LayerData>(serde_json::json!({
        "type": "Shape", "id": 12442, "name": "file icon shadow", "blendMode":"normal",
        "activeRange":{"start":0,"duration":2564},
        "transform": {"position": [8,8], "anchorPoint": [0,0], "scale": [100,100], "rotation":0, "opacity":55},
        "shape": {"path": {"commands": [
            {"type":"moveTo","x":0,"y":0},
            {"type":"lineTo","x":216,"y":0},
            {"type":"lineTo","x":216,"y":214},
            {"type":"lineTo","x":0,"y":214}, {"type":"close"}
        ]}, "fills":[{"paint":{"type":"solid","color":[1,1,1,1]},"fillRule":"nonZeroWinding","blendMode":"normal","opacity":1}]},
        "effects":[{"id":12441,"enabled":enabled,"effect":{"type":"gaussianBlur","blurriness":radius}}]
    })).unwrap()).unwrap()
}

#[test]
fn static_file_icon_shadow_encloses_exact_gaussian_radius_before_transform() {
    let canvas = fx_schema::Dimensions {
        width: 1080,
        height: 1920,
    };
    let empty = crate::export_document::AnimationIndex::new(&[]);
    for (radius, enabled, reach) in [
        (12.0, true, 12.0),
        (0.0, true, 0.0),
        (12.0, false, 0.0),
        (128.0, true, 128.0),
    ] {
        let layer = shadow(radius, enabled);
        let bounds = all_time_layer_bounds(&layer, &empty, &Default::default(), canvas)
            .unwrap()
            .unwrap();
        assert_eq!(bounds.min, [8.0 - reach; 2]);
        assert_eq!(bounds.max, [224.0 + reach, 222.0 + reach]);
        let animated = animated_bounds::layer_bounds(&layer, &empty, &Default::default(), canvas)
            .unwrap()
            .unwrap();
        assert_eq!(animated.min, bounds.min);
        assert_eq!(animated.max, bounds.max);
    }
    let mut scaled = serde_json::to_value(shadow(12.0, true).data()).unwrap();
    scaled["transform"]["scale"] = serde_json::json!([200, 50]);
    let scaled = Layer::from_data(&serde_json::from_value(scaled).unwrap()).unwrap();
    let bounds = all_time_layer_bounds(&scaled, &empty, &Default::default(), canvas)
        .unwrap()
        .unwrap();
    // A scaled Shape's continuous-rasterized native effect/transform order
    // has no independent support proof; retain the old content-only domain.
    assert_eq!(bounds.min, [8.0, 8.0]);
    assert_eq!(bounds.max, [440.0, 115.0]);
}

#[test]
fn repeat_edge_and_explicit_plane_blurs_do_not_acquire_a_new_certificate() {
    let canvas = fx_schema::Dimensions {
        width: 1080,
        height: 1920,
    };
    let empty = crate::export_document::AnimationIndex::new(&[]);
    for (field, value) in [
        ("repeatEdgePixels", serde_json::json!(true)),
        ("layerSize", serde_json::json!([216, 214])),
    ] {
        let mut value_layer = serde_json::to_value(shadow(12.0, true).data()).unwrap();
        value_layer["effects"][0]["effect"][field] = value;
        let layer = Layer::from_data(&serde_json::from_value(value_layer).unwrap()).unwrap();
        let bounds = all_time_layer_bounds(&layer, &empty, &Default::default(), canvas)
            .unwrap()
            .unwrap();
        assert_eq!(bounds.min, [8.0, 8.0]);
        assert_eq!(bounds.max, [224.0, 222.0]);
    }
}

#[test]
fn unproved_transforms_and_multiple_stages_keep_content_only_bounds() {
    let empty = crate::export_document::AnimationIndex::new(&[]);
    for (field, value) in [
        ("scale", serde_json::json!([50, 100])),
        ("rotation", serde_json::json!(30)),
        ("skew", serde_json::json!(20)),
        ("position", serde_json::json!([8, 8, 10])),
    ] {
        let mut value_layer = serde_json::to_value(shadow(12.0, true).data()).unwrap();
        value_layer["transform"][field] = value;
        let data: LayerData = serde_json::from_value(value_layer).unwrap();
        let LayerData::Shape(shape) = data else {
            panic!("Shape")
        };
        assert_eq!(blur_bounds::shape_reach(&shape, &empty), 0.0);
    }
    let LayerData::Shape(mut shape) = shadow(12.0, true).data().clone() else {
        panic!("Shape")
    };
    shape.effects.push(shape.effects[0].clone());
    assert_eq!(blur_bounds::shape_reach(&shape, &empty), 0.0);
    shape.effects.pop();
    let entries = [fx_schema::animator::AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(shape.id, fx_schema::PropType::ScaleX),
        animator: fx_schema::animator::PropertyAnimator::constant(fx_schema::PropertyValue::Float(
            50.0,
        ))
        .unwrap(),
        dependencies: vec![],
        random_seed_target: None,
        layer_refs: Default::default(),
    }];
    assert_eq!(
        blur_bounds::shape_reach(
            &shape,
            &crate::export_document::AnimationIndex::new(&entries)
        ),
        0.0
    );
}

#[test]
fn effect_owned_blur_keys_do_not_certify_the_authored_static_radius() {
    let entries = vec![serde_json::from_value::<fx_schema::animator::AnimationGraphEntry>(serde_json::json!({
        "target":{"kind":"effectProperty","effectId":12441,"paramName":"blurriness"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"radius-start","layerTime":0,"value":{"type":"float","value":12},"easing":{"type":"linear"}},
            {"id":"radius-end","layerTime":1000,"value":{"type":"float","value":100},"easing":{"type":"linear"}}
        ]},"dependencies":[],"layerRefs":{}
    })).unwrap()];
    let dynamics = crate::export_document::AnimationIndex::new(&entries);
    let bounds = all_time_layer_bounds(
        &shadow(12.0, true),
        &dynamics,
        &Default::default(),
        fx_schema::Dimensions {
            width: 1080,
            height: 1920,
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(bounds.min, [8.0, 8.0]);
    assert_eq!(bounds.max, [224.0, 222.0]);
}

fn glowing_shape() -> Layer {
    let mut value = serde_json::to_value(shadow(12.0, true).data()).unwrap();
    value["name"] = serde_json::json!("Glowing moving shape");
    value["activeRange"] = serde_json::json!({"start":0,"duration":1000});
    value["motionBlur"] = serde_json::json!(true);
    value["effects"][0]["effect"] = serde_json::json!({
        "type":"glow", "glowRadius":40,"glowThreshold":35,"glowIntensity":0.9
    });
    Layer::from_data(&serde_json::from_value(value).unwrap()).unwrap()
}

fn glow_position_keys() -> Vec<fx_schema::animator::AnimationGraphEntry> {
    vec![serde_json::from_value(serde_json::json!({
        "target":{"kind":"layer","layerId":12442,"propertyType":"positionY"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"a","layerTime":0,"value":{"type":"float","value":100},"easing":{"type":"linear"}},
            {"id":"b","layerTime":1000,"value":{"type":"float","value":0},"easing":{"type":"linear"}}
        ]},"dependencies":[],"layerRefs":{}
    })).unwrap()]
}

#[test]
fn shape_glow_keeps_logical_working_plane_without_certifying_kernel_reach() {
    let canvas = fx_schema::Dimensions {
        width: 640,
        height: 360,
    };
    let layer = glowing_shape();
    for entries in [vec![], glow_position_keys()] {
        let dynamics = crate::export_document::AnimationIndex::new(&entries);
        for bounds in [
            all_time_layer_bounds(&layer, &dynamics, &Default::default(), canvas),
            animated_bounds::layer_bounds(&layer, &dynamics, &Default::default(), canvas),
        ] {
            let bounds = bounds.unwrap().unwrap();
            assert_eq!(bounds.min, [0.0; 2]);
            assert_eq!(bounds.max, [640.0, 360.0]);
        }
    }
    assert!(
        effect_support::input_reach(
            layer.data().effects(),
            &crate::export_document::AnimationIndex::new(&[])
        )
        .is_err()
    );
    // The working plane must never crop validated offscreen paint.
    let mut value = serde_json::to_value(layer.data()).unwrap();
    value["transform"]["position"] = serde_json::json!([800, 8]);
    let outside = Layer::from_data(&serde_json::from_value(value).unwrap()).unwrap();
    let bounds = layer_bounds(&outside, &Default::default(), canvas)
        .unwrap()
        .unwrap();
    assert_eq!(bounds.min, [0.0; 2]);
    assert_eq!(bounds.max, [1016.0, 360.0]);
}

#[test]
fn shape_glow_working_plane_preserves_unproved_profile_guards() {
    let empty = crate::export_document::AnimationIndex::new(&[]);
    for (field, value) in [
        ("scale", serde_json::json!([50, 100])),
        ("rotation", serde_json::json!(20)),
        ("skew", serde_json::json!(10)),
        ("position", serde_json::json!([0, 0, 10])),
    ] {
        let mut data = serde_json::to_value(glowing_shape().data()).unwrap();
        data["transform"][field] = value;
        let LayerData::Shape(shape) = serde_json::from_value(data).unwrap() else {
            panic!("shape")
        };
        assert!(!shape_glow_working_plane(&shape, &empty));
    }
    let LayerData::Shape(mut shape) = glowing_shape().data().clone() else {
        panic!("shape")
    };
    let mut keys = glow_position_keys();
    keys[0].target =
        fx_schema::PropertyTarget::effect_param(fx_schema::EffectId::new(12441), "glowRadius");
    assert!(!shape_glow_working_plane(
        &shape,
        &crate::export_document::AnimationIndex::new(&keys)
    ));
    shape.effects.push(shape.effects[0].clone());
    assert!(!shape_glow_working_plane(&shape, &empty));
    let LayerData::Shape(mut shape) = glowing_shape().data().clone() else {
        panic!("shape")
    };
    shape.effects = shadow(12.0, true).data().effects().to_vec();
    assert!(!shape_glow_working_plane(&shape, &empty));
}

#[test]
fn masked_glowing_shape_exports_an_editable_full_plane_source() {
    use crate::structure::{ItemKind, read_project};
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    let mut value = crate::structure_document::to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    value["duration"] = serde_json::json!(1);
    value["dimensions"] = serde_json::json!({"width":640,"height":360});
    value["composition"]["layers"] = serde_json::json!([{
        "id":42,"type":"Group","name":"Masked glow","blendMode":"normal",
        "transform":{"position":[0,0],"anchorPoint":[0,0],"scale":[100,100],"rotation":0,"opacity":90},
        "masks":[{"id":43,"mode":"add","opacity":100,"feather":[0,0],"expansion":0,"inverted":false,
            "path":{"commands":[{"type":"moveTo","x":-1000,"y":-1000},
                {"type":"lineTo","x":1000,"y":-1000},
                {"type":"lineTo","x":1000,"y":180},
                {"type":"lineTo","x":-1000,"y":180},{"type":"close"}]}}],
        "layers":[glowing_shape().data()]
    }]);
    value["composition"]["layers"][0]["playback"] = super::super::tests::fixture_linear_playback(
        serde_json::json!({"start":0,"duration":1000}),
        serde_json::json!({"start":0,"duration":1000}),
    );
    value["composition"]["dynamics"] = serde_json::json!({"entries":glow_position_keys()});
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("root")
    };
    assert_eq!(root.layers.len(), 1);
    let ItemKind::Composition(source) =
        &native.item(root.layers[0].record.source_id()).unwrap().kind
    else {
        panic!("source")
    };
    assert_eq!([source.width, source.height], [640, 360]);
    assert_eq!(source.layers.len(), 1);
    assert_eq!(source.layers[0].name.as_ref(), "Glowing moving shape");
    assert!(source.layers[0].record.flags().effects_active);
    let (effects, _) =
        crate::effects::native::read_effects(&source.layers[0].content, [640.0, 360.0]);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].match_name, "ADBE Glo2");
    assert!(effects[0].enabled);
    for (name, expected) in [
        ("ADBE Glo2-0002", 89.25),
        ("ADBE Glo2-0003", 40.0),
        ("ADBE Glo2-0004", 0.9),
    ] {
        let parameter = effects[0]
            .parameters
            .iter()
            .find(|p| p.match_name == name)
            .unwrap();
        assert!((parameter.numeric.as_ref().unwrap().values[0] - expected).abs() < 1e-5);
    }
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(fx_schema::LayerId::new(12442))
                && d.message.contains("logical composition working plane"))
    );
}

#[test]
fn static_mitered_offset_paths_bounds_match_the_animated_analyzer() {
    let canvas = fx_schema::Dimensions {
        width: 1080,
        height: 1920,
    };
    let empty = crate::export_document::AnimationIndex::new(&[]);
    for (join, multiplier) in [("miter", 4.0), ("round", 1.0)] {
        let layer = Layer::from_data(&serde_json::from_value::<LayerData>(serde_json::json!({
            "type": "Shape", "id": 7, "name": "acute offset", "blendMode": "normal",
            "activeRange": {"start": 0, "duration": 1000},
            "transform": {"position": [100, 100], "anchorPoint": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
            "shape": {"path": {"commands": [
                {"type": "moveTo", "x": 0, "y": 0},
                {"type": "lineTo", "x": 200, "y": 20},
                {"type": "lineTo", "x": 0, "y": 40}, {"type": "close"}
            ]}, "fills": [{"paint": {"type": "solid", "color": [1, 1, 1, 1]}, "fillRule": "nonZeroWinding", "blendMode": "normal", "opacity": 1}],
            "offsetPaths": {"amount": 10, "lineJoin": join, "miterLimit": 4}}
        })).unwrap()).unwrap();
        let bounds = layer_bounds(&layer, &Default::default(), canvas)
            .unwrap()
            .unwrap();
        let reach = 10.0 * multiplier;
        assert_eq!(bounds.min, [100.0 - reach, 100.0 - reach], "{join}");
        assert_eq!(bounds.max, [300.0 + reach, 140.0 + reach], "{join}");
        let animated = animated_bounds::layer_bounds(&layer, &empty, &Default::default(), canvas)
            .unwrap()
            .unwrap();
        assert_eq!(animated.min, bounds.min, "{join}");
        assert_eq!(animated.max, bounds.max, "{join}");
    }
}
