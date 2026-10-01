//! Conservative semantic decoding for the verified AE 2026 composition subset.

use thiserror::Error;

use crate::{
    aep::Project,
    rifx::{Chunk, RifxError},
    schema::{CompositionRecord, HeadRecord, ItemRecord, RecordError},
};

/// Errors while interpreting a valid AEP chunk tree.
#[derive(Debug, Error)]
pub enum AepReadError {
    /// Malformed RIFX framing or fixed-layout record.
    #[error(transparent)]
    Binary(#[from] RifxError),
    /// A typed fixed-layout record is malformed.
    #[error(transparent)]
    Record(#[from] RecordError),
    /// A required record is missing, ambiguous, or invalid.
    #[error("invalid After Effects project: {0}")]
    Invalid(&'static str),
    /// Content cannot currently be represented without dropping semantics.
    #[error("unsupported After Effects project: {0}")]
    Unsupported(&'static str),
}

/// The composition fields representable by the first conversion slice.
#[derive(Clone, Debug, PartialEq)]
pub struct EmptyComposition {
    /// Project-local item identifier.
    pub id: u32,
    /// Composition name.
    pub name: String,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Duration in seconds.
    pub duration_secs: f64,
}

fn only_raw(chunks: &[Chunk], name: [u8; 4]) -> Result<&[u8], AepReadError> {
    let mut matching = chunks.iter().filter(|chunk| chunk.id() == name);
    let chunk = matching
        .next()
        .ok_or(AepReadError::Invalid("required record is missing"))?;
    if matching.next().is_some() {
        return Err(AepReadError::Invalid("duplicate record"));
    }
    chunk
        .data_payload()
        .ok_or(AepReadError::Invalid("record is not raw"))
}

fn only_list(chunks: &[Chunk], name: [u8; 4]) -> Result<&[Chunk], AepReadError> {
    let mut matching = chunks
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(name));
    let chunk = matching
        .next()
        .ok_or(AepReadError::Invalid("required LIST is missing"))?;
    if matching.next().is_some() {
        return Err(AepReadError::Unsupported("multiple matching LISTs"));
    }
    chunk
        .children()
        .ok_or(AepReadError::Invalid("LIST is not a child list"))
}

/// Reads an AE 2026 project with exactly one 24-fps empty composition.
///
/// Rejects unsupported structures rather than returning an incomplete FX
/// document. This does not yet convert the result to a Tesseract document.
pub fn read_empty_composition(bytes: &[u8]) -> Result<EmptyComposition, AepReadError> {
    let project = Project::parse(bytes)?;
    let head = HeadRecord::decode(only_raw(&project.chunks, *b"head")?)?;
    if head.format_version() != 97 {
        return Err(AepReadError::Unsupported(
            "only AE 2026 format is supported",
        ));
    }
    let folder = only_list(&project.chunks, *b"Fold")?;
    if folder.iter().any(|child| {
        child
            .list_kind()
            .is_some_and(|kind| kind != *b"Item" && kind != *b"FEE ")
    }) {
        return Err(AepReadError::Unsupported(
            "nested folders or unknown folder content",
        ));
    }
    let item = only_list(folder, *b"Item")?;
    if item.iter().any(
        |child| matches!(child.list_kind(), Some(kind) if kind == *b"Layr" || kind == *b"Pin "),
    ) {
        return Err(AepReadError::Unsupported(
            "layers and proxies are not supported",
        ));
    }
    if item.iter().any(|child| {
        child.list_kind().is_some_and(|kind| {
            !matches!(
                &kind,
                b"dats"
                    | b"PRin"
                    | b"DLay"
                    | b"SLay"
                    | b"CLay"
                    | b"SecL"
                    | b"Ewst"
                    | b"CIFO"
                    | b"CIF2"
                    | b"CIF3"
                    | b"Gide"
                    | b"GdV2"
            )
        })
    }) {
        return Err(AepReadError::Unsupported("unknown composition content"));
    }
    let streams = only_list(item, *b"dats")?;
    if streams.len() != 1 || only_raw(streams, *b"numS")? != [0; 4] {
        return Err(AepReadError::Unsupported(
            "composition streams are not supported",
        ));
    }
    let idta = ItemRecord::decode(only_raw(item, *b"idta")?)?;
    if idta.item_type() != 4 || idta.id() == 0 || head.next_item_id() <= idta.id() {
        return Err(AepReadError::Unsupported("item is not a valid composition"));
    }
    let name = std::str::from_utf8(only_raw(item, *b"Utf8")?)
        .map_err(|_| AepReadError::Invalid("composition name is not UTF-8"))?;
    if name.is_empty() || name.contains('\0') {
        return Err(AepReadError::Invalid(
            "composition name is empty or contains NUL",
        ));
    }
    let comp = CompositionRecord::decode(only_raw(item, *b"cdta")?)?;
    if comp.frame_rate() != 24.0 {
        return Err(AepReadError::Unsupported("only 24 fps is supported"));
    }
    if comp.pixel_aspect_fraction() != (1, 1)
        || comp.display_start_fraction().0 != 0
        || comp.display_start_fraction().1 == 0
        || comp.work_area_bounds().0.0 != 0
        || comp.work_area_bounds().0.1 == 0
        || comp.work_area_bounds().1.0 != u32::MAX
        || comp.flags() != [0, 0]
        || comp.resolution_divisors() != (1, 1)
    {
        return Err(AepReadError::Unsupported(
            "non-default composition settings",
        ));
    }
    let (width, height) = comp.dimensions();
    if width == 0 || height == 0 {
        return Err(AepReadError::Invalid("zero composition dimension"));
    }
    let (numerator, denominator) = comp.duration_fraction()?;
    if numerator == 0 {
        return Err(AepReadError::Invalid("zero composition duration"));
    }
    Ok(EmptyComposition {
        id: idta.id(),
        name: name.to_owned(),
        width,
        height,
        duration_secs: f64::from(numerator) / f64::from(denominator),
    })
}

#[cfg(test)]
mod tests {
    use super::{AepReadError, read_empty_composition};
    use crate::{aep::Project, rifx::Chunk};

    fn list_mut(chunks: &mut [Chunk], kind: [u8; 4]) -> &mut Vec<Chunk> {
        chunks
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(kind))
            .unwrap()
            .children_mut()
            .unwrap()
    }

    #[test]
    fn reads_independent_ae2026_composition() {
        let comp =
            read_empty_composition(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
        assert_eq!((comp.name.as_str(), comp.id), ("classic-3d", 1));
        assert_eq!(
            (comp.width, comp.height, comp.duration_secs),
            (1920, 1080, 1.0)
        );
    }

    #[test]
    fn rejects_nested_folder_instead_of_ignoring_its_items() {
        let mut project =
            Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
        list_mut(&mut project.chunks, *b"Fold").push(Chunk::list(*b"Fold", vec![]));
        assert!(matches!(
            read_empty_composition(&project.encode().unwrap()),
            Err(AepReadError::Unsupported(
                "nested folders or unknown folder content"
            ))
        ));
    }

    #[test]
    fn rejects_nonempty_or_unrecognized_composition_streams() {
        for replacement in [
            vec![],
            vec![Chunk::data(*b"numS", 1_u32.to_be_bytes()).unwrap()],
            vec![Chunk::data(*b"numS", vec![0; 3]).unwrap()],
            vec![Chunk::data(*b"numS", vec![0; 4]).unwrap(); 2],
            vec![
                Chunk::data(*b"numS", 0_u32.to_be_bytes()).unwrap(),
                Chunk::data(*b"data", vec![1]).unwrap(),
            ],
        ] {
            let mut project =
                Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
            let folder = list_mut(&mut project.chunks, *b"Fold");
            let item = list_mut(folder, *b"Item");
            *list_mut(item, *b"dats") = replacement;
            assert!(read_empty_composition(&project.encode().unwrap()).is_err());
        }
    }

    #[test]
    fn rejects_missing_or_duplicate_stream_lists_and_composition_records() {
        for case in 0..3 {
            let mut project =
                Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
            let folder = list_mut(&mut project.chunks, *b"Fold");
            let item = list_mut(folder, *b"Item");
            match case {
                0 => item.retain(|chunk| chunk.list_kind() != Some(*b"dats")),
                1 => item.push(Chunk::list(
                    *b"dats",
                    vec![Chunk::data(*b"numS", vec![0; 4]).unwrap()],
                )),
                _ => {
                    let cdta = item
                        .iter()
                        .find(|chunk| chunk.id() == *b"cdta")
                        .unwrap()
                        .clone();
                    item.push(cdta);
                }
            }
            assert!(read_empty_composition(&project.encode().unwrap()).is_err());
        }
    }

    #[test]
    fn rejects_opaque_lists_in_semantic_composition_content() {
        let mut project =
            Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
        let folder = list_mut(&mut project.chunks, *b"Fold");
        list_mut(folder, *b"Item").push(Chunk::opaque_list(*b"btdk", vec![1, 2, 3]));
        assert!(matches!(
            read_empty_composition(&project.encode().unwrap()),
            Err(AepReadError::Unsupported("unknown composition content"))
        ));
    }

    #[test]
    fn rejects_other_version_and_extra_layers() {
        assert!(matches!(
            read_empty_composition(include_bytes!("../tests/fixtures/compositions.aep")),
            Err(AepReadError::Unsupported(
                "only AE 2026 format is supported"
            ))
        ));
        let mut project =
            Project::parse(include_bytes!("../tests/fixtures/ae26_one_comp.aep")).unwrap();
        let folder = list_mut(&mut project.chunks, *b"Fold");
        list_mut(folder, *b"Item").push(Chunk::list(*b"Layr", vec![]));
        assert!(matches!(
            read_empty_composition(&project.encode().unwrap()),
            Err(AepReadError::Unsupported(
                "layers and proxies are not supported"
            ))
        ));
    }
}
