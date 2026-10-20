use fx_schema::ShapePath;
#[cfg(test)]
use fx_schema::ShapePathCommand;

use crate::{
    properties::{PropertyError, read_native_shape_path, unique_list},
    rifx::Chunk,
};

mod animation;
mod expressions;
pub(in crate::structure_document) use animation::entries;
#[cfg(test)]
pub(in crate::structure_document) use animation::is_dynamic;
pub(in crate::structure_document) use expressions::entries as expression_entries;

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

pub(super) fn property_run(run: &[Chunk]) -> Result<&[Chunk], PropertyError> {
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
    read_native_shape_path(shape, scale, preserve_cubic)
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
