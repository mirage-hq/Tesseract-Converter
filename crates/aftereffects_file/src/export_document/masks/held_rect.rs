//! Exact held placement of square Rect guides in certified source-local coordinates.

use fx_schema::{Layer, LayerId, Position, PropType, PropertyValue};

use crate::writer::{KeyframeEasing, PathKeyframe, PathTrack};

use super::super::{AnimationIndex, NativeTrack, track};
use super::{MaskOwner, checked_path, rectangle_path, relative_affine, transform_path};

pub(super) fn guide_path(
    id: LayerId,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &AnimationIndex<'_>,
) -> Result<(fx_schema::ShapePath, Option<PathTrack>), &'static str> {
    if owner.coordinate_owner.is_some() {
        return Err("held Rect mask placement requires certified source-local coordinates");
    }
    let clock = owner
        .clock
        .ok_or("mask owner has an unproven source clock")?;
    let guide = siblings
        .iter()
        .find(|layer| layer.id() == id)
        .ok_or("guide is not in the owner's immediate sibling stack")?;
    if guide.parent_id() != owner.parent
        || clock.start != fx_schema::Time::ZERO
        || guide.active_range() != clock
    {
        return Err("held Rect guide and source coordinate parent/clock are not equivalent");
    }
    let fx_schema::LayerData::Rect(layer) = guide.data() else {
        return Err("held mask guide is not a square Rect");
    };
    if layer.rect.roundness != 0.0
        || !layer.rect.position.iter().all(|value| value.is_finite())
        || !layer
            .rect
            .size
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
    {
        return Err("held Rect mask geometry must be finite, positive and square-cornered");
    }
    // This profile deliberately does not reinterpret any other guide animator.
    if dynamics.for_layer(id).any(|entry| {
        !entry.target.as_property().is_some_and(|property| {
            property.layer_id() == id
                && matches!(
                    property.property_type(),
                    PropType::PositionX | PropType::PositionY
                )
        })
    }) {
        return Err("held Rect mask guide has unsupported geometry or other animation");
    }
    let Position::TwoD(base) = layer.transform.position else {
        return Err("3D guide coordinates are not exported");
    };
    // Validate the static linear part before replacing only translation.
    relative_affine(owner.transform, &layer.transform)?;
    let x = scalar_keys(track(dynamics, id, PropType::PositionX)?, base[0])?;
    let y = scalar_keys(track(dynamics, id, PropType::PositionY)?, base[1])?;
    let mut times = std::collections::BTreeSet::from([0]);
    times.extend(x.iter().map(|(time, _)| *time));
    times.extend(y.iter().map(|(time, _)| *time));
    if u16::try_from(times.len()).is_err() {
        return Err("held Rect Path track exceeds the native key field");
    }
    let mut keyframes = Vec::with_capacity(times.len());
    for time_millis in times {
        let mut transform = layer.transform;
        transform.position =
            Position::TwoD([held_value(&x, time_millis), held_value(&y, time_millis)]);
        let path = checked_path(transform_path(
            rectangle_path(layer.rect.position, layer.rect.size),
            relative_affine(owner.transform, &transform)?,
        ))?;
        keyframes.push(PathKeyframe {
            time_millis,
            path,
            easing: KeyframeEasing::Hold,
        });
    }
    let track = PathTrack { keyframes };
    crate::writer::validate_path_track(&track)
        .map_err(|_| "held Rect Path keys exceed native geometry/timing bounds")?;
    Ok((track.keyframes[0].path.clone(), Some(track)))
}

fn scalar_keys(
    source: Option<NativeTrack<'_>>,
    base: f64,
) -> Result<Vec<(i64, f64)>, &'static str> {
    let number = |value: &PropertyValue| match value {
        PropertyValue::Float(value) if value.is_finite() => Ok(*value),
        _ => Err("held Rect position must contain finite scalar values"),
    };
    match source {
        None if base.is_finite() => Ok(vec![(0, base)]),
        None => Err("held Rect base position is non-finite"),
        Some(NativeTrack::Constant(value)) => Ok(vec![(0, number(value)?)]),
        Some(NativeTrack::Keyframes(track)) => {
            if track.keyframes().is_empty() || u16::try_from(track.keyframes().len()).is_err() {
                return Err("held Rect position keys are empty or exceed the native key field");
            }
            let mut keys = Vec::with_capacity(track.keyframes().len());
            for key in track.keyframes() {
                if key.easing() != fx_schema::PropertyKeyframeEasing::Hold {
                    return Err("held Rect position requires exclusively Hold keys");
                }
                let time = key.layer_time().as_millis();
                if keys.last().is_some_and(|(previous, _)| *previous >= time) {
                    return Err("held Rect position key times must be strictly increasing");
                }
                keys.push((time, number(key.value())?));
            }
            Ok(keys)
        }
    }
}

fn held_value(keys: &[(i64, f64)], time: i64) -> f64 {
    let index = keys.partition_point(|(key_time, _)| *key_time <= time);
    keys[index.saturating_sub(1)].1
}

#[cfg(test)]
mod tests {
    use super::held_value;

    #[test]
    fn source_position_holds_and_clamps_at_authored_steps() {
        let keys = [(2133, -200.0), (2143, -640.0)];
        assert_eq!(held_value(&keys, 0), -200.0);
        assert_eq!(held_value(&keys, 2142), -200.0);
        assert_eq!(held_value(&keys, 2143), -640.0);
        assert_eq!(held_value(&keys, 2918), -640.0);
    }
}
