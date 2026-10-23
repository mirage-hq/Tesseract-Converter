//! Lumetri RGB Curves as an FX `colorCurves`.
//!
//! Measured in Premiere 26.5.2 (renders of in-house byte patterns on an 8-bit
//! grey ramp and hue/saturation sweeps, Rec. 709 sequence): the four saved
//! curves are Luma (master), Red, Green and Blue in that order; points are
//! display code values 0 to 1 on both axes; Premiere draws a natural cubic
//! spline through the points (zero second derivative at both ends) and clamps
//! its output to 0 to 1. The Red, Green and Blue curves act on their channel
//! alone (within one 8-bit code). The Luma curve maps grey exactly like a
//! per-channel curve, but leaves fully saturated colours unchanged, so FX's
//! per-channel master curve differs from it on coloured pixels. Export
//! writes the FX points back as Lumetri points where Premiere's spline
//! through them stays within one 8-bit code of FX's straight segments, and
//! otherwise points chosen so that the spline follows them; a non-identity
//! FX Master, which FX applies per channel before the channel curves, is
//! folded into the Red, Green and Blue curves instead of Lumetri's Luma
//! curve.
use super::RgbCurves;
use fx_schema::{
    ColorCurve, ColorCurvePoint, ColorCurves, ColorCurvesSemanticVersion, LayerEffect,
    MAX_COLOR_CURVE_POINTS,
};

/// Samples of a curve at 8-bit input codes.
const SAMPLES: usize = 256;

/// Largest fitted deviation from Premiere's spline, in 8-bit codes.
const FIT_TOLERANCE_CODES: f64 = 1.0;

pub const LUMETRI_CURVES_APPROXIMATION: &str = "Lumetri RGB Curves map to colorCurves: Premiere's natural cubic spline through each curve's saved points (output clamped to 0 to 1) is fitted by at most 32 piecewise-linear points within one 8-bit code, and the Red, Green and Blue curves act per channel as in Premiere; the Luma curve becomes the FX Master curve, which equals Premiere's on greys but also changes saturated colours that Premiere's Luma curve leaves unchanged; export folds a non-identity Master into the Red, Green and Blue curves and writes an identity Luma curve, so a Luma curve does not survive a round trip unchanged";

/// The FX `colorCurves` that follows the saved Lumetri RGB Curves.
pub fn color_curves(curves: &RgbCurves) -> Result<LayerEffect, String> {
    let [master, red, green, blue] = &curves.curves;
    Ok(LayerEffect::ColorCurves {
        semantic_version: ColorCurvesSemanticVersion::V1,
        curves: ColorCurves {
            master: fitted(master)?,
            red: fitted(red)?,
            green: fitted(green)?,
            blue: fitted(blue)?,
        },
    })
}

