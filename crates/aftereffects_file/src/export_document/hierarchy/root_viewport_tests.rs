use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::to_structural_fx_document;
use serde_json::{Value, json};

fn document_value() -> Value {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    let mut value = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut child =
        value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    child["id"] = json!(90001);
    child["parent"] = json!(90000);
    child["name"] = json!("Uncropped distant plane");
    child["activeRange"] = json!({"start":0,"duration":2000});
    child["transform"]["position"] = json!([18000.0, 90.0]);
    child["transform"]["rotationY"] = json!(0.1);
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(90000);
    group["name"] = json!("Final root spatial group");
    group["transform"] = serde_json::to_value(super::super::identity_fx_transform()).unwrap();
    group["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":0,"duration":2000}),
        json!({"start":0,"duration":2000}),
    );
    group["effects"] = json!([]);
    group["masks"] = json!([]);
    group["trackMatte"] = Value::Null;
    group["motionBlur"] = json!(false);
    group["layers"] = json!([child]);
    value["duration"] = json!(2.0);
    value["dimensions"] = json!({"width":319,"height":179});
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    value
}

#[test]
fn final_root_viewport_keeps_oversized_2d_child_and_clock_without_camera() {
    let mut value = document_value();
    let group = &mut value["composition"]["layers"][0];
    group["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":500,"duration":1500}),
        json!({"start":0,"duration":1500}),
    );
    let child = &mut group["layers"][0];
    child["activeRange"] = json!({"start":0,"duration":1500});
    child["transform"]["rotationY"] = json!(0.0);
    child["transform"]["scale"] = json!([1_000_000.0, 100.0]);
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
    let occurrence = &root.layers[0];
    assert_eq!(occurrence.record.start_time(), Some(0.5));
    assert_eq!(occurrence.record.in_point(), Some(0.0));
    assert_eq!(occurrence.record.out_point(), Some(1.5));
    let ItemKind::Composition(source) = &native.item(occurrence.record.source_id()).unwrap().kind
    else {
        panic!("source")
    };
    assert_eq!([source.width, source.height], [319, 179]);
    assert_eq!(source.duration_secs, 1.5);
    assert_eq!(source.layers.len(), 1, "no extra camera or discarded child");
    assert_eq!(source.layers[0].name.as_ref(), "Uncropped distant plane");
    let properties = crate::properties::read_transform(&source.layers[0].content).unwrap();
    let scale = properties
        .iter()
        .find(|p| p.match_name == "ADBE Scale")
        .unwrap();
    assert_eq!(scale.numeric.as_ref().unwrap().values[..2], [10_000.0, 1.0]);
}

