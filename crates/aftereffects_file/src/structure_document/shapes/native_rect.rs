//! Native FX Rect for an unshared AE Rectangle with compatible owned paints.
//! Compound geometry keeps the existing ordered-program lowering.

#[path = "caption_box.rs"]
mod caption_box;
#[cfg(test)]
mod gradient_stroke_tests;
#[path = "rigged_box.rs"]
mod rigged_box;
#[cfg(test)]
mod tests;

use crate::structure::{Composition, Layer};

use super::{
    Collector, FxLayer, ShapePaint, blend, defaults, full_active_range, gradient_fill,
    identity_transform, numeric_leaf,
    program::{Draw, GeometryKind, NativeRun, PaintId, Program, ScopeId},
    property_group, solid_fill, solid_stroke,
};
use crate::rifx::Chunk;
use crate::structure_document::animation::NumericAnimationTarget;
use fx_schema::layer::{RectLayer, ShapeLineJoin};
use fx_schema::{LayerId, PercentageProperty, Position, PropType, PropertyTarget, RectShape};

/// Builds an unpainted typed Rectangle producer for a same-scope Boolean operand.
/// Paint and vector-group ownership remain on the Boolean consumer and its parent.
pub(super) fn producer(
    collector: &mut Collector<'_>,
    source: &NativeRun<'_>,
    name: &str,
    parent: LayerId,
) -> Option<RectLayer> {
    if source.name != "ADBE Vector Shape - Rect" {
        return None;
    }
    let mut warnings = Vec::new();
    let leaves = property_group(source.chunks, "Rectangle Path").ok()?;
    let size = initial_pair(&leaves, "ADBE Vector Rect Size")?;
    let position = initial_pair(&leaves, "ADBE Vector Rect Position")?;
    let roundness = initial_scalar(&leaves, "ADBE Vector Rect Roundness")?;
    if size.iter().any(|value| !value.is_finite() || *value <= 0.0)
        || position.iter().any(|value| !value.is_finite())
        || !roundness.is_finite()
        || roundness < 0.0
        || super::direction::reversed(source.chunks, &mut warnings)
    {
        return None;
    }
    for (property, dimensions, positive) in [
        ("ADBE Vector Rect Size", 2, true),
        ("ADBE Vector Rect Position", 2, false),
        ("ADBE Vector Rect Roundness", 1, false),
    ] {
        let Some(numeric) = numeric_leaf(&leaves, property, &mut warnings) else {
            if defaults::numeric(property).is_some()
                && !leaves.iter().any(|(candidate, _)| *candidate == property)
            {
                continue;
            }
            return None;
        };
        if numeric.expression_enabled
            || numeric.keyframes.iter().any(|key| {
                key.values.len() < dimensions
                    || key.values[..dimensions].iter().any(|value| {
                        !value.is_finite()
                            || (positive && *value <= 0.0)
                            || (property == "ADBE Vector Rect Roundness" && *value < 0.0)
                    })
            })
        {
            return None;
        }
    }
    let id = collector.allocate()?;
    let mut transform = identity_transform();
    transform.anchor_point = [size[0] / 2.0, size[1] / 2.0];
    transform.position = Position::TwoD(position);
    let rect = RectLayer {
        id,
        name: name.to_owned(),
        description: "Typed AE Rectangle producer for Boolean geometry".into(),
        is_hidden: true,
        parent: Some(parent),
        blend_mode: Default::default(),
        track_matte: None,
        masks: Vec::new(),
        active_range: full_active_range(),
        effects: Vec::new(),
        motion_blur: false,
        transform,
        rect: RectShape {
            size,
            position: [0.0, 0.0],
            roundness,
            fill_enabled: false,
            fill_color: [0.0, 0.0, 0.0, 1.0],
            fill_paint: None,
            fill_blend_mode: None,
            stroke_enabled: false,
            stroke_color: None,
            stroke_width: Default::default(),
            stroke_dashes: Vec::new(),
            stroke_dash_offset: 0.0,
            stroke_join: ShapeLineJoin::default(),
            stroke_miter_limit: 4.0,
        },
    };
    for (property, targets) in [
        (
            "ADBE Vector Rect Size",
            vec![
                NumericAnimationTarget::vector2(
                    PropertyTarget::layer(id, PropType::RectSize),
                    [0, 1],
                    [1.0, 1.0],
                ),
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::AnchorPointX),
                    0,
                    0.5,
                ),
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::AnchorPointY),
                    1,
                    0.5,
                ),
            ],
        ),
        (
            "ADBE Vector Rect Position",
            vec![
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::PositionX),
                    0,
                    1.0,
                ),
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::PositionY),
                    1,
                    1.0,
                ),
            ],
        ),
        (
            "ADBE Vector Rect Roundness",
            vec![NumericAnimationTarget::float(
                PropertyTarget::layer(id, PropType::RectRoundness),
                0,
                1.0,
            )],
        ),
    ] {
        if let Some(numeric) = numeric_leaf(&leaves, property, &mut warnings) {
            collector.add_leaf_numeric(&leaves, property, &numeric, &targets);
        }
    }
    collector.warnings.extend(warnings);
    Some(rect)
}

