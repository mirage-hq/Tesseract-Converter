//! Format-independent reporting without discarding native diagnostic types.

use std::{fmt, path::PathBuf};

/// The effect of a successful conversion's diagnostic on editable content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// Source content was not converted.
    Omission,
    /// Content was converted with explicitly different semantics.
    Approximation,
    /// A caveat, or a legacy diagnostic that does not distinguish the two above.
    Warning,
}

/// A portable diagnostic for frontends that do not know a format's native types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable, format-qualified identifier, not inferred from message text.
    pub code: &'static str,
    /// Only as specific as the format's existing evidence allows.
    pub kind: DiagnosticKind,
    /// Source/FX target identity when known; not necessarily a filesystem path.
    pub context: Option<String>,
    /// Complete human-readable diagnostic, including its original context.
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Project native diagnostics into a common frontend view without parsing prose.
/// Libraries may retain richer typed diagnostics in their conversion reports.
pub trait ConversionDiagnostic: fmt::Display {
    /// Preserve the complete diagnostic text and all available target context.
    fn diagnostic(&self) -> Diagnostic;
}

impl ConversionDiagnostic for Diagnostic {
    fn diagnostic(&self) -> Diagnostic {
        self.clone()
    }
}

/// A file's role in the final package; directories and temporary files are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// The selected editable project/document. Imports produce one `project.tsrct`.
    Project,
    /// An external media dependency of a published project.
    Media,
}

/// One planned (`Check`) or successfully published (`Write`) file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Relative to the requested output directory; never absolute or parent-traversing.
    pub path: PathBuf,
    /// Whether the file is a document or its media dependency.
    pub kind: ArtifactKind,
}

impl Artifact {
    /// Report a document's final relative filename, not its staging path.
    pub fn project(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: ArtifactKind::Project,
        }
    }

    /// Report packaged media; media embedded inside an archive is not a separate file.
    pub fn media(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            kind: ArtifactKind::Media,
        }
    }
}

/// Successful validation or publication. Success does not establish fidelity.
///
/// Artifacts enumerate all final files, with unique relative paths. With unchanged
/// inputs/options, Check and Write return the same report: Check describes planned
/// files, Write is returned only after publication succeeds. Output roots and
/// temporary files are deliberately absent so reports can be compared directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionReport<D = Diagnostic> {
    /// Omissions, approximations and other caveats, retaining native context.
    pub diagnostics: Vec<D>,
    /// All project and external media files in the final package.
    pub artifacts: Vec<Artifact>,
}

impl<D: ConversionDiagnostic> ConversionReport<D> {
    /// Erase format-specific diagnostic types only at the presentation boundary.
    pub fn into_common(self) -> ConversionReport {
        ConversionReport {
            diagnostics: self.diagnostics.iter().map(|d| d.diagnostic()).collect(),
            artifacts: self.artifacts,
        }
    }
}
