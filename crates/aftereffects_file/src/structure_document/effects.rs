//! Best-effort layer-owned effect import. Effect clocks are independent of source remap.

use std::collections::{HashMap, HashSet};

use fx_schema::{
    EffectData, EffectId, EffectPayload, EffectRecord, LayerEffect, PropertyTarget, PropertyValue,
    animator::{
        AnimationGraphEntry, AnimatorData, PropertyAnimator, PropertyKeyframe,
        PropertyKeyframeTrack,
    },
};

use super::{
    animation::{self, NumericAnimationClock, NumericAnimationTarget},
    animation_budget::AnimationBudget,
    control_links,
};
use crate::{
    effects::{keylight, mapping, native, special, toner},
    expression_samples::{ExpressionSamples, PropertyIdentity},
    properties::NumericProperty,
    structure::{Layer, ProjectItem},
};

#[path = "effects/color_balance_identity.rs"]
mod color_balance_identity;

#[cfg(test)]
#[path = "effects/toner_tests.rs"]
mod toner_tests;

mod frame_fade;

pub(super) use frame_fade::FrameFade;

/// Native effect controls and direct aliases resolve against one source project.
pub(super) struct ImportContext<'a> {
    pub evaluations: &'a ExpressionSamples,
    pub composition_id: u32,
    pub items: Option<&'a HashMap<u32, &'a ProjectItem>>,
}

pub(super) struct ImportedEffects {
    pub effects: Vec<EffectRecord>,
    pub native_ordinals: Vec<usize>,
    pub fractal_blends: Vec<super::fractal_blend::Stage>,
    pub animations: Vec<fx_schema::animator::AnimationGraphEntry>,
    pub warnings: Vec<String>,
    pub adjustment_opacity: Option<fx_schema::PercentageProperty>,
    /// An active Roto Brush was omitted, so the owner's pixels are unsegmented.
    pub unsupported_cutout: bool,
    /// A recognized frame-fade preset, whose native effects were not imported,
    /// or the warning why a present preset is not lowered. The caller resolves
    /// it after the Shape importer, which can commit the same preset itself.
    pub frame_fade: Result<Option<FrameFade>, String>,
}

/// Roto Brush & Refine Edge: AE segments the owner's foreground into alpha.
const ROTO_BRUSH: &str = "ADBE Samurai";

fn parameter_plane_size(match_name: &str, native_size: [u16; 2], group_size: [u16; 2]) -> [u16; 2] {
    if matches!(match_name, "ADBE Bulge" | "ADBE Ripple" | "ADBE Wave Warp") {
        // These effects evaluate in the destination Group's UV plane. Keep
        // native pixel controls fixed when a source-backed layer is hosted by
        // the composition-sized imported Group.
        group_size
    } else {
        native_size
    }
}

/// Lowers a control whose enabled expression is a pure alias of another control
/// of the same effect occurrence to that control's decoded values/keys,
/// including its integer-slider normalization. Captured Adobe samples take
/// precedence and are checked by the caller before this is consulted.
fn same_effect_alias(
    layer: &Layer,
    source: &native::DecodedEffect,
    destination: &native::DecodedParameter,
    numeric: &NumericProperty,
    warnings: &mut Vec<String>,
) -> Option<NumericProperty> {
    let resolved = control_links::resolve_same_effect_alias(
        &layer.content,
        source.index,
        &destination.match_name,
    )?;
    let scalar = |value: &NumericProperty| {
        !value.dimensions_separated
            && value.values.len() <= 1
            && value.keyframes.iter().all(|key| key.values.len() == 1)
    };
    let lowered = resolved
        .map_err(|error| error.to_string())
        .and_then(|target| {
            let referenced = source
                .parameters
                .iter()
                .find(|parameter| parameter.match_name == target)
                .ok_or_else(|| format!("referenced control {target} was not decoded"))?
                .numeric
                .as_ref()
                .map_err(|error| format!("referenced control {target}: {error}"))?;
            if referenced.expression_enabled {
                return Err(format!(
                    "referenced control {target} has an enabled expression"
                ));
            }
            if !scalar(referenced) || !scalar(numeric) {
                return Err("only scalar controls are lowered".into());
            }
            if referenced.value_kind != numeric.value_kind {
                return Err(format!(
                    "referenced control {target} and destination value kinds differ"
                ));
            }
            let mut lowered = referenced.clone();
            // The lowered values/keys are independent; no expression remains.
            lowered.expression_present = false;
            Ok((target, lowered))
        });
    match lowered {
        Ok((target, lowered)) => {
            warnings.push(format!(
                "Effect {} / {}: same-effect alias of {target} lowered to independent editable values/keys; live expression linkage is not retained",
                source.match_name, destination.match_name
            ));
            Some(lowered)
        }
        Err(error) => {
            warnings.push(format!(
                "Effect {} / {}: same-effect alias not lowered ({error}); existing expression fallback retained",
                source.match_name, destination.match_name
            ));
            None
        }
    }
}

fn bulge_pinning_can_enable(source: &native::DecodedEffect) -> bool {
    source.match_name == "ADBE Bulge"
        && source.parameters.iter().any(|parameter| {
            parameter.match_name == "ADBE Bulge-0007"
                && parameter.numeric.as_ref().is_ok_and(|numeric| {
                    numeric.expression_enabled
                        || numeric.values.iter().any(|value| *value != 0.0)
                        || numeric
                            .keyframes
                            .iter()
                            .any(|key| key.values.iter().any(|value| *value != 0.0))
                })
        })
}

/// Keylight lowers to one converter-owned shader rather than a typed mapping.
/// Its identity is committed only after the instance is admitted.
fn import_keylight(
    source: &native::DecodedEffect,
    layer: &Layer,
    next_id: &mut u64,
    result: &mut ImportedEffects,
) {
    let lowered = match keylight::lower(source) {
        Ok(lowered) => lowered,
        Err(reason) => {
            result.warnings.push(format!(
                "Effect {}: {reason}; effect omitted, owner and other effects retained",
                source.match_name
            ));
            return;
        }
    };
    let mut candidate_id = *next_id;
    let Some(raw_id) = super::reserve_ids(&mut candidate_id, 1) else {
        result.warnings.push(format!(
            "Effect {}: generated layer identifier space exhausted; effect omitted",
            source.match_name
        ));
        return;
    };
    match EffectRecord::from_data(&EffectData::Identified {
        id: EffectId::new(raw_id),
        enabled: source.enabled && layer.record.flags().effects_active,
        effect: EffectPayload::Known(lowered.effect),
    }) {
        Ok(record) => {
            *next_id = candidate_id;
            result.effects.push(record);
            result.native_ordinals.push(source.index);
            result.warnings.push(format!(
                "Effect {}: {}",
                source.match_name,
                keylight::APPROXIMATION
            ));
            if !lowered.defaulted.is_empty() {
                result.warnings.push(format!(
                    "Effect {}: the instance stores neither a value nor a declaration for {}; recorded Keylight 906 plugin defaults are used, and another plugin build with this match name is not established",
                    source.match_name,
                    lowered.defaulted.join(", ")
                ));
            }
        }
        Err(error) => result.warnings.push(format!(
            "Effect {}: shader is outside current FX representation ({error}); effect omitted, owner retained",
            source.match_name
        )),
    }
}

/// Largest imported Ripple `|amplitude| * frequency`: 25% beyond the FX ring
/// fold-over threshold of 1, a visual approximation that keeps a modest
/// strength for a strong native ripple. It is not derived from native units.
const RIPPLE_STRENGTH_LIMIT: f64 = 1.25;

/// Limits an imported Ripple's amplitude to [`RIPPLE_STRENGTH_LIMIT`].
///
/// FX Ripple samples `center + direction * g(r)`, with
/// `g(r) = r + amplitude * sin(frequency * r - phase)`, over its whole plane;
/// native Ripple is instead confined by its Radius, which FX cannot represent.
/// If `|amplitude| * frequency <= 1`, `g` is non-decreasing for every phase,
/// so rings keep their order along each ray; beyond it they fold over and tear.
/// [`RIPPLE_STRENGTH_LIMIT`] exceeds that threshold, so it does not prevent
/// slight fold-over near a limited peak.
///
/// Runs after the ordinary plane-width lowering, including AE-evaluated
/// expression fitting. When the retained static amplitude or any emitted
/// amplitude key exceeds `RIPPLE_STRENGTH_LIMIT / frequency` (the emitted FX
/// frequency), all of them are multiplied by one factor so the largest meets
/// it; a Ripple whose values are all within it is unchanged. The retained
/// value counts even while a track overrides it, because it is also the
/// static fallback when lowering fails. Key ids, times and easing are kept,
/// and each rescaled track is recharged exactly; one the allowance cannot hold
/// is omitted. Linear and hold segments stay within the limit, but overshooting
/// cubic easing between keys and the amplitude-sized disk around the center
/// are not bounded. This approximates native Ripple; it does not reproduce it.
fn limit_ripple_amplitude(
    id: EffectId,
    plane_width: f64,
    payload: &mut serde_json::Value,
    animations: &mut Vec<AnimationGraphEntry>,
    budget: &mut AnimationBudget,
) -> Vec<String> {
    let Some(limit) = payload["frequency"]
        .as_f64()
        .filter(|frequency| frequency.is_finite() && *frequency > 0.0)
        .map(|frequency| RIPPLE_STRENGTH_LIMIT / frequency)
    else {
        return Vec::new();
    };
    let target = PropertyTarget::effect_param(id, "amplitude");
    let base = payload["amplitude"]
        .as_f64()
        .filter(|value| value.is_finite());
    let peak = animations
        .iter()
        .filter(|entry| entry.target == target)
        .filter_map(|entry| entry.animator.finite_value_range())
        .flatten()
        .filter_map(|value| match value {
            PropertyValue::Float(value) => Some(value.abs()),
            _ => None,
        })
        .chain(base.map(f64::abs))
        .fold(0.0, f64::max);
    if peak <= limit {
        return Vec::new();
    }
    let factor = limit / peak;
    if let Some(base) = base {
        payload["amplitude"] = serde_json::json!(base * factor);
    }
    let mut warnings = vec![format!(
        "Effect ADBE Ripple / ADBE Ripple-0006: FX Ripple displaces its whole plane and its rings fold over once amplitude * frequency exceeds 1, while native Ripple is confined by its omitted Radius; after ordinary lowering, the retained amplitude and every emitted amplitude key are scaled by {factor:.6} so the largest, {:.3}px, meets the {:.3}px strength limit ({RIPPLE_STRENGTH_LIMIT} / frequency). That limit is a visual approximation, not a native unit conversion, and rings can still fold where amplitude * frequency exceeds 1. Key times and easing, center, phase and frequency are retained; overshooting easing between keys is not bounded, and this approximates, and does not reproduce, the native ripple",
        peak * plane_width,
        limit * plane_width,
    )];
    let charge = |entry: &AnimationGraphEntry| {
        super::animation_budget::committed_entry_reservation_bytes(entry)
            .map_err(|error| error.to_string())
    };
    animations.retain_mut(|entry| {
        if entry.target != target {
            return true;
        }
        let rescaled = rescaled_entry(entry, factor).and_then(|scaled| {
            let (old, new) = (charge(entry)?, charge(&scaled)?);
            budget
                .release(old)
                .and_then(|()| budget.reserve(new))
                .map_err(|error| error.to_string())?;
            Ok(scaled)
        });
        match rescaled {
            Ok(scaled) => {
                *entry = scaled;
                true
            }
            Err(error) => {
                warnings.push(format!("Effect ADBE Ripple / ADBE Ripple-0006: {error}; amplitude animation omitted, limited static amplitude retained"));
                false
            }
        }
    });
    warnings
}

