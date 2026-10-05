use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::to_structural_fx_document;
use fx_schema::{PropType, animator::AnimationGraphEntry};
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
fn checked_source_domain_retains_inverse_crop_and_refuses_failed_ancestors() {
    let mut value = document_value();
    let group = &mut value["composition"]["layers"][0];
    group["transform"]["position"] = json!([50.0, 20.0]);
    group["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":500,"duration":1500}),
        json!({"start":0,"duration":3000}),
    );
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("native-source Group expected");
    };
    let dynamics = super::super::AnimationIndex::new(&[]);
    let dimensions = document.dimensions();
    let inherited = root_demand(dimensions, 2000);
    let mut demand = child_demand(group, group, &[], &dynamics, dimensions, &inherited, false);
    assert!(demand.propagated().finite_union().is_err());
    demand.use_checked_source_domain(checked_source_demand(
        group,
        &[],
        &dynamics,
        dimensions,
        &inherited,
        false,
        3000,
    ));
    let bounds = demand.propagated().finite_union().unwrap();
    assert_eq!(bounds.min, [-50.0, -20.0]);
    assert_eq!(bounds.max, [269.0, 159.0]);
    let segments = demand.propagated().segments();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].local_start, 0.0);
    assert_eq!(segments[0].root_end, 3000);
    let mut failed = inherited;
    failed.full("Unproved ancestor effect");
    demand.use_checked_source_domain(checked_source_demand(
        group,
        &[],
        &dynamics,
        dimensions,
        &failed,
        false,
        3000,
    ));
    assert_eq!(
        demand.propagated().finite_union().unwrap_err(),
        "Unproved ancestor effect"
    );
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
    let certified = root_viewport::canvas(
        group,
        &crate::export_document::AnimationIndex::new(&[]),
        layers,
        document.dimensions(),
    )
    .unwrap();
    let HierarchyPlan::Precomposition(plan) = classify_precomposition_with_demand(
        group,
        Time::from_millis(2000),
        Duration24::from_frames(48).unwrap(),
        &crate::export_document::AnimationIndex::new(&[]),
        &BTreeMap::new(),
        document.dimensions(),
        Some(&certified),
        None,
        None,
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
        source_mask: false,
        intersect_content: false,
    };
    let HierarchyPlan::Precomposition(plan) = classify_precomposition_with_demand(
        group,
        Time::from_millis(2000),
        Duration24::from_frames(48).unwrap(),
        &crate::export_document::AnimationIndex::new(&[]),
        &BTreeMap::new(),
        document.dimensions(),
        Some(&certified),
        None,
        None,
    )
    .unwrap() else {
        panic!("precomposition")
    };
    assert_eq!([plan.width, plan.height], [512, 256]);
    assert_eq!(plan.origin, [-96.5, -38.5]);
    assert_eq!(plan.camera.as_ref().unwrap(), &camera);
    assert!(plan.consumer_3d);
}

fn unit_window_world_value() -> Value {
    let mut value = document_value();
    let mut world = value["composition"]["layers"][0].clone();
    world["id"] = json!(900002);
    world["name"] = json!("WORLD");
    world["layers"][0]["id"] = json!(900016);
    world["layers"][0]["parent"] = json!(900002);
    world["layers"][0]["transform"]["position"] = json!([45000.0, 90.0]);
    let mut consumer = world["layers"][0].clone();
    consumer["id"] = json!(900033);
    consumer["name"] = json!("Internal matte consumer");
    consumer["trackMatte"] = json!({"mode":"alpha","layer":900016});
    world["layers"].as_array_mut().unwrap().push(consumer);
    let root = &mut value["composition"]["layers"][0];
    root["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":20533,"duration":2000}),
        json!({"start":0,"duration":2000}),
    );
    root["layers"] = json!([world]);
    value["duration"] = json!(22.533);
    value
}

#[test]
fn unit_window_consumer_keeps_nested_world_clock_camera_and_internal_matte() {
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(unit_window_world_value())
            .unwrap();
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
            .any(|d| d.message.contains("finite consumer output viewport"))
    );
    let native = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("root")
    };
    assert_eq!(root.layers[0].record.start_time(), Some(20.533));
    let ItemKind::Composition(root_source) =
        &native.item(root.layers[0].record.source_id()).unwrap().kind
    else {
        panic!("root source")
    };
    let world = root_source
        .layers
        .iter()
        .find(|l| l.name.as_ref() == "WORLD")
        .unwrap();
    assert_eq!(world.record.start_time(), Some(0.0));
    let ItemKind::Composition(source) = &native.item(world.record.source_id()).unwrap().kind else {
        panic!("world source")
    };
    assert_eq!(
        source.layers.len(),
        3,
        "two editable planes and unchanged native camera"
    );
    let plane = source
        .layers
        .iter()
        .find(|l| l.name.as_ref() == "Uncropped distant plane")
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
    let consumer = source
        .layers
        .iter()
        .find(|l| l.name.as_ref() == "Internal matte consumer")
        .unwrap();
    assert_eq!(consumer.record.matte_layer_id(), Some(plane.record.id()));
}

