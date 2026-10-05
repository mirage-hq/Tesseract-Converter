//! Premiere implementation of the format-neutral FX document conversion boundary.

use crate::{
    format::PrProjectFile, premiere_package, tesseract_import::TesseractImport, validate_paths,
    ConversionError, FrameRate, Omission, OmissionKind, OmissionScope,
};
use fx_conv::{
    ConversionDiagnostic, ConversionMode, ConversionReport, Diagnostic, DiagnosticKind,
    ExportFromTesseract, ImportToTesseract, Progress, ValidatedMediaMap,
};
use std::path::Path;

impl ConversionDiagnostic for Omission {
    fn diagnostic(&self) -> Diagnostic {
        Diagnostic {
            code: match self.scope {
                OmissionScope::Feature => "PREMIERE-FEATURE",
                OmissionScope::Occurrence => "PREMIERE-OCCURRENCE",
                OmissionScope::Track => "PREMIERE-TRACK",
                OmissionScope::Sequence => "PREMIERE-SEQUENCE",
            },
            // Its scope identifies the target, not the effect. An omitted report
            // can also be a caveat about converted content, so only an
            // approximation claims a more specific kind.
            kind: match self.kind {
                OmissionKind::Omitted => DiagnosticKind::Warning,
                OmissionKind::Approximated => DiagnosticKind::Approximation,
            },
            context: Some(format!("{} {}", self.scope, self.record)),
            message: self.to_string(),
        }
    }
}

/// Premiere interchange; import and export can be used independently.
#[derive(Debug, Clone, Copy, Default)]
pub struct Premiere;

/// Premiere-specific import selection.
#[derive(Debug, Clone, Default)]
pub struct PremiereImportOptions {
    /// GUID of the one sequence to import, or `None` for the only selectable one.
    ///
    /// Selectable sequences, nested ones included, are those that
    /// [`ImportToTesseract::list_import_targets`] lists. A GUID must name one;
    /// `None` rejects a project with none or several.
    pub sequence: Option<String>,
}

/// Premiere-specific export settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct PremiereExportOptions {
    /// Sequence frame rate; `None` exports 30 fps.
    pub frame_rate: Option<FrameRate>,
}

impl Premiere {
    /// Inspect time-based media reached by exactly one native sequence.
    ///
    /// This uses the same target selection, native path resolution, explicit
    /// replacement lookup, and media admission as import. It never starts a
    /// decoder process or mutates the project or its sources.
    pub fn inspect_media(
        &self,
        input: &Path,
        options: &PremiereImportOptions,
        media_map: Option<&ValidatedMediaMap>,
    ) -> Result<fx_conv::MediaPreflight, ConversionError> {
        if input.to_str().is_none() {
            return Err(crate::error::BuildError::Path("input").into());
        }
        let input = input
            .canonicalize()
            .map_err(crate::error::BuildError::from)?;
        let targets = PrProjectFile::import_targets(&input)?;
        let target = match options.sequence.as_deref() {
            Some(id) if targets.iter().any(|target| target.id == id) => id,
            None if targets.len() == 1 => targets[0].id.as_str(),
            _ => {
                return Err(crate::error::unsupported(
                    "Premiere inspection must identify exactly one native sequence",
                )
                .into())
            }
        };
        if let Some(media_map) = media_map {
            if target.is_empty() {
                return Err(crate::error::unsupported(
                    "prepared media requires a stable sequence ID",
                )
                .into());
            }
            media_map
                .validate_for(&input, "premiere", target)
                .map_err(crate::error::BuildError::from)?;
        }
        Ok(crate::tesseract_output::inspect_native_premiere_media(
            &input, target, media_map,
        )?)
    }

    /// Import one sequence with source-bound prepared media substitutions.
    /// Native Premiere paths are resolved before the map is consulted, and
    /// every selected replacement still passes ordinary media admission.
    pub fn import_with_media_map(
        &self,
        input: &Path,
        output: &Path,
        options: &PremiereImportOptions,
        mode: ConversionMode,
        map: &ValidatedMediaMap,
    ) -> Result<ConversionReport<Omission>, ConversionError> {
        self.import_with_media_map_with_progress(
            input,
            output,
            options,
            mode,
            map,
            Progress::default(),
        )
    }

    /// [`Self::import_with_media_map`] with command-scoped progress observations.
    pub fn import_with_media_map_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &PremiereImportOptions,
        mode: ConversionMode,
        map: &ValidatedMediaMap,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<Omission>, ConversionError> {
        validate_paths(input, output)?;
        let mut import = TesseractImport::convert_with_media_map(
            input,
            output,
            options.sequence.as_deref(),
            map,
            progress,
        )?;
        let report = ConversionReport {
            diagnostics: std::mem::take(&mut import.omissions),
            artifacts: import.artifacts(),
        };
        if !mode.is_check() {
            progress.stage("write and publish Tesseract project");
            import.write_with_media_map(map)?;
        }
        Ok(report)
    }

