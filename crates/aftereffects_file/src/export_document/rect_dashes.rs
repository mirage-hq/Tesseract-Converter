//! Dashed FX rectangles lowered to the hard-corner contour actually consumed by FX.
//!
//! The parent exporter opts into this helper after building the current native
//! vector program. Keeping the parametric Rectangle and its non-stroke paints in
//! that program preserves their editability; only the dashed Stroke moves into
//! an isolated static Path group.

use fx_schema::{
    LayerId, PropType, RectLayer, ShapePath, ShapePathCommand, animator::AnimationGraphEntry,
};

use super::{ExportDiagnostic, NativeTrack};
use crate::writer::{
    GeometryAnimations, VectorContent, VectorGeometry, VectorGroupSpec, VectorGroupTransform,
    VectorLayerSpec, VectorPaintAnimations, VectorPaintSpec,
};

/// Result of isolating one dashed Rectangle Stroke from its parametric geometry.
///
/// The caller should append `diagnostics` to the export report and write
/// `program` in place of the input program. An unsupported contour omits only
/// the dashed Stroke; the Rectangle geometry and every sibling paint remain.
pub(super) struct RectDashContourOutput {
    pub program: VectorLayerSpec,
    pub diagnostics: Vec<ExportDiagnostic>,
}

/// Replace a dashed Rectangle Stroke with an isolated static Path + Stroke.
///
/// `program` must be the current Rectangle vector program and must already own
/// all non-stroke animation tracks. This helper assigns only Rectangle-owned
/// Stroke color/width/miter/join/dash-phase tracks. Call this before wrapping the
/// program in timing. A malformed program is an integration error; unsupported
/// FX semantics are instead diagnosed and omit only the dashed Stroke.
pub(super) fn isolate_static_dashed_stroke(
    layer: &RectLayer,
    dynamics: &[AnimationGraphEntry],
    mut program: VectorLayerSpec,
) -> Result<RectDashContourOutput, &'static str> {
    if !layer.rect.stroke_enabled || layer.rect.stroke_dashes.is_empty() {
        return Ok(RectDashContourOutput {
            program,
            diagnostics: Vec::new(),
        });
    }

    let stroke = take_rect_stroke(&mut program.contents)?;
    let eligibility = contour_eligibility(layer, dynamics);
    let stroke_animations = match rectangle_stroke_animations(layer.id, dynamics) {
        Ok(animations) => animations,
        Err(reason) => {
            return Ok(omitted_stroke(layer.id, program, reason));
        }
    };

    rewrite_program(
        layer.id,
        layer.rect.position,
        layer.rect.size,
        eligibility,
        stroke,
        stroke_animations,
        program,
    )
}

#[derive(Clone, Copy)]
enum ContourEligibility {
    HardCorner,
    Unsupported(&'static str),
}

fn contour_eligibility(layer: &RectLayer, dynamics: &[AnimationGraphEntry]) -> ContourEligibility {
    if layer.rect.roundness != 0.0 {
        return ContourEligibility::Unsupported(
            "rounded Rectangle dash contour is not exported because exact FX rounded-corner semantics have not been established for native Path encoding",
        );
    }
    if !constant_geometry_value(dynamics, layer.id, PropType::RectSize, |value| {
        super::vector_value(value).is_ok_and(|value| value == layer.rect.size)
    }) || !constant_geometry_value(dynamics, layer.id, PropType::RectRoundness, |value| {
        super::float_value(value).is_ok_and(|value| value == layer.rect.roundness)
    }) {
        return ContourEligibility::Unsupported(
            "animated or non-current Rectangle geometry cannot be frozen into a dashed static Path",
        );
    }
    let [x, y] = layer.rect.position;
    let [width, height] = layer.rect.size;
    if [x, y, width, height, x + width, y + height]
        .into_iter()
        .any(|value| !value.is_finite() || value.abs() > f32::MAX as f64)
    {
        return ContourEligibility::Unsupported(
            "Rectangle dash contour exceeds the finite native Path coordinate budget",
        );
    }
    ContourEligibility::HardCorner
}

fn constant_geometry_value(
    dynamics: &[AnimationGraphEntry],
    layer_id: LayerId,
    property: PropType,
    is_current: impl FnOnce(&fx_schema::PropertyValue) -> bool,
) -> bool {
    match super::track(dynamics, layer_id, property) {
        Ok(None) => true,
        Ok(Some(NativeTrack::Constant(value))) => is_current(value),
        Ok(Some(NativeTrack::Keyframes(_))) | Err(_) => false,
    }
}

fn rectangle_stroke_animations(
    layer_id: LayerId,
    dynamics: &[AnimationGraphEntry],
) -> Result<VectorPaintAnimations, &'static str> {
    let stroke = super::stroke_animations(dynamics, layer_id)?;
    Ok(VectorPaintAnimations {
        color: super::color_track(super::track(dynamics, layer_id, PropType::StrokeColor)?)?,
        opacity: None,
        width: stroke.width,
        miter_limit: stroke.miter_limit,
        join: stroke.join,
        dash_offset: super::scalar_track(
            super::track(dynamics, layer_id, PropType::StrokeDashOffset)?,
            1.0,
        )?,
    })
}