#[test]
fn unit_window_consumer_rejects_unproved_linear_clocks() {
    let playback = super::super::tests::fixture_linear_playback(
        json!({"start":20533,"duration":2000}),
        json!({"start":0,"duration":2000}),
    );
    let mut value = document_value();
    value["duration"] = json!(22.533);
    value["composition"]["layers"][0]["playback"] = playback.clone();
    assert!(consumer_eligible(value.clone()));
    for (label, clock) in [
        ("nonunit", {
            let mut clock = playback.clone();
            clock["mapping"]["output"]["duration"] = json!(1000);
            clock
        }),
        ("offset", {
            let mut clock = playback.clone();
            clock["inputOffsetMs"] = json!(1);
            clock
        }),
        ("source start", {
            let mut clock = playback.clone();
            clock["mapping"]["output"]["start"] = json!(1);
            clock
        }),
        ("TimeRemap", {
            let mut clock = playback.clone();
            clock["mapping"] = json!({"type":"timeRemap", "property": {
                "keyframes": [
                    {"id":"a", "time":20533, "value":0, "easing":{"type":"linear"}},
                    {"id":"b", "time":22533, "value":2000, "easing":{"type":"linear"}}
                ], "before":"inactive", "after":"inactive"
            }});
            clock
        }),
        ("independent mapping window", {
            let mut clock = playback.clone();
            clock["mapping"]["input"]["start"] = json!(20532);
            clock
        }),
    ] {
        value["composition"]["layers"][0]["playback"] = clock;
        assert!(!consumer_eligible(value.clone()), "{label}");
    }
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
        &crate::export_document::AnimationIndex::new(dynamics),
        document.dimensions(),
        &root_demand(document.dimensions(), 22533),
        false,
    );
    child
        .use_3d_consumer_viewport(
            group,
            layers,
            &crate::export_document::AnimationIndex::new(dynamics),
        )
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
fn exact_delayed_group_source_clock_keeps_finite_local_consumer_demand() {
    let mut value = document_value();
    let input = json!({"start":400,"duration":1600});
    let output = json!({"start":0,"duration":1600});
    value["composition"]["layers"][0]["playback"] =
        super::super::tests::fixture_linear_playback(input.clone(), output.clone());
    let dimensions = fx_schema::Dimensions::new(319, 179);
    let check = |value: Value| {
        let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let LayerData::Group(group) = doc.composition().layers()[0].data() else {
            panic!("group")
        };
        let animations = crate::export_document::AnimationIndex::new(&[]);
        child_demand(
            group,
            group,
            &[],
            &animations,
            dimensions,
            &root_demand(dimensions, 2_000),
            false,
        )
        .propagated()
        .clone()
    };
    let demand = check(value.clone());
    assert!(demand.finite_union().is_ok());
    assert_eq!(demand.segments()[0].root_start, 400);
    assert_eq!(demand.segments()[0].local_start, 0.0);

    value["composition"]["layers"][0]["playback"]["mapping"]["output"]["duration"] = json!(800);
    assert!(check(value.clone()).finite_union().is_err(), "nonunit rate");
    value["composition"]["layers"][0]["playback"]["mapping"]["output"] = output;
    value["composition"]["layers"][0]["playback"]["inputOffsetMs"] = json!(1);
    assert!(
        check(value).finite_union().is_err(),
        "offset is not unit shift"
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
        demand = child_demand(
            group,
            group,
            &[],
            &crate::export_document::AnimationIndex::new(&[]),
            document.dimensions(),
            &demand,
            false,
        )
        .propagated;
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
        &crate::export_document::AnimationIndex::new(&[]),
        document.dimensions(),
        &root_demand(document.dimensions(), 2000),
        false,
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
        &crate::export_document::AnimationIndex::new(&[entry(
            PropType::PositionX,
            [-5000.0, 5000.0],
        )]),
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
            &crate::export_document::AnimationIndex::new(&[entry(
                PropType::ScaleX,
                [100.0, -100.0]
            )]),
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
fn final_root_pointwise_adjustment_keeps_planar_text_and_checks_spatial_siblings() {
    for reverse in [false, true] {
        let mut value = document_value();
        let children = value["composition"]["layers"][0]["layers"]
            .as_array_mut()
            .unwrap();
        children.push(
            json!({"type":"Text","id":90002,"name":"Pointwise planar title",
            "parent":90000,"activeRange":{"start":0,"duration":2000},
            "transform":super::super::identity_fx_transform(),
            "sourceText":{"text":"AGENTS","fontFamily":"Inter-Regular","fontStyle":"Regular",
                "fontSize":42.0,"applyFill":true,"fillColor":[1.0,1.0,1.0,1.0]}}),
        );
        children.push(
            json!({"type":"Adjustment","id":90003,"name":"Retained Exposure",
            "parent":90000,"activeRange":{"start":0,"duration":2000},
            "transform":super::super::identity_fx_transform(),
            "effects":[{"type":"exposure","exposure":1.0}]}),
        );
        if reverse {
            children.reverse();
        }
        let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
        let output = super::super::to_aep(&doc).unwrap();
        assert!(
            !output.omitted_layer_ids.contains(&LayerId::new(90000)),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        for name in ["Pointwise planar title", "Retained Exposure"] {
            assert!(
                native.items.iter().any(|item| match &item.kind {
                    ItemKind::Composition(comp) =>
                        comp.layers.iter().any(|layer| layer.name.as_ref() == name),
                    _ => false,
                }),
                "missing native {name}"
            );
        }
        let plane = value["composition"]["layers"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == 90001)
            .unwrap();
        plane["transform"]["rotationY"] = json!(15.0);
        let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let rejected = super::super::to_aep(&doc).unwrap();
        assert!(rejected.omitted_layer_ids.contains(&LayerId::new(90000)));
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|d| d.message.contains("near plane"))
        );
    }
}

#[test]
fn final_root_spatial_sibling_keeps_planar_editable_text_and_checks_every_sibling_order() {
    let base = || {
        let mut value = document_value();
        let text = json!({
            "type":"Text", "id":90002, "name":"S03 AGENTS planar title",
            "parent":90000, "activeRange":{"start":0,"duration":2000},
            "transform":super::super::identity_fx_transform(),
            "sourceText":{"text":"AGENTS","fontFamily":"Inter-Regular",
                "fontStyle":"Regular","fontSize":42.0,"applyFill":true,
                "fillColor":[1.0,1.0,1.0,1.0]}
        });
        value["composition"]["layers"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .push(text);
        value
    };
    for reverse in [false, true] {
        let mut value = base();
        if reverse {
            value["composition"]["layers"][0]["layers"]
                .as_array_mut()
                .unwrap()
                .reverse();
        }
        let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
        let output = super::super::to_aep(&doc).unwrap();
        assert!(
            !output.omitted_layer_ids.contains(&LayerId::new(90000)),
            "{:?}",
            output.diagnostics
        );
        assert!(!output.omitted_layer_ids.contains(&LayerId::new(90002)));
        let native = read_project(&output.bytes).unwrap();
        assert!(native.items.iter().any(|item| match &item.kind {
            ItemKind::Composition(comp) => comp.layers.iter().any(|layer| {
                layer.name.as_ref() == "S03 AGENTS planar title" && layer.record.layer_type() == 3
            }),
            _ => false,
        }));
        let plane = value["composition"]["layers"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == 90001)
            .unwrap();
        plane["transform"]["rotationY"] = json!(15.0);
        let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let rejected = super::super::to_aep(&doc).unwrap();
        assert!(
            rejected.omitted_layer_ids.contains(&LayerId::new(90000)),
            "sibling order {reverse}"
        );
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|d| d.message.contains("near plane"))
        );
    }
    let mut value = base();
    value["composition"]["layers"][0]["layers"][1]["transform"]["rotationX"] = json!(1.0);
    let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let rejected = super::super::to_aep(&doc).unwrap();
    assert!(rejected.omitted_layer_ids.contains(&LayerId::new(90000)));
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Projective Text"))
    );
}

