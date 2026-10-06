//! Owned preparation for callers coordinating more than one native exporter.

use std::{
    collections::BTreeSet,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

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

/// Borrowed observations and cancellation for one AEP preparation call.
///
/// Set `cancelled` to true to stop at preparation boundaries and inside media
/// transcoding. Keep it set until the call returns. This does not interrupt
/// synchronous script preparation, image decoding, or native document writing.
#[derive(Clone, Copy, Default)]
pub struct AepPreparationControl<'a> {
    /// Existing phase-local observer; callbacks must return promptly.
    pub progress: Progress<'a>,
    /// Caller-owned token, using the media transcoder's relaxed atomic semantics.
    pub cancelled: Option<&'a AtomicBool>,
    /// Withhold video destination preparation in selected picture scopes.
    /// Unsupported originals are diagnosed; no remux or lossy encoding is run.
    pub preserve_video_assets: bool,
}

impl std::fmt::Debug for AepPreparationControl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AepPreparationControl")
            .field("cancelled", &self.cancelled)
            .finish_non_exhaustive()
    }
}

impl AepPreparationControl<'_> {
    pub(super) fn check_cancelled(self) -> Result<(), AepConversionError> {
        if self
            .cancelled
            .is_some_and(|token| token.load(Ordering::Relaxed))
        {
            return Err(AepConversionError::Cancelled);
        }
        Ok(())
    }
}

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
    pub(super) font_files: Vec<String>,
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
        self.stage_picture_layers_with_control(
            archive,
            document,
            roots,
            staging_parent,
            options,
            AepPreparationControl {
                progress,
                cancelled: None,
                preserve_video_assets: false,
            },
        )
    }

    /// Stage with caller-controlled cooperative cancellation and progress.
    pub fn stage_picture_layers_with_control(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        roots: std::ops::Range<usize>,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
        control: AepPreparationControl<'_>,
    ) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
        control.check_cancelled()?;
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
            NativeWriteTarget {
                write: to_picture_only_aep_with_document_views_and_media_and_fps_with_progress,
                roots: Some(roots),
                alias_directory: None,
            },
            control,
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
        self.stage_document_with_control(
            archive,
            document,
            staging_parent,
            options,
            AepPreparationControl {
                progress,
                cancelled: None,
                preserve_video_assets: false,
            },
        )
    }

    /// Stage with caller-controlled cooperative cancellation and progress.
    pub fn stage_document_with_control(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
        control: AepPreparationControl<'_>,
    ) -> Result<StagedAfterEffectsExport, AepConversionError> {
        control.check_cancelled()?;
        let directory = tempfile::Builder::new()
            .prefix(".conversion-aftereffects-")
            .tempdir_in(staging_parent)
            .map_err(|source| {
                AepConversionError::io("create AEP export staging", staging_parent, source)
            })?;
        prepare_with_control(archive, document, directory, options, None, control)
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
        self.stage_picture_only_document_with_control(
            archive,
            document,
            staging_parent,
            options,
            AepPreparationControl {
                progress,
                cancelled: None,
                preserve_video_assets: false,
            },
        )
    }

    /// Stage with caller-controlled cooperative cancellation and progress.
    pub fn stage_picture_only_document_with_control(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        staging_parent: &Path,
        options: &AfterEffectsExportOptions,
        control: AepPreparationControl<'_>,
    ) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
        control.check_cancelled()?;
        let directory = tempfile::Builder::new()
            .prefix(".conversion-aftereffects-picture-")
            .tempdir_in(staging_parent)
            .map_err(|source| {
                AepConversionError::io("create picture-only AEP staging", staging_parent, source)
            })?;
        prepare_picture_only_with_control(archive, document, directory, options, control)
    }
}

pub(super) fn prepare_with_progress(
    archive: &TesseractFile,
    document: &EditableFxCompositionDocument,
    directory: tempfile::TempDir,
    options: &AfterEffectsExportOptions,
    alias_directory: Option<&Path>,
    progress: Progress<'_>,
) -> Result<StagedAfterEffectsExport, AepConversionError> {
    prepare_with_control(
        archive,
        document,
        directory,
        options,
        alias_directory,
        AepPreparationControl {
            progress,
            cancelled: None,
            preserve_video_assets: false,
        },
    )
}

pub(super) fn prepare_with_control(
    archive: &TesseractFile,
    document: &EditableFxCompositionDocument,
    directory: tempfile::TempDir,
    options: &AfterEffectsExportOptions,
    alias_directory: Option<&Path>,
    control: AepPreparationControl<'_>,
) -> Result<StagedAfterEffectsExport, AepConversionError> {
    prepare_with_writer(
        archive,
        document,
        directory,
        options,
        NativeWriteTarget {
            write: to_aep_with_document_views_and_media_and_fps_with_progress,
            roots: None,
            alias_directory,
        },
        control,
    )
}

