//! Adaptive, millisecond-resolution fitting of an evaluated scalar curve.

use crate::value_curve::{ValueCurveError, fit_sampled_value_curve};

const INITIAL_OBSERVATIONS_PER_SECOND: u128 = 120;
const RELATIVE_FIT_TOLERANCE: f64 = 0.0015;
const ABSOLUTE_FIT_TOLERANCE: f64 = 1.0e-9;

/// Easing into a fitted key. Cubic handles have fixed time coordinates
/// `x1 = 1/3`, `x2 = 2/3`; only their value coordinates are fitted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FittedEasing {
    /// Keep the previous value until this key.
    Hold,
    /// Interpolate linearly from the previous key.
    Linear,
    /// Cubic Bézier with fixed, evenly spaced time handles.
    Cubic { y1: f64, y2: f64 },
}

impl FittedEasing {
    /// Evaluate easing at normalized linear progress between fitted keys.
    ///
    /// Cubic easing has fixed time handles (`x1 = 1/3`, `x2 = 2/3`), so its
    /// time parameter equals progress; this is not a general Bézier x solver.
    pub fn progress(self, progress: f64) -> f64 {
        match self {
            Self::Hold => 0.0,
            Self::Linear => progress,
            Self::Cubic { y1, y2 } => {
                // The fixed x handles make x(t) = t. Retain fx_model's
                // sample_curve arithmetic order for identical fitted errors.
                let inverse = 1.0 - progress;
                3.0 * inverse * inverse * progress * y1
                    + 3.0 * inverse * progress * progress * y2
                    + progress * progress * progress
            }
        }
    }
}

/// One sampled key, with easing from the preceding key into this one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FittedKey {
    /// Milliseconds from the beginning of the fitted interval.
    pub offset_ms: u64,
    /// Evaluated scalar at this offset.
    pub value: f64,
    /// Interpolation from the preceding key into this key.
    pub easing: FittedEasing,
}

/// The fitted keys and the absolute error tolerance used during fitting.
#[derive(Debug, Clone, PartialEq)]
pub struct FittedCurve {
    /// Ordered fitted keys, starting at zero.
    pub keys: Vec<FittedKey>,
    /// Observed-range-based tolerance, bounded by the caller's cap.
    pub tolerance: f64,
}

#[derive(Debug, Clone, Copy)]
struct SegmentCandidate {
    easing: FittedEasing,
    max_error: f64,
    max_error_offset: Option<u64>,
    max_observation_error: f64,
}

struct SegmentAnalysis {
    accepted: Option<SegmentCandidate>,
    best: SegmentCandidate,
    end_value: f64,
}

/// A required value at its original (possibly fractional) millisecond offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarObservation {
    /// Original time relative to the interval start, without key-time rounding.
    pub offset_ms: f64,
    /// Required scalar in the same units as the integer evaluator.
    pub value: f64,
}

/// Failure of opt-in actual-time constrained fitting.
#[derive(Debug, thiserror::Error)]
pub enum ObservationFitError<E> {
    #[error("invalid scalar observations: {0}")]
    InvalidObservations(&'static str),
    #[error("scalar evaluation failed: {0}")]
    Evaluation(E),
    #[error(
        "actual-time observations are unrepresentable with the sampled integer endpoints in {start_ms}..{end_ms}ms"
    )]
    Unrepresentable { start_ms: u64, end_ms: u64 },
}

#[derive(Clone, Copy)]
struct ObservationConstraints<'a> {
    samples: &'a [ScalarObservation],
    tolerance: f64,
}

impl ObservationConstraints<'_> {
    fn in_segment(&self, start: u64, end: u64) -> &[ScalarObservation] {
        let first = self
            .samples
            .partition_point(|sample| sample.offset_ms < start as f64);
        let last = self
            .samples
            .partition_point(|sample| sample.offset_ms <= end as f64);
        &self.samples[first..last]
    }
}

