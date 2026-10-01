use super::*;
use crate::{
    properties,
    rifx::Chunk,
    structure::{ItemKind, read_project},
    structure_document::to_structural_fx_document,
};
use fx_schema::layer::{ShapeLineCap, ShapeLineJoin, ShapePathCommand};
use sha2::{Digest, Sha256};

fn appearance() -> VectorAppearance {
    VectorAppearance {
        name: "Edited Shape".into(),
        stroke_dashes: Default::default(),
        paint_opacity: 100.0,
        fill_color: Some([0.25, 0.5, 0.75, 1.0]),
        fill_rule: Default::default(),
        stroke_color: None,
        stroke_cap: Default::default(),
        stroke_width: 0.0,
        stroke_join: ShapeLineJoin::Miter,
        stroke_miter_limit: 4.0,
        transform: super::super::SolidTransform {
            anchor: [0.0, 0.0],
            position: [320.0, 240.0],
            scale: [100.0, 100.0],
            rotation: 0.0,
            opacity: 100.0,
        },
    }
}

#[test]
fn paint_opacity_is_validated_independently_of_layer_opacity() {
    for opacity in [-1.0, 101.0, f64::NAN, f64::INFINITY] {
        let mut paint = appearance();
        paint.paint_opacity = opacity;
        paint.transform.opacity = 0.0;
        assert!(validate_appearance(&paint).is_err());
    }
    for opacity in [0.0, 25.0, 100.0] {
        let mut paint = appearance();
        paint.paint_opacity = opacity;
        assert!(validate_appearance(&paint).is_ok());
    }
}

fn roundtrip(geometry: VectorGeometry) -> fx_schema::ShapeLayer {
    let spec = VectorShapeSpec {
        appearance: appearance(),
        geometry,
        stroke_animations: Default::default(),
        animations: TransformAnimations::default(),
        geometry_animations: GeometryAnimations::default(),
    };
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Shapes".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::Shape(Box::new(spec))],
    )
    .unwrap();
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    assert_eq!(comp.layers.len(), 1);
    assert_eq!(comp.layers[0].record.layer_type(), 4);
    assert_eq!(comp.layers[0].record.source_id(), 0);
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    fn shape(layers: &[fx_schema::Layer]) -> Option<&fx_schema::ShapeLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            fx_schema::LayerData::Shape(value) if !value.is_hidden => Some(value),
            fx_schema::LayerData::Group(group) => shape(&group.layers),
            _ => None,
        })
    }
    shape(imported.document.composition().layers())
        .expect("editable native Shape")
        .clone()
}

fn roundtrip_boolean(
    op: BooleanOp,
    operands: Vec<VectorGeometry>,
) -> fx_schema::BooleanOperationLayer {
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Boolean Shapes".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::Boolean(VectorBooleanSpec {
            appearance: appearance(),
            op,
            operands,
            stroke_animations: Default::default(),
            animations: TransformAnimations::default(),
        })],
    )
    .unwrap();
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    assert_eq!(comp.layers.len(), 1);
    assert_eq!(comp.layers[0].record.layer_type(), 4);
    assert_eq!(comp.layers[0].record.source_id(), 0);
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    fn boolean(layers: &[fx_schema::Layer]) -> Option<&fx_schema::BooleanOperationLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            fx_schema::LayerData::BooleanOperation(value) if !value.is_hidden => Some(value),
            fx_schema::LayerData::Group(group) => boolean(&group.layers),
            _ => None,
        })
    }
    boolean(imported.document.composition().layers())
        .expect("editable native Boolean")
        .clone()
}

#[test]
fn fresh_ellipse_preserves_editable_native_parameters() {
    let ellipse = ShapeEllipse {
        size: [140.0, 90.0],
        position: [12.0, -4.0],
        reversed: true,
    };
    let actual = roundtrip(VectorGeometry::Ellipse(ellipse.clone()));
    assert_eq!(actual.shape.ellipse, Some(ellipse));
    assert_eq!(actual.shape.fills.len(), 1);
}

#[test]
fn fresh_star_preserves_editable_native_parameters() {
    let star = ShapePolyStar {
        star_type: ShapePolyStarType::Polygon,
        points: 7.0,
        position: [14.0, 5.0],
        rotation: 18.0,
        outer_radius: 57.0,
        inner_radius: 20.0,
        outer_roundness: 3.0,
        inner_roundness: 2.0,
        reversed: false,
    };
    let actual = roundtrip(VectorGeometry::PolyStar(star.clone()));
    assert_eq!(actual.shape.poly_star, Some(star));
}

#[test]
fn fresh_path_keeps_editable_cubic_and_closure() {
    let path = ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: 10.0,
                y: 20.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: 15.0,
                c1y: 20.0,
                c2x: 30.0,
                c2y: 35.0,
                x: 40.0,
                y: 40.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 10.0,
                y: 40.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::Close,
        ],
    };
    let actual = roundtrip(VectorGeometry::Path(path));
    assert!(actual.shape.ellipse.is_none());
    assert!(actual.shape.poly_star.is_none());
    assert!(actual.shape.path.is_finite());
    assert!(
        actual
            .shape
            .path
            .commands
            .iter()
            .any(|command| matches!(command, ShapePathCommand::CubicTo { .. }))
    );
    assert_eq!(
        actual.shape.path.commands.last(),
        Some(&ShapePathCommand::Close)
    );
}

fn compound_path() -> ShapePath {
    ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: 0.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 100.0,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 100.0,
                y: 100.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 0.0,
                y: 100.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::Close,
            ShapePathCommand::MoveTo {
                x: 25.0,
                y: 25.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 25.0,
                y: 75.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 75.0,
                y: 75.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: 75.0,
                y: 25.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::Close,
            ShapePathCommand::MoveTo {
                x: 10.0,
                y: 120.0,
                mirror: None,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: 35.0,
                c1y: 90.0,
                c2x: 65.0,
                c2y: 150.0,
                x: 90.0,
                y: 120.0,
                mirror: None,
                corner_radius: None,
            },
        ],
    }
}

