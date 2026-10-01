//! Native authored Path keys. Unsupported whole tracks retain the initial
//! editable outline, without scripts or sampled in-between geometry.

#[cfg(test)]
mod tests;

use crate::structure_document::{
    animation::NumericAnimationClock,
    animation_budget::{AnimationBudget, committed_entry_reservation_bytes},
};
use crate::{
    properties::{NumericKeyframe, data, read_path_metadata, unique_list},
    rifx::Chunk,
};
use fx_schema::{
    PropertyTarget, PropertyValue, TimeOffset,
    animator::{
        AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe,
        PropertyKeyframeEasing, PropertyKeyframeTrack,
    },
};

#[cfg(test)]
pub(in crate::structure_document) fn is_dynamic(run: &[Chunk]) -> bool {
    run.iter().any(|chunk| {
        if chunk.id() == *b"tdb4" {
            return chunk.data_payload().is_some_and(|bytes| {
                bytes.get(68).copied().unwrap_or(0) != 0
                    || bytes.get(120).copied().unwrap_or(0) & 1 != 0
            });
        }
        chunk.children().is_some_and(is_dynamic)
    })
}

pub(in crate::structure_document) fn entries(
    run: &[Chunk],
    scale: [f64; 2],
    target: PropertyTarget,
    clock: NumericAnimationClock,
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let checkpoint = budget.checkpoint();
    match build(run, scale, target, clock, budget) {
        Ok(Some(entry)) => (vec![entry], Vec::new()),
        Ok(None) => (Vec::new(), Vec::new()),
        Err(error) => {
            budget.rollback(checkpoint);
            (
                Vec::new(),
                vec![format!(
                    "Path animation omitted; initial static editable path retained: {error}"
                )],
            )
        }
    }
}

fn build(
    run: &[Chunk],
    scale: [f64; 2],
    target: PropertyTarget,
    clock: NumericAnimationClock,
    budget: &mut AnimationBudget,
) -> Result<Option<AnimationGraphEntry>, String> {
    let value = unique_list(
        super::property_run(run).map_err(|e| e.to_string())?,
        *b"om-s",
    )
    .map_err(|e| e.to_string())?;
    let metadata = read_path_metadata(unique_list(value, *b"tdbs").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if metadata.expression_enabled {
        return Err("Path expression is unsupported".into());
    }
    if !metadata.animated && metadata.keyframes.is_empty() {
        return Ok(None);
    }
    let shapes = unique_list(value, *b"omks").map_err(|e| e.to_string())?;
    if shapes.len() != metadata.keyframes.len() || shapes.is_empty() {
        return Err("Path timing/geometry key counts do not agree".into());
    }
    // Reserve before allocating decoded geometry. This conservative bound covers
    // six finite JSON f64 coordinates, key IDs and wrapper overhead per command.
    let mut reservation = 1024usize;
    for shape in shapes {
        if shape.list_kind() != Some(*b"shap") {
            return Err("unknown Path geometry record".into());
        }
        let list = unique_list(shape.children().ok_or("missing Path shape")?, *b"list")
            .map_err(|e| e.to_string())?;
        let bytes = data(list, *b"ldat").map_err(|e| e.to_string())?.len();
        reservation = reservation
            .checked_add((bytes / 24 + 2).saturating_mul(1024))
            .ok_or("Path budget overflow")?;
    }
    budget.reserve(reservation).map_err(|e| e.to_string())?;
    let mut keys = Vec::with_capacity(shapes.len());
    let mut previous = None;
    for (index, (native, shape)) in metadata.keyframes.iter().zip(shapes).enumerate() {
        let seconds = clock.seconds(native.time_secs);
        let millis = (seconds * 1000.0).round();
        if !millis.is_finite() || millis.abs() > i64::MAX as f64 {
            return Err("Path key time is outside the FX clock".into());
        }
        let millis = millis as i64;
        if previous.is_some_and(|time| {
            if clock.reversed() {
                millis >= time
            } else {
                millis <= time
            }
        }) {
            return Err("Path keys collapse or reorder on the millisecond clock".into());
        }
        previous = Some(millis);
        let path = super::decode_shape(shape.children().ok_or("missing Path shape")?, scale, true)
            .map_err(|e| e.to_string())?;
        let easing = if index == 0 {
            PropertyKeyframeEasing::Linear
        } else {
            easing(&metadata.keyframes[index - 1], native)?
        };
        keys.push((millis, path, easing));
    }
    for pair in keys.windows(2) {
        if pair[1].2 != PropertyKeyframeEasing::Hold
            && (pair[0].1.commands.len() != pair[1].1.commands.len()
                || pair[0]
                    .1
                    .commands
                    .iter()
                    .zip(&pair[1].1.commands)
                    .any(|(a, b)| std::mem::discriminant(a) != std::mem::discriminant(b)))
        {
            return Err("unequal Path topology morph is not fidelity-preserving in FX".into());
        }
    }
    if clock.reversed() {
        for index in 0..keys.len() - 1 {
            keys[index].2 = match keys[index + 1].2 {
                PropertyKeyframeEasing::Hold => {
                    return Err("reversed Hold Path boundary is not representable".into());
                }
                PropertyKeyframeEasing::Linear => PropertyKeyframeEasing::Linear,
                PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                    PropertyKeyframeEasing::CubicBezier {
                        x1: 1.0 - x2,
                        y1: 1.0 - y2,
                        x2: 1.0 - x1,
                        y2: 1.0 - y1,
                    }
                }
            };
        }
        keys.reverse();
        keys[0].2 = PropertyKeyframeEasing::Linear;
    }
    let owner = target
        .layer_id()
        .ok_or("Path animator needs a layer target")?;
    let keys = keys
        .into_iter()
        .enumerate()
        .map(|(index, (time, path, easing))| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("aep-path-{owner}-{index}")),
                TimeOffset::from_millis(time),
                PropertyValue::Path(path),
                easing,
            )
        })
        .collect();
    let track = PropertyKeyframeTrack::new(keys).map_err(|e| e.to_string())?;
    let entry = AnimationGraphEntry {
        target,
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let actual = committed_entry_reservation_bytes(&entry).map_err(|e| e.to_string())?;
    if actual > reservation {
        return Err("Path serialization exceeded preflight bound".into());
    }
    budget
        .release(reservation - actual)
        .map_err(|e| e.to_string())?;
    Ok(Some(entry))
}

