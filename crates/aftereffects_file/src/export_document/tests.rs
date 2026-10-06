use super::*;
use crate::structure::{ItemKind, StructuralProject, read_project};
use crate::structure_document::to_structural_fx_document;
use serde_json::{Value, json};

mod adjustment;
mod animated_direction;
mod audio;
mod bframe_presentation;
mod boolean_geometry;
mod core_native_panel;
mod directional_plane;
mod effectful_vector_groups;
mod effects;
mod effects_edge_coverage;
mod effects_native_coverage;
mod effects_native_panel;
mod empty_controls;
mod hold_endpoints;
mod implemented_feature_additions;
mod inert_trim;
mod io_regressions;
mod layer_styles;
mod matte_visibility;
mod media_native_panel;
mod mixed_one_second_path;
mod mosaic_domain;
mod non_audio_native_panel;
mod one_vertex_loop;
mod paint_modes;
mod paint_opacity;
mod paired_transform;
mod parametric_placeholder;
mod path_keys;
mod point_zero_speed;
mod pr4442_media_cases;
mod pr4442_scene_cases;
mod pr4442_text_cases;
mod pr4442_vector_cases;
mod radial_solid_origin;
mod reservations;
mod review_regressions;
mod root_adjustment_mask_fallback;
mod selector_index_aliases;
mod shader_owner;
mod signed_key_ease;
mod source_stroke_cases;
mod static_dashes;
mod static_polystar_enclosure;
mod stroke_join;
mod stroke_keys;
mod text_controls_native_panel;
mod vector_native_panel;

pub(super) fn imported() -> Value {
    let native = read_project(include_bytes!(
        "../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap()
}

/// Construct an explicit canonical Linear mapping for supplementary FX inputs.
pub(super) fn fixture_linear_playback(input: Value, output: Value) -> Value {
    json!({
        "type": "windowed",
        "inputRange": input,
        "mapping": {"type": "linear", "input": input, "output": output},
        "inputOffsetMs": 0
    })
}

/// Preserve an authored TimeRemap property independently of its visible window.
pub(super) fn fixture_remapped_playback(input_range: Value, property: Value) -> Value {
    json!({
        "type": "windowed",
        "inputRange": input_range,
        "mapping": {"type": "timeRemap", "property": property},
        "inputOffsetMs": 0
    })
}

pub(super) fn rect(value: &Value, id: u64) -> Value {
    let mut rect = value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    rect["id"] = json!(id);
    rect["parent"] = Value::Null;
    rect["name"] = json!(format!("Current solid {id}"));
    rect
}

fn export(value: Value) -> ExportedDocument {
    to_aep(&EditableFxCompositionDocument::from_json_value(value).unwrap()).unwrap()
}

fn layers(project: &StructuralProject) -> &[crate::structure::Layer] {
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("composition")
    };
    &comp.layers
}

fn has_descendant_named(layers: &[fx_schema::Layer], name: &str) -> bool {
    layers.iter().any(|layer| {
        layer.name() == name
            || matches!(layer.data(), LayerData::Group(group) if has_descendant_named(&group.layers, name))
    })
}

fn group_with_nonidentity_playback(layers: &[fx_schema::Layer]) -> Option<&GroupLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Group(group) if !playback_is_identity(&group.playback) => Some(group),
        LayerData::Group(group) => group_with_nonidentity_playback(&group.layers),
        _ => None,
    })
}

fn group_with_opacity(layers: &[fx_schema::Layer], opacity: f64) -> Option<&GroupLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Group(group) if group.transform.opacity.value() == opacity => Some(group),
        LayerData::Group(group) => group_with_opacity(&group.layers, opacity),
        _ => None,
    })
}

fn constant_entry(
    id: LayerId,
    property: PropType,
    value: PropertyValue,
) -> fx_schema::animator::AnimationGraphEntry {
    use fx_schema::animator::{AnimationGraphEntry, PropertyAnimator};

    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(id, property),
        animator: PropertyAnimator::constant(value).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

pub(super) fn keyed_entry<const N: usize>(
    id: LayerId,
    property: PropType,
    values: [(i64, PropertyValue); N],
) -> fx_schema::animator::AnimationGraphEntry {
    use fx_schema::animator::{
        AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    };
    let track = PropertyKeyframeTrack::new(
        values
            .into_iter()
            .enumerate()
            .map(|(index, (millis, value))| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("native-test-{id:?}-{property:?}-{index}")),
                    fx_schema::TimeOffset::from_millis(millis),
                    value,
                    PropertyKeyframeEasing::Linear,
                )
            })
            .collect(),
    )
    .unwrap();
    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(id, property),
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

#[test]
fn review_export_imported_group_solid_falls_back_to_native_hierarchy() {
    let value = imported();
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("subtree omitted"))
    );
}

#[test]
fn authored_rects_keep_order_names_and_nonzero_local_origin() {
    let mut value = imported();
    let mut first = rect(&value, 100);
    first["rect"]["position"] = json!([5.0, -7.0]);
    first["rect"]["size"] = json!([160.0, 80.0]);
    first["transform"]["anchorPoint"] = json!([30.0, 20.0]);
    first["transform"]["position"] = json!([100.0, 200.0]);
    let second = rect(&value, 101);
    value["composition"]["layers"] = json!([first, second]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native)
            .iter()
            .map(|l| l.name.as_ref())
            .collect::<Vec<_>>(),
        ["Current solid 100", "Current solid 101"]
    );
    let transform = crate::properties::read_transform(&layers(&native)[0].content).unwrap();
    let anchor = transform
        .iter()
        .find(|p| p.match_name == "ADBE Anchor Point")
        .unwrap();
    // Native Solid anchor cdat is relative to its 160×80 source, not pixels.
    assert_eq!(
        anchor.numeric.as_ref().unwrap().values,
        [25.0 / 160.0, 27.0 / 80.0, 0.0]
    );
}

#[test]
fn edited_isolated_native_rectangle_exports_as_fresh_shape_not_solid() {
    let native = read_project(include_bytes!(
        "../../tests/fixtures/geometry/geometry_probe.aep"
    ))
    .unwrap();
    let mut value = to_structural_fx_document(&native, Some(14))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn find_rect(value: &mut Value) -> Option<&mut Value> {
        if value.get("rect").is_some() && value["name"] == "shape_rect" {
            return Some(value);
        }
        value
            .get_mut("layers")?
            .as_array_mut()?
            .iter_mut()
            .find_map(find_rect)
    }
    let rectangle = value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find_map(find_rect)
        .expect("pinned native editable Rectangle");
    rectangle["rect"]["size"] = json!([120.0, 60.0]);
    rectangle["rect"]["position"] = json!([3.0, -4.0]);
    rectangle["rect"]["roundness"] = json!(8.0);
    rectangle["rect"]["strokeColor"] = json!([0.2, 0.3, 0.4, 1.0]);
    // This is an explicit single-feature FX export input. The native probe's
    // other layers and transformed multi-child wrapper are outside this slice.
    let mut single = rectangle.clone();
    single["parent"] = Value::Null;
    value["composition"]["layers"] = json!([single]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let edited = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let saved = edited.to_json_vec().unwrap();
    let reopened = EditableFxCompositionDocument::from_json_slice(&saved).unwrap();
    let output = to_aep(&reopened).unwrap();
    let exported = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(comp) = &exported.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "shape_rect")
        .unwrap_or_else(|| panic!("edited native Rect omitted: {:?}", output.diagnostics));
    assert_eq!(layer.record.layer_type(), 4);
    assert_eq!(layer.record.source_id(), 0);
    let converted = to_structural_fx_document(&exported, Some(1)).unwrap();
    fn native_rect(layers: &[fx_schema::Layer]) -> Option<&RectLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::Rect(rect) if rect.name == "shape_rect" => Some(rect),
            LayerData::Group(group) => native_rect(&group.layers),
            _ => None,
        })
    }
    let mapped = native_rect(converted.document.composition().layers()).unwrap();
    assert_eq!(mapped.rect.size, [120.0, 60.0]);
    assert_eq!(mapped.rect.roundness, 8.0);
    assert_eq!(mapped.transform.position, Position::TwoD([63.0, 26.0]));
    assert_eq!(mapped.rect.stroke_color, Some([0.2, 0.3, 0.4, 1.0]));
}

