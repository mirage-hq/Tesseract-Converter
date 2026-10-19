//! Converter-only Path observations become ordinary sparse editable keys.
use fx_keyframe_bake::value_curve::fit_sampled_value_curve;
use fx_schema::{
    KeyframeId, PropertyAnimator, PropertyKeyframeEasing, PropertyTarget, PropertyValue,
    TimeOffset,
    animator::{AnimationGraphEntry, PropertyKeyframe, PropertyKeyframeTrack},
};

use crate::structure_document::{
    animation::NumericAnimationClock,
    animation_budget::{AnimationBudget, committed_entry_reservation_bytes},
};
use crate::{expression_samples::EvaluatedPath, properties::NativePathValue, structure::Layer};

const TOLERANCE: f64 = 0.01;

#[cfg(test)]
#[path = "expression_tests.rs"]
mod tests;

pub(in crate::structure_document) fn entries(
    samples: &EvaluatedPath,
    owner: &Layer,
    clock: NumericAnimationClock,
    target: PropertyTarget,
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let checkpoint = budget.checkpoint();
    match build(samples, owner, clock, target, budget) {
        Ok(entry) => (
            vec![entry],
            vec![format!(
                "Path expression lowered to sparse editable Path keys at source-frame observations within {TOLERANCE} owner-pixel units; native API rounding, unsampled motion and live dependency linkage are not restored"
            )],
        ),
        Err(error) => {
            budget.rollback(checkpoint);
            (
                Vec::new(),
                vec![format!(
                    "Path expression not lowered ({error}); authored editable outline retained"
                )],
            )
        }
    }
}

