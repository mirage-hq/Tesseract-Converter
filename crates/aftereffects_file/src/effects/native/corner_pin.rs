use crate::properties::{NumericKeyframe, NumericProperty, NumericValueKind};

/// A monotone straight spatial curve has the same distance progress as a line.
/// With zero temporal speeds, its ease is independent of the native speed units.
/// Check before relative Point values are converted to anisotropic pixel units.
pub(super) fn normalize_straight_zero_speed(point: &mut NumericProperty) -> bool {
    if !point.animated
        || point.expression_enabled
        || point.dimensions_separated
        || point.value_kind != NumericValueKind::Continuous
        || point.keyframes.len() < 2
        || (!point.values.is_empty()
            && (point.values.len() != 2 || point.values.iter().any(|value| !value.is_finite())))
        || point.keyframes.iter().any(|key| {
            !key.time_secs.is_finite()
                || key.in_interpolation != 2
                || key.out_interpolation != 2
                || key.in_speed.as_slice() != [0.0]
                || key.out_speed.as_slice() != [0.0]
                || [&key.in_influence, &key.out_influence]
                    .into_iter()
                    .any(|influence| {
                        influence.len() != 1
                            || !influence[0].is_finite()
                            || !(0.0..=100.0).contains(&influence[0])
                    })
                || [&key.values, &key.spatial_in, &key.spatial_out]
                    .into_iter()
                    .any(|values| {
                        values.len() != 2 || values.iter().any(|value| !value.is_finite())
                    })
        })
        || !point.keyframes.iter().any(|key| {
            key.spatial_in
                .iter()
                .chain(&key.spatial_out)
                .any(|value| *value != 0.0)
        })
        || point.keyframes.windows(2).any(|pair| {
            pair[0].time_secs >= pair[1].time_secs || !straight_segment(&pair[0], &pair[1])
        })
    {
        return false;
    }
    for key in &mut point.keyframes {
        key.spatial_in.fill(0.0);
        key.spatial_out.fill(0.0);
    }
    true
}

fn straight_segment(start: &NumericKeyframe, end: &NumericKeyframe) -> bool {
    let delta = [
        end.values[0] - start.values[0],
        end.values[1] - start.values[1],
    ];
    if delta.iter().any(|value| !value.is_finite()) {
        return false;
    }
    let axis = usize::from(delta[1].abs() > delta[0].abs());
    if delta[axis] == 0.0 {
        return start
            .spatial_out
            .iter()
            .chain(&end.spatial_in)
            .all(|value| *value == 0.0);
    }
    let outgoing = start.spatial_out[axis] / delta[axis];
    let incoming = end.spatial_in[axis] / delta[axis];
    // Ordered control points on the segment are a sufficient monotonicity bound.
    if !(0.0..=1.0).contains(&outgoing)
        || !(-1.0..=0.0).contains(&incoming)
        || outgoing > 1.0 + incoming
    {
        return false;
    }
    [(&start.spatial_out, outgoing), (&end.spatial_in, incoming)]
        .into_iter()
        .all(|(handle, factor)| {
            (0..2).all(|component| {
                let expected = factor * delta[component];
                let tolerance = 32.0
                    * f64::EPSILON
                    * delta[component].abs().max(handle[component].abs()).max(1.0);
                (handle[component] - expected).abs() <= tolerance
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point() -> NumericProperty {
        let key = |time_secs, values| NumericKeyframe {
            time_secs,
            values,
            in_interpolation: 2,
            out_interpolation: 2,
            in_speed: vec![0.0],
            out_speed: vec![0.0],
            in_influence: vec![100.0 / 3.0],
            out_influence: vec![100.0 / 3.0],
            spatial_in: vec![-0.025, -0.01],
            spatial_out: vec![0.025, 0.01],
        };
        NumericProperty {
            values: vec![-0.15, -0.06],
            animated: true,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: vec![key(0.0, vec![-0.15, -0.06]), key(1.0, vec![0.0, 0.0])],
            value_kind: NumericValueKind::Continuous,
        }
    }

    #[test]
    fn corner_pin_straight_normalization_preserves_time_values_and_ease() {
        let mut point = point();
        let before = point.clone();
        assert!(normalize_straight_zero_speed(&mut point));
        for (key, original) in point.keyframes.iter_mut().zip(before.keyframes) {
            assert_eq!(key.spatial_in, [0.0, 0.0]);
            assert_eq!(key.spatial_out, [0.0, 0.0]);
            key.spatial_in = original.spatial_in.clone();
            key.spatial_out = original.spatial_out.clone();
            assert_eq!(*key, original);
        }
    }

    #[test]
    fn corner_pin_unsafe_spatial_profiles_are_not_modified() {
        for case in 0..10 {
            let mut point = point();
            match case {
                0 => point.keyframes[0].spatial_out[1] += 0.01,
                1 => point.keyframes[1].in_speed[0] = 1.0,
                2 => point.keyframes[0].spatial_out = vec![0.15, 0.06],
                3 => point.keyframes[1].values = point.keyframes[0].values.clone(),
                4 => point.keyframes[1].time_secs = 0.0,
                5 => {
                    point.keyframes[0].spatial_in.pop();
                }
                6 => point.keyframes[0].in_influence[0] = f64::NAN,
                7 => point.expression_enabled = true,
                8 => point.keyframes[1].out_interpolation = 1,
                9 => point.keyframes[0].in_speed.clear(),
                _ => unreachable!(),
            }
            let before = format!("{point:?}");
            assert!(!normalize_straight_zero_speed(&mut point), "case {case}");
            assert_eq!(format!("{point:?}"), before, "case {case} was mutated");
        }
    }
}
