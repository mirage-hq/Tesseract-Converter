//! Stable limitation identifiers shared by best-effort import and its support notes.

use std::fmt;

/// A fidelity limitation, not a failure to produce an editable document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limitation {
    /// A producer version outside the primary AE26 evidence.
    Version,
    /// A default target was chosen among multiple compositions.
    Selection,
    /// Project-only metadata is not represented by FX.
    ProjectMetadata,
    /// Composition settings do not all have a document equivalent.
    CompositionSettings,
    /// Group bounds differ from a fixed composition canvas.
    GroupBounds,
    /// Shared source compositions become independently editable occurrences.
    IndependentCopies,
    /// The layer retains its structural place but not its rendered content.
    Placeholder,
    /// Property payloads, including transforms, are not evaluated by this importer yet.
    Properties,
    /// AE transform parenting is not FX containment.
    Parenting,
    /// Matte references are read but not applied to structural placeholders.
    TrackMatte,
    /// Transfer modes are read but not applied to structural placeholders.
    BlendMode,
    /// Some layer switches have no equivalent in the structural import.
    LayerSwitches,
    /// Editor metadata is retained only as source information.
    LayerMetadata,
    /// Source time cannot be represented exactly in the FX millisecond clock.
    Timing,
    /// A referenced source, parent, or matte is absent.
    MissingReference,
    /// A cyclic source branch cannot be expanded.
    Cycle,
    /// Expansion was bounded to keep the imported document safe to consume.
    ExpansionLimit,
    /// An unknown item kind was retained by the loader but cannot supply pixels.
    UnknownItem,
}

impl Limitation {
    /// Stable key in `docs/after-effects-support.md`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Version => "AE-VERSION",
            Self::Selection => "AE-SELECTION",
            Self::ProjectMetadata => "AE-PROJECT-METADATA",
            Self::CompositionSettings => "AE-COMPOSITION-SETTINGS",
            Self::GroupBounds => "AE-GROUP-BOUNDS",
            Self::IndependentCopies => "AE-INDEPENDENT-COPIES",
            Self::Placeholder => "AE-PLACEHOLDER",
            Self::Properties => "AE-PROPERTIES",
            Self::Parenting => "AE-PARENTING",
            Self::TrackMatte => "AE-TRACK-MATTE",
            Self::BlendMode => "AE-BLEND-MODE",
            Self::LayerSwitches => "AE-LAYER-SWITCHES",
            Self::LayerMetadata => "AE-LAYER-METADATA",
            Self::Timing => "AE-TIMING",
            Self::MissingReference => "AE-MISSING-REFERENCE",
            Self::Cycle => "AE-CYCLE",
            Self::ExpansionLimit => "AE-EXPANSION-LIMIT",
            Self::UnknownItem => "AE-UNKNOWN-ITEM",
        }
    }
}

/// Identifies the source context and the exact approximation made by import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDiagnostic {
    /// Key in the committed support/approximation notes.
    pub limitation: Limitation,
    /// Source project item ID of the affected composition, when applicable.
    pub composition_id: Option<u32>,
    /// Source layer ID, scoped to that composition, when applicable.
    pub layer_id: Option<u32>,
    /// Human-readable source property and replacement behavior.
    pub message: String,
}

impl fmt::Display for ImportDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "[{}]", self.limitation.code())?;
        if let Some(id) = self.composition_id {
            write!(formatter, " composition {id}")?;
        }
        if let Some(id) = self.layer_id {
            write!(formatter, " layer {id}")?;
        }
        write!(formatter, ": {}", self.message)
    }
}
