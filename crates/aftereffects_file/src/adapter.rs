//! Best-effort AEP import and experimental editable native export boundaries.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

use fx_conv::{
    Artifact, ConversionMode, ConversionReport, ImportTarget, ImportToTesseract, MediaMapError,
    MediaPreflight as MediaPreflightReport, Progress, ValidatedMediaMap,
};
use tesseract_file::{TesseractFileBuilder, TesseractFileError};
use thiserror::Error;

use crate::{
    diagnostic::ImportDiagnostic,
    document::DocumentError,
    expression_samples::{ExpressionSamples, ExpressionSamplesError},
    structure::{ItemKind, MediaKind, StructuralProject, StructureError, read_project},
    structure_document::{
        AssetNamespace, MediaAssetRequest, asset_request_for_source,
        to_structural_fx_document_with_media_and_expressions_and_progress,
    },
};

mod diagnostics;
mod export;
mod linked_import;
mod media;

pub use export::{StagedAfterEffectsExport, StagedAfterEffectsPictureExport};
pub use linked_import::{
    DynamicLinkImportError, ImportedAfterEffectsComposition, LinkedMedia, LinkedPicture,
    LinkedPictureTarget, PreparedAfterEffectsImport, ResolvedAfterEffectsComposition,
};

const OUTPUT_NAME: &str = "project.tsrct";

/// Best-effort import and experimental editable native export, with explicit loss diagnostics.
#[derive(Debug, Clone, Copy, Default)]
pub struct AfterEffects;

/// Selects the AE source composition without confusing its name with its item ID.
#[derive(Debug, Clone, Default)]
pub struct AfterEffectsImportOptions {
    /// Source item ID. Omission is valid only when the source has one composition.
    pub composition: Option<u32>,
    /// Explicit AE-evaluated expression values, bound to the source AEP hash.
    pub expression_samples: Option<PathBuf>,
}

/// Output sampling rate for an edited FX composition. FX timestamps remain in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AfterEffectsExportOptions {
    /// Requested nominal frames per second; defaults to 24 for legacy callers.
    pub fps: f64,
}

impl Default for AfterEffectsExportOptions {
    fn default() -> Self {
        Self { fps: 24.0 }
    }
}