#[test]
fn native_compound_path_fixture_imports_editable_shared_scope_semantics() {
    const SOURCE: &[u8] =
        include_bytes!("../../../tests/fixtures/export_repairs/native-controls.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE)),
        "f62f72bf6ec281a63be63ec22b35e7552ee1ef48e4b8db97bb19773f372450a3"
    );

    let project = read_project(SOURCE).expect("pinned native export-repair fixture");
    let imported = to_structural_fx_document(&project, Some(1))
        .expect("compound static Paths should import as editable geometry");
    fn collect<'a>(layers: &'a [fx_schema::Layer], output: &mut Vec<&'a fx_schema::ShapeLayer>) {
        for layer in layers {
            match layer.data() {
                fx_schema::LayerData::Shape(shape) if !shape.is_hidden => output.push(shape),
                fx_schema::LayerData::Group(group) => collect(&group.layers, output),
                _ => {}
            }
        }
    }
    fn move_points(path: &ShapePath) -> Vec<(f64, f64)> {
        path.commands
            .iter()
            .filter_map(|command| match command {
                ShapePathCommand::MoveTo { x, y, .. } => Some((*x, *y)),
                _ => None,
            })
            .collect()
    }
    fn assert_points_close(actual: &[(f64, f64)], expected: &[(f64, f64)]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual.0 - expected.0).abs() < 0.001);
            assert!((actual.1 - expected.1).abs() < 0.001);
        }
    }
    fn signed_areas(path: &ShapePath) -> Vec<f64> {
        fn area(points: &[(f64, f64)]) -> f64 {
            points
                .iter()
                .zip(points.iter().cycle().skip(1))
                .take(points.len())
                .map(|(current, next)| current.0 * next.1 - next.0 * current.1)
                .sum::<f64>()
                / 2.0
        }

        let mut contours = Vec::new();
        let mut points = Vec::new();
        for command in &path.commands {
            match command {
                ShapePathCommand::MoveTo { x, y, .. } => {
                    if !points.is_empty() {
                        contours.push(area(&points));
                        points.clear();
                    }
                    points.push((*x, *y));
                }
                ShapePathCommand::LineTo { x, y, .. } | ShapePathCommand::CubicTo { x, y, .. } => {
                    points.push((*x, *y))
                }
                ShapePathCommand::Close => {
                    contours.push(area(&points));
                    points.clear();
                }
            }
        }
        if !points.is_empty() {
            contours.push(area(&points));
        }
        contours
    }

    let mut shapes = Vec::new();
    collect(imported.document.composition().layers(), &mut shapes);
    assert_eq!(shapes.len(), 3);
    let winding = shapes
        .iter()
        .copied()
        .find(|shape| {
            shape
                .shape
                .fills
                .first()
                .is_some_and(|fill| fill.fill_rule == ShapeFillRule::NonZeroWinding)
        })
        .expect("opposite-winding shared Fill");
    let even_odd = shapes
        .iter()
        .copied()
        .find(|shape| {
            shape
                .shape
                .fills
                .first()
                .is_some_and(|fill| fill.fill_rule == ShapeFillRule::EvenOdd)
        })
        .expect("same-winding Even-Odd shared Fill");
    let open = shapes
        .iter()
        .copied()
        .find(|shape| !shape.shape.strokes.is_empty())
        .expect("shared open-contour Stroke and Trim Paths");

    assert_points_close(
        &move_points(&winding.shape.path),
        &[(-292.0, -138.0), (-222.0, -82.0)],
    );
    assert_points_close(
        &move_points(&even_odd.shape.path),
        &[(38.0, -132.0), (105.0, -82.0)],
    );
    let winding_areas = signed_areas(&winding.shape.path);
    let even_odd_areas = signed_areas(&even_odd.shape.path);
    assert_eq!(winding_areas.len(), 2);
    assert_eq!(even_odd_areas.len(), 2);
    assert!(winding_areas[0] * winding_areas[1] < 0.0);
    assert!(even_odd_areas[0] * even_odd_areas[1] > 0.0);
    for shape in [winding, even_odd] {
        assert_eq!(
            shape
                .shape
                .path
                .commands
                .iter()
                .filter(|command| matches!(command, ShapePathCommand::Close))
                .count(),
            2
        );
    }

    assert_points_close(
        &move_points(&open.shape.path),
        &[(-284.0, 112.0), (38.0, 112.0)],
    );
    assert!(
        !open
            .shape
            .path
            .commands
            .iter()
            .any(|command| matches!(command, ShapePathCommand::Close))
    );
    assert!(
        open.shape
            .path
            .commands
            .iter()
            .any(|command| matches!(command, ShapePathCommand::CubicTo { .. }))
    );
    assert_eq!(open.shape.strokes.len(), 1);
    assert_eq!(open.shape.strokes[0].cap, ShapeLineCap::Round);
    assert_eq!(open.shape.strokes[0].join, ShapeLineJoin::Round);
    assert_eq!(open.shape.strokes[0].width.value(), 9.0);
    let trim = open.shape.trim.expect("shared editable Trim Paths");
    assert_eq!((trim.start, trim.end, trim.offset), (13.0, 83.0, 21.0));
    assert_eq!(trim.mode, ShapeTrimMode::Simultaneously);
}

#[test]
fn compound_path_rejects_nonfinite_coordinates_and_excessive_work() {
    let mut nonfinite = compound_path();
    let ShapePathCommand::MoveTo { x, .. } = &mut nonfinite.commands[0] else {
        panic!("fixture starts with MoveTo")
    };
    *x = f64::NAN;
    assert!(super::super::path_geometry::validated_contours(&nonfinite).is_err());

    // Separate one-vertex contours are valid, so the u16 triple-list bound is
    // per contour: one MoveTo plus LineTo commands, up to 21,845 vertices.
    let contour = |vertices: usize| ShapePath {
        commands: std::iter::once(ShapePathCommand::MoveTo {
            x: 0.0,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        })
        .chain((1..vertices).map(|index| ShapePathCommand::LineTo {
            x: index as f64,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        }))
        .collect(),
    };
    assert!(super::super::path_geometry::validated_contours(&contour(21_845)).is_ok());
    assert!(matches!(
        super::super::path_geometry::validated_contours(&contour(21_846)),
        Err(super::super::AepWriteError::Invalid(
            "native Path vertex count must fit a u16 triple list"
        ))
    ));

    let keyed = VectorShapeSpec {
        appearance: appearance(),
        geometry: VectorGeometry::Path(compound_path()),
        stroke_animations: Default::default(),
        animations: TransformAnimations::default(),
        geometry_animations: GeometryAnimations {
            rect_size: Some(rect_track(&[(0, &[10.0, 10.0])])),
            ..GeometryAnimations::default()
        },
    };
    assert!(validate(&keyed).is_err());
}

fn count_bytes(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn write_shape(geometry: VectorGeometry) -> Vec<u8> {
    let mut shape_appearance = appearance();
    shape_appearance.fill_rule = ShapeFillRule::EvenOdd;
    super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Compound Shapes".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::Shape(Box::new(VectorShapeSpec {
            appearance: shape_appearance,
            geometry,
            stroke_animations: Default::default(),
            animations: TransformAnimations::default(),
            geometry_animations: GeometryAnimations::default(),
        }))],
    )
    .expect("compound Path should split into native Path records")
}

#[test]
fn compound_path_direct_shape_emits_all_contours_under_one_paint_scope() {
    let bytes = write_shape(VectorGeometry::Path(compound_path()));
    assert_eq!(count_bytes(&bytes, b"ADBE Vector Shape - Group"), 3);
    assert_eq!(count_bytes(&bytes, b"ADBE Vector Graphic - Fill"), 1);

    let project = read_project(&bytes).expect("fresh compound Path project");
    let imported =
        to_structural_fx_document(&project, Some(1)).expect("fresh compound Path import");
    fn paths(layers: &[fx_schema::Layer], output: &mut Vec<ShapePath>) {
        for layer in layers {
            match layer.data() {
                fx_schema::LayerData::Shape(shape) if !shape.is_hidden => {
                    output.push(shape.shape.path.clone());
                }
                fx_schema::LayerData::Group(group) => paths(&group.layers, output),
                _ => {}
            }
        }
    }
    let mut actual = Vec::new();
    paths(imported.document.composition().layers(), &mut actual);
    assert_eq!(actual.len(), 1);
    let path = &actual[0];
    assert_eq!(
        path.commands
            .iter()
            .filter(|command| matches!(command, ShapePathCommand::MoveTo { .. }))
            .count(),
        3
    );
    assert_eq!(
        path.commands
            .iter()
            .filter(|command| matches!(command, ShapePathCommand::Close))
            .count(),
        2
    );
    assert!(matches!(
        path.commands.last(),
        Some(ShapePathCommand::CubicTo { .. })
    ));
    assert_eq!(path.commands[1].endpoint(), Some((100.0, 0.0)));
    assert_eq!(path.commands[7].endpoint(), Some((25.0, 75.0)));
}

