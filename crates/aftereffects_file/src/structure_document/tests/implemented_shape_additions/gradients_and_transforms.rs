use super::support::*;
use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_group_static_controls_use_fx_transform_units() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, field, expected) in [
        (
            1,
            "GROUP_ANCHOR_STATIC",
            "anchorPoint",
            serde_json::json!([55.0, 25.0]),
        ),
        (
            32,
            "GROUP_POSITION_STATIC",
            "position",
            serde_json::json!([120.0, -60.0]),
        ),
        (
            62,
            "GROUP_SCALE_STATIC",
            "scale",
            serde_json::json!([135.0, 70.0]),
        ),
        (
            92,
            "GROUP_ROTATION_STATIC",
            "rotation",
            serde_json::json!(35.0),
        ),
        (122, "GROUP_SKEW_STATIC", "skew", serde_json::json!(25.0)),
        (
            152,
            "GROUP_SKEW_AXIS_STATIC",
            "skewAxis",
            serde_json::json!(60.0),
        ),
        (
            182,
            "GROUP_OPACITY_STATIC",
            "opacity",
            serde_json::json!(45.0),
        ),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_group_transform_controls.aep",
            id,
            || {
                let converted = fresh_import("import_group_transform_controls.aep", id, name);
                let json = serde_json::to_value(geometry(&converted)).unwrap();
                assert!(
                    contains_field(&json, field, &expected),
                    "{name} must preserve the independently authored {field} value"
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shape_group_keyed_controls_target_editable_transform_components() {
    type TransformCase = (u32, &'static str, &'static [(&'static str, f64, f64)]);
    let transform_cases: &[TransformCase] = &[
        (
            17,
            "GROUP_ANCHOR_KEYED",
            &[("anchorPointX", 0.0, 55.0), ("anchorPointY", 0.0, 25.0)],
        ),
        (
            47,
            "GROUP_POSITION_KEYED",
            &[("positionX", 0.0, 120.0), ("positionY", 0.0, -60.0)],
        ),
        (
            77,
            "GROUP_SCALE_KEYED",
            &[("scaleX", 100.0, 135.0), ("scaleY", 100.0, 70.0)],
        ),
        (107, "GROUP_ROTATION_KEYED", &[("rotation", 0.0, 35.0)]),
        (137, "GROUP_SKEW_KEYED", &[("skew", 0.0, 25.0)]),
        (167, "GROUP_SKEW_AXIS_KEYED", &[("skewAxis", 0.0, 60.0)]),
        (197, "GROUP_OPACITY_KEYED", &[("opacity", 100.0, 45.0)]),
    ];
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, tracks) in transform_cases {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_group_transform_controls.aep",
            *id,
            || {
                let converted = fresh_import("import_group_transform_controls.aep", *id, name);
                for (property, first, last) in *tracks {
                    assert_track(
                        &converted,
                        property,
                        serde_json::json!({"type":"float","value":first}),
                        serde_json::json!({"type":"float","value":last}),
                    );
                }
            },
        );
    }
    cases.finish();
}

fn gradient_paints(
    converted: &StructuralConversion,
) -> Vec<(fx_schema::ShapeGradientType, [f64; 2], [f64; 2], usize)> {
    paints(converted)
        .into_iter()
        .filter_map(|paint| match paint {
            fx_schema::ShapePaint::Gradient {
                gradient_type,
                start,
                end,
                stops,
            } => Some((*gradient_type, *start, *end, stops.len())),
            fx_schema::ShapePaint::Solid { .. } => None,
        })
        .collect()
}

fn assert_ae_default_gradient(converted: &StructuralConversion, name: &str) {
    let stops = paints(converted)
        .into_iter()
        .find_map(|paint| match paint {
            fx_schema::ShapePaint::Gradient { stops, .. } => Some(stops),
            fx_schema::ShapePaint::Solid { .. } => None,
        })
        .unwrap_or_else(|| panic!("{name} must retain an editable gradient"));
    assert_eq!(
        stops
            .iter()
            .map(|stop| (stop.offset, stop.color))
            .collect::<Vec<_>>(),
        [(0.0, [1.0, 1.0, 1.0, 1.0]), (1.0, [0.0, 0.0, 0.0, 1.0]),],
        "{name} must reconstruct AE's omitted default Colors payload"
    );
    assert!(
        converted.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("default gradient Colors payload")),
        "{name} must diagnose the reconstructed default"
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_gradient_types_axes_opacity_and_stroke_width_remain_editable() {
    use fx_schema::ShapeGradientType::{Linear, Radial};

    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, expected_type) in [
        (1, "GRADIENT_FILL_LINEAR", Linear),
        (17, "GRADIENT_FILL_RADIAL", Radial),
        (32, "GRADIENT_STROKE_LINEAR", Linear),
        (47, "GRADIENT_STROKE_RADIAL", Radial),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_controls.aep",
            id,
            || {
                let converted = fresh_import("import_gradient_controls.aep", id, name);
                let gradients = gradient_paints(&converted);
                assert_eq!(gradients.len(), 1, "{name}");
                assert_eq!(gradients[0].0, expected_type, "{name}");
                assert!(gradients[0].3 >= 2, "{name} must retain native stops");
                assert_ae_default_gradient(&converted, name);
            },
        );
    }

    for (id, name, start, end) in [
        (62, "GRADIENT_START_POINT", [-160.0, -100.0], [240.0, 0.0]),
        (77, "GRADIENT_END_POINT", [-240.0, 0.0], [100.0, 130.0]),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_controls.aep",
            id,
            || {
                let converted = fresh_import("import_gradient_controls.aep", id, name);
                let gradients = gradient_paints(&converted);
                assert_eq!(gradients.len(), 1, "{name}");
                assert_eq!((gradients[0].1, gradients[0].2), (start, end), "{name}");
                assert_ae_default_gradient(&converted, name);
            },
        );
    }

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_controls.aep",
        92,
        || {
            let opacity = fresh_import("import_gradient_controls.aep", 92, "GRADIENT_OPACITY");
            assert!(geometry(&opacity).iter().any(|layer| match layer {
                FxLayer::Shape(shape) => {
                    shape.transform.opacity.value() == 45.0
                        && shape.shape.fills.iter().any(|fill| fill.opacity == 1.0)
                }
                FxLayer::Rect(rect) => rect.transform.opacity.value() == 45.0,
                _ => false,
            }));
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_controls.aep",
        107,
        || {
            let width = fresh_import("import_gradient_controls.aep", 107, "GRADIENT_STROKE_WIDTH");
            assert!(geometry(&width).iter().any(|layer| {
                match layer {
                    FxLayer::Shape(shape) => shape
                        .shape
                        .strokes
                        .iter()
                        .any(|stroke| stroke.width.value() == 30.0),
                    _ => false,
                }
            }));
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_gradient_stroke_details_keep_opacity_dashes_and_keyed_offset() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_stroke_details.aep",
        1,
        || {
            let opacity = fresh_import(
                "import_gradient_stroke_details.aep",
                1,
                "GRADIENT_STROKE_OPACITY",
            );
            assert!(geometry(&opacity).iter().any(|layer| {
                match layer {
                    FxLayer::Shape(shape) => shape
                        .shape
                        .strokes
                        .iter()
                        .any(|stroke| stroke.opacity == 0.45),
                    _ => false,
                }
            }));
        },
    );

    for (id, name, expected_offset) in [
        (14, "GRADIENT_STROKE_DASH_GAP", 0.0),
        (27, "GRADIENT_STROKE_DASH_OFFSET", 22.0),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_stroke_details.aep",
            id,
            || {
                let converted = fresh_import("import_gradient_stroke_details.aep", id, name);
                assert!(geometry(&converted).iter().any(|layer| match layer {
                    FxLayer::Shape(shape) => shape.shape.strokes.iter().any(|stroke| {
                        stroke
                            .dashes
                            .iter()
                            .map(|value| value.value())
                            .collect::<Vec<_>>()
                            == [30.0, 18.0]
                            && stroke.dash_offset == expected_offset
                    }),
                    _ => false,
                }));
            },
        );
    }

    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_stroke_details.aep",
        40,
        || {
            let keyed = fresh_import(
                "import_gradient_stroke_details.aep",
                40,
                "GRADIENT_STROKE_DASH_OFFSET_KEYED",
            );
            assert_track(
                &keyed,
                "strokeDashOffset",
                serde_json::json!({"type":"float","value":0.0}),
                serde_json::json!({"type":"float","value":80.0}),
            );
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn isolated_native_gradient_stops_are_owned_by_the_selected_paint_only() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, expected_kind) in [
        (14, "NATIVE_STOPS_FILL_ONLY", "fill"),
        (27, "NATIVE_STOPS_STROKE_ONLY", "stroke"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/shapes/import_isolated_native_gradients.aep",
            id,
            || {
                let converted = fresh_import("import_isolated_native_gradients.aep", id, name);
                assert_eq!(
                    converted.document.background_color(),
                    Some([0.2, 0.4, 0.6, 1.0]),
                    "{name} maps native #336699 preview RGB to an opaque FX background"
                );
                assert!(
                    converted.diagnostics.iter().any(|diagnostic| {
                        diagnostic.composition_id == Some(id)
                            && diagnostic.message.contains("opaque FX canvas content")
                            && diagnostic.message.contains("transparency is lost")
                    }),
                    "{name} must diagnose the background alpha approximation"
                );
                assert_eq!(paint_kinds(&converted), [expected_kind], "{name}");
                let gradients = gradient_paints(&converted);
                assert_eq!(gradients.len(), 1, "{name}");
                assert!(
                    gradients[0].3 >= 2,
                    "{name} must retain the independently pinned native stop list"
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_gradient_stroke_miter_animation_targets_the_stroke_style() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/shapes/import_remaining_mapped_controls.aep",
        47,
        || {
            let converted = fresh_import(
                "import_remaining_mapped_controls.aep",
                47,
                "GRADIENT_STROKE_MITER_KEYED",
            );
            assert_track(
                &converted,
                "strokeMiterLimit",
                serde_json::json!({"type":"float","value":2.0}),
                serde_json::json!({"type":"float","value":10.0}),
            );
        },
    );
    cases.finish();
}
