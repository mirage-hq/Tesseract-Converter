//! PR #4442 vector/paint export cases.
//!
//! These cases author explicit editable FX inputs and inspect a freshly written
//! native project. They are supplemental structural evidence: our own reader is
//! not independent Adobe acceptance or render-fidelity proof.

use super::*;
use crate::{properties, rifx::Chunk};
use fx_schema::animator::{KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack};

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

fn run_names(chunks: &[Chunk], output: &mut Vec<String>) {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            output.push(name.to_owned());
            run_names(run, output);
        }
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        run_names(children, output);
    }
}

fn names(chunks: &[Chunk]) -> Vec<String> {
    let mut output = Vec::new();
    run_names(chunks, &mut output);
    output
}

fn paint_orders(chunks: &[Chunk], output: &mut Vec<(String, f64)>) {
    if let Ok(runs) = properties::runs(chunks)
        && !runs.is_empty()
    {
        for (name, run) in runs {
            if name.starts_with("ADBE Vector Graphic - ") {
                output.push((
                    name.to_owned(),
                    numeric(run, "ADBE Vector Composite Order")
                        .expect("editable paint Composite Order")
                        .values[0],
                ));
            } else {
                paint_orders(run, output);
            }
        }
        return;
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        paint_orders(children, output);
    }
}

fn gradient(kind: &str) -> Value {
    json!({
        "type":"gradient",
        "gradientType":kind,
        "start":[-40.0,0.0],
        "end":[80.0,20.0],
        "stops":[
            {"offset":0.0,"color":[1.0,0.0,0.0,1.0]},
            {"offset":0.4,"color":[0.0,1.0,0.0,0.75]},
            {"offset":1.0,"color":[0.0,0.0,1.0,0.5]}
        ]
    })
}

fn path_shape(value: &Value, id: u64) -> Value {
    let mut layer = rect(value, id);
    layer["type"] = json!("Shape");
    layer["name"] = json!(format!("Vector {id}"));
    layer["transform"] = value["composition"]["layers"][0]["transform"].clone();
    layer.as_object_mut().unwrap().remove("rect");
    layer["shape"] = json!({
        "path":{"commands":[
            {"type":"moveTo","x":0.0,"y":0.0},
            {"type":"lineTo","x":120.0,"y":0.0},
            {"type":"lineTo","x":100.0,"y":80.0},
            {"type":"close"}
        ]},
        "fills":[],
        "strokes":[]
    });
    layer
}

fn fresh_native(value: Value) -> (ExportedDocument, StructuralProject) {
    assert!(
        !serde_json::to_string(&value).unwrap().contains("jsScript"),
        "feature input must not hide geometry in JavaScript"
    );
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    (output, native)
}

fn disabled_bool_entry(id: LayerId, property: PropType, value: bool) -> AnimationGraphEntry {
    let mut entry = keyed_entry(
        id,
        property,
        [
            (0, PropertyValue::Bool(!value)),
            (500, PropertyValue::Bool(value)),
        ],
    );
    let mut data = entry.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = &mut data
    else {
        panic!("keyed bool fixture")
    };
    *enabled = false;
    *disabled_value = Some(PropertyValue::Bool(value));
    entry.animator = PropertyAnimator::from_data(&data).unwrap();
    entry
}