/// An AEP conversion failed before success was reported.
#[derive(Debug, Error)]
pub enum AepConversionError {
    /// The source is not a regular file or has invalid input metadata.
    #[error("invalid AEP input: {0}")]
    Input(&'static str),
    /// A video source cannot be admitted to the playback/export decoder.
    #[error(
        "invalid or unsupported video asset {asset_id} at {path:?}: {reason}; transcode the source to a supported format before importing"
    )]
    VideoMedia {
        /// Logical identity referenced by the source project.
        asset_id: fx_schema::AssetId,
        /// Resolved local source path.
        path: PathBuf,
        /// Container or codec admission failure, not a successful omission.
        reason: String,
    },
    /// A prepared source no longer contains the exact bytes whose identity was resolved.
    #[error("prepared AEP source changed: {0:?}")]
    SourceChanged(PathBuf),
    /// A linked picture's local media no longer has the bytes, or the
    /// location, that its conversion read.
    #[error("linked AEP media changed after conversion read it: {0:?}")]
    MediaChanged(PathBuf),
    /// The output cannot designate a new directory.
    #[error("invalid AEP conversion output: {0}")]
    Output(&'static str),
    /// Filesystem failure with the operation and affected path preserved.
    #[error("{operation} {path:?}: {source}")]
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Path used by that operation.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },
    /// A prepared-media sidecar is invalid, stale, or bound to another source/target.
    #[error(transparent)]
    MediaMap(#[from] MediaMapError),
    /// Framing or required structural records could not be read safely.
    #[error(transparent)]
    Read(#[from] StructureError),
    /// The expression-sample source is not a regular file.
    #[error("invalid expression samples input: {0}")]
    ExpressionSamplesInput(&'static str),
    /// Expression-sample JSON failed bounded validation.
    #[error(transparent)]
    ExpressionSamples(#[from] ExpressionSamplesError),
    /// Editable FX construction failed.
    #[error(transparent)]
    Document(#[from] DocumentError),
    /// Archive validation or writing failed.
    #[error(transparent)]
    Archive(#[from] TesseractFileError),
    /// Encoding normalized still-image media failed, including output I/O.
    #[error("cannot encode normalized AEP image: {0}")]
    Image(#[from] image::ImageError),
    /// Fresh native output could not be represented within writer bounds.
    #[error(transparent)]
    Write(#[from] crate::writer::AepWriteError),
}

impl AepConversionError {
    /// Only an entirely omitted selected picture scope may retain native output.
    /// I/O, malformed input and other conversion failures are not fallback signals.
    pub fn picture_scope_omissions(&self) -> Option<&[crate::ExportDiagnostic]> {
        match self {
            Self::Write(crate::writer::AepWriteError::NoConvertiblePicture(diagnostics)) => {
                Some(diagnostics)
            }
            _ => None,
        }
    }

    fn io(operation: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            operation,
            path: path.to_owned(),
            source,
        }
    }
}

impl AfterEffects {
    /// Inspects exactly one selected composition's native video and audio references.
    pub fn inspect_media(
        &self,
        input: &Path,
        options: &AfterEffectsImportOptions,
        media_map: Option<&ValidatedMediaMap>,
    ) -> Result<MediaPreflightReport, AepConversionError> {
        let bytes = read_input(input)?;
        let project = read_project(&bytes)?;
        let target = selected_composition_id(&project, options.composition)?;
        if let Some(media_map) = media_map {
            media_map.validate_for(input, "after-effects", &target)?;
        }
        inspect_project_media(input, &project, options.composition, media_map)
    }

    /// Import with explicit, source-bound prepared media while preserving ordinary admission checks.
    pub fn import_with_media_map(
        &self,
        input: &Path,
        output: &Path,
        options: &AfterEffectsImportOptions,
        mode: ConversionMode,
        media_map: &ValidatedMediaMap,
    ) -> Result<ConversionReport<ImportDiagnostic>, AepConversionError> {
        self.import_with_media_map_with_progress(
            input,
            output,
            options,
            mode,
            media_map,
            Progress::default(),
        )
    }

    /// Imports with source-bound media and phase-local progress observations.
    pub fn import_with_media_map_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &AfterEffectsImportOptions,
        mode: ConversionMode,
        media_map: &ValidatedMediaMap,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<ImportDiagnostic>, AepConversionError> {
        self.import(input, output, options, mode, Some(media_map), progress)
    }

    fn import(
        &self,
        input: &Path,
        output: &Path,
        options: &AfterEffectsImportOptions,
        mode: ConversionMode,
        media_map: Option<&ValidatedMediaMap>,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<ImportDiagnostic>, AepConversionError> {
        let destination = fresh_destination(output)?;
        progress.stage("read AEP");
        let bytes = read_input(input)?;
        let expression_samples = options
            .expression_samples
            .as_deref()
            .map(|path| read_expression_samples(path, &bytes))
            .transpose()?
            .unwrap_or_default();
        let project = read_project(&bytes)?;
        let target = selected_composition_id(&project, options.composition)?;
        if let Some(media_map) = media_map {
            media_map.validate_for(input, "after-effects", &target)?;
        }
        let prepared = import_builder_with_media_map(
            input,
            &project,
            options.composition,
            &expression_samples,
            media_map,
            progress,
        )?;
        if let Some(media_map) = media_map {
            media_map.validate_for(input, "after-effects", &target)?;
        }
        if !mode.is_check() {
            progress.stage("write Tesseract");
            write_project_checked(prepared.builder, &destination, || {
                if let Some(media_map) = media_map {
                    media_map.validate_for(input, "after-effects", &target)?;
                }
                Ok(())
            })?;
        }
        Ok(ConversionReport {
            diagnostics: prepared.diagnostics,
            artifacts: vec![Artifact::project(OUTPUT_NAME)],
        })
    }
}

impl ImportToTesseract for AfterEffects {
    type Options = AfterEffectsImportOptions;
    type Diagnostic = ImportDiagnostic;
    type Error = AepConversionError;

    fn list_import_targets(&self, input: &Path) -> Result<Vec<ImportTarget>, Self::Error> {
        let bytes = read_input(input)?;
        let project = read_project(&bytes)?;
        Ok(import_targets(&project))
    }

    fn import_to_tesseract(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        self.import_to_tesseract_with_progress(input, output, options, mode, Progress::default())
    }

    fn import_to_tesseract_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        self.import(input, output, options, mode, None, progress)
    }
}

// Avoid whitespace inflation in deeply nested editable tracks while preserving
// every key and the exact editable model. The byte constructor revalidates the
// same schema and preserves these compact bytes when publishing.
fn compact_archive(
    document: fx_schema::EditableFxCompositionDocument,
) -> Result<TesseractFileBuilder, TesseractFileError> {
    let bytes = serde_json::to_vec(&document).map_err(fx_schema::EditableFxDocumentError::from)?;
    drop(document);
    TesseractFileBuilder::from_project_json(&bytes)
}

struct ImportBuild<'a> {
    builder: TesseractFileBuilder,
    diagnostics: Vec<ImportDiagnostic>,
    // Normalized PSD PNGs are temporary files owned by preflight. Keep them
    // alive through archive validation/writing, including partial field moves.
    _media_guard: media::MediaPreflight<'a>,
}

fn import_builder<'a>(
    input: &'a Path,
    project: &StructuralProject,
    composition: Option<u32>,
    expression_samples: &ExpressionSamples,
) -> Result<ImportBuild<'a>, AepConversionError> {
    import_builder_with_media_map(
        input,
        project,
        composition,
        expression_samples,
        None,
        Progress::default(),
    )
}

fn import_builder_with_media_map<'a>(
    input: &'a Path,
    project: &StructuralProject,
    composition: Option<u32>,
    expression_samples: &ExpressionSamples,
    media_map: Option<&'a ValidatedMediaMap>,
    progress: Progress<'_>,
) -> Result<ImportBuild<'a>, AepConversionError> {
    let mut preflight = media::MediaPreflight::with_project(input, media_map, project);
    for reference in native_media_references(project, composition)? {
        preflight.require(&reference.request)?;
    }
    let mut converted = to_structural_fx_document_with_media_and_expressions_and_progress(
        project,
        composition,
        &mut |request| preflight.resolve_media(request),
        expression_samples,
        progress,
    )?;
    if let Some(error) = preflight.failure.take() {
        return Err(error);
    }
    let builder = compact_archive(converted.document)?;
    let builder = preflight.add_used_assets(builder, &converted.assets)?;
    converted.diagnostics.append(&mut preflight.diagnostics);
    builder.validate()?;
    Ok(ImportBuild {
        builder,
        diagnostics: converted.diagnostics,
        _media_guard: preflight,
    })
}

#[derive(Clone)]
pub(super) struct NativeMediaReference {
    pub(super) source_id: u32,
    pub(super) source_name: String,
    pub(super) request: MediaAssetRequest,
    pub(super) references: Vec<String>,
}

pub(super) fn native_media_inventory(
    project: &StructuralProject,
    composition: Option<u32>,
) -> Result<(Vec<NativeMediaReference>, Vec<String>), DocumentError> {
    let selected = selected_composition_id(project, composition)?
        .parse::<u32>()
        .map_err(|_| DocumentError::NoComposition)?;
    let items: HashMap<_, _> = project.items.iter().map(|item| (item.id, item)).collect();
    let mut references = HashMap::<u32, NativeMediaReference>::new();
    let mut unassessed = Vec::new();
    collect_native_media(selected, &items, &mut references, &mut unassessed);
    let mut references: Vec<_> = references.into_values().collect();
    references.sort_by_key(|reference| reference.source_id);
    Ok((references, unassessed))
}

pub(super) fn native_media_references(
    project: &StructuralProject,
    composition: Option<u32>,
) -> Result<Vec<NativeMediaReference>, DocumentError> {
    native_media_inventory(project, composition).map(|(references, _)| references)
}

fn collect_native_media(
    selected_composition_id: u32,
    items: &HashMap<u32, &crate::structure::ProjectItem>,
    references: &mut HashMap<u32, NativeMediaReference>,
    unassessed: &mut Vec<String>,
) {
    enum Visit {
        Enter {
            composition_id: u32,
            replacements: HashMap<(u32, u32), u32>,
        },
        Exit(u32),
    }

    let mut visits = vec![Visit::Enter {
        composition_id: selected_composition_id,
        replacements: HashMap::new(),
    }];
    let mut active = HashSet::new();
    while let Some(visit) = visits.pop() {
        let (composition_id, replacements) = match visit {
            Visit::Enter {
                composition_id,
                replacements,
            } => (composition_id, replacements),
            Visit::Exit(composition_id) => {
                active.remove(&composition_id);
                continue;
            }
        };
        if !active.insert(composition_id) {
            continue;
        }
        let Some(item) = items.get(&composition_id) else {
            active.remove(&composition_id);
            continue;
        };
        let ItemKind::Composition(composition) = &item.kind else {
            active.remove(&composition_id);
            continue;
        };
        visits.push(Visit::Exit(composition_id));
        let mut nested_visits = Vec::new();
        for layer in &composition.layers {
            let source_id = replacements
                .get(&(composition_id, layer.record.id()))
                .copied()
                .unwrap_or_else(|| layer.record.source_id());
            let Some(source) = items.get(&source_id) else {
                if source_id != 0 {
                    unassessed.push(format!(
                        "composition {composition_id} layer {} references missing source {source_id}",
                        layer.record.id()
                    ));
                }
                continue;
            };
            let label = format!("composition {composition_id} layer {}", layer.record.id());
            if let Some(Err(error)) = &source.native_media {
                unassessed.push(format!(
                    "source {} ({}) native media descriptor is unreadable: {error}",
                    source.id, source.name
                ));
            }
            let request = source
                .native_media
                .as_ref()
                .and_then(|descriptor| descriptor.as_ref().ok())
                .filter(|descriptor| {
                    matches!(
                        descriptor.kind,
                        MediaKind::Video | MediaKind::Audio | MediaKind::AudioVideo
                    )
                })
                .and_then(|descriptor| {
                    let mut native_source = (*source).clone();
                    native_source.media = Some(Ok(descriptor.clone()));
                    asset_request_for_source(&native_source, AssetNamespace::STANDALONE)
                });
            if let Some(request) = request {
                references
                    .entry(source_id)
                    .and_modify(|reference| reference.references.push(label.clone()))
                    .or_insert_with(|| NativeMediaReference {
                        source_id,
                        source_name: source.name.clone(),
                        request,
                        references: vec![label],
                    });
            }
            if let ItemKind::Composition(nested) = &source.kind {
                let parsed = crate::essential::overrides(
                    &layer.content,
                    &nested.essential_properties.values,
                );
                unassessed.extend(
                    parsed
                        .warnings
                        .iter()
                        .filter(|warning| warning.affects_media)
                        .map(|warning| {
                            format!(
                                "composition {composition_id} layer {} Essential media scope: {}",
                                layer.record.id(),
                                warning.message
                            )
                        }),
                );
                let mut nested_replacements = HashMap::new();
                for value in parsed.values {
                    if let crate::essential::OverrideValue::Media { source_id } = value.value {
                        nested_replacements
                            .insert((value.source_comp_id, value.source_layer_id), source_id);
                    }
                }
                // Conversion applies occurrence-local values first, then enclosing values;
                // preserve that exact precedence while walking descendants.
                nested_replacements.extend(replacements.iter().map(|(key, value)| (*key, *value)));
                nested_visits.push(Visit::Enter {
                    composition_id: source.id,
                    replacements: nested_replacements,
                });
            }
        }
        visits.extend(nested_visits.into_iter().rev());
    }
}

fn inspect_project_media(
    input: &Path,
    project: &StructuralProject,
    composition: Option<u32>,
    media_map: Option<&ValidatedMediaMap>,
) -> Result<MediaPreflightReport, AepConversionError> {
    let target = selected_composition_id(project, composition)?;
    let (references, unassessed) = native_media_inventory(project, composition)?;
    let mut resolver = media::MediaPreflight::with_project(input, media_map, project);
    let mut inspected = Vec::with_capacity(references.len());
    for reference in references {
        inspected.push(resolver.inspect(
            &reference.request,
            reference.source_id.to_string(),
            reference.source_name,
            reference.references,
        ));
    }
    Ok(MediaPreflightReport {
        format: "after-effects".into(),
        target,
        media: inspected,
        unassessed,
    })
}

pub(super) fn selected_composition_id(
    project: &StructuralProject,
    composition: Option<u32>,
) -> Result<String, DocumentError> {
    let compositions: Vec<_> = project
        .items
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Composition(_)))
        .collect();
    let selected = if let Some(id) = composition {
        compositions
            .into_iter()
            .find(|item| item.id == id)
            .ok_or(DocumentError::CompositionSelection(id))?
    } else {
        match compositions.as_slice() {
            [] => return Err(DocumentError::NoComposition),
            [selected] => selected,
            multiple => {
                return Err(DocumentError::AmbiguousCompositionSelection {
                    count: multiple.len(),
                });
            }
        }
    };
    Ok(selected.id.to_string())
}