#[test]
fn edited_animated_rect_size_moves_native_center_with_its_extent() {
    let native = read_project(include_bytes!(
        "../../tests/fixtures/geometry/geometry_probe.aep"
    ))
    .unwrap();
    let mut value = to_structural_fx_document(&native, Some(14))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn find_rect(value: &Value) -> Option<&Value> {
        if value.get("rect").is_some() && value["name"] == "shape_rect" {
            return Some(value);
        }
        value.get("layers")?.as_array()?.iter().find_map(find_rect)
    }
    let mut leaf = value["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(find_rect)
        .unwrap()
        .clone();
    let id = LayerId::new(leaf["id"].as_u64().unwrap());
    leaf["parent"] = Value::Null;
    leaf["rect"]["position"] = json!([3.0, -4.0]);
    leaf["rect"]["size"] = json!([80.0, 40.0]);
    value["composition"]["layers"] = json!([leaf]);
    value["composition"]["dynamics"] = json!({"entries": [keyed_entry(
        id,
        PropType::RectSize,
        [
            (0, PropertyValue::Vector2([80.0, 40.0])),
            (500, PropertyValue::Vector2([140.0, 60.0])),
        ],
    )]});
    let saved = EditableFxCompositionDocument::from_json_value(value)
        .unwrap()
        .to_json_vec()
        .unwrap();
    let output = to_aep(&EditableFxCompositionDocument::from_json_slice(&saved).unwrap()).unwrap();
    let project = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&project).len(), 1, "{:?}", output.diagnostics);
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    fn named_rect(layers: &[fx_schema::Layer]) -> Option<LayerId> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::Rect(rect) if rect.name == "shape_rect" => Some(rect.id),
            LayerData::Group(group) => named_rect(&group.layers),
            _ => None,
        })
    }
    let result_id = named_rect(imported.document.composition().layers()).unwrap();
    let entries = imported.document.composition().dynamics().entries();
    let values = |property| {
        let entry = entries
            .iter()
            .find(|entry| entry.target == fx_schema::PropertyTarget::layer(result_id, property))
            .unwrap();
        entry
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()
            .iter()
            .map(|key| key.value().clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        values(PropType::RectSize),
        vec![
            PropertyValue::Vector2([80.0, 40.0]),
            PropertyValue::Vector2([140.0, 60.0])
        ]
    );
    assert_eq!(
        values(PropType::PositionX),
        vec![PropertyValue::Float(43.0), PropertyValue::Float(73.0)]
    );
    assert_eq!(
        values(PropType::PositionY),
        vec![PropertyValue::Float(16.0), PropertyValue::Float(26.0)]
    );
}

#[test]
fn identity_valued_group_with_keys_is_not_discarded() {
    let mut value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    let group_transform: Transform = serde_json::from_value(group["transform"].clone()).unwrap();
    assert!(identity(&group_transform));
    let group_id = LayerId::new(group["id"].as_u64().unwrap());
    let mut leaf = rect(&value, 701);
    leaf["parent"] = group["id"].clone();
    leaf["transform"] = group["transform"].clone();
    group["layers"] = json!([leaf]);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries": [keyed_entry(
        group_id,
        PropType::Opacity,
        [(0, PropertyValue::Float(100.0)), (500, PropertyValue::Float(25.0))],
    )]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let roundtrip = to_structural_fx_document(&native, Some(1)).unwrap();
    let opacity = roundtrip
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|target| target.property_type() == PropType::Opacity)
        })
        .expect("vector Group opacity keys survive fresh native authoring");
    let keys = opacity.animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[1].value(), &PropertyValue::Float(25.0));
}

#[test]
fn delayed_child_does_not_shift_inherited_group_keys_silently() {
    let mut value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    let group_id = LayerId::new(group["id"].as_u64().unwrap());
    let mut child = rect(&value, 701);
    child["parent"] = group["id"].clone();
    child["transform"] = group["transform"].clone();
    child["activeRange"] = json!({"start": 500, "duration": 1000});
    group["layers"] = json!([child]);
    let sibling = rect(&value, 702);
    value["composition"]["layers"] = json!([group, sibling]);
    value["composition"]["dynamics"] = json!({"entries": [keyed_entry(
        group_id,
        PropType::Opacity,
        [(0, PropertyValue::Float(100.0)), (1000, PropertyValue::Float(25.0))],
    )]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 702")
    );
    assert!(output.diagnostics.iter().all(|diagnostic| {
        diagnostic.layer_id != Some(group_id) || !diagnostic.message.contains("subtree omitted")
    }));
}

#[test]
fn transformed_multichild_group_is_not_flattened_and_sibling_survives() {
    let mut value = imported();
    let mut group = value["composition"]["layers"][0].clone();
    group["transform"]["opacity"] = json!(50.0);
    let mut a = rect(&value, 100);
    let mut b = rect(&value, 101);
    a["parent"] = group["id"].clone();
    b["parent"] = group["id"].clone();
    group["layers"] = json!([a, b]);
    let sibling = rect(&value, 102);
    value["composition"]["layers"] = json!([group, sibling]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 102"),
        "independent sibling was lost: {:?}",
        output.diagnostics
    );
    let roundtrip = to_structural_fx_document(&native, Some(1)).unwrap();
    let group = group_with_opacity(roundtrip.document.composition().layers(), 50.0)
        .expect("the transformed multi-child Group survives as editable hierarchy");
    assert_eq!(group.transform.opacity.value(), 50.0);
    assert!(has_descendant_named(&group.layers, "Current solid 100"));
    assert!(has_descendant_named(&group.layers, "Current solid 101"));
    assert!(has_descendant_named(
        roundtrip.document.composition().layers(),
        "Current solid 102"
    ));
    assert!(output.diagnostics.iter().all(|diagnostic| {
        diagnostic.layer_id != Some(LayerId::new(1))
            || !diagnostic.message.contains("subtree omitted")
    }));
}

fn mixed_scene_with_text_branch() -> Value {
    let mut value = imported();
    let mut scene = value["composition"]["layers"][0].clone();
    scene["id"] = json!(60_000);
    scene["name"] = json!("S06 scene");
    scene["transform"]["opacity"] = json!(50.0);
    let mut text_group = scene.clone();
    text_group["id"] = json!(60_060);
    text_group["name"] = json!("S06 words");
    text_group["transform"]["opacity"] = json!(100.0);
    text_group["parent"] = json!(60_000);
    let mut text = json!({
        "type": "Text", "id": 60_061, "name": "S06 title",
        "parent": null,
        "activeRange": {"start": 0, "duration": 2000},
        "transform": scene["transform"],
        "sourceText": {
            "text": "blue", "fontFamily": "Inter-Regular", "fontStyle": "Regular",
            "fontSize": 42.0, "applyFill": true, "fillColor": [1.0, 1.0, 1.0, 1.0]
        }
    });
    text["transform"]["opacity"] = json!(100.0);
    text_group["layers"] = json!([text]);
    let mut painted = rect(&value, 60_100);
    painted["name"] = json!("S06 shoe");
    painted["parent"] = json!(60_000);
    scene["layers"] = json!([painted, text_group]);
    let sibling = rect(&value, 60_200);
    value["composition"]["layers"] = json!([scene, sibling]);
    value["composition"]["dynamics"] = json!({"entries": [keyed_entry(
        LayerId::new(60_100), PropType::PositionX,
        [(0, PropertyValue::Float(120.0)), (1000, PropertyValue::Float(240.0))],
    )]});
    value
}

#[test]
fn mixed_scene_keeps_text_and_independent_paints_with_certified_consumer_bounds() {
    let output = export(mixed_scene_with_text_branch());
    let native = read_project(&output.bytes).unwrap();
    let roundtrip = to_structural_fx_document(&native, Some(1)).unwrap();
    let owner = group_with_opacity(roundtrip.document.composition().layers(), 50.0)
        .expect("mixed scene survives as editable precomposition");
    assert!(has_descendant_named(&owner.layers, "S06 shoe"));
    assert!(has_descendant_named(&owner.layers, "S06 title"));
    assert!(
        roundtrip
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .any(|entry| {
                entry.target.as_property().is_some_and(|target| {
                    target.property_type() == PropType::PositionX
                        && entry.animator.keyframe_track().is_some_and(|track| {
                            track.keyframes().len() == 2
                                && matches!(
                                    (track.keyframes()[0].value(), track.keyframes()[1].value()),
                                    (PropertyValue::Float(first), PropertyValue::Float(last))
                                        if (last - first - 120.0).abs() < 0.001
                                )
                        })
                })
            }),
        "moving non-Text sibling keeps editable native position keys"
    );
    assert!(has_descendant_named(
        roundtrip.document.composition().layers(),
        "Current solid 60200"
    ));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_060)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_061)));
}