/// Fit a fallible scalar evaluator over an inclusive millisecond interval.
///
/// Propagates evaluator errors unchanged. The host must supply finite samples,
/// a nonnegative tolerance cap (infinity is allowed), and its own work budget.
/// Cubic keys always use the fixed time handles described by [`FittedEasing`].
pub fn fit_scalar_curve<E>(
    duration_ms: u64,
    tolerance_cap: f64,
    mut evaluate: impl FnMut(u64) -> Result<f64, E>,
) -> Result<FittedCurve, E> {
    match fit_scalar_curve_internal(duration_ms, tolerance_cap, None, &mut evaluate) {
        Ok(curve) => Ok(curve),
        Err(ObservationFitError::Evaluation(error)) => Err(error),
        // Without constraints every adjacent-ms segment is accepted; input
        // validation belongs to the opt-in wrapper, not this legacy API.
        Err(_) => unreachable!("unconstrained fitting cannot reject observation constraints"),
    }
}

/// Fit integer-ms editable keys while preserving supplied actual-time values.
///
/// Observations constrain cubic construction, acceptance and key minimization.
/// `observation_tolerance` is a separate absolute numerical equality criterion,
/// never the range-relative fit tolerance. Integer evaluator samples remain soft
/// constraints under `tolerance_cap`; their endpoint values are not altered.
/// Inconsistent constraints at an indivisible 1ms interval return a diagnostic
/// error rather than rounded observations or a dense per-observation fallback.
/// As with [`fit_scalar_curve`], the host supplies finite evaluator values and a
/// work budget. Empty observations preserve the legacy fitter's behavior.
pub fn fit_scalar_curve_with_observations<E>(
    duration_ms: u64,
    tolerance_cap: f64,
    observations: &[ScalarObservation],
    observation_tolerance: f64,
    mut evaluate: impl FnMut(u64) -> Result<f64, E>,
) -> Result<FittedCurve, ObservationFitError<E>> {
    if duration_ms > (1_u64 << 53) - 1
        || tolerance_cap.is_nan()
        || tolerance_cap < 0.0
        || !observation_tolerance.is_finite()
        || observation_tolerance < 0.0
    {
        return Err(ObservationFitError::InvalidObservations(
            "invalid duration or tolerance",
        ));
    }
    if observations.iter().any(|sample| {
        !sample.offset_ms.is_finite()
            || !sample.value.is_finite()
            || sample.offset_ms < 0.0
            || sample.offset_ms > duration_ms as f64
    }) || observations
        .windows(2)
        .any(|pair| pair[0].offset_ms >= pair[1].offset_ms)
    {
        return Err(ObservationFitError::InvalidObservations(
            "values must be finite and times strictly ascending within the interval",
        ));
    }
    let constraints = (!observations.is_empty()).then_some(ObservationConstraints {
        samples: observations,
        tolerance: observation_tolerance,
    });
    fit_scalar_curve_internal(duration_ms, tolerance_cap, constraints, &mut evaluate)
}