#[test]
fn nested_planar_consumer_keeps_text_beside_spatial_content_without_hiding_near_plane_crossings() {
    let mut value = document_value();
    let mut plane = value["composition"]["layers"][0]["layers"][0].clone();
    plane["parent"] = json!(90010);
    let source = json!({
        "type":"Group","id":90010,"name":"S06 spatial source with planar title","parent":90000,
        "playback":super::super::tests::fixture_linear_playback(
            json!({"start":0,"duration":2000}),json!({"start":0,"duration":2000})),
        "transform":super::super::identity_fx_transform(),
        "layers":[plane,{
            "type":"Text","id":90002,"name":"S06 retained planar words","parent":90010,
            "activeRange":{"start":0,"duration":2000},
            "transform":super::super::identity_fx_transform(),
            "sourceText":{"text":"MAKE IT BLUE","fontFamily":"Inter-Regular",
                "fontStyle":"Regular","fontSize":42.0,"applyFill":true,"fillColor":[1.0,1.0,1.0,1.0]}
        }]
    });
    value["composition"]["layers"][0]["layers"] = json!([source]);
    for reverse in [false, true] {
        let mut input = value.clone();
        if reverse {
            input["composition"]["layers"][0]["layers"][0]["layers"]
                .as_array_mut()
                .unwrap()
                .reverse();
        }
        let document =
            fx_schema::EditableFxCompositionDocument::from_json_value(input.clone()).unwrap();
        let output = super::super::to_aep(&document).unwrap();
        for id in [90000, 90010, 90001, 90002] {
            assert!(
                !output.omitted_layer_ids.contains(&LayerId::new(id)),
                "{id}: {:?}",
                output.diagnostics
            );
        }
        let native = read_project(&output.bytes).unwrap();
        assert!(native.items.iter().any(|item| {
            match &item.kind {
                ItemKind::Composition(comp) => comp.layers.iter().any(|layer| {
                    layer.record.layer_type() == 3
                        && layer.name.as_ref() == "S06 retained planar words"
                }),
                _ => false,
            }
        }));
        let plane = input["composition"]["layers"][0]["layers"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == 90001)
            .unwrap();
        plane["transform"]["rotationY"] = json!(15.0);
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(input).unwrap();
        let rejected = super::super::to_aep(&document).unwrap();
        assert!(rejected.omitted_layer_ids.contains(&LayerId::new(90000)));
        assert!(
            rejected
                .diagnostics
                .iter()
                .any(|d| d.message.contains("near plane"))
        );
    }
    value["composition"]["layers"][0]["layers"][0]["layers"][1]["transform"]["rotationY"] =
        json!(1.0);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let rejected = super::super::to_aep(&document).unwrap();
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Projective Text"))
    );
}

fn masked_projective_door_value() -> Value {
    let mut value = document_value();
    let mut paint = value["composition"]["layers"][0]["layers"][0].clone();
    paint["parent"] = json!(90010);
    paint["transform"] = json!(super::super::identity_fx_transform());
    let mut guide = paint.clone();
    guide["id"] = json!(90003);
    guide["parent"] = Value::Null;
    guide["name"] = json!("Door source clip Rect");
    guide["rect"]["position"] = json!([0.0, 0.0]);
    guide["rect"]["size"] = json!([200.0, 140.0]);
    guide["rect"]["roundness"] = json!(0.0);
    guide["isHidden"] = json!(true);
    let door = json!({
        "type":"Group","id":90010,"name":"Masked door plane","parent":90000,
        "transform":super::super::identity_fx_transform(),
        "playback":super::super::tests::fixture_linear_playback(
            json!({"start":0,"duration":2000}),json!({"start":0,"duration":2000})),
        "layers":[{
            "type":"Text","id":90002,"name":"Title inside source door","parent":90010,
            "activeRange":{"start":0,"duration":2000},"transform":super::super::identity_fx_transform(),
            "sourceText":{"text":"AFTER EFFECTS","fontFamily":"Inter-Regular","fontStyle":"Regular",
                "fontSize":42.0,"applyFill":true,"fillColor":[1.0,1.0,1.0,1.0]}
        },paint,guide],
        "masks":[{"id":90004,"layer":90003,"mode":"add","inverted":false,
            "feather":[0.0,0.0],"opacity":1.0,"expansion":0.0}]
    });
    value["composition"]["layers"][0]["layers"] = json!([door]);
    value["composition"]["dynamics"]["entries"] = json!([super::super::tests::keyed_entry(
        LayerId::new(90010),
        PropType::RotationY,
        [
            (0, fx_schema::PropertyValue::Float(0.0)),
            (1000, fx_schema::PropertyValue::Float(20.0))
        ]
    )]);
    value
}

