//! Converter-local distance-along-path approximation for native spatial Position.

use super::{
    NumericAnimationClock, NumericKeyframe, NumericProperty, PropertyKeyframeEasing, TimeOffset,
    easing_for_key,
};

#[cfg(test)]
mod tests;

const POSITION_TOLERANCE: f64 = 0.25;
const LENGTH_TOLERANCE: f64 = 0.001;
const MAX_DEPTH: u32 = 20;
// Bounds generated work across the entire property, not just recursion depth.
// This is an approximation-convergence limit, not a source input-size quota.
const MAX_REFINEMENT_WORK: usize = 65_536;

fn charge_work(remaining: &mut usize, samples: usize) -> Result<(), String> {
    *remaining = remaining
        .checked_sub(samples)
        .ok_or("spatial Position total refinement work limit exceeded")?;
    Ok(())
}

pub(super) fn prepare(
    numeric: &NumericProperty,
    clock: NumericAnimationClock,
) -> Result<Option<NumericProperty>, String> {
    if !numeric.keyframes.windows(2).any(|pair| {
        pair[0]
            .spatial_out
            .iter()
            .chain(&pair[1].spatial_in)
            .any(|v| *v != 0.)
    }) {
        return Ok(None);
    }
    let mut remaining_work = MAX_REFINEMENT_WORK;
    let mut prepared = numeric.clone();
    prepared.keyframes.clear();
    prepared.keyframes.push(linear_key(&numeric.keyframes[0]));
    for pair in numeric.keyframes.windows(2) {
        let (from, to) = (&pair[0], &pair[1]);
        let dimensions = from.values.len();
        if !(2..=3).contains(&dimensions)
            || to.values.len() != dimensions
            || from.spatial_out.len() != dimensions
            || to.spatial_in.len() != dimensions
            || from
                .values
                .iter()
                .chain(&to.values)
                .chain(&from.spatial_out)
                .chain(&to.spatial_in)
                .any(|v| !v.is_finite())
            || curve_controls_overflow(from, to)
            || !from.time_secs.is_finite()
            || !to.time_secs.is_finite()
            || to.time_secs <= from.time_secs
        {
            return Err("invalid spatial Position geometry or clock".into());
        }
        let first_ms = target_millis(from, clock)?;
        let last_ms = target_millis(to, clock)?;
        if first_ms == last_ms || (last_ms < first_ms) != clock.reversed() {
            return Err("spatial Position authored endpoints collide or reverse on the FX millisecond clock".into());
        }
        if from.out_interpolation == 3 {
            prepared
                .keyframes
                .last_mut()
                .ok_or("missing spatial Position start key")?
                .out_interpolation = 3;
            prepared.keyframes.push(linear_key(to));
            continue;
        }
        let curve: Vec<[f64; 4]> = from
            .values
            .iter()
            .zip(&to.values)
            .zip(&from.spatial_out)
            .zip(&to.spatial_in)
            .map(|(((a, b), out), input)| [*a, a + out, b + input, *b])
            .collect();
        let straight = from
            .spatial_out
            .iter()
            .chain(&to.spatial_in)
            .all(|value| *value == 0.);
        if straight {
            // Two endpoint samples replace the redundant straight arc table.
            charge_work(&mut remaining_work, 2)?;
        }
        let mut table = vec![(0., from.values.clone())];
        if straight {
            table.push((1., to.values.clone()));
        } else {
            subdivide_curve(&curve, 0., 1., 0, &mut table, &mut remaining_work)?;
        }
        let mut lengths = vec![0.];
        for pair in table.windows(2) {
            lengths.push(lengths.last().copied().unwrap_or(0.) + distance(&pair[0].1, &pair[1].1));
        }
        let length = lengths.last().copied().unwrap_or(0.);
        if !length.is_finite() {
            return Err("non-finite spatial Position path length".into());
        }
        if length == 0. {
            prepared.keyframes.push(linear_key(to));
            continue;
        }
        if [
            from.out_speed.as_slice(),
            from.out_influence.as_slice(),
            to.in_speed.as_slice(),
            to.in_influence.as_slice(),
        ]
        .iter()
        .any(|v| v.len() != 1 || !v[0].is_finite())
            || from.out_speed[0] < 0.
            || to.in_speed[0] < 0.
            || !matches!(from.out_interpolation, 1 | 2)
            || !matches!(to.in_interpolation, 1 | 2)
        {
            return Err("unsupported spatial Position shared temporal speed".into());
        }
        let mut a = from.clone();
        let mut b = to.clone();
        a.values = vec![0.];
        b.values = vec![length];
        // This is a scalar *distance*, not an axis delta. The ordinary temporal
        // ease decoder can therefore be reused without sign-dependent handles.
        a.spatial_in.clear();
        a.spatial_out.clear();
        b.spatial_in.clear();
        b.spatial_out.clear();
        let easing = easing_for_key(
            &[a, b],
            1,
            0,
            1.,
            &mut Vec::new(),
            "spatial Position distance",
        );
        if let PropertyKeyframeEasing::CubicBezier { y1, y2, .. } = easing
            && !(0. <= y1 && y1 <= y2 && y2 <= 1.)
        {
            return Err("nonmonotone spatial Position temporal distance".into());
        }
        let evaluate = |time: f64| {
            let progress = temporal_progress(time, &easing);
            if straight {
                // With zero handles, cubic parameter speed is nonuniform but
                // normalized traveled distance gives an exact endpoint lerp.
                // Applying cubic(progress) here would ease the geometry twice.
                return from
                    .values
                    .iter()
                    .zip(&to.values)
                    .map(|(a, b)| a + (b - a) * progress)
                    .collect();
            }
            let target = progress * length;
            let index = lengths
                .partition_point(|v| *v < target)
                .clamp(1, lengths.len() - 1);
            let span = lengths[index] - lengths[index - 1];
            let fraction = if span == 0. {
                0.
            } else {
                (target - lengths[index - 1]) / span
            };
            let parameter = table[index - 1].0 + (table[index].0 - table[index - 1].0) * fraction;
            curve
                .iter()
                .map(|controls| cubic(parameter, *controls))
                .collect::<Vec<_>>()
        };
        fit_interval(
            from,
            to,
            &evaluate,
            clock,
            0.,
            1.,
            from.values.clone(),
            to.values.clone(),
            0,
            &mut prepared.keyframes,
            &mut remaining_work,
        )?;
    }
    Ok(Some(prepared))
}

