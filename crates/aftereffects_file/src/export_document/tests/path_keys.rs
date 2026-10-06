//! Fresh edited-FX export assertions. Native-record inspection is supplementary
//! structure evidence, not independent Adobe opening or rendered fidelity.

use super::*;
use crate::{properties, rifx::Chunk};

fn path(width: f64) -> fx_schema::ShapePath {
    serde_json::from_value(json!({"commands":[
        {"type":"moveTo","x":0.0,"y":0.0},
        {"type":"cubicTo","c1x":width / 3.0,"c1y":-12.0,
         "c2x":width * 0.75,"c2y":35.0,"x":width,"y":20.0},
        {"type":"lineTo","x":5.0,"y":40.0},
        {"type":"close"}
    ]}))
    .unwrap()
}

fn input() -> Value {
    let mut value = imported();
    let mut shape = rect(&value, 400);
    shape["type"] = json!("Shape");
    shape.as_object_mut().unwrap().remove("rect");
    shape["transform"] = json!(identity_fx_transform());
    shape["shape"] = json!({
        "path":path(80.0),
        "fills":[{"paint":{"type":"solid","color":[0.8,0.2,0.4,1.0]}}]
    });
    value["composition"]["layers"] = json!([shape, rect(&value, 499)]);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(400), PropType::ShapePath, [
            (0, PropertyValue::Path(path(80.0))),
            (500, PropertyValue::Path(path(140.0)))
        ])
    ]});
    value
}

fn path_storage(chunks: &[Chunk]) -> Option<&[Chunk]> {
    named_storage(chunks, "ADBE Vector Shape")
}

fn named_storage<'a>(chunks: &'a [Chunk], target: &str) -> Option<&'a [Chunk]> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                return Some(properties::unique_list(run, *b"om-s").unwrap());
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| named_storage(children, target))
}

#[test]
fn native_mask_path_child_guide_exports_all_five_keys_without_omission() {
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/path-keys-proof/mask-v2.aep"
    ))
    .unwrap();
    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    let output = to_aep(&imported.document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("Mask 1 omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let storage = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .find_map(|layer| named_storage(&layer.content, "ADBE Mask Shape"))
        .unwrap();
    let metadata =
        properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
            .unwrap();
    assert_eq!(metadata.keyframes.len(), 5);
    assert_eq!(metadata.keyframes[4].time_secs, 4.5);
}

fn delayed_masked_title_input() -> Value {
    let mut value = input();
    value["duration"] = json!(3.0);
    let mut guide = value["composition"]["layers"][0].clone();
    guide["parent"] = Value::Null;
    guide["activeRange"] = json!({"start":0,"duration":2000});
    let mut paint = value["composition"]["layers"][1].clone();
    paint["parent"] = Value::Null;
    paint["activeRange"] = json!({"start":0,"duration":2000});
    let title = json!({
        "type":"Text","id":401,"name":"S02 editable delayed title","parent":null,
        "activeRange":{"start":0,"duration":2000},"transform":identity_fx_transform(),
        "sourceText":{"text":"AFTER EFFECTS","fontFamily":"Inter-Regular", "fontSize":42.0,
            "applyFill":true,"fillColor":[1.0,1.0,1.0,1.0]}
    });
    value["composition"]["layers"] = json!([{
        "type":"Group","id":600,"name":"S02 delayed masked scene","parent":null,
        "transform":identity_fx_transform(),
        "playback":fixture_linear_playback(
            json!({"start":500,"duration":2000}),json!({"start":0,"duration":2000})),
        "layers":[title,paint,guide],
        "masks":[{"id":9001,"mode":"add","layer":400,"inverted":false,
            "feather":[0.0,0.0],"expansion":0.0,"opacity":1.0}]
    }]);
    value
}

#[test]
fn delayed_root_native_path_mask_retains_editable_title_on_source_zero_clock() {
    for (explicit_parent, owner_motion) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let mut value = delayed_masked_title_input();
        if explicit_parent {
            value["composition"]["layers"][0]["layers"][2]["parent"] = json!(600);
        }
        if owner_motion {
            value["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(json!(keyed_entry(
                    LayerId::new(600),
                    PropType::PositionX,
                    [
                        (0, PropertyValue::Float(0.0)),
                        (500, PropertyValue::Float(10.0))
                    ],
                )));
        }
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = to_aep(&document).unwrap();
        for id in [600, 401, 499] {
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
                .any(|layer| layer.name.as_ref() == "S02 editable delayed title"
                    && layer.record.layer_type() == 3)
        );
        let owner = native_layers
            .iter()
            .find(|layer| layer.name.as_ref() == "S02 delayed masked scene")
            .unwrap();
        assert_eq!(owner.record.start_time(), Some(0.5));
        if owner_motion {
            // The certified mask is source-local: its owner's occurrence
            // motion survives independently instead of changing guide keys.
            let transforms = properties::read_transform(&owner.content).unwrap();
            assert!(
                transforms
                    .iter()
                    .filter_map(|property| property.numeric.as_ref().ok())
                    .any(|numeric| numeric.animated && numeric.keyframes.len() == 2)
            );
        }
        let storage = named_storage(&owner.content, "ADBE Mask Shape").unwrap();
        let metadata =
            properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
                .unwrap();
        assert_eq!(metadata.keyframes.len(), 2);
        assert_eq!(metadata.keyframes[0].time_secs, 0.0);
        assert_eq!(metadata.keyframes[1].time_secs, 0.5);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(600))
                    && diagnostic.message.contains("guide layer 400 was copied"))
        );
    }
}

