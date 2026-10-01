//! Native keys for sampled tracks, fitted by the shared scalar fitter.
//!
//! The shared fitter chooses every key and easing. This adapter only checks
//! that its segments are native keys: Premiere keys a point's two axes at
//! the same times with one easing, some graphic parameters hold no cubic
//! Bezier easing, and a scalar key that starts a Hold drops its cubic
//! in-handle. A segment that breaks one of these rules is fitted again
//! inside its own window, from the axis whose fit the other axes follow.
//! Where cubic easing cannot stay, or no axis makes progress, the segment is
//! Linear or Hold, or is split where an axis leaves its straight line
//! furthest. A one-millisecond segment has no integer millisecond inside, so
//! every easing matches it there.
//!
//! Every fitted curve is then checked, by arithmetic, against every sampled
//! millisecond within each axis's tolerance: the shared fitter's tolerance
//! for the whole window, which is at most the target's cap.

use fx_keyframe_bake::curve_fit::{fit_scalar_curve, FittedCurve, FittedEasing};

use super::{super::keyframes::premiere_keeps_the_arrival_into_a_hold, owners::Rules};
use crate::schema::PrKeyframeEasing;

/// Sample lookups and error checks that fitting one axis may spend per
/// sampled millisecond. Measured fits of real scripts spend far fewer; the
/// bound stops a pathological curve rather than shaping ordinary ones.
const WORK_PER_SAMPLE: usize = 512;

/// One sampled axis of a track, and its tolerance cap in FX units.
#[derive(Debug, Clone, Copy)]
pub(super) struct Axis<'a> {
    /// The value at every millisecond of the window, from 0.
    pub(super) values: &'a [f64],
    pub(super) cap: f64,
}

/// One fitted key shared by every axis of a track, with the easing into it.
/// Its value on each axis is that axis's sample at `offset_ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Key {
    pub(super) offset_ms: u64,
    pub(super) easing: FittedEasing,
}

/// Why a sampled track has no native keys.
#[derive(Debug, thiserror::Error)]
pub(super) enum FitError {
    #[error("the value range overflows the fit tolerance")]
    Tolerance,
    #[error("fitting exceeded its bound of {0} work steps")]
    Work(usize),
    #[error("the fitted keys miss the script by more than {tolerance} at layer time {time_ms} ms")]
    Validation { time_ms: u64, tolerance: f64 },
}

