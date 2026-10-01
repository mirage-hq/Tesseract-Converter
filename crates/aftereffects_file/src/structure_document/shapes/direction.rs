//! AE winding: Bodymovin's pinned exporter preserves 1/2 and reverses 3.
use super::*;
use fx_schema::PropertyAnimator;

#[cfg(test)]
mod tests;

pub(super) fn reversed(run: &[Chunk], warnings: &mut Vec<String>) -> bool {
    let Ok(leaves) = property_group(run, "shape direction") else {
        return false;
    };
    if leaves
        .iter()
        .filter(|(name, _)| *name == "ADBE Vector Shape Direction")
        .count()
        > 1
    {
        warnings.push("duplicate shape direction leaves; default winding retained".into());
        return false;
    }
    let Some(value) = numeric_leaf(&leaves, "ADBE Vector Shape Direction", warnings) else {
        return false;
    };
    if value.animated || value.expression_enabled {
        warnings.push("animated/expression shape direction retained at its stored initial value; AE expressions are not executed".into());
    }
    match base_component(&value, 0) {
        Some(1.0 | 2.0) => false,
        Some(3.0) => true,
        other => {
            warnings.push(format!(
                "unknown/missing shape direction {other:?}; default winding retained"
            ));
            false
        }
    }
}

fn anchor(command: &ShapePathCommand, move_to: bool) -> Option<ShapePathCommand> {
    let (x, y, mirror, corner_radius) = match command {
        ShapePathCommand::MoveTo {
            x,
            y,
            mirror,
            corner_radius,
        }
        | ShapePathCommand::LineTo {
            x,
            y,
            mirror,
            corner_radius,
        }
        | ShapePathCommand::CubicTo {
            x,
            y,
            mirror,
            corner_radius,
            ..
        } => (*x, *y, *mirror, *corner_radius),
        ShapePathCommand::Close => return None,
    };
    Some(if move_to {
        ShapePathCommand::MoveTo {
            x,
            y,
            mirror,
            corner_radius,
        }
    } else {
        ShapePathCommand::LineTo {
            x,
            y,
            mirror,
            corner_radius,
        }
    })
}

/// Native source outlines have one contour. Closed paths retain their first
/// anchor; open paths start at the old endpoint. Cubic handles swap roles.
pub(super) fn reverse(path: &ShapePath) -> Option<ShapePath> {
    let closed = matches!(path.commands.last(), Some(ShapePathCommand::Close));
    let points = &path.commands[..path.commands.len().checked_sub(usize::from(closed))?];
    let first = points.first()?;
    let last = points.last()?;
    if !matches!(first, ShapePathCommand::MoveTo { .. })
        || points[1..].iter().any(|command| {
            matches!(
                command,
                ShapePathCommand::MoveTo { .. } | ShapePathCommand::Close
            )
        })
    {
        return None;
    }
    let mut segments: Vec<_> = points.windows(2).map(|pair| (&pair[0], &pair[1])).collect();
    let closing = anchor(first, false)?;
    if closed && last.endpoint() != first.endpoint() {
        segments.push((last, &closing));
    }
    let mut commands = vec![anchor(if closed { first } else { last }, true)?];
    for (from, to) in segments.into_iter().rev() {
        let ShapePathCommand::LineTo {
            x,
            y,
            mirror,
            corner_radius,
        } = anchor(from, false)?
        else {
            return None;
        };
        commands.push(match to {
            ShapePathCommand::CubicTo {
                c1x, c1y, c2x, c2y, ..
            } => ShapePathCommand::CubicTo {
                c1x: *c2x,
                c1y: *c2y,
                c2x: *c1x,
                c2y: *c1y,
                x,
                y,
                mirror,
                corner_radius,
            },
            _ => ShapePathCommand::LineTo {
                x,
                y,
                mirror,
                corner_radius,
            },
        });
    }
    if closed {
        if matches!(commands.last(), Some(ShapePathCommand::LineTo { .. }))
            && commands.last()?.endpoint() == first.endpoint()
        {
            commands.pop();
        }
        commands.push(ShapePathCommand::Close);
    }
    Some(ShapePath { commands })
}

pub(super) fn apply(
    layer: &mut ShapeLayer,
    entries: &mut [AnimationGraphEntry],
    warnings: &mut Vec<String>,
) -> Result<(), serde_json::Error> {
    if let Some(ellipse) = &mut layer.shape.ellipse {
        ellipse.reversed = true;
        return Ok(());
    }
    if let Some(star) = &mut layer.shape.poly_star {
        star.reversed = true;
        return Ok(());
    }
    let Some(reversed) = reverse(&layer.shape.path) else {
        warnings.push(
            "shape direction could not reverse malformed source contour; original retained".into(),
        );
        return Ok(());
    };
    layer.shape.path = reversed;
    for entry in entries {
        if entry.target != PropertyTarget::layer(layer.id, PropType::ShapePath) {
            continue;
        }
        if entry.animator.is_js_script() {
            warnings.push("reversed animated path has no native FX Path keyframe target; reversed static outline retained without motion".into());
            entry.animator = PropertyAnimator::constant(fx_schema::PropertyValue::Path(
                layer.shape.path.clone(),
            ))?;
            entry.dependencies.clear();
        }
    }
    Ok(())
}
