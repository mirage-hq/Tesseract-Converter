//! ShapePath values use the existing VM and typed FX keys, not per-coordinate
//! scripts or inferred particle identities. Topology changes remain held.

use super::*;
use fx_keyframe_bake::value_curve::{ValueCurveError, ValueKey, fit_value_curve};
use fx_schema::{ShapePath, ShapePathCommand};

// Geometry-space coordinate error, before ancestor transforms. This is not a
// screen-space or render-fidelity bound.
const TOLERANCE: f64 = 0.01;

pub(super) fn bake(
    entry: &AnimationGraphEntry,
    code: &str,
    owner: Owner,
    used_ids: &mut BTreeSet<String>,
    budget: &mut Budget,
) -> Result<PropertyAnimator, BakeError> {
    let seed = seed::prefix(entry.random_seed_target.as_ref().unwrap_or(&entry.target));
    let mut runtime = ScriptRuntime::new()?;
    let keys = fit_value_curve(
        owner.duration_ms,
        TOLERANCE,
        usize::from(u16::MAX),
        |time| evaluate(&mut runtime, code, seed, time, budget),
        interpolation_error,
    )
    .map_err(|error| match error {
        ValueCurveError::Evaluation(error) => error,
        ValueCurveError::KeyLimit => {
            BakeError::Unsupported("Path keys exceed the native u16 key field")
        }
    })?;
    validate_fresh(code, seed, owner.duration_ms, &keys, budget)?;
    let identity = conversion_identity_seed(&serde_json::to_vec(&entry.target)?, code.as_bytes());
    let keys = keys
        .into_iter()
        .map(|key| {
            let time = i64::try_from(key.offset_ms).map_err(|_| BakeError::Budget("key time"))?;
            Ok(PropertyKeyframe::new(
                fx_schema::KeyframeId::new(converted_keyframe_id(identity, time, used_ids)),
                TimeOffset::from_millis(time),
                PropertyValue::Path(key.value),
                if key.linear {
                    PropertyKeyframeEasing::Linear
                } else {
                    PropertyKeyframeEasing::Hold
                },
            ))
        })
        .collect::<Result<Vec<_>, BakeError>>()?;
    let track = PropertyKeyframeTrack::new(keys)?;
    track.validate_for_target(&entry.target)?;
    budget.keys = budget
        .keys
        .checked_add(track.keyframes().len())
        .ok_or(BakeError::Budget("key counter overflow"))?;
    Ok(PropertyAnimator::keyframes(track))
}

fn evaluate(
    runtime: &mut ScriptRuntime,
    code: &str,
    seed: u64,
    time: u64,
    budget: &mut Budget,
) -> Result<ShapePath, BakeError> {
    let value = evaluate_value(runtime, code, seed, time, budget)?;
    let json = value
        .to_json(runtime.context_mut())
        .map_err(ScriptError::from)?
        .ok_or(BakeError::Unsupported("Path script returned undefined"))?;
    let path: ShapePath = serde_json::from_value(json)?;
    if !path.is_finite()
        || path.commands.iter().any(|command| match command {
            ShapePathCommand::MoveTo {
                mirror,
                corner_radius,
                ..
            }
            | ShapePathCommand::LineTo {
                mirror,
                corner_radius,
                ..
            }
            | ShapePathCommand::CubicTo {
                mirror,
                corner_radius,
                ..
            } => mirror.is_some() || corner_radius.is_some(),
            ShapePathCommand::Close => false,
        })
    {
        return Err(BakeError::Unsupported(
            "Path script contains non-finite geometry or unsupported point controls",
        ));
    }
    Ok(path)
}

fn coordinates(command: &ShapePathCommand) -> [f64; 6] {
    match command {
        ShapePathCommand::MoveTo { x, y, .. } | ShapePathCommand::LineTo { x, y, .. } => {
            [*x, *y, 0.0, 0.0, 0.0, 0.0]
        }
        ShapePathCommand::CubicTo {
            c1x,
            c1y,
            c2x,
            c2y,
            x,
            y,
            ..
        } => [*x, *y, *c1x, *c1y, *c2x, *c2y],
        ShapePathCommand::Close => [0.0; 6],
    }
}

fn interpolation_error(
    from: &ShapePath,
    to: &ShapePath,
    actual: &ShapePath,
    progress: f64,
) -> Option<f64> {
    if from.commands.len() != to.commands.len() || from.commands.len() != actual.commands.len() {
        return None;
    }
    let mut maximum: f64 = 0.0;
    for ((from, to), actual) in from.commands.iter().zip(&to.commands).zip(&actual.commands) {
        if std::mem::discriminant(from) != std::mem::discriminant(to)
            || std::mem::discriminant(from) != std::mem::discriminant(actual)
        {
            return None;
        }
        for ((a, b), value) in coordinates(from)
            .into_iter()
            .zip(coordinates(to))
            .zip(coordinates(actual))
        {
            let deviation = (a + (b - a) * progress - value).abs();
            if !deviation.is_finite() {
                return None;
            }
            maximum = maximum.max(deviation);
        }
    }
    Some(maximum)
}

fn validate_fresh(
    code: &str,
    seed: u64,
    duration: u64,
    keys: &[ValueKey<ShapePath>],
    budget: &mut Budget,
) -> Result<(), BakeError> {
    let mut runtime = ScriptRuntime::new()?;
    // Probe out of playback order before the ascending pass, so a global
    // counter cannot masquerade as owner-local time by observing call order.
    for time in [duration, 0].into_iter().chain(0..=duration) {
        let index = keys
            .partition_point(|key| key.offset_ms <= time)
            .saturating_sub(1);
        let actual = evaluate(&mut runtime, code, seed, time, budget)?;
        let from = &keys[index];
        let error = if let Some(to) = keys.get(index + 1).filter(|key| key.linear) {
            let progress = (time - from.offset_ms) as f64 / (to.offset_ms - from.offset_ms) as f64;
            interpolation_error(&from.value, &to.value, &actual, progress)
        } else {
            (actual == from.value).then_some(0.0)
        };
        if error.is_none_or(|error| !error.is_finite() || error > TOLERANCE) {
            return Err(BakeError::Validation(time));
        }
    }
    Ok(())
}
