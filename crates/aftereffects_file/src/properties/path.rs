//! Native Path geometry shared by editable outlines and expression snapshots.

use fx_schema::{ShapePath, ShapePathCommand};
use serde::{Deserialize, Serialize};

use super::{PropertyError, data, unique_list};
use crate::rifx::Chunk;

const SHPH_BYTES: usize = 24;
const LHD3_BYTES: usize = 52;
const POINT_BYTES: usize = 8;

/// Converter-owned Path-local coordinates; tangents are relative to each vertex.
/// Open paths retain the unused first incoming and last outgoing handles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct NativePathValue {
    pub(crate) vertices: Vec<[f64; 2]>,
    pub(crate) in_tangents: Vec<[f64; 2]>,
    pub(crate) out_tangents: Vec<[f64; 2]>,
    pub(crate) closed: bool,
}

impl NativePathValue {
    /// Rejects malformed DTOs as well as non-finite expression output.
    pub(crate) fn validate(&self) -> Result<(), PropertyError> {
        if self.vertices.is_empty()
            || self.in_tangents.len() != self.vertices.len()
            || self.out_tangents.len() != self.vertices.len()
        {
            return Err(PropertyError::Layout("shape tangent count"));
        }
        if self
            .vertices
            .iter()
            .chain(&self.in_tangents)
            .chain(&self.out_tangents)
            .flatten()
            .any(|value| !value.is_finite())
        {
            return Err(PropertyError::NonFinite);
        }
        Ok(())
    }

    /// Uses the same segment and closing-command policy as native outline import.
    pub(crate) fn to_shape_path(&self, preserve_cubic: bool) -> Result<ShapePath, PropertyError> {
        self.validate()?;
        let mut points = Vec::with_capacity(self.vertices.len() * 3);
        for (index, vertex) in self.vertices.iter().enumerate() {
            let next = (index + 1) % self.vertices.len();
            points.push(*vertex);
            points.push(add(*vertex, self.out_tangents[index]));
            points.push(add(self.vertices[next], self.in_tangents[next]));
        }
        if points.iter().flatten().any(|value| !value.is_finite()) {
            return Err(PropertyError::NonFinite);
        }
        Ok(shape_path(&points, self.closed, preserve_cubic))
    }
}

/// Reads one `shap` record. Scale is supplied by the owner, never inferred here.
pub(crate) fn read_native_path(
    shape: &[Chunk],
    scale: [f64; 2],
) -> Result<NativePathValue, PropertyError> {
    let (points, closed) = read_points(shape, scale)?;
    let count = points.len();
    let mut value = NativePathValue {
        vertices: Vec::with_capacity(count / 3),
        in_tangents: Vec::with_capacity(count / 3),
        out_tangents: Vec::with_capacity(count / 3),
        closed,
    };
    for index in (0..count).step_by(3) {
        let vertex = points[index];
        value.vertices.push(vertex);
        value
            .in_tangents
            .push(subtract(points[(index + count - 1) % count], vertex));
        value.out_tangents.push(subtract(points[index + 1], vertex));
    }
    value.validate()?;
    Ok(value)
}

/// Keeps absolute native control points intact to avoid subtract/add rounding
/// changes in the existing outline importer.
pub(crate) fn read_native_shape_path(
    shape: &[Chunk],
    scale: [f64; 2],
    preserve_cubic: bool,
) -> Result<ShapePath, PropertyError> {
    let (points, closed) = read_points(shape, scale)?;
    Ok(shape_path(&points, closed, preserve_cubic))
}

