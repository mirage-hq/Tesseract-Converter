//! Explicit edited-FX export regressions. Native-record readback is supplemental
//! structural evidence, not independent Adobe acceptance or render fidelity.

use super::*;
use crate::{properties, rifx::Chunk};

fn numeric(chunks: &[Chunk], target: &str) -> Option<properties::NumericProperty> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                let storage = properties::unique_list(run, *b"tdbs").unwrap();
                return Some(properties::read_numeric(storage).unwrap());
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| numeric(children, target))
}

fn assert_keys(chunks: &[Chunk], target: &str, values: &[&[f64]]) {
    let property = numeric(chunks, target).unwrap_or_else(|| panic!("missing {target}"));
    assert_eq!(property.keyframes.len(), values.len(), "{target}");
    for (index, (key, expected)) in property.keyframes.iter().zip(values).enumerate() {
        assert_eq!(key.values, *expected, "{target}");
        assert_eq!(key.time_secs, index as f64 * 0.5, "{target}");
    }
}

fn operand(value: &Value, geometry: &str) -> Value {
    let mut child = rect(value, 401);
    child["transform"] = json!(identity_fx_transform());
    child["rect"]["position"] = json!([10.0, -20.0]);
    child["rect"]["size"] = json!([80.0, 40.0]);
    child["rect"]["roundness"] = json!(2.0);
    if geometry != "rect" {
        child["type"] = json!("Shape");
        child.as_object_mut().unwrap().remove("rect");
        child["shape"] = match geometry {
            "ellipse" => {
                json!({"path":{"commands":[]},"ellipse":{"size":[80.0,40.0],"position":[3.0,-4.0]}})
            }
            "star" | "polygon" => json!({"path":{"commands":[]},"polyStar":{
                "starType":geometry,"points":5.0,"position":[3.0,-4.0],
                "rotation":12.0,"outerRadius":50.0,"innerRadius":20.0,
                "outerRoundness":6.0,"innerRoundness":4.0
            }}),
            "path" => json!({"path":{"commands":[
                {"type":"moveTo","x":0.0,"y":0.0},
                {"type":"lineTo","x":80.0,"y":0.0},
                {"type":"lineTo","x":40.0,"y":60.0},
                {"type":"close"}
            ]}}),
            _ => panic!("unknown test geometry"),
        };
    }
    child
}

fn boolean(id: u64, mut children: Vec<Value>, op: &str) -> Value {
    for child in &mut children {
        child["parent"] = json!(id);
    }
    json!({
        "type":"BooleanOperation", "id":id, "name":format!("Boolean {id}"),
        "parent":null, "activeRange":children[0]["activeRange"],
        "transform":identity_fx_transform(), "op":op, "layers":children,
        "fills":[{"paint":{"type":"solid","color":[0.8,0.2,0.4,1.0]}}],
        "strokes":[]
    })
}