    /// Import with explicit, source/sequence/Media-UID-bound local relocation.
    /// Unlike prepared substitutions, these bindings precede native path admission.
    /// The selected bytes still pass normal media, clock and publication checks.
    pub fn import_with_media_relink(
        &self,
        input: &Path,
        output: &Path,
        options: &PremiereImportOptions,
        mode: ConversionMode,
        relink: &crate::ValidatedMediaRelink,
    ) -> Result<ConversionReport<Omission>, ConversionError> {
        self.import_with_media_relink_with_progress(
            input,
            output,
            options,
            mode,
            relink,
            Progress::default(),
        )
    }

    /// [`Self::import_with_media_relink`] with command-scoped progress.
    pub fn import_with_media_relink_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &PremiereImportOptions,
        mode: ConversionMode,
        relink: &crate::ValidatedMediaRelink,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<Omission>, ConversionError> {
        validate_paths(input, output)?;
        let mut import = TesseractImport::convert_with_media_relink(
            input,
            output,
            options.sequence.as_deref(),
            relink,
            progress,
        )?;
        let report = ConversionReport {
            diagnostics: std::mem::take(&mut import.omissions),
            artifacts: import.artifacts(),
        };
        if !mode.is_check() {
            import.write()?;
        }
        Ok(report)
    }

    /// Inspect the same explicit relocation used by import, without publishing.
    pub fn inspect_media_with_relink(
        &self,
        input: &Path,
        options: &PremiereImportOptions,
        relink: &crate::ValidatedMediaRelink,
    ) -> Result<fx_conv::MediaPreflight, ConversionError> {
        let input = input
            .canonicalize()
            .map_err(crate::error::BuildError::from)?;
        let targets = PrProjectFile::import_targets(&input)?;
        let target = match options.sequence.as_deref() {
            Some(id) if targets.iter().any(|target| target.id == id) => id,
            None if targets.len() == 1 => targets[0].id.as_str(),
            _ => {
                return Err(crate::error::unsupported(
                    "Premiere inspection must identify exactly one native sequence",
                )
                .into())
            }
        };
        Ok(
            crate::tesseract_output::inspect_native_premiere_media_with_relink(
                &input, target, relink,
            )?,
        )
    }
}

/// Converts and, unless `mode` only checks, publishes one sequence.
fn import(
    input: &Path,
    output: &Path,
    options: &PremiereImportOptions,
    mode: ConversionMode,
    progress: Progress<'_>,
) -> Result<ConversionReport<Omission>, ConversionError> {
    validate_paths(input, output)?;
    let mut import = TesseractImport::convert_with_progress(
        input,
        output,
        options.sequence.as_deref(),
        progress,
    )?;
    let report = ConversionReport {
        diagnostics: std::mem::take(&mut import.omissions),
        artifacts: import.artifacts(),
    };
    if !mode.is_check() {
        progress.stage("write and publish Tesseract project");
        import.write()?;
    }
    Ok(report)
}

impl ImportToTesseract for Premiere {
    type Options = PremiereImportOptions;
    type Diagnostic = Omission;
    type Error = ConversionError;

    fn list_import_targets(&self, input: &Path) -> Result<Vec<fx_conv::ImportTarget>, Self::Error> {
        if input.to_str().is_none() {
            return Err(crate::error::BuildError::Path("input").into());
        }
        Ok(PrProjectFile::import_targets(input)?
            .into_iter()
            .map(|target| fx_conv::ImportTarget {
                id: target.id,
                name: target.name,
                width: target.width,
                height: target.height,
                fps: target.fps,
                duration_secs: target.duration_secs,
                layer_count: None,
                video_track_count: target.video_track_count,
                audio_track_count: target.audio_track_count,
            })
            .collect())
    }

    fn import_to_tesseract(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        import(input, output, options, mode, Progress::default())
    }

    fn import_to_tesseract_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        import(input, output, options, mode, progress)
    }
}

impl ExportFromTesseract for Premiere {
    type Options = PremiereExportOptions;
    type Diagnostic = Omission;
    type Error = ConversionError;

    fn export_from_tesseract(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        self.export_from_tesseract_with_progress(input, output, options, mode, Progress::default())
    }

    fn export_from_tesseract_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        validate_paths(input, output)?;
        progress.stage("reading Tesseract project");
        Ok(premiere_package::save_tesseract_as_premiere_with_progress(
            input,
            output,
            options.frame_rate.unwrap_or(FrameRate::Fps30),
            mode.is_check(),
            progress,
        )?)
    }
}
