//! Lower converter-local vector artwork into existing editable FX Shape layers.

use fx_schema::{
    FxItemId, GroupLayer, LayerData as FxLayer, LayerId, MaskMode, NonNegativeProperty, PathMask,
    PercentageProperty, Position, ShapeContent, ShapeFillRule, ShapeFillStyle, ShapeLayer,
    ShapeLineCap, ShapeLineJoin, ShapePaint, ShapePath, ShapePathCommand, ShapeStrokeStyle,
    Transform,
};

use super::MediaLayerConversion;
use crate::{
    structure_document::shapes::OutputBudget,
    vector_media::{
        Affine, Artwork, LineCap as VectorLineCap, LineJoin as VectorLineJoin, PathCommand,
    },
};

pub(super) fn lower(
    artwork: &Artwork,
    occurrence: &GroupLayer,
    mut next_id: u64,
    budget: &mut OutputBudget,
) -> MediaLayerConversion {
    let mut layers = Vec::with_capacity(artwork.shapes.len() + 1);
    let mut warnings = artwork.warnings.clone();
    let guide_id = LayerId::new(next_id);
    let guide = page_clip(occurrence, guide_id, artwork.dimensions);
    for (index, source) in artwork.shapes.iter().enumerate() {
        // Probe without consuming IDs for invalid or byte-budget-denied paints.
        // The guide is emitted only with the first paint, and reserved only then.
        let mut next_available_id = next_id;
        let count = 2 + u64::from(layers.is_empty());
        let Some(first_id) = super::super::reserve_ids(&mut next_available_id, count) else {
            warnings.push(format!(
                "PDF-compatible AI paint {index} omitted because generated layer identifier space is exhausted; earlier independent paints retained"
            ));
            break;
        };
        let paint_id = first_id + u64::from(layers.is_empty());
        let Some(transform) = decompose(source.transform) else {
            warnings.push(format!(
                "PDF-compatible AI paint {index} has a singular or unrepresentable affine transform; that paint was omitted"
            ));
            continue;
        };
        let path = ShapePath {
            commands: source.path.iter().map(path_command).collect::<Vec<_>>(),
        };
        if path.commands.is_empty() || !path.is_finite() {
            warnings.push(format!(
                "PDF-compatible AI paint {index} has empty or non-finite geometry; that paint was omitted"
            ));
            continue;
        }
        let fills: Vec<_> = source
            .fill
            .map(|fill| ShapeFillStyle {
                paint: ShapePaint::Solid { color: fill.color },
                fill_rule: if fill.even_odd {
                    ShapeFillRule::EvenOdd
                } else {
                    ShapeFillRule::NonZeroWinding
                },
                blend_mode: Default::default(),
                opacity: 1.0,
            })
            .into_iter()
            .collect();
        let strokes: Vec<_> = source
            .stroke
            .as_ref()
            .and_then(|stroke| {
                if stroke.width == 0.0 {
                    warnings.push(format!(
                        "PDF-compatible AI paint {index} uses a device hairline stroke; the stroke was omitted because a zero-width FX stroke is not equivalent"
                    ));
                    return None;
                }
                let width = NonNegativeProperty::new(stroke.width)?;
                let dashes = stroke
                    .dashes
                    .iter()
                    .copied()
                    .map(NonNegativeProperty::new)
                    .collect::<Option<Vec<_>>>()?;
                Some(ShapeStrokeStyle {
                    enabled: true,
                    paint: ShapePaint::Solid {
                        color: stroke.color,
                    },
                    width,
                    cap: match stroke.cap {
                        VectorLineCap::Butt => ShapeLineCap::Butt,
                        VectorLineCap::Round => ShapeLineCap::Round,
                        VectorLineCap::Square => ShapeLineCap::Square,
                    },
                    join: match stroke.join {
                        VectorLineJoin::Miter => ShapeLineJoin::Miter,
                        VectorLineJoin::Round => ShapeLineJoin::Round,
                        VectorLineJoin::Bevel => ShapeLineJoin::Bevel,
                    },
                    miter_limit: stroke.miter_limit,
                    blend_mode: Default::default(),
                    opacity: 1.0,
                    dashes,
                    dash_offset: stroke.dash_offset,
                })
            })
            .into_iter()
            .collect();
        if fills.is_empty() && strokes.is_empty() {
            continue;
        }
        let layer = ShapeLayer {
            id: LayerId::new(paint_id),
            name: format!("{}: AI paint {}", occurrence.name, index + 1),
            description: "Editable solid path paint imported from a PDF-compatible .ai source; original AI bytes are not required at render time".into(),
            is_hidden: false,
            parent: Some(occurrence.id),
            blend_mode: Default::default(),
            track_matte: None,
            masks: vec![PathMask {
                id: FxItemId::new(paint_id + 1),
                mode: MaskMode::Add,
                inverted: false,
                layer: Some(guide_id),
                legacy_path: None,
                feather: [0.0, 0.0],
                expansion: 0.0,
                opacity: NonNegativeProperty::new(1.0).expect("one is non-negative"),
            }],
            active_range: static_source_range(),
            effects: Vec::new(),
            motion_blur: occurrence.motion_blur,
            transform,
            shape: ShapeContent {
                path,
                fills,
                strokes,
                round_corners: None,
                offset_paths: None,
                trim: None,
                poly_star: None,
                ellipse: None,
            },
        };
        let candidate = FxLayer::Shape(layer);
        let reserved = if layers.is_empty() {
            // Reserve both atomically: never emit a dangling mask reference.
            budget.reserve(&(&guide, &candidate))
        } else {
            budget.reserve(&candidate)
        };
        if !reserved {
            warnings.push(format!(
                "PDF-compatible AI paint {index} exceeded the shared shape-output byte allowance; that paint was omitted and smaller siblings remain eligible"
            ));
            continue;
        }
        if layers.is_empty() {
            layers.push(guide.clone());
        }
        layers.push(candidate);
        next_id = next_available_id;
    }
    // PDF paints bottom-to-top; FX stores timeline siblings topmost-first.
    layers.reverse();
    MediaLayerConversion {
        layers,
        animations: Vec::new(),
        next_id,
        warnings,
        assets: Vec::new(),
    }
}