fn join_entry(id: LayerId) -> AnimationGraphEntry {
    let keys = [(0, "miter"), (250, "round"), (500, "bevel")]
        .into_iter()
        .enumerate()
        .map(|(index, (millis, value))| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("pr4442-join-{index}")),
                fx_schema::TimeOffset::from_millis(millis),
                PropertyValue::String(value.into()),
                if index == 0 {
                    PropertyKeyframeEasing::Linear
                } else {
                    PropertyKeyframeEasing::Hold
                },
            )
        })
        .collect();
    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(id, PropType::StrokeJoin),
        animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn edited_rect_gradient_fill_and_solid_stroke_export_in_owned_order() {
    for gradient_type in ["linear", "radial"] {
        let mut value = imported();
        let mut layer = rect(&value, 4_442);
        layer["name"] = json!(format!("Paired {gradient_type} Rectangle"));
        layer["rect"]["fillEnabled"] = json!(true);
        layer["rect"]["fillPaint"] = gradient(gradient_type);
        layer["rect"]["strokeEnabled"] = json!(true);
        layer["rect"]["strokeColor"] = json!([0.2, 0.3, 0.4, 1.0]);
        layer["rect"]["strokeWidth"] = json!(9.0);
        layer["rect"]["strokeJoin"] = json!("round");
        layer["rect"]["strokeMiterLimit"] = json!(7.0);
        value["composition"]["layers"] = json!([layer]);
        value["composition"]["dynamics"] = json!({"entries":[]});

        let (output, native) = fresh_native(value);
        assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
        assert_eq!(
            layers(&native)[0].record.source_id(),
            0,
            "must be a fresh Shape layer"
        );
        let runs = names(&layers(&native)[0].content);
        let gradient_index = runs
            .iter()
            .position(|name| name == "ADBE Vector Graphic - G-Fill")
            .unwrap();
        let stroke_index = runs
            .iter()
            .position(|name| name == "ADBE Vector Graphic - Stroke")
            .unwrap();
        assert!(
            gradient_index < stroke_index,
            "Fill must remain below the later Stroke"
        );
        let mut orders = Vec::new();
        paint_orders(&layers(&native)[0].content, &mut orders);
        assert_eq!(
            orders,
            [
                ("ADBE Vector Graphic - G-Fill".into(), 1.0),
                ("ADBE Vector Graphic - Stroke".into(), 2.0),
            ]
        );
        assert_eq!(
            numeric(&layers(&native)[0].content, "ADBE Vector Stroke Width")
                .unwrap()
                .values,
            vec![9.0]
        );
        assert_eq!(
            numeric(&layers(&native)[0].content, "ADBE Vector Stroke Line Join")
                .unwrap()
                .values,
            vec![2.0]
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn edited_shape_multiple_paints_keep_order_fill_rule_cap_opacity_and_stroke_keys() {
    let mut value = imported();
    let mut layer = path_shape(&value, 4_443);
    layer["shape"]["fills"] = json!([
        {"paint":gradient("linear"),"fillRule":"evenOdd","opacity":0.8},
        {"paint":{"type":"solid","color":[0.1,0.2,0.9,0.6]},"fillRule":"nonZeroWinding","opacity":0.4}
    ]);
    layer["shape"]["strokes"] = json!([
        {"paint":{"type":"solid","color":[0.9,0.8,0.1,0.7]},"width":6.0,
         "cap":"square","join":"miter","miterLimit":11.0,"opacity":0.3}
    ]);
    value["composition"]["layers"] = json!([layer]);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(4_443), PropType::StrokeWidth,
            [(0, PropertyValue::Float(2.0)), (500, PropertyValue::Float(12.0))]),
        keyed_entry(LayerId::new(4_443), PropType::StrokeMiterLimit,
            [(0, PropertyValue::Float(4.0)), (500, PropertyValue::Float(10.0))]),
        join_entry(LayerId::new(4_443))
    ]});

    let (output, native) = fresh_native(value);
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let content = &layers(&native)[0].content;
    let mut orders = Vec::new();
    paint_orders(content, &mut orders);
    assert_eq!(
        orders,
        [
            ("ADBE Vector Graphic - G-Fill".into(), 1.0),
            ("ADBE Vector Graphic - Fill".into(), 2.0),
            ("ADBE Vector Graphic - Stroke".into(), 2.0),
        ]
    );
    assert_eq!(
        numeric(content, "ADBE Vector Fill Rule").unwrap().values,
        vec![2.0]
    );
    assert_eq!(
        numeric(content, "ADBE Vector Stroke Line Cap")
            .unwrap()
            .values,
        vec![3.0]
    );
    assert_eq!(
        numeric(content, "ADBE Vector Stroke Opacity")
            .unwrap()
            .values,
        vec![30.0]
    );
    for target in [
        "ADBE Vector Stroke Width",
        "ADBE Vector Stroke Miter Limit",
        "ADBE Vector Stroke Line Join",
    ] {
        let property = numeric(content, target).unwrap();
        assert!(property.animated, "{target}");
        assert!(property.keyframes.len() >= 2, "{target}");
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn reflected_gradient_is_normalized_but_conic_is_rejected_with_sibling_retained() {
    let mut reflected = imported();
    let mut reflected_layer = path_shape(&reflected, 4_444);
    reflected_layer["shape"]["fills"] = json!([{"paint":gradient("reflected")}]);
    reflected["composition"]["layers"] = json!([reflected_layer]);
    reflected["composition"]["dynamics"] = json!({"entries":[]});
    let (output, native) = fresh_native(reflected);
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert!(
        names(&layers(&native)[0].content)
            .iter()
            .any(|name| name == "ADBE Vector Graphic - G-Fill")
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(4_444))
            && diagnostic
                .message
                .contains("mirrored Linear native controls")
    }));

    let mut conic = imported();
    let mut conic_layer = path_shape(&conic, 4_445);
    conic_layer["shape"]["fills"] = json!([{"paint":gradient("conic")}]);
    conic["composition"]["layers"] = json!([conic_layer, rect(&conic, 4_446)]);
    conic["composition"]["dynamics"] = json!({"entries":[]});
    let (output, native) = fresh_native(conic);
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 4446");
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(4_445))
            && diagnostic.message.contains("conic gradient")
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn vector_modifiers_and_group_transform_keys_export_as_native_editable_records() {
    let mut value = imported();
    let mut child = path_shape(&value, 4_448);
    child["parent"] = json!(4_447);
    child["shape"]["fills"] = json!([{
        "paint":{"type":"solid","color":[0.2,0.7,0.4,1.0]}
    }]);
    child["shape"]["roundCorners"] = json!({"radius":12.0});
    child["shape"]["offsetPaths"] = json!({"amount":5.0,"lineJoin":"round","miterLimit":8.0});
    child["shape"]["trim"] = json!({"start":10.0,"end":85.0,"offset":30.0,"mode":"simultaneously"});

    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(4_447);
    group["parent"] = Value::Null;
    group["name"] = json!("Animated vector Group");
    group["transform"]["position"] = json!([20.0, -10.0]);
    group["transform"]["skew"] = json!(8.0);
    group["transform"]["skewAxis"] = json!(25.0);
    group["layers"] = json!([child]);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(4_447), PropType::Rotation,
            [(0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(45.0))]),
        keyed_entry(LayerId::new(4_448), PropType::RoundCornersRadius,
            [(0, PropertyValue::Float(4.0)), (500, PropertyValue::Float(20.0))]),
        keyed_entry(LayerId::new(4_448), PropType::OffsetPathsAmount,
            [(0, PropertyValue::Float(-5.0)), (500, PropertyValue::Float(9.0))]),
        keyed_entry(LayerId::new(4_448), PropType::TrimStart,
            [(0, PropertyValue::Float(10.0)), (500, PropertyValue::Float(40.0))])
    ]});

    let (output, native) = fresh_native(value);
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let content = &layers(&native)[0].content;
    let runs = names(content);
    for expected in [
        "ADBE Vector Group",
        "ADBE Vector Filter - RC",
        "ADBE Vector Filter - Offset",
        "ADBE Vector Filter - Trim",
    ] {
        assert!(
            runs.iter().any(|name| name == expected),
            "missing {expected}: {runs:?}"
        );
    }
    for target in [
        "ADBE Vector Rotation",
        "ADBE Vector RoundCorner Radius",
        "ADBE Vector Offset Amount",
        "ADBE Vector Trim Start",
    ] {
        assert!(numeric(content, target).unwrap().animated, "{target}");
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn boolean_operands_keep_transforms_keys_merge_mode_and_owned_paint() {
    for (operation, ordinal) in [
        ("union", 2.0),
        ("subtract", 3.0),
        ("intersect", 4.0),
        ("exclude", 5.0),
    ] {
        let mut value = imported();
        let mut first = path_shape(&value, 4_451);
        first["parent"] = json!(4_450);
        first["transform"]["position"] = json!([15.0, 5.0]);
        first["shape"]["fills"] = json!([]);
        let mut second = rect(&value, 4_452);
        second["parent"] = json!(4_450);
        second["rect"]["fillEnabled"] = json!(false);
        second["rect"]["strokeEnabled"] = json!(false);
        second["transform"]["rotation"] = json!(12.0);
        let boolean = json!({
            "type":"BooleanOperation","id":4450,"name":format!("Native {operation}"),
            "parent":null,"activeRange":first["activeRange"],
            "transform":first["transform"],"op":operation,"layers":[first,second],
            "fills":[{"paint":{"type":"solid","color":[0.8,0.2,0.4,1.0]},"fillRule":"evenOdd"}],
            "strokes":[]
        });
        value["composition"]["layers"] = json!([boolean]);
        value["composition"]["dynamics"] = json!({"entries":[
            keyed_entry(LayerId::new(4_451), PropType::PositionX,
                [(0, PropertyValue::Float(15.0)), (500, PropertyValue::Float(45.0))]),
            keyed_entry(LayerId::new(4_452), PropType::Rotation,
                [(0, PropertyValue::Float(12.0)), (500, PropertyValue::Float(35.0))])
        ]});

        let (output, native) = fresh_native(value);
        assert_eq!(
            layers(&native).len(),
            1,
            "{operation}: {:?}",
            output.diagnostics
        );
        let content = &layers(&native)[0].content;
        assert_eq!(
            numeric(content, "ADBE Vector Merge Type").unwrap().values,
            vec![ordinal]
        );
        assert_eq!(
            numeric(content, "ADBE Vector Fill Rule").unwrap().values,
            vec![2.0]
        );
        assert!(
            names(content)
                .iter()
                .filter(|name| *name == "ADBE Vector Group")
                .count()
                >= 2
        );
        assert!(numeric(content, "ADBE Vector Position").unwrap().animated);
        assert!(numeric(content, "ADBE Vector Rotation").unwrap().animated);
    }
}

// Positive contracts, intentionally separate from unsupported-target diagnostics.
// These explicit FX inputs provide own-reader evidence only; independent native
// feature fixtures and Adobe render comparisons are still required.
fn coverage_contract_boolean_geometry(
    property: PropType,
    values: [PropertyValue; 2],
    native_property: &str,
    expected: [Vec<f64>; 2],
) {
    let mut value = imported();
    let mut operand = rect(&value, 4_552);
    operand["parent"] = json!(4_550);
    operand["rect"]["position"] = json!([7.0, -3.0]);
    operand["rect"]["size"] = json!([80.0, 40.0]);
    operand["rect"]["roundness"] = json!(2.0);
    operand["rect"]["fillEnabled"] = json!(false);
    operand["rect"]["strokeEnabled"] = json!(false);
    let mut sibling = path_shape(&value, 4_553);
    sibling["parent"] = json!(4_550);
    let boolean = json!({
        "type":"BooleanOperation","id":4550,"name":"Animated Boolean operand",
        "parent":null,"activeRange":operand["activeRange"],
        "transform":operand["transform"],"op":"subtract","layers":[operand,sibling],
        "fills":[{"paint":{"type":"solid","color":[0.8,0.2,0.4,1.0]},"fillRule":"evenOdd"}],
        "strokes":[]
    });
    value["composition"]["layers"] = json!([boolean]);
    let [first, last] = values;
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(4_552), property, [(0, first), (500, last)])
    ]});

    let (output, native) = fresh_native(value);
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let content = &layers(&native)[0].content;
    assert_eq!(
        numeric(content, "ADBE Vector Merge Type").unwrap().values,
        vec![3.0]
    );
    let track = numeric(content, native_property).expect("editable operand geometry");
    assert!(track.animated, "geometry must not be frozen to its base");
    assert_eq!(track.keyframes.len(), 2);
    assert_eq!(track.keyframes[0].out_interpolation, 1);
    assert_eq!(track.keyframes[1].in_interpolation, 1);
    for ((key, values), time) in track.keyframes.iter().zip(expected).zip([0.0, 0.5]) {
        assert_eq!(key.values, values);
        assert!((key.time_secs - time).abs() < 1e-6);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coverage_contract_boolean_operand_size_keys() {
    coverage_contract_boolean_geometry(
        PropType::RectSize,
        [
            PropertyValue::Vector2([80.0, 40.0]),
            PropertyValue::Vector2([150.0, 70.0]),
        ],
        "ADBE Vector Rect Size",
        [vec![80.0, 40.0], vec![150.0, 70.0]],
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coverage_contract_boolean_operand_size_keys_move_native_center() {
    coverage_contract_boolean_geometry(
        PropType::RectSize,
        [
            PropertyValue::Vector2([80.0, 40.0]),
            PropertyValue::Vector2([150.0, 70.0]),
        ],
        "ADBE Vector Rect Position",
        [vec![47.0, 17.0], vec![82.0, 32.0]],
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coverage_contract_boolean_operand_roundness_keys() {
    coverage_contract_boolean_geometry(
        PropType::RectRoundness,
        [PropertyValue::Float(2.0), PropertyValue::Float(19.0)],
        "ADBE Vector Rect Roundness",
        [vec![2.0], vec![19.0]],
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn rectangle_size_center_roundness_and_dynamic_dashed_stroke_remain_separate() {
    let mut value = imported();
    let mut layer = rect(&value, 4_460);
    layer["name"] = json!("Dynamic dashed Rectangle");
    layer["rect"]["position"] = json!([10.0, 20.0]);
    layer["rect"]["size"] = json!([120.0, 80.0]);
    layer["rect"]["roundness"] = json!(0.0);
    layer["rect"]["fillEnabled"] = json!(true);
    layer["rect"]["strokeEnabled"] = json!(true);
    layer["rect"]["strokeColor"] = json!([0.0, 0.0, 0.0, 1.0]);
    layer["rect"]["strokeWidth"] = json!(5.0);
    layer["rect"]["strokeDashes"] = json!([8.0, 4.0]);
    layer["rect"]["strokeDashOffset"] = json!(-2.0);
    value["composition"]["layers"] = json!([layer]);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(4_460), PropType::StrokeDashOffset,
            [(0, PropertyValue::Float(-2.0)), (500, PropertyValue::Float(10.0))])
    ]});

    let (output, native) = fresh_native(value);
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let content = &layers(&native)[0].content;
    let runs = names(content);
    assert!(runs.iter().any(|name| name == "ADBE Vector Shape - Rect"));
    assert!(runs.iter().any(|name| name == "ADBE Vector Shape - Group"));
    assert_eq!(
        numeric(content, "ADBE Vector Rect Size").unwrap().values,
        vec![120.0, 80.0]
    );
    assert_eq!(
        numeric(content, "ADBE Vector Rect Position")
            .unwrap()
            .values,
        vec![70.0, 60.0]
    );
    assert_eq!(
        numeric(content, "ADBE Vector Rect Roundness")
            .unwrap()
            .values,
        vec![0.0]
    );
    assert!(
        numeric(content, "ADBE Vector Stroke Offset")
            .unwrap()
            .animated
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn paint_enable_constants_disabled_values_and_empty_array_defaults_are_materialized() {
    for (property, enabled, expected_run) in [
        (PropType::FillEnabled, true, "ADBE Vector Graphic - Fill"),
        (
            PropType::StrokeEnabled,
            true,
            "ADBE Vector Graphic - Stroke",
        ),
    ] {
        let mut value = imported();
        let mut layer = path_shape(&value, 4_470);
        layer["shape"]["fills"] = json!([]);
        layer["shape"]["strokes"] = json!([]);
        value["composition"]["layers"] = json!([layer]);
        value["composition"]["dynamics"] = json!({"entries":[
            constant_entry(LayerId::new(4_470), property, PropertyValue::Bool(enabled))
        ]});
        let (output, native) = fresh_native(value);
        assert_eq!(
            layers(&native).len(),
            1,
            "{property:?}: {:?}",
            output.diagnostics
        );
        assert!(
            names(&layers(&native)[0].content)
                .iter()
                .any(|name| name == expected_run)
        );
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(4_470))
                && diagnostic.message.contains("normalized")
        }));
    }

    let mut value = imported();
    let mut layer = path_shape(&value, 4_471);
    layer["shape"]["fills"] = json!([{
        "paint":{"type":"solid","color":[0.3,0.4,0.5,1.0]}
    }]);
    value["composition"]["layers"] = json!([layer]);
    value["composition"]["dynamics"] = json!({"entries":[
        disabled_bool_entry(LayerId::new(4_471), PropType::FillEnabled, false)
    ]});
    let (output, native) = fresh_native(value);
    assert!(
        layers(&native).is_empty(),
        "disabled only paint must not become visible"
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(4_471)) && diagnostic.message.contains("omitted")
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn wrong_kind_and_unconsumed_targets_omit_only_the_affected_layer() {
    for (property, value) in [
        (PropType::EllipseSize, PropertyValue::Vector2([80.0, 60.0])),
        (PropType::PolyStarPoints, PropertyValue::Float(7.0)),
        (
            PropType::FillColor,
            PropertyValue::Color([1.0, 0.0, 0.0, 1.0]),
        ),
    ] {
        let mut document = imported();
        let mut affected = rect(&document, 4_480);
        if property == PropType::FillColor {
            affected["rect"]["fillPaint"] = gradient("linear");
        }
        document["composition"]["layers"] = json!([affected, rect(&document, 4_481)]);
        document["composition"]["dynamics"] = json!({"entries":[
            constant_entry(LayerId::new(4_480), property, value)
        ]});
        let (output, native) = fresh_native(document);
        assert_eq!(
            layers(&native).len(),
            1,
            "{property:?}: {:?}",
            output.diagnostics
        );
        assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 4481");
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(4_480))
                && (diagnostic.message.contains("target")
                    || diagnostic.message.contains("gradient"))
        }));
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn malformed_dash_patterns_and_animated_rounded_dash_keep_convertible_siblings() {
    for (pattern, roundness, animate_geometry) in [
        (vec![4.0], 0.0, false),
        (vec![4.0, 0.0], 0.0, false),
        (vec![4.0; 8], 0.0, false),
        (vec![8.0, 4.0], 12.0, false),
        (vec![8.0, 4.0], 0.0, true),
    ] {
        let mut value = imported();
        let mut affected = rect(&value, 4_490);
        affected["rect"]["fillEnabled"] = json!(true);
        affected["rect"]["strokeEnabled"] = json!(true);
        affected["rect"]["strokeColor"] = json!([0.2, 0.2, 0.2, 1.0]);
        affected["rect"]["strokeDashes"] = json!(pattern);
        affected["rect"]["roundness"] = json!(roundness);
        value["composition"]["layers"] = json!([affected, rect(&value, 4_491)]);
        value["composition"]["dynamics"] = if animate_geometry {
            json!({"entries":[keyed_entry(
                LayerId::new(4_490), PropType::RectSize,
                [(0, PropertyValue::Vector2([100.0, 50.0])),
                 (500, PropertyValue::Vector2([150.0, 70.0]))]
            )]})
        } else {
            json!({"entries":[]})
        };
        let (output, native) = fresh_native(value);
        assert!(
            layers(&native)
                .iter()
                .any(|layer| layer.name.as_ref() == "Current solid 4491")
        );
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(4_490))
                && (diagnostic.message.contains("Dash/Gap")
                    || diagnostic.message.contains("dashed Stroke"))
        }));
    }
}