fn take_rect_stroke(contents: &mut Vec<VectorContent>) -> Result<VectorPaintSpec, &'static str> {
    let strokes: Vec<_> = contents
        .iter()
        .enumerate()
        .filter_map(|(index, content)| {
            matches!(
                content,
                VectorContent::Paint(VectorPaintSpec::Stroke { .. })
            )
            .then_some(index)
        })
        .collect();
    let [index] = strokes.as_slice() else {
        return Err("dashed Rectangle program must contain exactly one Stroke paint");
    };
    let VectorContent::Paint(stroke @ VectorPaintSpec::Stroke { .. }) = contents.remove(*index)
    else {
        return Err("selected Rectangle Stroke changed kind");
    };
    Ok(stroke)
}

fn rewrite_program(
    layer_id: LayerId,
    position: [f64; 2],
    size: [f64; 2],
    eligibility: ContourEligibility,
    mut stroke: VectorPaintSpec,
    stroke_animations: VectorPaintAnimations,
    mut program: VectorLayerSpec,
) -> Result<RectDashContourOutput, &'static str> {
    if let ContourEligibility::Unsupported(reason) = eligibility {
        return Ok(omitted_stroke(layer_id, program, reason));
    }
    let VectorPaintSpec::Stroke { animations, .. } = &mut stroke else {
        return Err("Rectangle dash contour received a non-Stroke paint");
    };
    animations.color = stroke_animations.color;
    animations.width = stroke_animations.width;
    animations.miter_limit = stroke_animations.miter_limit;
    animations.join = stroke_animations.join;
    animations.dash_offset = stroke_animations.dash_offset;
    program.contents.push(VectorContent::Group(VectorGroupSpec {
        name: "FX Rectangle dashed stroke contour".to_owned(),
        blend_mode: Default::default(),
        transform: VectorGroupTransform::default(),
        contents: vec![
            VectorContent::Geometry {
                geometry: VectorGeometry::Path(hard_corner_path(position, size)),
                animations: GeometryAnimations::default(),
            },
            VectorContent::Paint(stroke),
        ],
    }));
    Ok(RectDashContourOutput {
        program,
        diagnostics: vec![ExportDiagnostic {
            layer_id: Some(layer_id),
            message: "Dashed Rectangle Stroke was normalized to an explicit hard-corner Path using the FX stroke contour's top-left first point and right/down direction; the native Rectangle remains for sibling paints, but Rectangle geometry edits are no longer linked to the Stroke outline.".to_owned(),
        }],
    })
}

fn omitted_stroke(
    layer_id: LayerId,
    program: VectorLayerSpec,
    reason: &'static str,
) -> RectDashContourOutput {
    RectDashContourOutput {
        program,
        diagnostics: vec![ExportDiagnostic {
            layer_id: Some(layer_id),
            message: format!(
                "Dashed Rectangle Stroke was omitted while preserving the native Rectangle and sibling paints: {reason}."
            ),
        }],
    }
}

fn hard_corner_path([x, y]: [f64; 2], [width, height]: [f64; 2]) -> ShapePath {
    let linear = || (None, None);
    let (mirror, corner_radius) = linear();
    let mut commands = Vec::with_capacity(5);
    commands.push(ShapePathCommand::MoveTo {
        x,
        y,
        mirror,
        corner_radius,
    });
    let (mirror, corner_radius) = linear();
    commands.push(ShapePathCommand::LineTo {
        x: x + width,
        y,
        mirror,
        corner_radius,
    });
    let (mirror, corner_radius) = linear();
    commands.push(ShapePathCommand::LineTo {
        x: x + width,
        y: y + height,
        mirror,
        corner_radius,
    });
    let (mirror, corner_radius) = linear();
    commands.push(ShapePathCommand::LineTo {
        x,
        y: y + height,
        mirror,
        corner_radius,
    });
    commands.push(ShapePathCommand::Close);
    ShapePath { commands }
}

