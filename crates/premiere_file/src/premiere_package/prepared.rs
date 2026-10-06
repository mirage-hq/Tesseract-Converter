//! One native preparation/lowering result shared by inspection and emission.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use fx_conv::{Artifact, ConversionReport};
use fx_schema::{EditableFxCompositionDocument, LayerId, MediaFit};
use serde_json::{json, Value};
use tesseract_file::TesseractFile;

use super::{
    absolute_output_path, bind_media, copy_native_media_except, inspect_audio, inspect_media,
    output_is_fresh,
};
use crate::{
    convert::{
        active_asset_id, apply_empty_root_picture, apply_replacements, exported_video_layers,
        lower_document_with_progress, no_native_content, video_data, AfterEffectsPicture,
        BakedDocument, PicturePackingRecipe, PictureReplacement,
    },
    error::{unsupported, BuildError, Result},
    export_loss::LossCollector,
    format::PremiereProjectXml,
    media::MediaFacts,
    ConversionError, ExportLossReport, FrameRate, Omission, PrProjectFile, Premiere,
    PremiereExportOptions,
};

/// Immutable source views and the actual result of one native export traversal.
///
/// Preparation runs bounded JS fitting, media inspection and lowering once.
/// Reading losses or consuming this operation for staging never prepares again.
/// The original remains distinct from Premiere's fitted-key view, so a future
/// cross-format planner need not mistake native preparation for original input.
///
/// This is not complete positive semantic coverage, writer validation or Adobe
/// acceptance. Those are separate from the partial loss observations below.
#[derive(Debug)]
pub struct PreparedPremiereExport<'a> {
    archive: &'a TesseractFile,
    original: &'a EditableFxCompositionDocument,
    baked: BakedDocument<'a>,
    resolved: Option<EditableFxCompositionDocument>,
    project: Option<PrProjectFile>,
    no_native_error: Option<BuildError>,
    packing: PicturePackingRecipe,
    losses: ExportLossReport,
}

/// Owned, unpublished Premiere files with optional caller-supplied AEP scopes.
///
/// Authored paths target the requested final destination, not this directory.
/// Drop/error removes only private staging. The caller must verify whole-source
/// freshness and publish the common package; borrowing an archive is not a
/// filesystem lock or an atomic source snapshot.
#[derive(Debug)]
pub struct StagedPicturePremiereExport {
    directory: tempfile::TempDir,
    foreign_paths: Vec<PathBuf>,
    report: ConversionReport<Omission>,
    generated_project_sha256: [u8; 32],
}

impl StagedPicturePremiereExport {
    /// Private files, laid out according to the report's relative artifact paths.
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// Files actually staged and diagnostics from the retained native traversal.
    pub fn report(&self) -> &ConversionReport<Omission> {
        &self.report
    }

    /// SHA-256 of the compressed bytes written to the staged project.
    pub fn generated_project_sha256(&self) -> &[u8; 32] {
        &self.generated_project_sha256
    }

    /// Canonical package-relative paths; these foreign files are not staged here.
    pub fn after_effects_paths(&self) -> &[PathBuf] {
        &self.foreign_paths
    }
}

enum PictureStage<'a> {
    Replacements(&'a [PictureReplacement]),
    EmptyRoot(&'a AfterEffectsPicture),
}

fn validate_empty_root_document(document: &EditableFxCompositionDocument) -> Result<()> {
    let composition = document.composition();
    if !composition.layers().is_empty()
        || !composition.dynamics().entries().is_empty()
        || document.unknown_field_names().next().is_some()
        || composition.has_unknown_fields()
        || document
            .background_color()
            .is_some_and(|color| color[3] != 0.0)
    {
        return Err(unsupported("empty-root linking requires an originally empty document without unknown fields, animation or painted background"));
    }
    // The typed canvas exposes width/height only. The lossless envelope also
    // retains unknown dimensions controls, which cannot certify an empty root.
    let envelope = document
        .to_json_value()
        .map_err(|error| unsupported(error.to_string()))?;
    if !envelope["dimensions"].as_object().is_some_and(|fields| {
        fields
            .keys()
            .all(|name| matches!(name.as_str(), "width" | "height"))
    }) {
        return Err(unsupported(
            "empty-root linking cannot omit unknown dimensions fields",
        ));
    }
    Ok(())
}

impl Premiere {
    /// Prepare one caller-owned document view without publishing or running Adobe.
    ///
    /// Native-only, inspection and supplied-picture paths share this operation.
    /// Empty native content retains observations; attempting to stage it returns
    /// the ordinary no-convertible-content error. Other conversion failures are
    /// returned here. Writer-only constraints are checked on the final picture
    /// during staging, after any supplied replacements.
    pub fn prepare_export<'a>(
        &self,
        archive: &'a TesseractFile,
        document: &'a EditableFxCompositionDocument,
        options: &PremiereExportOptions,
    ) -> std::result::Result<PreparedPremiereExport<'a>, ConversionError> {
        prepare(archive, document, options).map_err(Into::into)
    }

    /// [`Self::prepare_export`] with command-scoped progress observations.
    pub fn prepare_export_with_progress<'a>(
        &self,
        archive: &'a TesseractFile,
        document: &'a EditableFxCompositionDocument,
        options: &PremiereExportOptions,
        progress: fx_conv::Progress<'_>,
    ) -> std::result::Result<PreparedPremiereExport<'a>, ConversionError> {
        prepare_with_progress(archive, document, options, progress).map_err(Into::into)
    }
}

