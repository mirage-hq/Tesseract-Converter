use fx_schema::{ShapePath, ShapePathCommand};

use crate::{
    properties::{PropertyError, data, unique_list},
    rifx::Chunk,
};

mod animation;
pub(in crate::structure_document) use animation::entries;
#[cfg(test)]
pub(in crate::structure_document) use animation::is_dynamic;

const SHPH_BYTES: usize = 24;
const LHD3_BYTES: usize = 52;
const POINT_BYTES: usize = 8;

/// Decodes AE's static `om-s/omks/shap/shph/list/lhd3/ldat` outline.
#[cfg(test)]
pub(in crate::structure_document) fn decode(
    run: &[Chunk],
    scale: [f64; 2],
) -> Result<ShapePath, PropertyError> {
    decode_value(run, scale, true)
}

/// Decodes the first native path value and reports whether more values exist.
pub(in crate::structure_document) fn decode_first(
    run: &[Chunk],
    scale: [f64; 2],
) -> Result<(ShapePath, bool), PropertyError> {
    let value = unique_list(property_run(run)?, *b"om-s")?;
    let keys = unique_list(value, *b"omks")?;
    let count = keys
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"shap"))
        .count();
    Ok((decode_value(run, scale, false)?, count > 1))
}

fn property_run(run: &[Chunk]) -> Result<&[Chunk], PropertyError> {
    if run.iter().any(|chunk| chunk.list_kind() == Some(*b"om-s")) {
        return Ok(run);
    }
    crate::properties::runs(unique_list(run, *b"tdgp")?)?
        .into_iter()
        .find(|(name, _)| *name == "ADBE Vector Shape")
        .map(|(_, property)| property)
        .ok_or(PropertyError::Layout("missing vector path property"))
}

fn decode_value(
    run: &[Chunk],
    scale: [f64; 2],
    require_single: bool,
) -> Result<ShapePath, PropertyError> {
    let value = unique_list(property_run(run)?, *b"om-s")?;
    let keys = unique_list(value, *b"omks")?;
    let mut shapes = keys
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"shap"));
    let shape = shapes
        .next()
        .and_then(Chunk::children)
        .ok_or(PropertyError::Layout("missing property LIST"))?;
    if require_single && shapes.next().is_some() {
        return Err(PropertyError::Layout("duplicate property LIST"));
    }
    decode_shape(shape, scale, false)
}

fn decode_shape(
    shape: &[Chunk],
    scale: [f64; 2],
    preserve_cubic: bool,
) -> Result<ShapePath, PropertyError> {
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
    for point in points.chunks_exact(POINT_BYTES) {
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

    let vertex_count = count / 3;
    let closed = header[3] & 8 == 0;
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
    Ok(ShapePath { commands })
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
    use crate::{
        properties::{root_runs, runs, unique_list},
        structure::{ItemKind, read_project},
    };

    #[test]
    fn native_static_path_decodes_normalized_vertices_and_cubic_handles() {
        let project = read_project(include_bytes!(
            "../../../tests/fixtures/shapes/shape_basic.aep"
        ))
        .expect("pinned native shape fixture parses");
        let path_run = project
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(&comp.layers),
                _ => None,
            })
            .flatten()
            .find_map(|layer| {
                let roots = root_runs(&layer.content).ok()?;
                let parade = roots
                    .into_iter()
                    .find(|(name, _)| *name == "ADBE Mask Parade")?;
                let parade = unique_list(parade.1, *b"tdgp").ok()?;
                let atom = runs(parade)
                    .ok()?
                    .into_iter()
                    .find(|(name, _)| *name == "ADBE Mask Atom")?;
                let atom = unique_list(atom.1, *b"tdgp").ok()?;
                runs(atom)
                    .ok()?
                    .into_iter()
                    .find(|(name, _)| *name == "ADBE Mask Shape")
                    .map(|(_, run)| run)
            })
            .expect("fixture has an explicit native shape path");
        let path = decode(path_run, [400.0, 400.0]).expect("native shape layout decodes");
        assert!(path.is_finite());
        assert!(matches!(
            path.commands.first(),
            Some(ShapePathCommand::MoveTo { x, y, .. })
                if (*x - 200.0).abs() < 0.001 && (*y - 50.0).abs() < 0.001
        ));
        assert!(
            path.commands
                .iter()
                .any(|command| matches!(command, ShapePathCommand::CubicTo { .. }))
        );
        assert_eq!(path.commands.last(), Some(&ShapePathCommand::Close));
    }

    #[test]
    fn rejects_missing_shape_records() {
        let run = Vec::<Chunk>::new();
        assert!(decode(&run, [1.0, 1.0]).is_err());
    }
}