#[test]
fn delayed_root_path_mask_viewport_rejects_unproven_clocks_parents_and_soft_masks() {
    let base = delayed_masked_title_input();
    for case in [
        "rate",
        "offset",
        "guide phase",
        "foreign parent",
        "feather",
        "subtract",
        "guide motion",
    ] {
        let mut value = base.clone();
        let owner = &mut value["composition"]["layers"][0];
        match case {
            "rate" => owner["playback"]["mapping"]["output"]["duration"] = json!(1000),
            "offset" => owner["playback"]["inputOffsetMs"] = json!(50),
            "guide phase" => owner["layers"][2]["activeRange"]["start"] = json!(100),
            "foreign parent" => owner["layers"][2]["parent"] = json!(499),
            "feather" => owner["masks"][0]["feather"] = json!([1.0, 0.0]),
            "subtract" => owner["masks"][0]["mode"] = json!("subtract"),
            "guide motion" => {
                let id = 400;
                value["composition"]["dynamics"]["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(keyed_entry(
                        LayerId::new(id),
                        PropType::PositionX,
                        [
                            (0, PropertyValue::Float(0.0)),
                            (500, PropertyValue::Float(10.0))
                        ]
                    )));
            }
            _ => unreachable!(),
        }
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let output = to_aep(&document).unwrap();
        assert!(
            output.omitted_layer_ids.contains(&LayerId::new(600)),
            "{case}: {:?}",
            output.diagnostics
        );
    }
}

fn sandy_compound_mask_input() -> Value {
    let circle: fx_schema::ShapePath = serde_json::from_value(json!({"commands":[
        {"type":"moveTo","x":10.0,"y":0.0},
        {"type":"cubicTo","c1x":10.0,"c1y":5.5,"c2x":5.5,"c2y":10.0,"x":0.0,"y":10.0},
        {"type":"cubicTo","c1x":-5.5,"c1y":10.0,"c2x":-10.0,"c2y":5.5,"x":-10.0,"y":0.0},
        {"type":"cubicTo","c1x":-10.0,"c1y":-5.5,"c2x":-5.5,"c2y":-10.0,"x":0.0,"y":-10.0},
        {"type":"cubicTo","c1x":5.5,"c1y":-10.0,"c2x":10.0,"c2y":-5.5,"x":10.0,"y":0.0},
        {"type":"close"}
    ]}))
    .unwrap();
    let rect: fx_schema::ShapePath = serde_json::from_value(json!({"commands":[
        {"type":"moveTo","x":20.0,"y":0.0},{"type":"lineTo","x":22.0,"y":0.0},
        {"type":"lineTo","x":22.0,"y":2.0},{"type":"lineTo","x":20.0,"y":2.0},{"type":"close"}
    ]}))
    .unwrap();
    let mut one_grain = circle.clone();
    one_grain.commands.extend(rect.commands.clone());
    let mut two_grains = one_grain.clone();
    two_grains.commands.extend(rect.commands);
    let mut entry = json!(keyed_entry(
        LayerId::new(400),
        PropType::ShapePath,
        [
            (0, PropertyValue::Path(circle.clone())),
            (500, PropertyValue::Path(one_grain))
        ]
    ));
    for (id, time, path) in [
        ("second grain", 1000, two_grains),
        ("grains gone", 1500, circle),
    ] {
        let mut key = entry["animator"]["keyframes"][1].clone();
        key["id"] = json!(id);
        key["layerTime"] = json!(time);
        key["value"] = json!(PropertyValue::Path(path));
        entry["animator"]["keyframes"]
            .as_array_mut()
            .unwrap()
            .push(key);
    }
    for key in entry["animator"]["keyframes"].as_array_mut().unwrap() {
        key["easing"] = json!({"type":"hold"});
    }
    let mut value = delayed_masked_title_input();
    value["composition"]["dynamics"] = json!({"entries":[entry]});
    value
}

#[test]
fn delayed_sandy_compound_mask_keeps_native_add_contours_births_and_editable_title() {
    let output = export(sandy_compound_mask_input());
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(600)),
        "{:?}",
        output.diagnostics
    );
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(401)));
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
            .any(|layer| layer.record.layer_type() == 3
                && layer.name.as_ref() == "S02 editable delayed title")
    );
    let owner = native_layers
        .iter()
        .find(|layer| layer.name.as_ref() == "S02 delayed masked scene")
        .unwrap();
    assert_eq!(owner.record.start_time(), Some(0.5));
    fn collect<'a>(chunks: &'a [Chunk], output: &mut Vec<&'a [Chunk]>) {
        if let Ok(runs) = properties::runs(chunks) {
            for (name, run) in runs {
                if name == "ADBE Mask Shape" {
                    output.push(properties::unique_list(run, *b"om-s").unwrap());
                    return;
                }
            }
        }
        for children in chunks.iter().filter_map(Chunk::children) {
            collect(children, output);
        }
    }
    let mut storage = Vec::new();
    collect(&owner.content, &mut storage);
    assert_eq!(
        storage.len(),
        3,
        "all circle/grain slots must remain native masks"
    );
    let disappearance = read_project(include_bytes!(
        "../../../tests/fixtures/path-keys-proof/hold-disappearance-v1.aep"
    ))
    .unwrap();
    let oracle = path_storage(&layers(&disappearance)[0].content).unwrap();
    let native_empty = &properties::unique_list(oracle, *b"omks").unwrap()[1];
    for (index, storage) in storage.iter().enumerate() {
        let metadata =
            properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
                .unwrap();
        assert_eq!(
            metadata
                .keyframes
                .iter()
                .map(|key| key.time_secs)
                .collect::<Vec<_>>(),
            [0.0, 0.5, 1.0, 1.5]
        );
        if index > 0 {
            let shapes = properties::unique_list(storage, *b"omks").unwrap();
            assert_eq!(
                &shapes[0], native_empty,
                "initial absence must use Adobe's native empty record"
            );
            assert_eq!(
                &shapes[3], native_empty,
                "held death must use Adobe's native empty record"
            );
        }
    }
}