#[test]
fn compound_path_vector_program_keeps_trim_mode_and_single_scope() {
    let spec = program_spec(vec![
        VectorContent::Geometry {
            geometry: VectorGeometry::Path(compound_path()),
            animations: GeometryAnimations::default(),
        },
        VectorContent::Modifier(VectorModifierSpec::TrimPaths {
            value: ShapeTrimPaths {
                start: 10.0,
                end: 80.0,
                offset: 15.0,
                mode: ShapeTrimMode::Individually,
            },
            start: None,
            end: None,
            offset: None,
        }),
        VectorContent::Paint(VectorPaintSpec::Fill {
            paint: ShapePaint::Solid {
                color: [0.25, 0.5, 0.75, 1.0],
            },
            fill_rule: ShapeFillRule::EvenOdd,
            blend_mode: BlendMode::Normal,
            opacity: 100.0,
            animations: VectorPaintAnimations::default(),
        }),
    ]);
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Compound Program".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::VectorProgram(spec)],
    )
    .expect("compound Path program should remain in one modifier/paint scope");
    assert_eq!(count_bytes(&bytes, b"ADBE Vector Shape - Group"), 3);
    assert_eq!(count_bytes(&bytes, b"ADBE Vector Filter - Trim"), 1);
    assert_eq!(count_bytes(&bytes, b"ADBE Vector Graphic - Fill"), 1);

    let project = read_project(&bytes).expect("fresh compound program");
    let ItemKind::Composition(composition) = &project.item(1).expect("composition").kind else {
        panic!("fresh composition")
    };
    assert_eq!(
        native_numeric(&composition.layers[0].content, "ADBE Vector Trim Type")
            .expect("Trim Paths mode")
            .values,
        vec![2.0]
    );
}

#[test]
fn compound_path_keeps_one_operand_for_non_associative_booleans() {
    let source =
        include_bytes!("../../../tests/fixtures/export_repairs/compound_boolean/native.aep");
    let readback = include_str!(
        "../../../tests/fixtures/export_repairs/compound_boolean/native-readback.json"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        "87802637746bc0c6f9af34bd163ca5be510c3019a88e858822e7b9c32a75ac2c"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(readback.as_bytes())),
        "b42f7c7238c9f67328b88a1c41a7320dc350c2e69758797bd4112fe7936ad616"
    );
    let readback: serde_json::Value = serde_json::from_str(readback).unwrap();
    assert_eq!(readback["composition"]["id"], 1);
    for (case_index, op) in [
        BooleanOp::Subtract,
        BooleanOp::Intersect,
        BooleanOp::Exclude,
    ]
    .into_iter()
    .enumerate()
    {
        let native = &readback["cases"][case_index]["contents"];
        let compound_index = usize::from(op == BooleanOp::Subtract);
        let native_compound = &native[compound_index];
        assert_eq!(native_compound["matchName"], "ADBE Vector Group");
        assert_eq!(native_compound["contents"].as_array().unwrap().len(), 4);
        assert_eq!(native_compound["contents"][3]["mode"], 1);
        assert_eq!(native[2]["mode"], boolean_ordinal(op));
        let entries = boolean_entries(
            op,
            &[
                VectorGeometry::Path(compound_path()),
                VectorGeometry::Ellipse(ShapeEllipse::default()),
            ],
            super::super::keyframes::PropertyClock::DEFAULT,
        )
        .unwrap();
        assert_eq!(
            entries.len(),
            native.as_array().unwrap().len() - 1,
            "retain the independently authored operand scopes, excluding its final Fill: {op:?}"
        );
        assert_eq!(entries[compound_index].0, "ADBE Vector Group");
        assert_eq!(entries[1 - compound_index].0, "ADBE Vector Shape - Ellipse");
        assert_eq!(entries[2].0, "ADBE Vector Filter - Merge");
        let contents = entries[compound_index].1.children().unwrap();
        assert_eq!(
            native_numeric(contents, "ADBE Vector Merge Type")
                .unwrap()
                .values,
            vec![1.0],
            "Merge (not Add) retains the compound operand's contour winding"
        );
    }
}

#[test]
fn compound_boolean_operand_wrapper_respects_depth_limit() {
    let operands = [VectorGeometry::Path(compound_path())];
    for (max_depth, valid) in [(0, false), (1, true)] {
        let mut count = 0;
        let result = validate_operands(&operands, 0, &mut count, max_depth);
        assert_eq!(result.is_ok(), valid);
        if valid {
            assert_eq!(count, 4, "one Group and its three contours");
        }
    }
    let single = super::super::path_geometry::contours(&compound_path())
        .unwrap()
        .remove(0);
    let mut count = 0;
    validate_operands(&[VectorGeometry::Path(single)], 0, &mut count, 0).unwrap();
    assert_eq!(count, 1, "a single contour needs no wrapper");
}

#[test]
fn aggregate_vector_contour_and_boolean_counts_exceed_old_policy_boundary() {
    const COUNT: usize = 10_001;

    let program = program_spec(
        (0..COUNT)
            .map(|_| VectorContent::Merge(BooleanOp::Union))
            .collect(),
    );
    validate_program(&program).expect("vector entries above the old policy limit");

    let operands = vec![VectorGeometry::Ellipse(ShapeEllipse::default()); COUNT];
    let mut count = 0;
    validate_operands(&operands, 0, &mut count, 48)
        .expect("Boolean operands above the old policy limit");
    assert_eq!(count, COUNT);

    let mut commands = Vec::with_capacity(COUNT * 2);
    for index in 0..COUNT {
        let x = index as f64;
        commands.push(ShapePathCommand::MoveTo {
            x,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        });
        commands.push(ShapePathCommand::LineTo {
            x,
            y: 1.0,
            mirror: None,
            corner_radius: None,
        });
    }
    let contours = super::super::path_geometry::validated_contours(&ShapePath { commands })
        .expect("contours above the old policy limit");
    assert_eq!(contours.len(), COUNT);
}

#[test]
fn compound_path_direct_and_nested_boolean_consumers_emit_every_contour() {
    let direct = roundtrip_boolean(
        BooleanOp::Union,
        vec![
            VectorGeometry::Path(compound_path()),
            VectorGeometry::Ellipse(ShapeEllipse::default()),
        ],
    );
    assert_eq!(direct.op, BooleanOp::Union);

    let nested = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Nested Compound Boolean".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::Boolean(VectorBooleanSpec {
            appearance: appearance(),
            op: BooleanOp::Subtract,
            operands: vec![
                VectorGeometry::Boolean {
                    op: BooleanOp::Union,
                    operands: vec![
                        VectorGeometry::Path(compound_path()),
                        VectorGeometry::Rect {
                            size: [50.0, 50.0],
                            position: [20.0, 20.0],
                            roundness: 0.0,
                        },
                    ],
                },
                VectorGeometry::Ellipse(ShapeEllipse::default()),
            ],
            stroke_animations: Default::default(),
            animations: TransformAnimations::default(),
        })],
    )
    .expect("nested Boolean should preserve each compound Path as one operand");
    assert_eq!(count_bytes(&nested, b"ADBE Vector Shape - Group"), 3);
    assert_eq!(count_bytes(&nested, b"ADBE Vector Filter - Merge"), 3);
}

