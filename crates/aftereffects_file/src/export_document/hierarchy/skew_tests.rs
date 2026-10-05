use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::to_structural_fx_document;
use crate::writer::{NativeMatteRef, NullLayerSpec};
use fx_schema::animator::AnimationGraphEntry;
use serde_json::{Value, json};

fn imported_value() -> Value {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap()
}

fn rect(value: &Value, id: u64, name: &str) -> Value {
    let mut rect = value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    rect["id"] = json!(id);
    rect["name"] = json!(name);
    rect["parent"] = json!(70_040);
    rect["activeRange"] = json!({"start": 0, "duration": 2000});
    rect
}

fn skew_group(value: &Value, blend_mode: &str) -> Value {
    let mut target = rect(value, 70_041, "Skew matte target");
    target["trackMatte"] = json!({"mode":"alpha","layer":70_042});
    let mut provider = rect(value, 70_042, "Skew matte provider");
    provider["effects"] = json!([{
        "id": 71_043,
        "enabled": true,
        "effect": {"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}
    }]);
    json!({
        "type": "Group",
        "id": 70_040,
        "name": "Static skew Group",
        "parent": null,
        "transform": {
            "anchorPoint":[464.0,110.0],
            "position":[1151.0121606945172,352.0],
            "scale":[95.0,95.0],
            "rotation":0.0,
            "skew":-12.0,
            "skewAxis":0.0,
            "rotationX":0.0,
            "rotationY":0.0,
            "orientation":[0.0,0.0,0.0],
            "opacity":100.0
        },
        "layers":[target,provider,rect(value, 70_043, "Collision sentinel")],
        "isHidden":false,
        "blendMode":blend_mode,
        "trackMatte":null,
        "masks":[],
        "effects":[],
        "motionBlur":false,
        "playback":super::super::tests::fixture_linear_playback(json!({"start":0,"duration":2000}), json!({"start":0,"duration":2000})),
        "paddingTop":0.0,
        "paddingRight":0.0,
        "paddingBottom":0.0,
        "paddingLeft":0.0,
        "fills":[],
        "cornerRadiusTopLeft":0.0,
        "cornerRadiusTopRight":0.0,
        "cornerRadiusBottomRight":0.0,
        "cornerRadiusBottomLeft":0.0
    })
}

fn plain_options(id: LayerId) -> NativeLayerOptions {
    NativeLayerOptions {
        fx_id: id,
        parent: None,
        matte: None,
        enabled: true,
        adjustment_layer: false,
        motion_blur: false,
        blend_mode: 2,
        masks: Vec::new(),
        effects: Vec::new(),
        styles: Vec::new(),
        source_clock: None,
        transform_3d: None,
    }
}

fn null(name: &str) -> NullLayerSpec {
    NullLayerSpec {
        name: name.to_owned(),
        transform: SolidTransform {
            anchor: [0.0; 2],
            position: [0.0; 2],
            scale: [100.0; 2],
            rotation: 0.0,
            opacity: 100.0,
        },
        transform_animations: TransformAnimations::default(),
    }
}

fn map(transform: &SolidTransform, point: [f64; 2]) -> [f64; 2] {
    let radians = transform.rotation.to_radians();
    let (sin, cos) = radians.sin_cos();
    let x = (point[0] - transform.anchor[0]) * transform.scale[0] / 100.0;
    let y = (point[1] - transform.anchor[1]) * transform.scale[1] / 100.0;
    [
        transform.position[0] + cos * x - sin * y,
        transform.position[1] + sin * x + cos * y,
    ]
}

#[test]
fn skew_plan_conjugates_nonzero_anchor_and_parents_matte_roots() {
    let value = imported_value();
    let group: GroupLayer = serde_json::from_value(skew_group(&value, "add")).unwrap();
    let duration = Duration24::from_frames(48).unwrap();
    let plan = classify_with_skew_helper(
        &group,
        Time::from_millis(2000),
        duration,
        &crate::export_document::AnimationIndex::new(&[]),
        &BTreeMap::new(),
        fx_schema::Dimensions {
            width: 1920,
            height: 1080,
        },
        LayerId::new(70_044),
    )
    .unwrap();
    let HierarchyPlan::Precomposition(plan) = plan else {
        panic!("skew must use a precomposition")
    };

    let mut target_options = plain_options(LayerId::new(70_041));
    target_options.matte = Some(NativeMatteRef {
        layer: LayerId::new(70_042),
        mode: 1,
    });
    let children = vec![
        LayerSpec::Options(Box::new(LayerSpec::Null(null("target"))), target_options),
        LayerSpec::Options(
            Box::new(LayerSpec::Null(null("provider"))),
            plain_options(LayerId::new(70_042)),
        ),
    ];
    let output = plan
        .finish(children, plain_options(group.id), None)
        .unwrap();
    let LayerSpec::Options(inner, _) = output else {
        panic!("outer options")
    };
    let LayerSpec::Precomposition(precomposition) = *inner else {
        panic!("precomposition")
    };
    assert_eq!(precomposition.layers.len(), 3);
    assert_eq!(
        precomposition.layers[0].local_reference_facts().unwrap(),
        crate::writer::LayerReferenceFacts {
            layer_id: Some(LayerId::new(70_041)),
            parent: Some(LayerId::new(70_044)),
            matte: Some(LayerId::new(70_042)),
        }
    );
    assert_eq!(
        precomposition.layers[1]
            .local_reference_facts()
            .unwrap()
            .parent,
        Some(LayerId::new(70_044))
    );
    let LayerSpec::Options(helper, helper_options) = &precomposition.layers[2] else {
        panic!("helper options")
    };
    let LayerSpec::Null(helper) = helper.as_ref() else {
        panic!("helper Null")
    };
    assert_eq!(helper_options.fx_id, LayerId::new(70_044));

    let point = [735.25, -41.5];
    let actual = map(&precomposition.transform, map(&helper.transform, point));
    let matrix = skew::matrix(&group.transform).unwrap();
    let anchor = group.transform.anchor_point;
    let Position::TwoD(position) = group.transform.position else {
        panic!("2D position")
    };
    let delta = [point[0] - anchor[0], point[1] - anchor[1]];
    let expected = [
        position[0] + matrix[0] * delta[0] + matrix[1] * delta[1],
        position[1] + matrix[2] * delta[0] + matrix[3] * delta[1],
    ];
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }
}

