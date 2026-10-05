//! Typed-value sampling for geometry which cannot be fitted one scalar at a time.
//! Equal samples become held intervals; continuous runs are reduced together so
//! all coordinates share keys and topology changes never acquire guessed morphs.

/// One owner-local key. `linear` describes the incoming segment.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueKey<T> {
    pub offset_ms: u64,
    pub value: T,
    pub linear: bool,
}

/// A failed evaluation or a result which cannot fit the destination key field.
#[derive(Debug, thiserror::Error)]
pub enum ValueCurveError<E> {
    #[error("value evaluation failed")]
    Evaluation(#[source] E),
    #[error("fitted curve exceeds the destination key count")]
    KeyLimit,
}

// This is a reduction window, not a duration limit: flushing keeps an extra
// boundary key rather than rejecting long motion or retaining its dense history.
const REDUCTION_WINDOW: usize = 256;

/// Sample every integer millisecond, discard equal held samples, then reduce
/// continuous runs using the caller's maximum interpolation error. Returning
/// `None` from `error` prohibits interpolation (for example, changed topology).
/// This bounds error on the sampled clock, not between milliseconds. Dense
/// working storage is bounded by a reduction window plus the retained result;
/// `maximum_keys` must be the host format's actual representation limit.
pub fn fit_value_curve<T: PartialEq, E>(
    duration_ms: u64,
    tolerance: f64,
    maximum_keys: usize,
    evaluate: impl FnMut(u64) -> Result<T, E>,
    error: impl Fn(&T, &T, &T, f64) -> Option<f64>,
) -> Result<Vec<ValueKey<T>>, ValueCurveError<E>> {
    fit_sampled_value_curve(0..=duration_ms, tolerance, maximum_keys, evaluate, error)
}

/// Fit values at caller-selected offsets, which must be nonempty and strictly
/// ascending. Error is bounded only at these offsets; the caller owns the
/// sampling policy. Equal values form held intervals between sampled offsets.
pub fn fit_sampled_value_curve<T: PartialEq, E>(
    offsets: impl IntoIterator<Item = u64>,
    tolerance: f64,
    maximum_keys: usize,
    mut evaluate: impl FnMut(u64) -> Result<T, E>,
    error: impl Fn(&T, &T, &T, f64) -> Option<f64>,
) -> Result<Vec<ValueKey<T>>, ValueCurveError<E>> {
    if maximum_keys == 0 {
        return Err(ValueCurveError::KeyLimit);
    }
    let mut offsets = offsets.into_iter();
    let first = offsets.next().expect("sample offsets must be nonempty");
    let mut result = Vec::new();
    let mut keys = vec![ValueKey {
        offset_ms: first,
        value: evaluate(first).map_err(ValueCurveError::Evaluation)?,
        linear: false,
    }];
    let mut previous_sample = first;
    for offset_ms in offsets {
        assert!(
            offset_ms > previous_sample,
            "sample offsets must be strictly ascending"
        );
        let value = evaluate(offset_ms).map_err(ValueCurveError::Evaluation)?;
        let previous = keys.last().expect("initial key exists");
        let adjacent = previous_sample == previous.offset_ms;
        previous_sample = offset_ms;
        if previous.value == value {
            continue;
        }
        let linear = adjacent && error(&previous.value, &value, &previous.value, 0.0).is_some();
        keys.push(ValueKey {
            offset_ms,
            value,
            linear,
        });
        if keys.len() == REDUCTION_WINDOW {
            let mut reduced = reduce(keys, tolerance, &error);
            let last = reduced
                .pop()
                .expect("nonempty reduction retains its endpoint");
            if result.len().saturating_add(reduced.len()).saturating_add(1) > maximum_keys {
                return Err(ValueCurveError::KeyLimit);
            }
            result.extend(reduced);
            keys = vec![last];
        }
    }
    let reduced = reduce(keys, tolerance, &error);
    if result.len().saturating_add(reduced.len()) > maximum_keys {
        return Err(ValueCurveError::KeyLimit);
    }
    result.extend(reduced);
    Ok(result)
}

fn reduce<T>(
    keys: Vec<ValueKey<T>>,
    tolerance: f64,
    error: &impl Fn(&T, &T, &T, f64) -> Option<f64>,
) -> Vec<ValueKey<T>> {
    let mut keep = vec![true; keys.len()];
    let mut start = 0;
    while start + 1 < keys.len() {
        let mut end = start;
        while end + 1 < keys.len() && keys[end + 1].linear {
            end += 1;
        }
        if end > start + 1 {
            keep[start + 1..end].fill(false);
            let mut pending = vec![(start, end)];
            while let Some((left, right)) = pending.pop() {
                let span = (keys[right].offset_ms - keys[left].offset_ms) as f64;
                let mut worst = None;
                let mut maximum = tolerance;
                for index in left + 1..right {
                    let progress = (keys[index].offset_ms - keys[left].offset_ms) as f64 / span;
                    let deviation = error(
                        &keys[left].value,
                        &keys[right].value,
                        &keys[index].value,
                        progress,
                    )
                    .unwrap_or(f64::INFINITY);
                    if !deviation.is_finite() || deviation > maximum {
                        maximum = deviation;
                        worst = Some(index);
                    }
                }
                if let Some(index) = worst {
                    keep[index] = true;
                    if index > left + 1 {
                        pending.push((left, index));
                    }
                    if right > index + 1 {
                        pending.push((index, right));
                    }
                }
            }
        }
        start = if end == start { start + 1 } else { end };
    }
    keys.into_iter()
        .zip(keep)
        .filter_map(|(key, keep)| keep.then_some(key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn error(a: &f64, b: &f64, actual: &f64, t: f64) -> Option<f64> {
        Some((a + (b - a) * t - actual).abs())
    }

    #[test]
    fn constant_and_linear_values_remove_redundant_keys() {
        let constant = fit_value_curve(100, 0.0, 65535, |_| Ok::<_, ()>(4.0), error).unwrap();
        assert_eq!(constant.len(), 1);
        let linear = fit_value_curve(100, 1e-10, 65535, |t| Ok::<_, ()>(t as f64), error).unwrap();
        assert_eq!(linear.len(), 2);
        assert_eq!(linear[1].offset_ms, 100);
        assert!(linear[1].linear);
    }

    #[test]
    fn long_continuous_curve_flushes_without_dropping_motion() {
        let keys = fit_value_curve(2000, 1e-10, 65535, |t| Ok::<_, ()>(t as f64), error).unwrap();
        assert!(keys.len() < 12);
        assert_eq!(keys.last().unwrap().offset_ms, 2000);
        for time in 0..=2000 {
            let index = keys.partition_point(|key| key.offset_ms <= time) - 1;
            let from = &keys[index];
            let value = keys.get(index + 1).map_or(from.value, |to| {
                from.value
                    + (to.value - from.value) * (time - from.offset_ms) as f64
                        / (to.offset_ms - from.offset_ms) as f64
            });
            assert!((value - time as f64).abs() < 1e-10);
        }
    }

    #[test]
    fn destination_key_limit_stops_without_evaluating_the_whole_duration() {
        let mut calls = 0;
        let result = fit_value_curve(
            10000,
            0.0,
            8,
            |t| {
                calls += 1;
                Ok::<_, ()>(t as f64)
            },
            |_, _, _, _| None,
        );
        assert!(matches!(result, Err(ValueCurveError::KeyLimit)));
        assert_eq!(calls, REDUCTION_WINDOW);
    }

    #[test]
    fn held_steps_keep_transition_time_without_tweening() {
        let keys =
            fit_value_curve(100, 0.01, 65535, |t| Ok::<_, ()>((t / 17) as f64), error).unwrap();
        assert_eq!(
            keys.iter().map(|k| k.offset_ms).collect::<Vec<_>>(),
            [0, 17, 34, 51, 68, 85]
        );
        assert!(keys.iter().all(|k| !k.linear));
    }

    #[test]
    fn sparse_offsets_preserve_endpoints_and_reduce_by_actual_time() {
        let offsets = [5, 12, 27, 91];
        let keys =
            fit_sampled_value_curve(offsets, 0.0, 2, |t| Ok::<_, ()>(t as f64), error).unwrap();
        assert_eq!(
            keys.iter().map(|key| key.offset_ms).collect::<Vec<_>>(),
            [5, 91]
        );
        assert!(keys[1].linear);
    }

    #[test]
    fn sparse_held_samples_do_not_tween_across_a_plateau() {
        let keys = fit_sampled_value_curve(
            [0, 8, 21, 40, 73],
            0.0,
            5,
            |t| Ok::<_, ()>(if t < 40 { 0.0 } else { (t - 40) as f64 + 1.0 }),
            error,
        )
        .unwrap();
        assert_eq!(
            keys.iter().map(|key| key.offset_ms).collect::<Vec<_>>(),
            [0, 40, 73]
        );
        assert!(!keys[1].linear);
        assert!(keys[2].linear);
    }

    #[test]
    fn sparse_topology_errors_and_limit_match_dense_behavior() {
        let offsets = [0, 4, 12];
        let keys = fit_sampled_value_curve(
            offsets,
            0.0,
            3,
            |t| Ok::<_, ()>(vec![0; t as usize]),
            |_, _, _, _| None,
        )
        .unwrap();
        assert_eq!(
            keys.iter().map(|key| key.offset_ms).collect::<Vec<_>>(),
            offsets
        );
        assert!(keys.iter().all(|key| !key.linear));
        assert!(matches!(
            fit_sampled_value_curve(offsets, 0.0, 2, Ok::<_, ()>, |_, _, _, _| None),
            Err(ValueCurveError::KeyLimit)
        ));
        assert!(matches!(
            fit_sampled_value_curve(
                offsets,
                0.0,
                3,
                |t| if t == 4 { Err("failed") } else { Ok(t) },
                |_, _, _, _| None
            ),
            Err(ValueCurveError::Evaluation("failed"))
        ));
    }

    #[test]
    fn topology_changes_and_errors_are_not_hidden() {
        let keys = fit_value_curve(
            3,
            0.0,
            65535,
            |t| Ok::<_, ()>(vec![1; t as usize]),
            |_, _, _, _| None,
        )
        .unwrap();
        assert_eq!(keys.len(), 4);
        assert!(keys.iter().all(|k| !k.linear));
        assert!(
            fit_value_curve(
                3,
                0.0,
                65535,
                |t| if t == 2 { Err("failed") } else { Ok(t as f64) },
                error
            )
            .is_err()
        );
    }
}
