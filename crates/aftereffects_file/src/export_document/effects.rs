//! Effect-local lowering: an unsupported control never discards its owning layer.

use std::collections::BTreeSet;

use fx_schema::animator::{AnimationGraphEntry, AnimatorData};
use fx_schema::{EffectData, EffectId, EffectPayload, EffectRecord, PropertyTarget};

use super::{NativeTrack, scalar_track};
use crate::{
    effects::{catalog, mapping, special},
    writer::{self, KeyframeEasing, NumericKeyframe, NumericTrack},
};

pub(super) struct LoweredEffects {
    pub effects: Vec<writer::effects::NativeEffect>,
    pub styles: Vec<crate::layer_styles::NativeLayerStyle>,
    pub warnings: Vec<String>,
}

/// Classifies only absent mappings, not size-dependent or parameter failures.
/// Hierarchy planning must not reject geometry for an effect we already omit.
pub(super) fn unmapped_warning(record: &EffectRecord) -> Option<String> {
    let payload = match record.data() {
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
    };
    let EffectPayload::Known(effect) = payload else {
        return Some(
            "Unknown FX effect payload omitted; owner and remaining effects retained".into(),
        );
    };
    let kind = catalog::effect_type(effect);
    if super::layer_styles::is_layer_style(effect) || mapping::by_fx(kind).is_some() {
        return None;
    }
    Some(format!(
        "Effect {kind}: {}; omitted, owner retained",
        catalog::unsupported_reason(effect)
            .unwrap_or("no verified equivalent native effect/control representation")
    ))
}

