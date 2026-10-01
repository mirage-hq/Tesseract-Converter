//! Observe actual export losses without choosing a replacement format or publishing.

use fx_schema::EditableFxCompositionDocument;
use tesseract_file::TesseractFile;

use crate::{ConversionError, ExportLossReport, Premiere, PremiereExportOptions};

impl Premiere {
    /// Runs ordinary bounded JS-key preparation, source inspection and native
    /// lowering with typed loss provenance, without staging or publishing a project.
    ///
    /// The supplied document may be a caller-owned view of the archive. Only its
    /// referenced native media is inspected, using the same verification as export.
    /// Empty native content retains its loss report instead of becoming the
    /// ordinary export's no-convertible-content error. Other failures propagate.
    ///
    /// This is not a complete capability assessment: unclassified observations,
    /// unreported source semantics, and native writer/Adobe acceptance still need
    /// separate analysis. Neither an empty loss list nor `has_native_content`
    /// establishes lossless conversion or a safe boundary for AE extraction.
    pub fn inspect_export_losses(
        &self,
        archive: &TesseractFile,
        document: &EditableFxCompositionDocument,
        options: &PremiereExportOptions,
    ) -> Result<ExportLossReport, ConversionError> {
        Ok(self
            .prepare_export(archive, document, options)?
            .into_losses())
    }
}

#[cfg(test)]
mod tests;
