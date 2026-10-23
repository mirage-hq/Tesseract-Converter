//! Package the `.cube` assets of imported Apply Color LUT instances and
//! Lumetri Color custom LUTs.
//!
//! The converter names each embedded LUT by its content
//! (`effects::apply_color_lut::EmbeddedLut::asset_id`); this adapter decodes
//! the same sequence data (Apply Color LUT) or embedded copy (Lumetri,
//! `effects::color_grades::lumetri_luts`) again from the project and writes
//! one temporary `.cube` per referenced asset, kept alive until the archive
//! is written.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

use fx_schema::{AssetId, FXComposition};
use tesseract_file::{AssetKind, TesseractFileBuilder, TesseractFileError};

use super::AepConversionError;
use crate::{
    effects::{
        apply_color_lut::{self, EmbeddedLut},
        color_grades::{self, lumetri_luts},
        native::read_effects,
    },
    properties,
    structure::{ItemKind, StructuralProject},
};

/// Temporary `.cube` files backing the LUT assets of one import.
#[derive(Debug, Default)]
pub(crate) struct ImportedLuts {
    files: BTreeMap<AssetId, PathBuf>,
    _directory: Option<tempfile::TempDir>,
}

impl ImportedLuts {
    /// Write the LUT of every Apply Color LUT instance and Lumetri custom LUT
    /// in `project` that `composition` references.
    pub(crate) fn prepare(
        project: &StructuralProject,
        composition: &FXComposition,
    ) -> Result<Self, AepConversionError> {
        let referenced: BTreeSet<&str> = composition
            .asset_refs()
            .into_iter()
            .map(|reference| reference.asset_id)
            .filter(|id| id.starts_with("aep-lut-"))
            .collect();
        if referenced.is_empty() {
            return Ok(Self::default());
        }
        let mut found = BTreeMap::new();
        for embedded in project_luts(project) {
            let id = embedded.asset_id();
            if referenced.contains(id.as_str()) {
                found.entry(id).or_insert(embedded);
            }
        }
        if referenced
            .iter()
            .any(|id| !found.keys().any(|found: &AssetId| found.as_str() == *id))
        {
            return Err(AepConversionError::Input(
                "an imported Apply Color LUT or Lumetri LUT asset has no embedded LUT in the project",
            ));
        }
        let directory = tempfile::Builder::new()
            .prefix(".conversion-aep-luts-")
            .tempdir()
            .map_err(|source| {
                AepConversionError::io("create LUT staging", &std::env::temp_dir(), source)
            })?;
        let mut files = BTreeMap::new();
        for (id, embedded) in found {
            let folder = directory.path().join(id.as_str());
            fs::create_dir(&folder)
                .map_err(|source| AepConversionError::io("create LUT staging", &folder, source))?;
            let path = folder.join(embedded.package_file_name());
            fs::write(&path, embedded.cube_text())
                .map_err(|source| AepConversionError::io("write LUT asset", &path, source))?;
            files.insert(id, path);
        }
        Ok(Self {
            files,
            _directory: Some(directory),
        })
    }

    /// Each LUT asset as `(id, file, kind)`; the files live as long as `self`.
    pub(crate) fn assets(&self) -> impl Iterator<Item = (&AssetId, &PathBuf, AssetKind)> {
        self.files
            .iter()
            .map(|(id, path)| (id, path, AssetKind::Other))
    }

    /// Add every LUT asset not already present in `added`.
    pub(crate) fn add_to(
        &self,
        mut builder: TesseractFileBuilder,
        added: &mut BTreeSet<AssetId>,
    ) -> Result<TesseractFileBuilder, TesseractFileError> {
        for (id, path, kind) in self.assets() {
            if added.insert(id.clone()) {
                builder = builder.add_asset(id.as_str(), path, kind)?;
            }
        }
        Ok(builder)
    }
}

/// Every decodable Apply Color LUT and Lumetri custom LUT in every
/// composition of `project`.
fn project_luts(project: &StructuralProject) -> Vec<EmbeddedLut> {
    let mut luts = Vec::new();
    for item in &project.items {
        let ItemKind::Composition(composition) = &item.kind else {
            continue;
        };
        for layer in &composition.layers {
            let Ok(roots) = properties::root_runs(&layer.content) else {
                continue;
            };
            let Some((_, parade)) = roots.iter().find(|(name, _)| *name == "ADBE Effect Parade")
            else {
                continue;
            };
            let Ok(instances) =
                properties::unique_list(parade, *b"tdgp").and_then(properties::runs)
            else {
                continue;
            };
            if instances
                .iter()
                .any(|(name, _)| *name == color_grades::LUMETRI)
            {
                let (effects, _) = read_effects(&layer.content, [1.0, 1.0]);
                for effect in effects
                    .iter()
                    .filter(|effect| effect.match_name == color_grades::LUMETRI)
                {
                    luts.extend(
                        lumetri_luts::browsed(effect)
                            .into_iter()
                            .flatten()
                            .filter_map(Result::ok)
                            .map(|browsed| browsed.lut),
                    );
                }
            }
            for (index, (name, _)) in instances.iter().enumerate() {
                if *name != apply_color_lut::MATCH_NAME {
                    continue;
                }
                if let Ok(data) = apply_color_lut::instance_sequence_data(&layer.content, index + 1)
                    && let Ok(embedded) = apply_color_lut::decode_sequence_data(data)
                {
                    luts.push(embedded);
                }
            }
        }
    }
    luts
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod lumetri_tests;