#[test]
fn sandy_compound_mask_does_not_flatten_holes_or_nonheld_births_to_add_masks() {
    for case in [
        "counterclockwise grain",
        "nonheld birth",
        "arbitrary curved contour",
        "feather",
    ] {
        let mut value = sandy_compound_mask_input();
        let entry = &mut value["composition"]["dynamics"]["entries"][0];
        match case {
            "counterclockwise grain" => {
                let commands = entry["animator"]["keyframes"][1]["value"]["value"]["commands"]
                    .as_array_mut()
                    .unwrap();
                // Reversing this rectangle creates a negative winding region,
                // which cannot be interpreted as independent positive Add masks.
                let a = commands[7].clone();
                commands[7] = commands[9].clone();
                commands[9] = a;
            }
            "nonheld birth" => {
                entry["animator"]["keyframes"][1]["easing"] = json!({"type":"linear"})
            }
            "arbitrary curved contour" => {
                entry["animator"]["keyframes"][1]["value"]["value"]["commands"][1]["c1x"] =
                    json!(500.0)
            }
            _ => value["composition"]["layers"][0]["masks"][0]["feather"] = json!([1.0, 0.0]),
        }
        let output = export(value);
        assert!(
            output.omitted_layer_ids.contains(&LayerId::new(600)),
            "{case}: {:?}",
            output.diagnostics
        );
    }
}

fn masked_input() -> Value {
    let mut value = input();
    let mut guide = value["composition"]["layers"][0].clone();
    guide["isHidden"] = json!(true);
    guide["transform"]["position"] = json!([13.0, 7.0]);
    let mut owner = value["composition"]["layers"][1].clone();
    owner["transform"] = json!(identity_fx_transform());
    owner["masks"] = json!([{"id":9001,"mode":"add","layer":400,
        "inverted":false,"feather":[0.0,0.0],"expansion":0.0,"opacity":1.0}]);
    value["composition"]["layers"] = json!([owner, guide]);
    value
}

fn mask_path_key_count(output: &ExportedDocument) -> usize {
    let native = read_project(&output.bytes).unwrap();
    let storage = named_storage(&layers(&native)[0].content, "ADBE Mask Shape")
        .unwrap_or_else(|| panic!("{:?}", output.diagnostics));
    properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
        .unwrap()
        .keyframes
        .len()
}

fn push_entry(
    value: &mut Value,
    id: LayerId,
    property: PropType,
    values: [(i64, PropertyValue); 2],
) {
    value["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!(keyed_entry(id, property, values)));
}

#[test]
fn path_keys_export_animated_mask_with_every_key_in_owner_coordinates() {
    let output = export(masked_input());
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let storage = named_storage(&layers(&native)[0].content, "ADBE Mask Shape")
        .unwrap_or_else(|| panic!("{:?}", output.diagnostics));
    let metadata =
        properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
            .unwrap();
    assert_eq!(metadata.keyframes.len(), 2);
    assert_eq!(metadata.keyframes[1].time_secs, 0.5);
    let converted = to_structural_fx_document(&native, Some(1)).unwrap();
    let entry = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|target| target.property_type() == PropType::ShapePath)
        })
        .unwrap();
    let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
        panic!("missing mask Path keys")
    };
    for (key, width) in track.keyframes().iter().zip([80.0, 140.0]) {
        let PropertyValue::Path(path) = key.value() else {
            panic!("not Path")
        };
        let fx_schema::ShapePathCommand::MoveTo { x, y, .. } = path.commands[0] else {
            panic!("not MoveTo")
        };
        assert!(
            (x - 13.0).abs() < 0.001 && (y - 7.0).abs() < 0.001,
            "{x},{y}"
        );
        let fx_schema::ShapePathCommand::CubicTo { x, y, .. } = path.commands[1] else {
            panic!("not cubic")
        };
        assert!((x - (13.0 + width)).abs() < 0.001 && (y - 27.0).abs() < 0.001);
    }
}

#[test]
fn path_keys_animated_mask_does_not_require_a_stale_guide_outline() {
    let mut value = masked_input();
    value["composition"]["layers"][1]["shape"]["path"]["commands"] = json!([]);
    let output = export(value);
    assert_eq!(mask_path_key_count(&output), 2);
}

#[test]
fn path_keys_animated_mask_allows_owner_opacity_animation() {
    let mut value = masked_input();
    push_entry(
        &mut value,
        LayerId::new(499),
        PropType::Opacity,
        [
            (0, PropertyValue::Float(100.0)),
            (500, PropertyValue::Float(50.0)),
        ],
    );
    let output = export(value);
    assert_eq!(mask_path_key_count(&output), 2);
}