/// The Lumetri RGB Curves written for the FX `curves`, and their export
/// diagnostic. With an identity Master, Luma stays as it is and each of Red,
/// Green and Blue keeps its FX points when Premiere's natural cubic spline
/// through them stays within [`FIT_TOLERANCE_CODES`] of FX's straight
/// segments, so simple curves stay editable as authored; a curve the spline
/// would draw further off (a sharp corner overshoots) is written through
/// points chosen on 8-bit input codes so that the spline follows the FX
/// polyline. A non-identity Master is folded into the channel curves,
/// because FX applies it per channel before them while Lumetri's Luma curve
/// leaves saturated colours unchanged: each channel `c` is written as
/// `c(master(x))` through points chosen the same way, and Luma stays
/// identity. The diagnostic names the largest deviation of Premiere's spline
/// through the written points from FX's straight segments on 8-bit input
/// codes. It is a retained native approximation, never a reason to move the
/// picture to another route.
pub fn lumetri_curves(curves: &ColorCurves) -> (RgbCurves, String) {
    let points = |curve: &ColorCurve| -> Vec<[f64; 2]> {
        curve
            .points()
            .iter()
            .map(|point| [point.x(), point.y()])
            .collect()
    };
    let channels = [
        ("Red", &curves.red),
        ("Green", &curves.green),
        ("Blue", &curves.blue),
    ];
    let master = points(&curves.master);
    let fold = !curves.master.is_identity();
    let mut deviation = 0.0_f64;
    // Channels whose own FX points Premiere would draw more than
    // FIT_TOLERANCE_CODES off, with that deviation.
    let mut densified: Vec<(&str, f64)> = Vec::new();
    let mut written: [Vec<[f64; 2]>; 3] = Default::default();
    for (written, (name, channel)) in written.iter_mut().zip(channels) {
        let channel = points(channel);
        let (table, corners) = if fold {
            let table: Vec<f64> = (0..SAMPLES)
                .map(|code| linear_value(&channel, linear_value(&master, code_x(code))))
                .collect();
            (table, &master)
        } else {
            let table = linear_table(&channel);
            let own = spline_deviation(&channel, &table);
            if own <= FIT_TOLERANCE_CODES {
                deviation = deviation.max(own);
                *written = channel;
                continue;
            }
            densified.push((name, own));
            (table, &channel)
        };
        let (chosen, error) = spline_points(&table, corners);
        deviation = deviation.max(error);
        *written = chosen;
    }
    let [red, green, blue] = written;
    if fold {
        let written = RgbCurves {
            curves: [vec![[0.0, 0.0], [1.0, 1.0]], red, green, blue],
        };
        let message = format!(
            "exports as native Lumetri Color RGB Curves with the Master curve folded into the Red, Green and Blue curves (each written as channel(Master(x)) through at most {MAX_COLOR_CURVE_POINTS} points on 8-bit input codes) and an identity Luma curve, because Lumetri's Luma curve leaves saturated colours unchanged where FX's per-channel Master does not; Premiere draws a natural cubic spline through the written points, at most {deviation:.2} 8-bit codes from FX's straight segments"
        );
        return (written, message);
    }
    let written = RgbCurves {
        curves: [master, red, green, blue],
    };
    if densified.is_empty() {
        let message = format!(
            "exports as native Lumetri Color RGB Curves (Luma, Red, Green, Blue, the same points); Premiere draws a natural cubic spline through them, at most {deviation:.2} 8-bit codes from FX's straight segments"
        );
        return (written, message);
    }
    let own = densified
        .iter()
        .map(|(name, own)| format!("{name} {own:.2}"))
        .collect::<Vec<_>>()
        .join(", ");
    let message = format!(
        "exports as native Lumetri Color RGB Curves (Luma, Red, Green, Blue); Premiere draws a natural cubic spline through the saved points, and through FX's own points it would deviate from FX's straight segments by up to {own} 8-bit codes, so points were added for rendering fidelity: those curves are written through at most {MAX_COLOR_CURVE_POINTS} points chosen on 8-bit input codes (end codes and FX's corners first, then where the spline deviates most) and are no longer edited as FX's points, the other curves keep the same points; Premiere's spline through the written points is at most {deviation:.2} 8-bit codes from FX's straight segments"
    );
    (written, message)
}

/// The x of 8-bit input `code`.
fn code_x(code: usize) -> f64 {
    code as f64 / (SAMPLES - 1) as f64
}

/// FX's straight segments through `points` sampled at 8-bit input codes.
fn linear_table(points: &[[f64; 2]]) -> Vec<f64> {
    (0..SAMPLES)
        .map(|code| linear_value(points, code_x(code)))
        .collect()
}

/// Largest distance, in 8-bit codes, between Premiere's spline through
/// `points` and `table` at the 8-bit input codes.
fn spline_deviation(points: &[[f64; 2]], table: &[f64]) -> f64 {
    table
        .iter()
        .enumerate()
        .map(|(code, value)| {
            (spline_value(points, code_x(code)) - value).abs() * (SAMPLES - 1) as f64
        })
        .fold(0.0, f64::max)
}

