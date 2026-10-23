//! Commit native colour effects and per-effect Compositing Options to FX records.

use std::collections::HashMap;

use fx_schema::{EffectData, EffectId, EffectPayload, EffectRecord, PropertyTarget};

use super::super::{
    animation::{self, NumericAnimationClock, NumericAnimationTarget},
    animation_budget::AnimationBudget,
    reserve_ids,
};
use super::ImportedEffects;
use crate::{
    effects::{
        color_grades,
        compositing::{self, NativeCompositing},
        native::DecodedEffect,
    },
    structure::Layer,
};

/// Lower one native colour effect and its affine keys, committing its
/// identity only after the record and every key binding are admitted.
pub(super) fn import(
    source: &DecodedEffect,
    domain: color_grades::ColorGradeDomain,
    layer: &Layer,
    next_id: &mut u64,
    budget: &mut AnimationBudget,
    result: &mut ImportedEffects,
) {
    let name = &source.match_name;
    let lowered = match color_grades::lower(source, domain) {
        Ok(lowered) => lowered,
        Err(reason) => {
            result.warnings.push(format!(
                "Effect {name}: {reason}; effect omitted, owner and other effects retained"
            ));
            return;
        }
    };
    let mut candidate_id = *next_id;
    let Some(raw_id) = reserve_ids(&mut candidate_id, 1) else {
        result.warnings.push(format!(
            "Effect {name}: generated layer identifier space exhausted; effect omitted"
        ));
        return;
    };
    let id = EffectId::new(raw_id);
    let record = match EffectRecord::from_data(&EffectData::Identified {
        id,
        enabled: source.enabled && layer.record.flags().effects_active,
        effect: EffectPayload::Known(lowered.effect),
        compositing_options: None,
        extensions: Default::default(),
    }) {
        Ok(record) => record,
        Err(error) => {
            result.warnings.push(format!(
                "Effect {name}: mapped grade is outside current FX representation ({error}); effect omitted, owner retained"
            ));
            return;
        }
    };
    let mut animations = Vec::new();
    if !lowered.keyed.is_empty() {
        match NumericAnimationClock::parent_identity(layer) {
            Ok(clock) => {
                for keyed in lowered.keyed {
                    let control = keyed.control;
                    let mut numeric = keyed.numeric;
                    // Targets scale values; shift keys so the offset survives scaling.
                    let shift = control.offset / control.scale;
                    for key in &mut numeric.keyframes {
                        key.values.iter_mut().for_each(|value| *value += shift);
                    }
                    let checkpoint = budget.checkpoint();
                    let (entries, warnings) = animation::numeric_entries(
                        control.native,
                        &numeric,
                        &[NumericAnimationTarget::float(
                            PropertyTarget::effect_param(id, control.param),
                            0,
                            control.scale,
                        )],
                        clock,
                        budget,
                    );
                    if entries.len() == 1 && warnings.is_empty() {
                        animations.extend(entries);
                    } else {
                        budget.rollback(checkpoint);
                        result.warnings.push(format!(
                            "Effect {name} / {}: key lowering failed ({}); animation omitted, initial value retained",
                            control.native,
                            warnings.join("; ")
                        ));
                    }
                }
            }
            Err(error) => result.warnings.push(format!(
                "Effect {name}: {error}; keys omitted, initial values retained"
            )),
        }
    }
    // Further static effects of the same instance (Lumetri Input LUT before
    // the grade; Look, curves, wheels, vignette after it), each with its own
    // identity, in native order.
    let mut sections = |effects: Vec<fx_schema::LayerEffect>| {
        let mut records = Vec::with_capacity(effects.len());
        for effect in effects {
            let Some(raw_id) = reserve_ids(&mut candidate_id, 1) else {
                result.warnings.push(format!(
                    "Effect {name}: generated layer identifier space exhausted; later Lumetri sections omitted"
                ));
                break;
            };
            match EffectRecord::from_data(&EffectData::Identified {
                id: EffectId::new(raw_id),
                enabled: source.enabled && layer.record.flags().effects_active,
                effect: EffectPayload::Known(effect),
                compositing_options: None,
                extensions: Default::default(),
            }) {
                Ok(record) => records.push(record),
                Err(error) => result.warnings.push(format!(
                    "Effect {name}: mapped section is outside current FX representation ({error}); section omitted"
                )),
            }
        }
        records
    };
    let before = sections(lowered.before);
    let after = sections(lowered.after);
    *next_id = candidate_id;
    for record in before
        .into_iter()
        .chain(std::iter::once(record))
        .chain(after)
    {
        result.native_ordinals.push(source.index);
        result.effects.push(record);
    }
    result.animations.extend(animations);
    result.warnings.extend(
        lowered
            .warnings
            .into_iter()
            .map(|warning| format!("Effect {name}: {warning}")),
    );
}

