//! Scene, hierarchy, mask, layout, clock, and 3D export contracts.
//!
//! Every document is an explicitly edited current FX input and every AEP is
//! freshly authored. Re-reading it with our parser is supplementary structure
//! evidence, not independent Adobe acceptance or render proof.

use super::*;
use fx_schema::layer::PathMask;
use fx_schema::{Layer, TextPathOptions};
use std::collections::BTreeMap;

use crate::writer::{NativeMaskMode, NativeMaskSpec};

fn document(
    mut value: Value,
    layers: Vec<Value>,
    entries: Vec<fx_schema::animator::AnimationGraphEntry>,
) -> EditableFxCompositionDocument {
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": entries});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn identity_group(value: &Value, id: u64, mut children: Vec<Value>) -> Value {
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(id);
    group["name"] = json!(format!("Scene group {id}"));
    group["parent"] = Value::Null;
    group["layers"] = Value::Array(
        children
            .iter_mut()
            .map(|child| {
                child["parent"] = json!(id);
                child.clone()
            })
            .collect(),
    );
    group["isHidden"] = json!(false);
    group["blendMode"] = json!("normal");
    group["trackMatte"] = Value::Null;
    group["masks"] = json!([]);
    group["effects"] = json!([]);
    group["motionBlur"] = json!(false);
    group["paddingTop"] = json!(0.0);
    group["paddingRight"] = json!(0.0);
    group["paddingBottom"] = json!(0.0);
    group["paddingLeft"] = json!(0.0);
    group["fills"] = json!([]);
    group["cornerRadiusTopLeft"] = json!(0.0);
    group["cornerRadiusTopRight"] = json!(0.0);
    group["cornerRadiusBottomRight"] = json!(0.0);
    group["cornerRadiusBottomLeft"] = json!(0.0);
    group
}

