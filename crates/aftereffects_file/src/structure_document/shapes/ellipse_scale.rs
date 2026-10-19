//! Keep an outer paint independent of a centered ellipse's inner uniform scale.

use fx_schema::animator::{
    AnimationGraphEntry, AnimatorData, KeyframeId, PropertyKeyframe, PropertyKeyframeEasing,
    PropertyKeyframeTrack,
};
use fx_schema::{LayerId, PropType, PropertyAnimator, PropertyTarget, PropertyValue, ShapeLayer};
use serde::ser::Error as _;

use super::bindings;

#[cfg(test)]
mod tests;
use crate::structure_document::animation_budget::{
    AnimationBudget, PropertyTrackEstimate, constant_entry_reservation_bytes,
};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-8 * a.abs().max(b.abs()).max(1.0)
}

fn scalar(value: &PropertyValue) -> Option<f64> {
    match value {
        PropertyValue::Float(value) if value.is_finite() && *value >= 0.0 => Some(*value),
        _ => None,
    }
}

fn same_easing(a: PropertyKeyframeEasing, b: PropertyKeyframeEasing) -> bool {
    use PropertyKeyframeEasing::{CubicBezier, Hold, Linear};
    match (a, b) {
        (Hold, Hold) | (Linear, Linear) => true,
        (
            CubicBezier { x1, y1, x2, y2 },
            CubicBezier {
                x1: bx1,
                y1: by1,
                x2: bx2,
                y2: by2,
            },
        ) => close(x1, bx1) && close(x2, bx2) && close(y1, by1) && close(y2, by2),
        _ => false,
    }
}

fn nonnegative_scale_segment(keys: &[PropertyKeyframe], base_size: [f64; 2]) -> bool {
    let PropertyKeyframeEasing::CubicBezier { y1, y2, .. } = keys[1].easing() else {
        return true;
    };
    let (Some(start), Some(end)) = (scalar(keys[0].value()), scalar(keys[1].value())) else {
        return false;
    };
    // Negative progress handles can still describe a wholly nonnegative scale curve.
    [start + (end - start) * y1, start + (end - start) * y2]
        .into_iter()
        .all(|scale| {
            scale.is_finite()
                && scale >= 0.0
                && base_size
                    .into_iter()
                    .all(|size| (size * scale / 100.0).is_finite())
        })
}

fn control(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    property: PropType,
) -> Result<Option<&AnimatorData>, ()> {
    let mut found = entries
        .iter()
        .filter(|entry| entry.target == PropertyTarget::layer(id, property));
    let Some(entry) = found.next() else {
        return Ok(None);
    };
    if found.next().is_some() || !entry.dependencies.is_empty() {
        return Err(());
    }
    Ok(Some(entry.animator.data()))
}

fn unchanged_control(
    entries: &[AnimationGraphEntry],
    id: LayerId,
    property: PropType,
    value: PropertyValue,
) -> bool {
    match control(entries, id, property) {
        Ok(None) => true,
        Ok(Some(AnimatorData::Constant { value: stored })) => stored == &value,
        _ => false,
    }
}