#[test]
fn fresh_merge_paths_roundtrips_ordinals_and_subtract_order() {
    let operands = vec![
        VectorGeometry::Ellipse(ShapeEllipse {
            size: [80.0, 60.0],
            position: [10.0, 0.0],
            reversed: false,
        }),
        VectorGeometry::Ellipse(ShapeEllipse {
            size: [40.0, 30.0],
            position: [90.0, 0.0],
            reversed: false,
        }),
    ];
    for op in [
        BooleanOp::Union,
        BooleanOp::Subtract,
        BooleanOp::Intersect,
        BooleanOp::Exclude,
    ] {
        let actual = roundtrip_boolean(op, operands.clone());
        assert_eq!(actual.op, op);
        assert_eq!(actual.fills.len(), 1);
        let positions: Vec<_> = actual
            .layers
            .iter()
            .map(|layer| match layer.data() {
                fx_schema::LayerData::Shape(shape) => {
                    shape
                        .shape
                        .ellipse
                        .as_ref()
                        .expect("Ellipse operand")
                        .position
                }
                _ => panic!("Shape operand"),
            })
            .collect();
        assert_eq!(positions, vec![[10.0, 0.0], [90.0, 0.0]]);
    }
}

#[test]
fn fresh_merge_paths_accepts_rect_and_nested_boolean_operands() {
    let actual = roundtrip_boolean(
        BooleanOp::Union,
        vec![
            VectorGeometry::Rect {
                size: [100.0, 40.0],
                position: [12.0, 20.0],
                roundness: 8.0,
            },
            VectorGeometry::Boolean {
                op: BooleanOp::Exclude,
                operands: vec![
                    VectorGeometry::Ellipse(ShapeEllipse::default()),
                    VectorGeometry::PolyStar(ShapePolyStar::default()),
                ],
            },
        ],
    );
    assert_eq!(actual.layers.len(), 2);
    let fx_schema::LayerData::Rect(rect) = actual.layers[0].data() else {
        panic!("Rectangle imports as an editable typed Rect inside Boolean geometry")
    };
    assert_eq!(rect.rect.size, [100.0, 40.0]);
    let fx_schema::Position::TwoD(position) = rect.transform.position else {
        panic!("Rectangle position remains editable in two dimensions")
    };
    assert_eq!(
        [
            rect.rect.position[0] - rect.transform.anchor_point[0] + position[0],
            rect.rect.position[1] - rect.transform.anchor_point[1] + position[1],
        ],
        [12.0, 20.0]
    );
    assert_eq!(rect.rect.roundness, 8.0);
    let fx_schema::LayerData::BooleanOperation(nested) = actual.layers[1].data() else {
        panic!("nested Boolean operand")
    };
    assert_eq!(nested.op, BooleanOp::Exclude);
    assert_eq!(nested.layers.len(), 2);
}

#[test]
fn empty_and_invalid_boolean_operands_are_rejected() {
    let empty = VectorBooleanSpec {
        appearance: appearance(),
        op: BooleanOp::Union,
        operands: Vec::new(),
        stroke_animations: Default::default(),
        animations: TransformAnimations::default(),
    };
    assert!(validate_boolean(&empty).is_err());

    let invalid_rect = VectorBooleanSpec {
        operands: vec![VectorGeometry::Rect {
            size: [0.0, 40.0],
            position: [0.0, 0.0],
            roundness: 0.0,
        }],
        ..empty
    };
    assert!(validate_boolean(&invalid_rect).is_err());
}

#[test]
fn animated_geometry_cannot_bypass_static_native_bounds() {
    let track = |values: Vec<f64>| NumericTrack {
        keys: vec![super::super::NumericKeyframe {
            time_millis: 0,
            easing: vec![super::super::KeyframeEasing::Linear; values.len()],
            values,
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        }],
    };
    let star = VectorShapeSpec {
        appearance: appearance(),
        geometry: VectorGeometry::PolyStar(ShapePolyStar::default()),
        stroke_animations: Default::default(),
        animations: TransformAnimations::default(),
        geometry_animations: GeometryAnimations {
            star_points: Some(track(vec![1001.0])),
            ..GeometryAnimations::default()
        },
    };
    assert!(validate(&star).is_err());
    let ellipse = VectorShapeSpec {
        appearance: appearance(),
        geometry: VectorGeometry::Ellipse(ShapeEllipse::default()),
        stroke_animations: Default::default(),
        animations: TransformAnimations::default(),
        geometry_animations: GeometryAnimations {
            ellipse_size: Some(track(vec![80.0, -1.0])),
            ..GeometryAnimations::default()
        },
    };
    assert!(validate(&ellipse).is_err());
}

fn program_spec(contents: Vec<VectorContent>) -> VectorLayerSpec {
    VectorLayerSpec {
        name: "Ordered Vector Program".into(),
        transform: appearance().transform,
        transform_animations: TransformAnimations::default(),
        contents,
    }
}

fn native_run<'a>(chunks: &'a [Chunk], target: &str) -> Option<&'a [Chunk]> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                return Some(run);
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| native_run(children, target))
}

fn native_property<'a>(chunks: &'a [Chunk], target: &str) -> Option<&'a [Chunk]> {
    properties::unique_list(native_run(chunks, target)?, *b"tdbs").ok()
}

fn native_numeric(chunks: &[Chunk], target: &str) -> Option<properties::NumericProperty> {
    properties::read_numeric(native_property(chunks, target)?).ok()
}

fn gradient_xml(chunks: &[Chunk]) -> &str {
    let storage = properties::unique_list(chunks, *b"GCst").expect("gradient storage");
    let values = properties::unique_list(storage, *b"GCky").expect("gradient values");
    let bytes = values
        .iter()
        .find(|chunk| chunk.id() == *b"Utf8")
        .and_then(Chunk::data_payload)
        .expect("gradient XML");
    std::str::from_utf8(bytes).expect("UTF-8 gradient XML")
}

#[test]
fn gradient_writer_matches_pinned_native_plist_grammar_and_roundtrips_alpha() {
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/shapes/gradient.aep"
    ))
    .expect("pinned native gradient");
    let ItemKind::Composition(composition) = &source.item(1).expect("composition").kind else {
        panic!("pinned composition")
    };
    let native = native_run(&composition.layers[0].content, "ADBE Vector Grad Colors")
        .expect("pinned native gradient Colors");
    let stops = [
        ShapeGradientStop {
            offset: 0.1,
            color: [0.2, 0.3, 0.4, 0.5],
        },
        ShapeGradientStop {
            offset: 0.9,
            color: [0.8, 0.7, 0.6, 0.25],
        },
    ];
    let generated = super::gradient_colors(&stops).expect("generated gradient Colors");
    for xml in [
        gradient_xml(native),
        gradient_xml(std::slice::from_ref(&generated)),
    ] {
        for marker in [
            "<prop.map version='4'>",
            "<key>Alpha Stops</key>",
            "<key>Color Stops</key>",
            "<key>Stops List</key>",
            "<key>Stops Size</key>",
            "<prop.pair>",
            "<array.type><float/></array.type>",
        ] {
            assert!(xml.contains(marker), "missing {marker}");
        }
    }
    let decoded = crate::structure_document::shapes::gradient::decode(
        std::slice::from_ref(&generated),
        1,
        [0.0, 0.0],
        [1.0, 0.0],
    )
    .expect("generated gradient decodes");
    let ShapePaint::Gradient { stops: actual, .. } = decoded.paint else {
        panic!("generated paint remains gradient")
    };
    assert_eq!(actual, stops);
}

