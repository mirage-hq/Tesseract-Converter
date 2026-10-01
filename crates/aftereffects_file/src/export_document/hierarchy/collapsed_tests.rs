use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};

const NATIVE: &[u8] = include_bytes!("../../../tests/fixtures/collapsed_vectors/source.aep");

#[test]
fn collapsed_native_control_flags_and_mask_source_are_pinned() {
    assert_eq!(
        format!("{:x}", Sha256::digest(NATIVE)),
        "d520c45353d7a16dad19793fa0c52aa5fd3b3a34bd00f68e5f6e746eeabfd41f"
    );
    let project = crate::structure::read_project(NATIVE).unwrap();
    for (id, source_id, collapsed) in [
        (21, 1, true),
        (34, 1, false),
        (94, 80, true),
        (121, 107, true),
    ] {
        let crate::structure::ItemKind::Composition(comp) = &project.item(id).unwrap().kind else {
            panic!("native target")
        };
        let owner = comp
            .layers
            .iter()
            .find(|layer| layer.record.source_id() == source_id)
            .expect("native source relationship");
        assert_eq!(
            owner.record.flags().collapse_transformation,
            collapsed,
            "target {id}"
        );
    }
}

#[test]
fn collapsed_export_preserves_mask_owner_and_wide_child() {
    // The native file is the independent capability oracle. This explicit FX
    // edit is supplementary fresh-export evidence, not an Adobe-render claim.
    let native = crate::structure::read_project(NATIVE).unwrap();
    let mut envelope = crate::structure_document::to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut group = serde_json::to_value(masked_group()).unwrap();
    group["type"] = json!("Group");
    envelope["duration"] = json!(2);
    envelope["composition"]["layers"] = json!([group]);
    envelope["composition"]["dynamics"] = json!({"entries": []});
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(envelope).unwrap();
    let exported = super::super::to_aep(&document).unwrap();
    assert!(
        !exported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        exported.diagnostics
    );
    let project = crate::structure::read_project(&exported.bytes).unwrap();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("export root")
    };
    let owner = comp
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Wide editable vector boundary")
        .unwrap();
    assert!(owner.record.flags().collapse_transformation);
    let crate::structure::ItemKind::Composition(source) =
        &project.item(owner.record.source_id()).unwrap().kind
    else {
        panic!("editable source")
    };
    assert!(
        source
            .layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Uncropped rail")
    );
    let imported = crate::structure_document::to_structural_fx_document(
        &project,
        Some(owner.record.source_id()),
    )
    .unwrap()
    .document
    .to_json_value()
    .unwrap();
    fn has_original_size(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => {
                map.get("rect")
                    .is_some_and(|rect| rect["size"] == json!([65520.0, 100.0]))
                    || map.values().any(has_original_size)
            }
            serde_json::Value::Array(values) => values.iter().any(has_original_size),
            _ => false,
        }
    }
    assert!(
        has_original_size(&imported),
        "native editable rail geometry must not be cropped"
    );
}

#[test]
fn collapsed_mask_keeps_three_d_ancestors_uncollapsed_and_finite() {
    let mut mask = serde_json::to_value(masked_group()).unwrap();
    mask["type"] = json!("Group");
    let mut projected = serde_json::to_value(group()).unwrap();
    projected["type"] = json!("Group");
    projected["id"] = json!(19);
    projected["transform"]["position"] = json!([0, 0, 0]);
    projected["layers"] = json!([mask]);
    let mut ancestor = group();
    ancestor.id = LayerId::new(20);
    ancestor.layers = vec![serde_json::from_value(projected).unwrap()];
    let plan = classify_wide(&ancestor).unwrap();
    assert_eq!(plan.collapsed_source(), None);
    assert!(plan.camera.is_some());
    assert!(plan.width < 10000 && plan.height < 10000);
}