#[cfg(test)]
mod tests {
    use fx_schema::{
        BlendMode, ShapeFillRule, ShapeLineCap, ShapeLineJoin, ShapePaint, ShapePathCommand,
    };

    use super::*;
    use crate::writer::{
        NumericKeyframe, NumericTrack, SolidTransform, StrokeDashes, TransformAnimations,
    };

    fn source_program() -> VectorLayerSpec {
        VectorLayerSpec {
            name: "source Rect".to_owned(),
            transform: SolidTransform {
                anchor: [0.0; 2],
                position: [0.0; 2],
                scale: [100.0; 2],
                rotation: 0.0,
                opacity: 100.0,
            },
            transform_animations: TransformAnimations::default(),
            contents: vec![
                VectorContent::Geometry {
                    geometry: VectorGeometry::Rect {
                        size: [30.0, 20.0],
                        position: [10.0, 40.0],
                        roundness: 0.0,
                    },
                    animations: GeometryAnimations::default(),
                },
                VectorContent::Paint(VectorPaintSpec::Fill {
                    paint: ShapePaint::Solid {
                        color: [0.1, 0.2, 0.3, 1.0],
                    },
                    fill_rule: ShapeFillRule::NonZeroWinding,
                    blend_mode: BlendMode::default(),
                    opacity: 100.0,
                    animations: VectorPaintAnimations::default(),
                }),
                VectorContent::Paint(VectorPaintSpec::Stroke {
                    paint: ShapePaint::Solid {
                        color: [1.0, 0.0, 0.0, 1.0],
                    },
                    blend_mode: BlendMode::default(),
                    opacity: 100.0,
                    width: 3.0,
                    cap: ShapeLineCap::Butt,
                    join: ShapeLineJoin::Miter,
                    miter_limit: 4.0,
                    dashes: StrokeDashes::new([4.0, 2.0], 1.0).expect("valid test dash pair"),
                    animations: VectorPaintAnimations::default(),
                }),
            ],
        }
    }

    fn one_key(value: f64) -> NumericTrack {
        NumericTrack {
            keys: vec![NumericKeyframe {
                time_millis: 250,
                values: vec![value],
                easing: Vec::new(),
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            }],
        }
    }

    #[test]
    fn hard_corner_contour_uses_fx_first_point_and_direction() {
        let path = hard_corner_path([10.0, 40.0], [30.0, 20.0]);
        assert!(matches!(
            path.commands.as_slice(),
            [
                ShapePathCommand::MoveTo {
                    x: 10.0,
                    y: 40.0,
                    ..
                },
                ShapePathCommand::LineTo {
                    x: 40.0,
                    y: 40.0,
                    ..
                },
                ShapePathCommand::LineTo {
                    x: 40.0,
                    y: 60.0,
                    ..
                },
                ShapePathCommand::LineTo {
                    x: 10.0,
                    y: 60.0,
                    ..
                },
                ShapePathCommand::Close,
            ]
        ));
    }

    #[test]
    fn rewrite_retains_parametric_rect_and_fill() {
        let mut program = source_program();
        let stroke = take_rect_stroke(&mut program.contents).expect("source Stroke");
        let output = rewrite_program(
            LayerId::new(7),
            [10.0, 40.0],
            [30.0, 20.0],
            ContourEligibility::HardCorner,
            stroke,
            VectorPaintAnimations::default(),
            program,
        )
        .expect("hard-corner rewrite");
        assert!(matches!(
            output.program.contents.as_slice(),
            [
                VectorContent::Geometry {
                    geometry: VectorGeometry::Rect { .. },
                    ..
                },
                VectorContent::Paint(VectorPaintSpec::Fill { .. }),
                VectorContent::Group(_),
            ]
        ));
    }