/// Fit `axes`, which share one window, to keys that `rules` allow.
pub(super) fn fit(axes: &[Axis<'_>], rules: &Rules) -> Result<Vec<Key>, FitError> {
    let samples = axes[0].values.len();
    let window = u64::try_from(samples - 1).expect("sample counts fit u64");
    let mut fitter = Fitter {
        axes,
        tolerances: Vec::with_capacity(axes.len()),
        rules,
        work: 0,
        limit: WORK_PER_SAMPLE
            .saturating_mul(samples)
            .saturating_mul(axes.len()),
    };
    let mut first = None;
    for (index, axis) in axes.iter().enumerate() {
        let curve = fitter.fit_window(index, 0, window, axis.cap)?;
        if !curve.tolerance.is_finite() {
            return Err(FitError::Tolerance);
        }
        fitter.tolerances.push(curve.tolerance);
        first.get_or_insert(curve);
    }
    let curve = first.expect("a track has at least one axis");
    let mut keys = vec![Key {
        offset_ms: 0,
        easing: FittedEasing::Hold,
    }];
    let mut previous = 0;
    for key in &curve.keys[1..] {
        keys.extend(fitter.segment(previous, key.offset_ms, key.easing, true)?);
        previous = key.offset_ms;
    }
    // The first axis's fit ends at its last change; another axis may not.
    if previous < window && !fitter.holds(previous, window)? {
        keys.extend(fitter.refine(previous, window, true)?);
    }
    if rules.scalar_keys {
        while let Some(index) = cubic_into_hold(&keys) {
            let start = keys[index - 1].offset_ms;
            let replacement = fitter.refine(start, keys[index].offset_ms, false)?;
            keys.splice(index..=index, replacement);
        }
    }
    for (axis, tolerance) in axes.iter().zip(&fitter.tolerances) {
        follows(&keys, axis.values, axis.values, *tolerance).map_err(|time_ms| {
            FitError::Validation {
                time_ms,
                tolerance: *tolerance,
            }
        })?;
    }
    Ok(keys)
}

/// The shared fitter's tolerance for `axis` over its whole window.
pub(super) fn tolerance(axis: Axis<'_>) -> Result<f64, FitError> {
    let window = u64::try_from(axis.values.len() - 1).expect("sample counts fit u64");
    let curve = fit_scalar_curve(window, axis.cap, |offset| {
        Ok::<_, FitError>(axis.values[index(offset)])
    })?;
    curve
        .tolerance
        .is_finite()
        .then_some(curve.tolerance)
        .ok_or(FitError::Tolerance)
}

/// Whether `target` stays within `tolerance` of the curve through `keys`
/// that takes each key's value from `source`: the first layer time where it
/// does not, otherwise. After the last key the curve holds its value.
pub(super) fn follows(
    keys: &[Key],
    source: &[f64],
    target: &[f64],
    tolerance: f64,
) -> Result<(), u64> {
    let mut left = 0;
    for (time, actual) in (0_u64..).zip(target) {
        while left + 1 < keys.len() && keys[left + 1].offset_ms <= time {
            left += 1;
        }
        let key = keys[left];
        let from = source[index(key.offset_ms)];
        let expected = match keys.get(left + 1) {
            Some(next) => {
                let span = (next.offset_ms - key.offset_ms) as f64;
                let progress = (time - key.offset_ms) as f64 / span;
                from + (source[index(next.offset_ms)] - from) * next.easing.progress(progress)
            }
            None => from,
        };
        let error = (actual - expected).abs();
        // A NaN error fails too.
        if error.is_nan() || error > tolerance {
            return Err(time);
        }
    }
    Ok(())
}

/// The first key whose cubic arrival Premiere drops because the key starts
/// a Hold (`export_scalar_keys`), if any.
fn cubic_into_hold(keys: &[Key]) -> Option<usize> {
    (1..keys.len().saturating_sub(1)).find(|&index| {
        let FittedEasing::Cubic { y1, y2 } = keys[index].easing else {
            return false;
        };
        keys[index + 1].easing == FittedEasing::Hold
            && !premiere_keeps_the_arrival_into_a_hold(PrKeyframeEasing::CubicBezier {
                x1: 1.0 / 3.0,
                y1,
                x2: 2.0 / 3.0,
                y2,
            })
    })
}

fn index(offset: u64) -> usize {
    usize::try_from(offset).expect("sample offsets index the samples")
}

struct Fitter<'a> {
    axes: &'a [Axis<'a>],
    tolerances: Vec<f64>,
    rules: &'a Rules,
    work: usize,
    limit: usize,
}

