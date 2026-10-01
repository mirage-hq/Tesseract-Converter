//! Owned preparation for callers coordinating more than one native exporter.

use std::{collections::BTreeSet, path::Path};

use fx_conv::{Artifact, ConversionReport, Progress};
use fx_schema::{EditableFxCompositionDocument, LayerId};
use sha2::{Digest, Sha256};
use tesseract_file::TesseractFile;

use super::{package, script_bake};
use crate::{
    AepConversionError, AfterEffects, AfterEffectsExportOptions, ExportDiagnostic,
    GeneratedRootComposition,
    export_document::{
        ExportDocumentViews, to_aep_with_document_views_and_media_and_fps_with_progress,
        to_picture_only_aep_with_document_views_and_media_and_fps_with_progress,
    },
    writer::footage::RelativeMediaPath,
};

/// An unpublished AEP and its external media, owned until this value is dropped.
///
/// Copy/link only the reported artifacts into a fresh package while preserving
/// their relative layout. Dropping this handle removes its private staging tree,
/// including unreferenced prepared media. It never removes a caller's output.
/// Staging success is not a fidelity or Adobe-acceptance guarantee.
#[derive(Debug)]
pub struct StagedAfterEffectsExport {
    directory: tempfile::TempDir,
    pub(super) publication_files: Vec<RelativeMediaPath>,
    pub(super) report: ConversionReport<ExportDiagnostic>,
    root: GeneratedRootComposition,
    omitted_layer_ids: BTreeSet<LayerId>,
    generated_project_sha256: [u8; 32],
}

impl StagedAfterEffectsExport {
    /// Private staging root; no path under it is embedded as a media reference.
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// Root identity and rounded native timing from this staged AEP's writer plan.
    pub fn root_composition(&self) -> &GeneratedRootComposition {
        &self.root
    }

    /// Digest captured from writer output before filesystem publication.
    /// Compare against captured project artifacts to detect subsequent changes;
    /// this does not validate native controls or external media.
    pub fn generated_project_sha256(&self) -> &[u8; 32] {
        &self.generated_project_sha256
    }

    /// Original selected FX layer owners dropped during lowering or dependency pruning.
    /// Effect-only approximations and consumed guides are not omitted owners.
    pub fn omitted_layer_ids(&self) -> &BTreeSet<LayerId> {
        &self.omitted_layer_ids
    }

    /// Relative files to publish and all best-effort conversion diagnostics.
    pub fn report(&self) -> &ConversionReport<ExportDiagnostic> {
        &self.report
    }
}

/// An unpublished AEP whose generated native layer audio switches are all off.
///
/// The writer proves this property for every retained root and nested layer
/// record. Audio owners, editable gain controls, and referenced media remain in
/// the package so picture staging cannot invalidate native dependencies. This is
/// a writer-structure assertion, not independent Adobe audibility proof.
#[derive(Debug)]
pub struct StagedAfterEffectsPictureExport {
    staged: StagedAfterEffectsExport,
}

impl StagedAfterEffectsPictureExport {
    /// Private staging root; no path under it is embedded as a media reference.
    pub fn directory(&self) -> &Path {
        self.staged.directory()
    }

    /// Root identity and rounded native timing from this staged AEP's writer plan.
    pub fn root_composition(&self) -> &GeneratedRootComposition {
        self.staged.root_composition()
    }

    /// Generation-time project digest, not a fresh hash of the mutable stage file.
    /// It does not certify native controls or external media.
    pub fn generated_project_sha256(&self) -> &[u8; 32] {
        self.staged.generated_project_sha256()
    }

    /// Original selected FX layer owners dropped during lowering or dependency pruning.
    pub fn omitted_layer_ids(&self) -> &BTreeSet<LayerId> {
        self.staged.omitted_layer_ids()
    }

    /// Relative files to publish and ordinary best-effort conversion diagnostics.
    pub fn report(&self) -> &ConversionReport<ExportDiagnostic> {
        self.staged.report()
    }
}