#[test]
fn path_keys_animated_mask_allows_guide_paint_animation() {
    let mut value = masked_input();
    push_entry(
        &mut value,
        LayerId::new(400),
        PropType::FillColor,
        [
            (0, PropertyValue::Color([0.8, 0.2, 0.4, 1.0])),
            (500, PropertyValue::Color([0.2, 0.4, 0.8, 1.0])),
        ],
    );
    let output = export(value);
    assert_eq!(mask_path_key_count(&output), 2);
}

#[test]
fn path_keys_animated_mask_rejects_owner_and_guide_transform_animation() {
    for (id, values) in [
        (
            LayerId::new(499),
            [
                (0, PropertyValue::Float(0.0)),
                (500, PropertyValue::Float(20.0)),
            ],
        ),
        (
            LayerId::new(400),
            [
                (0, PropertyValue::Float(13.0)),
                (500, PropertyValue::Float(23.0)),
            ],
        ),
    ] {
        let mut value = masked_input();
        push_entry(&mut value, id, PropType::PositionX, values);
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        assert!(
            named_storage(&layers(&native)[0].content, "ADBE Mask Shape").is_none(),
            "{id:?}: {:?}",
            output.diagnostics
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("animation")),
            "{id:?}: {:?}",
            output.diagnostics
        );
    }
}

#[test]
fn path_keys_mask_clock_mismatch_is_diagnosed_without_losing_owner() {
    let mut value = masked_input();
    value["composition"]["layers"][1]["activeRange"]["start"] = json!(100);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert!(!layers(&native).is_empty());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("clocks")),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn path_keys_export_fresh_editable_shape_records_and_preserve_sibling() {
    let output = export(input());
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let storage = path_storage(&layers(&native)[0].content).unwrap();
    let metadata =
        properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
            .unwrap();
    assert!(metadata.animated);
    assert_eq!(metadata.keyframes.len(), 2);
    assert_eq!(metadata.keyframes[0].time_secs, 0.0);
    assert_eq!(metadata.keyframes[1].time_secs, 0.5);
    let shapes = properties::unique_list(storage, *b"omks").unwrap();
    assert_eq!(
        shapes
            .iter()
            .filter(|chunk| chunk.list_kind() == Some(*b"shap"))
            .count(),
        2
    );
    let widths: Vec<_> = shapes
        .iter()
        .filter_map(Chunk::children)
        .map(|shape| {
            let bounds = properties::data(shape, *b"shph").unwrap();
            f32::from_be_bytes(bounds[12..16].try_into().unwrap())
        })
        .collect();
    assert_eq!(widths, [80.0, 140.0]);
}

#[test]
fn path_keys_export_does_not_require_a_stale_base_outline() {
    let mut value = input();
    value["composition"]["layers"][0]["shape"]["path"]["commands"] = json!([]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert!(path_storage(&layers(&native)[0].content).is_some());
}

#[test]
fn path_keys_disabled_export_uses_current_disabled_outline() {
    let mut value = input();
    let animator = &mut value["composition"]["dynamics"]["entries"][0]["animator"];
    animator["enabled"] = json!(false);
    animator["disabledValue"] = json!(PropertyValue::Path(path(35.0)));
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let storage = path_storage(&layers(&native)[0].content).unwrap();
    let shapes = properties::unique_list(storage, *b"omks").unwrap();
    assert_eq!(shapes.len(), 1);
    let bounds = properties::data(shapes[0].children().unwrap(), *b"shph").unwrap();
    assert_eq!(f32::from_be_bytes(bounds[12..16].try_into().unwrap()), 35.0);
}

#[test]
fn path_keys_stable_multicontour_exports_separate_editable_tracks() {
    let mut value = input();
    let mut first = path(80.0);
    first.commands.extend(path(20.0).commands);
    let mut second = path(140.0);
    second.commands.extend(path(35.0).commands);
    value["composition"]["layers"][0]["shape"]["path"] = json!(first.clone());
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(400), PropType::ShapePath, [
            (0, PropertyValue::Path(first)),
            (500, PropertyValue::Path(second))
        ])
    ]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let mut tracks = Vec::new();
    fn collect(chunks: &[Chunk], tracks: &mut Vec<Vec<f32>>) {
        if let Ok(runs) = properties::runs(chunks) {
            for (name, run) in runs {
                if name == "ADBE Vector Shape" {
                    let storage = properties::unique_list(run, *b"om-s").unwrap();
                    let metadata = properties::read_path_metadata(
                        properties::unique_list(storage, *b"tdbs").unwrap(),
                    )
                    .unwrap();
                    assert_eq!(metadata.keyframes.len(), 2);
                    let shapes = properties::unique_list(storage, *b"omks").unwrap();
                    tracks.push(
                        shapes
                            .iter()
                            .map(|shape| {
                                let bounds =
                                    properties::data(shape.children().unwrap(), *b"shph").unwrap();
                                f32::from_be_bytes(bounds[12..16].try_into().unwrap())
                            })
                            .collect(),
                    );
                }
            }
        }
        for chunk in chunks {
            if let Some(children) = chunk.children() {
                collect(children, tracks);
            }
        }
    }
    collect(&layers(&native)[0].content, &mut tracks);
    assert_eq!(tracks, [[80.0, 140.0], [20.0, 35.0]]);
}

