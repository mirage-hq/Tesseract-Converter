//! Shared boundaries for converting editable FX documents to and from Tesseract.
//!
//! Format-specific parsing, media handling, and publication remain in each
//! format crate. A format can support import, export, or both independently.

mod media;
mod preflight;
mod progress;
mod report;

pub use progress::{ConversionProgress, Progress, ProgressPhase};
mod swf;

pub use swf::{classify_swf, SwfClassification};

pub use preflight::{InspectedMedia, MediaKind, MediaPreflight, MediaRemediation, MediaStatus};

pub use media::{
    sha256_file, MediaMap, MediaMapError, MediaMapSource, MediaReplacement, ValidatedMediaMap,
};

pub use report::{
    Artifact, ArtifactKind, ConversionDiagnostic, ConversionReport, Diagnostic, DiagnosticKind,
};

use std::{error::Error, path::Path};

/// One selectable scene in a native source, including nested compositions/sequences.
/// Names are display-only: callers must select by the source's stable ID.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportTarget {
    /// Format-native identity (for example a Premiere GUID or decimal AE item ID).
    pub id: String,
    /// Native display name; it need not be unique.
    pub name: String,
    /// Native canvas width in pixels, when available.
    pub width: Option<u32>,
    /// Native canvas height in pixels, when available.
    pub height: Option<u32>,
    /// Native frame rate, without resampling.
    pub fps: Option<f64>,
    /// Native scene duration in seconds, when available.
    pub duration_secs: Option<f64>,
    /// Direct layers in formats with a layer model; excludes nested contents.
    pub layer_count: Option<usize>,
    /// Direct video tracks in timeline formats; excludes nested contents.
    pub video_track_count: Option<usize>,
    /// Direct audio tracks in timeline formats; excludes nested contents.
    pub audio_track_count: Option<usize>,
}

/// Whether to validate without final publication or publish the validated conversion.
/// Check may use temporary staging (cleaned on return); it never creates the final
/// output directory. It cannot promise that a later write will succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionMode {
    Check,
    Write,
}

impl ConversionMode {
    /// Whether publication must be skipped after validation.
    pub const fn is_check(self) -> bool {
        matches!(self, Self::Check)
    }
}

/// Import one selected scene into `project.tsrct` in a fresh directory.
///
/// Selection may be omitted only when the source has exactly one import target.
/// Multiple targets require an explicit ID, including when only one is a root.
/// Dependencies of the selected scene still convert; unrelated scenes do not.
///
/// Both modes validate the same conversion, including media and destination
/// freshness. The output parent must exist; an existing destination (including a
/// dangling symlink) must never be replaced. Ordinary failures clean up owned
/// partial output without removing foreign files. Publication is not necessarily
/// crash-atomic: only a successful Write return establishes a complete package.
/// Unsupported content is diagnosed rather than silently discarded; malformed
/// input and I/O failures remain typed errors. Implementations own staging and
/// media handling, not the caller.
pub trait ImportToTesseract {
    /// Format-specific selection and validation options.
    type Options;
    /// Typed format diagnostics with a portable frontend projection.
    type Diagnostic: ConversionDiagnostic;
    /// Preserves the underlying format or publication failure.
    type Error: Error + 'static;

    /// List all selectable scenes from source metadata without probing media,
    /// converting content, creating staging files, or publishing output.
    /// Listing does not establish that a target's content can be converted.
    fn list_import_targets(&self, input: &Path) -> Result<Vec<ImportTarget>, Self::Error>;

    /// Validate the selected scene and publish unless `mode` is `Check`.
    fn import_to_tesseract(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error>;

    /// Import with optional phase-local observations. Implementations without
    /// measurements retain their existing behavior through this default.
    fn import_to_tesseract_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
        _progress: Progress<'_>,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        self.import_to_tesseract(input, output, options, mode)
    }
}

/// Export a Tesseract document into a fresh directory in the target format.
///
/// The validation, freshness, diagnostics and publication contract is the same
/// as [`ImportToTesseract`]. Import and export are independent capabilities.
pub trait ExportFromTesseract {
    /// Format-specific export options.
    type Options;
    /// Typed format diagnostics with a portable frontend projection.
    type Diagnostic: ConversionDiagnostic;
    /// Preserves the underlying format or publication failure.
    type Error: Error + 'static;

    /// Validate the conversion, and publish unless `mode` is `Check`.
    fn export_from_tesseract(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error>;

    /// Export with optional phase-local observations. Measurements do not imply
    /// publication success; only a successful return establishes that result.
    fn export_from_tesseract_with_progress(
        &self,
        input: &Path,
        output: &Path,
        options: &Self::Options,
        mode: ConversionMode,
        _progress: Progress<'_>,
    ) -> Result<ConversionReport<Self::Diagnostic>, Self::Error> {
        self.export_from_tesseract(input, output, options, mode)
    }
}
