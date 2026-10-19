//! Inverse bindings on freshly assembled importer-owned FX, not persisted input.
//!
//! Only admitted colour fields and tagged animator values are visited. Geometry,
//! opaque metadata, unknown effects and script bodies are never array-rewritten.

use fx_conv::GeneratedHdrColorBinding;
use serde_json::Value;

use crate::{
    diagnostic::{ImportDiagnostic, Limitation},
    document::DocumentError,
    structure::ProjectColorContext,
};

pub(super) fn bind(
    document: fx_schema::EditableFxCompositionDocument,
    context: &ProjectColorContext,
    composition: u32,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<fx_schema::EditableFxCompositionDocument, DocumentError> {
    let transfer = match context {
        ProjectColorContext::Legacy => return Ok(document),
        ProjectColorContext::Unsupported { profile, reason } => {
            warn(
                diagnostics,
                composition,
                format!(
                    "working profile {profile:?}: {reason}; convertible geometry/siblings retained with legacy numeric colour approximation, not a colour-fidelity pass"
                ),
            );
            return Ok(document);
        }
        ProjectColorContext::Hdr {
            transfer,
            linear_blending,
        } => {
            warn(
                diagnostics,
                composition,
                format!(
                    "exact owned {transfer:?} ICC admitted for point-colour decoding; native graphics-white intent is not stored in a proved AE control, so light is normalized to FX 609 nits. Native linear blending={linear_blending}; gradients, interpolation, effects/blends and source-footage interpretation remain unproved"
                ),
            );
            *transfer
        }
    };
    let mut value = document.to_json_value()?;
    let mut messages = Vec::new();
    let mut binding = GeneratedHdrColorBinding::new(transfer, &mut messages);
    if let Some(layers) = value["composition"]["layers"].as_array_mut() {
        for layer in layers {
            binding.layer(layer);
        }
    }
    if let Some(entries) = value["composition"]["dynamics"]["entries"].as_array_mut() {
        for entry in entries {
            let scope = format!("animator target {}", entry["target"]);
            binding.entry(entry, &scope);
        }
    }
    for message in messages {
        warn(diagnostics, composition, message);
    }
    value["hdrGraphicsWhiteNits"] = Value::from(609.0);
    Ok(fx_schema::EditableFxCompositionDocument::from_json_value(
        value,
    )?)
}

fn warn(diagnostics: &mut Vec<ImportDiagnostic>, composition: u32, message: String) {
    diagnostics.push(ImportDiagnostic {
        limitation: Limitation::ColorManagement,
        composition_id: Some(composition),
        // FX identities below are explicitly named in messages, not mislabeled
        // as source-native layer IDs after expansion/identity allocation.
        layer_id: None,
        message,
    });
}
