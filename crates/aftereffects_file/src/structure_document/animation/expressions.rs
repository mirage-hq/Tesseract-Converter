//! Generic lowering of explicitly captured Adobe values, never expression code.

use fx_keyframe_bake::curve_fit::{FittedCurve, FittedEasing, fit_scalar_curve};

use crate::{
    expression_samples::{EvaluatedProperty, ExpressionSamples, PropertyIdentity},
    structure_document::control_links::read_layer_transform,
};

use super::{
    AnimationBudget, AnimationGraphEntry, KeyframeId, Layer, LayerId, NumericAnimationTarget,
    NumericTargetValue, PropertyAnimator, PropertyKeyframe, PropertyKeyframeEasing,
    PropertyKeyframeTrack, PropertyTarget, PropertyTrackEstimate, PropertyValue, TimeOffset,
};

const FIT_TOLERANCE: f64 = 0.001;

fn fitted_value_at(curve: &FittedCurve, time: f64, segment: &mut usize) -> f64 {
    if time <= curve.keys[0].offset_ms as f64 {
        return curve.keys[0].value;
    }
    while *segment + 1 < curve.keys.len() && (curve.keys[*segment + 1].offset_ms as f64) < time {
        *segment += 1;
    }
    let first = &curve.keys[*segment];
    let Some(next) = curve.keys.get(*segment + 1) else {
        return first.value;
    };
    if time >= next.offset_ms as f64 && *segment + 2 == curve.keys.len() {
        return next.value;
    }
    let progress = (time - first.offset_ms as f64) / (next.offset_ms - first.offset_ms) as f64;
    first.value + (next.value - first.value) * next.easing.progress(progress.clamp(0.0, 1.0))
}

fn fitted_value(curve: &FittedCurve, time: u64, segment: &mut usize) -> f64 {
    fitted_value_at(curve, time as f64, segment)
}

/// Enclose the native observations in integer-ms keys. Quantization can put
/// either endpoint outside the requested grid; at most one extra ms per end is
/// needed by the validated half-ms clock bound. Extrapolation there does not
/// relax validation at any captured native timestamp.
fn fitting_grid(samples: &EvaluatedProperty) -> Result<(i64, usize), String> {
    let duration = samples.values.len().checked_sub(1).ok_or_else(|| {
        "AE-evaluated expression dimensions do not match the FX target".to_owned()
    })?;
    let duration = i64::try_from(duration)
        .map_err(|_| "AE-evaluated expression duration exceeds i64".to_owned())?;
    let end = samples
        .start_ms
        .checked_add(duration)
        .ok_or_else(|| "AE-evaluated expression timestamp overflows i64".to_owned())?;
    let before = samples
        .sample_times_seconds
        .first()
        .is_some_and(|time| time * 1_000.0 < samples.start_ms as f64);
    let after = samples
        .sample_times_seconds
        .last()
        .is_some_and(|time| time * 1_000.0 > end as f64);
    let start = samples
        .start_ms
        .checked_sub(i64::from(before))
        .ok_or_else(|| "AE-evaluated expression timestamp overflows i64".to_owned())?;
    end.checked_add(i64::from(after))
        .ok_or_else(|| "AE-evaluated expression timestamp overflows i64".to_owned())?;
    Ok((
        start,
        samples.values.len() + usize::from(before) + usize::from(after),
    ))
}

fn resampled_component(
    samples: &EvaluatedProperty,
    component: usize,
    scale: f64,
    offset: f64,
) -> Result<Vec<f64>, String> {
    if samples.values.is_empty() || samples.values.iter().any(|value| component >= value.len()) {
        return Err("AE-evaluated expression dimensions do not match the FX target".into());
    }
    if samples.sample_times_seconds.is_empty() {
        return samples
            .values
            .iter()
            .map(|value| Ok(value[component] * scale + offset))
            .collect();
    }
    let (start_ms, count) = fitting_grid(samples)?;
    let mut right = usize::from(samples.values.len() > 1);
    (0..count)
        .map(|index| {
            let target = (start_ms as f64 + index as f64) / 1_000.0;
            while right + 1 < samples.sample_times_seconds.len()
                && samples.sample_times_seconds[right] < target
            {
                right += 1;
            }
            let value = if samples.sample_times_seconds[right] == target || right == 0 {
                samples.values[right][component]
            } else {
                let left = right - 1;
                let progress = (target - samples.sample_times_seconds[left])
                    / (samples.sample_times_seconds[right] - samples.sample_times_seconds[left]);
                samples.values[left][component]
                    + (samples.values[right][component] - samples.values[left][component])
                        * progress
            };
            let value = value * scale + offset;
            value
                .is_finite()
                .then_some(value)
                .ok_or_else(|| "AE-evaluated expression maps to a nonfinite FX value".to_owned())
        })
        .collect()
}