#[test]
fn exported_skew_groups_preserve_order_matte_glow_blend_and_helper_id() {
    for (blend_mode, native_blend_mode) in [("add", 4), ("normal", 2)] {
        let mut value = imported_value();
        value["duration"] = json!(2.0);
        value["composition"]["layers"] = json!([skew_group(&value, blend_mode)]);
        value["composition"]["dynamics"] = json!({"entries":[]});
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = super::super::to_aep(&document).unwrap();
        assert!(
            output
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("subtree omitted")),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
            panic!("root composition")
        };
        assert_eq!(root.layers.len(), 1);
        let occurrence = &root.layers[0];
        assert_eq!(occurrence.record.blend_mode(), native_blend_mode);
        let ItemKind::Composition(source) =
            &native.item(occurrence.record.source_id()).unwrap().kind
        else {
            panic!("skew source composition")
        };
        assert_ne!([source.width, source.height], [1920, 1080]);
        assert!((source.duration_secs - 2.0).abs() < 1e-9);
        assert_eq!(
            source
                .layers
                .iter()
                .map(|layer| layer.name.as_ref())
                .collect::<Vec<_>>(),
            [
                "Skew matte target",
                "Skew matte provider",
                "Collision sentinel",
                "Static skew Group — Skew basis"
            ]
        );
        for layer in &source.layers {
            assert!((layer.record.in_point().unwrap() - 0.0).abs() < 1e-9);
            assert!((layer.record.out_point().unwrap() - 2.0).abs() < 1e-9);
        }
        let target = &source.layers[0];
        let provider = &source.layers[1];
        let helper = &source.layers[3];
        assert!(helper.record.flags().null_layer);
        assert_eq!(target.record.parent_id(), helper.record.id());
        assert_eq!(provider.record.parent_id(), helper.record.id());
        assert_eq!(target.record.matte_layer_id(), Some(provider.record.id()));
        assert!(provider.record.flags().effects_active);
    }
}