fn import_targets(project: &StructuralProject) -> Vec<ImportTarget> {
    project
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Composition(composition) = &item.kind else {
                return None;
            };
            Some(ImportTarget {
                id: item.id.to_string(),
                name: item.name.clone(),
                width: Some(u32::from(composition.width)),
                height: Some(u32::from(composition.height)),
                fps: Some(composition.frame_rate),
                duration_secs: Some(composition.duration_secs),
                layer_count: Some(composition.layers.len()),
                video_track_count: None,
                audio_track_count: None,
            })
        })
        .collect()
}

// Resolve the existing parent once, but never follow or replace the final entry.
// Check mode performs exactly these same path checks without creating anything.
fn fresh_destination(output: &Path) -> Result<PathBuf, AepConversionError> {
    let name = output.file_name().ok_or(AepConversionError::Output(
        "a fresh directory name is required",
    ))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|source| AepConversionError::io("resolve output parent", parent, source))?;
    let destination = parent.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => Err(AepConversionError::Output("destination already exists")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(destination),
        Err(source) => Err(AepConversionError::io(
            "inspect output destination",
            &destination,
            source,
        )),
    }
}

fn read_expression_samples(
    input: &Path,
    source: &[u8],
) -> Result<ExpressionSamples, AepConversionError> {
    let metadata = fs::metadata(input).map_err(|source| {
        AepConversionError::io("inspect expression samples input", input, source)
    })?;
    if !metadata.is_file() {
        return Err(AepConversionError::ExpressionSamplesInput(
            "expected a regular file",
        ));
    }
    let mut file = File::open(input)
        .map_err(|source| AepConversionError::io("open expression samples input", input, source))?;
    let metadata = file.metadata().map_err(|source| {
        AepConversionError::io("inspect opened expression samples input", input, source)
    })?;
    if !metadata.is_file() {
        return Err(AepConversionError::ExpressionSamplesInput(
            "expected a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| AepConversionError::io("read expression samples input", input, source))?;
    Ok(ExpressionSamples::from_json_for_source(&bytes, source)?)
}

fn read_input(input: &Path) -> Result<Vec<u8>, AepConversionError> {
    let metadata = fs::metadata(input)
        .map_err(|source| AepConversionError::io("inspect AEP input", input, source))?;
    if !metadata.is_file() {
        return Err(AepConversionError::Input("expected a regular file"));
    }
    let file = File::open(input)
        .map_err(|source| AepConversionError::io("open AEP input", input, source))?;
    let metadata = file
        .metadata()
        .map_err(|source| AepConversionError::io("inspect opened AEP input", input, source))?;
    if !metadata.is_file() {
        return Err(AepConversionError::Input("expected a regular file"));
    }
    read_bytes(file, input)
}

fn read_bytes(mut reader: impl Read, input: &Path) -> Result<Vec<u8>, AepConversionError> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|source| AepConversionError::io("read AEP input", input, source))?;
    Ok(bytes)
}

fn write_project_checked(
    builder: TesseractFileBuilder,
    destination: &Path,
    verify_source: impl FnOnce() -> Result<(), AepConversionError>,
) -> Result<(), AepConversionError> {
    let parent = destination
        .parent()
        .ok_or(AepConversionError::Output("output parent is missing"))?;
    let staged = tempfile::Builder::new()
        .prefix(".conversion-aftereffects-")
        .tempdir_in(parent)
        .map_err(|source| {
            AepConversionError::io("create AEP conversion staging", parent, source)
        })?;
    let source = staged.path().join(OUTPUT_NAME);
    // Validate/reopen the complete archive before reserving the public directory.
    // Drop its file handle before linking (also important on Windows).
    drop(builder.write(&source)?);
    verify_source()?;
    publish(&source, destination)
}

// Match the existing conversion publisher: reserve a fresh directory, then link
// without replacement. This is not crash-atomic directory publication. A crash
// can leave staging or an empty destination; ordinary errors clean up owned data.
fn publish(source: &Path, destination: &Path) -> Result<(), AepConversionError> {
    publish_named(source, destination, OUTPUT_NAME)
}

fn publish_named(source: &Path, destination: &Path, name: &str) -> Result<(), AepConversionError> {
    fs::create_dir(destination)
        .map_err(|source| AepConversionError::io("create output directory", destination, source))?;
    let target = destination.join(name);
    if let Err(source) = fs::hard_link(source, &target) {
        // Never recurse: if another actor created entries, leave them untouched.
        let _ = fs::remove_dir(destination);
        return Err(AepConversionError::io(
            "publish converted project",
            &target,
            source,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