#[cfg(test)]
pub(super) fn lower(
    collector: &mut Collector<'_>,
    program: &Program<'_>,
    name: &str,
    parent: LayerId,
    budget: &mut super::OutputBudget,
) -> Result<Option<Vec<FxLayer>>, serde_json::Error> {
    lower_with_context(collector, program, name, parent, budget, None)
}

pub(super) fn lower_with_context(
    collector: &mut Collector<'_>,
    program: &Program<'_>,
    name: &str,
    parent: LayerId,
    budget: &mut super::OutputBudget,
    control_context: Option<(&Layer, &Composition)>,
) -> Result<Option<Vec<FxLayer>>, serde_json::Error> {
    if let Some((layer, composition)) = control_context
        && let Some(layers) =
            caption_box::lower(collector, program, name, parent, budget, layer, composition)?
    {
        return Ok(Some(layers));
    }
    let single = single_source(program);
    let mut dual_warnings = Vec::new();
    let dual = if single.is_none() {
        dual_source(program, &mut dual_warnings)
    } else {
        None
    };
    let (source, fill_run, stroke_run, group_scope) = match (single, dual) {
        (Some((source, paint, scope)), _) => (
            source,
            matches!(
                paint.name,
                "ADBE Vector Graphic - Fill" | "ADBE Vector Graphic - G-Fill"
            )
            .then_some(paint),
            (paint.name == "ADBE Vector Graphic - Stroke").then_some(paint),
            scope,
        ),
        (None, Some((source, fill, stroke, scope))) => (source, Some(fill), Some(stroke), scope),
        _ => return Ok(None),
    };
    if source.name != "ADBE Vector Shape - Rect" {
        return Ok(None);
    }
    let run = source.chunks;
    let mut local_warnings = Vec::new();
    let Ok(leaves) = property_group(run, "Rectangle Path") else {
        return Ok(None);
    };
    let expression_driven = [
        "ADBE Vector Rect Size",
        "ADBE Vector Rect Position",
        "ADBE Vector Rect Roundness",
    ]
    .into_iter()
    .filter_map(|property| numeric_leaf(&leaves, property, &mut local_warnings))
    .any(|numeric| numeric.expression_enabled);
    // The two recognized expression grammars are disjoint: at most one applies.
    let controls = match control_context {
        Some((layer, composition)) if expression_driven => {
            match rigged_box::resolve(layer, &leaves).or_else(|rigged| {
                rigged_box::resolve_sliders(layer, composition, &leaves)
                    .map_err(|sliders| (rigged, sliders))
            }) {
                Ok(controls) => Some(controls),
                Err((rigged, sliders)) => {
                    collector.warnings.push(format!(
                        "{name}: expression-driven Rectangle is neither the bounded Rigged Box formula ({rigged}) nor complete Slider references ({sliders}); typed Rect mapping rejected and static editable outline retained"
                    ));
                    return Ok(None);
                }
            }
        }
        _ => None,
    };
    match controls.as_ref().map(|controls| &controls.profile) {
        Some(rigged_box::Profile::RiggedBox { defaulted_x_anchor }) => {
            local_warnings.push(format!("{name}: bounded Rigged Box controls lowered to independent editable Rectangle values/keys; live controller linkage is not retained"));
            if *defaulted_x_anchor {
                local_warnings.push(format!(
                    "{name}: sparse Rigged Box omitted X Anchor slot 4; assumed 0 for the recognized stock profile (native control readback unverified)"
                ));
            }
        }
        Some(rigged_box::Profile::Sliders) => local_warnings.push(format!(
            "{name}: complete Slider references lowered to independent editable Rectangle values/keys; live controller linkage is not retained"
        )),
        None => {}
    }
    let (size, position, roundness) = if let Some(controls) = controls.as_ref() {
        let (Some(size), Some(position), Some(roundness)) = (
            rigged_box::initial(&controls.size, 2),
            rigged_box::initial(&controls.position, 2),
            rigged_box::initial(&controls.roundness, 1),
        ) else {
            return Ok(None);
        };
        ([size[0], size[1]], [position[0], position[1]], roundness[0])
    } else {
        let Some(size) = initial_pair(&leaves, "ADBE Vector Rect Size") else {
            return Ok(None);
        };
        let (Some(position), Some(roundness)) = (
            initial_pair(&leaves, "ADBE Vector Rect Position"),
            initial_scalar(&leaves, "ADBE Vector Rect Roundness"),
        ) else {
            return Ok(None);
        };
        (size, position, roundness)
    };
    if size.iter().any(|value| !value.is_finite() || *value < 0.0)
        || position.iter().any(|value| !value.is_finite())
        || !roundness.is_finite()
        || roundness < 0.0
        || super::direction::reversed(run, &mut local_warnings)
    {
        return Ok(None);
    }
    for (property, dimensions, positive) in [
        ("ADBE Vector Rect Size", 2, true),
        ("ADBE Vector Rect Position", 2, false),
        ("ADBE Vector Rect Roundness", 1, false),
    ] {
        let resolved = controls.as_ref().map(|controls| controls.curve(property));
        let parsed;
        let numeric = if let Some(resolved) = resolved {
            resolved
        } else {
            let Some(value) = numeric_leaf(&leaves, property, &mut local_warnings) else {
                if defaults::numeric(property).is_some()
                    && !leaves.iter().any(|(candidate, _)| *candidate == property)
                {
                    continue;
                }
                return Ok(None);
            };
            parsed = value;
            &parsed
        };
        if numeric.expression_enabled
            || if property == "ADBE Vector Rect Size" {
                !valid_rect_size_animation(size, numeric)
            } else {
                numeric.keyframes.iter().any(|key| {
                    key.values.len() < dimensions
                        || key.values[..dimensions].iter().any(|value| {
                            !value.is_finite()
                                || (positive && *value <= 0.0)
                                || (property == "ADBE Vector Rect Roundness" && *value < 0.0)
                        })
                })
            }
        {
            return Ok(None);
        }
    }
    let animated = |numeric: &crate::properties::NumericProperty| {
        numeric.animated || !numeric.keyframes.is_empty()
    };
    if fill_run.is_some_and(|paint| paint.name == "ADBE Vector Graphic - G-Fill")
        && ["ADBE Vector Rect Size", "ADBE Vector Rect Position"]
            .into_iter()
            .any(|name| match controls.as_ref() {
                // An expression-backed native leaf looks static; its resolved
                // control curve is what the Rect animates.
                Some(controls) => animated(controls.curve(name)),
                None => numeric_leaf(&leaves, name, &mut local_warnings)
                    .is_some_and(|numeric| animated(&numeric)),
            })
    {
        // Rect paint coordinates have no FX gradient-axis animation target. A
        // changing size or position needs a changing inverse axis offset.
        return Ok(None);
    }
    let mut paint_warnings = Vec::new();
    if fill_run.is_none() && stroke_run.is_none() {
        return Ok(None);
    }
    let fill = if let Some(paint) = fill_run {
        if paint.name == "ADBE Vector Graphic - G-Fill" {
            let Ok((fill, warnings)) = gradient_fill(paint.chunks) else {
                return Ok(None);
            };
            paint_warnings = warnings;
            Some(fill)
        } else {
            collector.solid_fill(paint.chunks, control_context).ok()
        }
    } else {
        None
    };
    let stroke = stroke_run.and_then(|paint| solid_stroke(paint.chunks).ok());
    if (fill_run.is_some() && fill.is_none()) || (stroke_run.is_some() && stroke.is_none()) {
        return Ok(None);
    }
    let opacity = fill.as_ref().map_or_else(
        || stroke.as_ref().map_or(1.0, |value| value.opacity),
        |value| value.opacity,
    );
    let paint_color = |paint: &ShapePaint| match paint {
        ShapePaint::Solid { color } => Some(*color),
        ShapePaint::Gradient { stops, .. } => stops.first().map(|stop| stop.color),
    };
    let fill_color = fill.as_ref().and_then(|value| paint_color(&value.paint));
    let stroke_color = stroke.as_ref().and_then(|value| paint_color(&value.paint));
    if fill.as_ref().is_some_and(|_| fill_color.is_none())
        || stroke.as_ref().is_some_and(|_| stroke_color.is_none())
        || fill_color
            .iter()
            .chain(stroke_color.iter())
            .flatten()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Ok(None);
    }
    if stroke
        .as_ref()
        .is_some_and(|value| !value.dashes.is_empty() || value.dash_offset != 0.0)
    {
        // Rect and AE rectangles have different contour start anchors for dashes.
        return Ok(None);
    }
    if group_scope.is_some_and(|scope| !scope.enabled) {
        return Ok(None);
    }
    local_warnings.extend(paint_warnings);
    local_warnings.extend(dual_warnings);
    let group_blend = group_scope
        .and_then(|scope| scope.operation)
        .map(|operation| blend::from_run(operation.chunks, &mut local_warnings))
        .unwrap_or_default();
    let preserve_group = group_scope.is_some()
        && (!identity_group(group_scope, &mut local_warnings)
            || group_blend != fx_schema::BlendMode::Normal);
    let start = collector.animations.len();
    let animation_checkpoint = collector.animation_budget.checkpoint();
    let id_checkpoint = collector.id_checkpoint();
    let mut owned_group = None;
    if preserve_group {
        let Some(group_id) = collector.allocate() else {
            return Ok(Some(Vec::new()));
        };
        let mut group = crate::structure_document::group(
            group_id,
            format!("{name}: vector group"),
            Some(parent),
            full_active_range(),
        );
        group.blend_mode = group_blend;
        if let Some(run) = group_scope.and_then(|scope| scope.transform) {
            group.transform = super::decode_transform(run, &mut local_warnings);
            collector.add_transform_entries(run, group_id);
        }
        owned_group = Some(group);
    }
    let rect_parent = owned_group.as_ref().map_or(parent, |group| group.id);
    let Some(id) = collector.allocate() else {
        collector.animations.truncate(start);
        collector.animation_budget.rollback(animation_checkpoint);
        collector.restore_ids(id_checkpoint);
        return Ok(Some(Vec::new()));
    };
    let mut transform = identity_transform();
    transform.anchor_point = [size[0] / 2.0, size[1] / 2.0];
    transform.position = Position::TwoD([position[0], position[1]]);
    transform.opacity = PercentageProperty::new((opacity * 100.0).clamp(0.0, 100.0))
        .unwrap_or_else(|| identity_transform().opacity);
    // AE gradient axes are in the vector group's coordinates, whereas FX Rect
    // paint coordinates use its top-left origin before the half-size anchor.
    // An animated Rect size/position is rejected above: the inverse offset
    // would need a paint-axis animation target that current FX does not have.
    let fill_paint = fill.as_ref().and_then(|fill| match &fill.paint {
        ShapePaint::Gradient {
            gradient_type,
            start,
            end,
            stops,
        } => Some(ShapePaint::Gradient {
            gradient_type: *gradient_type,
            start: [
                start[0] - position[0] + size[0] / 2.0,
                start[1] - position[1] + size[1] / 2.0,
            ],
            end: [
                end[0] - position[0] + size[0] / 2.0,
                end[1] - position[1] + size[1] / 2.0,
            ],
            stops: stops.clone(),
        }),
        ShapePaint::Solid { .. } => None,
    });
    let mut rect = RectLayer {
        id,
        name: name.to_owned(),
        description: "Editable AE parametric Rectangle and owned paint".into(),
        is_hidden: false,
        parent: Some(rect_parent),
        blend_mode: blend::from_run(
            fill_run
                .or(stroke_run)
                .expect("paint presence checked above")
                .chunks,
            &mut local_warnings,
        ),
        track_matte: None,
        masks: Vec::new(),
        active_range: full_active_range(),
        effects: Vec::new(),
        motion_blur: false,
        transform,
        rect: RectShape {
            size: [size[0], size[1]],
            position: [0.0, 0.0],
            roundness,
            fill_enabled: fill.is_some(),
            fill_color: fill_color.unwrap_or([0.0, 0.0, 0.0, 1.0]),
            fill_paint,
            fill_blend_mode: None,
            stroke_enabled: stroke.is_some(),
            stroke_color,
            stroke_width: stroke
                .as_ref()
                .map_or(Default::default(), |value| value.width),
            stroke_dashes: Vec::new(),
            stroke_dash_offset: 0.0,
            stroke_join: stroke
                .as_ref()
                .map_or(ShapeLineJoin::default(), |value| value.join),
            stroke_miter_limit: stroke.as_ref().map_or(4.0, |value| value.miter_limit),
        },
    };
    // A Slider-resolved Size component that provably never changes keeps only
    // its static half-size anchor: a flat anchor track would carry an easing
    // that differs from the moving axis, which one native anchor path rejects.
    let constant_axes = match controls.as_ref() {
        Some(controls) if matches!(controls.profile, rigged_box::Profile::Sliders) => {
            [0, 1].map(|axis| constant_component(&controls.size, axis, size[axis]))
        }
        _ => [false; 2],
    };
    let mut size_targets = vec![NumericAnimationTarget::vector2(
        PropertyTarget::layer(id, PropType::RectSize),
        [0, 1],
        [1.0, 1.0],
    )];
    let anchors = [PropType::AnchorPointX, PropType::AnchorPointY];
    for (axis, anchor) in anchors.into_iter().enumerate() {
        if !constant_axes[axis] {
            size_targets.push(NumericAnimationTarget::float(
                PropertyTarget::layer(id, anchor),
                axis,
                0.5,
            ));
        }
    }
    for (property, targets) in [
        ("ADBE Vector Rect Size", size_targets),
        (
            "ADBE Vector Rect Position",
            vec![
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::PositionX),
                    0,
                    1.0,
                ),
                NumericAnimationTarget::float(
                    PropertyTarget::layer(id, PropType::PositionY),
                    1,
                    1.0,
                ),
            ],
        ),
        (
            "ADBE Vector Rect Roundness",
            vec![NumericAnimationTarget::float(
                PropertyTarget::layer(id, PropType::RectRoundness),
                0,
                1.0,
            )],
        ),
    ] {
        if let Some(controls) = controls.as_ref() {
            collector.add_numeric(property, controls.curve(property), &targets);
        } else if let Some(numeric) = numeric_leaf(&leaves, property, &mut local_warnings) {
            collector.add_leaf_numeric(&leaves, property, &numeric, &targets);
        } else if defaults::numeric(property).is_none() {
            local_warnings.push(format!(
                "{property} has no native numeric data; base Rect retained"
            ));
        }
    }
    let entries: Vec<_> = [fill_run, stroke_run]
        .into_iter()
        .flatten()
        .map(|paint| (paint.name, paint.chunks))
        .collect();
    let decoration = collector.decorations(&entries, control_context);
    if let Some(stroke) = decoration.strokes.first() {
        rect.rect.stroke_width = stroke.width;
    }
    collector.add_scope_entries(
        &entries,
        &decoration,
        &[FxLayer::Rect(rect.clone())],
        control_context,
    );
    collector.warnings.extend(local_warnings);
    let output = if let Some(mut group) = owned_group {
        // Keep the two coordinate systems and alpha channels independent:
        // Rectangle Position/Size affect its center anchor; the vector-group
        // transform and opacity surround the complete painted Rectangle.
        group.layers = match crate::structure_document::stored_layers(vec![FxLayer::Rect(rect)]) {
            Ok(layers) => layers,
            Err(error) => {
                collector.animations.truncate(start);
                collector.animation_budget.rollback(animation_checkpoint);
                collector.restore_ids(id_checkpoint);
                return Err(error);
            }
        };
        FxLayer::Group(group)
    } else {
        if group_scope.is_some() {
            collector.warnings.push(format!(
                "{name}: identity vector-group wrapper normalized into a native FX Rect; group name and enable control are not independently editable"
            ));
        }
        FxLayer::Rect(rect)
    };
    if !budget.reserve(&(&output, &collector.animations[start..])) {
        collector.animations.truncate(start);
        collector.animation_budget.rollback(animation_checkpoint);
        collector.restore_ids(id_checkpoint);
        collector.warnings.push(super::budget::EXHAUSTED.into());
        return Ok(Some(Vec::new()));
    }
    Ok(Some(vec![output]))
}