#[test]
fn unmapped_owner_effect_does_not_discard_static_skew_content() {
    let mut value = imported_value();
    let mut group = skew_group(&value, "normal");
    group["effects"] = json!([{
        "id":71_044,"enabled":true,
        "effect":{"type":"chromaticAberration","amount":0.3,"direction":0.0}
    }]);
    value["duration"] = json!(2.0);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries":[]});
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
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(70_040))
                && d.message.contains("chromaticAberration")
                && d.message.contains("omitted, owner retained"))
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
    assert_eq!(source.layers.len(), 4);
    let helper = &source.layers[3];
    assert!(helper.record.flags().null_layer);
    for child in &source.layers[..3] {
        assert_eq!(child.record.parent_id(), helper.record.id());
    }
    assert_eq!(
        source.layers[0].record.matte_layer_id(),
        Some(source.layers[1].record.id())
    );
    assert!(
        source.layers[1].record.flags().effects_active,
        "child Glow retained"
    );
    // A mapped owner effect still needs a proved phase mapping; don't clear it.
    let mut mapped = document.to_json_value().unwrap();
    mapped["composition"]["layers"][0]["effects"][0]["effect"] =
        json!({"type":"glow","glowThreshold":20.0,"glowRadius":8.0,"glowIntensity":0.5});
    let mapped = fx_schema::EditableFxCompositionDocument::from_json_value(mapped).unwrap();
    let rejected = super::super::to_aep(&mapped).unwrap();
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Group effects or masks"))
    );
}

#[test]
fn uniform_scale_keys_preserve_static_skew_factorization() {
    let mut value = imported_value();
    let group = skew_group(&value, "normal");
    let typed_group: GroupLayer = serde_json::from_value(group.clone()).unwrap();
    let baseline = skew::lower(
        &typed_group,
        &crate::export_document::AnimationIndex::new(&[]),
        LayerId::new(70_044),
    )
    .unwrap();
    let entries: Vec<Value> = ["scaleX", "scaleY"].into_iter().map(|property| json!({
        "target":{"kind":"layer","layerId":70_040,"propertyType":property},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":format!("{property}-0"),"layerTime":0,"value":{"type":"float","value":95.0},"easing":{"type":"linear"}},
            {"id":format!("{property}-1"),"layerTime":250,"value":{"type":"float","value":110.0},"easing":{"type":"cubicBezier","x1":0.16,"y1":1.0,"x2":0.3,"y2":1.0}},
            {"id":format!("{property}-2"),"layerTime":1000,"value":{"type":"float","value":95.0},"easing":{"type":"linear"}}
        ]}
    })).collect();
    value["duration"] = json!(2.0);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries":entries});
    let typed_entries: Vec<AnimationGraphEntry> = serde_json::from_value(json!(entries)).unwrap();
    let lowered = skew::lower(
        &typed_group,
        &crate::export_document::AnimationIndex::new(&typed_entries),
        LayerId::new(70_044),
    )
    .unwrap();
    let track = lowered.outer_animations.scale.unwrap();
    assert_eq!(
        track
            .keys
            .iter()
            .map(|key| key.time_millis)
            .collect::<Vec<_>>(),
        [0, 250, 1000]
    );
    let unfactored = super::super::transform_animations(
        &crate::export_document::AnimationIndex::new(&typed_entries),
        typed_group.id,
        &typed_group.transform,
        typed_group.id,
    )
    .unwrap()
    .scale
    .unwrap();
    for (factored, original) in track.keys.iter().zip(&unfactored.keys) {
        assert_eq!(factored.easing, original.easing);
        assert_eq!(factored.spatial_in, original.spatial_in);
        assert_eq!(factored.spatial_out, original.spatial_out);
    }
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
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
    let properties = crate::properties::read_transform(&root.layers[0].content).unwrap();
    let scale = properties
        .iter()
        .find(|p| p.match_name == "ADBE Scale")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(scale.keyframes.len(), 3);
    for ((key, time), authored) in scale
        .keyframes
        .iter()
        .zip([0.0, 0.25, 1.0])
        .zip([95.0, 110.0, 95.0])
    {
        assert_eq!(key.time_secs, time);
        for (actual, base) in key.values[..2].iter().zip(baseline.outer.scale) {
            assert!((actual - base / 100.0 * authored / 95.0).abs() < 1e-6);
        }
        assert_eq!(key.values[2], 1.0);
    }
    let ItemKind::Composition(source) =
        &native.item(root.layers[0].record.source_id()).unwrap().kind
    else {
        panic!("source")
    };
    assert_eq!(source.layers.len(), 4);
    assert!(source.layers[3].record.flags().null_layer);
    for child in &source.layers[..3] {
        assert_eq!(child.record.parent_id(), source.layers[3].record.id());
    }
    // Equal key values alone are insufficient: different curves are nonuniform.
    value["composition"]["dynamics"]["entries"][1]["animator"]["keyframes"][1]["easing"]["x1"] =
        json!(0.5);
    let nonuniform = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let rejected = super::super::to_aep(&nonuniform).unwrap();
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|d| d.message.contains("subtree omitted"))
    );
}