fn fit_component(
    samples: &EvaluatedProperty,
    component: usize,
    scale: f64,
    offset: f64,
) -> Result<FittedCurve, String> {
    let resampled = resampled_component(samples, component, scale, offset)?;
    let (start_ms, _) = fitting_grid(samples)?;
    let duration = resampled
        .len()
        .checked_sub(1)
        .and_then(|duration| u64::try_from(duration).ok())
        .ok_or_else(|| "AE-evaluated expression duration exceeds u64".to_owned())?;
    let curve = fit_scalar_curve::<String>(duration, FIT_TOLERANCE, |index| {
        let index = usize::try_from(index)
            .map_err(|_| "AE-evaluated expression sample offset exceeds usize".to_owned())?;
        resampled
            .get(index)
            .copied()
            .ok_or_else(|| "AE-evaluated expression sample offset is out of range".to_owned())
    })?;
    let mut segment = 0;
    for (index, sample) in samples.values.iter().enumerate() {
        let expected = sample[component] * scale + offset;
        let time = samples
            .sample_times_seconds
            .get(index)
            .map_or(index as f64, |seconds| seconds * 1_000.0 - start_ms as f64);
        let actual = fitted_value_at(&curve, time, &mut segment);
        if !actual.is_finite() || (actual - expected).abs() > curve.tolerance + 1.0e-9 {
            return Err("AE-evaluated expression fit exceeds tolerance at an actual native timestamp; animation omitted rather than silently losing a discontinuity".into());
        }
    }
    Ok(curve)
}

fn fitted_key(
    curve: &FittedCurve,
    index: usize,
    target: &PropertyTarget,
    start_ms: i64,
) -> Result<PropertyKeyframe, String> {
    let key = &curve.keys[index];
    let easing = match key.easing {
        FittedEasing::Hold => PropertyKeyframeEasing::Hold,
        FittedEasing::Linear => PropertyKeyframeEasing::Linear,
        FittedEasing::Cubic { y1, y2 } => PropertyKeyframeEasing::CubicBezier {
            x1: 1.0 / 3.0,
            y1,
            x2: 2.0 / 3.0,
            y2,
        },
    };
    let offset_ms = i64::try_from(key.offset_ms)
        .map_err(|_| "AE-evaluated expression key timestamp exceeds i64".to_owned())?;
    let timestamp = start_ms
        .checked_add(offset_ms)
        .ok_or_else(|| "AE-evaluated expression key timestamp overflows i64".to_owned())?;
    Ok(PropertyKeyframe::new(
        KeyframeId::new(format!("aep-evaluated-{target}-{index}")),
        TimeOffset::from_millis(timestamp),
        PropertyValue::Float(key.value),
        easing,
    ))
}

/// Captured times are already in the parent composition clock. Never apply
/// source-layer start/stretch a second time. Affine offsets are per FX target.
pub(in crate::structure_document) fn evaluated_numeric_entries(
    name: &str,
    samples: &EvaluatedProperty,
    targets: &[NumericAnimationTarget],
    offsets: &[f64],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let converted = convert_numeric(samples, targets, offsets, budget);
    match converted {
        Ok(entries) => {
            let keys: usize = entries
                .iter()
                .filter_map(|entry| entry.animator.keyframe_track())
                .map(|track| track.keyframes().len())
                .sum();
            (
                entries,
                vec![format!(
                    "{name}: AE-evaluated expression approximated with {keys} editable scalar keys; fit verified at every captured native timestamp (maximum 0.001 FX-unit error); unsampled behavior and live expression linkage are not preserved"
                )],
            )
        }
        Err(error) => (
            Vec::new(),
            vec![format!(
                "{name}: {error}; static authored/default value retained"
            )],
        ),
    }
}

