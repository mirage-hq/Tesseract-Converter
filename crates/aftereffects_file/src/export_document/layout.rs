//! Converter-owned normalization for current FX Group layout and semantic containers.
//!
//! This module intentionally consumes only finite, static layout. It never samples
//! descendant motion or turns an animated auto-layout result into a static claim.

use std::collections::BTreeMap;

use fx_schema::{
    AiEditLayer, BlendMode, Dimensions, GroupLayer, Layer, LayerData, LayerId, NonNegativeProperty,
    PercentageProperty, Position, PropType, PropertyValue, RectLayer, RectShape, ShapeContent,
    ShapeFillStyle, ShapeHandleMirror, ShapeLayer, ShapeLineJoin, ShapePaint, ShapePath,
    ShapePathCommand, TimeRangeProperty, Transform,
};

use super::{ExportDiagnostic, effective_constant, hierarchy, media};

const KAPPA: f32 = 0.552_284_8;

/// A converter-owned Group whose dynamic child-bound layout has either been
/// represented exactly at the current static boundary or explicitly omitted.
pub(super) struct LayoutNormalization {
    pub group: GroupLayer,
    pub diagnostics: Vec<ExportDiagnostic>,
}

/// Materializes a Group's finite static background behind its original children.
///
/// `background_id` is allocated by the shared export owner so this helper does
/// not create a second identity namespace.
pub(super) fn normalize_group(
    group: &GroupLayer,
    background_id: LayerId,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: Dimensions,
) -> Result<LayoutNormalization, serde_json::Error> {
    let mut normalized = group.clone();
    let mut diagnostics = Vec::new();
    let had_layout = has_layout(&normalized);
    let layout_is_static = fold_layout_constants(&mut normalized, dynamics, &mut diagnostics);
    if !had_layout && !has_layout(&normalized) {
        return Ok(LayoutNormalization {
            group: normalized,
            diagnostics,
        });
    }

    if !layout_is_static {
        clear_layout(&mut normalized);
        diagnostics.push(diagnostic(
            group.id,
            "Animated Group padding/corner layout was not frozen: independent edge padding requires derived native Size/Position tracks and four independent radii have no exact single native Rectangle Roundness counterpart; the background is omitted and descendants are retained.",
        ));
        return Ok(LayoutNormalization {
            group: normalized,
            diagnostics,
        });
    }
    if has_descendant_dynamics(&group.layers, dynamics) {
        clear_layout(&mut normalized);
        diagnostics.push(diagnostic(
            group.id,
            "Group auto-layout depends on animated descendant bounds; the current layout was not frozen, the background is omitted, and descendants are retained.",
        ));
        return Ok(LayoutNormalization {
            group: normalized,
            diagnostics,
        });
    }

    let bounds = match hierarchy::static_child_union(group, resolved_media, canvas) {
        Ok(Some(bounds)) => bounds,
        Ok(None) => {
            clear_layout(&mut normalized);
            diagnostics.push(diagnostic(
                group.id,
                "Group background has no finite visual child union; auto-layout was not guessed, the background is omitted, and descendants are retained.",
            ));
            return Ok(LayoutNormalization {
                group: normalized,
                diagnostics,
            });
        }
        Err(reason) => {
            clear_layout(&mut normalized);
            diagnostics.push(diagnostic(
                group.id,
                format!(
                    "Group background auto-layout is not statically exact ({reason}); the background is omitted and descendants are retained."
                ),
            ));
            return Ok(LayoutNormalization {
                group: normalized,
                diagnostics,
            });
        }
    };

    let left = bounds.min[0] - normalized.padding_left.value();
    let top = bounds.min[1] - normalized.padding_top.value();
    let right = bounds.max[0] + normalized.padding_right.value();
    let bottom = bounds.max[1] + normalized.padding_bottom.value();
    let size = [right - left, bottom - top];
    if ![left, top, size[0], size[1]]
        .into_iter()
        .all(f64::is_finite)
        || size[0] <= 0.0
        || size[1] <= 0.0
    {
        clear_layout(&mut normalized);
        diagnostics.push(diagnostic(
            group.id,
            "Expanded Group background bounds are non-finite or empty; the background is omitted and descendants are retained.",
        ));
        return Ok(LayoutNormalization {
            group: normalized,
            diagnostics,
        });
    }

    let fill = background_fill(&normalized, &mut diagnostics);
    let radii = [
        normalized.corner_radius_top_left.value(),
        normalized.corner_radius_top_right.value(),
        normalized.corner_radius_bottom_right.value(),
        normalized.corner_radius_bottom_left.value(),
    ];
    let background = if radii.windows(2).all(|pair| pair[0] == pair[1]) {
        rect_background(group, background_id, [left, top], size, radii[0], fill)?
    } else {
        path_background(group, background_id, [left, top], size, radii, fill)?
    };
    normalized.layers.push(background);
    clear_layout(&mut normalized);
    diagnostics.push(diagnostic(
        group.id,
        "Current finite child-bound Group layout was normalized to an editable background child. Padding/background edit linkage and the original Group layout controls are not reconstructed; hierarchy origin translation remains owned by native precomposition lowering.",
    ));
    Ok(LayoutNormalization {
        group: normalized,
        diagnostics,
    })
}