fn group() -> GroupLayer {
    serde_json::from_value(json!({
        "id": 10, "name": "Wide editable vector boundary",
        "playback": {
            "type": "windowed",
            "inputRange": {"start": 0, "duration": 2000},
            "mapping": {"type": "linear",
                "input": {"start": 0, "duration": 2000},
                "output": {"start": 0, "duration": 2000}},
            "inputOffsetMs": 0
        },
        "transform": {"position": [0, 0], "anchorPoint": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "layers": [{
            "type": "Rect", "id": 11, "name": "Uncropped rail",
            "activeRange": {"start": 0, "duration": 2000},
            "transform": {"position": [0, 0], "anchorPoint": [0, 0], "scale": [115, 115], "rotation": 0, "opacity": 100},
            "rect": {"position": [-32760, -50], "size": [65520, 100], "fillColor": [0, 1, 1, 1]}
        }]
    })).unwrap()
}

fn canvas() -> fx_schema::Dimensions {
    fx_schema::Dimensions {
        width: 1920,
        height: 1080,
    }
}

fn classify_wide(group: &GroupLayer) -> Result<PrecompositionPlan, &'static str> {
    let plan = classify_precomposition(
        group,
        Time::from_millis(2000),
        Duration24::from_frames(48).unwrap(),
        &[],
        &BTreeMap::new(),
        canvas(),
    )?;
    let HierarchyPlan::Precomposition(plan) = plan else {
        panic!("expected native precomposition")
    };
    Ok(plan)
}

#[test]
fn collapsed_oversize_retains_source_geometry_and_sets_native_switch() {
    let group = group();
    let plan = classify_wide(&group).unwrap();
    assert_eq!(
        plan.collapsed_source(),
        Some(CollapsedSource::OversizedVector)
    );
    assert_eq!(plan.mask_space(), ([1920, 1080], [0.0, 0.0]));
    let LayerData::Rect(rect) = group.layers[0].data() else {
        panic!("expected rail")
    };
    assert_eq!(rect.rect.size, [65520.0, 100.0]);
}

#[test]
fn collapsed_unsafe_occurrences_are_not_enabled() {
    let mut value = group();
    value.motion_blur = true;
    assert!(classify_wide(&value).is_err());
    value.motion_blur = false;
    value.transform.rotation_x = 20.0;
    assert!(!collapsed::eligible(&value, &[]));
    value.transform.rotation_x = 0.0;
    let mut json = serde_json::to_value(value).unwrap();
    json["layers"][0]["transform"]["rotationX"] = json!(20);
    let value = serde_json::from_value(json).unwrap();
    assert!(classify_wide(&value).is_err());
}

fn masked_group() -> GroupLayer {
    let mut group = group();
    group.masks.push(
        serde_json::from_value(json!({
            "id": 12, "mode": "add", "opacity": 100,
            "path": {"commands": [
                {"type": "moveTo", "x": -1000, "y": -575},
                {"type": "lineTo", "x": 1000, "y": -575},
                {"type": "lineTo", "x": 1000, "y": 575},
                {"type": "lineTo", "x": -1000, "y": 575},
                {"type": "close"}
            ]}
        }))
        .unwrap(),
    );
    group
}

#[test]
fn collapsed_mask_output_bounds_do_not_crop_the_input() {
    let group = masked_group();
    let input = static_child_union(&group, &BTreeMap::new(), canvas())
        .unwrap()
        .unwrap();
    assert!(input.max[0] - input.min[0] > 65535.0);
    let layer: Layer = serde_json::from_value({
        let mut value = serde_json::to_value(&group).unwrap();
        value["type"] = json!("Group");
        value
    })
    .unwrap();
    let output = animated_bounds::layer_bounds(&layer, &[], &BTreeMap::new(), canvas())
        .unwrap()
        .unwrap();
    assert_eq!(output.min, [-1000.0, -575.0]);
    assert_eq!(output.max, [1000.0, 575.0]);
    let mut source = group;
    source.masks.clear(); // Occurrence masks are serialized outside the source.
    assert_eq!(
        classify_wide(&source).unwrap().collapsed_source(),
        Some(CollapsedSource::OversizedVector)
    );
}

