//! Saved graphic pictures select native item IDs, never Dynamic Link GUIDs.

use super::{AepConversionError, media, native_media_references};
use crate::{
    ImportDiagnostic,
    graphic_template::SavedGraphicText,
    structure::StructuralProject,
    structure_document::{AssetNamespace, Destination, to_graphic_picture},
};
use fx_schema::{AssetId, EditableFxCompositionDocument};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile};

/// Full editable picture and the files that back its archive assets.
/// Keep this owner alive until its assets have been written and verified.
pub struct GraphicPicture {
    document: Option<EditableFxCompositionDocument>,
    /// First unused host identity after this picture and its animations/effects.
    pub next_id: u64,
    /// Archive assets in the caller's unique namespace.
    pub assets: Vec<(AssetId, PathBuf, AssetKind)>,
    /// Ordinary AEP approximation/omission diagnostics.
    pub diagnostics: Vec<ImportDiagnostic>,
    sources: BTreeMap<PathBuf, String>,
    missing: BTreeSet<PathBuf>,
    packaged_hashes: BTreeMap<AssetId, String>,
    _normalized: Vec<tempfile::NamedTempFile>,
}

impl std::fmt::Debug for GraphicPicture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GraphicPicture")
            .field("next_id", &self.next_id)
            .field("assets", &self.assets)
            .finish_non_exhaustive()
    }
}

impl GraphicPicture {
    /// Move the document once while retaining ownership of normalized media.
    pub fn take_document(&mut self) -> Result<EditableFxCompositionDocument, AepConversionError> {
        self.document.take().ok_or(AepConversionError::Input(
            "graphic picture already consumed",
        ))
    }

    /// Recheck every selected source and higher-priority missing candidate.
    pub fn verify_sources(&self) -> Result<(), AepConversionError> {
        for path in &self.missing {
            if !std::fs::metadata(path).is_err_and(|error| media::is_missing(&error)) {
                return Err(AepConversionError::MediaChanged(path.clone()));
            }
        }
        for (path, expected) in &self.sources {
            if media::sha256_file(path)? != *expected {
                return Err(AepConversionError::MediaChanged(path.clone()));
            }
        }
        Ok(())
    }

    /// Verify exact packaged bytes, including normalized PSD/image output.
    pub fn verify_packaged(&self, archive: &TesseractFile) -> Result<(), AepConversionError> {
        for (id, expected) in &self.packaged_hashes {
            if archive.asset(id.as_str())?.descriptor().sha256 != *expected {
                return Err(AepConversionError::Input(
                    "packaged graphic picture asset changed",
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn import(
    project: &StructuralProject,
    media_context: &Path,
    composition: u32,
    first_id: u64,
    namespace: &str,
    text: &[SavedGraphicText],
    stage_collected_media: &mut dyn FnMut(&Path) -> std::io::Result<()>,
) -> Result<GraphicPicture, AepConversionError> {
    if first_id == 0 || AssetId::new(namespace).is_err() {
        return Err(AepConversionError::Input(
            "invalid graphic picture identity reservation",
        ));
    }
    let mut preflight = media::MediaPreflight::with_project(media_context, None, project);
    for reference in native_media_references(project, Some(composition))? {
        preflight.require_with_collected_staging(&reference.request, stage_collected_media)?;
    }
    let converted = to_graphic_picture(
        project,
        composition,
        &mut |request| {
            preflight.resolve_media_with_collected_staging(request, stage_collected_media)
        },
        Destination::LinkedPicture {
            parent: None,
            first_id,
            asset_namespace: AssetNamespace::new(namespace),
        },
        text,
    )?;
    if let Some(error) = preflight.failure.take() {
        return Err(error);
    }
    let mut sources = BTreeMap::new();
    let mut missing = BTreeSet::new();
    let mut assets = Vec::new();
    let mut packaged_hashes = BTreeMap::new();
    let mut kept = BTreeSet::new();
    let mut pin = |source: &media::SourceFile| -> Result<(), AepConversionError> {
        let actual = media::sha256_file(&source.path)?;
        if source
            .decoded_sha256
            .as_ref()
            .is_some_and(|expected| expected != &actual)
        {
            return Err(AepConversionError::MediaChanged(source.path.clone()));
        }
        sources.insert(source.path.clone(), actual);
        missing.extend(source.missing_paths.iter().cloned());
        Ok(())
    };
    for asset in preflight.used_assets(&converted.assets)? {
        pin(asset.source)?;
        kept.insert(asset.path.to_owned());
        packaged_hashes.insert(
            asset.request.logical_id.clone(),
            media::sha256_file(asset.path)?,
        );
        assets.push((
            asset.request.logical_id.clone(),
            asset.path.to_owned(),
            asset.kind,
        ));
    }
    for source in preflight.vector_sources() {
        pin(source)?;
    }
    let mut diagnostics = converted.diagnostics;
    diagnostics.append(&mut preflight.diagnostics);
    let normalized = preflight
        .into_normalized()
        .into_iter()
        .filter(|file| kept.contains(file.path()))
        .collect();
    let result = GraphicPicture {
        document: Some(converted.document),
        next_id: converted.next_id,
        assets,
        diagnostics,
        sources,
        missing,
        packaged_hashes,
        _normalized: normalized,
    };
    result.verify_sources()?;
    Ok(result)
}

#[cfg(test)]
mod tests;