impl AfterEffects {
    /// Stage a contiguous, dependency-closed range of original root layers.
    /// The coordinator owns selection; this borrows the selected source instead
    /// of serializing a second document. Native audio switches remain disabled.
    pub fn stage_picture_layers(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        roots: std::ops::Range<usize>,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
    ) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
        self.stage_picture_layers_with_progress(
            archive,
            document,
            roots,
            staging_parent,
            options,
            Progress::default(),
        )
    }

    /// Stage selected picture roots with phase-local progress observations.
    pub fn stage_picture_layers_with_progress(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        roots: std::ops::Range<usize>,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
        progress: Progress<'_>,
    ) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
        let directory = tempfile::Builder::new()
            .prefix(".conversion-aftereffects-picture-")
            .tempdir_in(staging_parent)
            .map_err(|source| {
                AepConversionError::io("create picture-only AEP staging", staging_parent, source)
            })?;
        let staged = prepare_with_writer(
            archive,
            document,
            directory,
            options,
            to_picture_only_aep_with_document_views_and_media_and_fps_with_progress,
            Some(roots),
            progress,
        )?;
        Ok(StagedAfterEffectsPictureExport { staged })
    }

    /// Stage an explicitly selected editable document using an archive's assets.
    ///
    /// `staging_parent` must exist. A fresh, exclusively owned temporary child is
    /// created there; nothing is published and no Adobe application is launched.
    /// `document` may differ from the archive's original document, allowing a
    /// coordinator to extract a scope without copying asset bytes into another
    /// archive. The coordinator must validate that extraction preserves clocks,
    /// compositing and cross-layer dependencies; this API does not partition FX.
    /// Missing/malformed assets fail as in ordinary export, while unsupported
    /// semantics retain the ordinary export diagnostics.
    pub fn stage_document(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
    ) -> Result<StagedAfterEffectsExport, AepConversionError> {
        self.stage_document_with_progress(
            archive,
            document,
            staging_parent,
            options,
            Progress::default(),
        )
    }

    /// Stage a document with phase-local progress observations.
    pub fn stage_document_with_progress(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
        progress: Progress<'_>,
    ) -> Result<StagedAfterEffectsExport, AepConversionError> {
        let directory = tempfile::Builder::new()
            .prefix(".conversion-aftereffects-")
            .tempdir_in(staging_parent)
            .map_err(|source| {
                AepConversionError::io("create AEP export staging", staging_parent, source)
            })?;
        prepare_with_progress(archive, document, directory, options, progress)
    }

    /// Stage the ordinary prepared/lowered picture with every native audio switch off.
    ///
    /// Preparation, media resolution, rollback, pruning, and diagnostics are the
    /// same single pass as [`Self::stage_document`]. The generated audio owners
    /// are retained; only final writer switches are disabled.
    pub fn stage_picture_only_document(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
    ) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
        self.stage_picture_only_document_with_progress(
            archive,
            document,
            staging_parent,
            options,
            Progress::default(),
        )
    }

    /// Stage a picture-only document with phase-local progress observations.
    pub fn stage_picture_only_document_with_progress(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
        progress: Progress<'_>,
    ) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
        let directory = tempfile::Builder::new()
            .prefix(".conversion-aftereffects-picture-")
            .tempdir_in(staging_parent)
            .map_err(|source| {
                AepConversionError::io("create picture-only AEP staging", staging_parent, source)
            })?;
        prepare_picture_only_with_progress(archive, document, directory, options, progress)
    }
}

pub(super) fn prepare_with_progress(
    archive: &TesseractFile,
    document: &EditableFxCompositionDocument,
    directory: tempfile::TempDir,
    options: &AfterEffectsExportOptions,
    progress: Progress<'_>,
) -> Result<StagedAfterEffectsExport, AepConversionError> {
    prepare_with_writer(
        archive,
        document,
        directory,
        options,
        to_aep_with_document_views_and_media_and_fps_with_progress,
        None,
        progress,
    )
}