fn input(geometry: &str, nested: bool, op: &str) -> Value {
    let mut value = imported();
    let first = operand(&value, geometry);
    let mut second = operand(&value, "path");
    second["id"] = json!(402);
    let mut owner = boolean(400, vec![first, second], op);
    if nested {
        let mut third = operand(&value, "path");
        third["id"] = json!(404);
        owner = boolean(403, vec![owner, third], "union");
    }
    value["composition"]["layers"] = json!([owner, rect(&value, 499)]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    value
}

fn native(value: Value) -> (ExportedDocument, StructuralProject) {
    let output = export(value);
    let project = read_project(&output.bytes).unwrap();
    (output, project)
}

#[test]
fn boolean_geometry_rect_size_center_roundness_and_transform_keys() {
    for op in ["union", "subtract", "intersect", "exclude"] {
        for nested in [false, true] {
            let mut value = input("rect", nested, op);
            value["composition"]["dynamics"] = json!({"entries":[
                keyed_entry(LayerId::new(401), PropType::RectSize, [
                    (0, PropertyValue::Vector2([80.0,40.0])),
                    (500, PropertyValue::Vector2([140.0,60.0]))]),
                keyed_entry(LayerId::new(401), PropType::RectRoundness, [
                    (0, PropertyValue::Float(2.0)), (500, PropertyValue::Float(19.0))]),
                keyed_entry(LayerId::new(401), PropType::PositionX, [
                    (0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(30.0))]),
                keyed_entry(LayerId::new(401), PropType::Rotation, [
                    (0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(35.0))])
            ]});
            let (output, project) = native(value);
            assert_eq!(
                layers(&project).len(),
                2,
                "{op}/{nested}: {:?}",
                output.diagnostics
            );
            let content = &layers(&project)[0].content;
            assert_keys(
                content,
                "ADBE Vector Rect Size",
                &[&[80.0, 40.0], &[140.0, 60.0]],
            );
            assert_keys(
                content,
                "ADBE Vector Rect Position",
                &[&[50.0, 0.0], &[80.0, 10.0]],
            );
            assert_keys(content, "ADBE Vector Rect Roundness", &[&[2.0], &[19.0]]);
            // Find the operand transform rather than an unanimated enclosing group.
            assert!(has_key_values(
                content,
                "ADBE Vector Position",
                &[30.0, 0.0]
            ));
            assert!(has_key_values(content, "ADBE Vector Rotation", &[35.0]));
        }
    }
}

fn has_key_values(chunks: &[Chunk], target: &str, values: &[f64]) -> bool {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                let storage = properties::unique_list(run, *b"tdbs").unwrap();
                if properties::read_numeric(storage)
                    .unwrap()
                    .keyframes
                    .iter()
                    .any(|key| key.time_secs == 0.5 && key.values == values)
                {
                    return true;
                }
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .any(|children| has_key_values(children, target, values))
}

#[test]
fn boolean_geometry_ellipse_size_and_position_keys() {
    let mut value = input("ellipse", true, "subtract");
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(401), PropType::EllipseSize, [
            (0, PropertyValue::Vector2([80.0,40.0])),
            (500, PropertyValue::Vector2([120.0,70.0]))]),
        keyed_entry(LayerId::new(401), PropType::EllipsePosition, [
            (0, PropertyValue::Vector2([3.0,-4.0])),
            (500, PropertyValue::Vector2([13.0,6.0]))])
    ]});
    let (output, project) = native(value);
    assert_eq!(layers(&project).len(), 2, "{:?}", output.diagnostics);
    let content = &layers(&project)[0].content;
    assert_keys(
        content,
        "ADBE Vector Ellipse Size",
        &[&[80.0, 40.0], &[120.0, 70.0]],
    );
    assert_keys(
        content,
        "ADBE Vector Ellipse Position",
        &[&[3.0, -4.0], &[13.0, 6.0]],
    );
}

#[test]
fn boolean_geometry_star_and_polygon_parameters_keep_keys_and_hold_points() {
    for geometry in ["star", "polygon"] {
        let mut value = input(geometry, false, "union");
        let controls = [
            (
                PropType::PolyStarPoints,
                "ADBE Vector Star Points",
                5.0,
                8.0,
            ),
            (
                PropType::PolyStarRotation,
                "ADBE Vector Star Rotation",
                12.0,
                40.0,
            ),
            (
                PropType::PolyStarOuterRadius,
                "ADBE Vector Star Outer Radius",
                50.0,
                75.0,
            ),
            (
                PropType::PolyStarOuterRoundness,
                "ADBE Vector Star Outer Roundess",
                6.0,
                15.0,
            ),
            (
                PropType::PolyStarInnerRadius,
                "ADBE Vector Star Inner Radius",
                20.0,
                30.0,
            ),
            (
                PropType::PolyStarInnerRoundness,
                "ADBE Vector Star Inner Roundess",
                4.0,
                10.0,
            ),
        ];
        let mut entries = Vec::new();
        for (property, _, first, last) in controls {
            let mut entry = json!(keyed_entry(
                LayerId::new(401),
                property,
                [
                    (0, PropertyValue::Float(first)),
                    (500, PropertyValue::Float(last))
                ]
            ));
            if property == PropType::PolyStarPoints {
                entry["animator"]["keyframes"][1]["easing"] = json!({"type":"hold"});
            }
            entries.push(entry);
        }
        entries.push(json!(keyed_entry(
            LayerId::new(401),
            PropType::PolyStarPosition,
            [
                (0, PropertyValue::Vector2([3.0, -4.0])),
                (500, PropertyValue::Vector2([13.0, 6.0]))
            ]
        )));
        value["composition"]["dynamics"] = json!({"entries":entries});
        let (output, project) = native(value);
        assert_eq!(
            layers(&project).len(),
            2,
            "{geometry}: {:?}",
            output.diagnostics
        );
        let content = &layers(&project)[0].content;
        for (_, target, first, last) in controls {
            assert_keys(content, target, &[&[first], &[last]]);
        }
        assert_keys(
            content,
            "ADBE Vector Star Position",
            &[&[3.0, -4.0], &[13.0, 6.0]],
        );
        // Native Hold interpolation is encoded as 3, independently of the FX enum.
        let points = numeric(content, "ADBE Vector Star Points").unwrap();
        assert_eq!(points.keyframes[0].out_interpolation, 3);
        assert_eq!(points.keyframes[1].in_interpolation, 3);
    }
}