#[test]
fn custom_shader_group_matches_unshaded_owner_without_discarding_children_or_paint() {
    let mut source = mixed_scene_with_text_branch();
    let words = &mut source["composition"]["layers"][0]["layers"][1];
    words["effects"] = json!([{
        "id": 60_063, "enabled": true,
        "effect": {"type": "customShader", "name": "text halo", "wgsl": "", "params": []}
    }]);
    source["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(
            serde_json::to_value(keyed_entry(
                LayerId::new(60_060),
                PropType::ScaleX,
                [
                    (0, PropertyValue::Float(100.0)),
                    (1000, PropertyValue::Float(-100.0)),
                ],
            ))
            .unwrap(),
        );
    let output = export(source.clone());
    let native = read_project(&output.bytes).unwrap();
    let imported = to_structural_fx_document(&native, Some(1)).unwrap();
    let scene = group_with_opacity(imported.document.composition().layers(), 50.0)
        .expect("scene, title and independent paint survive the omitted halo");
    assert!(has_descendant_named(&scene.layers, "S06 shoe"));
    assert!(has_descendant_named(&scene.layers, "S06 title"));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(60_060))
            && diagnostic.message.contains("CustomShader")
    }));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_060)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_061)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_000)));

    // A mapped effect must not be silently discarded with its text group.
    source["composition"]["layers"][0]["layers"][1]["effects"] = json!([{
        "id": 60_063, "enabled": true,
        "effect": {"type": "gaussianBlur", "blurriness": 7}
    }]);
    let rejected = export(source);
    assert!(rejected.omitted_layer_ids.contains(&LayerId::new(60_060)));
}

#[test]
fn mixed_shader_scope_retains_editable_title_and_paint() {
    let mut source = mixed_scene_with_text_branch();
    let mut scene = source["composition"]["layers"][0].clone();
    let mut scope = scene["layers"][1].clone();
    scope["name"] = json!("S13 CRT screen");
    scope["effects"] = json!([
        {"id": 60_063, "enabled": true, "effect": {
            "type": "tintTritone", "amount": 100,
            "blackR": 0.0, "blackG": 0.018, "blackB": 0.008,
            "whiteR": 0.52, "whiteG": 1.0, "whiteB": 0.7
        }},
        {"id": 60_064, "enabled": true, "effect": {
            "type": "customShader", "name": "omitted CRT look", "wgsl": "", "params": []
        }}
    ]);
    let mut paint = scene["layers"][0].clone();
    paint["parent"] = json!(60_060);
    let mut title = scope["layers"][0].clone();
    title["parent"] = json!(60_060);
    scope["layers"] = json!([paint, title]);
    scene["layers"] = json!([scope]);
    source["composition"]["layers"][0] = scene;
    source["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(
            serde_json::to_value(keyed_entry(
                LayerId::new(60_060),
                PropType::PositionX,
                [
                    (0, PropertyValue::Float(320.0)),
                    (1000, PropertyValue::Float(360.0)),
                ],
            ))
            .unwrap(),
        );

    let output = export(source.clone());
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_000)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_061)));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(60_060))
            && diagnostic.message.contains("CustomShader")
    }));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_060)));
    let native = read_project(&output.bytes).unwrap();
    let roundtrip = to_structural_fx_document(&native, Some(1)).unwrap();
    assert!(has_descendant_named(
        roundtrip.document.composition().layers(),
        "S06 title"
    ));
    assert!(has_descendant_named(
        roundtrip.document.composition().layers(),
        "S06 shoe"
    ));

    // A native Gaussian Blur samples beyond its input pixel, so the consumer
    // viewport cannot certify a canvas for unknown glyph extents.
    source["composition"]["layers"][0]["layers"][0]["effects"] = json!([{
        "id": 60_063, "enabled": true,
        "effect": {"type": "gaussianBlur", "blurriness": 7}
    }]);
    let rejected = export(source);
    assert!(rejected.omitted_layer_ids.contains(&LayerId::new(60_060)));
}

#[test]
fn opacity_keyed_multi_glyph_title_keeps_native_group_opacity_and_both_text_layers() {
    let mut source = mixed_scene_with_text_branch();
    let words = &mut source["composition"]["layers"][0]["layers"][1];
    let mut second = words["layers"][0].clone();
    second["id"] = json!(60_062);
    second["name"] = json!("S13 subtitle");
    words["layers"].as_array_mut().unwrap().push(second);
    source["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(
            serde_json::to_value(keyed_entry(
                LayerId::new(60_060),
                PropType::Opacity,
                [
                    (0, PropertyValue::Float(0.0)),
                    (1000, PropertyValue::Float(100.0)),
                ],
            ))
            .unwrap(),
        );
    let output = export(source);
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_060)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_061)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(60_062)));
    let native = read_project(&output.bytes).unwrap();
    let roundtrip = to_structural_fx_document(&native, Some(1)).unwrap();
    assert!(has_descendant_named(
        roundtrip.document.composition().layers(),
        "S06 title"
    ));
    assert!(has_descendant_named(
        roundtrip.document.composition().layers(),
        "S13 subtitle"
    ));
    assert!(
        roundtrip
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .any(|entry| entry.target.as_property().is_some_and(|target| {
                target.property_type() == PropType::Opacity
                    && entry
                        .animator
                        .keyframe_track()
                        .is_some_and(|track| track.keyframes().len() == 2)
            }))
    );
}

