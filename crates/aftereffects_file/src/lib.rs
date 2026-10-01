//! Native After Effects project file support.
//!
//! The RIFX layer preserves unknown chunks. Layer structure imports as editable
//! Groups with explicit best-effort diagnostics; render fidelity is not claimed.
//! Experimental Tesseract-to-AEP export supports selected editable native layers,
//! animation, and packaged media with explicit omissions. Independent Adobe
//! acceptance and render fidelity remain unverified; see the support ledger.

mod adapter;
pub mod aep;
mod alias;
pub mod diagnostic;
pub mod document;
mod effects;
mod essential;
mod export_document;
mod export_identity;
pub mod expression_samples;
mod layer_styles;
mod media;
pub mod properties;
pub mod reader;
pub mod rifx;
pub mod schema;
pub mod structure;
pub mod structure_document;
mod timing;
mod vector_media;
pub mod writer;

#[cfg(test)]
mod adobe_test_support;
#[cfg(test)]
mod test_fixtures;

pub use adapter::{
    AepConversionError, AfterEffects, AfterEffectsExportOptions, AfterEffectsImportOptions,
    DynamicLinkImportError, ImportedAfterEffectsComposition, LinkedMedia, LinkedPicture,
    LinkedPictureTarget, PreparedAfterEffectsImport, ResolvedAfterEffectsComposition,
    StagedAfterEffectsExport, StagedAfterEffectsPictureExport,
};
pub use diagnostic::{ImportDiagnostic, Limitation};
pub use export_document::ExportDiagnostic;
pub use export_identity::GeneratedRootComposition;
pub use expression_samples::{
    CaptureScope, EvaluatedProperty, ExpressionEvaluationError, ExpressionSamples,
    ExpressionSamplesError, PropertyIdentity,
};