/// Converts the actual explicit AiEdit child stack and 1x clock into an identity Group.
/// Semantic data outside that stack remains diagnosed rather than inferred from the name.
pub(super) fn normalize_ai_edit(
    ai_edit: &AiEditLayer,
) -> Result<LayoutNormalization, serde_json::Error> {
    let mut diagnostics = Vec::new();
    if ai_edit.background.is_some() {
        diagnostics.push(diagnostic(
            ai_edit.id,
            "AiEdit background metadata is host-owned semantic data outside the explicit descendant stack; it is omitted while descendants are retained.",
        ));
    }
    if !ai_edit.stickers.is_empty() {
        diagnostics.push(diagnostic(
            ai_edit.id,
            "AiEdit stickers are host-owned semantic data rather than explicit FX descendants; they are omitted without flattening or synthesizing PAG content.",
        ));
    }
    if !ai_edit
        .layers
        .iter()
        .any(|layer| layer.id() == ai_edit.source_layer_id)
    {
        diagnostics.push(diagnostic(
            ai_edit.id,
            "AiEdit source_layer_id does not identify a direct explicit descendant; no external source is reconstructed.",
        ));
    }
    diagnostics.push(diagnostic(
        ai_edit.id,
        "AiEdit's explicit descendant composition and 1x active-range clock were normalized to an identity Group; style/template lineage is not a native AE authoring object.",
    ));
    Ok(LayoutNormalization {
        group: GroupLayer {
            id: ai_edit.id,
            name: ai_edit.name.clone(),
            description: String::new(),
            is_hidden: false,
            parent: ai_edit.parent,
            blend_mode: BlendMode::Normal,
            track_matte: None,
            masks: Vec::new(),
            playback: fx_schema::LayerPlayback::linear(
                ai_edit.active_range,
                ai_edit.active_range,
                ai_edit.active_range,
                0,
            )
            .expect("validated AiEdit range defines identity playback"),
            effects: Vec::new(),
            motion_blur: false,
            padding_top: NonNegativeProperty::default(),
            padding_right: NonNegativeProperty::default(),
            padding_bottom: NonNegativeProperty::default(),
            padding_left: NonNegativeProperty::default(),
            fills: Vec::new(),
            corner_radius_top_left: NonNegativeProperty::default(),
            corner_radius_top_right: NonNegativeProperty::default(),
            corner_radius_bottom_right: NonNegativeProperty::default(),
            corner_radius_bottom_left: NonNegativeProperty::default(),
            transform: identity_transform(),
            layers: ai_edit.layers.clone(),
        },
        diagnostics,
    })
}

fn has_descendant_dynamics(
    layers: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    layers.iter().any(|layer| {
        dynamics.for_layer(layer.id()).next().is_some()
            || layer
                .child_layers()
                .is_some_and(|children| has_descendant_dynamics(children, dynamics))
    })
}

