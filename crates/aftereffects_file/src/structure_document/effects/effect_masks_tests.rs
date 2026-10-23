//! Failure paths of effect mask reference resolution. The resolved path is
//! pinned by the `color_ae_effect_compositing_options` fixture import.
use fx_schema::{Duration, Time, TimeRangeProperty};

use super::*;

fn match_name(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    Chunk::data(*b"tdmn", bytes).unwrap()
}

/// `mkif` with the identity word at bytes 8..12.
fn mask_info(identity: u32) -> Chunk {
    let mut bytes = vec![0; 16];
    bytes[8..12].copy_from_slice(&identity.to_be_bytes());
    Chunk::data(*b"mkif", bytes).unwrap()
}

/// Layer content whose property root holds `parades` mask parades, each with
/// one Mask Atom per `infos` entry.
fn content(parades: usize, infos: &[Chunk]) -> Vec<Chunk> {
    let atoms: Vec<_> = infos
        .iter()
        .flat_map(|info| [match_name("ADBE Mask Atom"), info.clone()])
        .collect();
    let root: Vec<_> = (0..parades)
        .flat_map(|_| {
            [
                match_name("ADBE Mask Parade"),
                Chunk::list(*b"tdgp", atoms.clone()),
            ]
        })
        .collect();
    vec![Chunk::list(*b"tdgp", root)]
}

fn occurrence() -> GroupLayer {
    crate::structure_document::group(
        LayerId::new(17),
        "Owner".into(),
        None,
        TimeRangeProperty::new(Time::ZERO, Duration::from_secs(1.0)),
    )
}

#[test]
fn mask_ordinal_matches_the_identity_word_and_diagnoses_every_failure() {
    let two = content(1, &[mask_info(4), mask_info(9)]);
    assert_eq!(mask_ordinal(&two, 9), Ok(2));
    assert_eq!(
        mask_ordinal(&two, 7),
        Err("names a mask absent from the layer".into())
    );
    assert_eq!(
        mask_ordinal(&[], 1),
        Err("names a mask, but the layer has no masks".into())
    );
    assert_eq!(
        mask_ordinal(&content(2, &[mask_info(1)]), 1),
        Err("is ambiguous: the layer has several mask parades".into())
    );
    assert_eq!(
        mask_ordinal(&content(1, &[mask_info(3), mask_info(3)]), 3),
        Err("is ambiguous: several masks share that identity".into())
    );
    let truncated = Chunk::data(*b"mkif", vec![0; 10]).unwrap();
    assert_eq!(
        mask_ordinal(&content(1, &[truncated]), 1),
        Err("cannot be matched: malformed mask identity record".into())
    );
}

#[test]
fn fx_mask_diagnoses_masks_that_were_not_imported_or_have_no_fx_mask() {
    let layer = content(1, &[mask_info(1)]);
    let owner = occurrence();
    assert_eq!(
        fx_mask(&layer, 1, &[], &owner),
        Err("names layer Mask 1, which was not imported".into())
    );
    // The guide exists but the occurrence hosts no FX mask built from it.
    assert_eq!(
        fx_mask(&layer, 1, &[(1, LayerId::new(40))], &owner),
        Err("names layer Mask 1, whose FX mask is absent".into())
    );
}

#[test]
fn unresolved_references_are_diagnosed_and_the_effect_stays_unrestricted() {
    let layer = content(1, &[mask_info(1)]);
    let mut owner = occurrence();
    let warnings = resolve(
        vec![PendingEffectMasks {
            effect: EffectId::new(5),
            label: "Effect ADBE Pro Levels2 (native ordinal 1)".into(),
            identities: vec![1, 6],
        }],
        &layer,
        &[],
        &mut owner,
    );
    assert_eq!(
        warnings,
        [
            "Effect ADBE Pro Levels2 (native ordinal 1): Compositing Options effect mask (native identity 1) names layer Mask 1, which was not imported; that restriction omitted",
            "Effect ADBE Pro Levels2 (native ordinal 1): Compositing Options effect mask (native identity 6) names a mask absent from the layer; that restriction omitted",
            "Effect ADBE Pro Levels2 (native ordinal 1): no effect mask reference resolved; effect applies to the whole layer",
        ]
    );
    assert!(owner.effects.is_empty());
}

/// `owner` hosting FX mask 30 (guide layer 40, `mode`) and a Levels record 5.
fn hosted(mode: &str) -> GroupLayer {
    let mut owner = occurrence();
    owner.masks = vec![
        serde_json::from_value(serde_json::json!({"id": 30, "mode": mode, "layer": 40})).unwrap(),
    ];
    owner.effects = vec![
        serde_json::from_value(serde_json::json!({"id": 5, "enabled": true, "effect": {
            "type": "levels", "inputBlack": 0, "inputWhite": 255, "gamma": 0.5,
            "outputBlack": 0, "outputWhite": 255
        }}))
        .unwrap(),
    ];
    owner
}

fn pending() -> Vec<PendingEffectMasks> {
    vec![PendingEffectMasks {
        effect: EffectId::new(5),
        label: "Effect ADBE Pro Levels2 (native ordinal 1)".into(),
        identities: vec![1],
    }]
}

fn mask_references(owner: &GroupLayer) -> serde_json::Value {
    serde_json::to_value(&owner.effects[0]).unwrap()["compositingOptions"]["maskReferences"].clone()
}

#[test]
fn a_referenced_mode_add_mask_restricts_the_effect_without_masking_the_layer() {
    // After Effects 26.5 (in-house aerender probe) restricts the effect to a
    // referenced mode-Add mask and does not apply that mask to the layer.
    let layer = content(1, &[mask_info(1)]);
    let mut owner = hosted("add");
    let warnings = resolve(pending(), &layer, &[(1, LayerId::new(40))], &mut owner);
    assert_eq!(owner.masks[0].mode, MaskMode::None);
    assert_eq!(mask_references(&owner), serde_json::json!([30]));
    assert_eq!(
        warnings,
        [
            "Effect ADBE Pro Levels2 (native ordinal 1): Compositing Options effect mask (native identity 1) is a mask in mode Add, which After Effects applies to the effect but not to the layer (measured, AE 26.5): imported as a referenced FX mask in mode none"
        ]
    );
}

#[test]
fn a_referenced_mode_none_mask_leaves_the_effect_unrestricted() {
    // After Effects 26.5 renders a reference to a mode-None mask as no
    // restriction (in-house probe and the color_ae_effect_compositing_options
    // reference render).
    let layer = content(1, &[mask_info(1)]);
    let mut owner = hosted("none");
    let warnings = resolve(pending(), &layer, &[(1, LayerId::new(40))], &mut owner);
    assert_eq!(owner.masks[0].mode, MaskMode::None);
    assert!(mask_references(&owner).is_null(), "{warnings:?}");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0]
            .contains("is a mask in mode None, which After Effects renders as no restriction")
    );
    assert!(
        warnings[1]
            .contains("no effect mask reference resolved; effect applies to the whole layer")
    );
}