pub(super) fn lower(
    records: &[EffectRecord],
    dynamics: &[AnimationGraphEntry],
    size: [f64; 2],
) -> LoweredEffects {
    let mut result = LoweredEffects {
        effects: Vec::new(),
        styles: Vec::new(),
        warnings: Vec::new(),
    };
    let mut last_style_rank = None;
    for record in records {
        let (id, enabled, payload) = match record.data() {
            EffectData::Identified {
                id,
                enabled,
                effect,
            } => (Some(*id), *enabled, effect),
            EffectData::Legacy(effect) => (None, true, effect),
        };
        let EffectPayload::Known(effect) = payload else {
            result.warnings.push(
                "Unknown FX effect payload omitted; owner and remaining effects retained".into(),
            );
            continue;
        };
        let kind = catalog::effect_type(effect);
        if super::layer_styles::is_layer_style(effect) {
            let key = super::layer_styles::style_key(effect);
            let duplicate = result.styles.iter().any(|style| {
                matches!(
                    (key, style),
                    (
                        "dropShadow",
                        crate::layer_styles::NativeLayerStyle::DropShadow(_)
                    ) | (
                        "outerGlow",
                        crate::layer_styles::NativeLayerStyle::OuterGlow(_)
                    ) | ("stroke", crate::layer_styles::NativeLayerStyle::Stroke(_))
                        | (
                            "colorOverlay",
                            crate::layer_styles::NativeLayerStyle::ColorOverlay(_)
                        )
                        | (
                            "gradientOverlay",
                            crate::layer_styles::NativeLayerStyle::GradientOverlay(_)
                        )
                        | (
                            "innerShadow",
                            crate::layer_styles::NativeLayerStyle::InnerShadow(_)
                        )
                        | (
                            "innerGlow",
                            crate::layer_styles::NativeLayerStyle::InnerGlow(_)
                        )
                        | ("satin", crate::layer_styles::NativeLayerStyle::Satin(_))
                        | (
                            "bevelEmboss",
                            crate::layer_styles::NativeLayerStyle::BevelEmboss(_)
                        )
                )
            });
            if duplicate {
                result.warnings.push(format!(
                    "Layer Style {}: duplicate FX occurrence omitted; first occurrence retained because AE permits one native style of this kind per layer",
                    super::layer_styles::style_label(effect)
                ));
                continue;
            }
            let rank = super::layer_styles::style_rank(effect);
            if last_style_rank.is_some_and(|previous| rank < previous) {
                result.warnings.push(format!(
                    "Layer Style {}: FX ordering was normalized to AE's fixed native Layer Styles phase order",
                    super::layer_styles::style_label(effect)
                ));
            }
            let lowered = super::layer_styles::lower(effect, enabled, id, dynamics, size);
            if let Some(style) = lowered.style {
                result.styles.push(style);
                last_style_rank = Some(rank);
            }
            result.warnings.extend(lowered.warnings);
            continue;
        }
        let Some(mapping) = mapping::by_fx(kind) else {
            result.warnings.push(format!(
                "Effect {kind}: {}; omitted, owner retained",
                catalog::unsupported_reason(effect)
                    .unwrap_or("no verified equivalent native effect/control representation")
            ));
            continue;
        };
        let mut native = match writer::effects::new_effect(mapping.native, enabled, size) {
            Ok(effect) => effect,
            Err(error) => {
                result
                    .warnings
                    .push(format!("Effect {kind}: {error}; omitted, owner retained"));
                continue;
            }
        };
        let value = match serde_json::to_value(effect) {
            Ok(value) => value,
            Err(error) => {
                result
                    .warnings
                    .push(format!("Effect {kind}: invalid payload: {error}; omitted"));
                continue;
            }
        };
        let defaults = mapping::default_effect(kind);
        for property in &mut native.properties {
            let fields: Vec<_> = mapping
                .fields
                .iter()
                .filter(|field| field.native == property.match_name)
                .collect();
            if fields.is_empty() {
                continue;
            }
            let mut tracks = vec![None; property.values.len()];
            for field in fields {
                let (Some(scale), Some(offset)) = (field.scale.factor(size), field.offset(size))
                else {
                    result.warnings.push(format!(
                        "Effect {kind} / {}: unknown source dimensions; native default retained",
                        field.param
                    ));
                    continue;
                };
                let base = value
                    .get(field.field)
                    .filter(|v| !v.is_null())
                    .or_else(|| defaults.get(field.field));
                let base = base.and_then(|v| {
                    v.as_f64()
                        .or_else(|| v.as_bool().map(|b| f64::from(u8::from(b))))
                });
                if let (Some(base), Some(slot)) = (base, property.values.get_mut(field.component)) {
                    let converted = (base - offset) / scale;
                    let hue_master = mapping.native == "ADBE HUE SATURATION"
                        && matches!(field.param, "hue" | "saturation" | "lightness");
                    if hue_master && writer::effects::hue_master_fixed(converted.round()).is_err() {
                        result.warnings.push(format!(
                            "Effect {kind} / {}: value exceeds native 16:16 range; native default retained",
                            field.param
                        ));
                    } else if hue_master && converted.fract() != 0.0 {
                        // The observed Channel Range state stores Master H/S/L
                        // as signed integers, unlike the visible pard defaults.
                        *slot = converted.round();
                        result.warnings.push(format!(
                            "Effect {kind} / {}: fractional Master value rounded to nearest integer for native Channel Range state",
                            field.param
                        ));
                    } else {
                        *slot = converted;
                    }
                }
                let Some(id) = id else {
                    continue;
                };
                if kind == "vignette"
                    && dynamics.iter().any(|entry| {
                        matches!(&entry.target, PropertyTarget::EffectProperty(target)
                            if target.effect_id() == id && target.param_name() == field.param)
                    })
                {
                    result.warnings.push(format!(
                        "Effect {kind} / {}: animated CC Vignette export unsupported: Adobe continuous rendering freezes keyed controls; animation omitted, authored base retained as a static native control",
                        field.param
                    ));
                    continue;
                }
                match parameter_track(dynamics, id, field.param, scale, offset) {
                    Ok(Some(_))
                        if mapping.native == "ADBE HUE SATURATION"
                            && matches!(
                                field.param,
                                "hue" | "saturation" | "lightness" | "colorize"
                            ) =>
                    {
                        // AE 26.5 can key the composite Channel Range state, but
                        // its Master color transfer differs from FX's HSV operation.
                        // The Colorize toggle's composite-key semantics are not
                        // established. Ordinary keys on these leaves are ignored;
                        // preserve independently keyable numeric Colorize tracks.
                        let reason = if field.param == "colorize" {
                            "Colorize toggle is not independently keyable and composite Channel Range behavior is unverified"
                        } else {
                            "composite Channel Range is keyable but FX HSV Master transfer has no verified equivalent"
                        };
                        result.warnings.push(format!(
                            "Effect {kind} / {}: {reason}; animation omitted, authored base retained as an editable native approximation",
                            field.param
                        ));
                    }
                    Ok(Some(track)) if field.animated && field.component < tracks.len() => {
                        tracks[field.component] = Some(track);
                    }
                    Ok(Some(_)) => result.warnings.push(format!(
                        "Effect {kind} / {}: static-only control animation omitted",
                        field.param
                    )),
                    Ok(None) => {}
                    Err(reason) => result.warnings.push(format!(
                        "Effect {kind} / {}: {reason}; authored base retained",
                        field.param
                    )),
                }
            }
            match merge_tracks(&property.values, &tracks) {
                Ok(Some(track))
                    if property.values.len() == 2
                        && track
                            .keys
                            .iter()
                            .flat_map(|key| &key.easing)
                            .any(|easing| matches!(easing, KeyframeEasing::CubicBezier { .. })) =>
                {
                    result.warnings.push(format!(
                        "Effect {kind} / {}: native Point uses shared temporal and spatial ease; cubic Point animation omitted, authored base retained",
                        property.match_name
                    ));
                }
                Ok(Some(track))
                    if property.values.len() == 4
                        && track
                            .keys
                            .iter()
                            .flat_map(|key| &key.easing)
                            .any(|easing| matches!(easing, KeyframeEasing::CubicBezier { .. })) =>
                {
                    result.warnings.push(format!(
                        "Effect {kind} / {}: native color uses a shared temporal ease; cubic color animation omitted, authored base retained",
                        property.match_name
                    ));
                }
                Ok(Some(track)) => property.animation = Some(track),
                Ok(None) => {}
                Err(reason) => result.warnings.push(format!(
                    "Effect {kind} / {}: {reason}; static property retained",
                    property.match_name
                )),
            }
        }
        result.warnings.extend(
            special::export(effect, size, &mut native)
                .into_iter()
                .map(|warning| format!("Effect {kind}: {warning}")),
        );
        if let Some(id) = id {
            for entry in dynamics {
                if let PropertyTarget::EffectProperty(target) = &entry.target
                    && target.effect_id() == id
                    && !mapping
                        .fields
                        .iter()
                        .any(|field| field.animated && field.param == target.param_name())
                {
                    result.warnings.push(format!(
                        "Effect {kind} / {}: no native animated target; animator omitted",
                        target.param_name()
                    ));
                }
            }
        }
        if !mapping.note.is_empty() {
            result
                .warnings
                .push(format!("Effect {kind}: {}", mapping.note));
        }
        result.effects.push(native);
    }
    if !result.effects.is_empty() && !result.styles.is_empty() {
        result.warnings.push("FX Layer Effects were normalized to AE's separate Layer Styles phase; their relative ordering with Effect Parade plugins cannot be preserved exactly.".into());
    }
    result
}