fn prepare_picture_only_with_control(
    archive: &TesseractFile,
    document: &EditableFxCompositionDocument,
    directory: tempfile::TempDir,
    options: &AfterEffectsExportOptions,
    control: AepPreparationControl<'_>,
) -> Result<StagedAfterEffectsPictureExport, AepConversionError> {
    let staged = prepare_with_writer(
        archive,
        document,
        directory,
        options,
        NativeWriteTarget {
            write: to_picture_only_aep_with_document_views_and_media_and_fps_with_progress,
            roots: None,
            alias_directory: None,
        },
        control,
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

struct NativeWriteTarget<'a> {
    write: DocumentWriter,
    roots: Option<std::ops::Range<usize>>,
    /// Only ordinary Write binds aliases to its final canonical package root.
    /// Public staging and Check retain their portable relative aliases.
    alias_directory: Option<&'a Path>,
}

fn prepare_with_writer(
    archive: &TesseractFile,
    document: &EditableFxCompositionDocument,
    directory: tempfile::TempDir,
    options: &AfterEffectsExportOptions,
    target: NativeWriteTarget<'_>,
    control: AepPreparationControl<'_>,
) -> Result<StagedAfterEffectsExport, AepConversionError> {
    control.check_cancelled()?;
    let progress = control.progress;
    let baked = match &target.roots {
        Some(range) => {
            let layers = document.composition().layers().get(range.clone()).ok_or(
                crate::writer::AepWriteError::Invalid("invalid selected root range"),
            )?;
            script_bake::prepare_layers_with_progress(document, layers, true, options, progress)?
        }
        None => script_bake::prepare_with_progress(document, options, progress)?,
    };
    let fonts = crate::export_document::fonts::ArchiveFonts::prepare(archive)?;
    let mut documents =
        ExportDocumentViews::from_preparation(document, baked.document.as_ref()).with_fonts(&fonts);
    let prepare_media = target.roots.is_some();
    if let Some(roots) = target.roots {
        documents = documents.select_roots(roots)?;
    }
    control.check_cancelled()?;
    let prepared_document = documents.prepared();
    let requests = documents.media_requests()?;
    // Preparation consumes archive source bytes, not effect output. Unmapped
    // effects remain local lowering diagnostics and cannot veto source media.
    let prepared = if prepare_media {
        let preparation = if control.preserve_video_assets {
            package::VideoPreparation::PreserveOriginal
        } else {
            package::VideoPreparation::Run
        };
        package::prepare_picture_scope_media(
            archive,
            &requests,
            directory.path(),
            preparation,
            control,
        )?
    } else {
        package::prepare_media(
            archive,
            &requests,
            directory.path(),
            prepared_document.dimensions(),
            control,
        )?
    };
    control.check_cancelled()?;
    let incomplete_scope =
        prepare_media && (!prepared.unsupported.is_empty() || prepared.preparation_withheld);
    let mut diagnostics = baked.diagnostics;
    for (asset_id, reason) in prepared.approximations {
        diagnostics.push(ExportDiagnostic {
            layer_id: None,
            message: format!("Asset {asset_id}: {reason}."),
        });
    }
    for (asset_id, reason) in prepared.unsupported {
        let consequence = if prepare_media {
            "complete selected picture dependency scope rejected"
        } else {
            "affected media layers omitted, convertible siblings retained"
        };
        diagnostics.push(ExportDiagnostic {
            layer_id: None,
            message: format!("Asset {asset_id}: {reason}; {consequence}."),
        });
    }
    // A dependency-closed picture scope cannot promote one prepared half of an
    // RGB/matte pair. The coordinator retains its complete native scope instead.
    if incomplete_scope {
        return Err(crate::writer::AepWriteError::NoConvertiblePicture(diagnostics).into());
    }
    for (asset_id, reason) in prepared.preparations {
        diagnostics.push(ExportDiagnostic {
            layer_id: None,
            message: format!("Asset {asset_id}: {reason}"),
        });
    }
    let converted = (target.write)(documents, &prepared.sources, options.fps, progress);
    control.check_cancelled()?;
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
    let font_files =
        package::fonts::prepare(&fonts, &converted.emitted_font_names, directory.path())?;
    if !font_files.is_empty() {
        converted.diagnostics.push(ExportDiagnostic {
            layer_id: None,
            message: "Embedded fonts used by exported Text are copied byte-for-byte into fonts/ with manifest.json. Install these fonts before opening/rendering in After Effects; a package-relative fonts folder does not activate them. Font redistribution remains subject to their licenses.".into(),
        });
    }
    if let Some(destination) = target.alias_directory {
        package::aliases::bind_published_media_aliases(
            &mut converted.bytes,
            &converted.emitted_media_paths,
            destination,
        )?;
    }
    let generated_project_sha256 = Sha256::digest(&converted.bytes).into();
    progress.stage("write staged AEP");
    control.check_cancelled()?;
    let source = directory.path().join("project.aep");
    std::fs::write(&source, &converted.bytes)
        .map_err(|error| AepConversionError::io("write staged AEP", &source, error))?;
    let artifacts = std::iter::once(Artifact::project("project.aep"))
        .chain(
            publication_files
                .iter()
                .map(|path| Artifact::media(path.as_str())),
        )
        .chain(font_files.iter().map(|path| Artifact::media(path.as_str())))
        .collect();
    Ok(StagedAfterEffectsExport {
        directory,
        publication_files,
        font_files,
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