#[test]
fn collapsed_mask_soft_inverted_and_owner_effects_keep_full_bounds() {
    let mut group = masked_group();
    assert!(collapsed::mask_output(&group, &[]).is_some());
    group.masks[0].inverted = true;
    assert!(collapsed::mask_output(&group, &[]).is_none());
    group.masks[0].inverted = false;
    group.masks[0].feather = [1.0, 0.0];
    assert!(collapsed::mask_output(&group, &[]).is_none());
    group.masks[0].feather = [0.0; 2];
    group
        .effects
        .push(serde_json::from_value(json!({"type":"exposure", "exposure": 1.0})).unwrap());
    assert!(collapsed::mask_output(&group, &[]).is_none());
}

/// The oversized vector boundary keeps its owner-clock guard, which Text
/// collapse does not share. Identity-shaped TimeRemap keys fail the owner
/// check, and an offset occurrence fails the export caller's identity-clock
/// check, while canonical linear identity playback collapses. The owner is
/// moved off the identity transform so the root output viewport cannot stand
/// in for collapse.
#[test]
fn collapsed_vector_owner_keeps_requiring_canonical_identity() {
    const NOT_CERTIFIED: &str = "not a certified collapsed 2D vector source";
    const CLOCKED: &str = "Collapsed vector source requires a 2D identity-clock occurrence";
    let keys = |pairs: [(u64, u64); 2]| {
        json!({
            "type": "windowed",
            "inputRange": {"start": pairs[0].0, "duration": pairs[1].0 - pairs[0].0},
            "mapping": {"type": "timeRemap", "property": {
            "before": "inactive",
            "after": "inactive",
            "keyframes": pairs
                .iter()
                .enumerate()
                .map(|(index, (time, value))| json!({
                    "id": format!("wide-vector-clock-{index}"),
                    "time": time,
                    "value": value,
                    "easing": {"type": "linear"}
                }))
                .collect::<Vec<_>>()
            }},
            "inputOffsetMs": 0
        })
    };
    let native = crate::structure::read_project(NATIVE).unwrap();
    let envelope = crate::structure_document::to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut wider_identity = serde_json::to_value(&group().playback).unwrap();
    wider_identity["mapping"]["input"]["duration"] = json!(4_000);
    wider_identity["mapping"]["output"]["duration"] = json!(4_000);
    for (label, playback, rejection) in [
        ("canonical linear identity", None, None),
        ("wider linear identity mapping", Some(wider_identity), None),
        (
            "identity keys",
            Some(keys([(0, 0), (2_000, 2_000)])),
            Some(NOT_CERTIFIED),
        ),
        (
            "offset clock",
            Some(keys([(500, 0), (2_000, 1_500)])),
            Some(CLOCKED),
        ),
    ] {
        let mut owner = serde_json::to_value(group()).unwrap();
        owner["type"] = json!("Group");
        owner["transform"]["position"] = json!([24, 12]);
        if let Some(playback) = playback {
            owner["playback"] = playback;
        }
        let mut envelope = envelope.clone();
        envelope["duration"] = json!(2);
        envelope["composition"]["layers"] = json!([owner]);
        envelope["composition"]["dynamics"] = json!({"entries": []});
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(envelope).unwrap();
        let exported = super::super::to_aep(&document).unwrap();
        let project = crate::structure::read_project(&exported.bytes).unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("export root")
        };
        let collapsed = comp.layers.iter().any(|layer| {
            layer.name.as_ref() == "Wide editable vector boundary"
                && layer.record.flags().collapse_transformation
        });
        match rejection {
            None => assert!(collapsed, "{label}: {:?}", exported.diagnostics),
            Some(reason) => assert!(
                !collapsed
                    && exported.diagnostics.iter().any(|diagnostic| {
                        diagnostic.layer_id == Some(LayerId::new(10))
                            && diagnostic.message.contains(reason)
                            && diagnostic.message.contains("subtree omitted")
                    }),
                "{label}: {:?}",
                exported.diagnostics
            ),
        }
    }
}