#[test]
fn zero_base_skew_keeps_flying_headline_position_opacity_and_uniform_scale() {
    // Source 40410 starts at zero base Scale, but its two authored Scale curves
    // are identical. Its Position X/Y and Opacity keys must survive as well.
    let value = imported_value();
    let mut group: GroupLayer = serde_json::from_value(skew_group(&value, "normal")).unwrap();
    group.id = LayerId::new(40_410);
    group.transform.scale = [0.0; 2];
    let entries: Vec<AnimationGraphEntry> = serde_json::from_value(json!([
        {"target":{"kind":"layer","layerId":40410,"propertyType":"scaleX"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"sx0","layerTime":0,"value":{"type":"float","value":16.666666666666664},"easing":{"type":"linear"}},
            {"id":"sx1","layerTime":517,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}}]}},
        {"target":{"kind":"layer","layerId":40410,"propertyType":"scaleY"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"sy0","layerTime":0,"value":{"type":"float","value":16.666666666666664},"easing":{"type":"linear"}},
            {"id":"sy1","layerTime":517,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}}]}},
        {"target":{"kind":"layer","layerId":40410,"propertyType":"positionX"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"px0","layerTime":0,"value":{"type":"float","value":1099.439392969622},"easing":{"type":"linear"}},
            {"id":"px1","layerTime":517,"value":{"type":"float","value":800.0},"easing":{"type":"linear"}}]}},
        {"target":{"kind":"layer","layerId":40410,"propertyType":"positionY"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"py0","layerTime":0,"value":{"type":"float","value":613.7159934494675},"easing":{"type":"linear"}},
            {"id":"py1","layerTime":517,"value":{"type":"float","value":850.0},"easing":{"type":"linear"}}]}},
        {"target":{"kind":"layer","layerId":40410,"propertyType":"opacity"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"op0","layerTime":0,"value":{"type":"float","value":0.0},"easing":{"type":"linear"}},
            {"id":"op1","layerTime":60,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}}]}}
    ])).unwrap();
    let lowering = skew::lower(
        &group,
        &crate::export_document::AnimationIndex::new(&entries),
        LayerId::new(40_411),
    )
    .unwrap();
    assert_eq!(lowering.outer.scale, [0.0; 2]);
    let animations = lowering.outer_animations;
    let scale = animations.scale.unwrap();
    let position = animations.position.unwrap();
    let opacity = animations.opacity.unwrap();
    assert_eq!(scale.keys.len(), 2);
    assert_eq!(position.keys.len(), 2);
    assert_eq!(opacity.keys.len(), 2);
    assert_eq!(opacity.keys[0].time_millis, 0);
    assert_eq!(opacity.keys[1].time_millis, 60);
    assert_eq!(
        position.keys[0].values[..2],
        [1099.439392969622, 613.7159934494675]
    );
    assert_eq!(position.keys[1].values[..2], [800.0, 850.0]);
    let unit = skew::matrix_components([100.0; 2], 0.0, -12.0, 0.0).unwrap();
    for (key, authored) in scale.keys.iter().zip([16.666666666666664, 100.0]) {
        let expected = unit.map(|element| element * authored / 100.0);
        let outer = SolidTransform {
            scale: [key.values[0] * 100.0, key.values[1] * 100.0],
            ..lowering.outer.clone()
        };
        for point in [[0.0, 0.0], [636.0, 84.0], [730.0, -25.0]] {
            let actual = map(&outer, map(&lowering.inner.transform, point));
            let delta = [
                point[0] - group.transform.anchor_point[0],
                point[1] - group.transform.anchor_point[1],
            ];
            let expected_point = [
                outer.position[0] + expected[0] * delta[0] + expected[1] * delta[1],
                outer.position[1] + expected[2] * delta[0] + expected[3] * delta[1],
            ];
            for (actual, expected) in actual.into_iter().zip(expected_point) {
                assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
            }
        }
    }
    // Distinct axis easing is not a uniform curve, even with identical endpoints.
    let mut different = entries.clone();
    let mut changed = serde_json::to_value(&different).unwrap();
    changed[1]["animator"]["keyframes"][1]["easing"] =
        json!({"type":"cubicBezier","x1":0.2,"y1":0.3,"x2":0.8,"y2":0.9});
    different = serde_json::from_value(changed).unwrap();
    assert!(
        skew::lower(
            &group,
            &crate::export_document::AnimationIndex::new(&different),
            LayerId::new(40_411)
        )
        .is_err()
    );
}