#[test]
fn mixed_scene_pruning_rejects_referenced_or_effectful_text_scope() {
    let document =
        EditableFxCompositionDocument::from_json_value(mixed_scene_with_text_branch()).unwrap();
    let LayerData::Group(scene) = document.composition().layers()[0].data() else {
        panic!("scene group");
    };
    let mut references = BTreeMap::new();
    let mut ids = BTreeSet::new();
    collect_source_layer_ids(document.composition().layers(), &mut ids);
    for id in ids {
        references.insert(id, source_variants::SourceVariantEligibility::default());
    }
    assert!(prune_independent_text_branches(scene, &references).is_some());
    references
        .get_mut(&LayerId::new(60_061))
        .unwrap()
        .referenced_as_matte = true;
    assert!(prune_independent_text_branches(scene, &references).is_none());
    references
        .get_mut(&LayerId::new(60_061))
        .unwrap()
        .referenced_as_matte = false;
    references
        .get_mut(&LayerId::new(60_000))
        .unwrap()
        .referenced_as_matte = true;
    assert!(prune_independent_text_branches(scene, &references).is_none());
    references
        .get_mut(&LayerId::new(60_000))
        .unwrap()
        .referenced_as_matte = false;
    let mut scene = scene.clone();
    scene.motion_blur = true;
    assert!(prune_independent_text_branches(&scene, &references).is_none());
    scene.motion_blur = false;
    let LayerData::Group(words) = scene.layers[1].data() else {
        panic!("text-only group");
    };
    let mut words = words.clone();
    words.motion_blur = true;
    scene.layers[1] = Layer::from_data(&LayerData::Group(words)).unwrap();
    assert!(prune_independent_text_branches(&scene, &references).is_none());
}

#[test]
fn edited_solid_with_vector_paint_exports_shape_and_keeps_solid_sibling() {
    let mut value = imported();
    let mut vector = rect(&value, 100);
    vector["rect"]["size"] = json!([12.5, 20.0]);
    vector["rect"]["fillColor"] = json!([0.2, 0.3, 0.4, 0.5]);
    let sibling = rect(&value, 101);
    value["composition"]["layers"] = json!([vector, sibling]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2);
    assert_eq!(layers(&native)[0].record.layer_type(), 4);
    assert_eq!(layers(&native)[0].record.source_id(), 0);
    assert_ne!(layers(&native)[1].record.source_id(), 0);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(100))
                && d.message.contains("instead of flattening"))
    );
    let imported = to_structural_fx_document(&native, Some(1)).unwrap();
    fn native_rect(layers: &[fx_schema::Layer]) -> Option<&RectLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::Rect(rect) if rect.name == "Current solid 100" => Some(rect),
            LayerData::Group(group) => native_rect(&group.layers),
            _ => None,
        })
    }
    let vector = native_rect(imported.document.composition().layers()).unwrap();
    assert_eq!(vector.rect.size, [12.5, 20.0]);
    assert_eq!(vector.rect.fill_color, [0.2, 0.3, 0.4, 0.5]);
}