pub(super) fn prepare<'a>(
    archive: &'a TesseractFile,
    document: &'a EditableFxCompositionDocument,
    options: &PremiereExportOptions,
) -> Result<PreparedPremiereExport<'a>> {
    prepare_with_progress(archive, document, options, fx_conv::Progress::default())
}

pub(super) fn prepare_with_progress<'a>(
    archive: &'a TesseractFile,
    document: &'a EditableFxCompositionDocument,
    options: &PremiereExportOptions,
    progress: fx_conv::Progress<'_>,
) -> Result<PreparedPremiereExport<'a>> {
    let mut collector = LossCollector::default();
    let baked = crate::convert::bake_scripts_with_progress(document, &mut collector, progress)?;
    progress.stage("inspecting Premiere media");
    let media = inspect_media(archive, baked.document())?;
    let mut natural_frames = BTreeSet::new();
    let resolved = resolve_natural_video_frames(baked.document(), &media, &mut natural_frames)?;
    let native_document = resolved.as_ref().unwrap_or_else(|| baked.document());
    let audio = inspect_audio(archive, native_document, &media)?;
    let lowered = lower_document_with_progress(
        native_document,
        &natural_frames,
        &media,
        &audio,
        &archive.metadata().fonts,
        options.frame_rate.unwrap_or(FrameRate::Fps30),
        &mut collector,
        progress,
    )?;
    // Ordinary export historically fails before the discarded-key summary when
    // no project survives. Preserve that error while retaining full inspection.
    let project = lowered.project;
    let has_native_content = project.is_some();
    let no_native_error = (!has_native_content).then(|| no_native_content(collector.diagnostics()));
    let packing = lowered.packing;
    baked.report_discarded(&lowered.written, &mut collector);
    let losses = collector.finish(has_native_content);
    Ok(PreparedPremiereExport {
        archive,
        original: document,
        baked,
        resolved,
        project,
        no_native_error,
        packing,
        losses,
    })
}

/// The renderer gives an omitted video frame the active media's natural size.
/// Resolve only already-inspected export candidates in a private view: omitted
/// roots stay unprobed, explicit frames retain their validation, and AE still
/// extracts the unmodified original. Script fitting retains its existing policy
/// before inspection; this does not add frame-dependent script support.
fn resolve_natural_video_frames(
    document: &EditableFxCompositionDocument,
    media: &BTreeMap<String, MediaFacts>,
    natural_frames: &mut BTreeSet<LayerId>,
) -> Result<Option<EditableFxCompositionDocument>> {
    let composition = document.composition();
    let dimensions = document.dimensions();
    let mut frames = BTreeMap::new();
    for layer in exported_video_layers(
        composition.layers(),
        composition.dynamics(),
        [dimensions.width, dimensions.height],
    ) {
        let Some(video) = video_data(layer)? else {
            continue;
        };
        if video.source.frame_rect.is_some() {
            continue;
        }
        let Some(MediaFacts::Video(facts)) = media.get(active_asset_id(&video.source).as_str())
        else {
            continue;
        };
        // Presets on the natural frame all have unit media scale. Custom keeps
        // its independently authored content geometry and existing diagnostic.
        let preset = !matches!(video.source.fit, MediaFit::Custom { .. });
        if preset {
            // Keep natural display geometry distinct from a fixed coded
            // frame while existing frame-dependent lowering uses that frame.
            natural_frames.insert(layer.id());
        }
        frames.insert(layer.id(), ([facts.width, facts.height], preset));
    }
    if frames.is_empty() {
        return Ok(None);
    }
    fn resolve(layers: &mut [Value], frames: &BTreeMap<LayerId, ([u32; 2], bool)>) {
        for layer in layers {
            if let Some((dimensions, preset)) = layer["id"]
                .as_u64()
                .and_then(|id| frames.get(&LayerId::new(id)))
            {
                layer["source"]["sourceRect"] = json!({
                    "x": 0, "y": 0, "width": dimensions[0], "height": dimensions[1]
                });
                if *preset {
                    layer["source"]["fit"] = json!("contain");
                }
            }
            if let Some(children) = layer.get_mut("layers").and_then(Value::as_array_mut) {
                resolve(children, frames);
            }
        }
    }
    let mut wire = document
        .to_json_value()
        .map_err(|error| unsupported(error.to_string()))?;
    let layers = wire["composition"]["layers"]
        .as_array_mut()
        .ok_or_else(|| unsupported("a composition must contain layers"))?;
    resolve(layers, &frames);
    EditableFxCompositionDocument::from_json_value(wire)
        .map(Some)
        .map_err(|error| unsupported(error.to_string()))
}

