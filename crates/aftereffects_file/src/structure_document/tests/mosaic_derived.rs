//! Public-derived Mosaic sources for converter regressions.
//!
//! The Mosaic plugin records come from the pinned, independently Adobe-authored
//! effects-coverage sources (`effects_coverage/imports.json`). Every edit made
//! here — an expression on a control or a renamed instance — is a labeled
//! converter-test derivation, not additional Adobe-authored or Adobe-rendered
//! evidence.

use crate::{rifx::Chunk, structure::Layer};

pub(super) const ANIMATED_CONTROLS: &[u8] =
    include_bytes!("../../../tests/fixtures/effects_coverage/native_animated_controls.aep");
pub(super) const STATIC_CONTROLS: &[u8] =
    include_bytes!("../../../tests/fixtures/effects_coverage/native_static_controls.aep");
/// Registered native target: Horizontal Blocks 12→24 and Vertical Blocks 8→16
/// at layer times 0s/1s, Linear, Sharp Colors on, Shape owner.
pub(super) const ANIMATED_MOSAIC: u32 = 326;
/// Registered native target: static Horizontal 12 / Vertical 8, Shape owner.
pub(super) const STATIC_MOSAIC: u32 = 339;
pub(super) const HORIZONTAL: &str = "ADBE Mosaic-0001";
pub(super) const VERTICAL: &str = "ADBE Mosaic-0002";
pub(super) const VERTICAL_ALIAS: &str = r#"effect("Mosaic")("Horizontal Blocks")"#;

fn match_name(chunk: &Chunk) -> Option<&str> {
    let bytes = chunk.data_payload()?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).ok()
}

fn display_name(value: &str) -> Chunk {
    let mut bytes = b"Utf8".to_vec();
    bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    bytes.extend(value.as_bytes());
    Chunk::data(*b"tdsn", bytes).expect("valid display name")
}

/// Children of the LIST of `kind` that follows the named run in a run table.
fn run_list_mut<'a>(children: &'a mut [Chunk], name: &str, kind: [u8; 4]) -> &'a mut Vec<Chunk> {
    let start = children
        .iter()
        .position(|chunk| chunk.id() == *b"tdmn" && match_name(chunk) == Some(name))
        .unwrap_or_else(|| panic!("native run {name} is present"));
    let end = children[start + 1..]
        .iter()
        .position(|chunk| chunk.id() == *b"tdmn")
        .map_or(children.len(), |offset| start + 1 + offset);
    let index = (start + 1..end)
        .find(|index| children[*index].list_kind() == Some(kind))
        .unwrap_or_else(|| panic!("native run {name} has its LIST"));
    children[index].children_mut().expect("LIST children")
}

fn property_root_mut(layer: &mut Layer) -> &mut Vec<Chunk> {
    layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .expect("native layer property root")
}

/// The explicit controls of the first native Mosaic occurrence.
pub(super) fn mosaic_controls_mut(layer: &mut Layer) -> &mut Vec<Chunk> {
    let root = property_root_mut(layer);
    let parade = run_list_mut(root, "ADBE Effect Parade", *b"tdgp");
    let plugin = run_list_mut(parade, "ADBE Mosaic", *b"sspc");
    plugin
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .expect("native Mosaic controls")
}

fn set_tdb4(leaf: &mut [Chunk], edit: impl FnOnce(&mut [u8])) {
    let meta = leaf
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdb4")
        .expect("native numeric metadata");
    let mut bytes = meta.data_payload().expect("tdb4 payload").to_vec();
    edit(&mut bytes);
    *meta = Chunk::data(*b"tdb4", bytes).expect("valid tdb4");
}

/// Derived: store AE expression text on one Mosaic control.
pub(super) fn set_expression(layer: &mut Layer, parameter: &str, text: &str, enabled: bool) {
    let leaf = run_list_mut(mosaic_controls_mut(layer), parameter, *b"tdbs");
    leaf.retain(|chunk| chunk.id() != *b"Utf8");
    leaf.push(Chunk::data(*b"Utf8", text.as_bytes().to_vec()).expect("expression text"));
    // Native tdb4 byte 119 bit 0 is AE's "expression disabled" switch.
    set_tdb4(leaf, |meta| {
        if enabled {
            meta[119] &= !1;
        } else {
            meta[119] |= 1;
        }
    });
}

/// Derived: replace AE's placeholder instance display name with an explicit rename.
pub(super) fn rename_mosaic(layer: &mut Layer, name: &str) {
    let controls = mosaic_controls_mut(layer);
    let tdsn = controls
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdsn")
        .expect("native instance display name");
    *tdsn = display_name(name);
}
