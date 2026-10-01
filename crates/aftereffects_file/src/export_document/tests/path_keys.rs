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
        "paint":{"type":"solid","color":[1,0,0,1]}, "width":16, "cap":"round"
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
