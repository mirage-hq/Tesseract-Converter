//! Failure paths of effect mask reference binding on final native layers.
//! The resolved path is pinned by the compositing-options export round trip.
use fx_schema::FxItemId;

use super::*;
use crate::writer::{NativeMaskSpec, effects};

fn mask(source: Option<u64>) -> NativeMaskSpec {
    let mut mask =
        NativeMaskSpec::crop_rectangle("Mask", [100, 100], [0.0, 0.0, 50.0, 100.0]).unwrap();
    mask.source_mask = source.map(FxItemId::new);
    mask
}

/// One Levels effect referencing `references` on a layer holding `masks`.
fn options(masks: Vec<NativeMaskSpec>, references: &[u64]) -> NativeLayerOptions {
    let mut levels = effects::new_effect("ADBE Pro Levels2", true, [100.0, 100.0]).unwrap();
    levels.compositing.unresolved_mask_references =
        references.iter().copied().map(FxItemId::new).collect();
    NativeLayerOptions {
        fx_id: LayerId::new(7),
        parent: None,
        matte: None,
        enabled: true,
        adjustment_layer: false,
        motion_blur: false,
        blend_mode: 0,
        masks,
        effects: vec![levels],
        styles: Vec::new(),
        source_clock: None,
        transform_3d: None,
    }
}

fn messages(diagnostics: &[ExportDiagnostic]) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

#[test]
fn a_reference_to_a_mask_not_exported_on_the_layer_is_omitted_with_a_diagnostic() {
    // FX mask 41 was lowered elsewhere (or omitted); only mask 40 is here.
    let mut layer = options(vec![mask(Some(40))], &[41]);
    let mut diagnostics = Vec::new();
    resolve(&mut layer, &mut diagnostics);
    assert_eq!(
        messages(&diagnostics),
        [
            "Effect Levels (Individual Controls) (ADBE Pro Levels2): compositingOptions effect mask reference to FX mask 41 omitted: it is not exported as a native mask on the effect's native layer; that restriction is not applied",
            "Effect Levels (Individual Controls) (ADBE Pro Levels2): no compositingOptions effect mask reference resolved; the effect applies to the whole layer",
        ]
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d.layer_id == Some(LayerId::new(7)))
    );
    let compositing = &layer.effects[0].compositing;
    assert!(compositing.mask_identities.is_empty());
    assert!(compositing.unresolved_mask_references.is_empty());
}

#[test]
fn a_reference_split_into_several_native_masks_is_omitted_and_others_still_resolve() {
    // An inverted compound lowers FX mask 40 to two native masks; FX mask 42
    // is the layer's third native mask (identity 3).
    let mut layer = options(
        vec![mask(Some(40)), mask(Some(40)), mask(Some(42))],
        &[40, 42],
    );
    let mut diagnostics = Vec::new();
    resolve(&mut layer, &mut diagnostics);
    assert_eq!(
        messages(&diagnostics),
        [
            "Effect Levels (Individual Controls) (ADBE Pro Levels2): compositingOptions effect mask reference to FX mask 40 omitted: it is exported as several native masks; that restriction is not applied",
            "Effect Levels (Individual Controls) (ADBE Pro Levels2): compositingOptions references FX mask 42 in mode Add; After Effects does not apply an effect-referenced Add mask to the layer (measured) and the other modes are unmeasured, so the layer's own masking may differ",
        ]
    );
    assert_eq!(
        layer.effects[0].compositing.mask_identities,
        [native_mask_identity(2).unwrap()]
    );
}

#[test]
fn a_referenced_mode_none_mask_is_written_in_mode_add() {
    // After Effects 26.5 ignores an effect reference to a mode-None mask and
    // restricts the effect to a referenced mode-Add mask without masking the
    // layer (in-house aerender probe), which is FX's referenced `none` mask.
    let mut referenced = mask(Some(40));
    referenced.mode = NativeMaskMode::None;
    let mut unreferenced = mask(Some(41));
    unreferenced.mode = NativeMaskMode::None;
    let mut layer = options(vec![referenced, unreferenced], &[40]);
    let mut diagnostics = Vec::new();
    resolve(&mut layer, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{:?}", messages(&diagnostics));
    assert_eq!(layer.masks[0].mode, NativeMaskMode::Add);
    assert_eq!(layer.masks[1].mode, NativeMaskMode::None);
    assert_eq!(
        layer.effects[0].compositing.mask_identities,
        [native_mask_identity(0).unwrap()]
    );
}