#[test]
fn out_of_bounds_edits_do_not_export_stale_fallbacks_or_discard_siblings() {
    let mut value = imported();
    let mut bad = rect(&value, 100);
    bad["rect"]["size"] = json!([65536.0, 20.0]);
    let good = rect(&value, 101);
    value["composition"]["layers"] = json!([bad, good]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 101");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(100))
                && diagnostic.message.contains("bounds")),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn edited_single_paint_shape_exports_current_archive_values_without_aep_source() {
    use fx_schema::layer::{ShapeEllipse, ShapePath, ShapePathCommand, ShapePolyStar};

    for (kind, shape_content) in [
        (
            "ellipse",
            json!({
                "path": ShapePath { commands: Vec::new() },
                "fills": [{"paint": {"type": "solid", "color": [0.3, 0.6, 0.2, 1.0]}}],
                "ellipse": ShapeEllipse {size: [125.0, 66.0], position: [11.0, -8.0], reversed: false},
            }),
        ),
        (
            "star",
            json!({
                "path": ShapePath { commands: Vec::new() },
                "fills": [{"paint": {"type": "solid", "color": [0.3, 0.6, 0.2, 1.0]}}],
                "polyStar": ShapePolyStar {points: 7.0, outer_radius: 55.0, ..ShapePolyStar::default()},
            }),
        ),
        (
            "path",
            json!({
                "path": ShapePath { commands: vec![
                    ShapePathCommand::MoveTo { x: 0.0, y: 0.0, mirror: None, corner_radius: None },
                    ShapePathCommand::LineTo { x: 50.0, y: 0.0, mirror: None, corner_radius: None },
                    ShapePathCommand::LineTo { x: 50.0, y: 25.0, mirror: None, corner_radius: None },
                    ShapePathCommand::Close,
                ] },
                "fills": [{"paint": {"type": "solid", "color": [0.3, 0.6, 0.2, 1.0]}}],
            }),
        ),
    ] {
        let mut value = imported();
        let mut shape = rect(&value, 210);
        shape["type"] = json!("Shape");
        shape.as_object_mut().unwrap().remove("rect");
        shape["name"] = json!(format!("edited-{kind}"));
        shape["shape"] = shape_content;
        value["composition"]["layers"] = json!([shape]);
        value["composition"]["dynamics"] = json!({"entries": []});
        let edited = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let saved = edited.to_json_vec().unwrap();
        let reopened = EditableFxCompositionDocument::from_json_slice(&saved).unwrap();
        let output = to_aep(&reopened).unwrap();
        let project = read_project(&output.bytes).unwrap();
        assert_eq!(
            layers(&project).len(),
            1,
            "{kind}: {:?}",
            output.diagnostics
        );
        let imported = to_structural_fx_document(&project, Some(1)).unwrap();
        fn visible(layers: &[fx_schema::Layer]) -> Option<&fx_schema::ShapeLayer> {
            layers.iter().find_map(|layer| match layer.data() {
                LayerData::Shape(shape) if !shape.is_hidden => Some(shape),
                LayerData::Group(group) => visible(&group.layers),
                _ => None,
            })
        }
        let actual = visible(imported.document.composition().layers()).unwrap();
        assert_eq!(actual.name, format!("edited-{kind}"));
        assert_eq!(actual.shape.fills.len(), 1);
        match kind {
            "ellipse" => assert_eq!(actual.shape.ellipse.as_ref().unwrap().size, [125.0, 66.0]),
            "star" => assert_eq!(actual.shape.poly_star.as_ref().unwrap().points, 7.0),
            "path" => assert_eq!(
                actual.shape.path.commands.last(),
                Some(&ShapePathCommand::Close)
            ),
            _ => unreachable!(),
        }
    }
}

#[test]
fn static_ellipse_native_source_import_and_edited_group_export() {
    let native = read_project(include_bytes!(
        "../../tests/fixtures/static_ellipse_enclosure/native.aep"
    ))
    .unwrap();
    let document = to_structural_fx_document(&native, Some(1)).unwrap();
    fn find_shape(value: &mut Value) -> Option<&mut Value> {
        if value["type"] == "Shape" {
            return Some(value);
        }
        value["layers"]
            .as_array_mut()?
            .iter_mut()
            .find_map(find_shape)
    }
    for size in [[82.0, 54.0], [126.0, 70.0]] {
        let mut value = document.document.to_json_value().unwrap();
        let group = &mut value["composition"]["layers"][0];
        let shape = find_shape(group).expect("native editable ellipse");
        assert_eq!(shape["shape"]["ellipse"]["size"], json!([82.0, 54.0]));
        assert_eq!(shape["shape"]["ellipse"]["position"], json!([11.0, -8.0]));
        shape["shape"]["ellipse"]["size"] = json!(size);
        let mut sibling = shape.clone();
        sibling["id"] = json!(100);
        sibling["parent"] = Value::Null;
        sibling["name"] = json!("independent-sibling");
        sibling["shape"]["ellipse"]["size"] = json!([12.0, 12.0]);
        sibling["shape"]["ellipse"]["position"] = json!([250.0, 190.0]);
        group["transform"]["opacity"] = json!(50.0);
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(sibling);
        let output = export(value);
        let generated = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&generated).len(), 2, "{:?}", output.diagnostics);
        assert!(
            output
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("subtree omitted")),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn static_ellipse_precomposition_retains_edited_geometry_and_sibling() {
    for size in [[82.0, 54.0], [126.0, 70.0]] {
        let mut value = imported();
        let mut group = value["composition"]["layers"][0].clone();
        group["name"] = json!("finite-ellipse-group");
        group["transform"]["opacity"] = json!(50.0);
        let mut shape = rect(&value, 210);
        shape["type"] = json!("Shape");
        shape.as_object_mut().unwrap().remove("rect");
        shape["name"] = json!("editable-ellipse");
        shape["parent"] = group["id"].clone();
        shape["shape"] = json!({
            "path": {"commands": []},
            "ellipse": {"size": size, "position": [11.0, -8.0]},
            "fills": [{"paint": {"type": "solid", "color": [0.0, 1.0, 1.0, 1.0]}}]
        });
        group["layers"] = json!([shape]);
        value["composition"]["layers"] = json!([group, rect(&value, 211)]);
        value["composition"]["dynamics"] = json!({"entries": []});
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
        let imported = to_structural_fx_document(&native, Some(1)).unwrap();
        fn ellipse(layers: &[fx_schema::Layer]) -> Option<&fx_schema::layer::ShapeEllipse> {
            layers.iter().find_map(|layer| match layer.data() {
                LayerData::Shape(shape) if shape.name == "editable-ellipse" => {
                    shape.shape.ellipse.as_ref()
                }
                LayerData::Group(group) => ellipse(&group.layers),
                _ => None,
            })
        }
        let actual =
            ellipse(imported.document.composition().layers()).expect("editable ellipse retained");
        assert_eq!(actual.size, size);
        assert_eq!(actual.position, [11.0, -8.0]);
        assert!(
            output
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("subtree omitted")),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn edited_boolean_and_supported_sibling_export_as_ordered_native_layers() {
    // Supplemental explicit edited FX input; the pinned Solid source supplies
    // only archive metadata, not an independent native Merge Paths oracle.
    let mut value = imported();
    let root_transform = value["composition"]["layers"][0]["transform"].clone();
    let span = value["composition"]["layers"][0]["playback"]["inputRange"].clone();
    let mut template = rect(&value, 604);
    template["type"] = json!("Shape");
    template.as_object_mut().unwrap().remove("rect");
    template["shape"] = json!({
        "path":{"commands":[]},
        "fills":[{"paint":{"type":"solid","color":[0.3,0.6,0.2,1.0]}}],
        "strokes":[]
    });
    let path = |left: f64| {
        json!({"commands": [
            {"type":"moveTo","x":left,"y":0.0},
            {"type":"lineTo","x":left+50.0,"y":0.0},
            {"type":"lineTo","x":left+50.0,"y":40.0},
            {"type":"lineTo","x":left,"y":40.0},
            {"type":"close"}
        ]})
    };
    let mut a = template.clone();
    a["id"] = json!(601);
    a["parent"] = json!(600);
    a["name"] = json!("Edited operand A");
    a["activeRange"] = span.clone();
    a["transform"] = root_transform.clone();
    a["shape"]["path"] = path(0.0);
    a["shape"]["ellipse"] = Value::Null;
    a["shape"]["polyStar"] = Value::Null;
    a["shape"]["fills"] = json!([]);
    a["shape"]["strokes"] = json!([]);
    let mut b = a.clone();
    b["id"] = json!(602);
    b["name"] = json!("Edited operand B");
    b["shape"]["path"] = path(25.0);
    let boolean = json!({
        "type":"BooleanOperation", "id":600, "name":"Edited subtract",
        "parent":null, "activeRange":span, "transform":root_transform,
        "op":"subtract", "layers":[a,b],
        "fills":template["shape"]["fills"],
        "strokes":template["shape"]["strokes"]
    });
    let mut sibling = template;
    sibling["id"] = json!(603);
    sibling["parent"] = Value::Null;
    sibling["name"] = json!("Editable sibling");
    sibling["activeRange"] = boolean["activeRange"].clone();
    sibling["transform"] = boolean["transform"].clone();
    sibling["shape"]["path"] = path(120.0);
    sibling["shape"]["ellipse"] = Value::Null;
    sibling["shape"]["polyStar"] = Value::Null;
    value["composition"]["layers"] = json!([boolean, sibling]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let edited = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let saved = edited.to_json_vec().unwrap();
    let output = to_aep(&EditableFxCompositionDocument::from_json_slice(&saved).unwrap()).unwrap();
    let project = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&project).len(), 2, "{:?}", output.diagnostics);
    assert_eq!(layers(&project)[0].name.as_ref(), "Edited subtract");
    assert_eq!(layers(&project)[1].name.as_ref(), "Editable sibling");
    let roundtrip = to_structural_fx_document(&project, Some(1)).unwrap();
    fn find_boolean(layers: &[fx_schema::Layer]) -> Option<&fx_schema::BooleanOperationLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::BooleanOperation(value) if value.name == "Edited subtract" => Some(value),
            LayerData::Group(group) => find_boolean(&group.layers),
            _ => None,
        })
    }
    let mapped = find_boolean(roundtrip.document.composition().layers()).unwrap();
    assert_eq!(mapped.op, fx_schema::BooleanOp::Subtract);
    assert_eq!(mapped.layers.len(), 2);
}

