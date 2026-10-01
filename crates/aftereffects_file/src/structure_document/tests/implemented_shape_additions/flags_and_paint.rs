use super::support::*;
use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn shape_addition_sources_are_sha_pinned_to_independent_authoring_oracles() {
    for source in SOURCES {
        assert_eq!(source.bytes.len(), source.byte_count, "{}", source.file);
        assert_eq!(
            format!("{:x}", Sha256::digest(source.bytes)),
            source.sha256,
            "{} ({})",
            source.file,
            source.oracle
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_enable_flags_omit_only_the_disabled_operation() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, expected_paints) in [
        (1, "GROUP_OFF", Vec::<&str>::new()),
        (17, "PATH_OFF", Vec::new()),
        (32, "FILL_OFF", vec!["stroke"]),
        (47, "STROKE_OFF", vec!["fill"]),
        (62, "MODIFIER_OFF", vec!["fill", "stroke"]),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_shape_flags_order.aep",
            id,
            || {
                let converted = fresh_import("import_shape_flags_order.aep", id, name);
                assert!(
                    diagnostic_contains(&converted, "disabled vector operation"),
                    "{name} must retain an explicit disabled-operation diagnostic"
                );
                assert_eq!(paint_kinds(&converted), expected_paints, "{name}");
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_paints_keep_authored_program_order_and_prefix_ownership() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, expected) in [
        (77, "FILL_THEN_STROKE", vec!["fill", "stroke"]),
        (92, "STROKE_THEN_FILL", vec!["stroke", "fill"]),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_shape_flags_order.aep",
            id,
            || {
                let converted = fresh_import("import_shape_flags_order.aep", id, name);
                assert_eq!(paint_kinds(&converted), expected, "{name}");
            },
        );
    }

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_shape_flags_order.aep",
        107,
        || {
            let prefix = fresh_import("import_shape_flags_order.aep", 107, "PAINT_PREFIX");
            assert_eq!(paint_kinds(&prefix), ["fill", "stroke"]);
            let command_counts = command_counts(&prefix);
            assert_eq!(command_counts.len(), 2);
            assert!(
                command_counts[0] < command_counts[1],
                "the prefix Fill must own only the first rectangle while the later Stroke owns both"
            );
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_composite_ordinals_preserve_both_independent_paint_targets() {
    // The historical author script names these cases ABOVE/BELOW, but its exact
    // native values are 1 and 2. The pinned libpag decoding contract says 1 is
    // Below Previous and 2 is Above Previous. This test therefore checks the
    // concrete values' observable target ordering without treating the names as
    // an independent visual oracle.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name) in [(122, "COMPOSITE_ABOVE"), (137, "COMPOSITE_BELOW")] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_shape_flags_order.aep",
            id,
            || {
                let converted = fresh_import("import_shape_flags_order.aep", id, name);
                assert_eq!(paint_kinds(&converted), ["fill", "stroke"]);
                assert_eq!(geometry(&converted).len(), 2);
                assert!(!diagnostic_contains(
                    &converted,
                    "unknown paint Composite order"
                ));
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_nested_paint_owns_its_geometry_without_becoming_parent_paint_input() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_modifier_order_cases.aep",
        107,
        || {
            let converted =
                fresh_import("import_modifier_order_cases.aep", 107, "NESTED_OWN_PAINT");
            assert_eq!(paint_kinds(&converted), ["fill", "fill"]);
            let counts: Vec<_> = command_counts(&converted)
                .into_iter()
                .filter(|count| *count > 0)
                .collect();
            assert_eq!(counts, [5, 5]);
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_rectangle_direction_keeps_parametric_forward_and_reversed_traversal() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_path_direction_cases.aep",
        62,
        || {
            let forward = fresh_import("import_path_direction_cases.aep", 62, "RECT_DIRECTION_1");
            assert!(geometry(&forward).iter().any(|layer| {
                matches!(layer, FxLayer::Rect(rect) if rect.rect.size == [300.0, 200.0])
            }));
        },
    );
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_path_direction_cases.aep",
        77,
        || {
            let reversed = fresh_import("import_path_direction_cases.aep", 77, "RECT_DIRECTION_3");
            let shape = geometry(&reversed)
                .into_iter()
                .find_map(|layer| match layer {
                    FxLayer::Shape(shape) => Some(shape),
                    _ => None,
                })
                .expect("reversed Rectangle must retain editable path traversal");
            let endpoints: Vec<_> = shape
                .shape
                .path
                .commands
                .iter()
                .filter_map(|command| command.endpoint())
                .collect();
            assert_eq!(
                endpoints,
                vec![
                    (150.0, -100.0),
                    (-150.0, -100.0),
                    (-150.0, 100.0),
                    (150.0, 100.0)
                ]
            );
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn shared_native_path_produces_independently_editable_fill_and_stroke_targets() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_path_direction_cases.aep",
        92,
        || {
            let converted = fresh_import(
                "import_path_direction_cases.aep",
                92,
                "SHARED_PATH_TWO_PAINTS",
            );
            assert_eq!(paint_kinds(&converted), ["fill", "stroke"]);
            let layers = geometry(&converted);
            assert_eq!(layers.len(), 2);
            let ids: Vec<_> = layers
                .iter()
                .filter_map(|layer| match layer {
                    FxLayer::Shape(shape) => Some(shape.id),
                    _ => None,
                })
                .collect();
            assert_eq!(ids.len(), 2);
            assert_ne!(
                ids[0], ids[1],
                "paint copies must be independently editable"
            );
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_solid_stroke_caps_and_shape_blends_keep_their_owners() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, cap) in [
        (77, "IMPORT_PAINT_CAP_BUTT", fx_schema::ShapeLineCap::Butt),
        (92, "IMPORT_PAINT_CAP_ROUND", fx_schema::ShapeLineCap::Round),
        (
            107,
            "IMPORT_PAINT_CAP_SQUARE",
            fx_schema::ShapeLineCap::Square,
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_solid_paint_controls.aep",
            id,
            || {
                let converted = fresh_import("import_solid_paint_controls.aep", id, name);
                let actual = geometry(&converted)
                    .into_iter()
                    .find_map(|layer| match layer {
                        FxLayer::Rect(rect) if rect.rect.stroke_enabled => {
                            Some(fx_schema::ShapeLineCap::Butt)
                        }
                        FxLayer::Shape(shape) => {
                            shape.shape.strokes.first().map(|stroke| stroke.cap)
                        }
                        _ => None,
                    });
                assert_eq!(actual, Some(cap), "{name}");
            },
        );
    }

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_shape_blend_ownership.aep",
        1,
        || {
            let group = fresh_import("import_shape_blend_ownership.aep", 1, "BLEND_GROUP");
            let json = document_json(&group);
            assert!(contains_field(
                &json,
                "blendMode",
                &serde_json::json!("multiply")
            ));
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_shape_blend_ownership.aep",
        17,
        || {
            let fill = fresh_import("import_shape_blend_ownership.aep", 17, "BLEND_FILL");
            assert!(geometry(&fill).iter().any(|layer| {
                match layer {
                    FxLayer::Rect(rect) => {
                        rect.rect.fill_blend_mode == Some(fx_schema::BlendMode::Multiply)
                    }
                    FxLayer::Shape(shape) => shape
                        .shape
                        .fills
                        .iter()
                        .any(|paint| paint.blend_mode == fx_schema::BlendMode::Multiply),
                    _ => false,
                }
            }));
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_shape_blend_ownership.aep",
        32,
        || {
            let stroke = fresh_import("import_shape_blend_ownership.aep", 32, "BLEND_STROKE");
            assert!(geometry(&stroke).iter().any(|layer| {
                match layer {
                    FxLayer::Shape(shape) => shape
                        .shape
                        .strokes
                        .iter()
                        .any(|paint| paint.blend_mode == fx_schema::BlendMode::Multiply),
                    FxLayer::BooleanOperation(boolean) => boolean
                        .strokes
                        .iter()
                        .any(|paint| paint.blend_mode == fx_schema::BlendMode::Multiply),
                    _ => false,
                }
            }));
        },
    );
    cases.finish();
}
