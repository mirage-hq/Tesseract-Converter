//! Resolve AE effect mask references (Compositing Options) to the owner's FX masks.
//!
//! Effects import before masks, so references wait here until the owner's
//! masks exist. A native reference names a mask by its `mkif` identity word;
//! the importer numbers masks by parade ordinal and hosts both the masks and
//! the effects on the same FX occurrence Group, which is the same-layer scope
//! FX `maskReferences` require.
//!
//! After Effects 26.5 (aerender, 8 bpc, measured on an in-house ramp) renders
//! a reference to a mask in mode None as no restriction: the effect covers the
//! whole layer, so that reference is omitted with a diagnostic. A referenced
//! mask in mode Add restricts the effect but no longer masks the layer, which
//! is FX's mode `none` with the reference, so it imports that way. A
//! referenced mask in another mode keeps it, diagnosed as unmeasured.

use fx_schema::{EffectData, EffectId, EffectRecord, FxItemId, GroupLayer, LayerId, MaskMode};

use crate::{
    properties::{data, root_runs, runs, unique_list},
    rifx::Chunk,
};

/// Native mask identities of one imported effect, keyed by its FX identity.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::structure_document) struct PendingEffectMasks {
    pub effect: EffectId,
    /// Diagnostic prefix naming the native effect.
    pub label: String,
    pub identities: Vec<u32>,
}

/// Commit resolvable references onto the occurrence's effect records and
/// return diagnostics for every reference left unresolved.
pub(in crate::structure_document) fn resolve(
    pending: Vec<PendingEffectMasks>,
    content: &[Chunk],
    guide_ids: &[(u32, LayerId)],
    occurrence: &mut GroupLayer,
) -> Vec<String> {
    let mut warnings = Vec::new();
    // Masks imported in mode Add and switched to `none` because an effect
    // references them; later references to them stay references.
    let mut referenced_add_masks = Vec::new();
    for entry in pending {
        let mut references = Vec::new();
        for identity in &entry.identities {
            match fx_mask(content, *identity, guide_ids, occurrence) {
                Ok(id) if references.contains(&id) => {}
                Ok(id) => {
                    let Some(mask) = occurrence.masks.iter_mut().find(|mask| mask.id == id) else {
                        continue;
                    };
                    match mask.mode {
                        MaskMode::None if referenced_add_masks.contains(&id) => {}
                        MaskMode::None => {
                            warnings.push(format!(
                                "{}: Compositing Options effect mask (native identity {identity}) is a mask in mode None, which After Effects renders as no restriction (measured, AE 26.5): reference omitted, effect applies to the whole layer",
                                entry.label
                            ));
                            continue;
                        }
                        MaskMode::Add => {
                            mask.mode = MaskMode::None;
                            referenced_add_masks.push(id);
                            warnings.push(format!(
                                "{}: Compositing Options effect mask (native identity {identity}) is a mask in mode Add, which After Effects applies to the effect but not to the layer (measured, AE 26.5): imported as a referenced FX mask in mode none",
                                entry.label
                            ));
                        }
                        mode => warnings.push(format!(
                            "{}: Compositing Options effect mask (native identity {identity}) is a mask in mode {mode:?}; how After Effects renders that reference is unmeasured, so the mask keeps its layer-masking role",
                            entry.label
                        )),
                    }
                    references.push(id);
                }
                Err(reason) => warnings.push(format!(
                    "{}: Compositing Options effect mask (native identity {identity}) {reason}; that restriction omitted",
                    entry.label
                )),
            }
        }
        if references.is_empty() {
            warnings.push(format!(
                "{}: no effect mask reference resolved; effect applies to the whole layer",
                entry.label
            ));
            continue;
        }
        let Some(record) = occurrence
            .effects
            .iter_mut()
            .find(|record| matches!(record.data(), EffectData::Identified { id, .. } if *id == entry.effect))
        else {
            warnings.push(format!(
                "{}: effect is no longer hosted with its masks; effect mask restriction omitted",
                entry.label
            ));
            continue;
        };
        let EffectData::Identified {
            id,
            enabled,
            effect,
            compositing_options,
            extensions,
        } = record.data()
        else {
            continue;
        };
        let mut options = compositing_options.clone().unwrap_or_default();
        options.mask_references = references;
        match EffectRecord::from_data(&EffectData::Identified {
            id: *id,
            enabled: *enabled,
            effect: effect.clone(),
            compositing_options: Some(options),
            extensions: extensions.clone(),
        }) {
            Ok(updated) => *record = updated,
            Err(error) => warnings.push(format!(
                "{}: effect mask references rejected by FX ({error}); effect applies to the whole layer",
                entry.label
            )),
        }
    }
    warnings
}

/// The FX mask imported from the native mask with `identity`.
fn fx_mask(
    content: &[Chunk],
    identity: u32,
    guide_ids: &[(u32, LayerId)],
    occurrence: &GroupLayer,
) -> Result<FxItemId, String> {
    let ordinal = mask_ordinal(content, identity)?;
    let guide = guide_ids
        .iter()
        .find(|(index, _)| *index == ordinal)
        .map(|(_, guide)| *guide)
        .ok_or_else(|| format!("names layer Mask {ordinal}, which was not imported"))?;
    occurrence
        .masks
        .iter()
        .find(|mask| mask.layer == Some(guide))
        .map(|mask| mask.id)
        .ok_or_else(|| format!("names layer Mask {ordinal}, whose FX mask is absent"))
}

/// One-based parade ordinal of the layer mask whose `mkif` identity matches.
fn mask_ordinal(content: &[Chunk], identity: u32) -> Result<u32, String> {
    let roots = root_runs(content).map_err(|e| e.to_string())?;
    let mut parades = roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Parade");
    let Some((_, parade)) = parades.next() else {
        return Err("names a mask, but the layer has no masks".into());
    };
    if parades.next().is_some() {
        return Err("is ambiguous: the layer has several mask parades".into());
    }
    let atoms = runs(unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut selected = None;
    for (index, (_, atom)) in atoms
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Atom")
        .enumerate()
    {
        let info = data(atom, *b"mkif").map_err(|e| e.to_string())?;
        let Some(word) = info.get(8..12) else {
            return Err("cannot be matched: malformed mask identity record".into());
        };
        if u32::from_be_bytes(word.try_into().expect("four-byte slice")) == identity {
            if selected.is_some() {
                return Err("is ambiguous: several masks share that identity".into());
            }
            selected = Some(u32::try_from(index + 1).map_err(|e| e.to_string())?);
        }
    }
    selected.ok_or_else(|| "names a mask absent from the layer".into())
}

#[cfg(test)]
#[path = "effect_masks_tests.rs"]
mod tests;