fn fold_layout_constants(
    group: &mut GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    diagnostics: &mut Vec<ExportDiagnostic>,
) -> bool {
    let mut is_static = true;
    for entry in dynamics.for_layer(group.id) {
        let Some(property) = entry.target.as_property() else {
            continue;
        };
        if property.layer_id() != group.id || !is_layout_property(property.property_type()) {
            continue;
        }
        match effective_constant(&entry.animator) {
            Some(PropertyValue::Float(value)) if value.is_finite() && *value >= 0.0 => {
                let value = NonNegativeProperty::new(*value)
                    .expect("finite non-negative layout constant was checked");
                set_layout_property(group, property.property_type(), value);
            }
            Some(_) => {
                is_static = false;
                diagnostics.push(diagnostic(
                    group.id,
                    format!(
                        "{} constant has an invalid value kind/range and cannot define native layout.",
                        property.property_type()
                    ),
                ));
            }
            None => is_static = false,
        }
    }
    is_static
}

fn is_layout_property(property: PropType) -> bool {
    matches!(
        property,
        PropType::PaddingTop
            | PropType::PaddingRight
            | PropType::PaddingBottom
            | PropType::PaddingLeft
            | PropType::CornerRadiusTopLeft
            | PropType::CornerRadiusTopRight
            | PropType::CornerRadiusBottomRight
            | PropType::CornerRadiusBottomLeft
    )
}

fn set_layout_property(group: &mut GroupLayer, property: PropType, value: NonNegativeProperty) {
    match property {
        PropType::PaddingTop => group.padding_top = value,
        PropType::PaddingRight => group.padding_right = value,
        PropType::PaddingBottom => group.padding_bottom = value,
        PropType::PaddingLeft => group.padding_left = value,
        PropType::CornerRadiusTopLeft => group.corner_radius_top_left = value,
        PropType::CornerRadiusTopRight => group.corner_radius_top_right = value,
        PropType::CornerRadiusBottomRight => group.corner_radius_bottom_right = value,
        PropType::CornerRadiusBottomLeft => group.corner_radius_bottom_left = value,
        _ => {}
    }
}

fn has_layout(group: &GroupLayer) -> bool {
    !group.fills.is_empty()
        || [
            group.padding_top.value(),
            group.padding_right.value(),
            group.padding_bottom.value(),
            group.padding_left.value(),
            group.corner_radius_top_left.value(),
            group.corner_radius_top_right.value(),
            group.corner_radius_bottom_right.value(),
            group.corner_radius_bottom_left.value(),
        ]
        .into_iter()
        .any(|value| value != 0.0)
}

fn clear_layout(group: &mut GroupLayer) {
    group.padding_top = NonNegativeProperty::default();
    group.padding_right = NonNegativeProperty::default();
    group.padding_bottom = NonNegativeProperty::default();
    group.padding_left = NonNegativeProperty::default();
    group.fills.clear();
    group.corner_radius_top_left = NonNegativeProperty::default();
    group.corner_radius_top_right = NonNegativeProperty::default();
    group.corner_radius_bottom_right = NonNegativeProperty::default();
    group.corner_radius_bottom_left = NonNegativeProperty::default();
}

fn background_fill(group: &GroupLayer, diagnostics: &mut Vec<ExportDiagnostic>) -> ShapeFillStyle {
    let Some(fill) = group.fills.first() else {
        return ShapeFillStyle::solid([0.0; 4]);
    };
    if group.fills.len() > 1 {
        diagnostics.push(diagnostic(
            group.id,
            "Ordered multi-fill Group background has no current bounded single-owner native export; paints are omitted while a transparent editable extent preserves the proven layout canvas.",
        ));
        return ShapeFillStyle::solid([0.0; 4]);
    }
    let mut fill = fill.clone();
    let opacity = fill.opacity;
    match &mut fill.paint {
        ShapePaint::Solid { color } => color[3] *= opacity,
        ShapePaint::Gradient { stops, .. } => {
            for stop in stops {
                stop.color[3] *= opacity;
            }
        }
    }
    fill.opacity = 1.0;
    fill
}