#[test]
fn pinned_source_native_frames_have_vector_property_records() {
    let cases = [
        ("geometry_ellipse_size.aep", "ADBE Vector Ellipse Size"),
        (
            "geometry_ellipse_position.aep",
            "ADBE Vector Ellipse Position",
        ),
        ("paint_fill_color.aep", "ADBE Vector Fill Color"),
        ("paint_fill_opacity.aep", "ADBE Vector Fill Opacity"),
        ("paint_stroke_width.aep", "ADBE Vector Stroke Width"),
        ("paint_stroke_cap.aep", "ADBE Vector Stroke Line Cap"),
        ("paint_stroke_miter.aep", "ADBE Vector Stroke Miter Limit"),
        ("geometry_rect_roundness.aep", "ADBE Vector Rect Roundness"),
        ("geometry_star_points.aep", "ADBE Vector Star Points"),
        (
            "geometry_star_outer_radius.aep",
            "ADBE Vector Star Outer Radius",
        ),
        (
            "geometry_star_roundness.aep",
            "ADBE Vector Star Outer Roundess",
        ),
        (
            "modifier_round_radius.aep",
            "ADBE Vector RoundCorner Radius",
        ),
        ("modifier_offset_amount.aep", "ADBE Vector Offset Amount"),
        ("modifier_offset_join.aep", "ADBE Vector Offset Line Join"),
        ("modifier_trim_start.aep", "ADBE Vector Trim Start"),
        ("modifier_trim_offset_v2.aep", "ADBE Vector Trim Offset"),
        ("vector_transform_anchor.aep", "ADBE Vector Anchor"),
        ("vector_transform_position.aep", "ADBE Vector Position"),
        ("vector_transform_scale.aep", "ADBE Vector Scale"),
        ("vector_transform_rotation.aep", "ADBE Vector Rotation"),
        ("vector_transform_skew.aep", "ADBE Vector Skew"),
        ("vector_transform_skew_axis.aep", "ADBE Vector Skew Axis"),
        ("vector_transform_opacity.aep", "ADBE Vector Group Opacity"),
    ];
    for (fixture, property) in cases {
        let path = format!(
            "{}/tests/fixtures/pr4442_native/sources/{fixture}",
            env!("CARGO_MANIFEST_DIR")
        );
        let project = read_project(&std::fs::read(path).unwrap()).unwrap();
        let storage = project
            .items
            .iter()
            .find_map(|item| {
                let ItemKind::Composition(composition) = &item.kind else {
                    return None;
                };
                composition
                    .layers
                    .iter()
                    .find_map(|layer| native_property(&layer.content, property))
            })
            .unwrap();
        let descriptor = properties::data(storage, *b"tdb4").unwrap();
        let flags = properties::data(storage, *b"tdsb").unwrap();
        let bounds = [*b"tdum", *b"tduM"].map(|id| {
            properties::data(storage, id)
                .ok()
                .map(|v| f64::from_be_bytes(v.try_into().unwrap()))
        });
        let list = storage
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"list"))
            .and_then(Chunk::children);
        let stride = list
            .and_then(|children| children.iter().find(|chunk| chunk.id() == *b"lhd3"))
            .and_then(Chunk::data_payload)
            .map(|v| u16::from_be_bytes([v[18], v[19]]));
        assert_eq!(flags, 1_u32.to_be_bytes());
        assert_eq!(descriptor.len(), 124);
        assert_eq!(descriptor[68] != 0, list.is_some());
        assert!(bounds[0].is_some() == bounds[1].is_some());
        assert_eq!(stride.is_some(), list.is_some());
    }
}

#[test]
fn review_writer_fill_opacity_animation_is_encoded_on_native_fill() {
    let spec = program_spec(vec![
        ellipse_content(),
        VectorContent::Paint(VectorPaintSpec::Fill {
            paint: ShapePaint::Solid {
                color: [0.25, 0.5, 0.75, 1.0],
            },
            fill_rule: ShapeFillRule::NonZeroWinding,
            blend_mode: BlendMode::Normal,
            opacity: 25.0,
            animations: VectorPaintAnimations {
                color: Some(rect_track(&[
                    (0, &[0.25, 0.5, 0.75, 1.0]),
                    (500, &[0.75, 0.5, 0.25, 1.0]),
                ])),
                opacity: Some(rect_track(&[(0, &[20.0]), (500, &[70.0])])),
                ..Default::default()
            },
        }),
    ]);
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Animated Fill Opacity".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::VectorProgram(spec)],
    )
    .expect("fresh animated Fill AEP");
    let project = read_project(&bytes).expect("fresh project parses");
    let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    let opacity = native_numeric(
        &composition.layers.first().expect("Fill layer").content,
        "ADBE Vector Fill Opacity",
    )
    .expect("native Fill opacity");

    assert!(
        opacity.animated,
        "Fill opacity track must not become static"
    );
    assert_eq!(opacity.keyframes.len(), 2);
    for (key, (time, value)) in opacity.keyframes.iter().zip([(0.0, 20.0), (0.5, 70.0)]) {
        assert_eq!(key.time_secs, time);
        assert_eq!(key.values, vec![value]);
        assert_eq!((key.in_interpolation, key.out_interpolation), (1, 1));
    }
    let color_storage = native_property(
        &composition.layers.first().expect("Fill layer").content,
        "ADBE Vector Fill Color",
    )
    .expect("native Fill color");
    let descriptor = properties::data(color_storage, *b"tdb4").unwrap();
    assert_eq!(u16::from_be_bytes([descriptor[4], descriptor[5]]), 6);
    assert_eq!(
        u32::from_be_bytes(descriptor[8..12].try_into().unwrap()),
        0x2ffff
    );
    let list = color_storage
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"list"))
        .and_then(Chunk::children)
        .unwrap();
    let header = properties::data(list, *b"lhd3").unwrap();
    assert_eq!(u16::from_be_bytes([header[18], header[19]]), 152);
    let color = properties::read_numeric(color_storage).expect("decodable native color keys");
    assert_eq!(color.keyframes[0].time_secs, 0.0);
    assert_eq!(color.keyframes[1].time_secs, 0.5);
    assert_eq!(color.keyframes[0].values, vec![0.25, 0.5, 0.75, 1.0]);
}

#[test]
fn fill_without_opacity_animation_remains_static() {
    let spec = program_spec(vec![
        ellipse_content(),
        VectorContent::Paint(VectorPaintSpec::Fill {
            paint: ShapePaint::Solid {
                color: [0.25, 0.5, 0.75, 1.0],
            },
            fill_rule: ShapeFillRule::NonZeroWinding,
            blend_mode: BlendMode::Normal,
            opacity: 25.0,
            animations: VectorPaintAnimations::default(),
        }),
    ]);
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Static Fill Opacity".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::VectorProgram(spec)],
    )
    .expect("fresh static Fill AEP");
    let project = read_project(&bytes).expect("fresh project parses");
    let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    let opacity = native_numeric(
        &composition.layers.first().expect("Fill layer").content,
        "ADBE Vector Fill Opacity",
    )
    .expect("native Fill opacity");

    assert!(!opacity.animated);
    assert_eq!(opacity.values, vec![25.0]);
    assert!(opacity.keyframes.is_empty());
}

fn solid_fill() -> VectorContent {
    VectorContent::Paint(VectorPaintSpec::Fill {
        paint: ShapePaint::Solid {
            color: [0.2, 0.3, 0.4, 1.0],
        },
        fill_rule: ShapeFillRule::NonZeroWinding,
        blend_mode: BlendMode::Normal,
        opacity: 100.0,
        animations: VectorPaintAnimations::default(),
    })
}