fn curve_controls_overflow(from: &NumericKeyframe, to: &NumericKeyframe) -> bool {
    from.values
        .iter()
        .zip(&from.spatial_out)
        .chain(to.values.iter().zip(&to.spatial_in))
        .any(|(value, tangent)| !(value + tangent).is_finite())
}

fn linear_key(source: &NumericKeyframe) -> NumericKeyframe {
    let mut key = source.clone();
    key.in_interpolation = 1;
    key.out_interpolation = 1;
    key.spatial_in.clear();
    key.spatial_out.clear();
    key
}

fn cubic(t: f64, p: [f64; 4]) -> f64 {
    let s = 1. - t;
    s * s * s * p[0] + 3. * s * s * t * p[1] + 3. * s * t * t * p[2] + t * t * t * p[3]
}

fn distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .fold(0_f64, |length, (a, b)| length.hypot(b - a))
}

fn subdivide_curve(
    curve: &[[f64; 4]],
    lo: f64,
    hi: f64,
    depth: u32,
    table: &mut Vec<(f64, Vec<f64>)>,
    remaining_work: &mut usize,
) -> Result<(), String> {
    // Five curve samples and three comparison vectors; charge before allocation.
    charge_work(remaining_work, 8)?;
    let points: Vec<Vec<f64>> = (0..=4)
        .map(|i| {
            curve
                .iter()
                .map(|p| cubic(lo + (hi - lo) * f64::from(i) / 4., *p))
                .collect()
        })
        .collect();
    let polygon: f64 = points
        .windows(2)
        .map(|pair| distance(&pair[0], &pair[1]))
        .sum();
    let chord = distance(&points[0], &points[4]);
    // Also bound parameter-to-distance linearization: even a collinear cubic
    // can have nonuniform parameter speed and needs a lookup subdivision.
    let parameter_error = (1_usize..4)
        .map(|i| {
            let linear: Vec<f64> = points[0]
                .iter()
                .zip(&points[4])
                .map(|(a, b)| a + (b - a) * i as f64 / 4.)
                .collect();
            distance(&points[i], &linear)
        })
        .fold(0_f64, f64::max);
    if polygon - chord <= LENGTH_TOLERANCE * (hi - lo) && parameter_error <= LENGTH_TOLERANCE {
        table.push((hi, points[4].clone()));
        return Ok(());
    }
    if depth == MAX_DEPTH {
        return Err("spatial Position arc-length refinement did not converge".into());
    }
    let mid = (lo + hi) / 2.;
    subdivide_curve(curve, lo, mid, depth + 1, table, remaining_work)?;
    subdivide_curve(curve, mid, hi, depth + 1, table, remaining_work)
}

