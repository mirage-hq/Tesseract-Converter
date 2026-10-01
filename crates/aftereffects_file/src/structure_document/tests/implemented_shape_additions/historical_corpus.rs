// These expectations come from the Adobe authoring readbacks removed in the
// fixture-sidecar cleanup commit, not from converter output.
use super::support::*;
use super::*;

fn first_boolean(converted: &StructuralConversion) -> &fx_schema::BooleanOperationLayer {
    geometry(converted)
        .into_iter()
        .find_map(|layer| match layer {
            FxLayer::BooleanOperation(boolean) => Some(boolean),
            _ => None,
        })
        .expect("native Merge Paths must remain editable Boolean geometry")
}

fn assert_three_key_track(
    converted: &StructuralConversion,
    property: &str,
    values: [Value; 3],
    easing: &str,
) {
    let json = document_json(converted);
    let matching: Vec<_> = json["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["propertyType"] == property)
        .collect();
    assert_eq!(matching.len(), 1, "expected one editable {property} track");
    let keys = matching[0]["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 3, "{property}");
    for (index, (key, expected)) in keys.iter().zip(values).enumerate() {
        assert_eq!(key["layerTime"], index as u64 * 1_000, "{property} time");
        assert_eq!(key["value"], expected, "{property} key {index}");
    }
    assert_eq!(keys[0]["easing"]["type"], easing, "{property} segment 0");
    assert_eq!(keys[1]["easing"]["type"], easing, "{property} segment 1");
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn historical_static_shape_readbacks_import_exact_editable_geometry_and_paint() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, join, width) in [
        (
            14,
            "E04_RECT_STROKE_MITER",
            fx_schema::ShapeLineJoin::Miter,
            3.0,
        ),
        (
            27,
            "E04_RECT_STROKE_ROUND",
            fx_schema::ShapeLineJoin::Round,
            4.0,
        ),
        (
            40,
            "E04_RECT_STROKE_BEVEL",
            fx_schema::ShapeLineJoin::Bevel,
            5.0,
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_static_paints_and_geometry.aep",
            id,
            || {
                let converted =
                    fresh_import("native_static_paints_and_geometry.aep", id, name);
                let rect = geometry(&converted)
                    .into_iter()
                    .find_map(|layer| match layer {
                        FxLayer::Rect(rect) => Some(rect),
                        _ => None,
                    })
                    .expect("reviewed Rectangle source must stay editable");
                assert_eq!((rect.rect.size, rect.rect.roundness), ([140.0, 60.0], 11.0));
                assert_eq!(rect.rect.stroke_width.value(), width);
                assert_eq!(rect.rect.stroke_join, join);
            },
        );
    }

    cases.run(
        "crates/aftereffects_file/tests/fixtures/implemented_additions/native_static_paints_and_geometry.aep",
        1,
        || {
            let fill =
                fresh_import("native_static_paints_and_geometry.aep", 1, "E04_RECT_FILL");
            let FxLayer::Rect(rect) = geometry(&fill)[0] else {
                panic!("fill must be an editable Rect")
            };
            assert_eq!((rect.rect.size, rect.rect.roundness), ([140.0, 60.0], 11.0));
            assert_eq!(
                rect.rect.fill_color,
                [0.2_f32, 0.3, 0.4, 1.0].map(f64::from)
            );
        },
    );

    for (id, name, kind) in [
        (53, "E05_ELLIPSE", "ellipse"),
        (66, "E05_POLYGON", "polygon"),
        (79, "E05_STAR", "star"),
        (92, "E05_STATIC_CUBIC_PATH", "path"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_static_paints_and_geometry.aep",
            id,
            || {
                let converted =
                    fresh_import("native_static_paints_and_geometry.aep", id, name);
                let shape = geometry(&converted)
                    .into_iter()
                    .find_map(|layer| match layer {
                        FxLayer::Shape(shape) => Some(shape),
                        _ => None,
                    })
                    .expect("reviewed Shape source must stay editable");
                assert_eq!(shape.shape.fills.len(), 1, "{name}");
                match kind {
            "ellipse" => {
                let ellipse = shape.shape.ellipse.as_ref().unwrap();
                assert_eq!(
                    (ellipse.size, ellipse.position, ellipse.reversed),
                    ([120.0, 66.0], [9.0, -6.0], true)
                );
            }
            "polygon" => {
                let star = shape.shape.poly_star.as_ref().unwrap();
                assert_eq!(star.star_type, fx_schema::ShapePolyStarType::Polygon);
                assert_eq!(
                    (
                        star.points,
                        star.position,
                        star.rotation,
                        star.outer_radius,
                        star.outer_roundness
                    ),
                    (6.0, [-2.0, 8.0], 22.0, 48.0, 3.0)
                );
                assert!(star.reversed);
            }
            "star" => {
                let star = shape.shape.poly_star.as_ref().unwrap();
                assert_eq!(star.star_type, fx_schema::ShapePolyStarType::Star);
                assert_eq!(
                    (
                        star.points,
                        star.position,
                        star.rotation,
                        star.inner_radius,
                        star.outer_radius
                    ),
                    (7.0, [3.0, 4.0], 15.0, 24.0, 60.0)
                );
            }
            "path" => {
                let endpoints: Vec<_> = shape
                    .shape
                    .path
                    .commands
                    .iter()
                    .filter_map(|command| command.endpoint())
                    .collect();
                assert_eq!(endpoints, [(4.0, 0.0), (44.0, 20.0), (4.0, 30.0)]);
                assert!(
                    shape.shape.path.commands.iter().any(|command| matches!(
                        command,
                        fx_schema::ShapePathCommand::CubicTo { .. }
                    ))
                );
                assert!(
                    shape
                        .shape
                        .path
                        .commands
                        .iter()
                        .any(|command| matches!(command, fx_schema::ShapePathCommand::Close))
                );
            }
                    _ => unreachable!(),
                }
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn historical_boolean_readbacks_preserve_operation_operands_paint_and_order() {
    use fx_schema::BooleanOp::{Exclude, Intersect, Subtract, Union};
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, op) in [
        (1, "E12_BOOLEAN_UNION", Union),
        (14, "E12_BOOLEAN_SUBTRACT", Subtract),
        (27, "E12_BOOLEAN_INTERSECT", Intersect),
        (40, "E12_BOOLEAN_EXCLUDE", Exclude),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_boolean_operations.aep",
            id,
            || {
                let converted = fresh_import("native_boolean_operations.aep", id, name);
                let boolean = first_boolean(&converted);
                assert_eq!(
                    (boolean.op, boolean.layers.len(), boolean.fills.len()),
                    (op, 2, 1),
                    "{name}"
                );
                assert!(boolean.strokes.is_empty(), "{name}");
            },
        );
    }

    for (id, name, op, nested, kinds) in [
        (1, "E12_PATH_RECT_UNION", Union, false, ["shape", "rect"]),
        (
            14,
            "E12_PATH_RECT_SUBTRACT",
            Subtract,
            false,
            ["shape", "rect"],
        ),
        (
            27,
            "E12_PATH_RECT_INTERSECT",
            Intersect,
            false,
            ["shape", "rect"],
        ),
        (
            40,
            "E12_PATH_RECT_EXCLUDE",
            Exclude,
            false,
            ["shape", "rect"],
        ),
        (53, "E12_NESTED_UNION", Union, true, ["boolean", "rect"]),
        (
            66,
            "E12_NESTED_SUBTRACT",
            Subtract,
            true,
            ["boolean", "rect"],
        ),
        (
            79,
            "E12_NESTED_INTERSECT",
            Intersect,
            true,
            ["boolean", "rect"],
        ),
        (92, "E12_NESTED_EXCLUDE", Exclude, true, ["boolean", "rect"]),
        (
            105,
            "E12_SUBTRACT_ORDER_PATH_RECT",
            Subtract,
            false,
            ["shape", "rect"],
        ),
        (
            118,
            "E12_SUBTRACT_ORDER_RECT_PATH",
            Subtract,
            false,
            ["rect", "shape"],
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_boolean_operand_structures.aep",
            id,
            || {
                let converted =
                    fresh_import("native_boolean_operand_structures.aep", id, name);
                let boolean = first_boolean(&converted);
                assert_eq!(
                    (boolean.op, boolean.layers.len(), boolean.fills.len()),
                    (op, 2, 1),
                    "{name}"
                );
                let actual = boolean
                    .layers
                    .iter()
                    .map(|layer| match layer.data() {
                        FxLayer::Shape(_) => "shape",
                        FxLayer::Rect(_) => "rect",
                        FxLayer::BooleanOperation(_) => "boolean",
                        _ => "other",
                    })
                    .collect::<Vec<_>>();
                assert_eq!(actual, kinds, "{name} operand order");
                assert_eq!(actual[0] == "boolean", nested, "{name} nesting");
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn historical_nested_boolean_identity_scopes_preserve_union_and_exclude() {
    use fx_schema::BooleanOp::{Exclude, Union};

    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, operation) in [
        (53, "E12_NESTED_UNION", Union),
        (92, "E12_NESTED_EXCLUDE", Exclude),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_boolean_operand_structures.aep",
            id,
            || {
                let converted =
                    fresh_import("native_boolean_operand_structures.aep", id, name);
                let outer = first_boolean(&converted);
                assert_eq!(outer.op, operation, "{name}");
                assert!(!outer.is_hidden, "{name} outer Boolean visibility");
                assert_eq!(outer.layers.len(), 2, "{name} outer operands");
                assert_eq!(outer.fills.len(), 1, "{name} outer paint ownership");
                assert!(outer.strokes.is_empty(), "{name} outer strokes");

                let nested = match outer.layers[0].data() {
                    FxLayer::BooleanOperation(nested) => nested,
                    _ => panic!("{name} first operand must remain a nested Boolean"),
                };
                assert_eq!(nested.op, Union, "{name} nested operation");
                assert_eq!(nested.layers.len(), 2, "{name} nested operands");
                assert!(nested.fills.is_empty(), "{name} nested fills");
                assert!(nested.strokes.is_empty(), "{name} nested strokes");
                assert!(!nested.is_hidden, "{name} nested visibility");

                let rect = match outer.layers[1].data() {
                    FxLayer::Rect(rect) => rect,
                    _ => panic!("{name} second operand must remain an editable Rect"),
                };
                assert!(!rect.rect.fill_enabled, "{name} Rect fill ownership");
                assert!(!rect.rect.stroke_enabled, "{name} Rect stroke ownership");

                let painted_geometry = geometry(&converted)
                    .into_iter()
                    .filter(|layer| match layer {
                        FxLayer::BooleanOperation(boolean) => {
                            !boolean.fills.is_empty() || !boolean.strokes.is_empty()
                        }
                        FxLayer::Shape(shape) => {
                            !shape.shape.fills.is_empty() || !shape.shape.strokes.is_empty()
                        }
                        FxLayer::Rect(rect) => rect.rect.fill_enabled || rect.rect.stroke_enabled,
                        _ => false,
                    })
                    .count();
                assert_eq!(painted_geometry, 1, "{name} independent painted helpers");
                assert!(
                    converted
                        .document
                        .composition()
                        .dynamics()
                        .entries()
                        .iter()
                        .all(|entry| !entry.animator.is_js_script()),
                    "{name} generated JavaScript"
                );
                assert!(
                    !diagnostic_contains(&converted, "every native operand is required"),
                    "{name} atomic operand omission diagnostic"
                );
                assert!(
                    !diagnostic_contains(
                        &converted,
                        "native Rectangle producer cannot cross a vector-group"
                    ),
                    "{name} Rectangle transport diagnostic"
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn historical_rectangle_key_readbacks_preserve_values_and_interpolation() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/implemented_additions/native_transform_keys.aep",
        66,
        || {
            let size = fresh_import("native_transform_keys.aep", 66, "E10_RECT_SIZE_KEYS");
            assert_three_key_track(
                &size,
                "rectSize",
                [
                    serde_json::json!({"type":"vector2","value":[80.0,40.0]}),
                    serde_json::json!({"type":"vector2","value":[115.0,55.0]}),
                    serde_json::json!({"type":"vector2","value":[150.0,70.0]}),
                ],
                "cubicBezier",
            );
        },
    );
    cases.run(
        "crates/aftereffects_file/tests/fixtures/implemented_additions/native_transform_keys.aep",
        79,
        || {
            let roundness =
                fresh_import("native_transform_keys.aep", 79, "E10_RECT_ROUNDNESS_KEYS");
            assert_three_key_track(
                &roundness,
                "rectRoundness",
                [
                    serde_json::json!({"type":"float","value":2.0}),
                    serde_json::json!({"type":"float","value":10.0}),
                    serde_json::json!({"type":"float","value":19.0}),
                ],
                "hold",
            );
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn historical_parametric_key_readbacks_preserve_all_isolated_shape_channels() {
    let cases: &[(u32, &str, &str, &str, [Value; 3])] = &[
        (
            1,
            "E11_ELLIPSE_SIZE",
            "ellipseSize",
            "linear",
            [
                serde_json::json!({"type":"vector2","value":[80.0,40.0]}),
                serde_json::json!({"type":"vector2","value":[140.0,70.0]}),
                serde_json::json!({"type":"vector2","value":[200.0,100.0]}),
            ],
        ),
        (
            14,
            "E11_ELLIPSE_POSITION",
            "ellipsePosition",
            "linear",
            [
                serde_json::json!({"type":"vector2","value":[-80.0,0.0]}),
                serde_json::json!({"type":"vector2","value":[0.0,20.0]}),
                serde_json::json!({"type":"vector2","value":[80.0,0.0]}),
            ],
        ),
        (
            27,
            "E11_STAR_POSITION",
            "polyStarPosition",
            "linear",
            [
                serde_json::json!({"type":"vector2","value":[-60.0,0.0]}),
                serde_json::json!({"type":"vector2","value":[0.0,25.0]}),
                serde_json::json!({"type":"vector2","value":[60.0,0.0]}),
            ],
        ),
        (
            40,
            "E11_STAR_POINTS_HOLD",
            "polyStarPoints",
            "hold",
            [
                serde_json::json!({"type":"float","value":5.0}),
                serde_json::json!({"type":"float","value":7.0}),
                serde_json::json!({"type":"float","value":9.0}),
            ],
        ),
        (
            53,
            "E11_STAR_ROTATION",
            "polyStarRotation",
            "linear",
            [
                serde_json::json!({"type":"float","value":0.0}),
                serde_json::json!({"type":"float","value":25.0}),
                serde_json::json!({"type":"float","value":50.0}),
            ],
        ),
        (
            66,
            "E11_STAR_INNER_RADIUS",
            "polyStarInnerRadius",
            "linear",
            [
                serde_json::json!({"type":"float","value":20.0}),
                serde_json::json!({"type":"float","value":35.0}),
                serde_json::json!({"type":"float","value":50.0}),
            ],
        ),
        (
            79,
            "E11_STAR_OUTER_RADIUS",
            "polyStarOuterRadius",
            "linear",
            [
                serde_json::json!({"type":"float","value":60.0}),
                serde_json::json!({"type":"float","value":85.0}),
                serde_json::json!({"type":"float","value":110.0}),
            ],
        ),
        (
            92,
            "E11_STAR_INNER_ROUNDNESS",
            "polyStarInnerRoundness",
            "linear",
            [
                serde_json::json!({"type":"float","value":0.0}),
                serde_json::json!({"type":"float","value":20.0}),
                serde_json::json!({"type":"float","value":40.0}),
            ],
        ),
        (
            105,
            "E11_STAR_OUTER_ROUNDNESS",
            "polyStarOuterRoundness",
            "linear",
            [
                serde_json::json!({"type":"float","value":0.0}),
                serde_json::json!({"type":"float","value":15.0}),
                serde_json::json!({"type":"float","value":30.0}),
            ],
        ),
        (
            118,
            "E11_POLYGON_POSITION",
            "polyStarPosition",
            "linear",
            [
                serde_json::json!({"type":"vector2","value":[-60.0,0.0]}),
                serde_json::json!({"type":"vector2","value":[0.0,25.0]}),
                serde_json::json!({"type":"vector2","value":[60.0,0.0]}),
            ],
        ),
        (
            131,
            "E11_POLYGON_POINTS_HOLD",
            "polyStarPoints",
            "hold",
            [
                serde_json::json!({"type":"float","value":4.0}),
                serde_json::json!({"type":"float","value":6.0}),
                serde_json::json!({"type":"float","value":8.0}),
            ],
        ),
        (
            144,
            "E11_POLYGON_OUTER_RADIUS",
            "polyStarOuterRadius",
            "linear",
            [
                serde_json::json!({"type":"float","value":50.0}),
                serde_json::json!({"type":"float","value":75.0}),
                serde_json::json!({"type":"float","value":100.0}),
            ],
        ),
        (
            157,
            "E11_POLYGON_ROTATION",
            "polyStarRotation",
            "linear",
            [
                serde_json::json!({"type":"float","value":0.0}),
                serde_json::json!({"type":"float","value":30.0}),
                serde_json::json!({"type":"float","value":60.0}),
            ],
        ),
        (
            170,
            "E11_POLYGON_OUTER_ROUNDNESS",
            "polyStarOuterRoundness",
            "linear",
            [
                serde_json::json!({"type":"float","value":0.0}),
                serde_json::json!({"type":"float","value":20.0}),
                serde_json::json!({"type":"float","value":40.0}),
            ],
        ),
    ];
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    for (id, name, property, easing, values) in cases {
        batch.run(
            "crates/aftereffects_file/tests/fixtures/implemented_additions/native_parametric_key_channels.aep",
            *id,
            || {
                let converted = fresh_import("native_parametric_key_channels.aep", *id, name);
                assert_three_key_track(&converted, property, values.clone(), easing);
                let shape = geometry(&converted)
                    .into_iter()
                    .find_map(|layer| match layer {
                        FxLayer::Shape(shape) => Some(shape),
                        _ => None,
                    })
                    .unwrap();
                if name.contains("POLYGON") {
                    assert_eq!(
                        shape.shape.poly_star.as_ref().unwrap().star_type,
                        fx_schema::ShapePolyStarType::Polygon,
                        "{name}"
                    );
                }
            },
        );
    }
    batch.finish();
}