#[test]
fn native_control_fixtures_pin_the_declared_group_effect_planes() {
    use sha2::Digest;
    let bulge_bytes =
        include_bytes!("../../../tests/fixtures/group_effect_domains/logical_bulge.aep");
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(bulge_bytes)),
        "3d32c8076c57d70f8b72c871353fd19ccf2b10145f2e4697c780db6ed9c3435b"
    );
    let native = read_project(bulge_bytes).unwrap();
    for (id, source_size, center) in [
        (230, [6144, 4096], [3008.0, 2054.4]),
        (471, [1920, 1080], [960.0, 518.4]),
    ] {
        let ItemKind::Composition(comp) = &native.item(id).unwrap().kind else {
            panic!("native target")
        };
        assert_eq!((comp.width, comp.height), (1920, 1080));
        let owner = &comp.layers[0];
        let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind
        else {
            panic!("native source")
        };
        assert_eq!([source.width, source.height], source_size);
        let (effects, warnings) =
            crate::effects::native::read_effects(&owner.content, source_size.map(f64::from));
        assert!(warnings.is_empty(), "{warnings:?}");
        let effect = effects
            .iter()
            .find(|effect| effect.match_name == "ADBE Bulge")
            .unwrap();
        let values = |name: &str| {
            effect
                .parameters
                .iter()
                .find(|parameter| parameter.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
                .values
                .clone()
        };
        for (name, expected) in [
            ("ADBE Bulge-0001", vec![1632.0]),
            ("ADBE Bulge-0002", vec![1026.0]),
            ("ADBE Bulge-0003", center.to_vec()),
            ("ADBE Bulge-0004", vec![0.08]),
            ("ADBE Bulge-0007", vec![0.0]),
        ] {
            // Native Point storage narrows to f32 (e.g.2054.39990234375).
            // This is a storage-precision assertion, not a pixel threshold.
            let actual = values(name);
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(expected) {
                assert!(
                    (actual - expected).abs() <= f64::from(f32::EPSILON) * expected.abs().max(1.0),
                    "{name}: {actual} != {expected}"
                );
            }
        }
    }
    let mask_bytes =
        include_bytes!("../../../tests/fixtures/group_effect_domains/inline_add_mask.aep");
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(mask_bytes)),
        "a5e19d66f4e66891f3e9c40d2c06329e04dfcebdf9c2d608bce5d4b3d9e10422"
    );
    let native = read_project(mask_bytes).unwrap();
    fn has_mask(chunks: &[crate::rifx::Chunk]) -> bool {
        crate::properties::runs(chunks)
            .is_ok_and(|runs| runs.iter().any(|(name, _)| *name == "ADBE Mask Shape"))
            || chunks
                .iter()
                .filter_map(crate::rifx::Chunk::children)
                .any(has_mask)
    }
    for (id, size) in [(103, [4096, 3072]), (217, [1920, 1080])] {
        let ItemKind::Composition(comp) = &native.item(id).unwrap().kind else {
            panic!("mask target")
        };
        assert_eq!((comp.width, comp.height), (4096, 3072));
        let owner = &comp.layers[0];
        assert!(has_mask(&owner.content));
        let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind
        else {
            panic!("mask source")
        };
        assert_eq!([source.width, source.height], size);
    }
}

fn assert_bulge_values(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }
}

fn logical_bulge_value(width: u32, height: u32) -> Value {
    let mut value = masked_projective_door_value();
    value["dimensions"] = json!({"width":width,"height":height});
    value["composition"]["dynamics"]["entries"] = json!([]);
    let group = &mut value["composition"]["layers"][0]["layers"][0];
    group["layers"].as_array_mut().unwrap().pop();
    group["masks"] = json!([]);
    group["name"] = json!("Logical Bulge owner");
    group["transform"]["position"] = json!([400.0, 267.0]);
    group["effects"] = json!([{"id":90005,"enabled":true,"effect":{
        "type":"bulge","centerX":0.5,"centerY":0.48,"horizontalRadius":0.85,
        "verticalRadius":0.95,"bulgeHeight":0.08,"pinning":false
    }}]);
    value
}

#[test]
fn group_corner_pin_keeps_logical_source_plane_without_capping_points() {
    let mut value = logical_bulge_value(1920, 1080);
    let group = &mut value["composition"]["layers"][0]["layers"][0];
    group["name"] = json!("Logical Corner Pin owner");
    group["effects"] = json!([{"id":90005,"enabled":true,"effect":{
        "type":"cornerPin","upperLeftX":0,"upperLeftY":0,
        "upperRightX":1,"upperRightY":0,"lowerLeftX":-10.802704436383948,
        "lowerLeftY":6.19239516513639,"lowerRightX":1,"lowerRightY":1
    }}]);
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let exported = super::super::to_aep(&document).unwrap();
    assert!(
        exported.omitted_layer_ids.is_empty(),
        "{:?}",
        exported.diagnostics
    );
    let native = read_project(&exported.bytes).unwrap();
    let owner = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == "Logical Corner Pin owner")
        .unwrap();
    let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind else {
        panic!("source")
    };
    assert_eq!((source.width, source.height), (1920, 1080));
    {
        let earlier = json!({"id":90006,"enabled":true,"effect":{
            "type":"bulge","centerX":0.5,"centerY":0.5,"horizontalRadius":0.5,
            "verticalRadius":0.5,"bulgeHeight":0.1,"pinning":false
        }});
        let mut guarded = value.clone();
        guarded["composition"]["layers"][0]["layers"][0]["effects"]
            .as_array_mut()
            .unwrap()
            .insert(0, earlier);
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(guarded).unwrap();
        let LayerData::Group(root) = document.composition().layers()[0].data() else {
            panic!("root")
        };
        let LayerData::Group(group) = root.layers[0].data() else {
            panic!("group")
        };
        assert!(
            super::super::logical_corner_pin_source_bounds(group, document.dimensions()).is_none()
        );
    }
}

#[test]
fn group_bulge_keeps_declared_logical_plane_and_all_editable_children() {
    for (width, height) in [(1920, 1080), (640, 360)] {
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(
            logical_bulge_value(width, height),
        )
        .unwrap();
        let LayerData::Group(root) = document.composition().layers()[0].data() else {
            panic!("root")
        };
        let LayerData::Group(group) = root.layers[0].data() else {
            panic!("Bulge Group")
        };
        let dynamics = super::super::AnimationIndex::new(&[]);
        let input =
            super::super::logical_bulge_source_bounds(group, &dynamics, document.dimensions())
                .unwrap();
        assert_eq!(input.min, [0.0, 0.0]);
        assert_eq!(input.max, [f64::from(width), f64::from(height)]);
        let output =
            super::super::logical_bulge_output_bounds(group, &dynamics, document.dimensions())
                .unwrap();
        assert!(
            output.min[0] < 0.0 && output.max[0] > f64::from(width),
            "output expansion must not be confused with input support"
        );
        let exported = super::super::to_aep(&document).unwrap();
        for id in [90000, 90010, 90002, 90001] {
            assert!(
                !exported.omitted_layer_ids.contains(&LayerId::new(id)),
                "{id}: {:?}",
                exported.diagnostics
            );
        }
        let native = read_project(&exported.bytes).unwrap();
        let owner = native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(&comp.layers),
                _ => None,
            })
            .flatten()
            .find(|layer| layer.name.as_ref() == "Logical Bulge owner")
            .unwrap();
        let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind
        else {
            panic!("source")
        };
        assert_eq!((source.width, source.height), (width as u16, height as u16));
        let (effects, _) = crate::effects::native::read_effects(
            &owner.content,
            [f64::from(source.width), f64::from(source.height)],
        );
        let bulge = effects
            .iter()
            .find(|effect| effect.match_name == "ADBE Bulge")
            .unwrap();
        let values = |name: &str| {
            bulge
                .parameters
                .iter()
                .find(|param| param.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
                .values
                .clone()
        };
        assert_bulge_values(&values("ADBE Bulge-0001"), &[0.85 * f64::from(width)]);
        assert_bulge_values(&values("ADBE Bulge-0002"), &[0.95 * f64::from(height)]);
        assert_bulge_values(
            &values("ADBE Bulge-0003"),
            &[0.5 * f64::from(width), 0.48 * f64::from(height)],
        );
    }
}

