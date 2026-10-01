//! Adaptive, millisecond-resolution fitting of an evaluated scalar curve.

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
}

struct SegmentAnalysis {
    accepted: Option<SegmentCandidate>,
    best: SegmentCandidate,
    end_value: f64,
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
    let observations = initial_observations(duration_ms, &mut evaluate)?;
    let tolerance = observation_tolerance(&observations).min(tolerance_cap);
    let boundaries = mandatory_boundaries(&observations, duration_ms, &mut evaluate)?;
    let mut pending = boundaries
        .windows(2)
        .rev()
        .map(|window| (window[0], window[1]))
        .collect::<Vec<_>>();
    let mut segments = Vec::new();

    while let Some((start, end)) = pending.pop() {
        let analysis = analyze_segment(start, end, duration_ms, tolerance, &mut evaluate)?;
        if let Some(candidate) = analysis.accepted {
            segments.push((start, end, analysis.end_value, candidate.easing));
            continue;
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
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<SegmentAnalysis, E> {
    let start_value = evaluate(start)?;
    let end_value = evaluate(end)?;
    if end == start {
        let candidate = SegmentCandidate {
            easing: FittedEasing::Hold,
            max_error: 0.0,
            max_error_offset: None,
        };
        return Ok(SegmentAnalysis {
            accepted: Some(candidate),
            best: candidate,
            end_value,
        });
    }
    if end == start + 1 {
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
        };
        return Ok(SegmentAnalysis {
            accepted: Some(candidate),
            best: candidate,
            end_value,
        });
    }

    let cubic_easing = fit_cubic_easing(start, end, start_value, end_value, evaluate)?;
    let mut hold = SegmentCandidate {
        easing: FittedEasing::Hold,
        max_error: 0.0,
        max_error_offset: None,
    };
    let mut linear = SegmentCandidate {
        easing: FittedEasing::Linear,
        max_error: 0.0,
        max_error_offset: None,
    };
    let mut cubic = cubic_easing.map(|easing| SegmentCandidate {
        easing,
        max_error: 0.0,
        max_error_offset: None,
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

    let accepted = [Some(hold), Some(linear), cubic]
        .into_iter()
        .flatten()
        .find(|candidate| candidate.max_error <= tolerance);
    let best = [Some(hold), Some(linear), cubic]
        .into_iter()
        .flatten()
        .min_by(|left, right| left.max_error.total_cmp(&right.max_error))
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
    evaluate: &mut impl FnMut(u64) -> Result<f64, E>,
) -> Result<Option<FittedEasing>, E> {
    let delta = end_value - start_value;
    if delta.abs() <= ABSOLUTE_FIT_TOLERANCE || end <= start + 2 {
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
    if determinant.abs() <= f64::EPSILON {
        return Ok(None);
    }
    let y1 = (b1 * a22 - b2 * a12) / determinant;
    let y2 = (a11 * b2 - a12 * b1) / determinant;
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
    use super::{FittedEasing, fit_scalar_curve};

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