#[test]
fn edited_boolean_rect_operands_export_without_flattening_to_media() {
    let mut value = imported();
    let span = value["composition"]["layers"][0]["playback"]["inputRange"].clone();
    let transform = value["composition"]["layers"][0]["transform"].clone();
    let mut a = rect(&value, 611);
    a["parent"] = json!(610);
    a["activeRange"] = span.clone();
    a["transform"] = transform.clone();
    a["rect"]["position"] = json!([2.0, -3.0]);
    a["rect"]["size"] = json!([80.0, 60.0]);
    let mut b = a.clone();
    b["id"] = json!(612);
    b["rect"]["position"] = json!([30.0, 10.0]);
    let boolean = json!({
        "type":"BooleanOperation", "id":610, "name":"Edited Rect intersection",
        "parent":null, "activeRange":span, "transform":transform,
        "op":"intersect", "layers":[a,b],
        "fills":[{"paint":{"type":"solid","color":[0.2,0.4,0.8,1.0]}}]
    });
    value["composition"]["layers"] = json!([boolean]);
    value["composition"]["dynamics"] = json!({"entries":[keyed_entry(
        LayerId::new(611),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(0.0)),
            (500, PropertyValue::Float(24.0)),
        ],
    )]});
    let mut animated_paint = value.clone();
    animated_paint["composition"]["dynamics"] = json!({"entries":[keyed_entry(
        LayerId::new(610),
        PropType::FillColor,
        [
            (0, PropertyValue::Color([0.2, 0.4, 0.8, 1.0])),
            (500, PropertyValue::Color([0.8, 0.2, 0.1, 1.0])),
        ],
    )]});
    let animated_output = export(animated_paint);
    let animated_native = read_project(&animated_output.bytes).unwrap();
    assert_eq!(
        layers(&animated_native).len(),
        1,
        "{:?}",
        animated_output.diagnostics
    );
    let animated_import = to_structural_fx_document(&animated_native, Some(1)).unwrap();
    assert!(
        animated_import
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .any(|entry| entry.target.as_property().is_some_and(|target| {
                target.property_type() == PropType::FillColor
                    && entry
                        .animator
                        .keyframe_track()
                        .is_some_and(|track| track.keyframes().len() == 2)
            })),
        "native Boolean owned paint keys must remain editable"
    );
    let output = export(value);
    let project = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&project).len(), 1, "{:?}", output.diagnostics);
    assert_eq!(layers(&project)[0].record.layer_type(), 4);
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    fn operation(layers: &[fx_schema::Layer]) -> Option<&fx_schema::BooleanOperationLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::BooleanOperation(value) if value.name == "Edited Rect intersection" => {
                Some(value)
            }
            LayerData::Group(group) => operation(&group.layers),
            _ => None,
        })
    }
    let result = operation(imported.document.composition().layers()).unwrap();
    assert_eq!(result.op, fx_schema::BooleanOp::Intersect);
    assert_eq!(result.layers.len(), 2);
    let operand_id = result.layers[0].id();
    let position = imported
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| {
            entry.target == fx_schema::PropertyTarget::layer(operand_id, PropType::PositionX)
        })
        .expect("Boolean operand owns its native vector Transform keys");
    assert_eq!(
        position
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()
            .len(),
        2
    );
}

#[test]
fn animated_native_transform_exports_native_keys_without_stale_base_fallback() {
    let native = read_project(include_bytes!(
        "../../tests/fixtures/properties/property_2D_position.aep"
    ))
    .unwrap();
    let document = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document;
    let output = to_aep(&document).unwrap();
    let exported = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&exported).len(), 1, "{:?}", output.diagnostics);
    assert!(matches!(
        exported
            .item(layers(&exported)[0].record.source_id())
            .map(|item| &item.kind),
        Some(ItemKind::Composition(_))
    ));
    let roundtrip = to_structural_fx_document(&exported, Some(1)).unwrap();
    for property in [PropType::PositionX, PropType::PositionY] {
        let entry = roundtrip
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|target| target.property_type() == property)
            })
            .expect("animated occurrence Position survives through the generated hierarchy");
        // The untransformed root Group uses the final-root output viewport, so
        // the round trip keeps the source keys in composition space.
        let keys = entry.animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].layer_time().as_millis(), 0);
        assert_eq!(keys[0].value(), &PropertyValue::Float(0.0));
        assert_eq!(keys[1].layer_time().as_millis(), 5_000);
        assert_eq!(keys[1].value(), &PropertyValue::Float(100.0));
    }
}

#[test]
fn edited_leaf_active_range_has_native_local_start_and_out_point() {
    let mut value = imported();
    let mut leaf = rect(&value, 401);
    leaf["activeRange"] = json!({"start": 250, "duration": 1500});
    value["composition"]["layers"] = json!([leaf]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let [layer] = layers(&native) else {
        panic!("edited timed layer omitted: {:?}", output.diagnostics);
    };
    assert!((layer.record.start_time().unwrap() - 0.25).abs() < 0.0001);
    assert_eq!(layer.record.in_point(), Some(0.0));
    assert!((layer.record.out_point().unwrap() - 1.5).abs() < 0.0001);
}

#[test]
fn root_duration_exports_positive_input_with_native_null_frame_endpoint() {
    let mut value = imported();
    value["duration"] = json!(0.001);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    assert_eq!(document.duration().as_millis(), 1);
    let output = to_aep_with_fps(&document, 30.0).unwrap();
    assert_eq!(output.root.duration_secs, 0.0);
    let project = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
        panic!("composition")
    };
    assert_eq!(composition.duration_secs, 0.0);
    assert!(!composition.layers.is_empty());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("quantized to 0 frames"))
    );
}

#[test]
fn root_duration_exports_current_duration_edits_without_millisecond_loss() {
    let mut value = imported();
    value["duration"] = json!(12.1833);
    let mut document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let original_duration = document.duration();
    let mut changed = document.to_json_value().unwrap();
    changed["composition"]["name"] = json!("Edited duration control");
    let changed = EditableFxCompositionDocument::from_json_value(changed).unwrap();
    document
        .replace_composition_and_duration(changed.composition().clone(), original_duration)
        .unwrap();
    let unchanged_endpoint = to_aep_with_fps(&document, 30.0).unwrap();
    assert_eq!(unchanged_endpoint.root.name, "Edited duration control");
    assert_eq!(
        unchanged_endpoint.root.duration_secs,
        f64::from(365 * 24_576 / 30) / 24_576.0
    );

    // These current JSON edits have the same typed milliseconds but cross the
    // independently measured native half-frame boundary (365 -> 366).
    let mut changed = document.to_json_value().unwrap();
    changed["duration"] = json!(12.1833666666667);
    let changed = EditableFxCompositionDocument::from_json_value(changed).unwrap();
    assert_eq!(changed.duration(), original_duration);
    assert_eq!(
        to_aep_with_fps(&changed, 30.0).unwrap().root.duration_secs,
        f64::from(366 * 24_576 / 30) / 24_576.0
    );

    document
        .replace_composition_and_duration(
            document.composition().clone(),
            fx_schema::time::Duration::from_secs(7.5),
        )
        .unwrap();
    assert_eq!(
        to_aep_with_fps(&document, 30.0).unwrap().root.duration_secs,
        7.5
    );
}

#[test]
fn root_duration_preserves_native_endpoint_quantization() {
    // Independently authored/saved/reopened AE 26.5 controls at 30fps.
    // The exact JSON endpoint must survive the typed millisecond projection.
    for (seconds, frames) in [
        (12.167, 365),
        (959.0 / 30.0, 959),
        (77.867, 2336),
        ((365.5 - 0.001) / 30.0, 365),
        (365.5 / 30.0, 366),
        ((365.5 + 0.001) / 30.0, 366),
    ] {
        let mut value = imported();
        value["duration"] = json!(seconds);
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let preserved = document.to_json_value().unwrap()["duration"]
            .as_f64()
            .unwrap();
        assert!((preserved - seconds).abs() <= seconds * f64::EPSILON);
        let output = to_aep_with_fps(&document, 30.0).unwrap();
        assert!(
            (output.root.duration_secs - f64::from(frames) / 30.0).abs() < 1.0 / 24_576.0,
            "{seconds}s: {}s instead of {frames} frames",
            output.root.duration_secs
        );
    }
}