#[test]
fn skewed_shockwave_keeps_nonuniform_scale_and_opacity_in_vector_group() {
    // Source 40440/40441: skew -12 degrees, unequal ScaleX/Y curves, three
    // Opacity keys. Fixed-SVD uniform-Scale factoring is not valid here.
    let value = imported_value();
    let mut group: GroupLayer = serde_json::from_value(skew_group(&value, "normal")).unwrap();
    group.id = LayerId::new(40_440);
    group.transform.anchor_point = [0.0; 2];
    group.transform.position = Position::TwoD([800.0, 850.0]);
    group.transform.scale = [100.0; 2];
    let entries: Vec<AnimationGraphEntry> = serde_json::from_value(json!([
        {"target":{"kind":"layer","layerId":40440,"propertyType":"scaleX"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"sx0","layerTime":0,"value":{"type":"float","value":99.0},"easing":{"type":"linear"}},
            {"id":"sx1","layerTime":240,"value":{"type":"float","value":124.0},"easing":{"type":"cubicBezier","x1":0.16,"y1":1.0,"x2":0.3,"y2":1.0}}]}},
        {"target":{"kind":"layer","layerId":40440,"propertyType":"scaleY"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"sy0","layerTime":0,"value":{"type":"float","value":97.0},"easing":{"type":"linear"}},
            {"id":"sy1","layerTime":240,"value":{"type":"float","value":165.0},"easing":{"type":"cubicBezier","x1":0.16,"y1":1.0,"x2":0.3,"y2":1.0}}]}},
        {"target":{"kind":"layer","layerId":40440,"propertyType":"opacity"},"animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"op0","layerTime":0,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}},
            {"id":"op1","layerTime":40,"value":{"type":"float","value":90.0},"easing":{"type":"linear"}},
            {"id":"op2","layerTime":220,"value":{"type":"float","value":0.0},"easing":{"type":"linear"}}]}}
    ])).unwrap();
    let animations = super::super::transform_animations(
        &crate::export_document::AnimationIndex::new(&entries),
        group.id,
        &group.transform,
        group.id,
    )
    .unwrap();
    let vector_keys = super::super::vector_animation::program_transform_animations(
        &crate::export_document::AnimationIndex::new(&entries),
        group.id,
        &group.transform,
    )
    .unwrap();
    let vector_scale = vector_keys.scale.as_ref().unwrap();
    assert_eq!(vector_scale.keys[0].values, [99.0, 97.0]);
    assert_eq!(vector_scale.keys[1].values, [124.0, 165.0]);
    assert_eq!(
        vector_scale.keys[0].easing,
        [super::super::KeyframeEasing::Linear; 2]
    );
    assert_eq!(vector_scale.keys[1].easing.len(), 2);
    for ease in &vector_scale.keys[1].easing {
        let super::super::KeyframeEasing::CubicBezier { x1, y1, x2, y2 } = ease else {
            panic!("both native vector Scale axes must preserve the authored cubic easing");
        };
        assert!((x1 - 0.16).abs() < 1e-12);
        assert_eq!((*y1, *y2), (1.0, 1.0));
        assert!((x2 - 0.3).abs() < 1e-12);
    }
    let vector_opacity = vector_keys.opacity.as_ref().unwrap();
    assert_eq!(vector_opacity.keys[0].values, [100.0]);
    assert_eq!(vector_opacity.keys[1].values, [90.0]);
    assert_eq!(vector_opacity.keys[2].values, [0.0]);
    let ((layer, layer_keys), contents) = super::super::program_transform(
        &group.transform,
        animations,
        &crate::export_document::AnimationIndex::new(&entries),
        group.id,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(layer, super::super::identity_solid_transform());
    assert_eq!(layer_keys, TransformAnimations::default());
    let [super::super::VectorContent::AnimatedGroup(vector, keys)] = contents.as_slice() else {
        panic!("shape Transform must own native vector-group keys under the static skew");
    };
    assert_eq!(vector.transform.skew, -12.0);
    assert_eq!(vector.transform.position, [800.0, 850.0]);
    assert_eq!(keys, &vector_keys);
    let scale = keys.scale.as_ref().unwrap();
    assert_eq!(
        scale
            .keys
            .iter()
            .map(|key| key.time_millis)
            .collect::<Vec<_>>(),
        [0, 240]
    );
    assert_eq!(scale.keys[0].values, [99.0, 97.0]);
    assert_eq!(scale.keys[1].values, [124.0, 165.0]);
    assert_eq!(scale.keys, vector_scale.keys);
    let opacity = keys.opacity.as_ref().unwrap();
    assert_eq!(
        opacity
            .keys
            .iter()
            .map(|key| key.time_millis)
            .collect::<Vec<_>>(),
        [0, 40, 220]
    );
    assert_eq!(
        opacity
            .keys
            .iter()
            .map(|key| key.values[0])
            .collect::<Vec<_>>(),
        [100.0, 90.0, 0.0]
    );
    assert!(opacity.keys.iter().all(|key| {
        key.values.len() == 1 && key.easing == [super::super::KeyframeEasing::Linear]
    }));
}

