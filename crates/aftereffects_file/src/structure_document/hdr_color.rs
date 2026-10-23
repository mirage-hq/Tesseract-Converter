//! Inverse bindings on freshly assembled importer-owned FX, not persisted input.
//!
//! Only admitted colour fields and tagged animator values are visited. Geometry,
//! opaque metadata, unknown effects and script bodies are never array-rewritten.

use fx_conv::GeneratedHdrColorBinding;
use serde_json::Value;

use crate::{
    diagnostic::{ImportDiagnostic, Limitation},
    document::DocumentError,
    structure::{HdrProfileBinding, ProjectColorContext},
};

pub(super) fn bind(
    document: fx_schema::EditableFxCompositionDocument,
    context: &ProjectColorContext,
    composition: u32,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<fx_schema::EditableFxCompositionDocument, DocumentError> {
    if let ProjectColorContext::Aces { config } = context {
        let mut document = document;
        document.set_color_management(fx_schema::ColorManagement::new_aces(
            fx_schema::OutputTransform::Aces2_0,
            // AE composites every layer, solids, text and effects included,
            // in the ACEScg working space: FX wholeProgram.
            fx_schema::Compositing::WholeProgram,
        ));
        warn(
            diagnostics,
            composition,
            format!(
                "OCIO colour management with the built-in {config:?} config and the ACEScg working colour space imported as FX colorManagement acescct / aces2.0 / wholeProgram (AE composites every layer in ACEScg); footage OCIO camera input colourspaces become cameraInput Input Transforms. AE's display and output-module colour spaces are not converted: FX renders the ACES 2.0 SDR Rec.709 (BT.1886) output, or HDR with `tsrct export --hdr`; AE's own ACES 2.0 SDR output of the in-house fixture differed from the FX render of its import by 2.7 8-bit codes (mean absolute difference, chart), above the 2-code render gate, measured with the Lumetri Exposure stops/2.2 scale that OCIO ACES projects keep; log footage is unmeasured because the tested FX renderer did not apply cameraInput"
            ),
        );
        return Ok(document);
    }
    let document = strip_camera_inputs(document, composition, diagnostics)?;
    let (transfer, white) = match context {
        ProjectColorContext::Aces { .. } | ProjectColorContext::Legacy => return Ok(document),
        ProjectColorContext::LegacyDeclared { declaration } => {
            warn(
                diagnostics,
                composition,
                format!(
                    "working space admitted as legacy: {declaration}; point colours are imported as unchanged display-referred numeric values, the same interpretation as a project without colour management"
                ),
            );
            return Ok(document);
        }
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
            binding,
        } => {
            let white = binding.graphics_white_nits();
            let admitted = match binding {
                HdrProfileBinding::OwnedIcc => format!(
                    "exact owned {transfer:?} ICC admitted for point-colour decoding; native graphics-white intent is not stored in a proved AE control, so light is normalized to FX {white} nits"
                ),
                HdrProfileBinding::AdobeRec2100Pq => format!(
                    "AE built-in \"Rec.2100 PQ\" working profile admitted by its exact AE 26.5 ICC payload; points decode as BT.2020 PQ code values (absolute luminance preserved) with FX graphics white {white} nits, the reference luminance the profile's ICC lumi tag declares (AE stores no separate graphics-white control)"
                ),
            };
            warn(
                diagnostics,
                composition,
                format!(
                    "{admitted}. Native linear blending={linear_blending}; gradients, interpolation, effects/blends and source-footage interpretation remain unproved"
                ),
            );
            (*transfer, white)
        }
    };
    let mut value = document.to_json_value()?;
    let mut messages = Vec::new();
    let mut binding = GeneratedHdrColorBinding::with_graphics_white(transfer, white, &mut messages)
        .map_err(|_| DocumentError::InvalidInput {
            field: "hdrGraphicsWhiteNits",
            reason: "profile binding white is outside the FX graphics-white range",
        })?
        // AE's float (32 bpc) working space keeps code values above 1.0.
        .with_super_white_headroom();
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
    value["hdrGraphicsWhiteNits"] = Value::from(white);
    Ok(fx_schema::EditableFxCompositionDocument::from_json_value(
        value,
    )?)
}

/// A document that is not an FX ACES document cannot hold the `cameraInput`
/// that an OCIO footage camera colourspace imported as (an OCIO project
/// outside the admitted ACES 2.0 configs); each is removed and reported.
fn strip_camera_inputs(
    document: fx_schema::EditableFxCompositionDocument,
    composition: u32,
    diagnostics: &mut Vec<ImportDiagnostic>,
) -> Result<fx_schema::EditableFxCompositionDocument, DocumentError> {
    fn visit(layers: &mut Value, removed: &mut Vec<String>) {
        for layer in layers.as_array_mut().into_iter().flatten() {
            if let Some(source) = layer.get_mut("source").and_then(Value::as_object_mut)
                && source
                    .get("inputTransform")
                    .and_then(|transform| transform.pointer("/transform/type"))
                    == Some(&Value::from("cameraInput"))
                && let Some(transform) = source.remove("inputTransform")
            {
                removed.push(format!(
                    "{} ({})",
                    layer["name"].as_str().unwrap_or("layer"),
                    transform["transform"]["inputEncoding"]
                ));
            }
            visit(&mut layer["layers"], removed);
        }
    }
    let mut value = document.to_json_value()?;
    let mut removed = Vec::new();
    visit(&mut value["composition"]["layers"], &mut removed);
    if removed.is_empty() {
        return Ok(document);
    }
    warn(
        diagnostics,
        composition,
        format!(
            "OCIO footage camera input colourspaces not converted on {}: cameraInput is valid only in an FX ACES document, which needs a built-in ACES 2.0 config with the ACEScg working colour space; footage decoded with FX's default display interpretation",
            removed.join(", ")
        ),
    );
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