pub(super) fn normalize(
    shape: &mut ShapeLayer,
    source: LayerId,
    owner: LayerId,
    entries: &mut Vec<AnimationGraphEntry>,
    budget: &mut AnimationBudget,
) -> Result<bool, serde_json::Error> {
    let Some(ellipse) = &shape.shape.ellipse else {
        return Ok(false);
    };
    let transform = &shape.transform;
    if shape.shape.strokes.is_empty()
        || shape.shape.trim.is_some()
        || shape.shape.offset_paths.is_some()
        || shape.shape.round_corners.is_some()
        || ellipse.position != [0.0, 0.0]
        || ellipse.size.iter().any(|value| {
            !value.is_finite() || *value < 0.0 || !(value * transform.scale[0] / 100.0).is_finite()
        })
        || transform.anchor_point != [0.0, 0.0]
        || transform.skew != 0.0
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0, 0.0, 0.0]
        || !close(transform.scale[0], transform.scale[1])
        || transform
            .scale
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        || !unchanged_control(
            entries,
            source,
            PropType::EllipseSize,
            PropertyValue::Vector2(ellipse.size),
        )
        || !unchanged_control(
            entries,
            source,
            PropType::EllipsePosition,
            PropertyValue::Vector2([0.0, 0.0]),
        )
        || [
            PropType::AnchorPointX,
            PropType::AnchorPointY,
            PropType::Skew,
            PropType::RotationX,
            PropType::RotationY,
        ]
        .into_iter()
        .any(|property| !unchanged_control(entries, owner, property, PropertyValue::Float(0.0)))
    {
        return Ok(false);
    }
    let base_size = ellipse.size;
    let target = PropertyTarget::layer(shape.id, PropType::EllipseSize);
    let make_key = |index, key: &PropertyKeyframe| -> Option<PropertyKeyframe> {
        let scale = scalar(key.value())? / 100.0;
        let size = base_size.map(|value| value * scale);
        size.into_iter().all(f64::is_finite).then(|| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("aep-ellipse-scale-{target}-{index}")),
                key.layer_time(),
                PropertyValue::Vector2(size),
                key.easing(),
            )
        })
    };
    let (Ok(x), Ok(y)) = (
        control(entries, owner, PropType::ScaleX),
        control(entries, owner, PropType::ScaleY),
    ) else {
        return Ok(false);
    };
    let checkpoint = budget.checkpoint();
    let animator = match (x, y) {
        (
            Some(AnimatorData::Keyframes {
                track: x,
                enabled: true,
                ..
            }),
            Some(AnimatorData::Keyframes {
                track: y,
                enabled: true,
                ..
            }),
        ) => {
            if x.keyframes().len() != y.keyframes().len()
                || !x
                    .keyframes()
                    .windows(2)
                    .all(|keys| nonnegative_scale_segment(keys, base_size))
                || !y
                    .keyframes()
                    .windows(2)
                    .all(|keys| nonnegative_scale_segment(keys, base_size))
            {
                return Ok(false);
            }
            let mut estimate = PropertyTrackEstimate::default();
            for (index, (x, y)) in x.keyframes().iter().zip(y.keyframes()).enumerate() {
                if x.layer_time() != y.layer_time()
                    || !same_easing(x.easing(), y.easing())
                    || !matches!((scalar(x.value()), scalar(y.value())), (Some(a), Some(b)) if close(a, b))
                    || x.spatial_in_tangent().is_some()
                    || x.spatial_out_tangent().is_some()
                    || y.spatial_in_tangent().is_some()
                    || y.spatial_out_tangent().is_some()
                {
                    return Ok(false);
                }
                let Some(key) = make_key(index, x) else {
                    return Ok(false);
                };
                if estimate.push_prospective(&key).is_err() {
                    return Ok(false);
                }
            }
            let Ok(reservation) = estimate.entry_reservation_bytes(&target) else {
                return Ok(false);
            };
            if budget.reserve(reservation).is_err() {
                return Ok(false);
            }
            let keys = x
                .keyframes()
                .iter()
                .enumerate()
                .filter_map(|(index, key)| make_key(index, key))
                .collect();
            let track = match PropertyKeyframeTrack::new(keys) {
                Ok(track) => track,
                Err(error) => {
                    budget.rollback(checkpoint);
                    return Err(serde_json::Error::custom(error));
                }
            };
            PropertyAnimator::keyframes(track)
        }
        _ => {
            let constant_scale = |data: Option<&AnimatorData>, fallback| match data {
                None => Some(fallback),
                Some(AnimatorData::Constant { value }) => scalar(value),
                _ => None,
            };
            let (Some(x), Some(y)) = (
                constant_scale(x, transform.scale[0]),
                constant_scale(y, transform.scale[1]),
            ) else {
                return Ok(false);
            };
            if !close(x, y) {
                return Ok(false);
            }
            let size = base_size.map(|value| value * x / 100.0);
            if !size.into_iter().all(f64::is_finite) {
                return Ok(false);
            }
            let value = PropertyValue::Vector2(size);
            let Ok(reservation) = constant_entry_reservation_bytes(&target, &value) else {
                return Ok(false);
            };
            if budget.reserve(reservation).is_err() {
                return Ok(false);
            }
            match PropertyAnimator::constant(value) {
                Ok(animator) => animator,
                Err(error) => {
                    budget.rollback(checkpoint);
                    return Err(error);
                }
            }
        }
    };
    // AE transforms the contour before the outer Stroke; FX transforms its paint too.
    shape.shape.ellipse.as_mut().expect("checked ellipse").size =
        base_size.map(|value| value * transform.scale[0] / 100.0);
    shape.transform.scale = [100.0, 100.0];
    entries.push(bindings::entry(target, animator, Vec::new()));
    Ok(true)
}