#[test]
fn selected_export_fps_preserves_seconds_based_layer_and_key_times() {
    let mut value = imported();
    value["duration"] = json!(2.001);
    let mut leaf = rect(&value, 801);
    leaf["activeRange"] = json!({"start": 250, "duration": 1500});
    value["composition"]["layers"] = json!([leaf]);
    value["composition"]["dynamics"] = json!({"entries": [keyed_entry(
        LayerId::new(801), PropType::Opacity,
        [(250, PropertyValue::Float(1.0)), (750, PropertyValue::Float(0.5))],
    )]});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    for fps in [24.0, 25.0, 29.97, 30.0, 60.0] {
        let output = to_aep_with_fps(&document, fps).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
            panic!("composition")
        };
        assert!((comp.frame_rate - fps).abs() < 0.00001, "{fps}");
        assert!(
            (comp.duration_secs - (2.001 * fps).round() / fps).abs() < 0.0001,
            "{fps}"
        );
        let [layer] = layers(&native) else {
            panic!("export dropped layer at {fps}fps: {:?}", output.diagnostics)
        };
        assert!((layer.record.start_time().unwrap() - 0.25).abs() < 0.0001);
        assert!((layer.record.out_point().unwrap() - 1.5).abs() < 0.0001);
        let opacity = crate::properties::read_transform(&layer.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Opacity")
            .unwrap();
        let keys = &opacity.numeric.unwrap().keyframes;
        assert_eq!(keys.len(), 2, "{fps}");
        assert!((keys[0].time_secs - 0.25).abs() < 0.0001);
        assert!((keys[1].time_secs - 0.75).abs() < 0.0001);
    }
    for fps in [0.0, -1.0, f64::NAN, f64::INFINITY, 241.0] {
        assert!(to_aep_with_fps(&document, fps).is_err(), "{fps}");
    }
}

#[test]
fn duration_rounding_and_nonidentity_source_clock_are_explicit() {
    let mut value = imported();
    value["duration"] = json!(1.001);
    let output = export(value);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("24 frames"))
    );
    assert_eq!(layers(&read_project(&output.bytes).unwrap()).len(), 1);

    fn set_source_clock_rate(layer: &mut Value) -> bool {
        if layer["type"] == "Group" && layer["name"] == "Source content clock" {
            let window = layer["playback"]["inputRange"].clone();
            let duration = window["duration"].as_u64().unwrap();
            layer["playback"] = json!({
                "type": "windowed",
                "inputRange": window,
                "mapping": {"type": "linear", "input": window,
                    "output": {"start": 0, "duration": duration * 2}},
                "inputOffsetMs": 0
            });
            return true;
        }
        layer["layers"]
            .as_array_mut()
            .is_some_and(|layers| layers.iter_mut().any(set_source_clock_rate))
    }

    let mut value = imported();
    assert!(
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .any(set_source_clock_rate)
    );
    let mut sibling = rect(&imported(), 9_901);
    sibling["name"] = json!("clock sibling");
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(sibling);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let group = group_with_nonidentity_playback(document.composition().layers())
        .expect("test edit targets a typed Group clock");
    assert!(
        matches!(group.playback.mapping(), LayerPlaybackMapping::Linear { input, output }
        if output.duration.as_millis() == input.duration.as_millis() * 2)
    );
    let clock_owner = group.id;
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    // 2x over the full 30-second occurrence exceeds its 30-second source
    // domain. This is a diagnosed omission, not an identity-clock success.
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(clock_owner)
            && diagnostic
                .message
                .contains("Group source clock cannot be represented exactly")
    }));
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "clock sibling")
    );
}

#[test]
fn constant_animators_use_validated_single_hold_keys_on_mapped_channels() {
    let id = LayerId::new(880);
    let entries = vec![
        constant_entry(id, PropType::AnchorPointX, PropertyValue::Float(12.0)),
        constant_entry(id, PropType::AnchorPointY, PropertyValue::Float(-4.0)),
        constant_entry(id, PropType::RectSize, PropertyValue::Vector2([80.0, 40.0])),
        constant_entry(
            id,
            PropType::FillColor,
            PropertyValue::Color([0.1, 0.2, 0.3, 1.0]),
        ),
        constant_entry(
            id,
            PropType::StrokeJoin,
            PropertyValue::String("round".into()),
        ),
        constant_entry(id, PropType::AudioVolume, PropertyValue::Float(0.5)),
    ];
    let pair = paired_track(
        track(
            &crate::export_document::AnimationIndex::new(&entries),
            id,
            PropType::AnchorPointX,
        )
        .unwrap(),
        track(
            &crate::export_document::AnimationIndex::new(&entries),
            id,
            PropType::AnchorPointY,
        )
        .unwrap(),
        [0.0; 2],
        1.0,
        true,
    )
    .unwrap()
    .unwrap();
    let vector = vector_track(
        track(
            &crate::export_document::AnimationIndex::new(&entries),
            id,
            PropType::RectSize,
        )
        .unwrap(),
    )
    .unwrap()
    .unwrap();
    let color = color_track(
        track(
            &crate::export_document::AnimationIndex::new(&entries),
            id,
            PropType::FillColor,
        )
        .unwrap(),
    )
    .unwrap()
    .unwrap();
    let join = stroke_join_track(
        track(
            &crate::export_document::AnimationIndex::new(&entries),
            id,
            PropType::StrokeJoin,
        )
        .unwrap(),
    )
    .unwrap()
    .unwrap();
    let audio = super::audio::levels_animation(
        &crate::export_document::AnimationIndex::new(&entries),
        id,
        true,
    )
    .unwrap()
    .unwrap();
    for track in [&pair, &vector, &color, &join, &audio] {
        assert_eq!(track.keys.len(), 1);
        assert_eq!(track.keys[0].time_millis, 0);
        assert!(
            track.keys[0]
                .easing
                .iter()
                .all(|easing| *easing == KeyframeEasing::Hold)
        );
    }
    let invalid = [constant_entry(
        id,
        PropType::Opacity,
        PropertyValue::String("not numeric".into()),
    )];
    assert!(
        scalar_track(
            track(
                &crate::export_document::AnimationIndex::new(&invalid),
                id,
                PropType::Opacity
            )
            .unwrap(),
            100.0
        )
        .is_err()
    );
}

#[test]
fn reflected_gradient_reports_editable_linear_normalization() {
    let mut value = imported();
    let mut shape = rect(&value, 881);
    shape["type"] = json!("Shape");
    shape.as_object_mut().unwrap().remove("rect");
    shape["shape"] = json!({
        "path":{"commands":[]},
        "ellipse":{"size":[120.0,80.0],"position":[0.0,0.0],"reversed":false},
        "fills":[{"paint":{
            "type":"gradient",
            "gradientType":"reflected",
            "start":[0.0,0.0],
            "end":[100.0,0.0],
            "stops":[
                {"offset":0.0,"color":[1.0,0.0,0.0,1.0]},
                {"offset":1.0,"color":[0.0,0.0,1.0,1.0]}
            ]
        }}],
        "strokes":[]
    });
    value["composition"]["layers"] = json!([shape]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = export(value);
    assert_eq!(layers(&read_project(&output.bytes).unwrap()).len(), 1);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(881))
            && diagnostic
                .message
                .contains("mirrored Linear native controls")
    }));
}