#[test]
fn nested_full_source_group_uses_owning_precomposition_clock() {
    let mut value = document_value();
    let mut root = value["composition"]["layers"][0].clone();
    root["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":500,"duration":1500}),
        json!({"start":0,"duration":1500}),
    );
    let mut child = root["layers"][0].clone();
    child["parent"] = json!(90010);
    child["activeRange"] = json!({"start":0,"duration":1500});
    child["transform"]["rotationY"] = json!(0.0);
    child["transform"]["scale"] = json!([1_000_000.0, 100.0]);
    let mut second = child.clone();
    second["id"] = json!(90002);
    let mut control = root.clone();
    control["id"] = json!(90010);
    control["parent"] = json!(90000);
    control["name"] = json!("Full-source control");
    control["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":0,"duration":1500}),
        json!({"start":0,"duration":1500}),
    );
    control["layers"] = json!([child, second]);
    root["layers"] = json!([control]);
    let mut sibling = child.clone();
    sibling["id"] = json!(90003);
    sibling["parent"] = Value::Null;
    sibling["name"] = json!("Following root sibling");
    sibling["activeRange"] = json!({"start":1625,"duration":250});
    value["composition"]["layers"] = json!([root, sibling]);
    value["composition"]["dynamics"]["entries"] = json!([{
        "target":{"kind":"layer","layerId":90010,"propertyType":"positionX"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"source-start","layerTime":0,"value":{"type":"float","value":0.0},"easing":{"type":"linear"}},
            {"id":"source-move","layerTime":1000,"value":{"type":"float","value":20.0},"easing":{"type":"linear"}}
        ]}
    }]);
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
    assert_eq!(root.layers.len(), 2);
    assert_eq!(root.layers[0].record.start_time(), Some(0.5));
    assert_eq!(root.layers[1].name.as_ref(), "Following root sibling");
    // Native in/out points are layer-relative, not composition timestamps.
    assert_eq!(root.layers[1].record.start_time(), Some(1.625));
    assert_eq!(root.layers[1].record.in_point(), Some(0.0));
    assert_eq!(root.layers[1].record.out_point(), Some(0.25));
    let ItemKind::Composition(source) =
        &native.item(root.layers[0].record.source_id()).unwrap().kind
    else {
        panic!("source")
    };
    assert_eq!(source.duration_secs, 1.5);
    assert_eq!(source.layers.len(), 3);
    let parent = source
        .layers
        .iter()
        .find(|layer| layer.record.flags().null_layer)
        .unwrap();
    assert_eq!(parent.name.as_ref(), "Full-source control");
    assert_eq!(parent.record.out_point(), Some(1.5));
    for child in source
        .layers
        .iter()
        .filter(|layer| !layer.record.flags().null_layer)
    {
        assert_eq!(child.record.parent_id(), parent.record.id());
        assert_eq!(child.record.in_point(), Some(0.0));
        assert_eq!(child.record.out_point(), Some(1.5));
    }
    let properties = crate::properties::read_transform(&parent.content).unwrap();
    let position = properties
        .iter()
        .find(|p| p.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(
        position
            .keyframes
            .iter()
            .map(|key| key.time_secs)
            .collect::<Vec<_>>(),
        [0.0, 1.0]
    );
    assert_eq!(position.keyframes[1].values[..2], [20.0, 0.0]);
}

#[test]
fn final_root_viewport_keeps_output_size_camera_and_distant_child() {
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(document_value()).unwrap();
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
    let ItemKind::Composition(source) =
        &native.item(root.layers[0].record.source_id()).unwrap().kind
    else {
        panic!("source")
    };
    assert_eq!([source.width, source.height], [319, 179]);
    assert!(
        source
            .layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Uncropped distant plane")
    );
    // The planner keeps the same root lens and exact (including half-pixel)
    // principal point, without expanding odd viewport dimensions by one pixel.
    let layers = document.composition().layers();
    let LayerData::Group(group) = layers[0].data() else {
        panic!("group")
    };
    let certified = root_viewport::canvas(group, &[], layers, document.dimensions()).unwrap();
    let HierarchyPlan::Precomposition(plan) = classify_precomposition_with_demand(
        group,
        Time::from_millis(2000),
        Duration24::from_frames(48).unwrap(),
        &[],
        &BTreeMap::new(),
        document.dimensions(),
        Some(&certified),
    )
    .unwrap() else {
        panic!("precomposition")
    };
    assert_eq!(plan.origin, [0.0, 0.0]);
    let camera = plan.camera.unwrap();
    let expected = crate::writer::NativeCameraSpec::root(319, 179);
    assert_eq!(camera.center, expected.center);
    assert_eq!(camera.distance, expected.distance);
}