fn static_source_range() -> fx_schema::TimeRangeProperty {
    // Occurrence wrappers own trimming/remap. Static geometry and its clip
    // must remain available when the wrapper samples outside the comp range.
    fx_schema::TimeRangeProperty::new(
        fx_schema::Time::ZERO,
        fx_schema::Duration::from_secs(super::super::MAX_TIME_SECS),
    )
}

fn page_clip(occurrence: &GroupLayer, id: LayerId, [width, height]: [f64; 2]) -> FxLayer {
    FxLayer::Shape(ShapeLayer {
        id,
        name: "AI page clip".into(),
        description: "Editable PDF page boundary; consumed by path masks, never a painted fallback"
            .into(),
        is_hidden: false,
        parent: Some(occurrence.id),
        blend_mode: Default::default(),
        track_matte: None,
        masks: Vec::new(),
        active_range: static_source_range(),
        effects: Vec::new(),
        motion_blur: occurrence.motion_blur,
        transform: super::identity_transform(),
        shape: ShapeContent {
            path: ShapePath {
                commands: [
                    PathCommand::Move([0.0, 0.0]),
                    PathCommand::Line([width, 0.0]),
                    PathCommand::Line([width, height]),
                    PathCommand::Line([0.0, height]),
                    PathCommand::Close,
                ]
                .iter()
                .map(path_command)
                .collect(),
            },
            fills: Vec::new(),
            strokes: Vec::new(),
            round_corners: None,
            offset_paths: None,
            trim: None,
            poly_star: None,
            ellipse: None,
        },
    })
}

fn path_command(command: &PathCommand) -> ShapePathCommand {
    match *command {
        PathCommand::Move([x, y]) => ShapePathCommand::MoveTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        },
        PathCommand::Line([x, y]) => ShapePathCommand::LineTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        },
        PathCommand::Cubic([c1x, c1y], [c2x, c2y], [x, y]) => ShapePathCommand::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
            mirror: None,
            corner_radius: None,
        },
        PathCommand::Close => ShapePathCommand::Close,
    }
}

// QR decomposition with skew_axis=0 exactly matches FX's matrix convention:
// R(rotation) * Shear(-tan(skew)) * Scale. A signed Y scale retains reflection.
fn decompose(matrix: Affine) -> Option<Transform> {
    let scale_x = matrix.a.hypot(matrix.b);
    let determinant = matrix.a * matrix.d - matrix.b * matrix.c;
    if !scale_x.is_finite() || scale_x <= 1.0e-12 || !determinant.is_finite() {
        return None;
    }
    let scale_y = determinant / scale_x;
    if !scale_y.is_finite() || scale_y.abs() <= 1.0e-12 {
        return None;
    }
    let shear = (matrix.a * matrix.c + matrix.b * matrix.d) / (scale_x * scale_y);
    let rotation = matrix.b.atan2(matrix.a).to_degrees();
    let skew = -shear.atan().to_degrees();
    let values = [scale_x, scale_y, shear, rotation, skew, matrix.e, matrix.f];
    if values.into_iter().any(|value| !value.is_finite()) || skew.abs() > 89.9 {
        return None;
    }
    Some(Transform {
        anchor_point: [0.0, 0.0],
        position: Position::TwoD([matrix.e, matrix.f]),
        scale: [scale_x * 100.0, scale_y * 100.0],
        rotation,
        skew,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0, 0.0, 0.0],
        opacity: PercentageProperty::new(100.0).expect("100 is a finite percentage in 0..=100"),
    })
}