/// `entry` with every float value multiplied by `factor`; key ids, times,
/// easing and tangents are unchanged.
fn rescaled_entry(entry: &AnimationGraphEntry, factor: f64) -> Result<AnimationGraphEntry, String> {
    let AnimatorData::Keyframes {
        track,
        enabled,
        disabled_value,
    } = entry.animator.data()
    else {
        return Err("only an editable keyframe track can be rescaled".into());
    };
    let scale = |value: &PropertyValue| match value {
        PropertyValue::Float(value) => PropertyValue::Float(value * factor),
        other => other.clone(),
    };
    let keys = track
        .keyframes()
        .iter()
        .map(|key| {
            PropertyKeyframe::new(
                key.id().clone(),
                key.layer_time(),
                scale(key.value()),
                key.easing(),
            )
            .with_spatial_tangents(key.spatial_in_tangent(), key.spatial_out_tangent())
        })
        .collect();
    let track = PropertyKeyframeTrack::new(keys).map_err(|error| error.to_string())?;
    let mut scaled = entry.clone();
    scaled.animator = PropertyAnimator::from_data(&AnimatorData::Keyframes {
        track,
        enabled: *enabled,
        disabled_value: disabled_value.as_ref().map(scale),
    })
    .map_err(|error| error.to_string())?;
    Ok(scaled)
}