#[test]
fn oversized_3d_consumer_retains_world_camera_and_uses_power_of_two_raster() {
    let mut value = document_value();
    value["composition"]["layers"][0]["layers"][0]["transform"]["position"] =
        json!([45000.0, 90.0]);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("group")
    };
    let camera = crate::writer::NativeCameraSpec::root(319, 179);
    let certified = CertifiedCanvas {
        bounds: Bounds {
            min: [camera.center[0] - 130.0, camera.center[1] - 70.0],
            max: [camera.center[0] + 130.0, camera.center[1] + 70.0],
        },
        root_output: false,
        consumer_3d: true,
    };
    let HierarchyPlan::Precomposition(plan) = classify_precomposition_with_demand(
        group,
        Time::from_millis(2000),
        Duration24::from_frames(48).unwrap(),
        &[],
        &BTreeMap::new(),
        document.dimensions(),
        Some(&certified),
    )
    .unwrap() else {
        panic!("precomposition")
    };
    assert_eq!([plan.width, plan.height], [512, 256]);
    assert_eq!(plan.origin, [-96.5, -38.5]);
    assert_eq!(plan.camera.as_ref().unwrap(), &camera);
    assert!(plan.consumer_3d);
}

fn consumer_eligible(value: Value) -> bool {
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let layers = document.composition().layers();
    let LayerData::Group(group) = layers[0].data() else {
        panic!("group")
    };
    let dynamics = document.composition().dynamics().entries();
    let mut child = child_demand(
        group,
        group,
        &group.masks,
        dynamics,
        document.dimensions(),
        &root_demand(document.dimensions(), 2000),
    );
    child
        .use_3d_consumer_viewport(group, layers, dynamics)
        .is_ok()
}

#[test]
fn consumer_viewport_rejects_unsafe_owner_and_external_consumers() {
    assert!(consumer_eligible(document_value()));
    for (field, replacement) in [
        ("motionBlur", json!(true)),
        (
            "effects",
            json!([{"id":91000,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":10.0}}]),
        ),
        (
            "effects",
            json!([{"id":91000,"enabled":true,"effect":{"type":"vignette","amount":0.5}}]),
        ),
    ] {
        let mut value = document_value();
        value["composition"]["layers"][0][field] = replacement;
        assert!(!consumer_eligible(value), "{field}");
    }
    for (property, replacement) in [
        ("scale", json!([0.0, 100.0])),
        ("rotationX", json!(1.0)),
        ("skew", json!(1.0)),
    ] {
        let mut value = document_value();
        value["composition"]["layers"][0]["transform"][property] = replacement;
        assert!(!consumer_eligible(value), "{property}");
    }
    let mut value = document_value();
    let mut sibling = value["composition"]["layers"][0]["layers"][0].clone();
    sibling["id"] = json!(90002);
    sibling["parent"] = Value::Null;
    sibling["trackMatte"] = json!({"mode":"alpha","layer":90000});
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(sibling);
    assert!(!consumer_eligible(value));

    let mut value = document_value();
    let mut guide = value["composition"]["layers"][0]["layers"][0].clone();
    guide["id"] = json!(90002);
    guide["transform"]["rotationY"] = json!(0.0);
    value["composition"]["layers"][0]["layers"][0]["parent"] = json!(90002);
    value["composition"]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .push(guide);
    assert!(
        !consumer_eligible(value),
        "a planar parent must not shift a 3D world child"
    );
}

#[test]
fn static_inverse_demand_does_not_inflate_identity_groups() {
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(document_value()).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("group")
    };
    let mut demand = root_demand(document.dimensions(), 2000);
    for _ in 0..8 {
        demand = child_demand(group, group, &[], &[], document.dimensions(), &demand).propagated;
    }
    let bounds = demand.finite_union().unwrap();
    assert_eq!(bounds.min, [0.0, 0.0]);
    assert_eq!(bounds.max, [319.0, 179.0]);

    let mut occurrence = group.clone();
    occurrence.transform.position = Position::TwoD([1000.0, 400.0]);
    occurrence.transform.anchor_point = [50.0, 50.0];
    // Native separated-position sidecars reset the geometry carrier to identity.
    let demand = child_demand(
        &occurrence,
        group,
        &[],
        &[],
        document.dimensions(),
        &root_demand(document.dimensions(), 2000),
    );
    let bounds = demand.propagated().finite_union().unwrap();
    assert_eq!(bounds.min, [-950.0, -350.0]);
    assert_eq!(bounds.max, [-631.0, -171.0]);
}

#[test]
fn animated_inverse_keeps_offscreen_motion_and_rejects_scale_crossing() {
    use fx_schema::animator::{
        KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    };
    use fx_schema::{PropType, PropertyKeyframeEasing, PropertyTarget, PropertyValue, TimeOffset};
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(document_value()).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("group")
    };
    let entry = |property, values: [f64; 2]| AnimationGraphEntry {
        target: PropertyTarget::layer(group.id, property),
        animator: PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(i, value)| {
                        PropertyKeyframe::new(
                            KeyframeId::new(format!("inverse-{i}")),
                            TimeOffset::from_millis(i as i64 * 1000),
                            PropertyValue::Float(value),
                            PropertyKeyframeEasing::Linear,
                        )
                    })
                    .collect(),
            )
            .unwrap(),
        ),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let mut demand = root_demand(document.dimensions(), 2000);
    animated_bounds::inverse_planar_demand(
        &mut demand,
        group.id,
        &group.transform,
        &[entry(PropType::PositionX, [-5000.0, 5000.0])],
        document.dimensions(),
    )
    .unwrap();
    let bounds = demand.finite_union().unwrap();
    assert!(bounds.min[0] <= -5000.0 && bounds.max[0] >= 5319.0);
    let mut demand = root_demand(document.dimensions(), 2000);
    assert!(
        animated_bounds::inverse_planar_demand(
            &mut demand,
            group.id,
            &group.transform,
            &[entry(PropType::ScaleX, [100.0, -100.0])],
            document.dimensions()
        )
        .is_err()
    );
}