pub(super) fn parameter_track(
    entries: &[AnimationGraphEntry],
    id: EffectId,
    name: &str,
    scale: f64,
    offset: f64,
) -> Result<Option<NumericTrack>, &'static str> {
    let mut matches = entries.iter().filter(|entry| matches!(&entry.target, PropertyTarget::EffectProperty(target) if target.effect_id()==id && target.param_name()==name));
    let Some(entry) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err("duplicate animator target");
    }
    if !entry.dependencies.is_empty()
        || !entry.layer_refs.is_empty()
        || entry.random_seed_target.is_some()
    {
        return Err("dependent animator cannot be exported without executing a runtime");
    }
    let source = match entry.animator.data() {
        AnimatorData::Constant { value } => NativeTrack::Constant(value),
        AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } => NativeTrack::Keyframes(track),
        AnimatorData::Keyframes {
            enabled: false,
            disabled_value: Some(value),
            ..
        } => NativeTrack::Constant(value),
        AnimatorData::Keyframes {
            enabled: false,
            disabled_value: None,
            ..
        } => return Err("disabled animator has no runtime-visible constant"),
        AnimatorData::JsScript { .. } => {
            return Err("script animators are not executed, replayed or baked");
        }
    };
    let mut track = scalar_track(Some(source), scale)?;
    if let Some(track) = &mut track {
        for key in &mut track.keys {
            key.values[0] -= offset / scale;
        }
    }
    Ok(track)
}