/// Apply each native instance's Compositing Options to the one FX record it
/// produced; effect mask references are queued until the owner's masks exist.
/// Multi-record lowerings cannot split a stage-wide opacity or mask. Authored
/// options that reach no FX effect record are diagnosed, never dropped silently.
pub(super) fn apply_compositing(
    result: &mut ImportedEffects,
    natives: &HashMap<usize, (String, Result<NativeCompositing, String>)>,
) {
    let mut authored: Vec<_> = natives
        .iter()
        .filter(|(_, (_, native))| {
            !native
                .as_ref()
                .is_ok_and(|native| *native == NativeCompositing::default())
        })
        .map(|(ordinal, (name, _))| (*ordinal, name.as_str()))
        .collect();
    authored.sort_unstable();
    if result.effects.len() != result.native_ordinals.len() {
        for (ordinal, name) in authored {
            result.warnings.push(format!(
                "Effect {name} (native ordinal {ordinal}): Compositing Options not imported; this layer's {} FX effect records cannot be paired with their {} native instances, so the effect applies at full strength over the whole layer",
                result.effects.len(),
                result.native_ordinals.len()
            ));
        }
        return;
    }
    for (ordinal, name) in &authored {
        if !result.native_ordinals.contains(ordinal) {
            result.warnings.push(format!(
                "Effect {name} (native ordinal {ordinal}): Compositing Options not imported; the native effect produced no FX effect record (omitted, or lowered to a layer opacity, generator or frame-fade record)"
            ));
        }
    }
    let mut counts = HashMap::<usize, usize>::new();
    for ordinal in &result.native_ordinals {
        *counts.entry(*ordinal).or_default() += 1;
    }
    for (record, ordinal) in result.effects.iter_mut().zip(&result.native_ordinals) {
        let Some((name, native)) = natives.get(ordinal) else {
            continue;
        };
        if native
            .as_ref()
            .is_ok_and(|native| *native == NativeCompositing::default())
        {
            continue;
        }
        let prefix = format!("Effect {name} (native ordinal {ordinal})");
        let EffectData::Identified {
            id,
            enabled,
            effect: EffectPayload::Known(effect),
            compositing_options: None,
            extensions,
        } = record.data()
        else {
            continue;
        };
        if counts.get(ordinal).is_some_and(|count| *count > 1) {
            result.warnings.push(format!(
                "{prefix}: Compositing Options not applied; the native stage lowers to several FX effects and its opacity/mask restriction is not representable per effect"
            ));
            continue;
        }
        let lowered = compositing::lower(effect, native);
        result.warnings.extend(
            lowered
                .warnings
                .into_iter()
                .map(|warning| format!("{prefix}: {warning}")),
        );
        if !lowered.mask_identities.is_empty() {
            result
                .pending_effect_masks
                .push(super::effect_masks::PendingEffectMasks {
                    effect: *id,
                    label: prefix.clone(),
                    identities: lowered.mask_identities,
                });
        }
        let Some(options) = lowered.options else {
            continue;
        };
        match EffectRecord::from_data(&EffectData::Identified {
            id: *id,
            enabled: *enabled,
            effect: EffectPayload::Known(effect.clone()),
            compositing_options: Some(options),
            extensions: extensions.clone(),
        }) {
            Ok(updated) => *record = updated,
            Err(error) => result.warnings.push(format!(
                "{prefix}: Compositing Options rejected by FX ({error}); effect applied at full strength"
            )),
        }
    }
}