    #[test]
    fn animated_dash_phase_is_owned_only_by_isolated_stroke() {
        let mut program = source_program();
        let stroke = take_rect_stroke(&mut program.contents).expect("source Stroke");
        let phase = one_key(9.0);
        let output = rewrite_program(
            LayerId::new(7),
            [10.0, 40.0],
            [30.0, 20.0],
            ContourEligibility::HardCorner,
            stroke,
            VectorPaintAnimations {
                dash_offset: Some(phase.clone()),
                ..Default::default()
            },
            program,
        )
        .expect("animated Stroke rewrite");
        let VectorContent::Group(group) = output.program.contents.last().expect("stroke group")
        else {
            panic!("last content is not isolated stroke group");
        };
        let [
            VectorContent::Geometry {
                animations: geometry,
                ..
            },
            VectorContent::Paint(VectorPaintSpec::Stroke { animations, .. }),
        ] = group.contents.as_slice()
        else {
            panic!("isolated group does not contain Path then Stroke");
        };
        assert_eq!(geometry, &GeometryAnimations::default());
        assert_eq!(animations.dash_offset.as_ref(), Some(&phase));
    }

    #[test]
    fn review_export_dashed_rect_preserves_stroke_enabled_opacity_track() {
        let mut program = source_program();
        let enabled_opacity = one_key(0.0);
        let VectorContent::Paint(VectorPaintSpec::Stroke { animations, .. }) =
            &mut program.contents[2]
        else {
            panic!("third content is the source Stroke")
        };
        animations.opacity = Some(enabled_opacity.clone());
        let stroke = take_rect_stroke(&mut program.contents).expect("source Stroke");
        let width = one_key(7.0);
        let dash_offset = one_key(9.0);
        let output = rewrite_program(
            LayerId::new(7),
            [10.0, 40.0],
            [30.0, 20.0],
            ContourEligibility::HardCorner,
            stroke,
            VectorPaintAnimations {
                width: Some(width.clone()),
                dash_offset: Some(dash_offset.clone()),
                ..Default::default()
            },
            program,
        )
        .expect("animated Stroke rewrite");
        let VectorContent::Group(group) = output.program.contents.last().expect("stroke group")
        else {
            panic!("last content is not isolated stroke group");
        };
        let [
            _,
            VectorContent::Paint(VectorPaintSpec::Stroke { animations, .. }),
        ] = group.contents.as_slice()
        else {
            panic!("isolated group does not contain Path then Stroke");
        };
        assert_eq!(animations.opacity.as_ref(), Some(&enabled_opacity));
        assert_eq!(animations.width.as_ref(), Some(&width));
        assert_eq!(animations.dash_offset.as_ref(), Some(&dash_offset));
    }

    #[test]
    fn animated_geometry_omits_only_stroke() {
        let mut program = source_program();
        let roundness = one_key(8.0);
        let VectorContent::Geometry { animations, .. } = &mut program.contents[0] else {
            panic!("first content is native Rectangle geometry")
        };
        animations.rect_roundness = Some(roundness.clone());
        let stroke = take_rect_stroke(&mut program.contents).expect("source Stroke");
        let output = rewrite_program(
            LayerId::new(7),
            [10.0, 40.0],
            [30.0, 20.0],
            ContourEligibility::Unsupported("animated Rectangle geometry"),
            stroke,
            VectorPaintAnimations::default(),
            program,
        )
        .expect("diagnosed omission");
        assert_eq!(output.program.contents.len(), 2);
        let VectorContent::Geometry { animations, .. } = &output.program.contents[0] else {
            panic!("native Rectangle geometry retained")
        };
        assert_eq!(animations.rect_roundness.as_ref(), Some(&roundness));
        assert!(output.diagnostics[0].message.contains("omitted"));
    }

    #[test]
    fn rounded_geometry_is_guarded_without_bezier_guess() {
        let mut program = source_program();
        let stroke = take_rect_stroke(&mut program.contents).expect("source Stroke");
        let output = rewrite_program(
            LayerId::new(7),
            [10.0, 40.0],
            [30.0, 20.0],
            ContourEligibility::Unsupported("rounded Rectangle semantics are unestablished"),
            stroke,
            VectorPaintAnimations::default(),
            program,
        )
        .expect("diagnosed omission");
        assert!(matches!(
            output.program.contents.as_slice(),
            [
                VectorContent::Geometry {
                    geometry: VectorGeometry::Rect { .. },
                    ..
                },
                VectorContent::Paint(VectorPaintSpec::Fill { .. }),
            ]
        ));
        assert!(output.diagnostics[0].message.contains("rounded Rectangle"));
    }
}