#[test]
fn zero_gain_video_without_a_source_audio_track_keeps_its_picture() {
    assert_silent_video_keeps_picture(0);
}

#[test]
fn positive_gain_video_without_a_source_audio_track_keeps_its_picture() {
    assert_silent_video_keeps_picture(1);
}

fn assert_silent_video_keeps_picture(gain: i32) {
    use crate::writer::footage::{NativeFrameRate, NativeSourceFormat, RelativeMediaPath};
    use std::collections::BTreeMap;

    let mut value = imported();
    value["composition"]["layers"] = json!([{
        "type": "Video",
        "id": 884,
        "name": "Silent source with an authored gain",
        "playback": fixture_linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
        "sourceRange": {"start": 0, "duration": 1000},
        "sourceIntrinsicDuration": 1000,
        "volume": gain,
        "transform": identity_fx_transform(),
        "source": {"assetId": "silent-mov", "fit": "contain"}
    }]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let request = media_requests(&document).unwrap().pop().unwrap();
    let mut resolved = BTreeMap::new();
    resolved.insert(
        "silent-mov".to_owned(),
        media::ResolvedMediaSource {
            asset_id: request.asset_id,
            path: RelativeMediaPath::new("media/silent.mov").unwrap(),
            native_duration: None,
            format: NativeSourceFormat::QuickTime,
            dimensions: [320, 180],
            duration_millis: 1000,
            duration_millis_floor: 1000,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(25),
            audio_sample_rate: 0.0,
            wave_metadata: None,
        },
    );
    let output = to_aep_with_media(&document, &resolved).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(884)),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
}

#[test]
fn legacy_media_reaches_existing_native_media_dispatch() {
    use crate::writer::footage::{NativeFrameRate, NativeSourceFormat, RelativeMediaPath};
    use std::collections::BTreeMap;

    let mut value = imported();
    let transform = value["composition"]["layers"][0]["transform"].clone();
    let active_range = value["composition"]["layers"][0]["playback"]["inputRange"].clone();
    value["composition"]["layers"] = json!([{
        "type":"Media",
        "id":882,
        "name":"Legacy image",
        "parent":null,
        "activeRange":active_range,
        "transform":transform,
        "source":{
            "assetId":"legacy-image",
            "kind":"image",
            "fit":"stretch"
        }
    }]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let request = media_requests(&document).unwrap().pop().unwrap();
    let mut resolved = BTreeMap::new();
    resolved.insert(
        "legacy-image".to_owned(),
        media::ResolvedMediaSource {
            asset_id: request.asset_id,
            path: RelativeMediaPath::new("media/legacy.exr".to_owned()).unwrap(),
            native_duration: None,
            format: NativeSourceFormat::OpenExr,
            dimensions: [320, 180],
            duration_millis: 0,
            duration_millis_floor: 0,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(0),
            audio_sample_rate: 0.0,
            wave_metadata: None,
        },
    );
    let output = to_aep_with_media(&document, &resolved).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert_ne!(layers(&native)[0].record.source_id(), 0);
}

#[test]
fn shared_takeover_dispatch_publishes_clip_then_slide_parent() {
    use crate::writer::footage::{NativeFrameRate, NativeSourceFormat, RelativeMediaPath};
    use std::collections::BTreeMap;

    let mut value = imported();
    let template = EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let dimensions = template.dimensions();
    let duration = template.duration().as_millis();
    assert!(duration > 240);
    let active_range = fx_schema::TimeRangeProperty::new(
        Time::ZERO,
        fx_schema::Duration::from_millis(duration - 120),
    );
    value["composition"]["layers"] = json!([{
        "type": "Image",
        "id": 883,
        "name": "Top takeover",
        "parent": null,
        "activeRange": active_range,
        "placement": fx_schema::MediaPlacement::TopHalf,
        "transform": identity_fx_transform(),
        "source": {
            "assetId": "takeover-image",
            "sourceRect": {
                "x": 0.0,
                "y": 0.0,
                "width": dimensions.width,
                "height": dimensions.height / 2
            },
            "fit": "cover"
        }
    }]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let request = media_requests(&document).unwrap().pop().unwrap();
    let mut resolved = BTreeMap::new();
    resolved.insert(
        "takeover-image".to_owned(),
        media::ResolvedMediaSource {
            asset_id: request.asset_id,
            path: RelativeMediaPath::new("media/takeover.exr").unwrap(),
            native_duration: None,
            format: NativeSourceFormat::OpenExr,
            dimensions: [
                u16::try_from(dimensions.width).unwrap(),
                u16::try_from(dimensions.height).unwrap(),
            ],
            duration_millis: 0,
            duration_millis_floor: 0,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(0),
            audio_sample_rate: 0.0,
            wave_metadata: None,
        },
    );
    let output = to_aep_with_media(&document, &resolved).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
}

#[test]
fn adapter_media_discovery_promotes_duplicate_document_identity() {
    let mut value = imported();
    let duplicate = value["composition"]["layers"][0].clone();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let error = media_requests(&document).unwrap_err();
    assert!(matches!(error, AepWriteError::InvalidDocument(_)));
}

#[test]
fn shared_rectangle_program_keeps_geometry_tracks_before_dash_rewrite() {
    let mut value = imported();
    let mut layer = rect(&value, 884);
    layer["rect"]["strokeDashes"] = json!([6.0, 3.0]);
    value["composition"]["layers"] = json!([layer]);
    value["composition"]["dynamics"] = json!({
        "entries": [keyed_entry(
            LayerId::new(884),
            PropType::RectSize,
            [
                (0, PropertyValue::Vector2([120.0, 80.0])),
                (500, PropertyValue::Vector2([180.0, 100.0])),
            ],
        )]
    });
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let rect = match document.composition().layers()[0].data() {
        LayerData::Rect(rect) => rect,
        _ => panic!("rectangle"),
    };
    let dynamics = document.composition().dynamics().entries();
    let paints = paint_controls::materialize(
        &LayerData::Rect(rect.clone()),
        &crate::export_document::AnimationIndex::new(dynamics),
    )
    .unwrap();
    let program = vector_rect_paints_program(
        rect,
        &rect.transform,
        TransformAnimations::default(),
        rect.id,
        &paints,
        &crate::export_document::AnimationIndex::new(dynamics),
        true,
    )
    .unwrap();
    let rewritten = rect_dashes::isolate_static_dashed_stroke(
        rect,
        &crate::export_document::AnimationIndex::new(dynamics),
        program,
    )
    .unwrap();
    let animations = rewritten
        .program
        .contents
        .iter()
        .find_map(|content| match content {
            VectorContent::Geometry { animations, .. } => Some(animations),
            _ => None,
        })
        .unwrap();
    assert!(animations.rect_size.is_some());
    assert!(animations.rect_position.is_some());
    assert!(animations.rect_roundness.is_none());
}
