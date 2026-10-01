//! Format-owned, target-scoped admission results. No decoder process is started here.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Media referenced by one native target, before unsupported layers are omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaPreflight {
    /// Stable converter format identifier (`after-effects` or `premiere`).
    pub format: String,
    /// Exact native composition/sequence identity.
    pub target: String,
    /// Reached video/audio sources, including disabled and off-range references.
    pub media: Vec<InspectedMedia>,
    /// Unresolved native references which prevent a complete readiness claim.
    pub unassessed: Vec<String>,
}

impl MediaPreflight {
    /// Admission only, not decoding, render fidelity, or effect support.
    pub fn is_ready(&self) -> bool {
        self.unassessed.is_empty()
            && self
                .media
                .iter()
                .all(|media| media.status == MediaStatus::Supported)
    }
}

/// One original file may have several native identities or placements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectedMedia {
    /// Owning native project; linked projects keep their own identity.
    pub owner: PathBuf,
    /// Native source identifier, scoped by `owner`.
    pub id: String,
    /// Human-readable source label.
    pub name: String,
    /// Native media classification, not inferred from the filename.
    pub kind: MediaKind,
    /// Path stored in the native source, when available.
    pub authored: Option<PathBuf>,
    /// Original selected by the importer-native relocation rules.
    pub original: Option<PathBuf>,
    /// Actually inspected path, after any explicit media-map replacement.
    pub selected: Option<PathBuf>,
    /// Native target/layer/clip references, for diagnosis.
    pub references: Vec<String>,
    /// Result of the format's ordinary admission check.
    pub status: MediaStatus,
    /// Container or extension when known; not a decoder-availability guarantee.
    pub container: Option<String>,
    /// Codec sample entry or codec name, when the reader can identify it.
    pub codec: Option<String>,
    /// Actionable admission failure or limitation.
    pub reason: Option<String>,
    /// A candidate still requires an explicit backend capability/preservation check.
    pub remediation: MediaRemediation,
}

/// Only time-based source media participates in this preparation workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// Video, possibly with embedded audio.
    Video,
    /// Standalone audio.
    Audio,
}

/// Inspection deliberately distinguishes bad bytes, missing files and unsupported content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaStatus {
    /// Passed the importer admission policy, not full decoding.
    Supported,
    /// Requires explicit compatible preparation.
    RequiresTranscode,
    /// The native resolver found no local source.
    Missing,
    /// An operational I/O error prevented assessment.
    Unreadable,
    /// A recognized media container is malformed.
    InvalidMedia,
    /// Source semantics or reachability could not be determined.
    Unassessed,
}

/// Inspection must not promise that FFmpeg can render Flash timelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRemediation {
    /// No preparation is required.
    None,
    /// A backend may remux or encode this file after preservation checks.
    TranscodeCandidate,
    /// Vector/script content requires an independent renderer.
    ExternalRenderRequired,
    /// More source or capability information is needed.
    Unknown,
}