#[test]
fn boolean_geometry_fresh_native_import_then_edited_rect_exports_keys() {
    use sha2::{Digest, Sha256};

    // The pinned source proves static Path/Rect Boolean parsing, not native
    // parameter animation. The added keys below are explicit FX edits.
    let bytes = include_bytes!(
        "../../../tests/fixtures/implemented_additions/native_boolean_operand_structures.aep"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "ad7a9d76dbb06d855b1ac181c92ae4f8e8f1febd315711179d5fcf14ca9f42ca"
    );
    let source = read_project(bytes).unwrap();
    let converted = to_structural_fx_document(&source, Some(1)).unwrap();
    fn find_boolean(layers: &[Layer]) -> Option<&BooleanOperationLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::BooleanOperation(boolean) => Some(boolean),
            LayerData::Group(group) => find_boolean(&group.layers),
            _ => None,
        })
    }
    let boolean = find_boolean(converted.document.composition().layers()).unwrap();
    assert_eq!(boolean.layers.len(), 2);
    let rect = boolean
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            LayerData::Rect(rect) => Some(rect),
            _ => None,
        })
        .unwrap();
    let origin = rect.rect.position;
    let mut owner = json!(boolean);
    owner["type"] = json!("BooleanOperation");
    owner["parent"] = Value::Null;
    let mut value = converted.document.to_json_value().unwrap();
    value["composition"]["layers"] = json!([owner]);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(rect.id, PropType::RectSize, [
            (0, PropertyValue::Vector2([80.0,40.0])),
            (500, PropertyValue::Vector2([140.0,60.0]))]),
        keyed_entry(rect.id, PropType::RectRoundness, [
            (0, PropertyValue::Float(2.0)), (500, PropertyValue::Float(19.0))])
    ]});
    let (output, project) = native(value);
    assert_eq!(layers(&project).len(), 1, "{:?}", output.diagnostics);
    let content = &layers(&project)[0].content;
    assert_keys(
        content,
        "ADBE Vector Rect Size",
        &[&[80.0, 40.0], &[140.0, 60.0]],
    );
    assert_keys(
        content,
        "ADBE Vector Rect Position",
        &[
            &[origin[0] + 40.0, origin[1] + 20.0],
            &[origin[0] + 70.0, origin[1] + 30.0],
        ],
    );
    assert_keys(content, "ADBE Vector Rect Roundness", &[&[2.0], &[19.0]]);
}

