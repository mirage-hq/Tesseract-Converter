//! Check and Write share lowering/encoding; only Write publishes bytes.

use fx_conv::{ConversionMode, ConversionReport, ExportFromTesseract, Progress};
use tesseract_file::TesseractFile;

use super::{
    AepConversionError, AfterEffects, AfterEffectsExportOptions, Path, fresh_destination, fs,
};
use crate::export_document::ExportDiagnostic;

mod package;
mod script_bake;
mod staging;

pub use staging::{
    AepPreparationControl, StagedAfterEffectsExport, StagedAfterEffectsPictureExport,
};

impl ExportFromTesseract for AfterEffects {
    type Options = AfterEffectsExportOptions;
    type Diagnostic = ExportDiagnostic;
    type Error = AepConversionError;

    fn export_from_tesseract(
        &self,
        input: &Path,
        output: &Path,
        options: &AfterEffectsExportOptions,
        mode: ConversionMode,
    ) -> Result<ConversionReport<ExportDiagnostic>, AepConversionError> {
        self.export_from_tesseract_with_progress(input, output, options, mode, Progress::default())
    }

    fn export_from_tesseract_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &AfterEffectsExportOptions,
        mode: ConversionMode,
        progress: Progress<'_>,
    ) -> Result<ConversionReport<ExportDiagnostic>, AepConversionError> {
        let destination = fresh_destination(output)?;
        progress.stage("read Tesseract");
        let metadata = fs::metadata(input)
            .map_err(|source| AepConversionError::io("inspect Tesseract input", input, source))?;
        if !metadata.is_file() {
            return Err(AepConversionError::Input(
                "expected a regular Tesseract file",
            ));
        }
        let archive = TesseractFile::open(input)?;
        let mut staging = tempfile::Builder::new();
        staging.prefix(".conversion-aftereffects-");
        let staged = if mode.is_check() {
            staging.tempdir().map_err(|source| {
                AepConversionError::io("create AEP export staging", input, source)
            })?
        } else {
            let parent = destination
                .parent()
                .ok_or(AepConversionError::Output("output parent is missing"))?;
            staging.tempdir_in(parent).map_err(|source| {
                AepConversionError::io("create AEP export staging", parent, source)
            })?
        };
        let staged = staging::prepare_with_progress(
            &archive,
            archive.project(),
            staged,
            options,
            (!mode.is_check()).then_some(destination.as_path()),
            progress,
        )?;
        if !mode.is_check() {
            progress.stage("publish AEP");
            package::publish_package(
                staged.directory(),
                &destination,
                &staged.publication_files,
                &staged.font_files,
            )?;
        }
        Ok(staged.report)
    }
}

impl AfterEffects {
    /// Compatibility spelling for the typed [`ExportFromTesseract`] entry point.
    /// Layer and keyframe positions remain second-based, not frame-snapped.
    pub fn export_from_tesseract_with_options(
        &self,
        input: &Path,
        output: &Path,
        options: &AfterEffectsExportOptions,
        mode: ConversionMode,
    ) -> Result<ConversionReport<ExportDiagnostic>, AepConversionError> {
        self.export_from_tesseract(input, output, options, mode)
    }
}

#[cfg(test)]
mod tests;
