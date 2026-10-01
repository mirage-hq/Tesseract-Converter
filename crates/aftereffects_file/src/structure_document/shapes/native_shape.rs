//! One unshared native Path, Ellipse or Star/Polygon with one or two owned paints.
//! Uses the FX shape's native editable geometry, not a generated ShapePath script.

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::structure::{Composition, Layer, ProjectItem};

use super::{
    Collector, FxLayer, OutputBudget, ShapePaint, blend, full_active_range, identity_transform,
    native_rect,
    program::{Draw, GeometryKind, PaintId, Program, ScopeId},
    property_group,
};
use fx_schema::{BlendMode, LayerId, PercentageProperty};

#[cfg(test)]
pub(super) fn lower(
    collector: &mut Collector<'_>,
    program: &Program<'_>,
    name: &str,
    parent: LayerId,
    budget: &mut OutputBudget,
) -> Result<Option<Vec<FxLayer>>, serde_json::Error> {
    lower_with_context(collector, program, name, parent, budget, None, None)
}

pub(super) fn lower_with_context(
    collector: &mut Collector<'_>,
    program: &Program<'_>,
    name: &str,
    parent: LayerId,
    budget: &mut OutputBudget,
    control_context: Option<(&Layer, &Composition)>,
    source_items: Option<&HashMap<u32, &ProjectItem>>,
) -> Result<Option<Vec<FxLayer>>, serde_json::Error> {
    let single = native_rect::single_source(program);
    let mut warnings = Vec::new();
    let dual = if single.is_none() {
        dual_source(program, &mut warnings)
    } else {
        None
    };
    let (source, paints, group_scope) = match (single, dual) {
        (Some((source, paint, scope)), _) => (source, vec![paint], scope),
        (None, Some((source, fill, stroke, scope))) => (source, vec![fill, stroke], scope),
        _ => return Ok(None),
    };
    if !matches!(
        source.name,
        "ADBE Vector Shape - Group" | "ADBE Vector Shape - Ellipse" | "ADBE Vector Shape - Star"
    ) {
        return Ok(None);
    }
    if !native_rect::identity_group(group_scope, &mut warnings) {
        return Ok(None);
    }
    let entries: Vec<_> = paints
        .iter()
        .map(|paint| (paint.name, paint.chunks))
        .collect();
    let mut style = collector.decorations(&entries, control_context);
    if style.fills.len() + style.strokes.len() != paints.len() {
        return Ok(None);
    }
    // A single paint's opacity may move onto its layer Transform. Two paints
    // retain independent *static* opacities on their own styles instead.
    let opacity = if paints.len() == 2 {
        1.0
    } else if let Some(fill) = style.fills.first_mut() {
        let value = fill.opacity;
        fill.opacity = 1.0;
        value
    } else if let Some(stroke) = style.strokes.first_mut() {
        let value = stroke.opacity;
        stroke.opacity = 1.0;
        value
    } else {
        return Ok(None);
    };
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Ok(None);
    }
    let group_blend = group_scope
        .and_then(|scope| scope.operation)
        .map_or(BlendMode::Normal, |operation| {
            blend::from_run(operation.chunks, &mut warnings)
        });
    let id_checkpoint = collector.id_checkpoint();
    let start = collector.animations.len();
    let animation_checkpoint = collector.animation_budget.checkpoint();
    let group_id = if group_blend != BlendMode::Normal {
        let Some(id) = collector.allocate() else {
            return Ok(Some(Vec::new()));
        };
        Some(id)
    } else {
        None
    };
    let source_layer = match collector.source_layer_with_sources(
        source.name,
        source.chunks,
        name,
        group_id.unwrap_or(parent),
        Some(&style),
        control_context,
        source_items,
    ) {
        Ok(source_layer) => source_layer,
        Err(error) => {
            collector.animations.truncate(start);
            collector.animation_budget.rollback(animation_checkpoint);
            collector.restore_ids(id_checkpoint);
            return Err(error);
        }
    };
    let Some(FxLayer::Shape(mut shape)) = source_layer else {
        collector.animations.truncate(start);
        collector.animation_budget.rollback(animation_checkpoint);
        collector.restore_ids(id_checkpoint);
        return Ok(Some(Vec::new()));
    };
    shape.name = name.to_owned();
    shape.description = "Native editable AE geometry and owned paint".into();
    shape.active_range = full_active_range();
    shape.blend_mode = if paints.len() == 2 {
        Default::default()
    } else {
        blend::from_run(paints[0].chunks, &mut warnings)
    };
    shape.transform = identity_transform();
    shape.transform.opacity =
        PercentageProperty::new(opacity * 100.0).unwrap_or_else(|| identity_transform().opacity);
    // Unsupported paint types must not become a plausible-looking solid.
    if shape.shape.fills.iter().any(|paint| {
        matches!(&paint.paint, ShapePaint::Solid { color } if color.iter().any(|value| !value.is_finite()))
    }) || shape.shape.strokes.iter().any(|paint| {
        matches!(&paint.paint, ShapePaint::Solid { color } if color.iter().any(|value| !value.is_finite()))
    }) {
        collector.animations.truncate(start);
        collector.animation_budget.rollback(animation_checkpoint);
        collector.restore_ids(id_checkpoint);
        return Ok(None);
    }
    collector.add_scope_entries(
        &entries,
        &style,
        &[FxLayer::Shape(shape.clone())],
        control_context,
    );
    collector.warnings.extend(warnings);
    let output = if let Some(id) = group_id {
        let mut group = super::super::group(
            id,
            format!("{name}: vector blend group"),
            Some(parent),
            full_active_range(),
        );
        group.blend_mode = group_blend;
        group.layers = match super::super::stored_layers(vec![FxLayer::Shape(shape)]) {
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
        FxLayer::Shape(shape)
    };
    if !budget.reserve(&(&output, &collector.animations[start..])) {
        collector.animations.truncate(start);
        collector.animation_budget.rollback(animation_checkpoint);
        collector.restore_ids(id_checkpoint);
        collector.warnings.push(super::budget::EXHAUSTED.into());
        return Ok(Some(Vec::new()));
    }
    if group_scope.is_some() && group_id.is_none() {
        collector.warnings.push(format!(
            "{name}: identity vector-group wrapper normalized into native FX Shape; group name and enable control are not independently editable"
        ));
    }
    Ok(Some(vec![output]))
}