#[test]
fn oversized_consumer_export_preserves_native_world_values_and_reports_approximation() {
    for (x, rescued) in [(18000.0, false), (45000.0, true)] {
        let mut value = document_value();
        let group = &mut value["composition"]["layers"][0];
        group["transform"]["rotation"] = json!(1.0); // Not the final-root shortcut.
        group["layers"][0]["transform"]["position"] = json!([x, 90.0]);
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = super::super::to_aep(&document).unwrap();
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("subtree omitted")),
            "{:?}",
            output.diagnostics
        );
        assert_eq!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("finite consumer output viewport")),
            rescued
        );
        let native = read_project(&output.bytes).unwrap();
        let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
            panic!("root")
        };
        let ItemKind::Composition(source) =
            &native.item(root.layers[0].record.source_id()).unwrap().kind
        else {
            panic!("source")
        };
        assert_eq!(source.layers.len(), 2); // Editable plane and native camera, not media.
        if rescued {
            let plane = source
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == "Uncropped distant plane")
                .unwrap();
            let transform = crate::properties::read_transform(&plane.content).unwrap();
            assert_eq!(
                transform
                    .iter()
                    .find(|p| p.match_name == "ADBE Position")
                    .unwrap()
                    .numeric
                    .as_ref()
                    .unwrap()
                    .values,
                [45000.0, 90.0, 0.0]
            );
        }
    }
}