pub(super) fn identity_group(
    group_scope: Option<&super::program::Scope<'_>>,
    warnings: &mut Vec<String>,
) -> bool {
    let Some(scope) = group_scope else {
        return true;
    };
    if !scope.enabled {
        return false;
    }
    let Some(transform) = scope.transform else {
        return true;
    };
    let mut transform_warnings = Vec::new();
    let decoded = super::decode_transform(transform, &mut transform_warnings);
    let leaves = match property_group(transform, "shape transform") {
        Ok(leaves) => leaves,
        Err(_) => {
            warnings.extend(transform_warnings);
            return false;
        }
    };
    let dynamic = [
        "ADBE Vector Anchor",
        "ADBE Vector Position",
        "ADBE Vector Scale",
        "ADBE Vector Rotation",
        "ADBE Vector Skew",
        "ADBE Vector Skew Axis",
        "ADBE Vector Group Opacity",
    ]
    .iter()
    .filter_map(|name| numeric_leaf(&leaves, name, &mut transform_warnings))
    .any(|value| value.animated || !value.keyframes.is_empty() || value.expression_enabled);
    let identity = decoded == identity_transform() && !dynamic && transform_warnings.is_empty();
    warnings.extend(transform_warnings);
    identity
}

fn initial_scalar(leaves: &[(&str, &[Chunk])], name: &str) -> Option<f64> {
    let Some(numeric) = numeric_leaf(leaves, name, &mut Vec::new()) else {
        if leaves.iter().any(|(property, _)| *property == name) {
            return None;
        }
        return defaults::numeric(name)?.first().copied();
    };
    if numeric.animated {
        numeric.keyframes.first()?.values.first().copied()
    } else {
        numeric.values.first().copied()
    }
}

