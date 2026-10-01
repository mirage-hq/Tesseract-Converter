use super::support::*;
use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_polygon_animation_keeps_each_authored_editable_control_track() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, property, first, last) in [
        (
            1,
            "POLYGON_KEYED_POINTS",
            "polyStarPoints",
            serde_json::json!(5.0),
            serde_json::json!(8.0),
        ),
        (
            14,
            "POLYGON_KEYED_POSITION",
            "polyStarPosition",
            serde_json::json!([-100.0, 0.0]),
            serde_json::json!([100.0, 0.0]),
        ),
        (
            27,
            "POLYGON_KEYED_ROTATION",
            "polyStarRotation",
            serde_json::json!(0.0),
            serde_json::json!(70.0),
        ),
        (
            40,
            "POLYGON_KEYED_RADIUS",
            "polyStarOuterRadius",
            serde_json::json!(100.0),
            serde_json::json!(200.0),
        ),
        (
            53,
            "POLYGON_KEYED_ROUNDNESS",
            "polyStarOuterRoundness",
            serde_json::json!(0.0),
            serde_json::json!(30.0),
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_polygon_animation.aep",
            id,
            || {
                let converted = fresh_import("import_polygon_animation.aep", id, name);
                let typed = if property == "polyStarPosition" {
                    (
                        serde_json::json!({"type":"vector2","value":first}),
                        serde_json::json!({"type":"vector2","value":last}),
                    )
                } else {
                    (
                        serde_json::json!({"type":"float","value":first}),
                        serde_json::json!({"type":"float","value":last}),
                    )
                };
                assert_track(&converted, property, typed.0, typed.1);
                assert!(geometry(&converted).iter().any(|layer| {
                    match layer {
                        FxLayer::Shape(shape) => {
                            shape.shape.poly_star.as_ref().is_some_and(|star| {
                                star.star_type == fx_schema::ShapePolyStarType::Polygon
                            })
                        }
                        _ => false,
                    }
                }));
                if name == "POLYGON_KEYED_POINTS" {
                    assert!(diagnostic_contains(&converted, "point counts are floored"));
                }
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_modifier_controls_keep_concrete_fx_values_and_individual_mode() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, field, expected) in [
        (
            1,
            "ROUND",
            "roundCorners",
            serde_json::json!({"radius":28.0}),
        ),
        (
            17,
            "OFFSET",
            "offsetPaths",
            serde_json::json!({"amount":24.0,"lineJoin":"miter","miterLimit":4.0}),
        ),
        (
            32,
            "TRIM_START",
            "trim",
            serde_json::json!({"start":20.0,"end":100.0,"offset":0.0,"mode":"simultaneously"}),
        ),
        (
            47,
            "TRIM_END",
            "trim",
            serde_json::json!({"start":0.0,"end":65.0,"offset":0.0,"mode":"simultaneously"}),
        ),
        (
            62,
            "TRIM_OFFSET",
            "trim",
            serde_json::json!({"start":0.0,"end":100.0,"offset":90.0,"mode":"simultaneously"}),
        ),
        (
            77,
            "TRIM_INDIVIDUAL",
            "trim",
            serde_json::json!({"start":0.0,"end":65.0,"offset":0.0,"mode":"individually"}),
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_modifier_controls.aep",
            id,
            || {
                let converted = fresh_import("import_modifier_controls.aep", id, name);
                let json = serde_json::to_value(geometry(&converted)).unwrap();
                assert!(contains_field(&json, field, &expected), "{name}");
            },
        );
    }
    cases.finish();
}

fn boolean_layer(converted: &StructuralConversion) -> Option<&fx_schema::BooleanOperationLayer> {
    geometry(converted)
        .into_iter()
        .find_map(|layer| match layer {
            FxLayer::BooleanOperation(boolean) => Some(boolean),
            _ => None,
        })
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_merge_paths_distinguishes_append_and_each_boolean_operation() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_modifier_controls.aep",
        92,
        || {
            let append = fresh_import("import_modifier_controls.aep", 92, "MERGE_APPEND");
            assert!(
                boolean_layer(&append).is_none(),
                "Append must not be substituted with Union"
            );
            assert!(
                command_counts(&append).iter().any(|count| *count >= 10),
                "Append must preserve both authored rectangle contours"
            );
        },
    );

    for (id, name, expected) in [
        (107, "MERGE_UNION", fx_schema::BooleanOp::Union),
        (122, "MERGE_SUBTRACT", fx_schema::BooleanOp::Subtract),
        (137, "MERGE_INTERSECT", fx_schema::BooleanOp::Intersect),
        (152, "MERGE_EXCLUDE", fx_schema::BooleanOp::Exclude),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_modifier_controls.aep",
            id,
            || {
                let converted = fresh_import("import_modifier_controls.aep", id, name);
                let boolean = boolean_layer(&converted)
                    .expect("Merge Paths must stay editable Boolean geometry");
                assert_eq!(boolean.op, expected, "{name}");
                assert_eq!(boolean.layers.len(), 2, "{name} operands");
                assert_eq!(boolean.fills.len(), 1, "{name} owned paint");
                if expected == fx_schema::BooleanOp::Subtract {
                    let positions: Vec<_> = boolean
                        .layers
                        .iter()
                        .filter_map(|layer| match layer.data() {
                            FxLayer::Rect(rect) => Some(rect.transform.position.xy_array()),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(positions, [[65.0, 50.0], [-65.0, 0.0]]);
                }
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_modifier_order_cases_keep_supported_pipeline_and_exact_omissions() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, diagnostic) in [
        (
            1,
            "ROUND_REPEATED",
            "cannot preserve its scope/repeated operation/order in the fixed FX modifier pipeline",
        ),
        (
            17,
            "ROUND_PARTIAL",
            "partially covered compound paint cannot apply a modifier to unrelated geometry",
        ),
        (32, "ROUND_NESTED", "intermediate Round Corners"),
        (
            47,
            "TRIM_PREFIX_PAINT",
            "Individual Trim requires the complete native path set",
        ),
        (
            77,
            "TRIM_THEN_OFFSET",
            "cannot preserve its scope/repeated operation/order in the fixed FX modifier pipeline",
        ),
        (
            92,
            "BOOLEAN_THEN_ROUND",
            "post-Boolean Round Corners/Offset Paths has no canonical resolved-outline target",
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_modifier_order_cases.aep",
            id,
            || {
                let converted = fresh_import("import_modifier_order_cases.aep", id, name);
                assert!(
                    diagnostic_contains(&converted, diagnostic),
                    "{name}: {diagnostic}"
                );
                assert!(
                    !paint_kinds(&converted).is_empty(),
                    "{name} must retain its convertible paint sibling"
                );
            },
        );
    }

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_modifier_order_cases.aep",
        62,
        || {
            let ordered = fresh_import("import_modifier_order_cases.aep", 62, "OFFSET_THEN_TRIM");
            let json = serde_json::to_value(geometry(&ordered)).unwrap();
            assert!(contains_field(
                &json,
                "offsetPaths",
                &serde_json::json!({"amount":20.0,"lineJoin":"miter","miterLimit":4.0})
            ));
            assert!(contains_field(
                &json,
                "trim",
                &serde_json::json!({"start":0.0,"end":60.0,"offset":0.0,"mode":"simultaneously"})
            ));
        },
    );
    cases.finish();
}
