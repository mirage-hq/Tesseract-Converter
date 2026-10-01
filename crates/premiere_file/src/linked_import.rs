//! Typed linked-composition boundary for a caller-supplied importer.
//!
//! Ordinary import resolves each linked composition itself
//! ([`crate::linked_compositions`]). `Premiere::import_with_linked_compositions`
//! takes the editable content of each placement from the caller instead; that
//! content is placed by the same clip rules (clocks, canvas clip, blend, masks,
//! stages and diagnostics). The caller owns the lifetime and freshness of the
//! asset files that it returns.
use crate::PrAfterEffectsComposition;
use fx_schema::{AssetId, EditableFxCompositionDocument};
use std::path::{Path, PathBuf};
use tesseract_file::AssetKind;

/// One freshly converted occurrence, with IDs allocated from the requested start.
pub struct LinkedComposition {
    pub document: EditableFxCompositionDocument,
    /// First unused numeric ID across layer, effect and FX-item namespaces.
    pub next_id: u64,
    pub assets: Vec<(AssetId, PathBuf, AssetKind)>,
}

/// Resolve a file-qualified composition for one placement, with IDs from the
/// given start. Keep any temporary asset files alive until the import call
/// returns; their bytes are packaged as returned.
pub type LinkedCompositionResolver<'a> =
    dyn FnMut(&Path, PrAfterEffectsComposition, u64) -> anyhow::Result<LinkedComposition> + 'a;
