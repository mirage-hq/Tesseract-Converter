//! Exact affine placement of a fixed contour over compatible native Hold/Linear scalar keys.

use fx_schema::{Layer, LayerId, Position, PropType, PropertyValue, ShapePathCommand};

use crate::writer::{KeyframeEasing, PathKeyframe, PathTrack};

use super::super::{AnimationIndex, NativeTrack, track};
use super::{MaskOwner, checked_path, checked_shape_guide, relative_affine, transform_path};

pub(super) fn guide_path(
    id: LayerId,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &AnimationIndex<'_>,
) -> Result<(fx_schema::ShapePath, Option<PathTrack>), &'static str> {
    if owner.coordinate_owner.is_some() {
        return Err("affine Shape mask placement requires certified source-local coordinates");
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
        return Err("affine Shape guide and source coordinate parent/clock are not equivalent");
    }
    if dynamics.for_layer(id).any(|entry| {
        !entry.target.as_property().is_some_and(|property| {
            property.layer_id() == id
                && matches!(
                    property.property_type(),
                    PropType::PositionX
                        | PropType::PositionY
                        | PropType::ScaleX
                        | PropType::ScaleY
                        | PropType::ShapePath
                )
        })
    }) {
        return Err("affine Shape mask guide has unsupported geometry or coordinate animation");
    }
    let (base_path, base_transform) = checked_shape_guide(guide)?;
    let path = match track(dynamics, id, PropType::ShapePath)? {
        None => base_path,
        Some(NativeTrack::Constant(PropertyValue::Path(path))) => path,
        Some(_) => return Err("affine Shape mask requires fixed Path geometry"),
    };
    let path = checked_path(path.clone())?;
    if !matches!(path.commands.last(), Some(ShapePathCommand::Close)) {
        return Err("affine Shape mask requires one closed native contour");
    }
    let Position::TwoD(position) = base_transform.position else {
        return Err("3D guide coordinates are not exported");
    };
    // Anchor and orientation stay fixed; the existing affine validator rejects
    // unsupported skew/3D rather than inventing a new transform model.
    relative_affine(owner.transform, base_transform)?;
    let channels = [
        scalar_keys(track(dynamics, id, PropType::PositionX)?, position[0])?,
        scalar_keys(track(dynamics, id, PropType::PositionY)?, position[1])?,
        scalar_keys(
            track(dynamics, id, PropType::ScaleX)?,
            base_transform.scale[0],
        )?,
        scalar_keys(
            track(dynamics, id, PropType::ScaleY)?,
            base_transform.scale[1],
        )?,
    ];
    let mut times = std::collections::BTreeSet::from([0]);
    for channel in &channels {
        times.extend(channel.iter().map(|key| key.time));
    }
    if u16::try_from(times.len()).is_err() {
        return Err("affine Shape Path track exceeds the native key field");
    }
    // With fixed anchor/rotation every point and cubic control is a linear
    // combination of these four channels. Their joint authored timeline is
    // exact for compatible prepared segments, not a second sampling/baking pass.
    let mut previous_time = None;
    let mut keyframes = Vec::with_capacity(times.len());
    for time_millis in times {
        let easing = match previous_time {
            Some(start) => joint_easing(&channels, start, time_millis)?,
            None => ScalarEasing::Linear,
        };
        let mut transform = *base_transform;
        transform.position = Position::TwoD([
            scalar_value(&channels[0], time_millis),
            scalar_value(&channels[1], time_millis),
        ]);
        transform.scale[0] = scalar_value(&channels[2], time_millis);
        transform.scale[1] = scalar_value(&channels[3], time_millis);
        let transformed = checked_path(transform_path(
            path.clone(),
            relative_affine(owner.transform, &transform)?,
        ))?;
        keyframes.push(PathKeyframe {
            time_millis,
            path: transformed,
            easing: match easing {
                ScalarEasing::Hold => KeyframeEasing::Hold,
                ScalarEasing::Linear => KeyframeEasing::Linear,
            },
        });
        previous_time = Some(time_millis);
    }
    let track = PathTrack { keyframes };
    crate::writer::validate_path_track(&track)
        .map_err(|_| "affine Shape Path keys exceed native geometry/timing bounds")?;
    Ok((track.keyframes[0].path.clone(), Some(track)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScalarEasing {
    Hold,
    Linear,
}

#[derive(Clone, Copy, Debug)]
struct ScalarKey {
    time: i64,
    value: f64,
    easing: ScalarEasing,
}

fn scalar_keys(source: Option<NativeTrack<'_>>, base: f64) -> Result<Vec<ScalarKey>, &'static str> {
    let number = |value: &PropertyValue| match value {
        PropertyValue::Float(value) if value.is_finite() => Ok(*value),
        _ => Err("affine Shape placement requires finite scalar values"),
    };
    match source {
        None if base.is_finite() => Ok(vec![ScalarKey {
            time: 0,
            value: base,
            easing: ScalarEasing::Linear,
        }]),
        None => Err("affine Shape base placement is non-finite"),
        Some(NativeTrack::Constant(value)) => Ok(vec![ScalarKey {
            time: 0,
            value: number(value)?,
            easing: ScalarEasing::Linear,
        }]),
        Some(NativeTrack::Keyframes(track)) => {
            if track.keyframes().is_empty() || u16::try_from(track.keyframes().len()).is_err() {
                return Err("affine Shape scalar keys are empty or exceed the native key field");
            }
            let mut keys = Vec::with_capacity(track.keyframes().len());
            for key in track.keyframes() {
                let easing = match key.easing() {
                    fx_schema::PropertyKeyframeEasing::Linear => ScalarEasing::Linear,
                    fx_schema::PropertyKeyframeEasing::Hold => ScalarEasing::Hold,
                    _ => return Err("affine Shape placement requires Hold or Linear keys"),
                };
                if key.spatial_in_tangent().is_some() || key.spatial_out_tangent().is_some() {
                    return Err("affine Shape placement requires keys without spatial tangents");
                }
                let time = key.layer_time().as_millis();
                if time < 0
                    || keys
                        .last()
                        .is_some_and(|previous: &ScalarKey| previous.time >= time)
                {
                    return Err(
                        "affine Shape scalar key times must be nonnegative and strictly increasing",
                    );
                }
                keys.push(ScalarKey {
                    time,
                    value: number(key.value())?,
                    easing,
                });
            }
            Ok(keys)
        }
    }
}

fn scalar_value(keys: &[ScalarKey], time: i64) -> f64 {
    let index = keys.partition_point(|key| key.time <= time);
    if index == 0 {
        return keys[0].value;
    }
    if index == keys.len() || keys[index].easing == ScalarEasing::Hold {
        return keys[index - 1].value;
    }
    let left = keys[index - 1];
    let right = keys[index];
    let fraction = (time as f64 - left.time as f64) / (right.time as f64 - left.time as f64);
    left.value * (1.0 - fraction) + right.value * fraction
}

fn joint_easing(
    channels: &[Vec<ScalarKey>; 4],
    start: i64,
    end: i64,
) -> Result<ScalarEasing, &'static str> {
    let mut common = None;
    for keys in channels {
        if scalar_value(keys, start) == scalar_value(keys, end) {
            continue;
        }
        // The union timeline cannot cross an authored key. Use the incoming
        // easing of the original segment, including when end is inside it.
        let index = keys.partition_point(|key| key.time < end);
        let easing = keys
            .get(index)
            .ok_or("affine Shape changing interval has no scalar segment")?
            .easing;
        if common.is_some_and(|previous| previous != easing) {
            return Err("affine Shape placement mixes changing Hold and Linear channels");
        }
        common = Some(easing);
    }
    Ok(common.unwrap_or(ScalarEasing::Hold))
}

#[cfg(test)]
mod tests {
    use super::{ScalarEasing, ScalarKey, joint_easing, scalar_value};

    fn keys(values: &[(i64, f64)], easing: ScalarEasing) -> Vec<ScalarKey> {
        values
            .iter()
            .map(|&(time, value)| ScalarKey {
                time,
                value,
                easing,
            })
            .collect()
    }

    #[test]
    fn source_linear_values_clamp_without_discarding_offscreen_coordinates() {
        let keys = keys(&[(100, -100_000.0), (200, 540.0)], ScalarEasing::Linear);
        assert_eq!(scalar_value(&keys, 0), -100_000.0);
        assert_eq!(scalar_value(&keys, 150), -49_730.0);
        assert_eq!(scalar_value(&keys, 300), 540.0);
    }

    #[test]
    fn constant_hold_channels_allow_a_linear_segment_split_by_other_keys() {
        let channels = [
            keys(&[(0, 10.0), (200, 30.0)], ScalarEasing::Linear),
            keys(&[(0, 7.0), (100, 7.0)], ScalarEasing::Hold),
            keys(&[(0, 100.0)], ScalarEasing::Hold),
            keys(&[(0, 100.0)], ScalarEasing::Linear),
        ];
        assert_eq!(joint_easing(&channels, 0, 100), Ok(ScalarEasing::Linear));
        assert_eq!(joint_easing(&channels, 100, 200), Ok(ScalarEasing::Linear));
    }

    #[test]
    fn standalone_hold_jump_preserves_clamping_and_offscreen_placement() {
        let held = keys(&[(100, -100_000.0), (2518, 540.0)], ScalarEasing::Hold);
        assert_eq!(scalar_value(&held, 0), -100_000.0);
        assert_eq!(scalar_value(&held, 1000), -100_000.0);
        assert_eq!(scalar_value(&held, 2517), -100_000.0);
        assert_eq!(scalar_value(&held, 2518), 540.0);
        assert_eq!(scalar_value(&held, 3000), 540.0);
        let channels = [
            held,
            keys(&[(0, 1300.0), (2160, 1301.0)], ScalarEasing::Linear),
            keys(&[(0, 100.0)], ScalarEasing::Hold),
            keys(&[(0, 100.0)], ScalarEasing::Linear),
        ];
        assert_eq!(joint_easing(&channels, 2160, 2518), Ok(ScalarEasing::Hold));
        assert_eq!(joint_easing(&channels, 2518, 3000), Ok(ScalarEasing::Hold));
    }

    #[test]
    fn simultaneous_changing_hold_and_linear_channels_are_rejected() {
        let channels = [
            keys(&[(0, -100_000.0), (200, 540.0)], ScalarEasing::Hold),
            keys(&[(0, 1300.0), (300, 1301.0)], ScalarEasing::Linear),
            keys(&[(0, 100.0)], ScalarEasing::Hold),
            keys(&[(0, 100.0)], ScalarEasing::Linear),
        ];
        assert!(joint_easing(&channels, 0, 200).is_err());
    }

    #[test]
    fn joint_channel_timeline_preserves_affine_interpolation() {
        let x = keys(&[(0, 10.0), (200, 30.0)], ScalarEasing::Linear);
        let scale = keys(
            &[(0, 100.0), (100, 200.0), (200, 100.0)],
            ScalarEasing::Linear,
        );
        let point = |time| scalar_value(&x, time) + 4.0 * scalar_value(&scale, time) / 100.0;
        assert_eq!(point(50), (point(0) + point(100)) / 2.0);
        assert_eq!(point(150), (point(100) + point(200)) / 2.0);
    }
}