/// Union authored knots, not sampled frames. Cubics are split analytically so
/// independent FX component timelines remain independently editable in AE.
pub(super) fn merge_tracks(
    base: &[f64],
    tracks: &[Option<NumericTrack>],
) -> Result<Option<NumericTrack>, &'static str> {
    let times: BTreeSet<_> = tracks
        .iter()
        .flatten()
        .flat_map(|track| track.keys.iter().map(|key| key.time_millis))
        .collect();
    if times.is_empty() {
        return Ok(None);
    }
    if u16::try_from(times.len()).is_err() {
        return Err("coupled native property exceeds the native key field");
    }
    let mut keys: Vec<NumericKeyframe> = Vec::with_capacity(times.len());
    for time in times {
        let mut values = Vec::with_capacity(base.len());
        let mut easing = Vec::with_capacity(base.len());
        for (component, initial) in base.iter().enumerate() {
            let previous = keys.last().map(|key| key.time_millis);
            let (value, curve) = match tracks.get(component).and_then(Option::as_ref) {
                Some(track) => sample_interval(track, previous, time)?,
                None => (*initial, KeyframeEasing::Linear),
            };
            if !value.is_finite() {
                return Err("nonfinite converted key value");
            }
            values.push(value);
            easing.push(curve);
        }
        if let Some(previous) = keys.last() {
            let active: Vec<_> = values
                .iter()
                .zip(&previous.values)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| i)
                .collect();
            let hold = active
                .iter()
                .any(|&i| matches!(easing[i], KeyframeEasing::Hold));
            if hold
                && active
                    .iter()
                    .any(|&i| !matches!(easing[i], KeyframeEasing::Hold))
            {
                return Err(
                    "mixed simultaneous Hold/continuous components have no shared native key interpolation",
                );
            }
            if hold {
                easing.fill(KeyframeEasing::Hold);
            } else if easing
                .iter()
                .any(|e| matches!(e, KeyframeEasing::CubicBezier { .. }))
            {
                for curve in &mut easing {
                    if matches!(curve, KeyframeEasing::Linear | KeyframeEasing::Hold) {
                        *curve = KeyframeEasing::CubicBezier {
                            x1: 1.0 / 3.0,
                            y1: 1.0 / 3.0,
                            x2: 2.0 / 3.0,
                            y2: 2.0 / 3.0,
                        };
                    }
                }
            } else {
                easing.fill(KeyframeEasing::Linear);
            }
        }
        keys.push(NumericKeyframe {
            time_millis: time,
            values,
            easing,
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        });
    }
    Ok(Some(NumericTrack { keys }))
}