fn fit_scalar_curve_internal<E>(
    duration_ms: u64,
    tolerance_cap: f64,
    constraints: Option<ObservationConstraints<'_>>,
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<FittedCurve, ObservationFitError<E>> {
    let mut evaluate = |offset| evaluate(offset).map_err(ObservationFitError::Evaluation);
    let observations = initial_observations(duration_ms, &mut evaluate)?;
    if duration_ms == 0
        && constraints.is_some_and(|constraints| {
            constraints
                .samples
                .iter()
                .any(|sample| (sample.value - observations[0].1).abs() > constraints.tolerance)
        })
    {
        return Err(ObservationFitError::Unrepresentable {
            start_ms: 0,
            end_ms: 0,
        });
    }
    let tolerance = observation_tolerance(&observations).min(tolerance_cap);
    let boundaries = mandatory_boundaries(&observations, duration_ms, &mut evaluate)?;
    let mut pending = boundaries
        .windows(2)
        .rev()
        .map(|window| (window[0], window[1]))
        .collect::<Vec<_>>();
    let mut segments = Vec::new();

    while let Some((start, end)) = pending.pop() {
        let analysis = analyze_segment(
            start,
            end,
            duration_ms,
            tolerance,
            constraints,
            &mut evaluate,
        )?;
        if let Some(candidate) = analysis.accepted {
            segments.push((start, end, analysis.end_value, candidate.easing));
            continue;
        }
        if end <= start + 1 {
            return Err(ObservationFitError::Unrepresentable {
                start_ms: start,
                end_ms: end,
            });
        }
        let split = analysis
            .best
            .max_error_offset
            .filter(|offset| *offset > start && *offset < end)
            .unwrap_or_else(|| start + (end - start) / 2);
        pending.push((split, end));
        pending.push((start, split));
    }
    segments.sort_unstable_by_key(|(start, _, _, _)| *start);

    let first_value = evaluate(0)?;
    let mut keys = vec![FittedKey {
        offset_ms: 0,
        value: first_value,
        easing: FittedEasing::Hold,
    }];
    for (_, end, end_value, easing) in segments {
        if keys.last().is_some_and(|key| key.offset_ms == end) {
            continue;
        }
        keys.push(FittedKey {
            offset_ms: end,
            value: end_value,
            easing,
        });
    }

    minimize_keys(
        &mut keys,
        &boundaries,
        duration_ms,
        tolerance,
        constraints,
        &mut evaluate,
    )?;
    if keys.len() >= 2 {
        let last = keys.len() - 1;
        if keys[last].easing == FittedEasing::Hold && keys[last].value == keys[last - 1].value {
            keys.pop();
        }
    }

    Ok(FittedCurve { keys, tolerance })
}

/// Fit only caller-selected offsets for opt-in fast baking, using Linear/Hold
/// keys rather than cubic easing. Error is bounded at sampled offsets only.
/// Offsets must be nonempty and strictly ascending; samples must be finite and
/// the tolerance cap nonnegative (infinity is allowed).
pub fn fit_sampled_scalar_curve<E>(
    offsets: impl IntoIterator<Item = u64>,
    tolerance_cap: f64,
    mut evaluate: impl FnMut(u64) -> Result<f64, E>,
) -> Result<FittedCurve, E> {
    let samples = offsets
        .into_iter()
        .map(|offset| evaluate(offset).map(|value| (offset, value)))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(!samples.is_empty(), "sample offsets must be nonempty");
    let tolerance = observation_tolerance(&samples).min(tolerance_cap);
    let mut values = samples.iter();
    let keys = fit_sampled_value_curve(
        samples.iter().map(|(offset, _)| *offset),
        tolerance,
        usize::MAX,
        |offset| {
            let (sample_offset, value) = values.next().expect("one value per sampled offset");
            debug_assert_eq!(*sample_offset, offset);
            Ok::<_, E>(*value)
        },
        |start, end, actual, progress| Some((start + (end - start) * progress - actual).abs()),
    );
    let keys = match keys {
        Ok(keys) => keys,
        Err(ValueCurveError::Evaluation(error)) => return Err(error),
        Err(ValueCurveError::KeyLimit) => {
            unreachable!("usize::MAX permits every sampled key")
        }
    };
    Ok(FittedCurve {
        keys: keys
            .into_iter()
            .map(|key| FittedKey {
                offset_ms: key.offset_ms,
                value: key.value,
                easing: if key.linear {
                    FittedEasing::Linear
                } else {
                    FittedEasing::Hold
                },
            })
            .collect(),
        tolerance,
    })
}

fn initial_observations<E>(
    duration_ms: u64,
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<Vec<(u64, f64)>, E> {
    let mut observations = Vec::new();
    for_observation_offsets(0, duration_ms, |offset| {
        observations.push((offset, evaluate(offset)?));
        Ok(())
    })?;
    Ok(observations)
}

fn observation_tolerance(observations: &[(u64, f64)]) -> f64 {
    let first = observations.first().map_or(0.0, |(_, value)| *value);
    let (minimum, maximum) = observations
        .iter()
        .map(|(_, value)| *value)
        .fold((first, first), |(minimum, maximum), value| {
            (minimum.min(value), maximum.max(value))
        });
    ((maximum - minimum) * RELATIVE_FIT_TOLERANCE).max(ABSOLUTE_FIT_TOLERANCE)
}

fn mandatory_boundaries<E>(
    observations: &[(u64, f64)],
    duration_ms: u64,
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<Vec<u64>, E> {
    let mut boundaries = vec![0];
    for window in observations.windows(3) {
        let (previous_offset, previous) = window[0];
        let (offset, value) = window[1];
        let (next_offset, next) = window[2];
        let incoming = value - previous;
        let outgoing = next - value;
        let is_maximum = incoming > ABSOLUTE_FIT_TOLERANCE && outgoing < -ABSOLUTE_FIT_TOLERANCE;
        let is_minimum = incoming < -ABSOLUTE_FIT_TOLERANCE && outgoing > ABSOLUTE_FIT_TOLERANCE;
        if is_maximum || is_minimum {
            let mut extremum = (offset, value);
            for candidate_offset in previous_offset + 1..next_offset {
                let candidate = evaluate(candidate_offset)?;
                let is_better = if is_maximum {
                    candidate > extremum.1
                } else {
                    candidate < extremum.1
                };
                if is_better {
                    extremum = (candidate_offset, candidate);
                }
            }
            boundaries.push(extremum.0);
        }
    }
    if boundaries.last().copied() != Some(duration_ms) {
        boundaries.push(duration_ms);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    Ok(boundaries)
}

fn minimize_keys<E>(
    keys: &mut Vec<FittedKey>,
    mandatory_boundaries: &[u64],
    maximum_offset: u64,
    tolerance: f64,
    constraints: Option<ObservationConstraints<'_>>,
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<(), E> {
    let mut index = 1;
    while index + 1 < keys.len() {
        if mandatory_boundaries
            .binary_search(&keys[index].offset_ms)
            .is_ok()
        {
            index += 1;
            continue;
        }
        let analysis = analyze_segment(
            keys[index - 1].offset_ms,
            keys[index + 1].offset_ms,
            maximum_offset,
            tolerance,
            constraints,
            evaluate,
        )?;
        if let Some(candidate) = analysis.accepted {
            keys[index + 1].easing = candidate.easing;
            keys[index + 1].value = analysis.end_value;
            keys.remove(index);
            index = index.saturating_sub(1).max(1);
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn analyze_segment<E>(
    start: u64,
    end: u64,
    maximum_offset: u64,
    tolerance: f64,
    constraints: Option<ObservationConstraints<'_>>,
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<SegmentAnalysis, E> {
    let start_value = evaluate(start)?;
    let end_value = evaluate(end)?;
    if end == start {
        let candidate = SegmentCandidate {
            easing: FittedEasing::Hold,
            max_error: 0.0,
            max_error_offset: None,
            max_observation_error: 0.0,
        };
        return Ok(SegmentAnalysis {
            accepted: Some(candidate),
            best: candidate,
            end_value,
        });
    }
    if end == start + 1 && constraints.is_none() {
        let easing = if adjacent_samples_form_jump(start, end, maximum_offset, tolerance, evaluate)?
        {
            FittedEasing::Hold
        } else {
            FittedEasing::Linear
        };
        let candidate = SegmentCandidate {
            easing,
            max_error: 0.0,
            max_error_offset: None,
            max_observation_error: 0.0,
        };
        return Ok(SegmentAnalysis {
            accepted: Some(candidate),
            best: candidate,
            end_value,
        });
    }

    let cubic_easing = fit_cubic_easing(start, end, start_value, end_value, constraints, evaluate)?;
    let mut hold = SegmentCandidate {
        easing: FittedEasing::Hold,
        max_error: 0.0,
        max_error_offset: None,
        max_observation_error: 0.0,
    };
    let mut linear = SegmentCandidate {
        easing: FittedEasing::Linear,
        max_error: 0.0,
        max_error_offset: None,
        max_observation_error: 0.0,
    };
    let mut cubic = cubic_easing.map(|easing| SegmentCandidate {
        easing,
        max_error: 0.0,
        max_error_offset: None,
        max_observation_error: 0.0,
    });
    let midpoint = start + (end - start) / 2;

    for offset in start + 1..end {
        let actual = evaluate(offset)?;
        update_error(
            &mut hold,
            actual,
            start_value,
            end_value,
            start,
            end,
            offset,
            midpoint,
        );
        update_error(
            &mut linear,
            actual,
            start_value,
            end_value,
            start,
            end,
            offset,
            midpoint,
        );
        if let Some(candidate) = &mut cubic {
            update_error(
                candidate,
                actual,
                start_value,
                end_value,
                start,
                end,
                offset,
                midpoint,
            );
        }
    }

    if let Some(constraints) = constraints {
        for sample in constraints.in_segment(start, end) {
            for candidate in [&mut hold, &mut linear].into_iter().chain(cubic.iter_mut()) {
                let progress = if end == start {
                    0.0
                } else {
                    (sample.offset_ms - start as f64) / (end - start) as f64
                };
                let expected = if sample.offset_ms == end as f64 {
                    end_value
                } else {
                    start_value + (end_value - start_value) * candidate.easing.progress(progress)
                };
                let error = (expected - sample.value).abs();
                if !error.is_finite() || error > candidate.max_observation_error {
                    candidate.max_observation_error = if error.is_finite() {
                        error
                    } else {
                        f64::INFINITY
                    };
                    if error > constraints.tolerance || !error.is_finite() {
                        // Quantize only the choice of an integer KEY boundary,
                        // never the timestamp/value used in the constraint.
                        candidate.max_error_offset =
                            Some((sample.offset_ms.round() as u64).clamp(start, end));
                    }
                }
            }
        }
    }
    let accepted = [Some(hold), Some(linear), cubic]
        .into_iter()
        .flatten()
        .find(|candidate| {
            candidate.max_error <= tolerance
                && constraints.is_none_or(|constraints| {
                    candidate.max_observation_error <= constraints.tolerance
                })
        });
    let best = [Some(hold), Some(linear), cubic]
        .into_iter()
        .flatten()
        .min_by(|left, right| {
            let score = |candidate: &SegmentCandidate| {
                let integer_error = candidate.max_error / tolerance.max(f64::MIN_POSITIVE);
                constraints.map_or(candidate.max_error, |constraints| {
                    integer_error.max(
                        candidate.max_observation_error
                            / constraints.tolerance.max(f64::MIN_POSITIVE),
                    )
                })
            };
            score(left).total_cmp(&score(right))
        })
        .expect("hold and linear candidates always exist");
    Ok(SegmentAnalysis {
        accepted,
        best,
        end_value,
    })
}

#[allow(clippy::too_many_arguments)]
fn update_error(
    candidate: &mut SegmentCandidate,
    actual: f64,
    start_value: f64,
    end_value: f64,
    start: u64,
    end: u64,
    offset: u64,
    midpoint: u64,
) {
    let progress = (offset - start) as f64 / (end - start) as f64;
    let expected = start_value + (end_value - start_value) * candidate.easing.progress(progress);
    let error = (actual - expected).abs();
    let replace = error > candidate.max_error
        || (error == candidate.max_error
            && candidate
                .max_error_offset
                .is_some_and(|current| offset.abs_diff(midpoint) < current.abs_diff(midpoint)));
    if replace {
        candidate.max_error = error;
        candidate.max_error_offset = Some(offset);
    }
}

fn fit_cubic_easing<E>(
    start: u64,
    end: u64,
    start_value: f64,
    end_value: f64,
    constraints: Option<ObservationConstraints<'_>>,
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<Option<FittedEasing>, E> {
    let delta = end_value - start_value;
    if delta == 0.0
        || (constraints.is_none() && (delta.abs() <= ABSOLUTE_FIT_TOLERANCE || end <= start + 2))
    {
        return Ok(None);
    }

    let mut a11 = 0.0;
    let mut a12 = 0.0;
    let mut a22 = 0.0;
    let mut b1 = 0.0;
    let mut b2 = 0.0;
    for_observation_offsets(start, end, |offset| {
        if offset == start || offset == end {
            return Ok(());
        }
        let progress = (offset - start) as f64 / (end - start) as f64;
        let inverse = 1.0 - progress;
        let basis1 = 3.0 * inverse * inverse * progress;
        let basis2 = 3.0 * inverse * progress * progress;
        let remainder = (evaluate(offset)? - start_value) / delta - progress.powi(3);
        a11 += basis1 * basis1;
        a12 += basis1 * basis2;
        a22 += basis2 * basis2;
        b1 += basis1 * remainder;
        b2 += basis2 * remainder;
        Ok(())
    })?;
    let determinant = a11 * a22 - a12 * a12;
    let (mut y1, mut y2) = if determinant.abs() > f64::EPSILON {
        (
            (b1 * a22 - b2 * a12) / determinant,
            (a11 * b2 - a12 * b1) / determinant,
        )
    } else if constraints.is_some() {
        (1.0 / 3.0, 2.0 / 3.0)
    } else {
        return Ok(None);
    };
    if let Some(constraints) = constraints {
        let basis = |sample: &ScalarObservation| {
            let t = (sample.offset_ms - start as f64) / (end - start) as f64;
            let inverse = 1.0 - t;
            (
                3.0 * inverse * inverse * t,
                3.0 * inverse * t * t,
                (sample.value - start_value) / delta - t * t * t,
            )
        };
        let interior = || {
            constraints
                .in_segment(start, end)
                .iter()
                .filter(|sample| sample.offset_ms > start as f64 && sample.offset_ms < end as f64)
        };
        if let Some(first) = interior().max_by(|left, right| {
            let norm = |sample| {
                let (a, b, _) = basis(sample);
                a * a + b * b
            };
            norm(left).total_cmp(&norm(right))
        }) {
            let (a, b, value) = basis(first);
            let second = interior()
                .filter(|sample| sample.offset_ms != first.offset_ms)
                .max_by(|left, right| {
                    let rank = |sample| {
                        let (c, d, _) = basis(sample);
                        (a * d - b * c).abs()
                    };
                    rank(left).total_cmp(&rank(right))
                });
            if let Some(second) = second {
                let (c, d, other) = basis(second);
                let determinant = a * d - b * c;
                if determinant != 0.0 {
                    y1 = (value * d - b * other) / determinant;
                    y2 = (a * other - value * c) / determinant;
                }
            } else {
                // Preserve the soft least-squares choice in the unconstrained
                // direction while satisfying this one hard observation.
                let correction = (value - a * y1 - b * y2) / (a * a + b * b);
                y1 += a * correction;
                y2 += b * correction;
            }
        }
    }
    if !y1.is_finite() || !y2.is_finite() {
        return Ok(None);
    }
    Ok(Some(FittedEasing::Cubic { y1, y2 }))
}

fn adjacent_samples_form_jump<E>(
    start: u64,
    end: u64,
    maximum_offset: u64,
    tolerance: f64,
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<bool, E> {
    let start_value = evaluate(start)?;
    let end_value = evaluate(end)?;
    let flat_before = start > 0 && (start_value - evaluate(start - 1)?).abs() <= tolerance;
    let flat_after = end < maximum_offset && (end_value - evaluate(end + 1)?).abs() <= tolerance;
    Ok(flat_before || flat_after)
}

fn for_observation_offsets<E>(
    start: u64,
    end: u64,
    mut observe: impl FnMut(u64) -> Result<(), E>,
) -> Result<(), E> {
    observe(start)?;
    let span = end - start;
    let mut ordinal = 1_u128;
    loop {
        let offset = (ordinal * 1_000 + INITIAL_OBSERVATIONS_PER_SECOND / 2)
            / INITIAL_OBSERVATIONS_PER_SECOND;
        if offset >= u128::from(span) {
            break;
        }
        let offset = u64::try_from(offset).expect("observation offset is bounded by a u64 span");
        observe(start + offset)?;
        ordinal += 1;
    }
    if end != start {
        observe(end)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        FittedEasing, ObservationFitError, ScalarObservation, fit_sampled_scalar_curve,
        fit_scalar_curve, fit_scalar_curve_with_observations,
    };

    #[test]
    fn constant_curve_has_one_key() {
        let curve = fit_scalar_curve(100, 1.0e-6, |_| Ok::<_, ()>(7.0)).unwrap();
        assert_eq!(curve.keys.len(), 1);
        assert_eq!(curve.keys[0].offset_ms, 0);
        assert_eq!(curve.keys[0].value, 7.0);
        assert_eq!(curve.keys[0].easing, FittedEasing::Hold);
    }

    #[test]
    fn linear_curve_uses_linear_easing() {
        let curve = fit_scalar_curve(100, 1.0e-6, |offset| Ok::<_, ()>(offset as f64)).unwrap();
        assert_eq!(curve.keys.len(), 2);
        assert_eq!(curve.keys[1].offset_ms, 100);
        assert_eq!(curve.keys[1].easing, FittedEasing::Linear);
    }

    #[test]
    fn jump_is_held_until_transition() {
        let curve = fit_scalar_curve(50, 1.0e-6, |offset| {
            Ok::<_, ()>(if offset < 25 { 0.0 } else { 1.0 })
        })
        .unwrap();
        assert!(curve.keys.iter().any(|key| {
            key.offset_ms == 25 && key.value == 1.0 && key.easing == FittedEasing::Hold
        }));
    }

    #[test]
    fn pulse_and_cubic_preserve_peak_and_curvature() {
        let pulse = fit_scalar_curve(100, 1.0e-6, |offset| {
            let p = offset as f64 / 100.0;
            Ok::<_, ()>(if p <= 0.5 {
                p * p * 4.0
            } else {
                (1.0 - p) * (1.0 - p) * 4.0
            })
        })
        .unwrap();
        assert!(
            pulse
                .keys
                .iter()
                .any(|key| key.offset_ms == 50 && key.value == 1.0)
        );
        assert!(
            pulse
                .keys
                .iter()
                .any(|key| matches!(key.easing, FittedEasing::Cubic { .. }))
        );
    }

    #[test]
    fn sparse_scalar_evaluates_once_and_preserves_linear_and_held_keys() {
        let mut calls = Vec::new();
        let curve = fit_sampled_scalar_curve([0, 7, 19, 30, 54], 0.01, |offset| {
            calls.push(offset);
            Ok::<_, ()>(if offset < 30 {
                0.0
            } else {
                (offset - 30) as f64 + 1.0
            })
        })
        .unwrap();
        assert_eq!(calls, [0, 7, 19, 30, 54]);
        assert_eq!(curve.tolerance, 0.01);
        assert_eq!(
            curve
                .keys
                .iter()
                .map(|key| (key.offset_ms, key.easing))
                .collect::<Vec<_>>(),
            [
                (0, FittedEasing::Hold),
                (30, FittedEasing::Hold),
                (54, FittedEasing::Linear)
            ]
        );
        assert_eq!(
            curve.keys.iter().map(|key| key.value).collect::<Vec<_>>(),
            [0.0, 1.0, 25.0]
        );
    }

    #[test]
    fn sparse_scalar_zero_duration_and_evaluator_error() {
        let single = fit_sampled_scalar_curve([0], f64::INFINITY, |_| Ok::<_, ()>(3.0)).unwrap();
        assert_eq!(single.keys.len(), 1);
        assert_eq!(single.keys[0].value, 3.0);
        assert_eq!(single.tolerance, super::ABSOLUTE_FIT_TOLERANCE);
        let error = fit_sampled_scalar_curve([0, 12], 0.01, |offset| {
            if offset == 12 { Err("failed") } else { Ok(0.0) }
        });
        assert_eq!(error.unwrap_err(), "failed");
    }

    #[test]
    fn actual_time_constraints_keep_legacy_and_empty_opt_in_identical() {
        for mode in 0..3 {
            let evaluate = |offset: u64| {
                Ok::<_, ()>(match mode {
                    0 => 7.0,
                    1 => offset as f64 / 100.0,
                    _ => (offset as f64 / 20.0).sin(),
                })
            };
            assert_eq!(
                fit_scalar_curve(100, 0.001, evaluate).unwrap(),
                fit_scalar_curve_with_observations(100, 0.001, &[], 1e-12, evaluate).unwrap()
            );
        }
    }

    #[test]
    fn actual_time_constant_and_linear_constraints_remain_sparse() {
        for linear in [false, true] {
            let samples = [0.0, 0.25, 42.75, 99.125, 100.0].map(|offset_ms| ScalarObservation {
                offset_ms,
                value: if linear { offset_ms } else { 7.0 },
            });
            let curve = fit_scalar_curve_with_observations(100, 0.001, &samples, 1e-12, |offset| {
                Ok::<_, ()>(if linear { offset as f64 } else { 7.0 })
            })
            .unwrap();
            assert_eq!(curve.keys.len(), if linear { 2 } else { 1 });
        }
    }

    #[test]
    fn actual_time_two_fractional_constraints_fit_adjacent_ms_cubic() {
        let samples = [0.25_f64, 0.75].map(|offset_ms| ScalarObservation {
            offset_ms,
            value: offset_ms.powi(3),
        });
        let curve = fit_scalar_curve_with_observations(1, 0.001, &samples, 1e-12, |offset| {
            Ok::<_, ()>(offset as f64)
        })
        .unwrap();
        assert_eq!(curve.keys.len(), 2);
        let FittedEasing::Cubic { .. } = curve.keys[1].easing else {
            panic!("fractional constraints need cubic controls")
        };
        for sample in samples {
            assert!(
                (curve.keys[1].easing.progress(sample.offset_ms) - sample.value).abs() <= 1e-12
            );
        }
    }

    #[test]
    fn actual_time_single_fractional_constraint_is_not_only_an_acceptance_probe() {
        let samples = [ScalarObservation {
            offset_ms: 0.5,
            value: 0.25,
        }];
        let curve = fit_scalar_curve_with_observations(1, 0.001, &samples, 1e-12, |offset| {
            Ok::<_, ()>(offset as f64)
        })
        .unwrap();
        assert!((curve.keys[1].easing.progress(0.5) - 0.25).abs() <= 1e-12);
    }

    #[test]
    fn actual_time_pulses_and_inconsistent_constraints_are_diagnosed() {
        let pulse = [ScalarObservation {
            offset_ms: 0.5,
            value: 1.0,
        }];
        assert!(matches!(
            fit_scalar_curve_with_observations(1, 0.001, &pulse, 1e-12, |_| Ok::<_, ()>(0.0)),
            Err(ObservationFitError::Unrepresentable {
                start_ms: 0,
                end_ms: 1
            })
        ));
        let inconsistent = [(0.25, 0.0), (0.5, 1.0), (0.75, 0.0)]
            .map(|(offset_ms, value)| ScalarObservation { offset_ms, value });
        assert!(matches!(
            fit_scalar_curve_with_observations(
                1,
                0.001,
                &inconsistent,
                1e-12,
                |offset| Ok::<_, ()>(offset as f64)
            ),
            Err(ObservationFitError::Unrepresentable { .. })
        ));
        assert!(matches!(
            fit_scalar_curve_with_observations(
                0,
                0.001,
                &[ScalarObservation {
                    offset_ms: 0.0,
                    value: 1.0
                }],
                1e-12,
                |_| Ok::<_, ()>(0.0)
            ),
            Err(ObservationFitError::Unrepresentable {
                start_ms: 0,
                end_ms: 0
            })
        ));
    }

    #[test]
    fn actual_time_constraints_validate_clocks_values_and_propagate_errors() {
        for samples in [
            vec![ScalarObservation {
                offset_ms: f64::NAN,
                value: 0.0,
            }],
            vec![ScalarObservation {
                offset_ms: 0.5,
                value: f64::INFINITY,
            }],
            vec![ScalarObservation {
                offset_ms: 2.0,
                value: 0.0,
            }],
            vec![
                ScalarObservation {
                    offset_ms: 0.5,
                    value: 0.0
                };
                2
            ],
        ] {
            assert!(matches!(
                fit_scalar_curve_with_observations(1, 0.001, &samples, 1e-12, |_| Ok::<_, ()>(0.0)),
                Err(ObservationFitError::InvalidObservations(_))
            ));
        }
        assert!(matches!(
            fit_scalar_curve_with_observations(1, 0.001, &[], 1e-12, |_| Err::<f64, _>("failed")),
            Err(ObservationFitError::Evaluation("failed"))
        ));
    }

    #[test]
    fn evaluator_error_is_propagated() {
        let error = fit_scalar_curve(100, 1.0e-6, |offset| {
            if offset == 50 {
                Err("sample failed")
            } else {
                Ok(0.0)
            }
        })
        .unwrap_err();
        assert_eq!(error, "sample failed");
    }
}