#[cfg(test)]
mod tests {
    use fx_schema::{Duration, Time, TimeRangeProperty};

    use super::*;

    #[test]
    fn specification_built_sources_lower_to_ordered_editable_shapes_without_assets_or_js() {
        let parent = super::super::super::group(
            LayerId::new(7),
            "AI occurrence".into(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(2.0)),
        );
        let mut budget = OutputBudget::default();
        let artworks = [
            crate::vector_media::decode(include_bytes!(
                "../../../tests/fixtures/vector_media/spec_case_1.ai"
            ))
            .expect("compound specification case decodes"),
            crate::vector_media::decode(include_bytes!(
                "../../../tests/fixtures/vector_media/spec_case_2.ai"
            ))
            .expect("stroke specification case decodes"),
            crate::vector_media::decode(include_bytes!(
                "../../../tests/fixtures/vector_media/spec_case_3.ai"
            ))
            .expect("transform specification case decodes"),
        ];
        let converted = artworks
            .iter()
            .map(|artwork| lower(artwork, &parent, 20, &mut budget))
            .collect::<Vec<_>>();

        assert_eq!(converted[0].layers.len(), 3);
        assert_eq!(converted[1].layers.len(), 4);
        assert_eq!(converted[2].layers.len(), 4);
        assert!(converted.iter().all(|result| result.assets.is_empty()));
        assert!(
            converted
                .iter()
                .flat_map(|result| &result.layers)
                .all(|layer| matches!(layer, FxLayer::Shape(_)))
        );
        let FxLayer::Shape(compound) = &converted[0].layers[1] else {
            panic!("compound paint must be an editable Shape")
        };
        assert_eq!(compound.id, LayerId::new(21));
        assert_eq!(compound.parent, Some(parent.id));
        assert_eq!(compound.shape.fills[0].fill_rule, ShapeFillRule::EvenOdd);
        assert_eq!(
            compound
                .shape
                .path
                .commands
                .iter()
                .filter(|command| matches!(command, ShapePathCommand::MoveTo { .. }))
                .count(),
            2
        );
        assert!(converted[0].layers.iter().any(|layer| matches!(
            layer,
            FxLayer::Shape(shape)
                if shape.shape.path.commands.iter().any(|command| matches!(command, ShapePathCommand::CubicTo { .. }))
        )));
        let caps = converted[1]
            .layers
            .iter()
            .filter_map(|layer| match layer {
                FxLayer::Shape(shape) => shape.shape.strokes.first().map(|stroke| stroke.cap),
                _ => unreachable!("only Shape output is allowed"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            caps,
            vec![
                ShapeLineCap::Square,
                ShapeLineCap::Round,
                ShapeLineCap::Butt
            ]
        );
        let FxLayer::Shape(transformed) = &converted[2].layers[1] else {
            panic!("transformed paint must be an editable Shape")
        };
        assert_ne!(transformed.transform.scale, [100.0, 100.0]);
        assert_ne!(transformed.transform.skew, 0.0);
        let json = serde_json::to_string(
            &converted
                .iter()
                .map(|value| &value.layers)
                .collect::<Vec<_>>(),
        )
        .expect("lowered Shapes serialize");
        assert!(!json.contains("JsScript"));
        assert!(!json.contains("assetId"));
    }

    #[test]
    fn vector_paint_reserves_guide_and_mask_atomically_at_counter_overflow() {
        let mut artwork = crate::vector_media::decode(include_bytes!(
            "../../../tests/fixtures/vector_media/spec_case_1.ai"
        ))
        .unwrap();
        let paint = artwork.shapes[0].clone();
        artwork.shapes = vec![paint.clone(), paint];
        let occurrence = super::super::super::group(
            LayerId::new(2),
            "AI".into(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(2.0)),
        );
        let converted = lower(
            &artwork,
            &occurrence,
            u64::MAX - 3,
            &mut OutputBudget::default(),
        );
        assert_eq!(converted.layers.len(), 2, "guide plus first paint only");
        assert_eq!(converted.next_id, u64::MAX);
        assert!(
            converted
                .warnings
                .iter()
                .any(|warning| warning.contains("identifier space is exhausted"))
        );
        let FxLayer::Shape(paint) = &converted.layers[0] else {
            panic!("first paint must be editable")
        };
        assert_eq!(paint.id, LayerId::new(u64::MAX - 2));
        assert_eq!(paint.masks[0].id, FxItemId::new(u64::MAX - 1));
        assert_eq!(paint.masks[0].layer, Some(LayerId::new(u64::MAX - 3)));

        let rejected = lower(
            &artwork,
            &occurrence,
            u64::MAX - 2,
            &mut OutputBudget::default(),
        );
        assert!(rejected.layers.is_empty(), "no dangling page clip");
        assert_eq!(rejected.next_id, u64::MAX - 2);
    }

    #[test]
    fn vector_paints_keep_editable_shapes_past_ten_thousand_ids() {
        let mut artwork = crate::vector_media::decode(include_bytes!(
            "../../../tests/fixtures/vector_media/spec_case_1.ai"
        ))
        .unwrap();
        let paint = artwork.shapes[0].clone();
        artwork.shapes = vec![paint; 5_001];
        let occurrence = super::super::super::group(
            LayerId::new(2),
            "AI".into(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(2.0)),
        );
        let converted = lower(&artwork, &occurrence, 3, &mut OutputBudget::default());
        assert_eq!(converted.layers.len(), 5_002); // Paints and their shared clip guide.
        assert_eq!(converted.next_id, 10_006);
        assert!(converted.assets.is_empty());
        assert!(
            converted
                .warnings
                .iter()
                .all(|warning| !warning.contains("exhausted"))
        );
        assert!(converted.layers.iter().any(|layer| matches!(layer,
            FxLayer::Shape(shape) if shape.id == LayerId::new(10_004)
                && shape.masks[0].layer == Some(LayerId::new(3))
        )));
    }

    #[test]
    fn pdf_paints_use_topmost_first_fx_order_and_share_an_editable_page_clip() {
        let artwork = crate::vector_media::decode(include_bytes!(
            "../../../tests/fixtures/vector_media/spec_case_3.ai"
        ))
        .unwrap();
        let occurrence = super::super::super::group(
            LayerId::new(7),
            "AI".into(),
            None,
            TimeRangeProperty::new(Time::from_secs(5.0), Duration::from_secs(2.0)),
        );
        let converted = lower(&artwork, &occurrence, 20, &mut OutputBudget::default());
        assert!(
            converted.layers.iter().all(|layer| matches!(layer,
                FxLayer::Shape(shape) if shape.active_range.start == Time::ZERO
                    && shape.active_range.end() > Time::from_secs(100.0)
            )),
            "static source shapes and clip must not inherit the occurrence trim"
        );
        let FxLayer::Shape(top) = &converted.layers[0] else {
            panic!("Shape expected")
        };
        assert_eq!(
            top.shape.strokes.len(),
            1,
            "last PDF paint must be topmost in FX"
        );
        assert_eq!(
            top.masks.len(),
            1,
            "page boundary must clip editable artwork"
        );
        let guide = top.masks[0].layer.unwrap();
        assert!(converted.layers.iter().any(|layer| matches!(layer,
            FxLayer::Shape(shape) if shape.id == guide && shape.shape.path.commands.len() == 5
        )));
    }

    #[test]
    fn affine_decomposition_reconstructs_nonuniform_reflected_skew() {
        let matrix = Affine {
            a: 1.25,
            b: 0.75,
            c: -0.4,
            d: -2.0,
            e: 12.0,
            f: 34.0,
        };
        let transform = decompose(matrix).expect("invertible affine decomposes");
        let rotation = transform.rotation.to_radians();
        let shear = -transform.skew.to_radians().tan();
        let sx = transform.scale[0] / 100.0;
        let sy = transform.scale[1] / 100.0;
        let (sin, cos) = rotation.sin_cos();
        let Position::TwoD(position) = transform.position else {
            panic!("vector transform must remain two-dimensional")
        };
        let reconstructed = Affine {
            a: cos * sx,
            b: sin * sx,
            c: sy * (shear * cos - sin),
            d: sy * (shear * sin + cos),
            e: position[0],
            f: position[1],
        };
        for (actual, expected) in [
            reconstructed.a - matrix.a,
            reconstructed.b - matrix.b,
            reconstructed.c - matrix.c,
            reconstructed.d - matrix.d,
            reconstructed.e - matrix.e,
            reconstructed.f - matrix.f,
        ]
        .into_iter()
        .zip([0.0; 6])
        {
            assert!((actual - expected).abs() < 1.0e-9);
        }
    }
}
