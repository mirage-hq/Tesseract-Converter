//! Explicitly approximate, editable complement of a bounded inline union.
use super::*;

pub(super) fn lower(
    mask: &PathMask,
    count: usize,
    index: usize,
    owner: MaskOwner<'_>,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<Vec<NativeMaskSpec>, &'static str> {
    if count != 1
        || mask.layer.is_some()
        || mask.mode != MaskMode::Add
        || !mask.inverted
        || mask.opacity.value().min(1.0) != 1.0
        || !mask.expansion.is_finite()
        || mask.expansion < 0.0
        || mask
            .feather
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        || dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return Err(
            "requires one static opaque inverted Add inline mask with nonnegative finite controls",
        );
    }
    let path = mask
        .legacy_path
        .as_ref()
        .ok_or("requires inline geometry")?;
    if !path.is_finite() {
        return Err("non-finite compound geometry");
    }
    let starts: Vec<_> = path
        .commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| matches!(c, ShapePathCommand::MoveTo { .. }).then_some(i))
        .collect();
    if starts.len() != 2 || starts[0] != 0 {
        return Err("requires exactly two contours");
    }
    let paths = [
        ShapePath {
            commands: path.commands[..starts[1]].to_vec(),
        },
        ShapePath {
            commands: path.commands[starts[1]..].to_vec(),
        },
    ];
    let signs = [winding(&paths[0])?, winding(&paths[1])?];
    if signs[0] != signs[1] {
        return Err("opposite-winding contours may encode a hole");
    }
    paths
        .into_iter()
        .enumerate()
        .map(|(ordinal, path)| {
            let mut contour = mask.clone();
            contour.legacy_path = Some(path);
            contour.mode = MaskMode::Subtract;
            contour.inverted = false;
            let (mut spec, _, _) = lower_mask(&contour, index, owner, &[], dynamics)?;
            spec.name = format!("Mask {index} contour {}", ordinal + 1);
            Ok(spec)
        })
        .collect()
}

// This is admission for a small authored profile, not a cubic Boolean operation.
// Require a strictly convex endpoint polygon and monotone handles along each
// chord. Keep all original control points; never approximate the exported path.
fn winding(path: &ShapePath) -> Result<bool, &'static str> {
    if !matches!(path.commands.last(), Some(ShapePathCommand::Close)) || path.commands.len() > 33 {
        return Err("requires closed bounded contours");
    }
    let mut points: Vec<(f64, f64)> = Vec::new();
    for command in &path.commands {
        match command {
            ShapePathCommand::MoveTo {
                mirror: None,
                corner_radius: None,
                ..
            }
            | ShapePathCommand::LineTo {
                mirror: None,
                corner_radius: None,
                ..
            }
            | ShapePathCommand::CubicTo {
                mirror: None,
                corner_radius: None,
                ..
            } => {
                let end = command.endpoint().ok_or("missing endpoint")?;
                if let ShapePathCommand::CubicTo {
                    c1x, c1y, c2x, c2y, ..
                } = command
                {
                    let &(x, y) = points.last().ok_or("cubic without start")?;
                    let dx = end.0 - x;
                    let dy = end.1 - y;
                    let length = dx * dx + dy * dy;
                    let first = (c1x - x) * dx + (c1y - y) * dy;
                    let second = (c2x - x) * dx + (c2y - y) * dy;
                    if !length.is_finite()
                        || !first.is_finite()
                        || !second.is_finite()
                        || length <= 0.0
                        || first < 0.0
                        || first > second
                        || second > length
                    {
                        return Err("cubic handles must progress along their chord");
                    }
                }
                points.push(end);
            }
            ShapePathCommand::Close => {}
            _ => return Err("unsupported contour command or vertex modifiers"),
        }
    }
    if points.last() == points.first() {
        points.pop();
    }
    if points.len() < 3 {
        return Err("degenerate contour");
    }
    let mut sign = None;
    // Every other endpoint must lie strictly on the same side of each edge.
    // Unlike consecutive-turn tests, this also excludes star polygons.
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        for (j, c) in points.iter().enumerate() {
            if j == i || j == (i + 1) % points.len() {
                continue;
            }
            let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if !cross.is_finite() || cross == 0.0 {
                return Err("non-strict convex endpoint polygon");
            }
            let next = cross > 0.0;
            if sign.is_some_and(|previous| previous != next) {
                return Err("nonconvex endpoint polygon");
            }
            sign = Some(next);
        }
    }
    sign.ok_or("degenerate contour")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inverted_compound_geometry_guards() {
        let path = rectangle_path([0.0, 0.0], [100.0, 100.0]);
        assert_eq!(winding(&path), Ok(true));
        let mut reversed = path.clone();
        reversed.commands.swap(1, 3);
        assert_eq!(winding(&reversed), Ok(false));
        let mut open = path.clone();
        open.commands.pop();
        assert!(winding(&open).is_err());
        let mut concave = path.clone();
        concave.commands[2] = anchor(20.0, 20.0, false);
        assert!(winding(&concave).is_err());
        let mut modifiers = path;
        modifiers.commands[1] = ShapePathCommand::LineTo {
            x: 100.0,
            y: 0.0,
            mirror: None,
            corner_radius: Some(serde_json::from_value(serde_json::json!(2.0)).unwrap()),
        };
        assert!(winding(&modifiers).is_err());
    }
}