fn rect_background(
    owner: &GroupLayer,
    id: LayerId,
    origin: [f64; 2],
    size: [f64; 2],
    radius: f64,
    fill: ShapeFillStyle,
) -> Result<Layer, serde_json::Error> {
    let (fill_color, fill_paint) = match &fill.paint {
        ShapePaint::Solid { color } => (*color, None),
        paint => ([0.0; 4], Some(paint.clone())),
    };
    Layer::from_data(&LayerData::Rect(RectLayer {
        id,
        name: format!("{} Background", owner.name),
        description: "Editable Group background normalized by AE export".into(),
        is_hidden: false,
        parent: Some(owner.id),
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range: TimeRangeProperty::new(
            fx_schema::Time::ZERO,
            owner.playback.input_range().duration,
        ),
        effects: Vec::new(),
        motion_blur: false,
        transform: identity_transform(),
        rect: RectShape {
            size,
            position: origin,
            roundness: radius,
            fill_enabled: true,
            fill_color,
            fill_paint,
            fill_blend_mode: (fill.blend_mode != BlendMode::Normal).then_some(fill.blend_mode),
            stroke_enabled: false,
            stroke_color: None,
            stroke_width: NonNegativeProperty::default(),
            stroke_dashes: Vec::new(),
            stroke_dash_offset: 0.0,
            stroke_join: ShapeLineJoin::Miter,
            stroke_miter_limit: 4.0,
        },
    }))
}

fn path_background(
    owner: &GroupLayer,
    id: LayerId,
    origin: [f64; 2],
    size: [f64; 2],
    radii: [f64; 4],
    fill: ShapeFillStyle,
) -> Result<Layer, serde_json::Error> {
    Layer::from_data(&LayerData::Shape(ShapeLayer {
        id,
        name: format!("{} Background", owner.name),
        description: "Editable unequal-radius Group background normalized by AE export".into(),
        is_hidden: false,
        parent: Some(owner.id),
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range: TimeRangeProperty::new(
            fx_schema::Time::ZERO,
            owner.playback.input_range().duration,
        ),
        effects: Vec::new(),
        motion_blur: false,
        transform: identity_transform(),
        shape: ShapeContent {
            path: rounded_background_path(origin, size, radii),
            fills: vec![fill],
            strokes: Vec::new(),
            round_corners: None,
            offset_paths: None,
            trim: None,
            poly_star: None,
            ellipse: None,
        },
    }))
}

fn rounded_background_path(origin: [f64; 2], size: [f64; 2], radii: [f64; 4]) -> ShapePath {
    let x = origin[0] as f32;
    let y = origin[1] as f32;
    let width = size[0] as f32;
    let height = size[1] as f32;
    let [mut tl, mut tr, mut br, mut bl] = radii.map(|radius| (radius as f32).max(0.0));
    let mut scale = 1.0_f32;
    for (edge, sum) in [
        (width, tl + tr),
        (width, bl + br),
        (height, tl + bl),
        (height, tr + br),
    ] {
        if sum > 0.0 {
            scale = scale.min((edge / sum).max(0.0));
        }
    }
    tl *= scale;
    tr *= scale;
    br *= scale;
    bl *= scale;
    let [ktl, ktr, kbr, kbl] = [tl, tr, br, bl].map(|radius| radius * KAPPA);
    let linear = Some(ShapeHandleMirror::Linear);
    let point = Some(ShapeHandleMirror::Point);
    ShapePath {
        commands: vec![
            ShapePathCommand::MoveTo {
                x: f64::from(x + tl),
                y: f64::from(y),
                mirror: linear,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: f64::from(x + width - tr),
                y: f64::from(y),
                mirror: linear,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: f64::from(x + width - tr + ktr),
                c1y: f64::from(y),
                c2x: f64::from(x + width),
                c2y: f64::from(y + tr - ktr),
                x: f64::from(x + width),
                y: f64::from(y + tr),
                mirror: point,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: f64::from(x + width),
                y: f64::from(y + height - br),
                mirror: linear,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: f64::from(x + width),
                c1y: f64::from(y + height - br + kbr),
                c2x: f64::from(x + width - br + kbr),
                c2y: f64::from(y + height),
                x: f64::from(x + width - br),
                y: f64::from(y + height),
                mirror: point,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: f64::from(x + bl),
                y: f64::from(y + height),
                mirror: linear,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: f64::from(x + bl - kbl),
                c1y: f64::from(y + height),
                c2x: f64::from(x),
                c2y: f64::from(y + height - bl + kbl),
                x: f64::from(x),
                y: f64::from(y + height - bl),
                mirror: point,
                corner_radius: None,
            },
            ShapePathCommand::LineTo {
                x: f64::from(x),
                y: f64::from(y + tl),
                mirror: linear,
                corner_radius: None,
            },
            ShapePathCommand::CubicTo {
                c1x: f64::from(x),
                c1y: f64::from(y + tl - ktl),
                c2x: f64::from(x + tl - ktl),
                c2y: f64::from(y),
                x: f64::from(x + tl),
                y: f64::from(y),
                mirror: point,
                corner_radius: None,
            },
            ShapePathCommand::Close,
        ],
    }
}