fn build(
    samples: &EvaluatedPath,
    owner: &Layer,
    clock: NumericAnimationClock,
    target: PropertyTarget,
    budget: &mut AnimationBudget,
) -> Result<AnimationGraphEntry, String> {
    let start = owner
        .record
        .start_time()
        .ok_or("invalid Path owner start time")?;
    let stretch = owner.record.stretch().ok_or("invalid Path owner stretch")?;
    if !start.is_finite() || !stretch.is_finite() || stretch <= 0.0 {
        return Err("unsupported Path expression owner clock".into());
    }
    if samples.values.is_empty() || samples.values.len() != samples.sample_times_seconds.len() {
        return Err("inconsistent Path observation grid".into());
    }
    let initial = &samples.values[0];
    initial.validate().map_err(|e| e.to_string())?;
    let mut times = Vec::with_capacity(samples.values.len());
    for (time, value) in samples.sample_times_seconds.iter().zip(&samples.values) {
        value.validate().map_err(|e| e.to_string())?;
        if value.closed != initial.closed || value.vertices.len() != initial.vertices.len() {
            return Err("changing Path expression topology or closure is not lowered".into());
        }
        let milliseconds = clock.seconds((time - start) / stretch) * 1000.0;
        if !milliseconds.is_finite() || milliseconds.abs() > ((1_u64 << 53) - 1) as f64 {
            return Err("Path expression time is outside the exact FX millisecond clock".into());
        }
        let milliseconds = milliseconds.round() as i64;
        if times
            .last()
            .is_some_and(|previous| milliseconds <= *previous)
        {
            return Err(
                "Path expression observations collapse or reorder on the millisecond clock".into(),
            );
        }
        times.push(milliseconds);
    }
    let origin = times[0];
    let offsets = times
        .iter()
        .map(|time| {
            u64::try_from(time - origin).map_err(|_| "Path fitting offset overflow".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fitted = fit_sampled_value_curve(
        offsets.iter().copied(),
        TOLERANCE,
        usize::MAX,
        |offset| {
            let index = offsets
                .binary_search(&offset)
                .map_err(|_| "missing Path fitting observation")?;
            Ok::<_, String>(samples.values[index].clone())
        },
        interpolation_error,
    )
    .map_err(|e| format!("Path fitting failed: {e:?}"))?;
    // Reduction is required, not merely a call to a fitter that may return all frames.
    if offsets.len() >= 3 && fitted.len() >= offsets.len() {
        return Err("Path expression requires a key for every observation; per-frame key output is prohibited".into());
    }
    // Recheck quantized destination interpolation against every retained observation.
    for ((offset, value), time) in offsets.iter().zip(&samples.values).zip(&times) {
        let right = fitted.partition_point(|key| key.offset_ms <= *offset);
        let left = right.saturating_sub(1);
        let error = if right >= fitted.len() || !fitted[right].linear {
            interpolation_error(&fitted[left].value, &fitted[left].value, value, 0.0)
        } else {
            let a = &fitted[left];
            let b = &fitted[right];
            let progress = (*offset - a.offset_ms) as f64 / (b.offset_ms - a.offset_ms) as f64;
            interpolation_error(&a.value, &b.value, value, progress)
        }
        .ok_or("invalid fitted Path topology")?;
        if error > TOLERANCE {
            return Err(format!(
                "Path playback exceeds {TOLERANCE} tolerance at {time}ms"
            ));
        }
    }
    let owner_id = target
        .layer_id()
        .ok_or("Path expression target needs a layer")?;
    let reservation = fitted
        .len()
        .checked_mul(
            initial
                .vertices
                .len()
                .saturating_add(2)
                .saturating_mul(1024),
        )
        .and_then(|size| size.checked_add(1024))
        .ok_or("Path track reservation overflow")?;
    budget.reserve(reservation).map_err(|e| e.to_string())?;
    let keys = fitted
        .into_iter()
        .enumerate()
        .map(|(index, key)| {
            let offset = i64::try_from(key.offset_ms).map_err(|_| "Path key offset overflow")?;
            let time = origin.checked_add(offset).ok_or("Path key time overflow")?;
            Ok(PropertyKeyframe::new(
                KeyframeId::new(format!("aep-expression-path-{owner_id}-{index}")),
                TimeOffset::from_millis(time),
                PropertyValue::Path(key.value.to_shape_path(true).map_err(|e| e.to_string())?),
                if key.linear {
                    PropertyKeyframeEasing::Linear
                } else {
                    PropertyKeyframeEasing::Hold
                },
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let track = PropertyKeyframeTrack::new(keys).map_err(|e| e.to_string())?;
    track
        .validate_for_target(&target)
        .map_err(|e| e.to_string())?;
    let entry = AnimationGraphEntry {
        target,
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let actual = committed_entry_reservation_bytes(&entry).map_err(|e| e.to_string())?;
    if actual > reservation {
        return Err("Path track serialization exceeds preflight bound".into());
    }
    budget
        .release(reservation - actual)
        .map_err(|e| e.to_string())?;
    Ok(entry)
}

fn interpolation_error(
    from: &NativePathValue,
    to: &NativePathValue,
    actual: &NativePathValue,
    progress: f64,
) -> Option<f64> {
    if from.closed != to.closed
        || from.closed != actual.closed
        || from.vertices.len() != to.vertices.len()
        || from.vertices.len() != actual.vertices.len()
    {
        return None;
    }
    let mut maximum: f64 = 0.0;
    let from = from
        .vertices
        .iter()
        .chain(&from.in_tangents)
        .chain(&from.out_tangents)
        .flatten();
    let to = to
        .vertices
        .iter()
        .chain(&to.in_tangents)
        .chain(&to.out_tangents)
        .flatten();
    let actual = actual
        .vertices
        .iter()
        .chain(&actual.in_tangents)
        .chain(&actual.out_tangents)
        .flatten();
    for ((a, b), value) in from.zip(to).zip(actual) {
        let error = (a + (b - a) * progress - value).abs();
        if !error.is_finite() {
            return None;
        }
        maximum = maximum.max(error);
    }
    Some(maximum)
}