#[cfg(test)]
pub(super) fn import(
    evaluations: &ExpressionSamples,
    comp_id: u32,
    layer: &Layer,
    native_size: [u16; 2],
    group_size: [u16; 2],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> ImportedEffects {
    import_with_context(
        ImportContext {
            evaluations,
            composition_id: comp_id,
            items: None,
        },
        layer,
        native_size,
        group_size,
        next_id,
        budget,
    )
}

pub(super) fn import_with_context(
    context: ImportContext<'_>,
    layer: &Layer,
    native_size: [u16; 2],
    group_size: [u16; 2],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> ImportedEffects {
    let (native_effects, warnings) =
        native::read_effects(&layer.content, native_size.map(f64::from));
    let frame_fade = frame_fade::recognize(layer).map_err(|reason| {
        format!(
            "Fade In+Out - frames: frame fade not lowered ({reason}); its controller and Solid Composite are imported as ordinary effects"
        )
    });
    let consumed = frame_fade.as_ref().ok().copied().flatten();
    let mut result = ImportedEffects {
        effects: Vec::new(),
        native_ordinals: Vec::new(),
        fractal_blends: Vec::new(),
        animations: Vec::new(),
        warnings,
        adjustment_opacity: None,
        unsupported_cutout: false,
        frame_fade,
    };
    let mut identities = HashSet::new();
    for source in &native_effects {
        if source.match_name == color_balance_identity::MATCH_NAME {
            match color_balance_identity::is_identity(layer, source, context.items) {
                Ok(true) => { identities.insert(source.index); }
                Ok(false) => {}
                Err(reason) => result.warnings.push(format!("Effect {}: identity not established ({reason}); unsupported effect retained as an omission", source.match_name)),
            }
        }
    }
    let opaque_fractal =
        crate::effects::fractal_noise::opaque_ordinals(layer, &native_effects, context.items);
    let fractal_canvas = crate::effects::fractal_noise::blend_canvas(layer, context.items);
    for source in native_effects {
        if identities.contains(&source.index) {
            result.warnings.push(format!("Effect {}: all three static neutral controls proved; identity stage omitted without changing other effect ownership", source.match_name));
            continue;
        }
        // These need layer-level geometry/mask/opacity stages, not Group
        // shaders. Their dedicated converters report rejected/approximated cases.
        if matches!(
            source.match_name.as_str(),
            "ADBE Geometry2" | "ADBE Radial Wipe" | "CC Split 2" | "APC Vegas"
        ) || consumed.is_some_and(|fade| fade.consumes(source.index))
        {
            continue;
        }
        if source.match_name == crate::effects::fractal_noise::MATCH_NAME {
            let candidate = (|| -> Result<_, String> {
                let (effect, note, blend) = match crate::effects::fractal_noise::lower(
                    &source,
                    layer,
                    native_size,
                    opaque_fractal.contains(&source.index),
                ) {
                    Ok((effect, note)) => (effect, note, None),
                    Err(reason) => {
                        if !source.enabled || !layer.record.flags().effects_active {
                            return Err(reason);
                        }
                        let (effect, mode, opacity) = crate::effects::fractal_noise::blend_stage(
                            layer,
                            &source,
                            native_size,
                            fractal_canvas,
                        )
                        .map_err(|blend_reason| format!("{reason}; {blend_reason}"))?;
                        (
                            effect,
                            "static Basic/Spline Multiply/Screen generator staged for source-layer blend approximation; native kernel/scale/evolution/HDR semantics differ",
                            Some((
                                mode,
                                fx_schema::PercentageProperty::new(opacity)
                                    .ok_or("invalid Fractal opacity")?,
                            )),
                        )
                    }
                };
                let mut cursor = *next_id;
                let id = super::reserve_ids(&mut cursor, 1)
                    .ok_or("effect identity allocation exhausted")?;
                let effect = EffectRecord::from_data(&EffectData::Identified {
                    id: EffectId::new(id),
                    enabled: source.enabled && layer.record.flags().effects_active,
                    effect: EffectPayload::Known(effect),
                })
                .map_err(|e| e.to_string())?;
                Ok((cursor, effect, note, blend))
            })();
            match candidate {
                Ok((cursor, effect, note, blend)) => {
                    *next_id=cursor;
                    if let Some((blend_mode,opacity))=blend {result.fractal_blends.push(super::fractal_blend::Stage {native_ordinal:source.index,generator:effect,blend_mode,opacity});}
                    else {result.native_ordinals.push(source.index);result.effects.push(effect);}
                    result.warnings.push(format!("Effect ADBE Fractal Noise: {note}"));
                }
                Err(reason) => result.warnings.push(format!("Effect ADBE Fractal Noise: {reason}; effect omitted, owner and siblings retained")),
            }
            continue;
        }
        if source.match_name == "ADBE Box Blur2" {
            let size = if layer.record.flags().adjustment_layer {
                group_size
            } else {
                native_size
            };
            let candidate = (|| -> Result<_, String> {
                let effect = crate::effects::box_blur::lower(&source, layer, size)?;
                let mut cursor = *next_id;
                let id = super::reserve_ids(&mut cursor, 1)
                    .ok_or("effect identity allocation exhausted")?;
                let effect = EffectRecord::from_data(&EffectData::Identified {
                    id: EffectId::new(id),
                    enabled: source.enabled && layer.record.flags().effects_active,
                    effect: EffectPayload::Known(effect),
                })
                .map_err(|e| e.to_string())?;
                Ok((cursor, effect))
            })();
            match candidate {
                Ok((cursor, effect)) => {
                    *next_id = cursor;
                    result.native_ordinals.push(source.index);
                    result.effects.push(effect);
                    result.warnings.push("Effect ADBE Box Blur2: static Both-dimensions Fast Box Blur approximated by editable GaussianBlur with source-derived sequential box variance; native kernel, fractional-radius sampling and edge alpha are unverified".into());
                }
                Err(reason) => result.warnings.push(format!(
                    "Effect ADBE Box Blur2: {reason}; effect omitted, owner and siblings retained"
                )),
            }
            continue;
        }
        if source.match_name == "CC Toner" {
            let candidate = (|| -> Result<_, String> {
                let lowered = toner::lower(&source)?;
                let enabled = source.enabled && layer.record.flags().effects_active;
                let wet = if lowered.original > 0.0 && enabled {
                    toner_original_gate(layer, &identities)?;
                    Some(
                        fx_schema::PercentageProperty::new((1.0 - lowered.original) * 100.0)
                            .ok_or("Toner wet opacity is outside FX representation")?,
                    )
                } else {
                    None
                };
                let mut candidate_id = *next_id;
                let raw_id = super::reserve_ids(&mut candidate_id, 2)
                    .ok_or("generated effect identifier space exhausted")?;
                let records = lowered
                    .effects
                    .into_iter()
                    .enumerate()
                    .map(|(index, effect)| {
                        EffectRecord::from_data(&EffectData::Identified {
                            id: EffectId::new(raw_id + index as u64),
                            enabled,
                            effect: EffectPayload::Known(effect),
                        })
                        .map_err(|error| error.to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((candidate_id, records, wet))
            })();
            match candidate {
                Ok((candidate_id, records, wet)) => {
                    *next_id = candidate_id;
                    result
                        .native_ordinals
                        .extend(std::iter::repeat_n(source.index, records.len()));
                    result.effects.extend(records);
                    if wet.is_some() {
                        result.adjustment_opacity = wet;
                    }
                    result.warnings.push("Effect CC Toner: approximated with editable grayscale TintTritone and RGB ColorCurves; existing FX luma weights 0.299/0.587/0.114 and equally spaced piecewise-linear ramps are not established as native Cycore semantics".into());
                    if wet.is_some() {
                        result.warnings.push("Effect CC Toner: Blend with Original retained through sole-effect Adjustment wet opacity; opaque RGB mixing is represented, translucent dry-plus-wet alpha equivalence is not established".into());
                    }
                }
                Err(reason) => result.warnings.push(format!(
                    "Effect CC Toner: {reason}; effect omitted, owner and other effects retained"
                )),
            }
            continue;
        }
        if source.match_name == keylight::MATCH_NAME {
            import_keylight(&source, layer, next_id, &mut result);
            continue;
        }
        if let Err(reason) = special::validate_import(&source) {
            result.warnings.push(format!(
                "Effect {}: {reason}; effect omitted, owner and other effects retained",
                source.match_name
            ));
            continue;
        }
        let Some(mapping) = mapping::by_native(&source.match_name) else {
            result.unsupported_cutout |= source.match_name == ROTO_BRUSH
                && source.enabled
                && layer.record.flags().effects_active;
            result.warnings.push(format!("Effect {}: no current native FX counterpart/mapping; effect omitted, owner and other effects retained", source.match_name));
            continue;
        };
        // Probe the budget without consuming an ID for an effect discarded below.
        let mut candidate_id = *next_id;
        let Some(raw_id) = super::reserve_ids(&mut candidate_id, 1) else {
            result.warnings.push(format!(
                "Effect {}: generated layer identifier space exhausted; effect omitted",
                source.match_name
            ));
            continue;
        };
        let id = EffectId::new(raw_id);
        let size = parameter_plane_size(&source.match_name, native_size, group_size);
        let mut value = mapping::default_effect(mapping.fx_type);
        let mut animations = Vec::new();
        let checkpoint = budget.checkpoint();
        let mut unmapped_parameters = Vec::new();
        for parameter in &source.parameters {
            let fields: Vec<_> = mapping
                .fields
                .iter()
                .filter(|field| field.native == parameter.match_name)
                .collect();
            if fields.is_empty() {
                unmapped_parameters.push(parameter);
                continue;
            }
            let numeric = match &parameter.numeric {
                Ok(numeric) => numeric,
                Err(error) => {
                    result.warnings.push(format!(
                        "Effect {} / {}: {error}; FX default retained",
                        source.match_name, parameter.match_name
                    ));
                    continue;
                }
            };
            let evaluated = numeric
                .expression_enabled
                .then(|| {
                    let index = u32::try_from(source.index).ok()?;
                    context.evaluations.lookup(
                        context.composition_id,
                        layer.record.id(),
                        &PropertyIdentity::Effect {
                            index,
                            match_name: parameter.match_name.clone(),
                        },
                    )
                })
                .flatten();
            let alias = (numeric.expression_enabled && evaluated.is_none())
                .then(|| {
                    same_effect_alias(layer, &source, parameter, numeric, &mut result.warnings)
                })
                .flatten();
            let numeric = alias.as_ref().unwrap_or(numeric);
            let mut shifted = numeric.clone();
            let mut shifted_any = false;
            let mut targets = Vec::new();
            let mut offsets = Vec::new();
            for field in fields {
                let (Some(scale), Some(offset)) = (
                    field.scale.factor(size.map(f64::from)),
                    field.offset(size.map(f64::from)),
                ) else {
                    result.warnings.push(format!("Effect {} / {}: source dimensions unavailable; normalized property omitted", source.match_name, parameter.match_name));
                    continue;
                };
                let initial = numeric
                    .keyframes
                    .first()
                    .map(|key| key.values.as_slice())
                    .unwrap_or(&numeric.values);
                if let Some(initial) = initial.get(field.component) {
                    let initial = initial * scale + offset;
                    if initial.is_finite() {
                        value[field.field] = if field.boolean {
                            serde_json::Value::Bool(initial != 0.0)
                        } else {
                            serde_json::json!(initial)
                        };
                    }
                }
                if numeric.expression_enabled && evaluated.is_none() {
                    result.warnings.push(format!("Effect {} / {}: AE expression not executed; initial authored/default value retained", source.match_name, parameter.match_name));
                }
                if !field.animated {
                    if numeric.animated || numeric.expression_enabled {
                        result.warnings.push(format!("Effect {} / {}: FX control is static; animation omitted, initial value retained", source.match_name, parameter.match_name));
                    }
                    continue;
                }
                if offset != 0.0 {
                    shifted_any = true;
                    for key in &mut shifted.keyframes {
                        if let Some(value) = key.values.get_mut(field.component) {
                            *value += offset / scale;
                        }
                    }
                }
                offsets.push(offset);
                targets.push(NumericAnimationTarget::float(
                    PropertyTarget::effect_param(id, field.param),
                    field.component,
                    scale,
                ));
            }
            if targets.is_empty() {
                continue;
            }
            if let Some(samples) = evaluated {
                let (entries, warnings) = animation::evaluated_numeric_entries(
                    &parameter.match_name,
                    samples,
                    &targets,
                    &offsets,
                    budget,
                );
                animations.extend(entries);
                result.warnings.extend(
                    warnings
                        .into_iter()
                        .map(|warning| format!("Effect {}: {warning}", source.match_name)),
                );
                continue;
            }
            let clock = match NumericAnimationClock::parent_identity(layer) {
                Ok(clock) => clock,
                Err(error) => {
                    result.warnings.push(format!(
                        "Effect {} / {}: {error}; initial values retained",
                        source.match_name, parameter.match_name
                    ));
                    continue;
                }
            };
            let numeric = if shifted_any { &shifted } else { numeric };
            let (entries, warnings) =
                animation::numeric_entries(&parameter.match_name, numeric, &targets, clock, budget);
            animations.extend(entries);
            result.warnings.extend(
                warnings
                    .into_iter()
                    .map(|warning| format!("Effect {}: {warning}", source.match_name)),
            );
        }
        if source.match_name == "ADBE Gaussian Blur 2" && value["repeatEdgePixels"] == true {
            // Adjustment gates filter the composition stack, irrespective of
            // their dummy solid source. Other Groups retain source-local bounds.
            let bounds = if layer.record.flags().adjustment_layer {
                group_size
            } else {
                native_size
            };
            if bounds.iter().all(|dimension| *dimension > 0) {
                value["layerSize"] = serde_json::json!(bounds);
            }
        }
        let special = special::import(&source, size.map(f64::from), &mut value);
        if let Some(scale) = special.shadow_distance_scale {
            // The static adapter initialized the same offset; the animation keeps
            // distance's native key values/easing on the effect's occurrence clock.
            if let Some(parameter) = source
                .parameters
                .iter()
                .find(|p| p.match_name == "ADBE Drop Shadow-0004")
            {
                match (
                    parameter.numeric.as_ref(),
                    NumericAnimationClock::parent_identity(layer),
                ) {
                    (Ok(numeric), Ok(clock)) => {
                        let target = NumericAnimationTarget::vector2(
                            PropertyTarget::effect_param(id, "offset"),
                            [0, 0],
                            scale,
                        );
                        let (entries, warnings) = animation::numeric_entries(
                            &parameter.match_name,
                            numeric,
                            &[target],
                            clock,
                            budget,
                        );
                        animations.extend(entries);
                        result.warnings.extend(
                            warnings
                                .into_iter()
                                .map(|w| format!("Effect {}: {w}", source.match_name)),
                        );
                    }
                    (_, Err(error)) => result.warnings.push(format!(
                        "Effect {} / {}: {error}; initial offset retained",
                        source.match_name, parameter.match_name
                    )),
                    _ => {}
                }
            }
        }
        if source.match_name == "ADBE Ripple" {
            result.warnings.extend(limit_ripple_amplitude(
                id,
                f64::from(size[0]),
                &mut value,
                &mut animations,
                budget,
            ));
        }
        for parameter in unmapped_parameters {
            if special.consumed(&parameter.match_name) {
                continue;
            }
            let omitted = if parameter
                .numeric
                .as_ref()
                .is_ok_and(|numeric| numeric.animated || numeric.expression_enabled)
            {
                "native control and its animation/expression"
            } else {
                "native control"
            };
            result.warnings.push(format!(
                "Effect {} / {}: {omitted}: no mapped FX property; omitted",
                source.match_name, parameter.match_name
            ));
        }
        result.warnings.extend(
            special
                .warnings
                .into_iter()
                .map(|warning| format!("Effect {}: {warning}", source.match_name)),
        );
        match serde_json::from_value::<LayerEffect>(value).and_then(|effect| {
            EffectRecord::from_data(&EffectData::Identified {
                id,
                enabled: source.enabled && layer.record.flags().effects_active,
                effect: EffectPayload::Known(effect),
            })
        }) {
            Ok(effect) => {
                *next_id = candidate_id;
                result.native_ordinals.push(source.index);
                result.effects.push(effect);
                result.animations.extend(animations);
                if bulge_pinning_can_enable(&source) {
                    result.warnings.push(format!(
                        "Effect {} / ADBE Bulge-0007: Pin All Edges is retained as editable FX pinning, but current FX pinning clamps out-of-content samples into its content rectangle and can repeat occupied boundary pixels beyond the native owner; the pinned AE Shape-owner evidence does not, so native edge semantics remain approximate",
                        source.match_name
                    ));
                }
            }
            Err(error) => {
                budget.rollback(checkpoint);
                result.warnings.push(format!("Effect {}: mapped values are outside current FX representation ({error}); effect omitted, owner retained", source.match_name));
            }
        }
        if !mapping.note.is_empty() {
            result
                .warnings
                .push(format!("Effect {}: {}", source.match_name, mapping.note));
        }
    }
    result
}

/// Original mixing can reuse the whole Adjustment gate only when it cannot
/// attenuate another enabled native effect, mask, matte, or authored opacity.
fn toner_original_gate(layer: &Layer, identities: &HashSet<usize>) -> Result<(), String> {
    use crate::properties;
    let reason = "Blend with Original requires a normal sole-effect Adjustment with static 100% opacity and no masks/matte";
    if !layer.record.flags().adjustment_layer
        || super::compositing::blend_mode(layer.record.blend_mode())
            != Some(fx_schema::BlendMode::Normal)
        || layer.record.track_matte_type() != 0
    {
        return Err(reason.into());
    }
    let styles = crate::layer_styles::read(&layer.content, [1.0, 1.0]);
    if !styles.styles.is_empty() || !styles.warnings.is_empty() {
        return Err(
            "Blend with Original cannot attenuate authored or malformed Adjustment Layer Styles"
                .into(),
        );
    }
    let transform = properties::read_transform(&layer.content).map_err(|_| reason.to_owned())?;
    let opacities: Vec<_> = transform
        .iter()
        .filter(|property| property.match_name == "ADBE Opacity")
        .collect();
    if opacities.len() > 1
        || opacities.iter().any(|property| {
            property.numeric.as_ref().map_or(true, |numeric| {
                numeric.animated
                    || numeric.expression_enabled
                    || !numeric.keyframes.is_empty()
                    || numeric.values != [1.0]
            })
        })
    {
        return Err(reason.into());
    }
    let roots = properties::root_runs(&layer.content).map_err(|_| reason.to_owned())?;
    for (_, run) in roots.iter().filter(|(name, _)| *name == "ADBE Mask Parade") {
        let body = properties::unique_list(run, *b"tdgp").map_err(|_| reason.to_owned())?;
        if !properties::runs(body)
            .map_err(|_| reason.to_owned())?
            .is_empty()
        {
            return Err(reason.into());
        }
    }
    let mut parades = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade");
    let (_, parade) = parades.next().ok_or(reason)?;
    if parades.next().is_some() {
        return Err(reason.into());
    }
    let body = properties::unique_list(parade, *b"tdgp").map_err(|_| reason.to_owned())?;
    let mut enabled = Vec::new();
    for (index, (name, run)) in properties::runs(body)
        .map_err(|_| reason.to_owned())?
        .into_iter()
        .enumerate()
    {
        let sspc = properties::unique_list(run, *b"sspc").map_err(|_| reason.to_owned())?;
        let mut warnings = Vec::new();
        let active = properties::group_enabled_or_warn(sspc, name, &mut warnings);
        if !warnings.is_empty() {
            return Err(reason.into());
        }
        if active && !identities.contains(&(index + 1)) {
            enabled.push(name);
        }
    }
    if enabled != ["CC Toner"] {
        return Err(reason.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fx_schema::{
        EffectId, PropertyTarget, PropertyValue,
        animator::{AnimationGraphEntry, PropertyKeyframeEasing},
    };
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    use super::super::{
        animation::{self, NumericAnimationTarget},
        animation_budget::committed_entry_reservation_bytes,
        to_structural_fx_document, to_structural_fx_document_with_animation_limit,
    };
    use super::{ImportedEffects, import, parameter_plane_size};
    use crate::{
        effects::native,
        expression_samples::ExpressionSamples,
        rifx::Chunk,
        structure::{ItemKind, Layer, read_project},
        structure_document::animation_budget::AnimationBudget,
        writer::{
            KeyframeEasing, NumericKeyframe, NumericTrack,
            effects::{NativeEffect, effect_parade, new_effect},
        },
    };

    fn chunk_match_name(chunk: &Chunk) -> Option<&str> {
        let bytes = chunk.data_payload()?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        std::str::from_utf8(&bytes[..end]).ok()
    }

    fn named_group_mut<'a>(chunks: &'a mut [Chunk], target: &str) -> Option<&'a mut Vec<Chunk>> {
        let group_index = chunks.iter().enumerate().find_map(|(index, chunk)| {
            (chunk.id() == *b"tdmn" && chunk_match_name(chunk) == Some(target)).then(|| {
                let end = chunks[index + 1..]
                    .iter()
                    .position(|candidate| candidate.id() == *b"tdmn")
                    .map_or(chunks.len(), |offset| index + 1 + offset);
                (index + 1..end).find(|candidate| chunks[*candidate].list_kind() == Some(*b"tdgp"))
            })?
        });
        if let Some(index) = group_index {
            return chunks[index].children_mut();
        }
        for chunk in chunks {
            if let Some(children) = chunk.children_mut()
                && let Some(group) = named_group_mut(children, target)
            {
                return Some(group);
            }
        }
        None
    }

    /// Rewrites the first `id` data chunk at or below `chunks`.
    fn patch_first(
        chunks: &mut [Chunk],
        id: [u8; 4],
        patch: &mut impl FnMut(&mut Vec<u8>),
    ) -> bool {
        for chunk in chunks {
            if chunk.id() == id {
                let mut payload = chunk.data_payload().expect("data chunk").to_vec();
                patch(&mut payload);
                *chunk = Chunk::data(id, payload).expect("patched payload is valid RIFX");
                return true;
            }
            if let Some(children) = chunk.children_mut()
                && patch_first(children, id, patch)
            {
                return true;
            }
        }
        false
    }

    /// Rewrites the first `id` data chunk of native control `target`.
    fn patch_control(
        chunks: &mut [Chunk],
        target: &str,
        id: [u8; 4],
        patch: &mut impl FnMut(&mut Vec<u8>),
    ) -> bool {
        let run = chunks.iter().enumerate().find_map(|(index, chunk)| {
            (chunk.id() == *b"tdmn" && chunk_match_name(chunk) == Some(target)).then(|| {
                let end = chunks[index + 1..]
                    .iter()
                    .position(|candidate| candidate.id() == *b"tdmn")
                    .map_or(chunks.len(), |offset| index + 1 + offset);
                (index + 1, end)
            })
        });
        if let Some((start, end)) = run {
            return patch_first(&mut chunks[start..end], id, patch);
        }
        for chunk in chunks {
            if let Some(children) = chunk.children_mut()
                && patch_control(children, target, id, patch)
            {
                return true;
            }
        }
        false
    }

    fn replace_static_numeric(chunks: &mut [Chunk], target: &str, value: f64) -> bool {
        patch_control(chunks, target, *b"cdat", &mut |payload| {
            assert_eq!(payload.len(), 40, "writer scalar has five value slots");
            payload[..8].copy_from_slice(&value.to_be_bytes());
        })
    }

    /// Marks native control `target`'s expression present and enabled (`tdb4`
    /// flag bytes 120 and 119), so import looks up AE-evaluated samples.
    fn enable_expression(chunks: &mut [Chunk], target: &str) -> bool {
        patch_control(chunks, target, *b"tdb4", &mut |payload| {
            assert_eq!(payload.len(), 124, "native property descriptor");
            payload[120] |= 1;
            payload[119] &= !1;
        })
    }

    fn black_color_tracks(document: &Value) -> Vec<&Value> {
        document["composition"]["dynamics"]["entries"]
            .as_array()
            .expect("animation entries")
            .iter()
            .filter(|entry| {
                entry["target"]["kind"] == "effectProperty"
                    && matches!(
                        entry["target"]["paramName"].as_str(),
                        Some("blackR" | "blackG" | "blackB")
                    )
            })
            .collect()
    }

    fn cosmic_effect_layer(bytes: &[u8], adjustment: bool) -> crate::structure::Layer {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("native shape composition")
        };
        let mut layer = comp.layers[0].clone();
        let container = crate::rifx::Rifx::parse_with(bytes, |_| false).unwrap();
        layer.content = container.chunks().to_vec();
        if adjustment {
            let mut bytes = layer.record.encode();
            bytes[38] |= 2;
            layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
        }
        layer
    }

    fn import_cosmic(layer: &crate::structure::Layer) -> ImportedEffects {
        import(
            &Default::default(),
            1,
            layer,
            [1920, 1080],
            [3840, 2160],
            &mut 999,
            &mut AnimationBudget::default(),
        )
    }

    #[test]
    fn cosmic_native_rgb_invert_and_adjustment_blur_bounds() {
        let invert = cosmic_effect_layer(
            include_bytes!("../../tests/fixtures/effects/cosmic-invert-controls.rifx"),
            false,
        );
        let imported = import_cosmic(&invert);
        assert_eq!(
            imported.effects.len(),
            1,
            "native sparse RGB Invert survives"
        );
        let effect = serde_json::to_value(&imported.effects[0]).unwrap()["effect"].clone();
        assert_eq!(effect["type"], "levels");
        assert_eq!(effect["inputBlack"].as_f64(), Some(0.0));
        assert_eq!(effect["inputWhite"].as_f64(), Some(255.0));
        assert_eq!(effect["gamma"].as_f64(), Some(1.0));
        assert_eq!(effect["outputBlack"].as_f64(), Some(255.0));
        assert_eq!(effect["outputWhite"].as_f64(), Some(0.0));
        let blur = cosmic_effect_layer(
            include_bytes!("../../tests/fixtures/effects/cosmic-adjustment-blur-controls.rifx"),
            true,
        );
        let imported = import_cosmic(&blur);
        assert_eq!(imported.effects.len(), 1);
        let effect = serde_json::to_value(&imported.effects[0]).unwrap()["effect"].clone();
        assert_eq!(effect["repeatEdgePixels"], true);
        assert_eq!(effect["layerSize"], serde_json::json!([3840.0, 2160.0]));
    }

    #[test]
    fn cosmic_invert_rejects_non_rgb_or_non_static_controls() {
        let layer = cosmic_effect_layer(
            include_bytes!("../../tests/fixtures/effects/cosmic-invert-controls.rifx"),
            false,
        );
        let (effects, _) = crate::effects::native::read_effects(&layer.content, [1920.0, 1080.0]);
        let original = &effects[0];
        for (suffix, value, animated, expression) in [
            ("-0001", 2.0, false, false),
            ("-0001", 1.0, true, false),
            ("-0001", 1.0, false, true),
            ("-0002", 0.0, true, false),
            ("-0002", 0.0, false, true),
            ("-0002", -1.0, false, false),
            ("-0002", 101.0, false, false),
            ("-0002", f64::NAN, false, false),
        ] {
            let mut effect = original.clone();
            let numeric = effect
                .parameters
                .iter_mut()
                .find(|p| p.match_name.ends_with(suffix))
                .unwrap()
                .numeric
                .as_mut()
                .unwrap();
            numeric.values = vec![value];
            numeric.animated = animated;
            numeric.expression_enabled = expression;
            assert!(
                crate::effects::special::validate_import(&effect).is_err(),
                "{suffix} {value} {animated} {expression}"
            );
        }
        for blend in [0.0, 25.0, 50.0, 100.0] {
            let mut effect = original.clone();
            effect
                .parameters
                .iter_mut()
                .find(|p| p.match_name.ends_with("-0002"))
                .unwrap()
                .numeric
                .as_mut()
                .unwrap()
                .values = vec![blend];
            assert!(crate::effects::special::validate_import(&effect).is_ok());
            let mapped = crate::effects::mapping::by_native("ADBE Invert").unwrap();
            let values: Vec<_> = mapped
                .fields
                .iter()
                .map(|field| {
                    blend * field.scale.factor([1920.0, 1080.0]).unwrap()
                        + field.offset([1920.0, 1080.0]).unwrap()
                })
                .collect();
            assert_eq!(values, vec![255.0 - 2.55 * blend, 2.55 * blend]);
        }
    }

    #[test]
    fn cosmic_non_rgb_invert_omits_only_effect_without_consuming_id() {
        fn replace_channel(chunks: &mut [Chunk]) -> bool {
            for index in 0..chunks.len() {
                if chunks[index].id() == *b"tdmn"
                    && chunk_match_name(&chunks[index]) == Some("ADBE Invert-0001")
                    && let Some(pard) = chunks[index + 1..].iter_mut().find(|c| c.id() == *b"pard")
                {
                    let mut bytes = pard.data_payload().unwrap().to_vec();
                    assert_eq!(&bytes[12..16], &7_u32.to_be_bytes());
                    bytes[56..60].copy_from_slice(&2_u32.to_be_bytes());
                    *pard = Chunk::data(*b"pard", bytes).unwrap();
                    return true;
                }
                if let Some(children) = chunks[index].children_mut()
                    && replace_channel(children)
                {
                    return true;
                }
            }
            false
        }
        let mut layer = cosmic_effect_layer(
            include_bytes!("../../tests/fixtures/effects/cosmic-invert-controls.rifx"),
            false,
        );
        assert!(replace_channel(&mut layer.content));
        let mut id = 999;
        let imported = import(
            &Default::default(),
            1,
            &layer,
            [1920, 1080],
            [3840, 2160],
            &mut id,
            &mut AnimationBudget::default(),
        );
        assert!(imported.effects.is_empty());
        assert_eq!(id, 999);
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("static RGB Channel")
                    && warning.contains("owner and other effects retained"))
        );
    }

    #[test]
    fn cosmic_blur_ordinary_owner_keeps_source_local_bounds() {
        let layer = cosmic_effect_layer(
            include_bytes!("../../tests/fixtures/effects/cosmic-adjustment-blur-controls.rifx"),
            false,
        );
        let imported = import_cosmic(&layer);
        assert_eq!(imported.effects.len(), 1);
        let effect = serde_json::to_value(&imported.effects[0]).unwrap();
        assert_eq!(
            effect["effect"]["layerSize"],
            serde_json::json!([1920.0, 1080.0])
        );
    }

    #[test]
    fn rejected_effect_does_not_consume_occurrence_identity() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .expect("pinned Adobe-native Shape-owner Gaussian Blur source");
        let ItemKind::Composition(composition) = &project
            .item(1)
            .expect("pinned Shape-owner Gaussian Blur composition")
            .kind
        else {
            panic!("pinned target must be a composition")
        };
        let mut valid = composition
            .layers
            .first()
            .expect("pinned Shape-owner Gaussian Blur layer")
            .clone();

        // The native fixture stores this control sparsely in its parameter table.
        // Build an explicit positive control locally so this CPU regression can
        // mutate only its static value without claiming an additional Adobe oracle.
        let mut gaussian =
            crate::writer::effects::new_effect("ADBE Gaussian Blur 2", true, [120.0, 80.0])
                .expect("canonical Gaussian effect");
        gaussian
            .properties
            .iter_mut()
            .find(|property| property.match_name == "ADBE Gaussian Blur 2-0001")
            .expect("canonical blurriness control")
            .values = vec![25.0];
        let generated =
            crate::writer::effects::effect_parade(&[gaussian], valid.record.id(), [120.0, 80.0])
                .expect("writer-generated explicit Effect Parade");
        *named_group_mut(&mut valid.content, "ADBE Effect Parade")
            .expect("fixture Effect Parade") = generated
            .children()
            .expect("generated Effect Parade group")
            .to_vec();

        let mut invalid = valid.clone();
        assert!(
            replace_static_numeric(&mut invalid.content, "ADBE Gaussian Blur 2-0001", -1.0,),
            "generated control must contain explicit blurriness payload"
        );

        let mut next_id = 9_999;
        let mut budget = AnimationBudget::default();
        let rejected = import(
            &Default::default(),
            1,
            &invalid,
            [120, 80],
            [320, 180],
            &mut next_id,
            &mut budget,
        );
        assert!(
            rejected.effects.is_empty(),
            "negative blur must be rejected"
        );
        assert_eq!(
            next_id, 9_999,
            "an omitted occurrence must not consume the shared identity budget"
        );

        let imported = import(
            &Default::default(),
            1,
            &valid,
            [120, 80],
            [320, 180],
            &mut next_id,
            &mut budget,
        );
        assert_eq!(imported.effects.len(), 1, "valid later sibling survives");
        assert_eq!(next_id, 10_000, "identity commits with the valid effect");
        let beyond_former_limit = import(
            &Default::default(),
            1,
            &valid,
            [120, 80],
            [320, 180],
            &mut next_id,
            &mut budget,
        );
        assert_eq!(beyond_former_limit.effects.len(), 1);
        assert_eq!(next_id, 10_001);
        next_id = u64::MAX - 1;
        for expected in [1, 0] {
            let imported = import(
                &Default::default(),
                1,
                &valid,
                [120, 80],
                [320, 180],
                &mut next_id,
                &mut budget,
            );
            assert_eq!(imported.effects.len(), expected);
            assert_eq!(next_id, u64::MAX);
        }
    }

    #[test]
    fn effect_parameter_planes_match_destination_runtime_contracts() {
        let native = [120, 80];
        let group = [320, 180];
        for name in ["ADBE Bulge", "ADBE Ripple", "ADBE Wave Warp"] {
            assert_eq!(parameter_plane_size(name, native, group), group, "{name}");
        }
        for name in ["ADBE Corner Pin", "ADBE Tile", "ADBE Twirl"] {
            assert_eq!(parameter_plane_size(name, native, group), native, "{name}");
        }
    }

    #[test]
    fn effect_color_components_are_admitted_atomically() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects/animated_catalog.aep"
        ))
        .expect("Adobe-authored animated catalog");
        let full = to_structural_fx_document(&project, Some(408))
            .expect("unlimited animated Tint import")
            .document
            .to_json_value()
            .expect("editable FX JSON");
        let tracks = black_color_tracks(&full);
        assert_eq!(tracks.len(), 3, "full color mapping");

        // One independently admitted scalar track fits this allowance. The
        // three components sourced from the same native color must not.
        let one_track_limit = serde_json::to_vec(tracks[0])
            .expect("persisted animation entry")
            .len()
            + 1;
        let limited =
            to_structural_fx_document_with_animation_limit(&project, Some(408), one_track_limit)
                .expect("budget-limited animated Tint import");
        let limited_json = limited
            .document
            .to_json_value()
            .expect("editable limited FX JSON");
        assert_eq!(
            black_color_tracks(&limited_json).len(),
            0,
            "a native color must never retain only some component motion"
        );
        assert!(
            limited.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains("coupled animation target set omitted")),
            "atomic budget rejection must be diagnosed"
        );
    }
    #[test]
    fn native_effect_shadow_softness_uses_sigma_units_for_static_and_animated_controls() {
        for bytes in [
            include_bytes!("../../tests/fixtures/effects_coverage/native_static_controls.aep")
                .as_slice(),
            include_bytes!("../../tests/fixtures/effects/animated_catalog.aep").as_slice(),
        ] {
            let p = read_project(bytes).unwrap();
            let (comp, layer, source) = p
                .items
                .iter()
                .filter_map(|i| {
                    if let ItemKind::Composition(c) = &i.kind {
                        Some(c)
                    } else {
                        None
                    }
                })
                .find_map(|comp| {
                    comp.layers.iter().find_map(|layer| {
                        let (effects, _) = crate::effects::native::read_effects(
                            &layer.content,
                            [f64::from(comp.width), f64::from(comp.height)],
                        );
                        effects
                            .into_iter()
                            .find(|e| e.match_name == "ADBE Drop Shadow")
                            .map(|e| (comp, layer, e))
                    })
                })
                .unwrap();
            let numeric = source
                .parameters
                .iter()
                .find(|p| p.match_name == "ADBE Drop Shadow-0005")
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            let imported = import(
                &Default::default(),
                1,
                layer,
                [comp.width, comp.height],
                [comp.width, comp.height],
                &mut 1000,
                &mut AnimationBudget::default(),
            );
            let (id, shadow) =
                imported
                    .effects
                    .iter()
                    .find_map(|e| match e.data() {
                        fx_schema::EffectData::Identified {
                            id,
                            effect:
                                fx_schema::EffectPayload::Known(fx_schema::LayerEffect::DropShadow(
                                    shadow,
                                )),
                            ..
                        } => Some((*id, shadow)),
                        _ => None,
                    })
                    .unwrap();
            let initial = numeric
                .values
                .first()
                .copied()
                .or_else(|| {
                    numeric
                        .keyframes
                        .first()
                        .and_then(|k| k.values.first().copied())
                })
                .unwrap();
            assert_eq!(shadow.blur_radius.value(), initial / 2.);
            if !numeric.keyframes.is_empty() {
                let target = fx_schema::PropertyTarget::effect_param(id, "blurRadius");
                let track = imported
                    .animations
                    .iter()
                    .find(|e| e.target == target)
                    .unwrap()
                    .animator
                    .keyframe_track()
                    .unwrap();
                for (key, native) in track.keyframes().iter().zip(&numeric.keyframes) {
                    assert_eq!(
                        *key.value(),
                        fx_schema::PropertyValue::Float(native.values[0] / 2.)
                    );
                }
            }
        }
        // Independent native scalar keys exercise the same affine field used
        // by effect import and inverse export, without generated ease values.
        let scalar = read_project(include_bytes!(
            "../../tests/fixtures/properties/property_1D_opacity.aep"
        ))
        .unwrap();
        let numeric = scalar
            .items
            .iter()
            .filter_map(|i| {
                if let ItemKind::Composition(c) = &i.kind {
                    c.layers.first()
                } else {
                    None
                }
            })
            .find_map(|layer| {
                crate::properties::read_transform(&layer.content)
                    .unwrap()
                    .into_iter()
                    .find(|p| p.match_name == "ADBE Opacity")
            })
            .unwrap()
            .numeric
            .unwrap();
        let mapping = crate::effects::mapping::by_native("ADBE Drop Shadow").unwrap();
        let field = &mapping.fields[0];
        let scale = field.scale.factor([320., 180.]).unwrap();
        let target = super::super::animation::NumericAnimationTarget::float(
            fx_schema::PropertyTarget::effect_param(fx_schema::EffectId::new(7), field.param),
            field.component,
            scale,
        );
        let (entries, warnings) = super::super::animation::numeric_entries(
            "generic animated Softness",
            &numeric,
            &[target],
            super::super::animation::NumericAnimationClock::source_local(),
            &mut AnimationBudget::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let track = entries[0].animator.keyframe_track().unwrap();
        for (key, native) in track.keyframes().iter().zip(&numeric.keyframes) {
            let fx_schema::PropertyValue::Float(value) = key.value() else {
                panic!()
            };
            assert_eq!(*value, native.values[0] / 2.);
            assert_eq!(
                *value / scale,
                native.values[0],
                "shared writer inverse restores native units"
            );
        }
    }

    #[test]
    fn native_static_box_blur_keeps_ordinal_and_source_derived_variance() {
        let p = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
            panic!()
        };
        let parsed = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/native-static-box-blur.rifx"),
            |_| false,
        )
        .unwrap();
        for (chunk, radius) in parsed.chunks().iter().zip([50_f64, 11., 60., 122.]) {
            let mut layer = comp.layers[0].clone();
            layer.content = chunk.children().unwrap().to_vec();
            layer.record = crate::schema::layer_records::LayerRecord::decode(
                crate::properties::data(&layer.content, *b"ldta").unwrap(),
            )
            .unwrap();
            let imported = import_cosmic(&layer);
            let (native, _) = crate::effects::native::read_effects(&layer.content, [1920., 1080.]);
            let expected: Vec<_> = native
                .iter()
                .filter_map(|source| {
                    if source.match_name == "ADBE Box Blur2" {
                        Some("gaussianBlur")
                    } else {
                        crate::effects::mapping::by_native(&source.match_name).map(|m| m.fx_type)
                    }
                })
                .collect();
            let actual: Vec<_> = imported
                .effects
                .iter()
                .map(|record| {
                    let fx_schema::EffectData::Identified {
                        effect: fx_schema::EffectPayload::Known(effect),
                        ..
                    } = record.data()
                    else {
                        panic!()
                    };
                    serde_json::to_value(effect).unwrap()["type"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                })
                .collect();
            assert_eq!(actual, expected, "effect order: {:?}", imported.warnings);
            let effect = imported
                .effects
                .iter()
                .find_map(|e| match e.data() {
                    fx_schema::EffectData::Identified {
                        effect:
                            fx_schema::EffectPayload::Known(fx_schema::LayerEffect::GaussianBlur {
                                blurriness,
                                repeat_edge_pixels,
                                layer_size,
                            }),
                        ..
                    } => Some((blurriness, repeat_edge_pixels, layer_size)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Box Blur missing: {:?}", imported.warnings));
            assert!(
                (effect.0.value() - 4.0_f64 * (2. * radius * (radius + 1.) / 3.).sqrt()).abs()
                    < 1e-10
            );
            assert_eq!(*effect.1, Some(true));
            assert_eq!(*effect.2, Some((1920., 1080.)));
        }
    }
    #[test]
    fn native_fractal_normal_and_constant_multiply_are_editable_effects() {
        use super::{ImportContext, import_with_context};
        use fx_schema::{EffectData, EffectPayload, LayerEffect};
        use std::collections::HashMap;
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects_coverage/native_static_controls.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &project.item(79).unwrap().kind else {
            panic!()
        };
        let mut source = project.items[0].clone();
        source.kind = ItemKind::Footage;
        source.solid = Some(Ok(crate::structure::SolidSource {
            width: 1920,
            height: 1080,
            pixel_aspect: (1, 1),
            color: [0.; 3],
        }));
        let fixture = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/native-fractal-noise-controls.rifx"),
            |_| false,
        )
        .unwrap();
        for (chunk, constant) in [(&fixture.chunks()[0], false), (&fixture.chunks()[2], true)] {
            let mut layer = comp.layers[0].clone();
            layer.content = chunk.children().unwrap().to_vec();
            layer.record = crate::schema::layer_records::LayerRecord::decode(
                crate::properties::data(&layer.content, *b"ldta").unwrap(),
            )
            .unwrap();
            layer.name = "Unrelated procedural graphic".into();
            let items = HashMap::from([(layer.record.source_id(), &source)]);
            let imported = import_with_context(
                ImportContext {
                    evaluations: &Default::default(),
                    composition_id: 1,
                    items: Some(&items),
                },
                &layer,
                [1920, 1080],
                [3840, 2160],
                &mut 999,
                &mut AnimationBudget::default(),
            );
            let effect = imported
                .effects
                .iter()
                .find_map(|e| match e.data() {
                    EffectData::Identified {
                        effect: EffectPayload::Known(effect),
                        ..
                    } if matches!(
                        (effect, constant),
                        (LayerEffect::TurbulentNoise { .. }, false)
                            | (LayerEffect::Exposure { .. }, true)
                    ) =>
                    {
                        Some(effect)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("native Fractal missing: {:?}", imported.warnings));
            if let LayerEffect::TurbulentNoise {
                scale,
                contrast,
                brightness,
                evolution,
                complexity,
                ..
            } = effect
            {
                assert_eq!(
                    (*scale, *contrast, *brightness, *evolution, *complexity),
                    (Some(800.), Some(44.), Some(-40.), Some(-112.), Some(1.))
                );
                assert_eq!(imported.native_ordinals[0], 1);
            } else if let LayerEffect::Exposure {
                exposure,
                offset,
                gamma_correction,
            } = effect
            {
                assert_eq!(
                    (*exposure, *offset, *gamma_correction),
                    (Some(-1.), Some(0.), Some(1.))
                );
                assert_eq!(*imported.native_ordinals.last().unwrap(), 3);
            }
            if !constant {
                let mut next = 999;
                let mut budget = AnimationBudget::default();
                let rejected = import_with_context(
                    ImportContext {
                        evaluations: &Default::default(),
                        composition_id: 1,
                        items: None,
                    },
                    &layer,
                    [1920, 1080],
                    [3840, 2160],
                    &mut next,
                    &mut budget,
                );
                assert_eq!(next, 999);
                assert_eq!(budget.used(), 0);
                assert!(
                    rejected.effects.is_empty()
                        && rejected.animations.is_empty()
                        && rejected.native_ordinals.is_empty()
                );
            }
            assert!(
                imported
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("Fractal Noise")
                        && warning.contains("approximat"))
            );
        }
    }

    const CATALOG: &[u8] = include_bytes!("../../tests/fixtures/effects/catalog.aep");
    /// Adobe-authored 320x180 catalog composition whose owner layer hosts Ripple.
    const RIPPLE_COMPOSITION: u32 = 310;
    const PLANE: [u16; 2] = [320, 180];
    const RIPPLE_ID: u64 = 100;

    /// One FX Wave Height key: layer time in ms, value and incoming easing.
    type Key = (i64, f64, PropertyKeyframeEasing);

    /// Catalog composition 310's owner with its Effect Parade replaced by the
    /// given writer-generated effects. The Adobe occurrence stores Wave Height
    /// sparsely, so explicit controls are built locally; these CPU regressions
    /// claim no additional Adobe oracle.
    fn catalog_owner(effects: &[NativeEffect]) -> Layer {
        let project = read_project(CATALOG).expect("Adobe-authored effect catalog");
        let ItemKind::Composition(composition) = &project
            .item(RIPPLE_COMPOSITION)
            .expect("catalog Ripple composition")
            .kind
        else {
            panic!("catalog Ripple target must be a composition")
        };
        let mut owner = composition
            .layers
            .first()
            .expect("catalog Ripple owner")
            .clone();
        let generated = effect_parade(effects, owner.record.id(), PLANE.map(f64::from))
            .expect("writer-generated explicit Effect Parade");
        *named_group_mut(&mut owner.content, "ADBE Effect Parade")
            .expect("catalog Effect Parade") = generated
            .children()
            .expect("generated Effect Parade group")
            .to_vec();
        owner
    }

    /// A native effect with the given static control values.
    fn native_effect(match_name: &str, controls: &[(&str, f64)]) -> NativeEffect {
        let mut effect =
            new_effect(match_name, true, PLANE.map(f64::from)).expect("canonical native effect");
        for (control, value) in controls {
            effect
                .properties
                .iter_mut()
                .find(|property| property.match_name.ends_with(control))
                .expect("canonical control")
                .values = vec![*value];
        }
        effect
    }

    /// Native Ripple with Wave Width and Wave Height in pixels.
    fn native_ripple(
        wave_width: f64,
        wave_height: f64,
        keys: Option<NumericTrack>,
    ) -> NativeEffect {
        let mut effect = native_effect(
            "ADBE Ripple",
            &[("-0005", wave_width), ("-0006", wave_height)],
        );
        effect
            .properties
            .iter_mut()
            .find(|property| property.match_name == "ADBE Ripple-0006")
            .expect("Wave Height")
            .animation = keys;
        effect
    }

    /// A validated version-2 sidecar for the owner's Wave Height, bound to the
    /// pinned source bytes; times are native seconds around the 1ms grid.
    fn wave_height_samples(owner: &Layer, times: &[f64], heights: &[f64]) -> ExpressionSamples {
        let sidecar = serde_json::json!({
            "version": 2,
            "source_sha256": format!("{:x}", Sha256::digest(CATALOG)),
            "sample_interval_ms": 1,
            "capture_scope": {
                "mode": "selected_composition",
                "root_composition_id": RIPPLE_COMPOSITION,
            },
            "properties": [{
                "composition_id": RIPPLE_COMPOSITION,
                "layer_id": owner.record.id(),
                "property": {"kind": "effect", "index": 1, "match_name": "ADBE Ripple-0006"},
                "start_ms": 0,
                "sample_times_seconds": times,
                "values": heights.iter().map(|height| [height]).collect::<Vec<_>>(),
            }],
            "errors": [],
        });
        ExpressionSamples::from_json_for_source(
            &serde_json::to_vec(&sidecar).expect("sidecar JSON"),
            CATALOG,
        )
        .expect("admissible AE capture")
    }

    fn import_ripple(
        owner: &Layer,
        samples: &ExpressionSamples,
        plane: [u16; 2],
        budget: &mut AnimationBudget,
    ) -> ImportedEffects {
        let mut next_id = RIPPLE_ID;
        import(
            samples,
            RIPPLE_COMPOSITION,
            owner,
            plane,
            plane,
            &mut next_id,
            budget,
        )
    }

    fn payload(imported: &ImportedEffects, kind: &str) -> Value {
        imported
            .effects
            .iter()
            .map(|effect| serde_json::to_value(effect).expect("effect JSON"))
            .find(|effect| effect["effect"]["type"] == kind)
            .unwrap_or_else(|| panic!("imported {kind}: {:?}", imported.warnings))["effect"]
            .clone()
    }

    fn keys(entry: &AnimationGraphEntry) -> Vec<Key> {
        entry
            .animator
            .keyframe_track()
            .expect("editable keys")
            .keyframes()
            .iter()
            .map(|key| {
                let PropertyValue::Float(value) = key.value() else {
                    panic!("scalar Wave Height key")
                };
                (key.layer_time().as_millis(), *value, key.easing())
            })
            .collect()
    }

    fn amplitude_keys(imported: &ImportedEffects) -> Option<Vec<Key>> {
        let target = PropertyTarget::effect_param(EffectId::new(RIPPLE_ID), "amplitude");
        imported
            .animations
            .iter()
            .find(|entry| entry.target == target)
            .map(keys)
    }

    /// The ordinary plane-width lowering of the sidecar's Wave Height samples.
    fn ordinary_lowering(samples: &ExpressionSamples) -> Vec<Key> {
        let target = NumericAnimationTarget::float(
            PropertyTarget::effect_param(EffectId::new(RIPPLE_ID), "amplitude"),
            0,
            f64::from(PLANE[0]).recip(),
        );
        let (entries, warnings) = animation::evaluated_numeric_entries(
            "ADBE Ripple-0006",
            &samples.properties()[0],
            &[target],
            &[0.0],
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries.len(), 1, "{warnings:?}");
        keys(&entries[0])
    }

    /// Asserts `reduced` is `ordinary` with every value multiplied by one
    /// factor, keeping key selection, times and easing; returns the factor.
    fn assert_uniformly_reduced(reduced: &[Key], ordinary: &[Key]) -> f64 {
        assert_eq!(reduced.len(), ordinary.len(), "ordinary key selection");
        let peak = |keys: &[Key]| keys.iter().map(|key| key.1.abs()).fold(0.0, f64::max);
        let factor = peak(reduced) / peak(ordinary);
        for (reduced, ordinary) in reduced.iter().zip(ordinary) {
            assert_eq!(
                (reduced.0, reduced.2),
                (ordinary.0, ordinary.2),
                "time/easing"
            );
            assert!(
                (reduced.1 - ordinary.1 * factor).abs() <= 1e-15,
                "{reduced:?} is not {ordinary:?} times {factor}"
            );
        }
        factor
    }

    /// `|amplitude| * frequency`; FX rings fold over above 1, and import
    /// limits it to the 1.25 strength limit.
    fn fold_over(amplitude: f64, ripple: &Value) -> f64 {
        amplitude.abs() * ripple["frequency"].as_f64().expect("FX frequency")
    }

    fn limit_warnings(warnings: &[String]) -> usize {
        warnings
            .iter()
            .filter(|warning| {
                warning.starts_with("Effect ADBE Ripple / ADBE Ripple-0006:")
                    && warning.contains("strength limit")
            })
            .count()
    }

    #[test]
    fn evaluated_ripple_amplitude_keeps_its_ordinary_fit_before_one_uniform_reduction() {
        // 320px plane, 20px Wave Width. The 200.5px midpoint deviates 0.5px
        // (0.0015625 FX units) from linear, beyond the 0.001 fitting cap.
        let mut owner = catalog_owner(&[native_ripple(20.0, 20.0, None)]);
        assert!(enable_expression(&mut owner.content, "ADBE Ripple-0006"));
        let samples = wave_height_samples(&owner, &[0.0, 0.001, 0.002], &[0.0, 200.5, 400.0]);
        let imported = import_ripple(&owner, &samples, PLANE, &mut AnimationBudget::default());
        let ripple = payload(&imported, "ripple");
        let reduced = amplitude_keys(&imported)
            .unwrap_or_else(|| panic!("evaluated Wave Height track: {:?}", imported.warnings));
        let ordinary = ordinary_lowering(&samples);
        assert_eq!(
            ordinary.iter().map(|key| key.0).collect::<Vec<_>>(),
            [0, 1, 2],
            "the ordinary fit keeps the midpoint key"
        );
        let factor = assert_uniformly_reduced(&reduced, &ordinary);
        assert!((fold_over(reduced[2].1, &ripple) - 1.25).abs() < 1e-12);
        assert!((reduced[1].1 / reduced[2].1 - 200.5 / 400.0).abs() < 1e-12);
        assert!(
            (ripple["amplitude"].as_f64().unwrap() - 20.0 / 320.0 * factor).abs() < 1e-15,
            "the retained base takes the same factor"
        );
        assert_eq!(
            limit_warnings(&imported.warnings),
            1,
            "{:?}",
            imported.warnings
        );
    }

    #[test]
    fn evaluated_ripple_amplitude_is_limited_at_its_extrapolated_emitted_keys() {
        // Captures 0 and 20px at 0.25 and 1.25ms are within the half-ms native
        // clock bound; integer-ms lowering extrapolates them to -5, 15 and 35px.
        let mut owner = catalog_owner(&[native_ripple(20.0, 20.0, None)]);
        assert!(enable_expression(&mut owner.content, "ADBE Ripple-0006"));
        let samples = wave_height_samples(&owner, &[0.00025, 0.00125], &[0.0, 20.0]);
        let imported = import_ripple(&owner, &samples, PLANE, &mut AnimationBudget::default());
        let ripple = payload(&imported, "ripple");
        let reduced = amplitude_keys(&imported)
            .unwrap_or_else(|| panic!("evaluated Wave Height track: {:?}", imported.warnings));
        let ordinary = ordinary_lowering(&samples);
        assert_eq!(ordinary.iter().map(|key| key.0).collect::<Vec<_>>(), [0, 2]);
        assert!((ordinary[1].1 - 35.0 / 320.0).abs() < 1e-12, "{ordinary:?}");
        assert_uniformly_reduced(&reduced, &ordinary);
        assert!(
            (fold_over(reduced[1].1, &ripple) - 1.25).abs() < 1e-12,
            "the emitted 35px key, not the 20px capture, meets the limit: {reduced:?}"
        );
        assert!((reduced[0].1 / reduced[1].1 + 5.0 / 35.0).abs() < 1e-12);
    }

    #[test]
    fn retained_ripple_base_counts_toward_the_reduction_and_is_the_failed_lowering_fallback() {
        // A 400px authored Wave Height beside 2px AE-evaluated samples.
        let mut owner = catalog_owner(&[native_ripple(20.0, 400.0, None)]);
        assert!(enable_expression(&mut owner.content, "ADBE Ripple-0006"));
        let samples = wave_height_samples(&owner, &[0.0, 0.001, 0.002], &[2.0, 2.0, 2.0]);
        let imported = import_ripple(&owner, &samples, PLANE, &mut AnimationBudget::default());
        let ripple = payload(&imported, "ripple");
        let base = ripple["amplitude"].as_f64().unwrap();
        assert!(
            (fold_over(base, &ripple) - 1.25).abs() < 1e-12,
            "the base sets the peak"
        );
        let reduced = amplitude_keys(&imported)
            .unwrap_or_else(|| panic!("evaluated Wave Height track: {:?}", imported.warnings));
        let factor = assert_uniformly_reduced(&reduced, &ordinary_lowering(&samples));
        assert!(
            (base - 400.0 / 320.0 * factor).abs() < 1e-15,
            "the 2px keys take the base's factor"
        );

        // When evaluated lowering fails, the same limited base is the static fallback.
        let mut budget = AnimationBudget::with_limit(1);
        let fallback = import_ripple(&owner, &samples, PLANE, &mut budget);
        assert!(amplitude_keys(&fallback).is_none());
        assert!(
            fallback
                .warnings
                .iter()
                .any(|warning| warning.contains("ADBE Ripple-0006")
                    && warning.contains("static authored/default value retained")),
            "{:?}",
            fallback.warnings
        );
        assert_eq!(
            payload(&fallback, "ripple")["amplitude"],
            ripple["amplitude"]
        );
        assert_eq!(limit_warnings(&fallback.warnings), 1);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn keyed_ripple_amplitude_keeps_nonzero_speed_bezier_easing_and_exact_charge() {
        let key = |time_millis, height, easing| NumericKeyframe {
            time_millis,
            values: vec![height],
            easing: vec![easing],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        let bezier = NumericTrack {
            keys: vec![
                key(0, 2.0, KeyframeEasing::Linear),
                key(
                    1_000,
                    12.0,
                    KeyframeEasing::CubicBezier {
                        x1: 0.25,
                        y1: 0.4,
                        x2: 0.6,
                        y2: 0.85,
                    },
                ),
            ],
        };
        // The sibling's 40px Wave Height keeps its ordinary plane-height units.
        let wave_warp = native_effect("ADBE Wave Warp", &[("-0002", 40.0), ("-0003", 20.0)]);
        // The 12px peak is within 1.25 / frequency at a 100px Wave Width
        // (19.9px), but not at 20px (4.0px).
        let within = catalog_owner(&[
            native_ripple(100.0, 2.0, Some(bezier.clone())),
            wave_warp.clone(),
        ]);
        let owner = catalog_owner(&[native_ripple(20.0, 2.0, Some(bezier)), wave_warp]);
        let (native, _) = native::read_effects(&owner.content, PLANE.map(f64::from));
        let height = native[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Ripple-0006")
            .and_then(|parameter| parameter.numeric.as_ref().ok())
            .expect("keyed native Wave Height");
        assert!(
            height.keyframes[0].out_speed[0] != 0.0 && height.keyframes[1].in_speed[0] != 0.0,
            "nonzero-speed native Bezier source: {height:?}"
        );

        let mut ordinary_budget = AnimationBudget::default();
        let ordinary = import_ripple(
            &within,
            &ExpressionSamples::default(),
            PLANE,
            &mut ordinary_budget,
        );
        assert_eq!(limit_warnings(&ordinary.warnings), 0);
        let ordinary_keys = amplitude_keys(&ordinary).expect("keyed Wave Height");
        let PropertyKeyframeEasing::CubicBezier { y1, y2, .. } = ordinary_keys[1].2 else {
            panic!("native Bezier must stay an editable cubic ease: {ordinary_keys:?}")
        };
        assert!(y1 != 0.0 && y2 != 1.0, "nonzero end speeds: {y1}, {y2}");

        let mut budget = AnimationBudget::default();
        let imported = import_ripple(&owner, &ExpressionSamples::default(), PLANE, &mut budget);
        let ripple = payload(&imported, "ripple");
        let reduced = amplitude_keys(&imported).expect("keyed Wave Height");
        let factor = assert_uniformly_reduced(&reduced, &ordinary_keys);
        assert!((fold_over(reduced[1].1, &ripple) - 1.25).abs() < 1e-12);
        assert!((ripple["amplitude"].as_f64().unwrap() - 2.0 / 320.0 * factor).abs() < 1e-15);
        let warp = payload(&imported, "waveWarp");
        assert!((warp["waveHeight"].as_f64().unwrap() - 40.0 / 180.0).abs() < 1e-15);
        assert!((warp["waveWidth"].as_f64().unwrap() - 320.0 / 20.0).abs() < 1e-12);
        let charged: usize = imported
            .animations
            .iter()
            .map(|entry| committed_entry_reservation_bytes(entry).expect("entry size"))
            .sum();
        assert_eq!(budget.used(), charged, "rescaled keys are charged exactly");

        // An allowance holding only the ordinary track cannot take the longer
        // rescaled values: the track is omitted and the limited base retained.
        let mut tight = AnimationBudget::with_limit(ordinary_budget.used());
        let omitted = import_ripple(&owner, &ExpressionSamples::default(), PLANE, &mut tight);
        assert!(amplitude_keys(&omitted).is_none(), "{:?}", omitted.warnings);
        assert_eq!(
            payload(&omitted, "ripple")["amplitude"],
            ripple["amplitude"]
        );
        assert!(
            omitted
                .warnings
                .iter()
                .any(|warning| warning.contains("amplitude animation omitted")),
            "{:?}",
            omitted.warnings
        );
        assert_eq!(tight.used(), 0);
    }

    #[test]
    fn ripple_reduction_depends_on_wave_height_over_width_not_the_plane() {
        // 2pi * 20/20 exceeds 1.25 on every plane; 0px, 2pi * 3/40 and
        // 2pi * 3.5/20 (beyond the fold-over threshold of 1) never do.
        for plane in [[320, 180], [640, 480], [1080, 1920], [3840, 2160]] {
            let width = f64::from(plane[0]);
            let import_static = |wave_width, wave_height| {
                let owner = catalog_owner(&[native_ripple(wave_width, wave_height, None)]);
                let imported = import_ripple(
                    &owner,
                    &ExpressionSamples::default(),
                    plane,
                    &mut AnimationBudget::default(),
                );
                (payload(&imported, "ripple"), imported.warnings)
            };
            let (strong, warnings) = import_static(20.0, 20.0);
            assert_eq!(limit_warnings(&warnings), 1, "{plane:?}: {warnings:?}");
            assert!(
                (strong["frequency"].as_f64().unwrap() - std::f64::consts::TAU * width / 20.0)
                    .abs()
                    < 1e-9
            );
            assert!(
                (fold_over(strong["amplitude"].as_f64().unwrap(), &strong) - 1.25).abs() < 1e-12
            );
            let (zero, warnings) = import_static(20.0, 0.0);
            assert_eq!(limit_warnings(&warnings), 0, "{plane:?}: {warnings:?}");
            assert_eq!(zero["amplitude"].as_f64(), Some(0.0));
            for field in ["centerX", "centerY", "phase", "frequency"] {
                assert_eq!(strong[field], zero[field], "{plane:?}/{field}");
            }
            let (weak, warnings) = import_static(40.0, 3.0);
            assert_eq!(limit_warnings(&warnings), 0, "{plane:?}: {warnings:?}");
            assert!((weak["amplitude"].as_f64().unwrap() - 3.0 / width).abs() < 1e-15);
            let (folding, warnings) = import_static(20.0, 3.5);
            assert_eq!(limit_warnings(&warnings), 0, "{plane:?}: {warnings:?}");
            let amplitude = folding["amplitude"].as_f64().unwrap();
            assert!((amplitude - 3.5 / width).abs() < 1e-15);
            assert!((1.0..1.25).contains(&fold_over(amplitude, &folding)));
        }
    }
}