fn gradient_fill() -> VectorContent {
    VectorContent::Paint(VectorPaintSpec::Fill {
        paint: ShapePaint::Gradient {
            gradient_type: ShapeGradientType::Linear,
            start: [0.0, 0.0],
            end: [100.0, 0.0],
            stops: vec![
                ShapeGradientStop {
                    offset: 0.0,
                    color: [1.0, 0.0, 0.0, 1.0],
                },
                ShapeGradientStop {
                    offset: 1.0,
                    color: [0.0, 0.0, 1.0, 1.0],
                },
            ],
        },
        fill_rule: ShapeFillRule::NonZeroWinding,
        blend_mode: BlendMode::Normal,
        opacity: 100.0,
        animations: VectorPaintAnimations::default(),
    })
}

fn solid_stroke() -> VectorContent {
    VectorContent::Paint(VectorPaintSpec::Stroke {
        paint: ShapePaint::Solid {
            color: [1.0, 0.5, 0.0, 1.0],
        },
        blend_mode: BlendMode::Normal,
        opacity: 100.0,
        width: 7.26,
        cap: ShapeLineCap::Butt,
        join: ShapeLineJoin::Miter,
        miter_limit: 4.0,
        dashes: Default::default(),
        animations: VectorPaintAnimations::default(),
    })
}

fn gradient_stroke() -> VectorContent {
    let VectorContent::Paint(VectorPaintSpec::Fill { paint, .. }) = gradient_fill() else {
        unreachable!("gradient_fill returns a Fill")
    };
    VectorContent::Paint(VectorPaintSpec::Stroke {
        paint,
        blend_mode: BlendMode::Normal,
        opacity: 100.0,
        width: 7.26,
        cap: ShapeLineCap::Butt,
        join: ShapeLineJoin::Miter,
        miter_limit: 4.0,
        dashes: Default::default(),
        animations: VectorPaintAnimations::default(),
    })
}

fn encoded_paint_orders(contents: &[VectorContent]) -> Vec<(&'static str, f64)> {
    let mut ordinal = 1;
    let encoded = encode_contents(
        contents,
        &mut ordinal,
        super::super::keyframes::PropertyClock::DEFAULT,
    )
    .expect("valid native contents");
    encoded
        .into_iter()
        .filter(|(name, _)| name.starts_with("ADBE Vector Graphic - "))
        .map(|(name, chunk)| {
            let order = native_numeric(std::slice::from_ref(&chunk), "ADBE Vector Composite Order")
                .expect("paint Composite Order")
                .values[0];
            (name, order)
        })
        .collect()
}

#[test]
fn native_paint_stack_preserves_fill_then_stroke_in_each_scope() {
    let geometry = ellipse_content();
    for paints in [
        vec![solid_fill(), solid_stroke()],
        vec![gradient_fill(), gradient_stroke()],
        vec![
            solid_fill(),
            gradient_fill(),
            solid_stroke(),
            gradient_stroke(),
        ],
    ] {
        let mut contents = vec![geometry.clone()];
        contents.extend(paints);
        let orders = encoded_paint_orders(&contents);
        assert_eq!(orders[0].1, 1.0, "first paint may use Below Previous");
        assert!(
            orders.iter().skip(1).all(|(_, order)| *order == 2.0),
            "later paints must stack above earlier paints: {orders:?}"
        );
        assert!(
            orders
                .iter()
                .take_while(|(name, _)| name.contains("Fill"))
                .count()
                >= 1
        );
    }
}

#[test]
fn nested_paint_scopes_reset_without_moving_geometry_modifiers_or_groups() {
    let group = VectorContent::Group(VectorGroupSpec {
        name: "Nested".into(),
        blend_mode: BlendMode::Normal,
        transform: VectorGroupTransform::default(),
        contents: vec![ellipse_content(), gradient_fill(), solid_stroke()],
    });
    let contents = vec![
        ellipse_content(),
        solid_fill(),
        VectorContent::Modifier(VectorModifierSpec::RoundCorners {
            value: ShapeRoundCorners {
                radius: fx_schema::NonNegativeProperty::new(6.0).unwrap(),
            },
            radius: None,
        }),
        solid_stroke(),
        group,
        solid_fill(),
    ];
    let mut ordinal = 1;
    let encoded = encode_contents(
        &contents,
        &mut ordinal,
        super::super::keyframes::PropertyClock::DEFAULT,
    )
    .expect("nested native contents");
    let names: Vec<_> = encoded.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        [
            "ADBE Vector Shape - Ellipse",
            "ADBE Vector Graphic - Fill",
            "ADBE Vector Filter - RC",
            "ADBE Vector Graphic - Stroke",
            "ADBE Vector Group",
            "ADBE Vector Graphic - Fill",
        ]
    );
    assert_eq!(
        encoded_paint_orders(&contents),
        [
            ("ADBE Vector Graphic - Fill", 1.0),
            ("ADBE Vector Graphic - Stroke", 2.0),
            ("ADBE Vector Graphic - Fill", 1.0),
        ]
    );
    let nested = encoded[4].1.children().expect("nested group");
    assert_eq!(
        native_numeric(nested, "ADBE Vector Composite Order")
            .unwrap()
            .values,
        vec![1.0]
    );
}

#[test]
fn fresh_program_emits_ordered_gradient_paints_modifiers_and_nested_transform() {
    let gradient = ShapePaint::Gradient {
        gradient_type: ShapeGradientType::Linear,
        start: [0.0, 0.0],
        end: [200.0, 0.0],
        stops: vec![
            ShapeGradientStop {
                offset: 0.0,
                color: [1.0, 0.0, 0.0, 1.0],
            },
            ShapeGradientStop {
                offset: 1.0,
                color: [0.0, 0.0, 1.0, 0.5],
            },
        ],
    };
    let spec = program_spec(vec![VectorContent::Group(VectorGroupSpec {
        name: "Operand".into(),
        blend_mode: BlendMode::Multiply,
        transform: VectorGroupTransform {
            position: [12.0, 24.0],
            skew: 8.0,
            skew_axis: 30.0,
            ..Default::default()
        },
        contents: vec![
            VectorContent::Geometry {
                geometry: VectorGeometry::Ellipse(ShapeEllipse::default()),
                animations: GeometryAnimations::default(),
            },
            VectorContent::Modifier(VectorModifierSpec::RoundCorners {
                value: ShapeRoundCorners {
                    radius: fx_schema::NonNegativeProperty::new(6.0).unwrap(),
                },
                radius: None,
            }),
            VectorContent::Paint(VectorPaintSpec::Fill {
                paint: gradient,
                fill_rule: ShapeFillRule::EvenOdd,
                blend_mode: BlendMode::Screen,
                opacity: 75.0,
                animations: VectorPaintAnimations::default(),
            }),
            VectorContent::Paint(VectorPaintSpec::Stroke {
                paint: ShapePaint::Solid {
                    color: [0.0, 1.0, 0.0, 1.0],
                },
                blend_mode: BlendMode::Normal,
                opacity: 100.0,
                width: 3.0,
                cap: ShapeLineCap::Round,
                join: ShapeLineJoin::Bevel,
                miter_limit: 4.0,
                dashes: super::super::StrokeDashes::new([8.0, 4.0], -3.0).unwrap(),
                animations: VectorPaintAnimations::default(),
            }),
        ],
    })]);
    validate_program(&spec).unwrap();
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Program".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::VectorProgram(spec)],
    )
    .unwrap();
    for name in [
        "ADBE Vector Graphic - G-Fill",
        "ADBE Vector Grad Colors",
        "ADBE Vector Filter - RC",
        "ADBE Vector Skew",
        "ADBE Vector Stroke Offset",
    ] {
        assert!(
            bytes
                .windows(name.len())
                .any(|window| window == name.as_bytes())
        );
    }
}