fn sample_interval(
    track: &NumericTrack,
    from: Option<i64>,
    to: i64,
) -> Result<(f64, KeyframeEasing), &'static str> {
    let Some(first) = track.keys.first() else {
        return Err("empty property track");
    };
    if to <= first.time_millis {
        return Ok((first.values[0], KeyframeEasing::Linear));
    }
    let next = track.keys.partition_point(|key| key.time_millis < to);
    let Some(right) = track.keys.get(next) else {
        return Ok((
            track.keys.last().ok_or("empty track")?.values[0],
            KeyframeEasing::Linear,
        ));
    };
    let left = &track.keys[next - 1];
    let span = (right.time_millis - left.time_millis) as f64;
    if span <= 0.0 {
        return Err("unordered property keys");
    }
    let x1 = ((from.unwrap_or(to).max(left.time_millis) - left.time_millis) as f64 / span)
        .clamp(0.0, 1.0);
    let x2 = ((to - left.time_millis) as f64 / span).clamp(0.0, 1.0);
    let delta = right.values[0] - left.values[0];
    match right.easing[0] {
        KeyframeEasing::Hold => Ok((
            if to == right.time_millis {
                right.values[0]
            } else {
                left.values[0]
            },
            KeyframeEasing::Hold,
        )),
        KeyframeEasing::Linear => Ok((left.values[0] + delta * x2, KeyframeEasing::Linear)),
        KeyframeEasing::CubicBezier {
            x1: a,
            y1: b,
            x2: c,
            y2: d,
        } => {
            let u1 = invert_bezier(x1, a, c);
            let u2 = invert_bezier(x2, a, c);
            let y1 = bezier(u1, b, d);
            let y2 = bezier(u2, b, d);
            let value = left.values[0] + delta * y2;
            if from.is_none() || x1 == x2 || delta == 0.0 {
                return Ok((value, KeyframeEasing::Linear));
            }
            if (y2 - y1).abs() < 1e-12 {
                return Err("cubic split has equal-valued endpoints with nonconstant interior");
            }
            let du = (u2 - u1) / 3.0;
            let curve = KeyframeEasing::CubicBezier {
                x1: du * derivative(u1, a, c) / (x2 - x1),
                y1: du * derivative(u1, b, d) / (y2 - y1),
                x2: 1.0 - du * derivative(u2, a, c) / (x2 - x1),
                y2: 1.0 - du * derivative(u2, b, d) / (y2 - y1),
            };
            Ok((value, curve))
        }
    }
}
fn bezier(u: f64, a: f64, b: f64) -> f64 {
    let v = 1.0 - u;
    3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
}
fn derivative(u: f64, a: f64, b: f64) -> f64 {
    3.0 * (1.0 - u).powi(2) * a + 6.0 * (1.0 - u) * u * (b - a) + 3.0 * u * u * (1.0 - b)
}
fn invert_bezier(x: f64, a: f64, b: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = (lo + hi) * 0.5;
        if bezier(mid, a, b) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(times: &[i64], values: &[f64]) -> NumericTrack {
        NumericTrack {
            keys: times
                .iter()
                .zip(values)
                .map(|(&time, &value)| NumericKeyframe {
                    time_millis: time,
                    values: vec![value],
                    easing: vec![KeyframeEasing::Linear],
                    spatial_in: vec![],
                    spatial_out: vec![],
                })
                .collect(),
        }
    }
    #[test]
    fn independent_component_knots_are_not_frame_baked() {
        let result = merge_tracks(
            &[0.0, 0.0],
            &[
                Some(track(&[0, 1000], &[0.0, 10.0])),
                Some(track(&[0, 500, 1000], &[0.0, 20.0, 0.0])),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.keys.len(), 3);
        assert_eq!(result.keys[1].values, vec![5.0, 20.0]);
    }
    #[test]
    fn static_components_keep_their_value() {
        let result = merge_tracks(&[3.0, 7.0], &[Some(track(&[0, 1000], &[0.0, 10.0])), None])
            .unwrap()
            .unwrap();
        assert_eq!(result.keys[1].values, vec![10.0, 7.0]);
    }
    #[test]
    fn coupled_property_keys_exceed_old_policy_boundary() {
        let times = (0..=10_000).collect::<Vec<i64>>();
        let values = times.iter().map(|time| *time as f64).collect::<Vec<_>>();
        let result = merge_tracks(&[0.0], &[Some(track(&times, &values))])
            .unwrap()
            .unwrap();
        assert_eq!(result.keys.len(), times.len());
    }

    #[test]
    fn cubic_split_preserves_middle_value() {
        let mut a = track(&[0, 1000], &[0.0, 10.0]);
        a.keys[1].easing[0] = KeyframeEasing::CubicBezier {
            x1: 0.25,
            y1: 0.0,
            x2: 0.75,
            y2: 1.0,
        };
        let result = merge_tracks(
            &[0.0, 0.0],
            &[Some(a), Some(track(&[0, 500, 1000], &[0.0, 1.0, 0.0]))],
        )
        .unwrap()
        .unwrap();
        assert!((result.keys[1].values[0] - 5.0).abs() < 1e-10);
    }
}