#[test]
fn native_held_disappearance_imports_editable_path_controls() {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("../../../tests/fixtures/path-keys-proof/hold-disappearance-v1.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "c4746691f91a5f771d12ee9e6435f83e6b7ae4ae4c9100f1b32ed04c82ca921c"
    );
    let source = read_project(bytes).unwrap();
    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    let track = imported
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter_map(|entry| entry.animator.keyframe_track())
        .find(|track| {
            track.keyframes().len() == 4
                && matches!(track.keyframes()[0].value(), PropertyValue::Path(_))
        })
        .expect("native disappearing Path must stay editable");
    assert_eq!(
        track
            .keyframes()
            .iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        [0, 500, 1000, 1500]
    );
    for (index, point) in [(1, [0.0, 0.0]), (3, [160.0, 90.0])] {
        let key = &track.keyframes()[index];
        assert_eq!(key.easing(), PropertyKeyframeEasing::Hold);
        let PropertyValue::Path(path) = key.value() else {
            panic!("Path required")
        };
        assert!(
            matches!(path.commands.as_slice(), [fx_schema::ShapePathCommand::MoveTo {x,y,..}] if [*x,*y] == point)
        );
    }
}

#[test]
fn path_keys_held_disappearance_uses_adobe_native_one_vertex_record() {
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/path-keys-proof/hold-disappearance-v1.aep"
    ))
    .unwrap();
    let storage = path_storage(&layers(&source)[0].content).unwrap();
    let native_shapes = properties::unique_list(storage, *b"omks").unwrap();
    let native_empty = &native_shapes[1];
    let mut value = input();
    let empty = fx_schema::ShapePath { commands: vec![] };
    let mut compound = path(140.0);
    compound.commands.extend(path(35.0).commands);
    let mut entry = json!(keyed_entry(
        LayerId::new(400),
        PropType::ShapePath,
        [
            (0, PropertyValue::Path(empty)),
            (500, PropertyValue::Path(compound)),
        ]
    ));
    let mut last = entry["animator"]["keyframes"][0].clone();
    last["id"] = json!("disappeared");
    last["layerTime"] = json!(1000);
    entry["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
        .push(last);
    for key in entry["animator"]["keyframes"].as_array_mut().unwrap() {
        key["easing"] = json!({"type":"hold"});
    }
    value["composition"]["dynamics"] = json!({"entries":[entry]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let storage = path_storage(&layers(&native)[0].content).unwrap();
    let shapes = properties::unique_list(storage, *b"omks").unwrap();
    assert_eq!(shapes.len(), 3);
    for index in [0, 2] {
        assert_eq!(
            shapes[index], *native_empty,
            "empty Path geometry must match Adobe's own record"
        );
    }
}

#[test]
fn path_keys_static_point_keeps_a_zero_opacity_native_group() {
    let mut value = input();
    value["composition"]["layers"][0]["shape"]["path"] =
        json!({"commands":[{"type":"moveTo","x":0,"y":0}]});
    value["composition"]["layers"][0]["shape"]["strokes"] = json!([{
        "paint":{"type":"solid","color":[1,0,0,1]}, "width":16, "cap":"round"
    }]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    fn has_zero_opacity(chunks: &[Chunk]) -> bool {
        if let Ok(runs) = properties::runs(chunks) {
            for (name, run) in runs {
                if name == "ADBE Vector Group Opacity" {
                    let storage = properties::unique_list(run, *b"tdbs").unwrap();
                    let opacity = properties::read_numeric(storage).unwrap();
                    if (!opacity.animated && opacity.values == [0.0])
                        || (opacity.keyframes.len() == 1 && opacity.keyframes[0].values == [0.0])
                    {
                        return true;
                    }
                }
            }
        }
        chunks
            .iter()
            .filter_map(Chunk::children)
            .any(has_zero_opacity)
    }
    assert!(has_zero_opacity(&layers(&native)[0].content));
}

#[test]
fn path_keys_stroked_partial_disappearance_is_diagnosed_without_losing_sibling() {
    let mut value = input();
    value["composition"]["layers"][0]["shape"]["strokes"] = json!([{
        "paint":{"type":"solid","color":[1,0,0,0.5]}, "width":16, "cap":"round"
    }]);
    let mut compound = path(80.0);
    compound.commands.extend(path(20.0).commands);
    let mut entry = json!(keyed_entry(
        LayerId::new(400),
        PropType::ShapePath,
        [
            (0, PropertyValue::Path(compound)),
            (500, PropertyValue::Path(path(140.0))),
        ]
    ));
    entry["animator"]["keyframes"][1]["easing"] = json!({"type":"hold"});
    value["composition"]["dynamics"] = json!({"entries":[entry]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|entry| entry.message.contains("partial disappearance"))
    );
}

// Source-derived FX discriminator for the launch trail/orbit disappearance.
// Open contours and static round-cap paint expose the absent-slot dot risk;
// this is supplementary native-record evidence, not an Adobe-authored oracle.
fn partial_stroke_input() -> Value {
    let mut value = input();
    value["composition"]["layers"][0]["shape"]["fills"] = json!([]);
    value["composition"]["layers"][0]["shape"]["strokes"] = json!([{
        "paint":{"type":"solid","color":[1,0,0,1]}, "width":16, "cap":"round"
    }]);
    let open = |width| {
        let mut value = path(width);
        value.commands.pop();
        value
    };
    let mut compound = open(80.0);
    compound.commands.extend(open(20.0).commands);
    value["composition"]["layers"][0]["shape"]["path"] = json!(compound.clone());
    let mut entry = json!(keyed_entry(
        LayerId::new(400),
        PropType::ShapePath,
        [
            (0, PropertyValue::Path(compound)),
            (500, PropertyValue::Path(open(140.0))),
        ]
    ));
    for key in entry["animator"]["keyframes"].as_array_mut().unwrap() {
        key["easing"] = json!({"type":"hold"});
    }
    value["composition"]["dynamics"] = json!({"entries":[entry]});
    value
}

fn collect_named_runs<'a>(chunks: &'a [Chunk], target: &str, result: &mut Vec<&'a [Chunk]>) {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                result.push(run);
            }
        }
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        collect_named_runs(children, target, result);
    }
}