impl PreparedPremiereExport<'_> {
    /// Unmodified input for cross-format source extraction; never the fitted view.
    pub fn original_document(&self) -> &EditableFxCompositionDocument {
        self.original
    }

    /// The fitted native view with inspected natural video frames resolved.
    pub fn prepared_document(&self) -> &EditableFxCompositionDocument {
        self.resolved
            .as_ref()
            .unwrap_or_else(|| self.baked.document())
    }

    /// Partial observations from that traversal, not a complete support verdict.
    pub fn losses(&self) -> &ExportLossReport {
        &self.losses
    }

    /// Source slots recorded by the actual retained native lowering traversal.
    pub fn packing_recipe(&self) -> &PicturePackingRecipe {
        &self.packing
    }

    pub(super) fn into_losses(self) -> ExportLossReport {
        self.losses
    }

    pub(super) fn into_native(self) -> Result<(PrProjectFile, Vec<Omission>)> {
        let project = self.project.ok_or_else(|| {
            self.no_native_error
                .unwrap_or_else(|| no_native_content(&self.losses.diagnostics))
        })?;
        Ok((project, self.losses.diagnostics))
    }

    /// Replay supplied source-slot replacements against this exact preparation.
    /// An empty slice stages native content only. Final output must be fresh,
    /// with an existing parent; no final output directory is created here.
    /// No lowering, fitting, or source evaluation is repeated. Admission of the
    /// foreign picture's semantics remains the coordinator's responsibility.
    pub fn stage_with_picture_replacements(
        self,
        staging_parent: &Path,
        final_output: &Path,
        replacements: &[PictureReplacement],
    ) -> std::result::Result<StagedPicturePremiereExport, ConversionError> {
        self.stage(
            staging_parent,
            final_output,
            PictureStage::Replacements(replacements),
        )
        .map_err(Into::into)
    }

    /// Link an independently staged AEP of the original, genuinely empty root.
    ///
    /// Both the archive and caller's view must be genuinely empty, including
    /// retained document, dimensions and composition fields: a stripped view
    /// cannot hide authored content or unknown semantics. The caller supplies
    /// the actual generated root GUID and verifies its source duration;
    /// ordinary picture/path/clock validation and publication apply.
    pub fn stage_empty_root_with_after_effects(
        self,
        staging_parent: &Path,
        final_output: &Path,
        picture: &AfterEffectsPicture,
    ) -> std::result::Result<StagedPicturePremiereExport, ConversionError> {
        validate_empty_root_document(self.archive.project())?;
        validate_empty_root_document(self.original)?;
        self.stage(
            staging_parent,
            final_output,
            PictureStage::EmptyRoot(picture),
        )
        .map_err(Into::into)
    }

    fn stage(
        self,
        staging_parent: &Path,
        final_output: &Path,
        pictures: PictureStage<'_>,
    ) -> Result<StagedPicturePremiereExport> {
        let output = absolute_output_path(final_output, Path::new("."))?;
        output_is_fresh(&output)?;
        let Self {
            archive,
            mut project,
            no_native_error,
            packing,
            losses,
            ..
        } = self;
        if let Some(project) = &mut project {
            bind_media(project, archive, &output)?;
        }
        let mut foreign_paths = Vec::new();
        let mut foreign_media_ids = BTreeSet::new();
        let packed = match pictures {
            PictureStage::Replacements([]) => None,
            PictureStage::Replacements(replacements) => Some(apply_replacements(
                project.take(),
                packing,
                replacements,
                &output,
            )?),
            PictureStage::EmptyRoot(picture) => {
                Some(apply_empty_root_picture(packing, picture, &output)?)
            }
        };
        let project = if let Some(packed) = packed {
            foreign_paths = packed.foreign_paths;
            foreign_media_ids = packed.foreign_media_ids;
            packed.project
        } else {
            project.ok_or_else(|| {
                no_native_error.unwrap_or_else(|| no_native_content(&losses.diagnostics))
            })?
        };
        let diagnostics = losses.diagnostics;
        let artifacts = std::iter::once(Artifact::project("project.prproj"))
            .chain(
                project
                    .media
                    .iter()
                    .filter(|(id, media)| !media.is_generator() && !foreign_media_ids.contains(*id))
                    .map(|(_, media)| Artifact::media(Path::new("media").join(&media.name))),
            )
            .collect();
        let directory = tempfile::Builder::new()
            .prefix(".conversion-premiere-")
            .tempdir_in(staging_parent)?;
        // Foreign sources are never inspected/copied as archive-native media.
        copy_native_media_except(archive, directory.path(), &project, &foreign_media_ids)?;
        let generated_project_sha256 = PremiereProjectXml::new(&project)?
            .write_new(&directory.path().join("project.prproj"))?;
        Ok(StagedPicturePremiereExport {
            directory,
            foreign_paths,
            generated_project_sha256,
            report: ConversionReport {
                diagnostics,
                artifacts,
            },
        })
    }
}

#[cfg(test)]
mod tests;
