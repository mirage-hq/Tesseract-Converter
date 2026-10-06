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