fn convert_numeric(
    samples: &EvaluatedProperty,
    targets: &[NumericAnimationTarget],
    offsets: &[f64],
    budget: &mut AnimationBudget,
) -> Result<Vec<AnimationGraphEntry>, String> {
    if targets.is_empty() {
        return Err("AE-evaluated expression has no supported FX scalar target".into());
    }
    let (start_ms, sample_count) = fitting_grid(samples)?;
    let curves = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let NumericTargetValue::Float { component, scale } = target.value else {
                return Err("AE-evaluated expression requires an existing scalar FX target".into());
            };
            fit_component(
                samples,
                component,
                scale,
                offsets.get(index).copied().unwrap_or(0.0),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut reservations = Vec::with_capacity(targets.len());
    for (curve, target) in curves.iter().zip(targets) {
        // Track validation checks value kinds, but opacity's finite range is
        // enforced only when FX applies the property. Reject invalid fitted
        // values here rather than publishing a track that fails during playback.
        if target
            .target
            .as_property()
            .is_some_and(|property| property.property_type() == fx_schema::PropType::Opacity)
        {
            let mut segment = 0;
            for time in 0..sample_count as u64 {
                if !(0.0..=100.0).contains(&fitted_value(curve, time, &mut segment)) {
                    return Err("AE-evaluated expression produced an invalid FX track: opacity is outside 0..100".into());
                }
            }
        }
        let mut estimate = PropertyTrackEstimate::default();
        for index in 0..curve.keys.len() {
            let key = fitted_key(curve, index, &target.target, start_ms)?;
            estimate
                .push_prospective(&key)
                .map_err(|error| error.to_string())?;
        }
        reservations.push(
            estimate
                .entry_reservation_bytes(&target.target)
                .map_err(|error| error.to_string())?,
        );
    }
    let checkpoint = budget.checkpoint();
    budget
        .reserve_all(reservations)
        .map_err(|error| error.to_string())?;
    let mut entries = Vec::with_capacity(targets.len());
    for (curve, target) in curves.iter().zip(targets) {
        let keys = (0..curve.keys.len())
            .map(|index| fitted_key(curve, index, &target.target, start_ms))
            .collect::<Result<Vec<_>, _>>()?;
        let track = match PropertyKeyframeTrack::new(keys).and_then(|track| {
            track.validate_for_target(&target.target)?;
            Ok(track)
        }) {
            Ok(track) => track,
            Err(error) => {
                budget.rollback(checkpoint);
                return Err(format!(
                    "AE-evaluated expression produced an invalid FX track: {error}"
                ));
            }
        };
        entries.push(AnimationGraphEntry {
            target: target.target.clone(),
            animator: PropertyAnimator::keyframes(track),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        });
    }
    Ok(entries)
}

pub(in crate::structure_document) fn evaluated_transform_entries(
    evaluations: &ExpressionSamples,
    (comp_id, composition): (u32, &crate::structure::Composition),
    layer: &Layer,
    target_id: LayerId,
    anchor_scale: [f64; 2],
    include_opacity: bool,
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let Ok((properties, _)) = read_layer_transform(layer, composition) else {
        return (Vec::new(), Vec::new());
    };
    let separated = properties
        .iter()
        .find(|property| property.match_name == "ADBE Position")
        .and_then(|property| property.numeric.as_ref().ok())
        .is_some_and(|numeric| numeric.dimensions_separated);
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for property in properties {
        let name = property.match_name.as_str();
        if (!include_opacity && name == "ADBE Opacity")
            || (name == "ADBE Position" && separated)
            || (name.starts_with("ADBE Position_") && !separated)
            || !property
                .numeric
                .is_ok_and(|numeric| numeric.expression_enabled)
        {
            continue;
        }
        let identity = PropertyIdentity::Transform {
            match_name: name.to_owned(),
        };
        let Some(samples) = evaluations.lookup(comp_id, layer.record.id(), &identity) else {
            continue;
        };
        let mut targets = super::targets(
            name,
            layer.record.flags().three_d_layer,
            target_id,
            anchor_scale,
        );
        // AE's API returns percentage values; native AEP storage uses fractions.
        if matches!(name, "ADBE Scale" | "ADBE Opacity") {
            for target in &mut targets {
                if let NumericTargetValue::Float { scale, .. } = &mut target.value {
                    *scale /= 100.0;
                }
            }
        }
        let (converted, messages) = evaluated_numeric_entries(name, samples, &targets, &[], budget);
        entries.extend(converted);
        warnings.extend(messages);
    }
    (entries, warnings)
}

#[cfg(test)]
mod tests;