fn initial_pair(leaves: &[(&str, &[Chunk])], name: &str) -> Option<[f64; 2]> {
    let numeric = numeric_leaf(leaves, name, &mut Vec::new());
    let values = match numeric.as_ref() {
        Some(numeric) if numeric.animated => numeric.keyframes.first()?.values.as_slice(),
        Some(numeric) => numeric.values.as_slice(),
        None if !leaves.iter().any(|(property, _)| *property == name) => defaults::numeric(name)?,
        None => return None,
    };
    Some([*values.first()?, *values.get(1)?])
}

/// Whether every key stores `base` in `component` with zero temporal speed on
/// both sides. Equal key values alone do not prove it: a nonzero Bezier speed
/// overshoots between them.
fn constant_component(
    numeric: &crate::properties::NumericProperty,
    component: usize,
    base: f64,
) -> bool {
    numeric.keyframes.iter().all(|key| {
        key.values.get(component) == Some(&base)
            && key.in_speed.get(component) == Some(&0.0)
            && key.out_speed.get(component) == Some(&0.0)
    })
}

fn valid_rect_size_animation(
    initial: [f64; 2],
    numeric: &crate::properties::NumericProperty,
) -> bool {
    let valid_keys = numeric.keyframes.iter().all(|key| {
        key.values.len() >= 2
            && key.values[..2]
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0)
    });
    valid_keys
        && initial
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
        && (initial.iter().all(|value| *value > 0.0)
            || (numeric.animated
                && numeric.keyframes.iter().any(|key| {
                    key.values.len() >= 2 && key.values[..2].iter().all(|value| *value > 0.0)
                })))
}