fn prepare_picture_only_with_progress(
    archive: &TesseractFile,
    document: &EditableFxCompositionDocument,
    directory: tempfile::TempDir,
    options: &AfterEffectsExportOptions,
    progress: Progress<'_>,
) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
    let staged = prepare_with_writer(
        archive,
        document,
        directory,
        options,
        to_picture_only_aep_with_document_views_and_media_and_fps_with_progress,
        None,
        progress,
    )?;
    Ok(StagedAfterEffectsPictureExport { staged })
}

type DocumentWriter =
    fn(
        ExportDocumentViews<'_>,
        &std::collections::BTreeMap<String, crate::export_document::media::ResolvedMediaSource>,
        f64,
        Progress<'_>,
    ) -> Result<crate::export_document::ExportedDocument, crate::writer::AepWriteError>;

fn prepare_with_writer(
    archive: &TesseractFile,
    document: &EditableFxCompositionDocument,
    directory: tempfile::TempDir,
    options: &AfterEffectsExportOptions,
    write: DocumentWriter,
    roots: Option<std::ops::Range<usize>>,
    progress: Progress<'_>,
) -> Result<StagedAfterEffectsExport, AepConversionError> {
    let baked = match &roots {
        Some(range) => {
            let layers = document.composition().layers().get(range.clone()).ok_or(
                crate::writer::AepWriteError::Invalid("invalid selected root range"),
            )?;
            script_bake::prepare_layers_with_progress(document, layers, true, progress)?
        }
        None => script_bake::prepare_with_progress(document, progress)?,
    };
    let mut documents = ExportDocumentViews::from_preparation(document, baked.document.as_ref());
    if let Some(roots) = roots {
        documents = documents.select_roots(roots)?;
    }
    let prepared_document = documents.prepared();
    let requests = documents.media_requests()?;
    progress.stage("prepare AEP media");
    let prepared = package::prepare_media(
        archive,
        &requests,
        directory.path(),
        prepared_document.dimensions(),
    )?;
    let converted = write(documents, &prepared.sources, options.fps, progress);
    let mut diagnostics = baked.diagnostics;
    for (asset_id, reason) in prepared.unsupported {
        diagnostics.push(ExportDiagnostic {
            layer_id: None,
            message: format!(
                "Asset {asset_id}: {reason}; affected media layers omitted, convertible siblings retained."
            ),
        });
    }
    let mut converted = match converted {
        Ok(converted) => converted,
        Err(crate::writer::AepWriteError::NoConvertiblePicture(mut omissions)) => {
            omissions.extend(diagnostics);
            return Err(crate::writer::AepWriteError::NoConvertiblePicture(omissions).into());
        }
        Err(error) => return Err(error.into()),
    };
    converted.diagnostics.extend(diagnostics);
    let publication_files = prepared
        .files
        .into_iter()
        .filter(|path| converted.emitted_media_paths.contains(path))
        .collect::<Vec<_>>();
    let generated_project_sha256 = Sha256::digest(&converted.bytes).into();
    progress.stage("write staged AEP");
    let source = directory.path().join("project.aep");
    std::fs::write(&source, &converted.bytes)
        .map_err(|error| AepConversionError::io("write staged AEP", &source, error))?;
    let artifacts = std::iter::once(Artifact::project("project.aep"))
        .chain(
            publication_files
                .iter()
                .map(|path| Artifact::media(path.as_str())),
        )
        .collect();
    Ok(StagedAfterEffectsExport {
        directory,
        publication_files,
        root: converted.root,
        omitted_layer_ids: converted.omitted_layer_ids,
        generated_project_sha256,
        report: ConversionReport {
            diagnostics: converted.diagnostics,
            artifacts,
        },
    })
}

#[cfg(test)]
mod tests;