#[test]
fn oversized_consumer_accepts_planar_separated_position_sidecar() {
    use fx_schema::animator::{
        KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    };
    use fx_schema::{PropType, PropertyKeyframeEasing, PropertyTarget, PropertyValue, TimeOffset};
    let mut value = document_value();
    let group = &mut value["composition"]["layers"][0];
    group["transform"]["position"] = json!([159.5, 89.5]);
    group["transform"]["anchorPoint"] = json!([159.5, 89.5]);
    group["layers"][0]["transform"]["position"] = json!([45000.0, 90.0]);
    let entries: Vec<_> = [
        (PropType::PositionX, 1000, 159.5),
        (PropType::PositionY, 500, 89.5),
    ]
    .into_iter()
    .map(|(property, end, base)| AnimationGraphEntry {
        target: PropertyTarget::layer(LayerId::new(90000), property),
        animator: PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    KeyframeId::new(format!("{property:?}-start")),
                    TimeOffset::from_millis(0),
                    PropertyValue::Float(base),
                    PropertyKeyframeEasing::Linear,
                ),
                PropertyKeyframe::new(
                    KeyframeId::new(format!("{property:?}-end")),
                    TimeOffset::from_millis(end),
                    PropertyValue::Float(base + 20.0),
                    PropertyKeyframeEasing::Linear,
                ),
            ])
            .unwrap(),
        ),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    })
    .collect();
    value["composition"]["dynamics"]["entries"] = serde_json::to_value(entries).unwrap();
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("finite consumer output viewport"))
    );
    let native = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("root")
    };
    assert_eq!(root.layers.len(), 1);
    assert!(!root.layers[0].record.flags().three_d_layer);
    let properties = crate::properties::read_transform(&root.layers[0].content).unwrap();
    assert!(
        properties
            .iter()
            .find(|p| p.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .dimensions_separated
    );
}

#[test]
fn final_root_viewport_does_not_bypass_near_plane_validation() {
    let mut value = document_value();
    value["composition"]["layers"][0]["layers"][0]["transform"]["rotationY"] = json!(15.0);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("near plane"))
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("uses the output viewport"))
    );
}

fn eligible(value: Value) -> bool {
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let layers = document.composition().layers();
    let LayerData::Group(group) = layers[0].data() else {
        panic!("group")
    };
    root_viewport::canvas(
        group,
        document.composition().dynamics().entries(),
        layers,
        document.dimensions(),
    )
    .is_some()
}

#[test]
fn final_root_viewport_rejects_transforms_owner_effects_and_motion_blur() {
    for (field, replacement) in [
        (
            "transform",
            json!({"position":[1.0,0.0],"anchorPoint":[0.0,0.0],"scale":[100.0,100.0],"rotation":0.0,"opacity":100.0}),
        ),
        ("motionBlur", json!(true)),
        (
            "effects",
            json!([{"id":91000,"enabled":true,"effect":{"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}}]),
        ),
    ] {
        let mut value = document_value();
        value["composition"]["layers"][0][field] = replacement;
        assert!(!eligible(value), "{field}");
    }
}

#[test]
fn final_root_viewport_rejects_animated_owner_controls() {
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(document_value()).unwrap();
    let layers = document.composition().layers();
    let LayerData::Group(group) = layers[0].data() else {
        panic!("group")
    };
    let animation = AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(group.id, fx_schema::PropType::Rotation),
        animator: fx_schema::animator::PropertyAnimator::constant(fx_schema::PropertyValue::Float(
            0.0,
        ))
        .unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    assert!(root_viewport::canvas(group, &[animation], layers, document.dimensions()).is_none());
}

#[test]
fn final_root_viewport_rejects_matte_consumers_and_spatial_adjustments() {
    let mut value = document_value();
    let mut sibling = value["composition"]["layers"][0]["layers"][0].clone();
    sibling["id"] = json!(90002);
    sibling["parent"] = Value::Null;
    sibling["trackMatte"] = json!({"mode":"alpha","layer":90000});
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(sibling);
    assert!(!eligible(value));

    for (effect, expected) in [
        (json!({"type":"gaussianBlur","blurriness":10.0}), false),
        (json!({"type":"grain","intensity":1.0}), true),
        (json!({"type":"vignette","amount":0.5}), true),
    ] {
        let mut value = document_value();
        let mut adjustment = value["composition"]["layers"][0].clone();
        adjustment["type"] = json!("Adjustment");
        adjustment["activeRange"] = adjustment["playback"]["inputRange"].clone();
        adjustment.as_object_mut().unwrap().remove("playback");
        adjustment["id"] = json!(90003);
        adjustment.as_object_mut().unwrap().remove("layers");
        adjustment["effects"] = json!([{"id":91000,"enabled":true,"effect":effect}]);
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(adjustment);
        assert_eq!(eligible(value), expected);
    }
}