/// Lumetri points on `table` (an FX polyline sampled at 8-bit input codes)
/// whose natural cubic spline stays within [`FIT_TOLERANCE_CODES`] of it
/// where at most [`MAX_COLOR_CURVE_POINTS`] points allow, and the spline's
/// largest deviation in 8-bit codes. Starts from the end codes and the codes
/// of the `corners` (the FX Master's or channel's points), then repeatedly
/// adds the code where the spline deviates most; returns the best set seen.
fn spline_points(table: &[f64], corners: &[[f64; 2]]) -> (Vec<[f64; 2]>, f64) {
    let mut codes: Vec<usize> = corners
        .iter()
        .map(|point| (point[0] * (SAMPLES - 1) as f64).round() as usize)
        .chain([0, SAMPLES - 1])
        .collect();
    codes.sort_unstable();
    codes.dedup();
    if codes.len() > MAX_COLOR_CURVE_POINTS {
        codes = vec![0, SAMPLES - 1];
    }
    let mut best: Option<(Vec<[f64; 2]>, f64)> = None;
    loop {
        let points: Vec<[f64; 2]> = codes
            .iter()
            .map(|&code| [code_x(code), table[code]])
            .collect();
        let (worst, error) = (0..SAMPLES)
            .map(|code| {
                let error = (spline_value(&points, code_x(code)) - table[code]).abs()
                    * (SAMPLES - 1) as f64;
                (code, error)
            })
            .fold(
                (0, 0.0),
                |most, next| if next.1 > most.1 { next } else { most },
            );
        if best.as_ref().is_none_or(|(_, previous)| error < *previous) {
            best = Some((points, error));
        }
        if error <= FIT_TOLERANCE_CODES || codes.len() == MAX_COLOR_CURVE_POINTS {
            break;
        }
        let index = codes
            .binary_search(&worst)
            .expect_err("the spline passes through its own points");
        codes.insert(index, worst);
    }
    best.expect("one candidate is always evaluated")
}

/// FX's straight-segment value of `points` at `x`.
fn linear_value(points: &[[f64; 2]], x: f64) -> f64 {
    let segment = points
        .windows(2)
        .position(|pair| x <= pair[1][0])
        .unwrap_or(points.len() - 2);
    let ([x0, y0], [x1, y1]) = (points[segment], points[segment + 1]);
    y0 + (x - x0) / (x1 - x0) * (y1 - y0)
}

/// The piecewise-linear curve within [`FIT_TOLERANCE_CODES`] of Premiere's
/// spline through `points`, by greedy knot insertion on 8-bit input codes.
fn fitted(points: &[[f64; 2]]) -> Result<ColorCurve, String> {
    let table: Vec<f64> = (0..SAMPLES)
        .map(|code| spline_value(points, code as f64 / (SAMPLES - 1) as f64))
        .collect();
    let mut knots = vec![0, SAMPLES - 1];
    loop {
        let mut worst: Option<(usize, f64)> = None;
        for segment in knots.windows(2) {
            let [start, end] = [segment[0], segment[1]];
            for code in start + 1..end {
                let fraction = (code - start) as f64 / (end - start) as f64;
                let predicted = table[start] + fraction * (table[end] - table[start]);
                let error = (predicted - table[code]).abs() * (SAMPLES - 1) as f64;
                if error > FIT_TOLERANCE_CODES && worst.is_none_or(|(_, previous)| error > previous)
                {
                    worst = Some((code, error));
                }
            }
        }
        let Some((code, _)) = worst else {
            break;
        };
        if knots.len() == MAX_COLOR_CURVE_POINTS {
            return Err(format!(
                "a Lumetri RGB curve needs more than {MAX_COLOR_CURVE_POINTS} linear points within one 8-bit code"
            ));
        }
        let index = knots
            .binary_search(&code)
            .expect_err("a fitted knot lies strictly inside a segment");
        knots.insert(index, code);
    }
    ColorCurve::new(
        knots
            .into_iter()
            .map(|code| ColorCurvePoint::new(code as f64 / (SAMPLES - 1) as f64, table[code]))
            .collect(),
    )
    .map_err(|error| format!("fitted Lumetri RGB curve: {error}"))
}

/// Premiere's curve through `points` at `x`: the natural cubic spline,
/// clamped to 0 to 1. `points` hold at least two points with increasing x
/// from 0 to 1 ([`RgbCurves::decode`]).
pub(super) fn spline_value(points: &[[f64; 2]], x: f64) -> f64 {
    let second = natural_second_derivatives(points);
    let segment = points
        .windows(2)
        .position(|pair| x <= pair[1][0])
        .unwrap_or(points.len() - 2);
    let ([x0, y0], [x1, y1]) = (points[segment], points[segment + 1]);
    // The saved points themselves, exactly.
    if x == x0 || x == x1 {
        return (if x == x0 { y0 } else { y1 }).clamp(0.0, 1.0);
    }
    let (m0, m1) = (second[segment], second[segment + 1]);
    let h = x1 - x0;
    let t = x - x0;
    let slope = (y1 - y0) / h - h * (2.0 * m0 + m1) / 6.0;
    let value = y0 + slope * t + m0 / 2.0 * t * t + (m1 - m0) / (6.0 * h) * t * t * t;
    value.clamp(0.0, 1.0)
}