fn read_points(shape: &[Chunk], scale: [f64; 2]) -> Result<(Vec<[f64; 2]>, bool), PropertyError> {
    if scale.iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::NonFinite);
    }
    let header = data(shape, *b"shph")?;
    if header.len() != SHPH_BYTES || header[3] & 1 == 0 {
        return Err(PropertyError::Layout("shape header"));
    }
    let bounds = [
        read_f32(header, 4)?,
        read_f32(header, 8)?,
        read_f32(header, 12)?,
        read_f32(header, 16)?,
    ];
    let list = unique_list(shape, *b"list")?;
    let list_header = data(list, *b"lhd3")?;
    let points = data(list, *b"ldat")?;
    if list_header.len() != LHD3_BYTES {
        return Err(PropertyError::Layout("shape list header"));
    }
    let count = usize::from(u16::from_be_bytes([list_header[10], list_header[11]]));
    let item_size = usize::from(u16::from_be_bytes([list_header[18], list_header[19]]));
    if item_size != POINT_BYTES || list_header[23] != 4 || count == 0 || count % 3 != 0 {
        return Err(PropertyError::Layout("shape point list"));
    }
    let expected = count
        .checked_mul(item_size)
        .ok_or(PropertyError::Layout("shape point count"))?;
    if points.len() != expected {
        return Err(PropertyError::Layout("shape point data length"));
    }
    let mut decoded = Vec::with_capacity(count);
    for point in points.as_chunks::<POINT_BYTES>().0.iter() {
        let normalized = [read_f32(point, 0)?, read_f32(point, 4)?];
        let absolute = [
            (bounds[0] + (bounds[2] - bounds[0]) * normalized[0]) * scale[0],
            (bounds[1] + (bounds[3] - bounds[1]) * normalized[1]) * scale[1],
        ];
        if absolute.iter().any(|value| !value.is_finite()) {
            return Err(PropertyError::NonFinite);
        }
        decoded.push(absolute);
    }
    Ok((decoded, header[3] & 8 == 0))
}

fn shape_path(decoded: &[[f64; 2]], closed: bool, preserve_cubic: bool) -> ShapePath {
    let count = decoded.len();
    let vertex_count = count / 3;
    let mut commands = Vec::with_capacity(vertex_count + usize::from(closed) + 1);
    let first = decoded[0];
    commands.push(ShapePathCommand::MoveTo {
        x: first[0],
        y: first[1],
        mirror: None,
        corner_radius: None,
    });
    let segment_count = if closed {
        vertex_count
    } else {
        vertex_count - 1
    };
    for segment in 0..segment_count {
        let from = segment % vertex_count;
        let to = (segment + 1) % vertex_count;
        let c1 = decoded[from * 3 + 1];
        let c2 = decoded[(to * 3 + count - 1) % count];
        let end = decoded[to * 3];
        if !preserve_cubic
            && approximately_equal(c1, decoded[from * 3])
            && approximately_equal(c2, end)
        {
            commands.push(ShapePathCommand::LineTo {
                x: end[0],
                y: end[1],
                mirror: None,
                corner_radius: None,
            });
        } else {
            commands.push(ShapePathCommand::CubicTo {
                c1x: c1[0],
                c1y: c1[1],
                c2x: c2[0],
                c2y: c2[1],
                x: end[0],
                y: end[1],
                mirror: None,
                corner_radius: None,
            });
        }
    }
    if closed {
        commands.push(ShapePathCommand::Close);
    }
    ShapePath { commands }
}

fn add(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] + right[0], left[1] + right[1]]
}
fn subtract(left: [f64; 2], right: [f64; 2]) -> [f64; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f64, PropertyError> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or(PropertyError::Layout("shape float"))?
        .try_into()
        .map_err(|_| PropertyError::Layout("shape float"))?;
    let value = f64::from(f32::from_be_bytes(raw));
    value
        .is_finite()
        .then_some(value)
        .ok_or(PropertyError::NonFinite)
}