#[test]
fn path_keys_partial_stroke_preserves_shared_compound_fill_and_fill_keys() {
    let mut value = input();
    value["composition"]["layers"][0]["shape"]["fills"][0]["fillRule"] = json!("evenOdd");
    value["composition"]["layers"][0]["shape"]["strokes"] = json!([{
        "paint":{"type":"solid","color":[1,0,0,1]}, "width":16, "cap":"round"
    }]);
    let mut compound = path(80.0);
    compound.commands.extend(path(20.0).commands);
    value["composition"]["layers"][0]["shape"]["path"] = json!(compound.clone());
    let mut entry = json!(keyed_entry(
        LayerId::new(400),
        PropType::ShapePath,
        [
            (0, PropertyValue::Path(compound)),
            (500, PropertyValue::Path(path(140.0))),
        ],
    ));
    for key in entry["animator"]["keyframes"].as_array_mut().unwrap() {
        key["easing"] = json!({"type":"hold"});
    }
    value["composition"]["dynamics"] = json!({"entries":[entry]});
    push_entry(
        &mut value,
        LayerId::new(400),
        PropType::FillColor,
        [
            (0, PropertyValue::Color([0.8, 0.2, 0.4, 1.0])),
            (500, PropertyValue::Color([0.2, 0.4, 0.8, 1.0])),
        ],
    );
    let mut fill_only = value.clone();
    fill_only["composition"]["layers"][0]["shape"]["strokes"] = json!([]);
    let reference = read_project(&export(fill_only).bytes).unwrap();
    let mut reference_paths = Vec::new();
    collect_named_runs(
        &layers(&reference)[0].content,
        "ADBE Vector Shape",
        &mut reference_paths,
    );

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let mut groups = Vec::new();
    collect_named_runs(
        &layers(&native)[0].content,
        "ADBE Vector Group",
        &mut groups,
    );
    let fill_groups: Vec<_> = groups
        .iter()
        .filter(|group| {
            let mut fills = Vec::new();
            let mut strokes = Vec::new();
            collect_named_runs(group, "ADBE Vector Graphic - Fill", &mut fills);
            collect_named_runs(group, "ADBE Vector Graphic - Stroke", &mut strokes);
            fills.len() == 1 && strokes.is_empty()
        })
        .collect();
    assert_eq!(
        fill_groups.len(),
        1,
        "Fill must not be copied into stroke slots"
    );
    let mut paths = Vec::new();
    collect_named_runs(fill_groups[0], "ADBE Vector Shape", &mut paths);
    assert_eq!(paths.len(), 2);
    assert_eq!(reference_paths.len(), 2);
    for (actual, expected) in paths.iter().zip(reference_paths) {
        let actual = properties::unique_list(actual, *b"om-s").unwrap();
        let expected = properties::unique_list(expected, *b"om-s").unwrap();
        assert_eq!(
            properties::unique_list(actual, *b"omks").unwrap(),
            properties::unique_list(expected, *b"omks").unwrap(),
            "compound Fill geometry/winding must equal the fill-only export"
        );
        let metadata =
            properties::read_path_metadata(properties::unique_list(actual, *b"tdbs").unwrap())
                .unwrap();
        assert_eq!(metadata.keyframes.len(), 2);
        assert_eq!(metadata.keyframes[1].time_secs, 0.5);
    }
    let mut colors = Vec::new();
    collect_named_runs(fill_groups[0], "ADBE Vector Fill Color", &mut colors);
    assert_eq!(colors.len(), 1);
    let color =
        properties::read_numeric(properties::unique_list(colors[0], *b"tdbs").unwrap()).unwrap();
    assert_eq!(color.keyframes.len(), 2);
    assert_eq!(color.keyframes[1].time_secs, 0.5);
    let mut strokes = Vec::new();
    collect_named_runs(
        &layers(&native)[0].content,
        "ADBE Vector Graphic - Stroke",
        &mut strokes,
    );
    assert_eq!(strokes.len(), 2);
    let fill_index = groups
        .iter()
        .position(|group| group == fill_groups[0])
        .unwrap();
    let stroke_index = groups
        .iter()
        .position(|group| {
            let mut strokes = Vec::new();
            let mut fills = Vec::new();
            collect_named_runs(group, "ADBE Vector Graphic - Stroke", &mut strokes);
            collect_named_runs(group, "ADBE Vector Graphic - Fill", &mut fills);
            strokes.len() == 1 && fills.is_empty()
        })
        .unwrap();
    assert!(
        fill_index < stroke_index,
        "Fill must precede the Stroke slots"
    );
    let mut rule = Vec::new();
    collect_named_runs(fill_groups[0], "ADBE Vector Fill Rule", &mut rule);
    assert_eq!(rule.len(), 1);
    assert_eq!(
        properties::read_numeric(properties::unique_list(rule[0], *b"tdbs").unwrap())
            .unwrap()
            .values,
        [2.0]
    );
    let mut opacities = Vec::new();
    collect_named_runs(
        &layers(&native)[0].content,
        "ADBE Vector Group Opacity",
        &mut opacities,
    );
    assert_eq!(
        opacities
            .iter()
            .filter(|run| {
                let opacity =
                    properties::read_numeric(properties::unique_list(run, *b"tdbs").unwrap())
                        .unwrap();
                opacity.keyframes.len() == 2
                    && opacity.keyframes[0].time_secs == 0.0
                    && opacity.keyframes[0].values == [100.0]
                    && opacity.keyframes[1].time_secs == 0.5
                    && opacity.keyframes[1].values == [0.0]
                    && opacity.keyframes[0].out_interpolation == 3
                    && opacity.keyframes[1].in_interpolation == 3
            })
            .count(),
        1
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|entry| entry.message.contains("per-contour native Stroke controls"))
    );
}