pub(super) fn single_source<'a>(
    program: &'a Program<'a>,
) -> Option<(
    &'a super::program::NativeRun<'a>,
    &'a super::program::NativeRun<'a>,
    Option<&'a super::program::Scope<'a>>,
)> {
    if program.geometry.len() != 1 {
        return None;
    }
    let mut enabled = program
        .paints
        .iter()
        .enumerate()
        .filter(|(_, paint)| paint.enabled);
    let (paint_index, paint) = enabled.next()?;
    if enabled.next().is_some() {
        return None;
    }
    let draws_paint = |scope: &super::program::Scope<'_>| {
        scope
            .draws
            .iter()
            .filter(|draw| match draw {
                Draw::Paint(id) => program.paints[id.0].enabled,
                _ => true,
            })
            .eq([&Draw::Paint(PaintId(paint_index))])
    };
    let scope = match program.scopes.as_slice() {
        [root] if root.enabled && draws_paint(root) => None,
        [root, group]
            if root.enabled
                && group.enabled
                && root.draws == [Draw::Group(ScopeId(1))]
                && draws_paint(group)
                && group.parent == Some(ScopeId(0)) =>
        {
            Some(group)
        }
        _ => return None,
    };
    let geometry = &program.geometry[0];
    let owner = if scope.is_some() {
        ScopeId(1)
    } else {
        ScopeId(0)
    };
    if !paint.enabled
        || paint.owner != owner
        || paint.geometry != [super::program::GeometryId(0)]
        || geometry.owner != owner
        || !geometry.modifiers.is_empty()
    {
        return None;
    }
    let GeometryKind::Source(source) = &geometry.kind else {
        return None;
    };
    Some((source, &paint.operation, scope))
}