impl Fitter<'_> {
    fn spend(&mut self, steps: usize) -> Result<(), FitError> {
        self.work = self.work.saturating_add(steps);
        if self.work > self.limit {
            return Err(FitError::Work(self.limit));
        }
        Ok(())
    }

    /// The shared fitter's curve of `axis` from `start` to `end`, with key
    /// offsets from `start`.
    fn fit_window(
        &mut self,
        axis: usize,
        start: u64,
        end: u64,
        cap: f64,
    ) -> Result<FittedCurve, FitError> {
        let values = self.axes[axis].values;
        let remaining = self.limit - self.work;
        let mut steps = 0_usize;
        let curve = fit_scalar_curve(end - start, cap, |offset| {
            steps += 1;
            if steps > remaining {
                return Err(FitError::Work(0));
            }
            Ok(values[index(start + offset)])
        })
        .map_err(|error| match error {
            FitError::Work(_) => FitError::Work(self.limit),
            error => error,
        })?;
        self.spend(steps)?;
        Ok(curve)
    }

    /// Whether every axis follows `easing` from `start` to `end` within its
    /// tolerance, where `cubic` and the rules allow that easing.
    fn fits(
        &mut self,
        start: u64,
        end: u64,
        easing: FittedEasing,
        cubic: bool,
    ) -> Result<bool, FitError> {
        if matches!(easing, FittedEasing::Cubic { .. }) && !(cubic && self.rules.cubic) {
            return Ok(false);
        }
        let span = end - start;
        self.spend(index(span).saturating_mul(self.axes.len()))?;
        Ok(self
            .axes
            .iter()
            .zip(&self.tolerances)
            .all(|(axis, tolerance)| {
                let from = axis.values[index(start)];
                let to = axis.values[index(end)];
                (start + 1..end).all(|time| {
                    let progress = (time - start) as f64 / span as f64;
                    let expected = from + (to - from) * easing.progress(progress);
                    (axis.values[index(time)] - expected).abs() <= *tolerance
                })
            }))
    }

    /// Whether every axis keeps its value at `start` through `end`.
    fn holds(&mut self, start: u64, end: u64) -> Result<bool, FitError> {
        self.spend(index(end - start).saturating_mul(self.axes.len()))?;
        Ok(self
            .axes
            .iter()
            .zip(&self.tolerances)
            .all(|(axis, tolerance)| {
                let value = axis.values[index(start)];
                (start + 1..=end).all(|time| (axis.values[index(time)] - value).abs() <= *tolerance)
            }))
    }

    /// Keys after `start` through `end`: one segment with `easing` when every
    /// axis follows it, otherwise [`Self::refine`]'s.
    fn segment(
        &mut self,
        start: u64,
        end: u64,
        easing: FittedEasing,
        cubic: bool,
    ) -> Result<Vec<Key>, FitError> {
        if self.fits(start, end, easing, cubic)? {
            return Ok(vec![Key {
                offset_ms: end,
                easing,
            }]);
        }
        self.refine(start, end, cubic)
    }

    /// Keys after `start` through `end` that every axis follows. Where cubic
    /// easing is allowed, the shared fitter's keys of the first axis whose
    /// fit of this window splits it or fits every axis, each segment refined
    /// again where it must be. Otherwise, or when no axis makes progress, a
    /// Linear or Hold segment, or two halves split at the largest linear error.
    fn refine(&mut self, start: u64, end: u64, cubic: bool) -> Result<Vec<Key>, FitError> {
        if end - start <= 1 {
            return Ok(vec![Key {
                offset_ms: end,
                easing: FittedEasing::Linear,
            }]);
        }
        let axes = if cubic && self.rules.cubic {
            self.axes.len()
        } else {
            // The shared fitter prefers cubic segments, which cannot stay.
            0
        };
        for axis in 0..axes {
            let curve = self.fit_window(axis, start, end, self.tolerances[axis])?;
            let mut pieces = Vec::with_capacity(curve.keys.len());
            let mut previous = start;
            for key in &curve.keys[1..] {
                pieces.push((previous, start + key.offset_ms, key.easing));
                previous = start + key.offset_ms;
            }
            // A trimmed final hold keeps the last value, which equals the end's.
            if previous < end {
                pieces.push((previous, end, FittedEasing::Linear));
            }
            if let [(_, _, easing)] = pieces.as_slice() {
                if self.fits(start, end, *easing, cubic)? {
                    return Ok(vec![Key {
                        offset_ms: end,
                        easing: *easing,
                    }]);
                }
                continue;
            }
            let mut keys = Vec::new();
            for (from, to, easing) in pieces {
                keys.extend(self.segment(from, to, easing, cubic)?);
            }
            return Ok(keys);
        }
        for easing in [FittedEasing::Linear, FittedEasing::Hold] {
            if self.fits(start, end, easing, cubic)? {
                return Ok(vec![Key {
                    offset_ms: end,
                    easing,
                }]);
            }
        }
        let split = self.largest_linear_error(start, end)?;
        let mut keys = self.refine(start, split, cubic)?;
        keys.extend(self.refine(split, end, cubic)?);
        Ok(keys)
    }

    /// The millisecond inside `start..end` where an axis leaves the straight
    /// line between its values at the ends furthest, relative to its
    /// tolerance.
    fn largest_linear_error(&mut self, start: u64, end: u64) -> Result<u64, FitError> {
        self.spend(index(end - start).saturating_mul(self.axes.len()))?;
        let span = (end - start) as f64;
        let mut largest = (start + (end - start) / 2, f64::NEG_INFINITY);
        for (axis, tolerance) in self.axes.iter().zip(&self.tolerances) {
            let from = axis.values[index(start)];
            let to = axis.values[index(end)];
            for time in start + 1..end {
                let expected = from + (to - from) * (time - start) as f64 / span;
                let error = (axis.values[index(time)] - expected).abs() / tolerance;
                if error > largest.1 {
                    largest = (time, error);
                }
            }
        }
        Ok(largest.0)
    }
}