fn identity_transform() -> Transform {
    Transform {
        anchor_point: [0.0, 0.0],
        position: Position::xy(0.0, 0.0),
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0, 0.0, 0.0],
        opacity: PercentageProperty::new(100.0).expect("100 is a valid percentage"),
    }
}

fn diagnostic(layer_id: LayerId, message: impl Into<String>) -> ExportDiagnostic {
    ExportDiagnostic {
        layer_id: Some(layer_id),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unequal_radii_copy_current_css_clamp_geometry() {
        let path = rounded_background_path([0.0, 0.0], [100.0, 40.0], [80.0, 40.0, 20.0, 20.0]);
        assert_eq!(path.commands.len(), 10);
        assert!(matches!(
            path.commands[0],
            ShapePathCommand::MoveTo { x, y, .. }
                if (x - 32.0).abs() < 1e-6 && y.abs() < 1e-6
        ));
    }

    #[test]
    fn background_fill_folds_static_opacity_without_changing_rgb() {
        let mut group = empty_group();
        group.fills.push(ShapeFillStyle {
            paint: ShapePaint::Solid {
                color: [0.2, 0.3, 0.4, 0.8],
            },
            fill_rule: Default::default(),
            blend_mode: BlendMode::Normal,
            opacity: 0.25,
        });
        let mut diagnostics = Vec::new();
        let fill = background_fill(&group, &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert!(matches!(fill.paint, ShapePaint::Solid { color } if color == [0.2, 0.3, 0.4, 0.2]));
        assert_eq!(fill.opacity, 1.0);
    }

    #[test]
    fn ai_edit_normalization_preserves_pag_descendant_without_flattening() {
        let value = serde_json::json!({
            "type": "AiEdit", "id": 1, "name": "shot", "activeRange": {"start": 0, "duration": 1000},
            "styleId": "style", "sourceLayerId": 2,
            "layers": [
                {"type": "Image", "id": 2, "name": "source", "activeRange": {"start": 0, "duration": 1000}, "transform": identity_transform(), "source": {"assetId": "image", "sourceRect": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0}, "fit": "stretch"}},
                {"type": "Pag", "id": 3, "name": "pag", "activeRange": {"start": 0, "duration": 1000}, "items": [{"assetId": "pag"}]}
            ]
        });
        let layer: Layer = serde_json::from_value(value).expect("fixture is valid");
        let LayerData::AiEdit(ai_edit) = layer.data() else {
            panic!("expected AiEdit");
        };
        let normalized = normalize_ai_edit(ai_edit).expect("normalization serializes");
        assert_eq!(normalized.group.layers.len(), 2);
        assert!(matches!(
            normalized.group.layers[1].data(),
            LayerData::Pag(_)
        ));
    }

    fn empty_group() -> GroupLayer {
        GroupLayer {
            id: LayerId::new(1),
            name: "group".into(),
            description: String::new(),
            is_hidden: false,
            parent: None,
            blend_mode: BlendMode::Normal,
            track_matte: None,
            masks: Vec::new(),
            playback: {
                let range = TimeRangeProperty::new(
                    fx_schema::Time::ZERO,
                    fx_schema::Duration::from_millis(1000),
                );
                fx_schema::LayerPlayback::linear(range, range, range, 0).unwrap()
            },
            effects: Vec::new(),
            motion_blur: false,
            padding_top: NonNegativeProperty::default(),
            padding_right: NonNegativeProperty::default(),
            padding_bottom: NonNegativeProperty::default(),
            padding_left: NonNegativeProperty::default(),
            fills: Vec::new(),
            corner_radius_top_left: NonNegativeProperty::default(),
            corner_radius_top_right: NonNegativeProperty::default(),
            corner_radius_bottom_right: NonNegativeProperty::default(),
            corner_radius_bottom_left: NonNegativeProperty::default(),
            transform: identity_transform(),
            layers: Vec::new(),
        }
    }
}