fn temporal_progress(time: f64, easing: &PropertyKeyframeEasing) -> f64 {
    let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = *easing else {
        return time;
    };
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..52 {
        let mid = (lo + hi) / 2.;
        if cubic(mid, [0., x1, x2, 1.]) < time {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic((lo + hi) / 2., [0., y1, y2, 1.])
}

#[allow(clippy::too_many_arguments)]
fn fit_interval(
    from: &NumericKeyframe,
    to: &NumericKeyframe,
    evaluate: &impl Fn(f64) -> Vec<f64>,
    clock: NumericAnimationClock,
    lo: f64,
    hi: f64,
    a: Vec<f64>,
    b: Vec<f64>,
    depth: u32,
    output: &mut Vec<NumericKeyframe>,
    remaining_work: &mut usize,
) -> Result<(), String> {
    // Three sampled positions, three comparison vectors, and at most one midpoint.
    charge_work(remaining_work, 7)?;
    // Recomposition can move an authored endpoint across a millisecond boundary.
    // Only generated interior samples need the affine interpolation.
    let source_time = |unit| {
        if unit == 0. {
            from.time_secs
        } else if unit == 1. {
            to.time_secs
        } else {
            from.time_secs + (to.time_secs - from.time_secs) * unit
        }
    };
    let first_ms = target_millis_at(source_time(lo), clock)?;
    let last_ms = target_millis_at(source_time(hi), clock)?;
    let error = (1..4)
        .map(|i| {
            let sample_ms = interior_millis(first_ms, last_ms, i, 4)?;
            if sample_ms == first_ms || sample_ms == last_ms {
                return Ok(0.);
            }
            let sample = source_unit(sample_ms, from, to, clock)?;
            // Milliseconds are limited to exact f64 integers by target_millis_at.
            let fraction =
                (sample_ms as f64 - first_ms as f64) / (last_ms as f64 - first_ms as f64);
            let actual = evaluate(sample);
            let linear: Vec<f64> = a
                .iter()
                .zip(&b)
                .map(|(a, b)| a + (b - a) * fraction)
                .collect();
            Ok(distance(&actual, &linear))
        })
        .try_fold(0_f64, |error, sample: Result<f64, String>| {
            sample.map(|sample| error.max(sample))
        })?;
    if error <= POSITION_TOLERANCE {
        let mut key = linear_key(to);
        key.time_secs = source_time(hi);
        key.values = b;
        output.push(key);
        return Ok(());
    }
    if depth == MAX_DEPTH {
        return Err("spatial Position adaptive key fitting did not converge".into());
    }
    let mid_ms = interior_millis(first_ms, last_ms, 1, 2)?;
    let mid = source_unit(mid_ms, from, to, clock)?;
    if !(lo < mid && mid < hi) {
        return Err("spatial Position target-clock subdivision is not representable".into());
    }
    let point = evaluate(mid);
    fit_interval(
        from,
        to,
        evaluate,
        clock,
        lo,
        mid,
        a,
        point.clone(),
        depth + 1,
        output,
        remaining_work,
    )?;
    fit_interval(
        from,
        to,
        evaluate,
        clock,
        mid,
        hi,
        point,
        b,
        depth + 1,
        output,
        remaining_work,
    )
}

fn target_millis(key: &NumericKeyframe, clock: NumericAnimationClock) -> Result<i64, String> {
    target_millis_at(key.time_secs, clock)
}

fn target_millis_at(source_secs: f64, clock: NumericAnimationClock) -> Result<i64, String> {
    let milliseconds = clock.seconds(source_secs) * 1000.;
    // Avoid saturating TimeOffset conversion and retain exact integer arithmetic
    // in the affine inverse. Cancellation is additionally checked by round-trip.
    if !milliseconds.is_finite() || milliseconds.abs() > (1_u64 << 52) as f64 {
        return Err("spatial Position FX millisecond clock is outside the fitting range".into());
    }
    Ok(TimeOffset::from_millis_f64(milliseconds).as_millis())
}

fn interior_millis(
    first: i64,
    last: i64,
    numerator: i128,
    denominator: i128,
) -> Result<i64, String> {
    // Wider subtraction/multiplication keeps both reversed and negative clocks
    // overflow-free. Truncation selects an addressable point toward the start.
    i64::try_from(
        i128::from(first) + (i128::from(last) - i128::from(first)) * numerator / denominator,
    )
    .map_err(|_| "spatial Position target-clock midpoint overflow".into())
}

fn source_unit(
    milliseconds: i64,
    from: &NumericKeyframe,
    to: &NumericKeyframe,
    clock: NumericAnimationClock,
) -> Result<f64, String> {
    // The caller admits only exactly representable integer milliseconds.
    let seconds = milliseconds as f64 / 1000.;
    let source = match clock {
        NumericAnimationClock::SourceLocal { offset_secs } => seconds + offset_secs,
        NumericAnimationClock::ParentIdentity { start, stretch } => (seconds - start) / stretch,
    };
    let unit = (source - from.time_secs) / (to.time_secs - from.time_secs);
    let reconstructed = from.time_secs + (to.time_secs - from.time_secs) * unit;
    if !unit.is_finite() || target_millis_at(reconstructed, clock)? != milliseconds {
        return Err("spatial Position target-clock inverse does not round-trip".into());
    }
    Ok(unit.clamp(0., 1.))
}
