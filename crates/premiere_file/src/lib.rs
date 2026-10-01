#![deny(unreachable_pub)]

//! Premiere/Tesseract conversion with validation before filesystem writes.
//!
//! Call [`premiere_to_tesseract`] or [`tesseract_to_premiere`].
//! The shared Premiere model is public. Conversion coordination, media checks,
//! and safe publication stay private.
//!
//! ```no_run
//! use premiere_file::{premiere_to_tesseract, tesseract_to_premiere};
//!
//! # fn main() -> Result<(), premiere_file::ConversionError> {
//! premiere_to_tesseract("project.prproj", "tesseract_output", None, false)?;
//! tesseract_to_premiere("tesseract_output/project.tsrct", "premiere_output", false)?;
//! # Ok(())
//! # }
//! ```

mod adapter;
mod audio_media;
mod convert;
mod error;
mod export_loss;
mod format;
mod hash;
mod image_media;
mod linked_compositions;
mod linked_import;
mod media;
mod media_metadata;
mod premiere_package;
mod publication;
mod schema;
mod tesseract_import;
mod tesseract_output;
mod video_format;

use error::BuildError;
use std::path::Path;

pub use adapter::{Premiere, PremiereExportOptions, PremiereImportOptions};
pub use convert::{
    AfterEffectsPicture, PictureContainer, PictureContainerToken, PicturePackingId,
    PicturePackingRecipe, PictureReplacement, PictureSourceBoundary, SourceBoundaryToken,
};
pub use export_loss::{
    ExportField, ExportLoss, ExportLossDomain, ExportLossKind, ExportLossReport, ExportLossSource,
};
pub use format::{
    FrameRate, MediaId, PrGraphic, PrMedia, PrProjectFile, PrSequence, PrVideoItem,
    PrVideoOccurrence,
};
pub use linked_import::{LinkedComposition, LinkedCompositionResolver};
pub use premiere_package::{
    prepared::{PreparedPremiereExport, StagedNativePremiereExport, StagedPicturePremiereExport},
    staging::{AfterEffectsOverlay, StagedPremiereExport},
};
pub use schema::PrAfterEffectsComposition;

/// The source content that a partial Premiere conversion did not include, or
/// converted only approximately ([`OmissionKind`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Omission {
    pub scope: OmissionScope,
    pub kind: OmissionKind,
    pub record: String,
    pub reason: String,
}

/// What a conversion did with a reported source unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OmissionKind {
    /// Not converted. Also every other report that is not an approximation,
    /// such as a caveat about converted content.
    Omitted,
    /// Converted with the nearest available form, not exactly.
    Approximated,
}

/// The size of a reported source unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OmissionScope {
    Feature,
    Occurrence,
    Track,
    Sequence,
}

impl std::fmt::Display for OmissionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Feature => "feature",
            Self::Occurrence => "occurrence",
            Self::Track => "track",
            Self::Sequence => "sequence",
        })
    }
}

impl std::fmt::Display for Omission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.scope, self.record, self.reason)
    }
}

/// Reports an omission once, in first-report order.
pub(crate) fn omit(
    omissions: &mut dyn export_loss::OmissionSink,
    scope: OmissionScope,
    record: impl Into<String>,
    reason: impl Into<String>,
) {
    omissions.emit(Omission {
        scope,
        kind: OmissionKind::Omitted,
        record: record.into(),
        reason: reason.into(),
    });
}

/// Reports, as [`omit`] does, one feature of `record` that converted
/// approximately: `reason` names the parameter, the approximation and its
/// expected error, one report per approximated parameter.
pub(crate) fn approximate(
    omissions: &mut dyn export_loss::OmissionSink,
    record: impl Into<String>,
    reason: impl Into<String>,
) {
    omissions.emit(Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Approximated,
        record: record.into(),
        reason: reason.into(),
    });
}

/// Deduplicate identical reports while retaining all distinct diagnostics.
fn push_omission(omissions: &mut Vec<Omission>, omission: Omission) {
    if !omissions.contains(&omission) {
        omissions.push(omission);
    }
}

/// Failure to validate or publish a conversion, with its underlying error preserved.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ConversionError(#[from] BuildError);

/// Convert one Premiere sequence into `project.tsrct` in a new directory.
///
/// `sequence` selects an exact GUID. It may be omitted only when the project has
/// exactly one sequence. `check` validates without writing files.
///
/// # Errors
/// Returns an error if sequence selection is missing or invalid, the selected
/// timeline cannot be converted, or publication fails.
pub fn premiere_to_tesseract(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    sequence: Option<&str>,
    check: bool,
) -> Result<Vec<Omission>, ConversionError> {
    use fx_conv::ImportToTesseract;

    let options = PremiereImportOptions {
        sequence: sequence.map(str::to_owned),
    };
    Ok(Premiere
        .import_to_tesseract(input.as_ref(), output.as_ref(), &options, mode(check))?
        .diagnostics)
}

/// Save a Tesseract document as a new Premiere project and media package with
/// a 30 fps sequence.
///
/// `check` validates without writing files.
///
/// # Errors
/// Returns an error if no usable video remains, an input is invalid, or publication fails.
/// Feature omissions and approximations are returned as diagnostics instead.
pub fn tesseract_to_premiere(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    check: bool,
) -> Result<Vec<Omission>, ConversionError> {
    use fx_conv::ExportFromTesseract;

    Ok(Premiere
        .export_from_tesseract(
            input.as_ref(),
            output.as_ref(),
            &PremiereExportOptions::default(),
            mode(check),
        )?
        .diagnostics)
}

fn mode(check: bool) -> fx_conv::ConversionMode {
    if check {
        fx_conv::ConversionMode::Check
    } else {
        fx_conv::ConversionMode::Write
    }
}

fn validate_paths(input: &Path, output: &Path) -> Result<(), ConversionError> {
    for (label, path) in [("input", input), ("output", output)] {
        if path.to_str().is_none() {
            return Err(BuildError::Path(label).into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod test_support;
#[cfg(test)]
mod tests;