#[test]
fn known_bounds_group_bulge_uses_root_plane_even_inside_a_smaller_parent_capture() {
    let mut value = logical_bulge_value(640, 360);
    let group = &mut value["composition"]["layers"][0]["layers"][0];
    group["layers"].as_array_mut().unwrap().remove(0);
    group["layers"][0]["rect"]["position"] = json!([100.0, 200.0]);
    group["layers"][0]["rect"]["size"] = json!([120.0, 80.0]);
    group["layers"][0]["rect"]["strokeEnabled"] = json!(false);
    group["effects"][0]["effect"]["bulgeHeight"] = json!(0.0);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let exported = super::super::to_aep(&document).unwrap();
    assert!(
        exported.omitted_layer_ids.is_empty(),
        "{:?}",
        exported.diagnostics
    );
    let native = read_project(&exported.bytes).unwrap();
    let owner = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == "Logical Bulge owner")
        .unwrap();
    let ItemKind::Composition(source) = &native.item(owner.record.source_id()).unwrap().kind else {
        panic!("source")
    };
    assert!(source.width < 640 && source.height < 360);
    let (effects, _) = crate::effects::native::read_effects(
        &owner.content,
        [f64::from(source.width), f64::from(source.height)],
    );
    let bulge = effects
        .iter()
        .find(|effect| effect.match_name == "ADBE Bulge")
        .unwrap();
    let values = |name: &str| {
        bulge
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == name)
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values
            .clone()
    };
    assert_bulge_values(&values("ADBE Bulge-0001"), &[544.0]);
    assert_bulge_values(&values("ADBE Bulge-0002"), &[342.0]);
    assert_bulge_values(&values("ADBE Bulge-0003"), &[220.0, -27.2]);
}

#[test]
fn group_bulge_normalization_preserves_logical_point_keys_across_capture_origin() {
    let mut value = logical_bulge_value(640, 360);
    let mut entry = serde_json::to_value(super::super::tests::keyed_entry(
        LayerId::new(90010),
        PropType::Rotation,
        [
            (0, fx_schema::PropertyValue::Float(0.5)),
            (1000, fx_schema::PropertyValue::Float(0.75)),
        ],
    ))
    .unwrap();
    entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(90005),
        "centerX",
    ))
    .unwrap();
    value["composition"]["dynamics"]["entries"] = json!([entry]);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let LayerData::Group(root) = document.composition().layers()[0].data() else {
        panic!("root")
    };
    let LayerData::Group(group) = root.layers[0].data() else {
        panic!("Group")
    };
    let dynamics = super::super::AnimationIndex::new(document.composition().dynamics().entries());
    let mut native = super::super::effects::lower(&group.effects, &dynamics, [120.0, 80.0]).effects;
    super::super::normalize_group_bulge_frame(
        &mut native,
        &group.effects,
        &dynamics,
        document.dimensions(),
        [100.0, -20.0],
    );
    let center = native[0]
        .properties
        .iter()
        .find(|prop| prop.match_name == "ADBE Bulge-0003")
        .unwrap();
    assert_bulge_values(&center.values, &[220.0, 192.8]);
    let keys = &center.animation.as_ref().unwrap().keys;
    assert_eq!(keys.len(), 2);
    assert_bulge_values(&keys[0].values, &[220.0, 192.8]);
    assert_bulge_values(&keys[1].values, &[380.0, 192.8]);
    assert_eq!(keys[1].time_millis, 1000);
    assert_eq!(
        native[0]
            .properties
            .iter()
            .find(|prop| prop.match_name == "ADBE Bulge-0001")
            .unwrap()
            .values,
        [544.0]
    );
}