/// A simple Fill+Stroke pair can share one native FX Rect. More complex paint
/// ownership keeps the ordered Shape fallback rather than changing compositing.
fn dual_source<'a>(
    program: &'a Program<'a>,
    warnings: &mut Vec<String>,
) -> Option<(
    &'a super::program::NativeRun<'a>,
    &'a super::program::NativeRun<'a>,
    &'a super::program::NativeRun<'a>,
    Option<&'a super::program::Scope<'a>>,
)> {
    if program.geometry.len() != 1 || program.paints.len() != 2 {
        return None;
    }
    let scope = match program.scopes.as_slice() {
        [root] if root.enabled && root.draws.len() == 2 => None,
        [root, group]
            if root.enabled
                && group.enabled
                && root.draws == [Draw::Group(ScopeId(1))]
                && group.draws.len() == 2
                && group.parent == Some(ScopeId(0)) =>
        {
            Some(group)
        }
        _ => return None,
    };
    let paint_scope = scope.unwrap_or(&program.scopes[0]);
    let owner = if scope.is_some() {
        ScopeId(1)
    } else {
        ScopeId(0)
    };
    let geometry = &program.geometry[0];
    let GeometryKind::Source(source) = &geometry.kind else {
        return None;
    };
    if geometry.owner != owner || !geometry.modifiers.is_empty() {
        return None;
    }
    let fill_index = program.paints.iter().position(|paint| {
        matches!(
            paint.operation.name,
            "ADBE Vector Graphic - Fill" | "ADBE Vector Graphic - G-Fill"
        )
    })?;
    let stroke_index = program
        .paints
        .iter()
        .position(|paint| paint.operation.name == "ADBE Vector Graphic - Stroke")?;
    if fill_index == stroke_index {
        return None;
    }
    let fill = &program.paints[fill_index];
    let stroke = &program.paints[stroke_index];
    if [fill, stroke].iter().any(|paint| {
        !paint.enabled || paint.owner != owner || paint.geometry != [super::program::GeometryId(0)]
    }) {
        return None;
    }
    let order = paint_scope
        .paint_order(|id| blend::composite_order(program.paints[id.0].operation.chunks, warnings));
    let stroke_above_fill = [
        Draw::Paint(PaintId(stroke_index)),
        Draw::Paint(PaintId(fill_index)),
    ];
    if order != stroke_above_fill || !warnings.is_empty() {
        return None;
    }
    let fill_run = &fill.operation;
    let stroke_run = &stroke.operation;
    if blend::from_run(fill_run.chunks, warnings) != fx_schema::BlendMode::Normal
        || blend::from_run(stroke_run.chunks, warnings) != fx_schema::BlendMode::Normal
        || !warnings.is_empty()
    {
        return None;
    }
    let fill_opacity = if fill_run.name == "ADBE Vector Graphic - G-Fill" {
        // Eligibility only: lower() decodes again and retains gradient diagnostics.
        gradient_fill(fill_run.chunks).ok()?.0.opacity
    } else {
        solid_fill(fill_run.chunks).ok()?.opacity
    };
    let stroke_style = solid_stroke(stroke_run.chunks).ok()?;
    if fill_opacity != 1.0
        || stroke_style.opacity != 1.0
        || stroke_style.cap != fx_schema::layer::ShapeLineCap::Butt
        || !stroke_style.dashes.is_empty()
        || stroke_style.dash_offset != 0.0
    {
        return None;
    }
    for (run, name) in [
        (fill_run.chunks, "ADBE Vector Fill Opacity"),
        (stroke_run.chunks, "ADBE Vector Stroke Opacity"),
    ] {
        let leaves = property_group(run, "dual paint").ok()?;
        if let Some((_, property_run)) = leaves.iter().find(|(candidate, _)| *candidate == name) {
            let numeric = super::numeric_from_run(property_run)?;
            if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
                return None;
            }
        }
    }
    Some((source, fill_run, stroke_run, scope))
}