fn approximately_equal(left: [f64; 2], right: [f64; 2]) -> bool {
    (left[0] - right[0]).abs() <= f64::EPSILON && (left[1] - right[1]).abs() <= f64::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(closed: bool, points: &[[f32; 2]], count: u16) -> Vec<Chunk> {
        let mut header = vec![0; SHPH_BYTES];
        header[3] = if closed { 1 } else { 9 };
        header[12..16].copy_from_slice(&1_f32.to_be_bytes());
        header[16..20].copy_from_slice(&1_f32.to_be_bytes());
        let mut list_header = vec![0; LHD3_BYTES];
        list_header[10..12].copy_from_slice(&count.to_be_bytes());
        list_header[18..20].copy_from_slice(&8_u16.to_be_bytes());
        list_header[23] = 4;
        let bytes: Vec<u8> = points
            .iter()
            .flatten()
            .flat_map(|value| value.to_be_bytes())
            .collect();
        vec![
            Chunk::data(*b"shph", header).unwrap(),
            Chunk::list(
                *b"list",
                vec![
                    Chunk::data(*b"lhd3", list_header).unwrap(),
                    Chunk::data(*b"ldat", bytes).unwrap(),
                ],
            ),
        ]
    }

    #[test]
    fn open_endpoint_handles_survive_native_snapshot() {
        let shape = record(
            false,
            &[
                [1., 2.],
                [3., 4.],
                [5., 6.],
                [7., 8.],
                [9., 10.],
                [11., 12.],
            ],
            6,
        );
        let value = read_native_path(&shape, [2., 3.]).unwrap();
        assert!(!value.closed);
        assert_eq!(value.vertices, vec![[2., 6.], [14., 24.]]);
        assert_eq!(value.in_tangents, vec![[20., 30.], [-4., -6.]]);
        assert_eq!(value.out_tangents, vec![[4., 6.], [4., 6.]]);
        assert_eq!(
            value.to_shape_path(false).unwrap(),
            read_native_shape_path(&shape, [2., 3.], false).unwrap()
        );
        assert_eq!(value.to_shape_path(false).unwrap().commands.len(), 2);
    }

    #[test]
    fn closed_lines_and_preserved_cubics_use_existing_commands() {
        let shape = record(
            true,
            &[[1., 2.], [1., 2.], [3., 4.], [3., 4.], [3., 4.], [1., 2.]],
            6,
        );
        let value = read_native_path(&shape, [1., 1.]).unwrap();
        assert!(value.closed);
        for preserve in [false, true] {
            let path = value.to_shape_path(preserve).unwrap();
            assert_eq!(
                path,
                read_native_shape_path(&shape, [1., 1.], preserve).unwrap()
            );
            assert_eq!(path.commands.last(), Some(&ShapePathCommand::Close));
            assert_eq!(path.commands.len(), 4);
            assert_eq!(
                matches!(path.commands[1], ShapePathCommand::CubicTo { .. }),
                preserve
            );
        }
    }

    #[test]
    fn rejects_malformed_counts_and_nonfinite_native_values() {
        assert!(read_native_path(&[], [1., 1.]).is_err());
        for count in [0, 2, 6] {
            assert!(read_native_path(&record(false, &[[0., 0.]; 3], count), [1., 1.]).is_err());
        }
        assert_eq!(
            read_native_path(&record(false, &[[f32::NAN, 0.]; 3], 3), [1., 1.]),
            Err(PropertyError::NonFinite)
        );
        assert_eq!(
            read_native_path(&record(false, &[[0., 0.]; 3], 3), [f64::INFINITY, 1.]),
            Err(PropertyError::NonFinite)
        );
    }

    #[test]
    fn rejects_invalid_dto_counts_and_nonfinite_or_overflowing_handles() {
        let mut value = NativePathValue {
            vertices: vec![[0., 0.]],
            in_tangents: vec![],
            out_tangents: vec![[0., 0.]],
            closed: false,
        };
        assert!(value.to_shape_path(false).is_err());
        value.in_tangents.push([f64::NAN, 0.]);
        assert_eq!(value.to_shape_path(false), Err(PropertyError::NonFinite));
        value.in_tangents[0] = [f64::MAX, 0.];
        value.vertices[0] = [f64::MAX, 0.];
        assert_eq!(value.to_shape_path(false), Err(PropertyError::NonFinite));
    }
}
