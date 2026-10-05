//! Encode editable AE26 Bézier contours as native `om-s/omks/shap`.
//! Fields are constructed from FX commands; no source AEP subtree is reused.

use fx_schema::layer::{ShapePath, ShapePathCommand};

use super::{AepWriteError, views};
use crate::{rifx::Chunk, schema::view_records::StaticPropertyRecord};

const MAX_PATH_VERTICES: usize = 21_845;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_fps_path_changes_only_property_clock_not_geometry() {
        let path = ShapePath {
            commands: vec![
                ShapePathCommand::MoveTo {
                    x: 0.0,
                    y: 0.0,
                    mirror: None,
                    corner_radius: None,
                },
                ShapePathCommand::LineTo {
                    x: 10.0,
                    y: 20.0,
                    mirror: None,
                    corner_radius: None,
                },
            ],
        };
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let old = property(&path).unwrap();
        let new = property_with_clock(&path, clock).unwrap();
        fn collect(chunk: &Chunk, properties: &mut Vec<Vec<u8>>, geometry: &mut Vec<Vec<u8>>) {
            if chunk.id() == *b"tdb4" {
                properties.push(chunk.data_payload().unwrap().to_vec());
            }
            if chunk.id() == *b"ldat" {
                geometry.push(chunk.data_payload().unwrap().to_vec());
            }
            if let Some(children) = chunk.children() {
                for child in children {
                    collect(child, properties, geometry);
                }
            }
        }
        let (mut old_props, mut old_geometry) = (Vec::new(), Vec::new());
        let (mut new_props, mut new_geometry) = (Vec::new(), Vec::new());
        collect(&old, &mut old_props, &mut old_geometry);
        collect(&new, &mut new_props, &mut new_geometry);
        assert_eq!(old_props.len(), 1);
        assert_eq!(
            u32::from_be_bytes(old_props[0][12..16].try_into().unwrap()),
            24_576
        );
        assert_eq!(
            u32::from_be_bytes(new_props[0][12..16].try_into().unwrap()),
            30_720
        );
        old_props[0][12..16].fill(0);
        new_props[0][12..16].fill(0);
        assert_eq!(old_props, new_props);
        assert_eq!(old_geometry.len(), 1);
        assert_eq!(old_geometry, new_geometry);
    }
}

mod animation;
pub(super) use animation::animated_property;
pub(crate) use animation::{PathKeyframe, PathTrack, split_path_track, validate_path_track};

