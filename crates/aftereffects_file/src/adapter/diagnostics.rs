//! Portable metadata for existing diagnostics; never classify by message text.

use fx_conv::{ConversionDiagnostic, Diagnostic, DiagnosticKind};

use crate::{ImportDiagnostic, Limitation, export_document::ExportDiagnostic};

impl ConversionDiagnostic for ImportDiagnostic {
    fn diagnostic(&self) -> Diagnostic {
        let context = match (self.composition_id, self.layer_id) {
            (Some(composition), Some(layer)) => {
                Some(format!("composition {composition} layer {layer}"))
            }
            (Some(composition), None) => Some(format!("composition {composition}")),
            (None, Some(layer)) => Some(format!("layer {layer}")),
            (None, None) => None,
        };
        Diagnostic {
            code: self.limitation.code(),
            // Most legacy limitation codes cover both omissions and fallbacks.
            // Do not claim a more specific effect than their typed data proves.
            kind: match self.limitation {
                Limitation::IndependentCopies => DiagnosticKind::Approximation,
                _ => DiagnosticKind::Warning,
            },
            context,
            message: self.to_string(),
        }
    }
}

impl ConversionDiagnostic for ExportDiagnostic {
    fn diagnostic(&self) -> Diagnostic {
        Diagnostic {
            code: "AE-EXPORT",
            // The existing exporter mixes omissions and normalizations under one
            // code. Preserve that uncertainty rather than inspecting prose.
            kind: DiagnosticKind::Warning,
            context: self.layer_id.map(|id| format!("FX layer {id}")),
            message: self.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_metadata_preserves_native_identity_and_text() {
        let native = ImportDiagnostic {
            limitation: Limitation::IndependentCopies,
            composition_id: Some(42),
            layer_id: Some(7),
            message: "shared composition expanded into an independent editable group".into(),
        };
        let common = native.diagnostic();
        assert_eq!(common.code, "AE-INDEPENDENT-COPIES");
        assert_eq!(common.kind, DiagnosticKind::Approximation);
        assert_eq!(common.context.as_deref(), Some("composition 42 layer 7"));
        assert_eq!(common.to_string(), native.to_string());
    }

    #[test]
    fn timing_code_covers_both_approximations_and_omissions() {
        for message in ["source timing rounded", "source time mapping omitted"] {
            let native = ImportDiagnostic {
                limitation: Limitation::Timing,
                composition_id: None,
                layer_id: None,
                message: message.into(),
            };
            assert_eq!(native.diagnostic().kind, DiagnosticKind::Warning);
        }
    }

    #[test]
    fn legacy_export_prose_does_not_determine_classification() {
        for message in ["omitted", "approximated", "normalized"] {
            let native = ExportDiagnostic {
                layer_id: None,
                message: message.into(),
            };
            let common = native.diagnostic();
            assert_eq!(common.code, "AE-EXPORT");
            assert_eq!(common.kind, DiagnosticKind::Warning);
            assert_eq!(common.context, None);
            assert_eq!(common.to_string(), native.to_string());
        }
    }
}
