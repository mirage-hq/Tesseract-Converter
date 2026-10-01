//! PR #4442 vector/paint import cases.
//!
//! The primary cases below freshly import immutable Adobe-native sources with
//! pinned bytes, SHA-256 and composition IDs. Record mutations are explicitly
//! supplemental diagnostics cases, not independent native oracles.

use super::*;
use crate::rifx::Chunk;
use fx_schema::{BooleanOp, ShapeGradientType, ShapeLineCap, ShapeLineJoin, ShapePaint};

struct PinnedSource {
    path: &'static str,
    bytes: &'static [u8],
    sha256: &'static str,
    byte_count: usize,
}

const RECTANGLES: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_rectangle_controls.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_rectangle_controls.aep"),
    sha256: "694bb9df366bc23a9a72aaf078497058a65b197eaa19c558cfe69f410b6fd2f6",
    byte_count: 468_843,
};
const PAINTS: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_solid_paint_controls.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_solid_paint_controls.aep"),
    sha256: "4c7115020ff2a81da03f205a025688dc1dddf03f963c8fdbaa2dece6cd2650bf",
    byte_count: 935_211,
};
const DASHES: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_stroke_dash_caps.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_stroke_dash_caps.aep"),
    sha256: "293603b08abae9bf0eb553e9a3849dd0f11040345cc3ecf791f09b3453388ec2",
    byte_count: 863_673,
};
const GRADIENTS: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_controls.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_gradient_controls.aep"),
    sha256: "5406fe7f92d697e0882d11a8804c436785b51f533d813419364826b5f35ffcce",
    byte_count: 625_839,
};
const GRADIENT_STROKES: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_gradient_stroke_details.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_gradient_stroke_details.aep"),
    sha256: "17fc50911102dec5cde381ab8ef234c62d315e5b9d3d128dd6d5bc3fe43fe919",
    byte_count: 270_693,
};
const MODIFIERS: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_modifier_controls.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_modifier_controls.aep"),
    sha256: "5bc36fb8210de9bde79d2313b07c4c677e1c040e1d7764ca3a49ba9ed9fcb7e2",
    byte_count: 866_707,
};
const GROUPS: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_group_transform_controls.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_group_transform_controls.aep"),
    sha256: "ea4ba57dc9d672d4e321208298541018464b88a2fce11c25a4ff9ff35d371ed5",
    byte_count: 1_083_591,
};
const BOOLEAN_PAINT: PinnedSource = PinnedSource {
    path: "crates/aftereffects_file/tests/fixtures/shapes/import_boolean_paint_blend.aep",
    bytes: include_bytes!("../../../tests/fixtures/shapes/import_boolean_paint_blend.aep"),
    sha256: "cd215299543670e4a6b1e81c7453ee18b4af8beff3bd59a056aadf3e19663e94",
    byte_count: 165_903,
};

fn pin(source: &PinnedSource) {
    assert_eq!(source.bytes.len(), source.byte_count, "{}", source.path);
    assert_eq!(format!("{:x}", Sha256::digest(source.bytes)), source.sha256);
}

fn fresh_import(source: &PinnedSource, composition_id: u32, name: &str) -> StructuralConversion {
    pin(source);
    let project = read_project(source.bytes).unwrap();
    let native = composition(&project, composition_id);
    assert_eq!(project.item(composition_id).unwrap().name, name);
    assert_eq!(native.frame_rate, 24.0);
    let converted = to_structural_fx_document(&project, Some(composition_id)).unwrap();
    assert_imported_canvas_matches_source(native, &converted, source.path);
    assert_eq!(root(&converted).name, name);
    assert!(
        converted
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .all(|entry| !entry.animator.is_js_script()),
        "conversion must not generate geometry JavaScript"
    );
    converted
}

fn collect_geometry<'a>(layers: &'a [fx_schema::Layer], output: &mut Vec<&'a FxLayer>) {
    for layer in layers {
        match layer.data() {
            FxLayer::Rect(_) | FxLayer::Shape(_) | FxLayer::BooleanOperation(_) => {
                output.push(layer.data());
            }
            FxLayer::Group(group) => collect_geometry(&group.layers, output),
            _ => {}
        }
    }
}