fn named_native<'a>(project: &'a StructuralProject, name: &str) -> &'a crate::structure::Layer {
    layers(project)
        .iter()
        .find(|layer| layer.name.as_ref() == name)
        .unwrap()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_common_layer_options_author_enabled_blend_motion_parent_and_matte_records() {
    let value = imported();
    let mut provider = rect(&value, 700);
    provider["name"] = json!("Matte provider");
    let mut target = rect(&value, 701);
    target["name"] = json!("Matted target");
    target["isHidden"] = json!(true);
    target["blendMode"] = json!("multiply");
    target["motionBlur"] = json!(true);
    target["trackMatte"] = json!({"mode":"alphaInverted","layer":700});
    let edited = document(value, vec![target, provider], Vec::new());
    let output = to_aep(&edited).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let target = named_native(&native, "Matted target");
    let provider = named_native(&native, "Matte provider");
    assert!(!target.record.flags().enabled);
    assert!(target.record.flags().motion_blur);
    assert_eq!(target.record.blend_mode(), 5);
    assert_eq!(target.record.track_matte_type(), 2);
    assert_eq!(target.record.matte_layer_id(), Some(provider.record.id()));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_finite_group_prefers_null_parent_and_isolates_an_unsupported_sibling() {
    let value = imported();
    let mut first = rect(&value, 710);
    first["name"] = json!("Child A");
    let mut second = rect(&value, 711);
    second["name"] = json!("Child B");
    let group = identity_group(&value, 712, vec![first, second]);
    let mut bad = rect(&value, 713);
    bad["name"] = json!("Unsupported sibling");
    bad["transform"]["skew"] = json!(12.0);
    let edited = document(value, vec![group, bad], Vec::new());
    let output = to_aep(&edited).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let parent = named_native(&native, "Scene group 712");
    assert!(parent.record.flags().null_layer);
    for name in ["Child A", "Child B"] {
        assert_eq!(
            named_native(&native, name).record.parent_id(),
            parent.record.id()
        );
    }
    assert!(
        layers(&native)
            .iter()
            .all(|layer| layer.name.as_ref() != "Unsupported sibling")
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(713))
                && diagnostic.message.contains("siblings retained"))
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_static_mask_parade_and_text_guide_use_stable_one_based_indices() {
    let value = imported();
    let owner: Layer = serde_json::from_value(rect(&value, 720)).unwrap();
    let mut guide_value = rect(&value, 721);
    guide_value["rect"]["position"] = json!([10.0, 20.0]);
    guide_value["rect"]["size"] = json!([30.0, 40.0]);
    let guide: Layer = serde_json::from_value(guide_value).unwrap();
    let path = fx_schema::ShapePath {
        commands: vec![
            fx_schema::ShapePathCommand::MoveTo {
                x: 1.0,
                y: 2.0,
                mirror: None,
                corner_radius: None,
            },
            fx_schema::ShapePathCommand::LineTo {
                x: 11.0,
                y: 2.0,
                mirror: None,
                corner_radius: None,
            },
            fx_schema::ShapePathCommand::LineTo {
                x: 11.0,
                y: 12.0,
                mirror: None,
                corner_radius: None,
            },
            fx_schema::ShapePathCommand::Close,
        ],
    };
    let mask: PathMask = serde_json::from_value(json!({
        "id": 9001, "mode":"subtract", "inverted":true, "path":path,
        "feather":[4.0,6.0], "expansion":3.0, "opacity":0.75
    }))
    .unwrap();
    let text_path: TextPathOptions = serde_json::from_value(json!({
        "id":9002, "pathLayer":721, "firstMargin":5.0, "lastMargin":7.0
    }))
    .unwrap();
    let transform = match owner.data() {
        LayerData::Rect(rect) => &rect.transform,
        _ => panic!("rect"),
    };
    let lowered = masks::lower(
        &[mask],
        Some(&text_path),
        masks::MaskOwner {
            id: owner.id(),
            parent: None,
            transform,
            source_size: [1920, 1080],
            clock: Some(owner.active_range()),
        },
        &[owner.clone(), guide],
        &[],
    );
    assert_eq!(lowered.masks.len(), 2);
    assert_eq!(lowered.text_path_index, Some(2));
    let authored = &lowered.masks[0];
    assert_eq!(authored.name, "Mask 1");
    assert_eq!(authored.path, path);
    assert_eq!(authored.source_size, [1920, 1080]);
    assert_eq!(authored.mode, NativeMaskMode::Subtract);
    assert!(authored.inverted);
    assert_eq!(authored.feather, [4.0, 6.0]);
    assert_eq!(authored.opacity, 0.75);
    assert_eq!(authored.expansion, 3.0);
    assert_eq!(authored.feather_track, None);
    assert_eq!(authored.opacity_track, None);
    assert_eq!(authored.expansion_track, None);

    let guide = &lowered.masks[1];
    assert_eq!(guide.name, "Text Path Guide");
    assert_eq!(guide.source_size, [1920, 1080]);
    assert_eq!(guide.mode, NativeMaskMode::None);
    assert!(!guide.inverted);
    assert_eq!(guide.feather, [0.0, 0.0]);
    assert_eq!(guide.opacity, 1.0);
    assert_eq!(guide.expansion, 0.0);
    assert_eq!(guide.feather_track, None);
    assert_eq!(guide.opacity_track, None);
    assert_eq!(guide.expansion_track, None);
    assert_eq!(
        guide
            .path
            .commands
            .iter()
            .filter_map(fx_schema::ShapePathCommand::endpoint)
            .collect::<Vec<_>>(),
        [(10.0, 20.0), (40.0, 20.0), (40.0, 60.0), (10.0, 60.0)]
    );
    assert_eq!(lowered.consumed_guides, BTreeSet::from([LayerId::new(721)]));
}

#[test]
fn review_disabled_group_mask_track_is_clock_independent_effective_constant() {
    let path = fx_schema::ShapePath {
        commands: vec![
            fx_schema::ShapePathCommand::MoveTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            fx_schema::ShapePathCommand::Close,
        ],
    };
    let mask: PathMask = serde_json::from_value(json!({
        "id": 9_010,
        "mode": "add",
        "inverted": false,
        "path": path,
        "feather": [0.0, 0.0],
        "expansion": 0.0,
        "opacity": 1.0
    }))
    .unwrap();
    let mut enabled = keyed_entry(
        LayerId::new(9_011),
        PropType::Opacity,
        [
            (0, PropertyValue::Float(0.25)),
            (1_000, PropertyValue::Float(0.75)),
        ],
    );
    enabled.target = fx_schema::PropertyTarget::fx_item(mask.id, "opacity");
    assert!(group_has_dynamic_mask_properties(
        std::slice::from_ref(&mask),
        std::slice::from_ref(&enabled)
    ));

    let mut disabled = enabled;
    let mut animator = disabled.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = &mut animator
    else {
        panic!("mask opacity fixture is keyed")
    };
    *enabled = false;
    *disabled_value = Some(PropertyValue::Float(0.5));
    disabled.animator = fx_schema::animator::PropertyAnimator::from_data(&animator).unwrap();
    assert!(!group_has_dynamic_mask_properties(
        std::slice::from_ref(&mask),
        &[disabled]
    ));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_masked_precomposition_uses_proven_origin_for_canvas_and_mask_translation() {
    let value = imported();
    let mut child = rect(&value, 730);
    child["rect"]["position"] = json!([100.25, 200.75]);
    child["rect"]["size"] = json!([50.0, 20.0]);
    let group_value = identity_group(&value, 731, vec![child]);
    let group_layer: Layer = serde_json::from_value(group_value).unwrap();
    let LayerData::Group(group) = group_layer.data() else {
        panic!("group")
    };
    let duration = crate::timing::Duration24::from_frames(48).unwrap();
    let plan = hierarchy::classify_precomposition(
        group,
        Time::from_millis(2000),
        duration,
        &[],
        &BTreeMap::new(),
        fx_schema::Dimensions::new(1920, 1080),
    )
    .unwrap();
    let hierarchy::HierarchyPlan::Precomposition(plan) = plan else {
        panic!("precomposition")
    };
    let (size, origin) = plan.mask_space();
    assert_eq!(size, [50, 21]);
    assert_eq!(origin, [100.0, 200.0]);
    let mut mask = NativeMaskSpec {
        name: "translated".into(),
        path: fx_schema::ShapePath {
            commands: vec![
                fx_schema::ShapePathCommand::MoveTo {
                    x: 100.0,
                    y: 200.0,
                    mirror: None,
                    corner_radius: None,
                },
                fx_schema::ShapePathCommand::LineTo {
                    x: 110.0,
                    y: 200.0,
                    mirror: None,
                    corner_radius: None,
                },
            ],
        },
        path_track: None,
        source_size: [1920, 1080],
        mode: NativeMaskMode::Add,
        inverted: false,
        feather: [0.0; 2],
        opacity: 1.0,
        expansion: 0.0,
        feather_track: None,
        opacity_track: None,
        expansion_track: None,
    };
    masks::translate(std::slice::from_mut(&mut mask), [-origin[0], -origin[1]]);
    assert_eq!(mask.path.commands[0].endpoint(), Some((0.0, 0.0)));
}

#[test]
fn legacy_inline_closed_loop_exports_closed_native_group_mask() {
    use crate::rifx::Chunk;

    fn native_shape_header(chunks: &[Chunk]) -> Option<&[u8]> {
        for chunk in chunks {
            if chunk.id() == *b"shph" {
                return chunk.data_payload();
            }
            if let Some(header) = chunk.children().and_then(native_shape_header) {
                return Some(header);
            }
        }
        None
    }

    let value = imported();
    let mut grid = rect(&value, 730);
    grid["name"] = json!("Grid child");
    grid["rect"]["position"] = json!([100.25, 200.75]);
    grid["rect"]["size"] = json!([50.0, 20.0]);
    let mut group = identity_group(&value, 731, vec![grid]);
    group["masks"] = json!([{
        "id": 9001, "mode": "add", "inverted": false,
        "path": {"commands": [
            {"type":"moveTo", "x":100.0, "y":200.0},
            {"type":"lineTo", "x":150.0, "y":200.0},
            {"type":"lineTo", "x":150.0, "y":220.0},
            {"type":"lineTo", "x":100.0, "y":220.0},
            {"type":"lineTo", "x":100.0, "y":200.0}
        ]},
        "feather": [0.0, 0.0], "expansion": 0.0, "opacity": 1.0
    }]);
    let output = to_aep(&document(value, vec![group], Vec::new())).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let owner = named_native(&native, "Scene group 731");
    let header = native_shape_header(&owner.content).expect("native group mask path");
    assert_eq!(
        &header[..4],
        &[0xb3, 0xde, 0x02, 1],
        "native mask must be closed"
    );
    let bounds: Vec<_> = header[4..20]
        .chunks_exact(4)
        .map(|bytes| f32::from_be_bytes(bytes.try_into().unwrap()))
        .collect();
    // The fractional child bounds produce a 51 × 21 source. Native masks
    // store translated coordinates normalized to that source, not pixels.
    assert_eq!(bounds, [0.0, 0.0, 50.0 / 51.0, 20.0 / 21.0]);
    let imported = to_structural_fx_document(&native, Some(1)).unwrap();
    assert!(
        imported
            .document
            .to_json_value()
            .unwrap()
            .to_string()
            .contains("Grid child"),
        "grid child survives"
    );
    assert!(
        output.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("duplicate-endpoint inline mask")),
        "{:?}",
        output.diagnostics
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_analytic_bounds_include_temporal_and_spatial_control_hulls() {
    use fx_schema::animator::{
        AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    };
    let value = imported();
    let layer: Layer = serde_json::from_value(rect(&value, 740)).unwrap();
    let track = PropertyKeyframeTrack::new(vec![
        PropertyKeyframe::new(
            KeyframeId::new("bounds-a"),
            fx_schema::TimeOffset::from_millis(0),
            PropertyValue::Float(0.0),
            PropertyKeyframeEasing::Linear,
        )
        .with_spatial_tangents(None, Some(300.0)),
        PropertyKeyframe::new(
            KeyframeId::new("bounds-b"),
            fx_schema::TimeOffset::from_millis(1000),
            PropertyValue::Float(10.0),
            PropertyKeyframeEasing::CubicBezier {
                x1: 0.2,
                y1: 2.0,
                x2: 0.8,
                y2: 2.0,
            },
        )
        .with_spatial_tangents(Some(300.0), None),
    ])
    .unwrap();
    let entry = AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(LayerId::new(740), PropType::PositionX),
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let bounds = hierarchy::all_time_layer_bounds(
        &layer,
        &[entry],
        &BTreeMap::new(),
        fx_schema::Dimensions::new(1920, 1080),
    )
    .unwrap()
    .unwrap();
    assert!(
        bounds.max[0] > 300.0,
        "control hull must exceed endpoint-only bounds"
    );
    assert!(
        bounds
            .min
            .iter()
            .chain(&bounds.max)
            .all(|value| value.is_finite())
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_group_source_clock_eligibility_rejects_nonidentity_without_overwriting_children() {
    let value = imported();
    let child = rect(&value, 750);
    let mut group_value = identity_group(&value, 751, vec![child]);
    group_value["playback"] = json!({"rate":2.0});
    let layer: Layer = serde_json::from_value(group_value).unwrap();
    let LayerData::Group(group) = layer.data() else {
        panic!("group")
    };
    let result = hierarchy::classify(
        group,
        Time::from_millis(2000),
        crate::timing::Duration24::from_frames(48).unwrap(),
        &[],
        &BTreeMap::new(),
        fx_schema::Dimensions::new(1920, 1080),
    );
    assert!(matches!(result, Err(message) if message.contains("source clock")));

    let mut value = imported();
    value["composition"]["layers"] =
        json!([serde_json::to_value(layer).unwrap(), rect(&value, 752)]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 752")
    );
}

#[test]
fn review_disabled_layout_track_uses_runtime_visible_value() {
    let value = imported();
    let child = rect(&value, 7_600);
    let mut group_value = identity_group(&value, 7_601, vec![child]);
    group_value["paddingLeft"] = json!(3.0);
    group_value["fills"] =
        json!([{"paint":{"type":"solid","color":[0.2,0.3,0.4,1.0]},"opacity":1.0}]);
    let actual_layer: Layer = serde_json::from_value(group_value.clone()).unwrap();
    let expected_layer: Layer = serde_json::from_value(group_value).unwrap();
    let (LayerData::Group(actual_group), LayerData::Group(expected_group)) =
        (actual_layer.data(), expected_layer.data())
    else {
        panic!("layout fixtures are Groups")
    };
    let mut disabled = keyed_entry(
        actual_group.id,
        PropType::PaddingLeft,
        [
            (0, PropertyValue::Float(5.0)),
            (1_000, PropertyValue::Float(9.0)),
        ],
    );
    let mut animator = disabled.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = &mut animator
    else {
        panic!("padding fixture is keyed")
    };
    *enabled = false;
    *disabled_value = Some(PropertyValue::Float(41.0));
    disabled.animator = fx_schema::animator::PropertyAnimator::from_data(&animator).unwrap();

    let actual = layout::normalize_group(
        actual_group,
        LayerId::new(7_602),
        &[disabled],
        &BTreeMap::new(),
        fx_schema::Dimensions::new(1920, 1080),
    )
    .unwrap();
    let expected = layout::normalize_group(
        expected_group,
        LayerId::new(7_602),
        &[constant_entry(
            expected_group.id,
            PropType::PaddingLeft,
            PropertyValue::Float(41.0),
        )],
        &BTreeMap::new(),
        fx_schema::Dimensions::new(1920, 1080),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(actual.group.layers.last().unwrap()).unwrap(),
        serde_json::to_value(expected.group.layers.last().unwrap()).unwrap(),
        "disabled layout keys must use disabledValue rather than the stale typed base"
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_static_layout_materializes_background_radii_and_aiedit_keeps_explicit_stack() {
    let value = imported();
    let child = rect(&value, 760);
    let mut group_value = identity_group(&value, 761, vec![child]);
    group_value["paddingTop"] = json!(4.0);
    group_value["paddingRight"] = json!(5.0);
    group_value["paddingBottom"] = json!(6.0);
    group_value["paddingLeft"] = json!(7.0);
    group_value["cornerRadiusTopLeft"] = json!(12.0);
    group_value["cornerRadiusTopRight"] = json!(8.0);
    group_value["cornerRadiusBottomRight"] = json!(4.0);
    group_value["cornerRadiusBottomLeft"] = json!(2.0);
    group_value["fills"] =
        json!([{"paint":{"type":"solid","color":[0.2,0.3,0.4,0.8]},"opacity":0.5}]);
    let layer: Layer = serde_json::from_value(group_value).unwrap();
    let LayerData::Group(group) = layer.data() else {
        panic!("group")
    };
    let normalized = layout::normalize_group(
        group,
        LayerId::new(762),
        &[],
        &BTreeMap::new(),
        fx_schema::Dimensions::new(1920, 1080),
    )
    .unwrap();
    assert_eq!(normalized.group.layers.len(), 2);
    assert!(matches!(
        normalized.group.layers.last().unwrap().data(),
        LayerData::Shape(_)
    ));
    assert_eq!(normalized.group.padding_top.value(), 0.0);
    assert!(
        normalized
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("editable background child"))
    );

    let ai: Layer = serde_json::from_value(json!({
        "type":"AiEdit","id":770,"name":"AI shot","activeRange":{"start":0,"duration":2000},
        "styleId":"style","sourceLayerId":771,"layers":[rect(&value,771)],
        "background":{"type":"color","color":[1.0,0.0,0.0,1.0]}
    }))
    .unwrap();
    let LayerData::AiEdit(ai) = ai.data() else {
        panic!("AiEdit")
    };
    let normalized = layout::normalize_ai_edit(ai).unwrap();
    assert_eq!(normalized.group.layers.len(), 1);
    assert!(
        normalized
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("host-owned"))
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_stored_3d_orientation_rebases_owner_tracks_and_rejects_cubic_quaternion_ease() {
    let value = imported();
    let mut layer = rect(&value, 780);
    layer["transform"]["position"] = json!([100.0, 200.0, 30.0]);
    layer["transform"]["rotationX"] = json!(10.0);
    layer["transform"]["rotationY"] = json!(-20.0);
    layer["transform"]["orientation"] = json!([1.0, 2.0, 3.0]);
    let layer: Layer = serde_json::from_value(layer).unwrap();
    let LayerData::Rect(rect) = layer.data() else {
        panic!("rect")
    };
    let lowered = transform3d::lower(
        &[],
        &rect.transform,
        rect.id,
        transform3d::Native2dGeometry::centered([3.0, 4.0]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(lowered.transform.position, [100.0, 200.0, 30.0]);
    assert_eq!(lowered.transform.orientation, [1.0, 2.0, 3.0]);
    assert!(
        lowered
            .projection_diagnostic
            .contains("equal rendering is not claimed")
    );

    let mut entries = Vec::new();
    for property in [
        PropType::OrientationX,
        PropType::OrientationY,
        PropType::OrientationZ,
    ] {
        let mut entry = keyed_entry(
            LayerId::new(780),
            property,
            [
                (0, PropertyValue::Float(0.0)),
                (500, PropertyValue::Float(30.0)),
            ],
        );
        let fx_schema::animator::AnimatorData::Keyframes {
            track,
            enabled,
            disabled_value,
        } = entry.animator.data().clone()
        else {
            unreachable!()
        };
        let mut keys = track.keyframes().to_vec();
        keys[1] = fx_schema::animator::PropertyKeyframe::new(
            keys[1].id().clone(),
            keys[1].layer_time(),
            keys[1].value().clone(),
            PropertyKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: 0.25,
                x2: 0.75,
                y2: 0.75,
            },
        );
        entry.animator = fx_schema::animator::PropertyAnimator::from_data(
            &fx_schema::animator::AnimatorData::Keyframes {
                track: fx_schema::animator::PropertyKeyframeTrack::new(keys).unwrap(),
                enabled,
                disabled_value,
            },
        )
        .unwrap();
        entries.push(entry);
    }
    assert!(
        matches!(transform3d::lower(&entries, &rect.transform, rect.id, transform3d::Native2dGeometry::IDENTITY), Err(message) if message.contains("cubic quaternion"))
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_composition_motion_options_are_exact_and_numeric_speed_overflow_omits_only_leaf() {
    let settings: fx_schema::MotionBlurSettings = serde_json::from_value(json!({
        "enabled":true,"shutterAngle":271,"shutterPhase":-137,
        "samplesPerFrame":23,"adaptiveSampleLimit":191
    }))
    .unwrap();
    assert_eq!(
        composition_options::from_motion_blur(settings).unwrap(),
        CompositionOptions::motion_blur(true, 271, -137, 23, 191)
    );

    let mut value = imported();
    value["composition"]["motionBlur"] = json!({"enabled":true,"shutterAngle":271,"shutterPhase":-137,"samplesPerFrame":23,"adaptiveSampleLimit":191});
    let mut bad = rect(&value, 790);
    bad["name"] = json!("Overflow easing");
    let good = rect(&value, 791);
    let mut entry = keyed_entry(
        LayerId::new(790),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(-f64::MAX)),
            (1, PropertyValue::Float(f64::MAX)),
        ],
    );
    let fx_schema::animator::AnimatorData::Keyframes {
        track,
        enabled,
        disabled_value,
    } = entry.animator.data().clone()
    else {
        unreachable!()
    };
    let mut keys = track.keyframes().to_vec();
    keys[1] = fx_schema::animator::PropertyKeyframe::new(
        keys[1].id().clone(),
        keys[1].layer_time(),
        keys[1].value().clone(),
        PropertyKeyframeEasing::CubicBezier {
            x1: 0.5,
            y1: 0.5,
            x2: 0.5,
            y2: 0.5,
        },
    );
    entry.animator = fx_schema::animator::PropertyAnimator::from_data(
        &fx_schema::animator::AnimatorData::Keyframes {
            track: fx_schema::animator::PropertyKeyframeTrack::new(keys).unwrap(),
            enabled,
            disabled_value,
        },
    )
    .unwrap();
    let edited = document(value, vec![bad, good], vec![entry]);
    let output = to_aep(&edited).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 791")
    );
    assert!(
        layers(&native)
            .iter()
            .all(|layer| layer.name.as_ref() != "Overflow easing")
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(790))
                && diagnostic.message.contains("omitted"))
    );
}