/// Second derivatives of the natural cubic spline through `points` (zero at
/// both ends), by the tridiagonal (Thomas) solve.
fn natural_second_derivatives(points: &[[f64; 2]]) -> Vec<f64> {
    let count = points.len();
    let mut second = vec![0.0; count];
    if count < 3 {
        return second;
    }
    let h: Vec<f64> = points
        .windows(2)
        .map(|pair| pair[1][0] - pair[0][0])
        .collect();
    let slope: Vec<f64> = points
        .windows(2)
        .zip(&h)
        .map(|(pair, h)| (pair[1][1] - pair[0][1]) / h)
        .collect();
    // Interior rows i = 1..count-1: h[i-1] M[i-1] + 2(h[i-1]+h[i]) M[i] + h[i] M[i+1] = 6 (s[i] - s[i-1]).
    let mut diagonal = vec![0.0; count];
    let mut right = vec![0.0; count];
    for i in 1..count - 1 {
        diagonal[i] = 2.0 * (h[i - 1] + h[i]);
        right[i] = 6.0 * (slope[i] - slope[i - 1]);
        if i > 1 {
            let factor = h[i - 1] / diagonal[i - 1];
            diagonal[i] -= factor * h[i - 1];
            right[i] -= factor * right[i - 1];
        }
    }
    for i in (1..count - 1).rev() {
        second[i] = (right[i] - h[i] * second[i + 1]) / diagonal[i];
    }
    second
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Premiere 26.5.2 renders (8-bit codes) of in-house curves on a grey
    /// ramp, at input codes 16, 48, 64, 96, 128, 176, 208 and 240.
    const MEASURED: [(&[[f64; 2]], [f64; 8]); 3] = [
        (
            &[[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]],
            [28.0, 82.0, 108.0, 154.0, 192.0, 228.0, 242.0, 251.0],
        ),
        (
            &[
                [0.0, 0.0],
                [0.25, 0.15],
                [0.5, 0.6],
                [0.75, 0.7],
                [1.0, 1.0],
            ],
            [3.0, 20.0, 39.0, 99.0, 154.0, 173.0, 192.0, 233.0],
        ),
        (
            &[[0.0, 0.0], [0.3, 0.7], [1.0, 1.0]],
            [42.0, 120.0, 155.0, 209.0, 242.0, 255.0, 255.0, 255.0],
        ),
    ];

    #[test]
    fn spline_and_fit_follow_premiere_renders_within_one_code() {
        for (points, measured) in MEASURED {
            let curve = fitted(points).unwrap();
            for (code, expected) in [16, 48, 64, 96, 128, 176, 208, 240]
                .into_iter()
                .zip(measured)
            {
                let x = f64::from(code) / 255.0;
                let spline = spline_value(points, x) * 255.0;
                assert!(
                    (spline - expected).abs() <= 1.0,
                    "{points:?} {code}: {spline}"
                );
                let linear = fitted_value(&curve, x) * 255.0;
                assert!(
                    (linear - expected).abs() <= 2.0,
                    "{points:?} {code}: {linear}"
                );
            }
        }
        // Two points are exact: the line through them.
        assert_eq!(
            fitted(&[[0.0, 0.2], [1.0, 0.8]]).unwrap().points(),
            [
                ColorCurvePoint::new(0.0, 0.2),
                ColorCurvePoint::new(1.0, 0.8)
            ]
        );
    }

    fn curve(points: &[[f64; 2]]) -> ColorCurve {
        ColorCurve::new(
            points
                .iter()
                .map(|&[x, y]| ColorCurvePoint::new(x, y))
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn master_folds_into_channel_curves_within_one_code_of_the_fx_polyline() {
        let curves = ColorCurves {
            master: curve(&[[0.0, 0.0], [0.3, 0.7], [1.0, 1.0]]),
            red: curve(&[[0.0, 0.1], [1.0, 0.9]]),
            green: ColorCurve::identity(),
            blue: curve(&[[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]]),
        };
        let (written, message) = lumetri_curves(&curves);
        assert_eq!(written.curves[0], [[0.0, 0.0], [1.0, 1.0]]);
        let fx =
            |channel: &ColorCurve, x: f64| fitted_value(channel, fitted_value(&curves.master, x));
        let mut deviation = 0.0_f64;
        for (points, channel) in
            written.curves[1..]
                .iter()
                .zip([&curves.red, &curves.green, &curves.blue])
        {
            assert!(points.len() <= MAX_COLOR_CURVE_POINTS, "{points:?}");
            for code in 0..SAMPLES {
                let x = code_x(code);
                deviation = deviation.max((spline_value(points, x) - fx(channel, x)).abs() * 255.0);
            }
        }
        assert!(deviation <= FIT_TOLERANCE_CODES, "{deviation}");
        assert!(
            message.contains(&format!("at most {deviation:.2} 8-bit codes")),
            "{message}"
        );
        // An identity Master writes simple FX points as they are and reports
        // the spline's deviation from FX's straight segments.
        let unfolded = ColorCurves {
            master: ColorCurve::identity(),
            blue: curve(&[[0.0, 0.2], [1.0, 0.8]]),
            ..curves.clone()
        };
        let (written, message) = lumetri_curves(&unfolded);
        assert_eq!(
            written.curves,
            [
                vec![[0.0, 0.0], [1.0, 1.0]],
                vec![[0.0, 0.1], [1.0, 0.9]],
                vec![[0.0, 0.0], [1.0, 1.0]],
                vec![[0.0, 0.2], [1.0, 0.8]],
            ]
        );
        assert!(message.contains("the same points"), "{message}");
        assert!(message.contains("at most 0.00 8-bit codes"), "{message}");
        assert!(!message.contains("folded"), "{message}");
    }

    #[test]
    fn sharp_channel_curve_is_written_through_points_premiere_draws_within_one_code() {
        // Premiere's spline through these three points overshoots FX's
        // corner by about 46 codes (measured 46 in Premiere 26.5.2).
        let sharp = curve(&[[0.0, 0.0], [0.3, 0.7], [1.0, 1.0]]);
        let gentle = curve(&[[0.0, 0.1], [1.0, 0.9]]);
        let curves = ColorCurves {
            master: ColorCurve::identity(),
            red: sharp.clone(),
            green: ColorCurve::identity(),
            blue: gentle,
        };
        let (written, message) = lumetri_curves(&curves);
        assert_eq!(written.curves[0], [[0.0, 0.0], [1.0, 1.0]]);
        // Curves Premiere already draws within one code keep their points.
        assert_eq!(written.curves[2], [[0.0, 0.0], [1.0, 1.0]]);
        assert_eq!(written.curves[3], [[0.0, 0.1], [1.0, 0.9]]);
        let red = &written.curves[1];
        assert!((4..=MAX_COLOR_CURVE_POINTS).contains(&red.len()), "{red:?}");
        let own = spline_deviation(
            &[[0.0, 0.0], [0.3, 0.7], [1.0, 1.0]],
            &linear_table(&[[0.0, 0.0], [0.3, 0.7], [1.0, 1.0]]),
        );
        assert!(own > 40.0, "{own}");
        let deviation = (0..SAMPLES)
            .map(|code| {
                let x = code_x(code);
                (spline_value(red, x) - fitted_value(&sharp, x)).abs() * 255.0
            })
            .fold(0.0, f64::max);
        assert!(deviation <= FIT_TOLERANCE_CODES, "{deviation} {red:?}");
        for expected in [
            "points were added for rendering fidelity".to_owned(),
            format!("by up to Red {own:.2} 8-bit codes"),
            format!("at most {deviation:.2} 8-bit codes from FX's straight segments"),
            "the other curves keep the same points".to_owned(),
        ] {
            assert!(message.contains(&expected), "{expected}: {message}");
        }
        assert!(!message.contains("folded"), "{message}");
    }

    fn fitted_value(curve: &ColorCurve, x: f64) -> f64 {
        let points = curve.points();
        let index = points.windows(2).position(|pair| x <= pair[1].x()).unwrap();
        let (a, b) = (points[index], points[index + 1]);
        a.y() + (x - a.x()) / (b.x() - a.x()) * (b.y() - a.y())
    }
}
