//! Adobe-authored structural defaults for editable Point and Box Source Text.
//!
//! The public AE26 fixture is independent of the input FX document. Only its
//! default property structure is reused: authored text, font, style, geometry,
//! document keys, and property identity are replaced from typed input.

use fx_schema::Justification;
use sha2::{Digest, Sha256};

use crate::rifx::{Chunk, Rifx};

use super::{AepWriteError, keyframes::PropertyClock, text_document::TextDocumentTimeline};

mod profile;
#[cfg(test)]
mod tests;

const BOX_FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/pr4442_native/sources/text_document_box_v3.aep");
const POINT_FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/point_text_envelope/native_empty_point.aep");
const INVALID: AepWriteError = AepWriteError::Invalid("native Text reference layout changed");

pub(super) fn boxed_properties(
    timeline: &TextDocumentTimeline,
    id: u32,
    clock: PropertyClock,
) -> Result<Chunk, AepWriteError> {
    properties(timeline, id, clock, BOX_FIXTURE, profile::render(timeline)?)
}

pub(super) fn point_properties(
    timeline: &TextDocumentTimeline,
    id: u32,
    clock: PropertyClock,
) -> Result<Chunk, AepWriteError> {
    properties(
        timeline,
        id,
        clock,
        POINT_FIXTURE,
        profile::render_point(timeline)?,
    )
}

fn properties(
    timeline: &TextDocumentTimeline,
    id: u32,
    clock: PropertyClock,
    fixture: &[u8],
    payload: Vec<u8>,
) -> Result<Chunk, AepWriteError> {
    timeline.validate_with_clock(clock)?;
    let native = Rifx::parse_with(fixture, |kind| kind == *b"btdk")?;
    let group = native
        .chunks()
        .iter()
        .find_map(find_box_group)
        .ok_or(INVALID)?;
    let mut group = group.clone();
    let children = group.children_mut().ok_or(INVALID)?;
    let document = children
        .windows(2)
        .position(|pair| {
            pair[0].id() == *b"tdmn"
                && pair[0]
                    .data_payload()
                    .is_some_and(|data| data.starts_with(b"ADBE Text Document"))
                && pair[1].list_kind() == Some(*b"btds")
        })
        .map(|index| index + 1)
        .ok_or(INVALID)?;
    let guid = children
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"btgu"))
        .and_then(Chunk::children_mut)
        .and_then(|children| children.iter_mut().find(|chunk| chunk.id() == *b"pgui"))
        .ok_or(INVALID)?;
    if guid.data_payload().is_none_or(|data| data.len() != 16) {
        return Err(INVALID);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"FX-native-Source-Text-owner-v1");
    hasher.update(id.to_be_bytes());
    let digest = hasher.finalize();
    *guid = Chunk::data(*b"pgui", digest[..16].to_vec())?;
    let source = children[document].children_mut().ok_or(INVALID)?;
    let native_metadata = source
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .and_then(Chunk::children_mut)
        .ok_or(INVALID)?;
    let descriptor = native_metadata
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdb4")
        .ok_or(INVALID)?;
    let mut bytes: [u8; 124] = descriptor
        .data_payload()
        .ok_or(INVALID)?
        .try_into()
        .map_err(|_| INVALID)?;
    bytes[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
    if timeline.keyed {
        bytes = super::keyframes::animated_descriptor(bytes, false);
    }
    *descriptor = Chunk::data(*b"tdb4", bytes.to_vec())?;
    native_metadata.retain(|chunk| chunk.id() != *b"cdat" && chunk.list_kind() != Some(*b"list"));
    let generated = super::text_document::source_metadata(timeline, clock)?;
    let event = generated
        .children()
        .and_then(|children| children.last())
        .ok_or(INVALID)?;
    native_metadata.push(event.clone());

    let blob = source
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"btdk"))
        .ok_or(INVALID)?;
    *blob = Chunk::opaque_list(*b"btdk", payload);
    Ok(group)
}

fn property<'a>(group: &'a Chunk, name: &[u8], kind: [u8; 4]) -> Option<&'a Chunk> {
    group.children()?.windows(2).find_map(|pair| {
        (pair[0].id() == *b"tdmn"
            && pair[0].data_payload()?.starts_with(name)
            && pair[1].list_kind() == Some(kind))
        .then_some(&pair[1])
    })
}

fn find_box_group(chunk: &Chunk) -> Option<&Chunk> {
    if let Some(group) = property(chunk, b"ADBE Text Properties", *b"tdgp") {
        return Some(group);
    }
    chunk.children()?.iter().find_map(find_box_group)
}

#[cfg(test)]
fn source_in_group(group: &Chunk) -> Option<&Chunk> {
    property(group, b"ADBE Text Document", *b"btds")
}