/// Only a paint pair whose native order matches FX's fill-then-stroke renderer
/// can share one editable source Shape without reversing their compositing.
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
    let stroke_index = program.paints.iter().position(|paint| {
        matches!(
            paint.operation.name,
            "ADBE Vector Graphic - Stroke" | "ADBE Vector Graphic - G-Stroke"
        )
    })?;
    let fill = &program.paints[fill_index];
    let stroke = &program.paints[stroke_index];
    if [fill, stroke].iter().any(|paint| {
        !paint.enabled || paint.owner != owner || paint.geometry != [super::program::GeometryId(0)]
    }) {
        return None;
    }
    let order = paint_scope
        .paint_order(|id| blend::composite_order(program.paints[id.0].operation.chunks, warnings));
    if order
        != [
            Draw::Paint(PaintId(stroke_index)),
            Draw::Paint(PaintId(fill_index)),
        ]
        || !warnings.is_empty()
    {
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
    for (run, name) in [
        (fill_run.chunks, "ADBE Vector Fill Opacity"),
        (stroke_run.chunks, "ADBE Vector Stroke Opacity"),
    ] {
        let leaves = property_group(run, "paint opacity").ok()?;
        if let Some((_, property_run)) = leaves.iter().find(|(candidate, _)| *candidate == name) {
            let value = super::numeric_from_run(property_run)?;
            if value.animated || value.expression_enabled || !value.keyframes.is_empty() {
                return None;
            }
        }
    }
    Some((source, fill_run, stroke_run, scope))
}