#[test]
fn short_skew_group_preserves_occurrence_and_child_clocks() {
    for start in [0, 12_551] {
        let mut value = imported_value();
        let mut group = skew_group(&value, "normal");
        group["playback"] = super::super::tests::fixture_linear_playback(
            json!({"start":start,"duration":2449}),
            json!({"start":0,"duration":2449}),
        );
        value["duration"] = json!(15.0);
        value["composition"]["layers"] = json!([group]);
        value["composition"]["dynamics"] = json!({"entries":[]});
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = super::super::to_aep(&document).unwrap();
        assert!(
            output
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("subtree omitted")),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
            panic!("root composition")
        };
        assert_eq!(root.layers.len(), 1);
        let occurrence = &root.layers[0];
        assert!((occurrence.record.start_time().unwrap() - start as f64 / 1000.0).abs() < 1e-6);
        assert_eq!(occurrence.record.stretch(), Some(1.0));
        assert_eq!(occurrence.record.in_point(), Some(0.0));
        assert!((occurrence.record.out_point().unwrap() - 2.449).abs() < 1e-6);
        let ItemKind::Composition(source) =
            &native.item(occurrence.record.source_id()).unwrap().kind
        else {
            panic!("skew source composition")
        };
        assert!((source.duration_secs - 2.449).abs() < 1.0 / 24_576.0);
        assert_eq!(source.layers.len(), 4);
        for child in &source.layers[..3] {
            assert!(child.record.in_point().unwrap().abs() < 1e-9);
            assert!((child.record.out_point().unwrap() - 2.0).abs() < 1e-9);
        }
        assert!(source.layers[3].record.flags().null_layer);
        assert_eq!(
            source.layers[0].record.parent_id(),
            source.layers[3].record.id()
        );
        assert_eq!(
            source.layers[0].record.matte_layer_id(),
            Some(source.layers[1].record.id())
        );
    }
}

#[test]
fn skew_controls_reject_effect_phase_animation_and_singular_scale() {
    let value = imported_value();
    let mut group: GroupLayer = serde_json::from_value(skew_group(&value, "add")).unwrap();
    group.effects.push(
        serde_json::from_value(json!({
            "id":1,"enabled":true,
            "effect":{"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}
        }))
        .unwrap(),
    );
    assert!(
        skew::lower(
            &group,
            &crate::export_document::AnimationIndex::new(&[]),
            LayerId::new(80_000)
        )
        .is_err()
    );

    group.effects.clear();
    let animation = fx_schema::animator::AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(group.id, fx_schema::PropType::Skew),
        animator: fx_schema::animator::PropertyAnimator::constant(fx_schema::PropertyValue::Float(
            -12.0,
        ))
        .unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    assert!(
        skew::lower(
            &group,
            &crate::export_document::AnimationIndex::new(&[animation]),
            LayerId::new(80_000)
        )
        .is_err()
    );

    group.transform.scale[0] = 0.0;
    assert!(
        skew::lower(
            &group,
            &crate::export_document::AnimationIndex::new(&[]),
            LayerId::new(80_000)
        )
        .is_err()
    );
}