#[test]
fn boolean_geometry_delayed_owner_keeps_local_keys_and_interpolation() {
    for (easing, interpolation) in [
        (json!({"type":"linear"}), 1),
        (json!({"type":"hold"}), 3),
        (
            json!({"type":"cubicBezier","x1":0.25,"y1":0.1,"x2":0.75,"y2":0.9}),
            2,
        ),
    ] {
        let mut value = input("rect", false, "union");
        let owner = &mut value["composition"]["layers"][0];
        owner["activeRange"] = json!({"start":250,"duration":1500});
        for child in owner["layers"].as_array_mut().unwrap() {
            child["activeRange"] = json!({"start":250,"duration":1500});
        }
        let mut entry = json!(keyed_entry(
            LayerId::new(401),
            PropType::RectSize,
            [
                (0, PropertyValue::Vector2([80.0, 40.0])),
                (500, PropertyValue::Vector2([140.0, 60.0]))
            ]
        ));
        entry["animator"]["keyframes"][1]["easing"] = easing;
        value["composition"]["dynamics"] = json!({"entries":[entry]});
        let (output, project) = native(value);
        assert_eq!(layers(&project).len(), 2, "{:?}", output.diagnostics);
        let layer = &layers(&project)[0];
        assert_eq!(layer.record.start_time().unwrap(), 0.25);
        assert_keys(
            &layer.content,
            "ADBE Vector Rect Size",
            &[&[80.0, 40.0], &[140.0, 60.0]],
        );
        assert_keys(
            &layer.content,
            "ADBE Vector Rect Position",
            &[&[50.0, 0.0], &[80.0, 10.0]],
        );
        for target in ["ADBE Vector Rect Size", "ADBE Vector Rect Position"] {
            let property = numeric(&layer.content, target).unwrap();
            assert_eq!(
                property.keyframes[0].out_interpolation, interpolation,
                "{target}"
            );
            assert_eq!(
                property.keyframes[1].in_interpolation, interpolation,
                "{target}"
            );
        }
    }
}

#[test]
fn boolean_geometry_constants_and_disabled_values_use_effective_geometry() {
    for disabled in [false, true] {
        let mut value = input("rect", false, "union");
        let entry = if disabled {
            let mut entry = json!(keyed_entry(
                LayerId::new(401),
                PropType::RectSize,
                [
                    (0, PropertyValue::Vector2([20.0, 20.0])),
                    (500, PropertyValue::Vector2([30.0, 30.0]))
                ]
            ));
            entry["animator"]["enabled"] = json!(false);
            entry["animator"]["disabledValue"] = json!(PropertyValue::Vector2([100.0, 60.0]));
            entry
        } else {
            json!(constant_entry(
                LayerId::new(401),
                PropType::RectSize,
                PropertyValue::Vector2([100.0, 60.0])
            ))
        };
        value["composition"]["dynamics"] = json!({"entries":[entry]});
        let (output, project) = native(value);
        assert_eq!(layers(&project).len(), 2, "{:?}", output.diagnostics);
        assert_keys(
            &layers(&project)[0].content,
            "ADBE Vector Rect Size",
            &[&[100.0, 60.0]],
        );
        assert_keys(
            &layers(&project)[0].content,
            "ADBE Vector Rect Position",
            &[&[60.0, 10.0]],
        );
    }
}

#[test]
fn boolean_geometry_unmapped_and_wrong_kind_targets_do_not_disappear_silently() {
    for (geometry, property, first, last) in [
        (
            "rect",
            PropType::EllipseSize,
            PropertyValue::Vector2([80.0, 40.0]),
            PropertyValue::Vector2([90.0, 50.0]),
        ),
        (
            "ellipse",
            PropType::PolyStarOuterRadius,
            PropertyValue::Float(50.0),
            PropertyValue::Float(60.0),
        ),
        (
            "path",
            PropType::EllipseSize,
            PropertyValue::Vector2([80.0, 40.0]),
            PropertyValue::Vector2([90.0, 50.0]),
        ),
        (
            "star",
            PropType::PolyStarPoints,
            PropertyValue::Float(5.0),
            PropertyValue::Float(8.0),
        ),
        (
            "rect",
            PropType::StrokeWidth,
            PropertyValue::Float(2.0),
            PropertyValue::Float(4.0),
        ),
    ] {
        let mut value = input(geometry, false, "union");
        value["composition"]["dynamics"] = json!({"entries":[
            keyed_entry(LayerId::new(401), property, [(0, first), (500, last)])
        ]});
        let (output, project) = native(value);
        assert_eq!(layers(&project).len(), 1, "{geometry}/{property:?}");
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(400))),
            "{geometry}/{property:?}: {:?}",
            output.diagnostics
        );
        assert_eq!(layers(&project)[0].name.as_ref(), "Current solid 499");
    }
}