fn easing(from: &NumericKeyframe, to: &NumericKeyframe) -> Result<PropertyKeyframeEasing, String> {
    if from.out_interpolation == 3 {
        return Ok(PropertyKeyframeEasing::Hold);
    }
    if from.out_interpolation == 1 && to.in_interpolation == 1 {
        return Ok(PropertyKeyframeEasing::Linear);
    }
    if (from.out_interpolation == 1 || to.in_interpolation == 1)
        && to.time_secs - from.time_secs != 2.0
    {
        return Err(
            "mixed Linear/Bezier Path easing outside the pinned two-second segment is unproved"
                .into(),
        );
    }
    let side = |kind: u8, speed: &[f64], influence: &[f64]| -> Result<(f64, f64), String> {
        match kind {
            // Pinned AE26 Path valueAtTime samples establish the mixed
            // Linear/Bezier side's diagonal handle at one sixth, not the
            // generic numeric property's stored (zero) influence.
            1 => Ok((1.0 / 6.0, 1.0 / 6.0)),
            2 => {
                let speed = speed.first().copied().ok_or("missing Path ease speed")?;
                let x = influence
                    .first()
                    .copied()
                    .ok_or("missing Path ease influence")?
                    / 100.0;
                if !(0.0..=1.0).contains(&x) {
                    return Err("invalid Path ease influence".into());
                }
                // Native Path speed is a normalized slope, not a numeric
                // property's units/second. Independent AE readback pins this
                // mapping across unequal durations and different path distances.
                let y = x * speed;
                if !speed.is_finite() || !(0.0..=1.0).contains(&y) {
                    return Err(
                        "Path negative or overshooting temporal handles are unproved".into(),
                    );
                }
                Ok((x, y))
            }
            _ => Err("unknown Path temporal easing is unsupported".into()),
        }
    };
    let (x1, y1) = side(from.out_interpolation, &from.out_speed, &from.out_influence)?;
    let (incoming_x, incoming_y) = side(to.in_interpolation, &to.in_speed, &to.in_influence)?;
    Ok(PropertyKeyframeEasing::CubicBezier {
        x1,
        y1,
        x2: 1.0 - incoming_x,
        y2: 1.0 - incoming_y,
    })
}
