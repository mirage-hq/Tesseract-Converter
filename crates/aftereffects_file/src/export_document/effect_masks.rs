//! Resolve exported effects' FX mask references to native mask identities.
//!
//! An FX effect's `compositingOptions.maskReferences` name same-layer FX
//! masks. Native AE effect Compositing Options name a layer mask by its native
//! identity (`writer::masks::native_identity`). Effects and masks are lowered
//! separately and several lowerings move effects or masks between native
//! layers, so references are resolved once against the final emitted layers:
//! a reference resolves only when exactly one native mask on the same native
//! layer was lowered from that FX mask. Every other reference is diagnosed and
//! that restriction omitted.
//!
//! After Effects 26.5 (aerender, 8 bpc, measured on an in-house ramp) renders
//! an effect mask reference to a mask in mode None as no restriction (the
//! effect covers the whole layer), and restricts the effect to a referenced
//! mask in mode Add without applying that mask to the layer. FX restricts the
//! effect to a referenced mask in mode `none` and leaves the layer whole, so a
//! referenced FX `none` mask is written in mode Add; a referenced mask in
//! another mode keeps it, diagnosed (AE does not cut the layer with a
//! referenced Add mask; other modes are unmeasured).

use fx_schema::LayerId;

use super::ExportDiagnostic;
use crate::writer::{LayerSpec, NativeLayerOptions, NativeMaskMode, native_mask_identity};

pub(super) fn bind(layers: &mut [LayerSpec], diagnostics: &mut Vec<ExportDiagnostic>) {
    for layer in layers {
        bind_layer(layer, diagnostics);
    }
}

fn bind_layer(layer: &mut LayerSpec, diagnostics: &mut Vec<ExportDiagnostic>) {
    match layer {
        LayerSpec::Options(inner, options) => {
            resolve(options, diagnostics);
            bind_layer(inner, diagnostics);
        }
        LayerSpec::Timed(inner, _) => bind_layer(inner, diagnostics),
        LayerSpec::Precomposition(value) => bind(&mut value.layers, diagnostics),
        _ => {}
    }
}

fn resolve(options: &mut NativeLayerOptions, diagnostics: &mut Vec<ExportDiagnostic>) {
    let owner: LayerId = options.fx_id;
    for effect in &mut options.effects {
        let references = std::mem::take(&mut effect.compositing.unresolved_mask_references);
        if references.is_empty() {
            continue;
        }
        let mut identities = Vec::new();
        for reference in &references {
            let positions: Vec<_> = options
                .masks
                .iter()
                .enumerate()
                .filter(|(_, mask)| mask.source_mask == Some(*reference))
                .map(|(position, _)| position)
                .collect();
            if let [position] = positions.as_slice() {
                let mask = &mut options.masks[*position];
                match mask.mode {
                    NativeMaskMode::None => mask.mode = NativeMaskMode::Add,
                    mode => diagnostics.push(ExportDiagnostic {
                        layer_id: Some(owner),
                        message: format!(
                            "Effect {} ({}): compositingOptions references FX mask {reference} in mode {mode:?}; After Effects does not apply an effect-referenced Add mask to the layer (measured) and the other modes are unmeasured, so the layer's own masking may differ",
                            effect.name, effect.match_name
                        ),
                    }),
                }
            }
            let identity = match positions.as_slice() {
                [position] => native_mask_identity(*position)
                    .ok_or("its native mask index exceeds the native range"),
                [] => Err("it is not exported as a native mask on the effect's native layer"),
                _ => Err("it is exported as several native masks"),
            };
            match identity {
                Ok(identity) if !identities.contains(&identity) => identities.push(identity),
                Ok(_) => {}
                Err(reason) => diagnostics.push(ExportDiagnostic {
                    layer_id: Some(owner),
                    message: format!(
                        "Effect {} ({}): compositingOptions effect mask reference to FX mask {reference} omitted: {reason}; that restriction is not applied",
                        effect.name, effect.match_name
                    ),
                }),
            }
        }
        if identities.is_empty() {
            diagnostics.push(ExportDiagnostic {
                layer_id: Some(owner),
                message: format!(
                    "Effect {} ({}): no compositingOptions effect mask reference resolved; the effect applies to the whole layer",
                    effect.name, effect.match_name
                ),
            });
        }
        effect.compositing.mask_identities = identities;
    }
}

#[cfg(test)]
#[path = "effect_masks_tests.rs"]
mod tests;