fn gradient_program(gradient_type: ShapeGradientType) -> VectorLayerSpec {
    program_spec(vec![
        VectorContent::Geometry {
            geometry: VectorGeometry::Ellipse(ShapeEllipse::default()),
            animations: GeometryAnimations::default(),
        },
        VectorContent::Paint(VectorPaintSpec::Fill {
            paint: ShapePaint::Gradient {
                gradient_type,
                start: [0.0, 0.0],
                end: [100.0, 0.0],
                stops: vec![
                    ShapeGradientStop {
                        offset: 0.0,
                        color: [0.0, 0.0, 0.0, 1.0],
                    },
                    ShapeGradientStop {
                        offset: 1.0,
                        color: [1.0, 1.0, 1.0, 1.0],
                    },
                ],
            },
            fill_rule: ShapeFillRule::NonZeroWinding,
            blend_mode: BlendMode::Normal,
            opacity: 100.0,
            animations: VectorPaintAnimations::default(),
        }),
    ])
}

#[test]
fn reflected_gradient_is_fresh_native_linear_with_mirrored_stops() {
    let spec = gradient_program(ShapeGradientType::Reflected);
    assert!(timeline_program(&spec, 13, Duration24::from_frames(24).unwrap()).is_ok());
}

#[test]
fn conic_gradient_without_exact_native_mapping_is_rejected() {
    let spec = gradient_program(ShapeGradientType::Conic);
    assert!(timeline_program(&spec, 13, Duration24::from_frames(24).unwrap()).is_err());
}

fn group_track(values: &[f64]) -> NumericTrack {
    NumericTrack {
        keys: vec![super::super::NumericKeyframe {
            time_millis: 0,
            values: values.to_vec(),
            easing: values
                .iter()
                .map(|_| super::super::KeyframeEasing::Linear)
                .collect(),
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        }],
    }
}

fn ellipse_content() -> VectorContent {
    VectorContent::Geometry {
        geometry: VectorGeometry::Ellipse(ShapeEllipse::default()),
        animations: GeometryAnimations::default(),
    }
}

#[test]
fn thirty_fps_vector_descendants_share_descriptor_and_key_clock() {
    let mut position = group_track(&[3.0, 4.0]);
    position.keys[0].time_millis = 1100;
    let spec = program_spec(vec![VectorContent::AnimatedGroup(
        VectorGroupSpec {
            name: "30 fps group".into(),
            blend_mode: BlendMode::Normal,
            transform: VectorGroupTransform::default(),
            contents: vec![VectorContent::Geometry {
                geometry: VectorGeometry::Ellipse(ShapeEllipse::default()),
                animations: GeometryAnimations {
                    ellipse_size: Some(group_track(&[80.0, 40.0])),
                    ..Default::default()
                },
            }],
        },
        VectorGroupAnimations {
            position: Some(position),
            ..Default::default()
        },
    )]);
    let clock = super::super::keyframes::PropertyClock::for_rate(
        crate::timing::FrameRate::new(30.0).unwrap(),
    )
    .unwrap();
    let timeline =
        timeline_program_with_clock(&spec, 13, Duration24::from_frames(60).unwrap(), clock)
            .unwrap();
    for (property, expected_time) in [
        ("ADBE Vector Position", 33792),
        ("ADBE Vector Ellipse Size", 0),
    ] {
        let storage = native_property(std::slice::from_ref(&timeline), property).unwrap();
        let descriptor = properties::data(storage, *b"tdb4").unwrap();
        assert_eq!(
            u32::from_be_bytes(descriptor[12..16].try_into().unwrap()),
            30_720
        );
        let list = properties::unique_list(storage, *b"list").unwrap();
        let data = properties::data(list, *b"ldat").unwrap();
        assert_eq!(
            i32::from_be_bytes(data[..4].try_into().unwrap()),
            expected_time
        );
    }
    let gradient = timeline_program_with_clock(
        &gradient_program(ShapeGradientType::Linear),
        14,
        Duration24::from_frames(60).unwrap(),
        clock,
    )
    .unwrap();
    for (timeline, names) in [
        (&timeline, &["ADBE Vector Blend Mode"][..]),
        (
            &gradient,
            &[
                "ADBE Vector Grad Type",
                "ADBE Vector Grad Start Pt",
                "ADBE Vector Grad End Pt",
                "ADBE Vector Fill Opacity",
            ][..],
        ),
    ] {
        for name in names {
            let storage = native_property(std::slice::from_ref(timeline), name).unwrap();
            let descriptor = properties::data(storage, *b"tdb4").unwrap();
            assert_eq!(
                u32::from_be_bytes(descriptor[12..16].try_into().unwrap()),
                30_720,
                "{name}"
            );
        }
    }
    let path = timeline_program_with_clock(
        &program_spec(vec![VectorContent::Geometry {
            geometry: VectorGeometry::Path(compound_path()),
            animations: GeometryAnimations::default(),
        }]),
        15,
        Duration24::from_frames(60).unwrap(),
        clock,
    )
    .unwrap();
    fn path_clocks(chunk: &crate::rifx::Chunk, output: &mut Vec<u32>) {
        if chunk.id() == *b"tdb4" {
            let bytes = chunk.data_payload().unwrap();
            if bytes[56..60] == 0x10008_u32.to_be_bytes() {
                output.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
            }
        }
        if let Some(children) = chunk.children() {
            for child in children {
                path_clocks(child, output);
            }
        }
    }
    let mut clocks = Vec::new();
    path_clocks(&path, &mut clocks);
    assert_eq!(clocks, vec![30_720; 3]);
}

#[test]
fn animated_group_writes_all_seven_owned_transform_families() {
    let animations = VectorGroupAnimations {
        anchor: Some(group_track(&[1.0, 2.0])),
        position: Some(group_track(&[3.0, 4.0])),
        scale: Some(group_track(&[125.0, 80.0])),
        rotation: Some(group_track(&[15.0])),
        skew: Some(group_track(&[8.0])),
        skew_axis: Some(group_track(&[30.0])),
        opacity: Some(group_track(&[75.0])),
    };
    let spec = program_spec(vec![VectorContent::AnimatedGroup(
        VectorGroupSpec {
            name: "Animated operand".into(),
            blend_mode: BlendMode::Normal,
            transform: VectorGroupTransform::default(),
            contents: vec![ellipse_content()],
        },
        animations,
    )]);

    validate_program(&spec).unwrap();
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Animated groups".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::VectorProgram(spec)],
    )
    .unwrap();
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
        panic!("fresh composition")
    };
    let content = &composition.layers.first().unwrap().content;
    let position_storage = native_property(content, "ADBE Vector Position").unwrap();
    let descriptor = properties::data(position_storage, *b"tdb4").unwrap();
    assert_eq!(u16::from_be_bytes([descriptor[4], descriptor[5]]), 14);
    let list = position_storage
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"list"))
        .and_then(Chunk::children)
        .unwrap();
    let header = properties::data(list, *b"lhd3").unwrap();
    assert_eq!(u16::from_be_bytes([header[18], header[19]]), 104);
    let position = properties::read_numeric(position_storage).unwrap();
    assert_eq!(position.keyframes[0].values, [3.0, 4.0]);
    assert_eq!(position.keyframes[0].spatial_in, [0.0, 0.0]);
    assert_eq!(position.keyframes[0].spatial_out, [0.0, 0.0]);

    for property in [
        "ADBE Vector Anchor",
        "ADBE Vector Position",
        "ADBE Vector Scale",
        "ADBE Vector Rotation",
        "ADBE Vector Skew",
        "ADBE Vector Skew Axis",
        "ADBE Vector Group Opacity",
    ] {
        assert!(
            bytes
                .windows(property.len())
                .any(|window| window == property.as_bytes())
        );
    }
}