fn geometry(converted: &StructuralConversion) -> Vec<&FxLayer> {
    let mut output = Vec::new();
    collect_geometry(&root(converted).layers, &mut output);
    output
}

fn has_track(converted: &StructuralConversion, property: fx_schema::PropType) -> bool {
    converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .any(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|target| target.property_type() == property)
        })
}

fn replace_property_storage(chunks: &mut Vec<Chunk>, target: &str) -> bool {
    let mut index = 0;
    while index + 1 < chunks.len() {
        let named = chunks[index]
            .data_payload()
            .is_some_and(|payload| payload.starts_with(target.as_bytes()));
        if named && chunks[index + 1].list_kind() == Some(*b"tdbs") {
            chunks[index + 1] =
                Chunk::list(*b"tdbs", vec![Chunk::data(*b"tdb4", vec![0]).unwrap()]);
            return true;
        }
        index += 1;
    }
    for chunk in chunks {
        if chunk
            .children_mut()
            .is_some_and(|children| replace_property_storage(children, target))
        {
            return true;
        }
    }
    false
}

fn malformed_import(
    source: &PinnedSource,
    composition_id: u32,
    property: &str,
) -> StructuralConversion {
    let mut project = read_project(source.bytes).unwrap();
    let composition = composition_mut(&mut project, composition_id);
    assert!(
        composition
            .layers
            .iter_mut()
            .any(|layer| replace_property_storage(&mut layer.content, property))
    );
    to_structural_fx_document(&project, Some(composition_id)).unwrap()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pinned_native_rectangle_geometry_and_paint_channels_import_as_editable_values() {
    for (id, name, size, position, roundness) in [
        (
            1,
            "IMPORT_RECT_SIZE_STATIC",
            [600.0, 200.0],
            [0.0, 0.0],
            0.0,
        ),
        (
            17,
            "IMPORT_RECT_POSITION_STATIC",
            [480.0, 270.0],
            [95.0, -65.0],
            0.0,
        ),
        (
            32,
            "IMPORT_RECT_ROUNDNESS_STATIC",
            [480.0, 270.0],
            [0.0, 0.0],
            64.0,
        ),
    ] {
        let converted = fresh_import(&RECTANGLES, id, name);
        let [FxLayer::Rect(rect)] = geometry(&converted).as_slice() else {
            panic!("{name} must retain one editable Rect")
        };
        assert_eq!(rect.rect.size, size);
        assert_eq!(rect.rect.roundness, roundness);
        assert_eq!(rect.transform.position, fx_schema::Position::TwoD(position));
        assert_eq!(rect.rect.fill_color, [0.75, 0.125, 0.25, 1.0]);
    }

    for (id, name, property) in [
        (47, "IMPORT_RECT_SIZE_KEYED", fx_schema::PropType::RectSize),
        (
            62,
            "IMPORT_RECT_POSITION_KEYED",
            fx_schema::PropType::PositionX,
        ),
        (
            77,
            "IMPORT_RECT_ROUNDNESS_KEYED",
            fx_schema::PropType::RectRoundness,
        ),
    ] {
        let converted = fresh_import(&RECTANGLES, id, name);
        assert!(matches!(
            geometry(&converted).as_slice(),
            [FxLayer::Rect(_)]
        ));
        assert!(has_track(&converted, property), "{name}/{property:?}");
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pinned_native_stroke_cap_join_miter_dash_phase_and_opacity_keep_distinct_owners() {
    for (id, name, cap, join, miter) in [
        (
            1,
            "CAP_BUTT",
            ShapeLineCap::Butt,
            ShapeLineJoin::Miter,
            10.0,
        ),
        (
            17,
            "CAP_ROUND",
            ShapeLineCap::Round,
            ShapeLineJoin::Miter,
            10.0,
        ),
        (
            32,
            "CAP_SQUARE",
            ShapeLineCap::Square,
            ShapeLineJoin::Miter,
            10.0,
        ),
        (
            62,
            "JOIN_ROUND",
            ShapeLineCap::Butt,
            ShapeLineJoin::Round,
            10.0,
        ),
        (
            77,
            "JOIN_BEVEL",
            ShapeLineCap::Butt,
            ShapeLineJoin::Bevel,
            10.0,
        ),
        (
            92,
            "MITER_LOW",
            ShapeLineCap::Butt,
            ShapeLineJoin::Miter,
            2.0,
        ),
    ] {
        let converted = fresh_import(&DASHES, id, name);
        let [FxLayer::Shape(shape)] = geometry(&converted).as_slice() else {
            panic!("{name} must retain editable Shape paint")
        };
        let stroke = &shape.shape.strokes[0];
        assert_eq!(
            (stroke.cap, stroke.join, stroke.miter_limit),
            (cap, join, miter)
        );
        assert_eq!(stroke.width.value(), 24.0);
    }

    for (id, name, dashes, phase) in [
        (122, "DASH_GAP", vec![30.0, 18.0], 0.0),
        (137, "DASH_OFFSET", vec![30.0, 18.0], 22.0),
    ] {
        let converted = fresh_import(&DASHES, id, name);
        let [FxLayer::Shape(shape)] = geometry(&converted).as_slice() else {
            panic!("{name} must retain editable dash paint")
        };
        let stroke = &shape.shape.strokes[0];
        assert_eq!(
            stroke
                .dashes
                .iter()
                .map(|dash| dash.value())
                .collect::<Vec<_>>(),
            dashes
        );
        assert_eq!(stroke.dash_offset, phase);
    }

    let miter = fresh_import(&PAINTS, 167, "IMPORT_PAINT_MITER_LIMIT");
    let [FxLayer::Rect(rect)] = geometry(&miter).as_slice() else {
        panic!("Miter target must stay on a Rect")
    };
    assert_eq!(rect.rect.stroke_miter_limit, 8.0);
    let opacity = fresh_import(&PAINTS, 62, "IMPORT_PAINT_STROKE_OPACITY");
    let [FxLayer::Rect(rect)] = geometry(&opacity).as_slice() else {
        panic!("Stroke opacity target must stay on a Rect")
    };
    assert_eq!(rect.transform.opacity.value(), 55.0);
    let keyed_phase = fresh_import(&DASHES, 152, "DASH_OFFSET_KEYED");
    assert!(has_track(
        &keyed_phase,
        fx_schema::PropType::StrokeDashOffset
    ));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pinned_native_linear_radial_gradient_and_gradient_stroke_controls_remain_editable() {
    for (id, name, expected_type, stroke) in [
        (1, "GRADIENT_FILL_LINEAR", ShapeGradientType::Linear, false),
        (17, "GRADIENT_FILL_RADIAL", ShapeGradientType::Radial, false),
        (
            32,
            "GRADIENT_STROKE_LINEAR",
            ShapeGradientType::Linear,
            true,
        ),
        (
            47,
            "GRADIENT_STROKE_RADIAL",
            ShapeGradientType::Radial,
            true,
        ),
    ] {
        let converted = fresh_import(&GRADIENTS, id, name);
        let paints: Vec<_> = geometry(&converted)
            .into_iter()
            .flat_map(|layer| match layer {
                FxLayer::Rect(rect) => rect.rect.fill_paint.iter().collect(),
                FxLayer::Shape(shape) if stroke => shape
                    .shape
                    .strokes
                    .iter()
                    .map(|style| &style.paint)
                    .collect(),
                FxLayer::Shape(shape) => {
                    shape.shape.fills.iter().map(|style| &style.paint).collect()
                }
                FxLayer::BooleanOperation(boolean) if stroke => {
                    boolean.strokes.iter().map(|style| &style.paint).collect()
                }
                FxLayer::BooleanOperation(boolean) => {
                    boolean.fills.iter().map(|style| &style.paint).collect()
                }
                _ => Vec::new(),
            })
            .collect();
        assert!(paints.iter().any(|paint| matches!(paint,
            ShapePaint::Gradient { gradient_type, stops, .. }
                if *gradient_type == expected_type && stops.len() >= 2)));
    }

    for (id, name, property) in [
        (14, "GRADIENT_STROKE_DASH_GAP", None),
        (27, "GRADIENT_STROKE_DASH_OFFSET", None),
        (
            40,
            "GRADIENT_STROKE_DASH_OFFSET_KEYED",
            Some(fx_schema::PropType::StrokeDashOffset),
        ),
    ] {
        let converted = fresh_import(&GRADIENT_STROKES, id, name);
        let strokes: Vec<_> = geometry(&converted)
            .into_iter()
            .filter_map(|layer| match layer {
                FxLayer::Shape(shape) => shape.shape.strokes.first(),
                FxLayer::BooleanOperation(boolean) => boolean.strokes.first(),
                _ => None,
            })
            .collect();
        assert!(strokes.iter().any(|stroke| {
            matches!(stroke.paint, ShapePaint::Gradient { .. }) && !stroke.dashes.is_empty()
        }));
        if let Some(property) = property {
            assert!(has_track(&converted, property));
        }
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pinned_native_append_stays_contours_while_boolean_modes_keep_typed_operators() {
    let append = fresh_import(&MODIFIERS, 92, "MERGE_APPEND");
    assert!(
        geometry(&append)
            .iter()
            .all(|layer| !matches!(layer, FxLayer::BooleanOperation(_)))
    );

    for (id, name, expected) in [
        (107, "MERGE_UNION", BooleanOp::Union),
        (122, "MERGE_SUBTRACT", BooleanOp::Subtract),
        (137, "MERGE_INTERSECT", BooleanOp::Intersect),
        (152, "MERGE_EXCLUDE", BooleanOp::Exclude),
    ] {
        let converted = fresh_import(&MODIFIERS, id, name);
        let boolean = geometry(&converted)
            .into_iter()
            .find_map(|layer| match layer {
                FxLayer::BooleanOperation(boolean) => Some(boolean),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} must retain an editable Boolean operator"));
        assert_eq!(boolean.op, expected);
        assert!(boolean.layers.len() >= 2);
        assert!(
            boolean
                .layers
                .iter()
                .all(|layer| layer.parent_id() == Some(boolean.id))
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pinned_native_round_offset_trim_and_boolean_paint_ownership_survive_fresh_import() {
    let round = fresh_import(&MODIFIERS, 1, "ROUND");
    assert!(geometry(&round).iter().any(|layer| matches!(layer,
        FxLayer::Shape(shape) if shape.shape.round_corners.is_some())));
    let offset = fresh_import(&MODIFIERS, 17, "OFFSET");
    assert!(geometry(&offset).iter().any(|layer| matches!(layer,
        FxLayer::Shape(shape) if shape.shape.offset_paths.is_some())));
    for (id, name) in [(32, "TRIM_START"), (47, "TRIM_END"), (62, "TRIM_OFFSET")] {
        let converted = fresh_import(&MODIFIERS, id, name);
        assert!(geometry(&converted).iter().any(|layer| match layer {
            FxLayer::Shape(shape) => shape.shape.trim.is_some(),
            FxLayer::BooleanOperation(boolean) => boolean.trim.is_some(),
            _ => false,
        }));
    }

    for (id, name, stroke) in [
        (1, "BOOLEAN_FILL_MULTIPLY", false),
        (17, "BOOLEAN_STROKE_MULTIPLY", true),
    ] {
        let converted = fresh_import(&BOOLEAN_PAINT, id, name);
        let boolean = geometry(&converted)
            .into_iter()
            .find_map(|layer| match layer {
                FxLayer::BooleanOperation(boolean) => Some(boolean),
                _ => None,
            })
            .unwrap();
        assert_eq!(boolean.blend_mode, fx_schema::BlendMode::Multiply);
        assert_eq!(boolean.fills.is_empty(), stroke);
        assert_eq!(boolean.strokes.is_empty(), !stroke);
        assert!(
            boolean
                .layers
                .iter()
                .all(|child| child.parent_id() == Some(boolean.id))
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pinned_native_vector_group_static_and_keyed_transforms_keep_group_ownership() {
    for (id, name, property) in [
        (32, "GROUP_POSITION_STATIC", None),
        (
            47,
            "GROUP_POSITION_KEYED",
            Some(fx_schema::PropType::PositionX),
        ),
        (62, "GROUP_SCALE_STATIC", None),
        (77, "GROUP_SCALE_KEYED", Some(fx_schema::PropType::ScaleX)),
        (92, "GROUP_ROTATION_STATIC", None),
        (
            107,
            "GROUP_ROTATION_KEYED",
            Some(fx_schema::PropType::Rotation),
        ),
        (122, "GROUP_SKEW_STATIC", None),
        (137, "GROUP_SKEW_KEYED", Some(fx_schema::PropType::Skew)),
        (152, "GROUP_SKEW_AXIS_STATIC", None),
        (
            167,
            "GROUP_SKEW_AXIS_KEYED",
            Some(fx_schema::PropType::SkewAxis),
        ),
        (182, "GROUP_OPACITY_STATIC", None),
        (
            197,
            "GROUP_OPACITY_KEYED",
            Some(fx_schema::PropType::Opacity),
        ),
    ] {
        let converted = fresh_import(&GROUPS, id, name);
        let foreground = as_group(&root(&converted).layers[0]);
        assert!(!foreground.layers.is_empty(), "{name}");
        assert!(
            foreground
                .layers
                .iter()
                .all(|child| child.parent_id() == Some(foreground.id))
        );
        if let Some(property) = property {
            assert!(has_track(&converted, property), "{name}/{property:?}");
        }
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn malformed_dash_and_gradient_controls_are_contextual_and_keep_siblings() {
    // The source bytes and composition targets are pinned above, but corrupting
    // one property storage is deliberately supplemental malformed-input evidence.
    let dash = malformed_import(&GRADIENT_STROKES, 14, "ADBE Vector Stroke Dash 1");
    assert!(!geometry(&dash).is_empty());
    assert!(dash.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("stroke dashes are malformed")
            || diagnostic.message.contains("malformed")
    }));

    let gradient = malformed_import(&GRADIENTS, 1, "ADBE Vector Grad Type");
    assert!(!geometry(&gradient).is_empty());
    assert!(gradient.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("ADBE Vector Grad Type")
            && diagnostic.message.contains("malformed")
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn paired_gradient_rect_import_has_separate_pinned_oracles_but_no_combined_native_claim() {
    // No independently Adobe-authored gradient-Fill + solid-Stroke Rectangle is
    // pinned in this checkout. This supplemental case deliberately proves only
    // that each immutable source half imports editably; it must not be registered
    // as independent proof of the combined fast path.
    let gradient = fresh_import(&GRADIENTS, 1, "GRADIENT_FILL_LINEAR");
    assert!(geometry(&gradient).iter().any(|layer| {
        match layer {
            FxLayer::Rect(rect) => {
                matches!(rect.rect.fill_paint, Some(ShapePaint::Gradient { .. }))
            }
            FxLayer::Shape(shape) => shape
                .shape
                .fills
                .iter()
                .any(|fill| matches!(fill.paint, ShapePaint::Gradient { .. })),
            _ => false,
        }
    }));
    let stroke = fresh_import(&PAINTS, 32, "IMPORT_PAINT_STROKE_COLOR");
    assert!(geometry(&stroke).iter().any(|layer| match layer {
        FxLayer::Rect(rect) => rect.rect.stroke_color == Some([0.25, 0.875, 0.125, 1.0]),
        _ => false,
    }));
}