#[test]
fn logical_bulge_source_rejects_unproved_spatial_text_inputs_and_bypass() {
    for variant in ["spatial Text", "zero height", "pinning", "animated center"] {
        let mut value = logical_bulge_value(640, 360);
        match variant {
            "spatial Text" => {
                value["composition"]["layers"][0]["layers"][0]["layers"][0]["effects"] = json!([{"id":90006,"enabled":true,"effect":{"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}}])
            }
            "zero height" => {
                value["composition"]["layers"][0]["layers"][0]["effects"][0]["effect"]["bulgeHeight"] =
                    json!(0.0)
            }
            "pinning" => {
                value["composition"]["layers"][0]["layers"][0]["effects"][0]["effect"]["pinning"] =
                    json!(true)
            }
            "animated center" => {
                let mut entry = serde_json::to_value(super::super::tests::keyed_entry(
                    LayerId::new(90010),
                    PropType::Rotation,
                    [
                        (0, fx_schema::PropertyValue::Float(0.5)),
                        (1000, fx_schema::PropertyValue::Float(0.75)),
                    ],
                ))
                .unwrap();
                entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
                    fx_schema::EffectId::new(90005),
                    "centerX",
                ))
                .unwrap();
                value["composition"]["dynamics"]["entries"] = json!([entry]);
            }
            _ => unreachable!(),
        }
        let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let LayerData::Group(root) = doc.composition().layers()[0].data() else {
            panic!("root")
        };
        let LayerData::Group(group) = root.layers[0].data() else {
            panic!("Group")
        };
        let dynamics = super::super::AnimationIndex::new(doc.composition().dynamics().entries());
        assert!(
            super::super::logical_bulge_source_bounds(group, &dynamics, doc.dimensions()).is_none(),
            "{variant}"
        );
    }
}

fn inline_rounded_mask_value() -> Value {
    let mut value = masked_projective_door_value();
    value["dimensions"] = json!({"width":1920,"height":1080});
    value["composition"]["dynamics"]["entries"] = json!([]);
    let door = &mut value["composition"]["layers"][0]["layers"][0];
    door["layers"].as_array_mut().unwrap().pop();
    door["masks"] = json!([{
        "id":90004,"mode":"add","inverted":false,
        "feather":[0.0,0.0],"opacity":100.0,"expansion":0.0,
        "path":{"commands":[
            {"type":"moveTo","x":76.0,"y":0.0},
            {"type":"lineTo","x":1844.0,"y":0.0},
            {"type":"cubicTo","c1x":1885.97364098714,"c1y":0.0,
                "c2x":1920.0,"c2y":34.026359012859686,"x":1920.0,"y":76.0},
            {"type":"lineTo","x":1920.0,"y":1004.0},
            {"type":"cubicTo","c1x":1920.0,"c1y":1045.9736409871402,
                "c2x":1885.97364098714,"c2y":1080.0,"x":1844.0,"y":1080.0},
            {"type":"lineTo","x":76.0,"y":1080.0},
            {"type":"cubicTo","c1x":34.026359012859686,"c1y":1080.0,
                "c2x":0.0,"c2y":1045.9736409871402,"x":0.0,"y":1004.0},
            {"type":"lineTo","x":0.0,"y":76.0},
            {"type":"cubicTo","c1x":0.0,"c1y":34.026359012859686,
                "c2x":34.026359012859686,"c2y":0.0,"x":76.0,"y":0.0},
            {"type":"close"}
        ]}
    }]);
    value
}

#[test]
fn inline_hard_add_mask_encloses_nested_text_without_dropping_root_or_sibling() {
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(inline_rounded_mask_value())
            .unwrap();
    let layers = document.composition().layers();
    let LayerData::Group(root) = layers[0].data() else {
        panic!("root Group")
    };
    let LayerData::Group(door) = root.layers[0].data() else {
        panic!("masked Group")
    };
    let dynamics = super::super::AnimationIndex::new(document.composition().dynamics().entries());
    let support =
        super::super::source_rect_mask_bounds(door, &dynamics, document.dimensions()).unwrap();
    assert_eq!(support.min, [0.0, 0.0]);
    assert_eq!(support.max, [1920.0, 1080.0]);
    let bounds = all_time_layer_bounds(
        &root.layers[0],
        &dynamics,
        &BTreeMap::new(),
        document.dimensions(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(bounds.min, support.min);
    assert_eq!(bounds.max, support.max);
    let output = super::super::to_aep(&document).unwrap();
    for id in [90000, 90010, 90002, 90001] {
        assert!(
            !output.omitted_layer_ids.contains(&LayerId::new(id)),
            "{id}: {:?}",
            output.diagnostics
        );
    }
    let native = read_project(&output.bytes).unwrap();
    let native_layers: Vec<_> = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(
        native_layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Title inside source door"
                && layer.record.layer_type() == 3)
    );
    assert!(
        native_layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Uncropped distant plane")
    );
}

#[test]
fn inline_mask_support_refuses_soft_or_spatial_text_input() {
    for variant in ["feather", "invert", "text glow"] {
        let mut value = inline_rounded_mask_value();
        let door = &mut value["composition"]["layers"][0]["layers"][0];
        match variant {
            "feather" => door["masks"][0]["feather"] = json!([1.0, 1.0]),
            "invert" => door["masks"][0]["inverted"] = json!(true),
            "text glow" => {
                door["layers"][0]["effects"] = json!([{"id":90005,"enabled":true,
                "effect":{"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}}])
            }
            _ => unreachable!(),
        }
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let LayerData::Group(root) = document.composition().layers()[0].data() else {
            panic!("root Group")
        };
        let LayerData::Group(door) = root.layers[0].data() else {
            panic!("masked Group")
        };
        assert!(
            super::super::source_rect_mask_bounds(
                door,
                &super::super::AnimationIndex::new(&[]),
                document.dimensions()
            )
            .is_none(),
            "{variant}"
        );
    }
}

#[test]
fn local_source_rect_mask_retains_text_before_animated_door_projection() {
    for reverse in [false, true] {
        let mut value = masked_projective_door_value();
        if reverse {
            value["composition"]["layers"][0]["layers"][0]["layers"]
                .as_array_mut()
                .unwrap()
                .reverse();
        }
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = super::super::to_aep(&document).unwrap();
        for id in [90000, 90010, 90002, 90001] {
            assert!(
                !output.omitted_layer_ids.contains(&LayerId::new(id)),
                "{id}: {:?}",
                output.diagnostics
            );
        }
        let native = read_project(&output.bytes).unwrap();
        let layers: Vec<_> = native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(&comp.layers),
                _ => None,
            })
            .flatten()
            .collect();
        assert!(
            layers
                .iter()
                .any(|layer| layer.name.as_ref() == "Title inside source door"
                    && layer.record.layer_type() == 3)
        );
        let door = layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Masked door plane")
            .unwrap();
        assert!(door.record.flags().three_d_layer);
        fn has_mask(chunks: &[crate::rifx::Chunk]) -> bool {
            if crate::properties::runs(chunks)
                .is_ok_and(|runs| runs.iter().any(|(name, _)| *name == "ADBE Mask Shape"))
            {
                return true;
            }
            chunks
                .iter()
                .filter_map(crate::rifx::Chunk::children)
                .any(has_mask)
        }
        assert!(
            has_mask(&door.content),
            "the clipping source must remain an editable native mask"
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.layer_id == Some(LayerId::new(90010))
                    && d.message.contains("guide layer 90003 was copied"))
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Mask 1 omitted"))
        );
    }
}

#[test]
fn source_rect_mask_preserves_exact_held_placement_and_its_all_key_support() {
    let mut value = masked_projective_door_value();
    let mut guide_motion = json!(super::super::tests::keyed_entry(
        LayerId::new(90003),
        PropType::PositionY,
        [
            (500, fx_schema::PropertyValue::Float(20.0)),
            (510, fx_schema::PropertyValue::Float(-20.0))
        ],
    ));
    for key in guide_motion["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
    {
        key["easing"] = json!({"type":"hold"});
    }
    value["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(guide_motion);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let group = match document.composition().layers()[0].data() {
        LayerData::Group(root) => match root.layers[0].data() {
            LayerData::Group(door) => door,
            _ => panic!("door fixture"),
        },
        _ => panic!("root fixture"),
    };
    let dynamics = super::super::animation_index::AnimationIndex::new(
        document.composition().dynamics().entries(),
    );
    let bounds = super::super::source_rect_mask_bounds(
        group,
        &dynamics,
        fx_schema::Dimensions {
            width: 319,
            height: 179,
        },
    )
    .unwrap();
    assert_eq!(bounds.min, [0.0, -20.0]);
    assert_eq!(bounds.max, [200.0, 160.0]);
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(90002)),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let roundtrip = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let entries = roundtrip["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let native_mask = entries
        .iter()
        .find(|e| e["target"]["propertyType"] == "shapePath")
        .expect("editable native mask Path keys");
    let keys = native_mask["animator"]["keyframes"].as_array().unwrap();
    assert!(keys.iter().any(|k| k["layerTime"] == 500));
    assert!(keys.iter().any(|k| k["layerTime"] == 510));
    // The native reader normalizes the first key's unused incoming easing
    // to Linear. Every actual incoming segment must retain Hold semantics.
    assert!(keys.iter().skip(1).all(|k| k["easing"]["type"] == "hold"));
    assert_eq!(keys[0]["value"], keys[1]["value"]);
}

#[test]
fn source_rect_mask_keeps_overlay_video_and_rejects_its_real_camera_crossing() {
    use crate::writer::footage::{NativeFrameRate, NativeSourceFormat, RelativeMediaPath};
    let sources = BTreeMap::from([(
        "door-movie".into(),
        media::ResolvedMediaSource {
            asset_id: fx_schema::AssetId::new("door-movie").unwrap(),
            path: RelativeMediaPath::new("media/door.mov").unwrap(),
            format: NativeSourceFormat::QuickTime,
            dimensions: [200, 140],
            duration_millis: 2000,
            duration_millis_floor: 2000,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(24),
            audio_sample_rate: 0.0,
            wave_metadata: None,
            native_duration: None,
        },
    )]);
    for crossing in [false, true] {
        let mut sources = sources.clone();
        if crossing {
            sources.get_mut("door-movie").unwrap().dimensions = [4000, 140];
        }
        let mut value = masked_projective_door_value();
        let mut video = json!({
            "type":"Video","id":90006,"name":"Editable Overlay footage","parent":90010,
            "blendMode":"overlay","transform":super::super::identity_fx_transform(),
            "sourceRange":{"start":0,"duration":2000},"sourceIntrinsicDuration":2000,
            "playback":super::super::tests::fixture_linear_playback(
                json!({"start":0,"duration":2000}),json!({"start":0,"duration":2000})),
            "source":{"assetId":"door-movie","fit":"contain"}
        });
        if crossing {
            video["transform"]["position"] = json!([18000.0, 90.0]);
            video["transform"]["rotationY"] = json!(15.0);
        }
        value["composition"]["layers"][0]["layers"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .push(video);
        let doc = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = super::super::to_aep_with_media(&doc, &sources).unwrap();
        if crossing {
            assert!(
                output.omitted_layer_ids.contains(&LayerId::new(90000)),
                "{:?}",
                output.diagnostics
            );
            assert!(
                output
                    .diagnostics
                    .iter()
                    .any(|d| d.message.contains("near plane"))
            );
            continue;
        }
        for id in [90000, 90010, 90002, 90006] {
            assert!(
                !output.omitted_layer_ids.contains(&LayerId::new(id)),
                "{id}: {:?}",
                output.diagnostics
            );
        }
        let native = read_project(&output.bytes).unwrap();
        let video = native
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) => comp
                    .layers
                    .iter()
                    .find(|layer| layer.name.as_ref() == "Editable Overlay footage"),
                _ => None,
            })
            .unwrap();
        assert_eq!(video.record.blend_mode(), 7);
        assert_ne!(video.record.source_id(), 0);
    }
}

fn masked_affine_counter_value() -> serde_json::Value {
    let mut value = masked_projective_door_value();
    let door = &mut value["composition"]["layers"][0]["layers"][0];
    door["layers"].as_array_mut().unwrap().push(json!({
        "type":"Shape","id":90007,"name":"Animated counter subtraction","parent":90010,
        "transform":super::super::identity_fx_transform(),"activeRange":{"start":0,"duration":2000},
        "shape":{"path":{"commands":[{"type":"moveTo","x":10.0,"y":10.0},
            {"type":"lineTo","x":30.0,"y":10.0},{"type":"lineTo","x":20.0,"y":30.0},{"type":"close"}]},
            "fills":[{"paint":{"type":"solid","color":[1.0,1.0,1.0,1.0]},"fillRule":"nonZeroWinding","blendMode":"normal","opacity":1.0}]}
    }));
    door["masks"].as_array_mut().unwrap().push(json!({
        "id":90008,"layer":90007,"mode":"subtract","opacity":1.0,"feather":[0.0,0.0],"expansion":0.0,"inverted":false
    }));
    value
}

#[test]
fn source_rect_mask_keeps_exact_linear_affine_subtraction_without_expanding_support() {
    let mut value = masked_affine_counter_value();
    for entry in [
        super::super::tests::keyed_entry(
            LayerId::new(90007),
            PropType::PositionX,
            [
                (0, fx_schema::PropertyValue::Float(-100000.0)),
                (500, fx_schema::PropertyValue::Float(15.0)),
            ],
        ),
        super::super::tests::keyed_entry(
            LayerId::new(90007),
            PropType::ScaleX,
            [
                (0, fx_schema::PropertyValue::Float(100.0)),
                (1000, fx_schema::PropertyValue::Float(200.0)),
            ],
        ),
    ] {
        value["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(json!(entry));
    }
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let LayerData::Group(root) = document.composition().layers()[0].data() else {
        panic!("root");
    };
    let LayerData::Group(group) = root.layers[0].data() else {
        panic!("door");
    };
    let dynamics = super::super::AnimationIndex::new(document.composition().dynamics().entries());
    let bounds = super::super::source_rect_mask_bounds(
        group,
        &dynamics,
        fx_schema::Dimensions {
            width: 319,
            height: 179,
        },
    )
    .unwrap();
    assert_eq!(bounds.min, [0.0, 0.0]);
    assert_eq!(bounds.max, [200.0, 140.0]);
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(90002)),
        "{:?}",
        output.diagnostics
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Mask 2 omitted"))
    );
    let native = read_project(&output.bytes).unwrap();
    let roundtrip = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    assert!(
        roundtrip["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["target"]["propertyType"] == "shapePath")
    );
    // The suffix is required native content, not a disposable bounding hint.
    value["composition"]["layers"][0]["layers"][0]["layers"][3]["parent"] = json!(90001);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let rejected = super::super::to_aep(&document).unwrap();
    assert!(rejected.omitted_layer_ids.contains(&LayerId::new(90000)));
}

#[test]
fn source_rect_mask_preserves_separate_hold_jump_and_linear_affine_motion() {
    let mut value = masked_affine_counter_value();
    let mut position = json!(super::super::tests::keyed_entry(
        LayerId::new(90007),
        PropType::PositionX,
        [
            (0, fx_schema::PropertyValue::Float(-100000.0)),
            (500, fx_schema::PropertyValue::Float(-100000.0)),
            (510, fx_schema::PropertyValue::Float(15.0))
        ]
    ));
    for key in position["animator"]["keyframes"].as_array_mut().unwrap() {
        key["easing"] = json!({"type":"hold"});
    }
    let scale = json!(super::super::tests::keyed_entry(
        LayerId::new(90007),
        PropType::ScaleX,
        [
            (0, fx_schema::PropertyValue::Float(100.0)),
            (500, fx_schema::PropertyValue::Float(200.0)),
            (1000, fx_schema::PropertyValue::Float(200.0))
        ]
    ));
    value["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .extend([position, scale]);
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(90002)),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let roundtrip = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let entries = roundtrip["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let keys = entries
        .iter()
        .filter(|entry| entry["target"]["propertyType"] == "shapePath")
        .flat_map(|entry| entry["animator"]["keyframes"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert!(
        keys.iter()
            .any(|key| key["layerTime"] == 500 && key["easing"]["type"] == "linear")
    );
    assert!(
        keys.iter()
            .any(|key| key["layerTime"] == 510 && key["easing"]["type"] == "hold")
    );
    // With Scale still changing during the jump, no single native Path easing
    // can preserve both channels. The required subtraction must not be dropped.
    let entries = value["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    entries.last_mut().unwrap()["animator"]["keyframes"][1]["value"] =
        json!(fx_schema::PropertyValue::Float(150.0));
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(output.omitted_layer_ids.contains(&LayerId::new(90000)));
}

#[test]
fn source_rect_mask_certificate_reaches_mixed_spatial_source_classification() {
    let mut value = masked_affine_counter_value();
    value["composition"]["layers"][0]["layers"][0]["layers"][1]["transform"]["rotationY"] =
        json!(10.0);
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(90002)),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let roundtrip = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    assert!(roundtrip.to_string().contains("AFTER EFFECTS"));
    // A real child crossing remains invalid before mask support is used.
    let paint = &mut value["composition"]["layers"][0]["layers"][0]["layers"][1];
    paint["rect"]["position"] = json!([-2000.0, 0.0]);
    paint["rect"]["size"] = json!([4000.0, 140.0]);
    paint["transform"]["rotationY"] = json!(90.0);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(output.omitted_layer_ids.contains(&LayerId::new(90000)));
}

#[test]
fn source_rect_mask_does_not_hide_unproved_projective_text_or_known_camera_crossings() {
    for case in [
        "guide phase",
        "foreign parent",
        "feather",
        "rate",
        "guide motion",
        "own Text 3D",
        "known child crossing",
        "mask surface crossing",
        "blur",
    ] {
        let mut value = masked_projective_door_value();
        let door = &mut value["composition"]["layers"][0]["layers"][0];
        match case {
            "guide phase" => door["layers"][2]["activeRange"]["start"] = json!(100),
            "foreign parent" => door["layers"][2]["parent"] = json!(90001),
            "feather" => door["masks"][0]["feather"] = json!([1.0, 0.0]),
            "rate" => door["playback"]["mapping"]["output"]["duration"] = json!(1000),
            "own Text 3D" => door["layers"][0]["transform"]["rotationY"] = json!(1.0),
            "known child crossing" => {
                door["layers"][1]["transform"]["position"] = json!([18000.0, 90.0]);
                door["layers"][1]["transform"]["rotationY"] = json!(15.0);
            }
            "mask surface crossing" => door["layers"][2]["rect"]["size"] = json!([4000.0, 140.0]),
            "blur" => door["motionBlur"] = json!(true),
            _ => value["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(json!(super::super::tests::keyed_entry(
                    LayerId::new(90003),
                    PropType::PositionX,
                    [
                        (0, fx_schema::PropertyValue::Float(0.0)),
                        (1000, fx_schema::PropertyValue::Float(10.0))
                    ]
                ))),
        }
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = super::super::to_aep(&document).unwrap();
        assert!(
            output.omitted_layer_ids.contains(&LayerId::new(90000)),
            "{case}: {:?}",
            output.diagnostics
        );
        if case.ends_with("crossing") {
            assert!(
                output
                    .diagnostics
                    .iter()
                    .any(|d| d.message.contains("near plane"))
            );
        }
    }
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
        &crate::export_document::AnimationIndex::new(document.composition().dynamics().entries()),
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
    assert!(
        root_viewport::canvas(
            group,
            &crate::export_document::AnimationIndex::new(&[animation]),
            layers,
            document.dimensions()
        )
        .is_none()
    );
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
        (
            json!({"type":"customShader","name":"omitted look","wgsl":"","params":[]}),
            true,
        ),
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