#[test]
fn nested_animated_boolean_operand_groups_keep_tracks_on_their_owners() {
    let inner = VectorContent::AnimatedGroup(
        VectorGroupSpec {
            name: "Boolean operand".into(),
            blend_mode: BlendMode::Normal,
            transform: VectorGroupTransform::default(),
            contents: vec![VectorContent::Geometry {
                geometry: VectorGeometry::Boolean {
                    op: fx_schema::layer::BooleanOp::Union,
                    operands: vec![VectorGeometry::Ellipse(ShapeEllipse::default())],
                },
                animations: GeometryAnimations::default(),
            }],
        },
        VectorGroupAnimations {
            rotation: Some(group_track(&[45.0])),
            ..Default::default()
        },
    );
    let outer = VectorContent::AnimatedGroup(
        VectorGroupSpec {
            name: "Nested FX group".into(),
            blend_mode: BlendMode::Normal,
            transform: VectorGroupTransform::default(),
            contents: vec![inner],
        },
        VectorGroupAnimations {
            position: Some(group_track(&[20.0, 40.0])),
            ..Default::default()
        },
    );
    let spec = program_spec(vec![outer]);

    validate_program(&spec).unwrap();
    assert!(timeline_program(&spec, 22, Duration24::from_frames(24).unwrap()).is_ok());
}

#[test]
fn animated_group_rejects_wrong_dimensions_bounds_and_key_limit() {
    let group = |animations| {
        program_spec(vec![VectorContent::AnimatedGroup(
            VectorGroupSpec {
                name: "Invalid animation".into(),
                blend_mode: BlendMode::Normal,
                transform: VectorGroupTransform::default(),
                contents: vec![ellipse_content()],
            },
            animations,
        )])
    };
    assert!(
        validate_program(&group(VectorGroupAnimations {
            position: Some(group_track(&[1.0])),
            ..Default::default()
        }))
        .is_err()
    );
    assert!(
        validate_program(&group(VectorGroupAnimations {
            opacity: Some(group_track(&[101.0])),
            ..Default::default()
        }))
        .is_err()
    );

    let key = group_track(&[0.0]).keys.remove(0);
    // Keys above the removed 10,000-key policy validate, and the writer encodes them
    // (keyframes::tests::numeric_keys_exceed_old_policy_boundary_but_retain_native_u16_limit).
    assert!(
        validate_program(&group(VectorGroupAnimations {
            rotation: Some(NumericTrack {
                keys: vec![key.clone(); 10_001],
            }),
            ..Default::default()
        }))
        .is_ok()
    );
    // The native u16 key count still bounds a track.
    assert!(
        validate_program(&group(VectorGroupAnimations {
            rotation: Some(NumericTrack {
                keys: vec![key; usize::from(u16::MAX) + 1],
            }),
            ..Default::default()
        }))
        .is_err()
    );
}

#[test]
fn fractional_star_is_rejected_before_emitting_aep() {
    let star = ShapePolyStar {
        points: 4.5,
        ..ShapePolyStar::default()
    };
    let spec = VectorShapeSpec {
        appearance: appearance(),
        geometry: VectorGeometry::PolyStar(star),
        stroke_animations: Default::default(),
        animations: TransformAnimations::default(),
        geometry_animations: GeometryAnimations::default(),
    };
    assert!(validate(&spec).is_err());
}

fn rect_track(keys: &[(i64, &[f64])]) -> NumericTrack {
    NumericTrack {
        keys: keys
            .iter()
            .map(|(time_millis, values)| super::super::NumericKeyframe {
                time_millis: *time_millis,
                values: values.to_vec(),
                easing: vec![super::super::KeyframeEasing::Linear; values.len()],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            })
            .collect(),
    }
}

#[test]
fn fresh_program_rect_emits_owned_size_center_and_roundness_keys() {
    let spec = program_spec(vec![
        VectorContent::Geometry {
            geometry: VectorGeometry::Rect {
                size: [80.0, 40.0],
                position: [3.0, -4.0],
                roundness: 2.0,
            },
            animations: GeometryAnimations {
                rect_size: Some(rect_track(&[(0, &[80.0, 40.0]), (500, &[140.0, 60.0])])),
                rect_position: Some(rect_track(&[(0, &[43.0, 16.0]), (500, &[73.0, 26.0])])),
                rect_roundness: Some(rect_track(&[(0, &[2.0]), (500, &[8.0])])),
                ..Default::default()
            },
        },
        VectorContent::Paint(VectorPaintSpec::Fill {
            paint: ShapePaint::Solid {
                color: [0.25, 0.5, 0.75, 1.0],
            },
            fill_rule: ShapeFillRule::NonZeroWinding,
            blend_mode: BlendMode::Normal,
            opacity: 100.0,
            animations: VectorPaintAnimations::default(),
        }),
    ]);
    validate_program(&spec).expect("animated native Rectangle program");
    let bytes = super::super::write_composition(
        &super::super::CompositionSpec {
            name: "Animated Rectangle".into(),
            width: 640,
            height: 480,
            duration_frames: 24,
        },
        &[super::super::LayerSpec::VectorProgram(spec)],
    )
    .expect("fresh animated Rectangle AEP");
    let project = read_project(&bytes).expect("fresh project parses");
    let imported = to_structural_fx_document(&project, Some(1)).expect("Rectangle reimports");
    fn rect_id(layers: &[fx_schema::Layer]) -> Option<fx_schema::LayerId> {
        layers.iter().find_map(|layer| match layer.data() {
            fx_schema::LayerData::Rect(rect) => Some(rect.id),
            fx_schema::LayerData::Group(group) => rect_id(&group.layers),
            _ => None,
        })
    }
    let owner = rect_id(imported.document.composition().layers()).expect("editable Rectangle");
    let values = |property| {
        imported
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|entry| entry.target == fx_schema::PropertyTarget::layer(owner, property))
            .expect("owned Rectangle track")
            .animator
            .keyframe_track()
            .expect("native keys")
            .keyframes()
            .iter()
            .map(|key| key.value().clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        values(fx_schema::PropType::RectSize),
        [
            fx_schema::PropertyValue::Vector2([80.0, 40.0]),
            fx_schema::PropertyValue::Vector2([140.0, 60.0]),
        ]
    );
    assert_eq!(
        values(fx_schema::PropType::PositionX),
        [
            fx_schema::PropertyValue::Float(43.0),
            fx_schema::PropertyValue::Float(73.0),
        ]
    );
    assert_eq!(
        values(fx_schema::PropType::PositionY),
        [
            fx_schema::PropertyValue::Float(16.0),
            fx_schema::PropertyValue::Float(26.0),
        ]
    );
    assert_eq!(
        values(fx_schema::PropType::RectRoundness),
        [
            fx_schema::PropertyValue::Float(2.0),
            fx_schema::PropertyValue::Float(8.0),
        ]
    );
}

#[test]
fn rect_program_rejects_tracks_owned_by_another_geometry_kind() {
    let invalid = program_spec(vec![VectorContent::Geometry {
        geometry: VectorGeometry::Rect {
            size: [80.0, 40.0],
            position: [3.0, -4.0],
            roundness: 2.0,
        },
        animations: GeometryAnimations {
            ellipse_size: Some(rect_track(&[(0, &[80.0, 40.0])])),
            ..Default::default()
        },
    }]);
    assert!(validate_program(&invalid).is_err());
}