pub(super) fn property_with_clock(
    path: &ShapePath,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut descriptor = StaticPropertyRecord::new(1, 7, 1, 0x20007, 0x10008, 0, false);
    descriptor.set_initialized();
    let mut descriptor = descriptor.encode();
    descriptor[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
    Ok(Chunk::list(
        *b"om-s",
        vec![
            Chunk::list(
                *b"tdbs",
                vec![
                    Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
                    views::name_payload("-_0_/-")?,
                    Chunk::data(*b"tdb4", descriptor)?,
                    Chunk::data(*b"cdat", [0_u8; 4])?,
                ],
            ),
            Chunk::list(*b"omks", vec![shape(path)?]),
        ],
    ))
}

#[derive(Clone, Copy)]
struct Vertex {
    point: [f64; 2],
    incoming: [f64; 2],
    outgoing: [f64; 2],
}

/// Splits an FX compound path into independently encodable native contours.
///
/// AE stores each contour in its own Path property. Keeping the returned
/// records adjacent lets their existing paint and modifier operators continue
/// to consume the compound geometry as one authored scope.
pub(super) fn contours(path: &ShapePath) -> Result<Vec<ShapePath>, AepWriteError> {
    if path.commands.is_empty() {
        return Ok(vec![empty_path()]);
    }

    let mut contours = Vec::new();
    let mut commands = Vec::new();
    for command in &path.commands {
        match command {
            ShapePathCommand::MoveTo { .. } => {
                if !commands.is_empty() {
                    contours.push(ShapePath {
                        commands: std::mem::take(&mut commands),
                    });
                }
                commands.push(command.clone());
            }
            ShapePathCommand::LineTo { .. } | ShapePathCommand::CubicTo { .. } => {
                if commands.is_empty() {
                    return Err(AepWriteError::Invalid(
                        "native compound Path contour must start with MoveTo",
                    ));
                }
                commands.push(command.clone());
            }
            ShapePathCommand::Close => {
                if commands.is_empty() {
                    return Err(AepWriteError::Invalid(
                        "native compound Path Close needs an open contour",
                    ));
                }
                commands.push(ShapePathCommand::Close);
                contours.push(ShapePath {
                    commands: std::mem::take(&mut commands),
                });
            }
        }
    }
    if !commands.is_empty() {
        contours.push(ShapePath { commands });
    }
    if contours.is_empty() {
        return Err(AepWriteError::Invalid(
            "native compound Path needs at least one contour",
        ));
    }
    Ok(contours)
}

// AE rejects zero vertices but accepts an open one-vertex path. This is only
// the geometry placeholder: the lowerer must also hide empty painted groups
// because some native stroke/paint orders turn round caps into visible dots.
fn empty_path() -> ShapePath {
    ShapePath {
        commands: vec![ShapePathCommand::MoveTo {
            x: 0.0,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        }],
    }
}

pub(super) fn validated_contours(path: &ShapePath) -> Result<Vec<ShapePath>, AepWriteError> {
    let contours = contours(path)?;
    for contour in &contours {
        property(contour)?;
    }
    Ok(contours)
}

pub(super) fn property(path: &ShapePath) -> Result<Chunk, AepWriteError> {
    property_with_clock(path, super::keyframes::PropertyClock::DEFAULT)
}

fn shape(path: &ShapePath) -> Result<Chunk, AepWriteError> {
    shape_with_seam(path, true)
}

fn shape_with_seam(path: &ShapePath, fold_seam: bool) -> Result<Chunk, AepWriteError> {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut closed = false;
    for (index, command) in path.commands.iter().enumerate() {
        match command {
            ShapePathCommand::MoveTo {
                x,
                y,
                mirror,
                corner_radius,
            } if index == 0 => {
                if mirror.is_some() || corner_radius.is_some() {
                    return Err(AepWriteError::Invalid(
                        "Path mirror/corner controls need native geometry modifiers",
                    ));
                }
                vertices.push(Vertex {
                    point: [*x, *y],
                    incoming: [*x, *y],
                    outgoing: [*x, *y],
                });
            }
            ShapePathCommand::LineTo {
                x,
                y,
                mirror,
                corner_radius,
            } if !vertices.is_empty() && !closed => {
                if mirror.is_some() || corner_radius.is_some() {
                    return Err(AepWriteError::Invalid(
                        "Path mirror/corner controls need native geometry modifiers",
                    ));
                }
                vertices.push(Vertex {
                    point: [*x, *y],
                    incoming: [*x, *y],
                    outgoing: [*x, *y],
                });
            }
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                mirror,
                corner_radius,
            } if !vertices.is_empty() && !closed => {
                if mirror.is_some() || corner_radius.is_some() {
                    return Err(AepWriteError::Invalid(
                        "Path mirror/corner controls need native geometry modifiers",
                    ));
                }
                vertices.last_mut().expect("nonempty contour").outgoing = [*c1x, *c1y];
                vertices.push(Vertex {
                    point: [*x, *y],
                    incoming: [*c2x, *c2y],
                    outgoing: [*x, *y],
                });
            }
            ShapePathCommand::Close if index + 1 == path.commands.len() && vertices.len() > 1 => {
                closed = true;
            }
            _ => {
                return Err(AepWriteError::Invalid(
                    "native Path supports one MoveTo, segments and an optional terminal Close",
                ));
            }
        }
    }
    if vertices.is_empty() || vertices.len() > MAX_PATH_VERTICES {
        return Err(AepWriteError::Invalid(
            "native Path vertex count must fit a u16 triple list",
        ));
    }
    if fold_seam
        && closed
        && vertices
            .last()
            .is_some_and(|last| last.point == vertices[0].point)
    {
        let last = vertices.pop().expect("at least two vertices");
        vertices[0].incoming = last.incoming;
    }
    // A closed cubic may return to its sole vertex. After seam folding its
    // outgoing/incoming handles still define a drawable native Bezier loop;
    // Adobe does not require two distinct vertices for a closed contour.
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for point in vertices
        .iter()
        .flat_map(|vertex| [vertex.point, vertex.incoming, vertex.outgoing])
    {
        if point
            .iter()
            .any(|value| !value.is_finite() || value.abs() > f32::MAX as f64)
        {
            return Err(AepWriteError::Invalid(
                "native Path coordinates must be finite f32 values",
            ));
        }
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    }
    let bounds = bounds.map(|value| (value as f32) as f64);
    let mut header = [0_u8; 24];
    header[..4].copy_from_slice(&[0xb3, 0xde, 0x02, if closed { 1 } else { 9 }]);
    for (slot, value) in bounds.iter().enumerate() {
        header[4 + slot * 4..8 + slot * 4].copy_from_slice(&(*value as f32).to_be_bytes());
    }
    header[20] = 1;
    let count = vertices.len() * 3;
    let blocks = count.div_ceil(4);
    let mut list_header = [0_u8; 52];
    list_header[..4].copy_from_slice(&[0, 0xd0, 0x0b, 0xee]);
    list_header[10..12].copy_from_slice(&(count as u16).to_be_bytes());
    list_header[12..16].copy_from_slice(&(blocks as u32).to_be_bytes());
    list_header[18..20].copy_from_slice(&8_u16.to_be_bytes());
    list_header[23] = 4;
    list_header[24..28].copy_from_slice(&1_u32.to_be_bytes());
    list_header[28..32].copy_from_slice(&((blocks * 4) as u32).to_be_bytes());
    let mut points = Vec::with_capacity(count * 8);
    for (index, vertex) in vertices.iter().enumerate() {
        // Native triples describe a segment: its starting vertex, outgoing
        // handle, then the NEXT vertex's incoming handle. The final slot wraps
        // even for an open contour; its unused closing segment is not drawn.
        let next = &vertices[(index + 1) % vertices.len()];
        for point in [vertex.point, vertex.outgoing, next.incoming] {
            for axis in 0..2 {
                let span = bounds[axis + 2] - bounds[axis];
                let value = if span == 0.0 {
                    0.0
                } else {
                    (point[axis] - bounds[axis]) / span
                };
                if !value.is_finite() || value.abs() > f32::MAX as f64 {
                    return Err(AepWriteError::Invalid(
                        "native normalized Path coordinate is invalid",
                    ));
                }
                points.extend_from_slice(&(value as f32).to_be_bytes());
            }
        }
    }
    Ok(Chunk::list(
        *b"shap",
        vec![
            Chunk::data(*b"shph", header)?,
            Chunk::list(
                *b"list",
                vec![
                    Chunk::data(*b"lhd3", list_header)?,
                    Chunk::data(*b"ldat", points)?,
                ],
            ),
            Chunk::data(*b"omtn", Vec::new())?,
        ],
    ))
}