#[test]
fn path_keys_butt_partial_contours_keep_shared_gradient_and_dashed_stroke_controls() {
    for paint in [
        json!({"paint":{"type":"solid","color":[0.4,0.6,0.8,0.5]},"dashes":[5,9]}),
        json!({"paint":{"type":"gradient","gradientType":"linear","start":[0,0],"end":[100,0],"stops":[{"offset":0,"color":[1,0,0,0.2]},{"offset":1,"color":[0,0,1,0.7]}]}}),
    ] {
        let mut value = partial_stroke_input();
        let stroke = &mut value["composition"]["layers"][0]["shape"]["strokes"][0];
        stroke["cap"] = json!("butt");
        stroke["join"] = json!("bevel");
        stroke["opacity"] = json!(0.6);
        for (key, replacement) in paint.as_object().unwrap() {
            stroke[key] = replacement.clone();
        }
        let mut stable = value.clone();
        stable["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1]["value"] =
            stable["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][0]["value"]
                .clone();
        let reference = read_project(&export(stable).bytes).unwrap();
        let output = export(value.clone());
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
        assert!(!output.diagnostics.iter().any(|entry| {
            entry.layer_id == Some(LayerId::new(400))
                && (entry.message.contains("omitted")
                    || entry.message.contains("per-contour native Stroke controls"))
        }));
        let mut shared_stroke_count = 0;
        for match_name in [
            "ADBE Vector Graphic - Stroke",
            "ADBE Vector Graphic - G-Stroke",
        ] {
            let mut actual = Vec::new();
            let mut expected = Vec::new();
            collect_named_runs(&layers(&native)[0].content, match_name, &mut actual);
            collect_named_runs(&layers(&reference)[0].content, match_name, &mut expected);
            shared_stroke_count += actual.len();
            assert_eq!(
                actual, expected,
                "shared Stroke controls must stay unchanged"
            );
        }
        assert_eq!(
            shared_stroke_count, 1,
            "paint must not be copied per contour"
        );
        let mut paths = Vec::new();
        collect_named_runs(&layers(&native)[0].content, "ADBE Vector Shape", &mut paths);
        assert_eq!(paths.len(), 2);
        for run in &paths {
            let storage = properties::unique_list(run, *b"om-s").unwrap();
            let keys =
                properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
                    .unwrap()
                    .keyframes;
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[1].time_secs, 0.5);
            assert_eq!(keys[1].in_interpolation, 3);
        }
        let mut edited = value;
        edited["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1]["value"] =
            json!(PropertyValue::Path(path(200.0)));
        let edited = read_project(&export(edited).bytes).unwrap();
        let mut edited_paths = Vec::new();
        collect_named_runs(
            &layers(&edited)[0].content,
            "ADBE Vector Shape",
            &mut edited_paths,
        );
        assert_ne!(
            paths, edited_paths,
            "fresh FX geometry edits must affect native Path keys"
        );
    }
}

#[test]
fn path_keys_butt_partial_contours_do_not_bypass_geometry_modifier_or_other_cap_guards() {
    for mutation in [
        json!({"trim":{"start":0,"end":50,"offset":0}}),
        json!({"offsetPaths":{"amount":4}}),
        json!({"roundCorners":{"radius":8}}),
    ] {
        let mut value = partial_stroke_input();
        value["composition"]["layers"][0]["shape"]["strokes"][0]["cap"] = json!("butt");
        value["composition"]["layers"][0]["shape"]["strokes"][0]["opacity"] = json!(0.5);
        for (key, replacement) in mutation.as_object().unwrap() {
            value["composition"]["layers"][0]["shape"][key] = replacement.clone();
        }
        let output = export(value);
        assert_eq!(layers(&read_project(&output.bytes).unwrap()).len(), 1);
    }
    for cap in ["round", "square"] {
        let mut value = partial_stroke_input();
        value["composition"]["layers"][0]["shape"]["strokes"][0]["cap"] = json!(cap);
        value["composition"]["layers"][0]["shape"]["strokes"][0]["opacity"] = json!(0.5);
        let output = export(value);
        assert_eq!(layers(&read_project(&output.bytes).unwrap()).len(), 1);
    }
}

#[test]
fn path_keys_opaque_partial_stroke_retains_two_painted_slots_and_hold_visibility() {
    let output = export(partial_stroke_input());
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert!(!output.diagnostics.iter().any(
        |entry| entry.layer_id == Some(LayerId::new(400)) && entry.message.contains("omitted")
    ));
    assert!(output.diagnostics.iter().any(|entry| {
        entry.message.contains("per-contour native Stroke controls")
            && entry.message.contains("overlap/AA")
    }));
    let mut groups = Vec::new();
    collect_named_runs(
        &layers(&native)[0].content,
        "ADBE Vector Group",
        &mut groups,
    );
    let slots: Vec<_> = groups
        .into_iter()
        .filter(|group| {
            let mut paths = Vec::new();
            let mut strokes = Vec::new();
            collect_named_runs(group, "ADBE Vector Shape", &mut paths);
            collect_named_runs(group, "ADBE Vector Graphic - Stroke", &mut strokes);
            paths.len() == 1 && strokes.len() == 1
        })
        .collect();
    assert_eq!(slots.len(), 2);
    let mut disappearing = 0;
    for slot in slots {
        let storage = path_storage(slot).unwrap();
        let metadata =
            properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
                .unwrap();
        assert_eq!(metadata.keyframes.len(), 2);
        assert_eq!(metadata.keyframes[1].time_secs, 0.5);
        let mut opacity = Vec::new();
        collect_named_runs(slot, "ADBE Vector Group Opacity", &mut opacity);
        for run in opacity {
            let keys = properties::read_numeric(properties::unique_list(run, *b"tdbs").unwrap())
                .unwrap()
                .keyframes;
            if keys.len() == 2 {
                assert_eq!(keys[0].time_secs, 0.0);
                assert_eq!(keys[0].values, [100.0]);
                assert_eq!(keys[1].time_secs, 0.5);
                assert_eq!(keys[1].values, [0.0]);
                assert_eq!(keys[0].out_interpolation, 3);
                assert_eq!(keys[1].in_interpolation, 3);
                disappearing += 1;
            }
        }
    }
    assert_eq!(disappearing, 1);
    assert!(path_storage(&layers(&native)[1].content).is_none());
}

#[test]
fn path_keys_partial_stroke_unsupported_controls_keep_omission() {
    for mutation in [
        json!({"paint":{"type":"solid","color":[1,0,0,0.5]}}),
        json!({"opacity":0.5}),
        json!({"blendMode":"multiply"}),
        json!({"dashes":[8,4]}),
        json!({"paint":{"type":"gradient","gradientType":"linear", "start":[0,0], "end":[100,0], "stops":[{"offset":0,"color":[1,0,0,1]},{"offset":1,"color":[0,0,1,1]}]}}),
    ] {
        let mut value = partial_stroke_input();
        for (key, replacement) in mutation.as_object().unwrap() {
            value["composition"]["layers"][0]["shape"]["strokes"][0][key] = replacement.clone();
        }
        let output = export(value);
        assert_eq!(
            layers(&read_project(&output.bytes).unwrap()).len(),
            1,
            "{:?}",
            output.diagnostics
        );
    }
    let mut multiple = partial_stroke_input();
    let stroke = multiple["composition"]["layers"][0]["shape"]["strokes"][0].clone();
    multiple["composition"]["layers"][0]["shape"]["strokes"]
        .as_array_mut()
        .unwrap()
        .push(stroke);
    let mut modifier = partial_stroke_input();
    modifier["composition"]["layers"][0]["shape"]["roundCorners"] = json!({"radius":8});
    let mut keyed = partial_stroke_input();
    let entry = keyed_entry(
        LayerId::new(400),
        PropType::StrokeWidth,
        [
            (0, PropertyValue::Float(16.0)),
            (500, PropertyValue::Float(20.0)),
        ],
    );
    keyed["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(json!(entry));
    let mut blended_fill = partial_stroke_input();
    blended_fill["composition"]["layers"][0]["shape"]["fills"] = json!([{
        "paint":{"type":"solid","color":[1,1,1,1]}, "blendMode":"screen"
    }]);
    for value in [multiple, modifier, keyed, blended_fill] {
        let output = export(value);
        assert_eq!(
            layers(&read_project(&output.bytes).unwrap()).len(),
            1,
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn path_keys_stroke_all_empty_remains_hidden_without_partial_approximation() {
    let mut value = partial_stroke_input();
    for key in value["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
    {
        key["value"] = json!(PropertyValue::Path(fx_schema::ShapePath {
            commands: vec![]
        }));
    }
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|entry| entry.message.contains("per-contour"))
    );
    let mut opacity = Vec::new();
    collect_named_runs(
        &layers(&native)[0].content,
        "ADBE Vector Group Opacity",
        &mut opacity,
    );
    assert!(opacity.iter().any(|run| {
        let value =
            properties::read_numeric(properties::unique_list(run, *b"tdbs").unwrap()).unwrap();
        value.values == [0.0] || value.keyframes.iter().any(|key| key.values == [0.0])
    }));
}

#[test]
fn path_keys_unsupported_multicontour_preserves_exportable_sibling() {
    let mut value = input();
    let mut second = path(140.0);
    second.commands.extend(path(20.0).commands);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(400), PropType::ShapePath, [
            (0, PropertyValue::Path(path(80.0))),
            (500, PropertyValue::Path(second))
        ])
    ]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Path"))
    );
}
