//! Converts the supported AEP composition metadata into an editable FX document.

use fx_schema::{
    AnimationGraph, CompositionId, Dimensions, Duration, EditableFxCompositionDocument,
    EditableFxDocumentError, FXComposition, ValidationError,
};
use thiserror::Error;

use crate::reader::EmptyComposition;

/// Errors building a Tesseract FX document from a validated AEP composition.
#[derive(Debug, Error)]
pub enum DocumentError {
    /// Caller-provided composition metadata is invalid.
    #[error("invalid input: {field} {reason}")]
    InvalidInput {
        field: &'static str,
        reason: &'static str,
    },
    /// The canonical composition failed structural validation.
    #[error(transparent)]
    Composition(#[from] ValidationError),
    /// Imported animation graph failed canonical validation.
    #[error(transparent)]
    AnimationGraph(#[from] fx_schema::animator::AnimationGraphError),
    /// The resulting editable document failed validation.
    #[error(transparent)]
    Document(#[from] EditableFxDocumentError),
    /// Authored FX record failed canonical schema validation.
    #[error(transparent)]
    Record(#[from] serde_json::Error),
    /// Expression samples are not valid for the resolved import root.
    #[error(transparent)]
    ExpressionSamples(#[from] crate::expression_samples::ExpressionSamplesError),
    /// No composition was available as an import target.
    #[error("AEP contains no composition")]
    NoComposition,
    /// Omitted selection is ambiguous because the source has multiple compositions.
    #[error(
        "AEP contains {count} compositions; select one with --composition <ID> (run `tsrct-conv inspect <INPUT>` to list composition IDs)"
    )]
    AmbiguousCompositionSelection { count: usize },
    /// An explicit target does not identify a composition in this source.
    #[error("AEP composition item {0} was not found")]
    CompositionSelection(u32),
}

/// Builds the editable representation of a validated, empty AE composition.
pub fn to_fx_document(
    comp: &EmptyComposition,
) -> Result<EditableFxCompositionDocument, DocumentError> {
    let duration = Duration::from_secs(comp.duration_secs);
    // Preserve the former project/action validation boundary for direct callers,
    // even though the AEP reader has already checked supported input geometry.
    if comp.width == 0 || comp.height == 0 {
        return Err(DocumentError::InvalidInput {
            field: "video_metadata.display_dimensions",
            reason: "width and height must be non-zero",
        });
    }
    if duration.is_zero() {
        return Err(DocumentError::InvalidInput {
            field: "duration",
            reason: "must be greater than zero",
        });
    }
    if !comp.duration_secs.is_finite() || duration == Duration::MAX {
        return Err(DocumentError::InvalidInput {
            field: "duration",
            reason: "must be finite and representable",
        });
    }
    let fx = FXComposition::try_from_parts(
        CompositionId::new("main"),
        comp.name.clone(),
        AnimationGraph::new(),
        Vec::new(),
    )?;
    Ok(EditableFxCompositionDocument::new(
        Dimensions::new(u32::from(comp.width), u32::from(comp.height)),
        duration,
        None,
        fx,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{DocumentError, to_fx_document};
    use crate::reader::read_empty_composition;

    #[test]
    fn invalid_metadata_preserves_input_errors() {
        let source = include_bytes!("../tests/fixtures/ae26_one_comp.aep");
        let original = read_empty_composition(source).unwrap();
        for (width, height) in [(0, 1080), (1920, 0), (0, 0)] {
            let mut parsed = original.clone();
            parsed.width = width;
            parsed.height = height;
            parsed.duration_secs = 0.0;
            assert_eq!(
                to_fx_document(&parsed).unwrap_err().to_string(),
                "invalid input: video_metadata.display_dimensions width and height must be non-zero"
            );
        }
        for duration in [0.0, -1.0, f64::NAN, 0.0001] {
            let mut parsed = original.clone();
            parsed.duration_secs = duration;
            assert_eq!(
                to_fx_document(&parsed).unwrap_err().to_string(),
                "invalid input: duration must be greater than zero"
            );
        }
    }

    #[test]
    fn review_import_nonfinite_and_saturating_duration_is_rejected() {
        let source = include_bytes!("../tests/fixtures/ae26_one_comp.aep");
        let original = read_empty_composition(source).unwrap();
        for duration_secs in [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            (i64::MAX as f64) / 1_000.0,
        ] {
            let mut parsed = original.clone();
            parsed.duration_secs = duration_secs;
            assert!(matches!(
                to_fx_document(&parsed),
                Err(DocumentError::InvalidInput {
                    field: "duration",
                    ..
                })
            ));
        }
    }

    #[test]
    fn ae26_empty_comp_yields_editable_document() {
        let source = include_bytes!("../tests/fixtures/ae26_one_comp.aep");
        let parsed = read_empty_composition(source).unwrap();
        let document = to_fx_document(&parsed).unwrap();
        assert_eq!(document.dimensions().width, 1920);
        assert_eq!(document.dimensions().height, 1080);
        assert_eq!(document.duration().as_secs(), 1.0);
        assert_eq!(document.composition().id(), "main");
        assert_eq!(document.composition().name(), "classic-3d");
        assert!(document.composition().layers().is_empty());
        assert!(document.to_json_vec().is_ok());
    }
}
