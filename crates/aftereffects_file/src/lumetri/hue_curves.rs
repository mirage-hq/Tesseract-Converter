//! Lumetri Hue Saturation Curves as an FX `hueCurves`, in both directions.
//!
//! Measured in Premiere 26.5.2 (16 in-house curves rendered on an 8-bit grey
//! ramp with saturated, pastel and saturation-sweep rows, Rec. 709 sequence):
//! each saved curve ([`super::LumetriCurve`]) holds `(x, y)` points with `y`
//! a signed adjustment, 0 neutral, evaluated by a monotone cubic Hermite
//! spline (Fritsch–Carlson slopes; the three hue curves wrap at x 1, the
//! Luma and Saturation curves have flat ends). With display RGB `v`,
//! gamma-2.4 linear light `v^2.4` and Rec. 709 luma weights:
//!
//! - Hue vs Sat, Luma vs Sat and Sat vs Sat scale the linear chroma
//!   `v^2.4 - Y` by `1 + y`, keeping the linear luminance `Y`;
//! - Hue vs Hue turns the display chroma `v - Y'` about the grey axis by
//!   `-180° y`, keeping the display luma `Y'`;
//! - Hue vs Luma adds `Y' y` to each display channel (luma `x (1 + y)`,
//!   display chroma kept); greys are unchanged;
//! - the hue curves are keyed on the HSV hue of `v^2.4` (x 0 red, 1/3
//!   green), Luma vs Sat on `Y'`, Sat vs Sat on the length of `v - Y'`.
//!
//! This model is within 0.03–0.25 mean 8-bit codes of Premiere for each
//! measured saturation and hue curve and within 1.6 for Hue vs Luma. FX
//! `hueCurves` keys on display HSV hue and saturation, scales HSV saturation
//! by `2y` and offsets display luma by `y - 0.5` (y 0.5 neutral), so the
//! conversion matches the two at reference colours: for each FX input, a
//! colour of HSV saturation [`REFERENCE_SATURATION`] and value
//! [`REFERENCE_VALUE`] (the Luma vs Sat and Sat vs Sat rows average twelve
//! hues). Away from those colours the pictures differ, most for fully
//! saturated colours ([`HUE_CURVES_APPROXIMATION`]).

use super::{HUE_CURVE_IDS, LumetriCurve, REC709};
use fx_schema::HueCurves;
use fx_schema::color_tools::HueCurvePoint;

/// Lumetri's linear-light exponent.
const GAMMA: f64 = 2.4;
/// HSV saturation and value of the colours at which the conversion matches
/// FX to Lumetri (fitted to the probe renders over 0.35–0.8 and 0.6–0.9).
const REFERENCE_SATURATION: f64 = 0.65;
const REFERENCE_VALUE: f64 = 0.75;
/// Hues averaged for the Luma vs Sat and Sat vs Sat rows.
const REFERENCE_HUES: usize = 12;
/// Samples of an FX curve (half-degree hue steps) and of a Lumetri curve.
const FX_SAMPLES: usize = 721;
const LUMETRI_SAMPLES: usize = 361;
/// Largest fitted deviation, in curve `y` units.
const FIT_TOLERANCE: f64 = 0.004;
/// Most points of an FX hue curve (`fx_schema::HueCurves` validation).
const MAX_FX_POINTS: usize = 16;
/// Kinds Premiere 26.5.2 saves for the five curves (hue-keyed 3, else 2).
const KINDS: [u32; 5] = [3, 3, 3, 2, 2];

pub const HUE_CURVES_APPROXIMATION: &str = "Lumetri Hue Saturation Curves and FX hueCurves are matched at reference colours (HSV saturation 0.65, value 0.75; the Luma vs Sat and Sat vs Sat curves over twelve hues): Premiere 26.5.2 renders show Lumetri scaling chroma in gamma-2.4 linear light at constant linear luminance, keying its hue curves on linear-light hue and Sat vs Sat on display chroma, and turning hue about the grey axis, while FX keys on display HSV and keeps display luma, so other colours differ (on an in-house probe of 16 single curves, mean 8.9 8-bit codes from Premiere against a mean effect of 17.6, most on fully saturated colours); FX hue curves (piecewise linear) and Lumetri's (monotone cubic) are fitted to each other within 0.004";

/// One of the five Hue Saturation Curves, in [`HUE_CURVE_IDS`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Curve {
    HueVsSat,
    HueVsHue,
    HueVsLuma,
    LumaVsSat,
    SatVsSat,
}

impl Curve {
    const ALL: [Self; 5] = [
        Self::HueVsSat,
        Self::HueVsHue,
        Self::HueVsLuma,
        Self::LumaVsSat,
        Self::SatVsSat,
    ];

    fn periodic(self) -> bool {
        matches!(self, Self::HueVsSat | Self::HueVsHue | Self::HueVsLuma)
    }

    fn name(self) -> &'static str {
        match self {
            Self::HueVsSat => "Hue vs Sat",
            Self::HueVsHue => "Hue vs Hue",
            Self::HueVsLuma => "Hue vs Luma",
            Self::LumaVsSat => "Luma vs Sat",
            Self::SatVsSat => "Sat vs Sat",
        }
    }
}

/// The saved points of the five Hue Saturation Curves (Hue vs Sat, Hue vs
/// Hue, Hue vs Luma, Luma vs Sat, Sat vs Sat). An empty curve changes no
/// picture; a curve has x strictly increasing in 0 to 1 and y in -1 to 1.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HueSatCurves {
    pub curves: [Vec<[f64; 2]>; 5],
}

impl HueSatCurves {
    /// The curves saved in the five values `saved` ([`HUE_CURVE_IDS`] order;
    /// `None` for a curve the host did not save, which has no points), or why
    /// one is not representable. One point at y 0 is the identity; one point
    /// elsewhere, or points under another kind than Premiere 26.5.2's
    /// (Premiere 22/23 saves Hue vs Sat as kind 1), is unmeasured.
    pub fn decode(saved: [Option<&[u8]>; 5]) -> Result<Self, String> {
        let mut curves: [Vec<[f64; 2]>; 5] = Default::default();
        for ((bytes, curve), kind) in saved.into_iter().zip(&mut curves).zip(Curve::ALL) {
            let Some(bytes) = bytes else {
                continue;
            };
            let saved = LumetriCurve::decode(bytes)
                .ok_or_else(|| format!("{} is not a saved curve", kind.name()))?;
            let points = saved.points;
            let expected = KINDS[kind as usize];
            if !points.is_empty() && saved.kind != expected {
                return Err(format!(
                    "{} has points under saved kind {}, not Premiere 26.5.2's measured kind {expected}",
                    kind.name(),
                    saved.kind
                ));
            }
            let valid = points
                .iter()
                .all(|[x, y]| (0.0..=1.0).contains(x) && (-1.0..=1.0).contains(y))
                && points.windows(2).all(|pair| pair[0][0] < pair[1][0]);
            if !valid {
                return Err(format!(
                    "{} has a point outside x 0 to 1 or y -1 to 1, or x not increasing",
                    kind.name()
                ));
            }
            match points.as_slice() {
                [[_, y]] if *y != 0.0 => {
                    return Err(format!(
                        "{} holds a single point off neutral, which was not measured",
                        kind.name()
                    ));
                }
                [_] => {}
                _ => *curve = points,
            }
        }
        Ok(Self { curves })
    }

    /// Whether no curve changes the picture.
    pub fn is_identity(&self) -> bool {
        self.curves
            .iter()
            .all(|curve| curve.iter().all(|[_, y]| *y == 0.0))
    }

    /// The five saved values ([`HUE_CURVE_IDS`] order) with Premiere
    /// 26.5.2's kinds; none when a curve has more than 32 points.
    pub fn encode(&self) -> Option<[(usize, Vec<u8>); 5]> {
        let mut out: [(usize, Vec<u8>); 5] = Default::default();
        for (index, slot) in out.iter_mut().enumerate() {
            let bytes = LumetriCurve {
                kind: KINDS[index],
                points: self.curves[index].clone(),
            }
            .encode()?;
            *slot = (HUE_CURVE_IDS[index], bytes);
        }
        Some(out)
    }
}

/// The FX `hueCurves` matching `curves` at the reference colours, or why it
/// is not representable.
pub fn to_fx(curves: &HueSatCurves) -> Result<HueCurves, String> {
    let mut fx: [Option<Vec<HueCurvePoint>>; 5] = Default::default();
    for ((points, kind), slot) in curves.curves.iter().zip(Curve::ALL).zip(&mut fx) {
        if points.iter().all(|[_, y]| *y == 0.0) {
            continue;
        }
        let xs: Vec<f64> = (0..FX_SAMPLES)
            .map(|index| index as f64 / (FX_SAMPLES - 1) as f64)
            .collect();
        let mut ys: Vec<f64> = xs
            .iter()
            .map(|&x| fx_value(kind, points, x).clamp(0.0, 1.0))
            .collect();
        if kind.periodic() {
            ys[FX_SAMPLES - 1] = ys[0];
        }
        let knots = fit_linear(&xs, &ys);
        *slot = Some(
            knots
                .into_iter()
                .map(|index| HueCurvePoint {
                    x: xs[index],
                    y: ys[index],
                })
                .collect(),
        );
    }
    let [
        hue_vs_saturation,
        hue_vs_hue,
        hue_vs_luminance,
        luminance_vs_saturation,
        saturation_vs_saturation,
    ] = fx;
    validated(HueCurves {
        semantic_version: 1,
        hue_vs_hue,
        hue_vs_saturation,
        hue_vs_luminance,
        luminance_vs_saturation,
        saturation_vs_saturation,
    })
}

/// The Lumetri curves matching FX `curves` at the reference colours, each
/// within [`FIT_TOLERANCE`] of the matched values where 32 points suffice.
pub fn from_fx(curves: &HueCurves) -> HueSatCurves {
    let fx = [
        &curves.hue_vs_saturation,
        &curves.hue_vs_hue,
        &curves.hue_vs_luminance,
        &curves.luminance_vs_saturation,
        &curves.saturation_vs_saturation,
    ];
    let mut out = HueSatCurves::default();
    for ((points, kind), slot) in fx.into_iter().zip(Curve::ALL).zip(&mut out.curves) {
        let Some(points) = points else {
            continue;
        };
        if points.iter().all(|point| point.y == 0.5) {
            continue;
        }
        let (xs, ys) = lumetri_targets(kind, points);
        *slot = fit_lumetri(&xs, &ys, kind.periodic())
            .into_iter()
            .map(|index| [xs[index], ys[index]])
            .collect();
    }
    out
}

/// The FX `y` at FX input `x` that matches the Lumetri curve `points` of
/// `kind` at the reference colour(s) for `x`.
fn fx_value(kind: Curve, points: &[[f64; 2]], x: f64) -> f64 {
    match kind {
        Curve::HueVsSat | Curve::HueVsHue | Curve::HueVsLuma => {
            let colour = hsv_rgb(x, REFERENCE_SATURATION, REFERENCE_VALUE);
            let y = hermite(points, linear_hue(colour), true);
            match kind {
                Curve::HueVsSat => {
                    hsv_saturation(chroma_gain(colour, 1.0 + y)) / (2.0 * REFERENCE_SATURATION)
                }
                Curve::HueVsHue => {
                    let turned = rotate(colour, -std::f64::consts::PI * y);
                    0.5 + wrap(hsv_hue(turned) - x)
                }
                _ => 0.5 + luma(luma_gain(colour, y)) - luma(colour),
            }
        }
        // The reference colours share one key: the FX input's luma, or its
        // saturation's mean display chroma length (as export inverts it).
        Curve::LumaVsSat => {
            let gain = 1.0 + hermite(points, x, false);
            mean_saturation_ratio(&luma_colours(x.max(1e-3)), gain) / 2.0
        }
        Curve::SatVsSat => {
            let gain = 1.0 + hermite(points, x * saturation_axis_scale(), false);
            mean_saturation_ratio(&saturation_colours(x.max(1e-3)), gain) / 2.0
        }
    }
}

/// The Lumetri `(x, y)` samples that match the FX curve `points` of `kind`.
fn lumetri_targets(kind: Curve, points: &[HueCurvePoint]) -> (Vec<f64>, Vec<f64>) {
    let fx = |x: f64| fx_curve(points, x, kind.periodic());
    // Periodic curves wrap, so x 1 repeats x 0 and is left out.
    let count = if kind.periodic() {
        LUMETRI_SAMPLES - 1
    } else {
        LUMETRI_SAMPLES
    };
    let inputs = (0..count).map(|index| index as f64 / (LUMETRI_SAMPLES - 1) as f64);
    let pairs: Vec<[f64; 2]> = match kind {
        Curve::HueVsSat | Curve::HueVsHue | Curve::HueVsLuma => inputs
            .map(|x| {
                let colour = hsv_rgb(x, REFERENCE_SATURATION, REFERENCE_VALUE);
                let target = fx(x);
                let y = match kind {
                    Curve::HueVsSat => {
                        let wanted = (2.0 * target * REFERENCE_SATURATION).clamp(0.0, 1.0);
                        bisect(|y| hsv_saturation(chroma_gain(colour, 1.0 + y)) - wanted)
                    }
                    Curve::HueVsHue => {
                        let wanted = x + target - 0.5;
                        -turn_to(colour, wanted) / std::f64::consts::PI
                    }
                    _ => (target - 0.5) / luma(colour).max(1e-3),
                };
                [linear_hue(colour), y.clamp(-1.0, 1.0)]
            })
            .collect(),
        Curve::LumaVsSat => inputs
            .map(|x| {
                let colours = luma_colours(x.max(1e-3));
                let wanted = 2.0 * fx(x);
                let y = bisect(|y| mean_saturation_ratio(&colours, 1.0 + y) - wanted);
                [x, y]
            })
            .collect(),
        Curve::SatVsSat => {
            let scale = saturation_axis_scale();
            inputs
                .map(|x| {
                    let colours = saturation_colours(x.max(1e-3));
                    let wanted = 2.0 * fx(x);
                    let y = bisect(|y| mean_saturation_ratio(&colours, 1.0 + y) - wanted);
                    [x * scale, y]
                })
                .collect()
        }
    };
    let mut pairs = pairs;
    // Linear-light hue is monotone in display hue; sorting makes the seam
    // explicit.
    pairs.sort_by(|a, b| a[0].total_cmp(&b[0]));
    pairs.dedup_by(|a, b| a[0] <= b[0]);
    pairs.into_iter().map(|[x, y]| (x, y)).unzip()
}

/// Greedy knot insertion at the worst sample until the piecewise-linear
/// curve through the knots is within [`FIT_TOLERANCE`] of every sample or
/// holds [`MAX_FX_POINTS`] knots. Returns the knot indices.
fn fit_linear(xs: &[f64], ys: &[f64]) -> Vec<usize> {
    let mut knots = vec![0, xs.len() - 1];
    while knots.len() < MAX_FX_POINTS {
        let mut worst = (0, 0.0);
        for segment in knots.windows(2) {
            let [start, end] = [segment[0], segment[1]];
            for index in start + 1..end {
                let fraction = (xs[index] - xs[start]) / (xs[end] - xs[start]);
                let error = (ys[start] + fraction * (ys[end] - ys[start]) - ys[index]).abs();
                if error > worst.1 {
                    worst = (index, error);
                }
            }
        }
        if worst.1 <= FIT_TOLERANCE {
            break;
        }
        let at = knots.partition_point(|&knot| knot < worst.0);
        knots.insert(at, worst.0);
    }
    knots
}

/// Greedy knot insertion for Lumetri's spline: knots at the samples nearest
/// x 0 and 0.5 (periodic) or at both ends, then the worst sample until the
/// spline is within [`FIT_TOLERANCE`] or holds 32 points.
fn fit_lumetri(xs: &[f64], ys: &[f64], periodic: bool) -> Vec<usize> {
    let nearest = |target: f64| {
        (0..xs.len())
            .min_by(|&a, &b| (xs[a] - target).abs().total_cmp(&(xs[b] - target).abs()))
            .unwrap_or(0)
    };
    let mut knots = if periodic {
        vec![nearest(0.0), nearest(0.5)]
    } else {
        vec![0, xs.len() - 1]
    };
    knots.sort_unstable();
    knots.dedup();
    while knots.len() < super::CURVE_POINTS {
        let points: Vec<[f64; 2]> = knots.iter().map(|&index| [xs[index], ys[index]]).collect();
        let (worst, error) = xs
            .iter()
            .zip(ys)
            .enumerate()
            .map(|(index, (&x, &y))| (index, (hermite(&points, x, periodic) - y).abs()))
            .fold(
                (0, 0.0),
                |best, item| if item.1 > best.1 { item } else { best },
            );
        if error <= FIT_TOLERANCE || knots.contains(&worst) {
            break;
        }
        let at = knots.partition_point(|&knot| knot < worst);
        knots.insert(at, worst);
    }
    knots
}

/// Lumetri's monotone cubic Hermite spline through `points` at `x`:
/// Fritsch–Carlson interior slopes, flat ends, or (periodic) the points
/// repeated one unit either side, a last point at x 1 standing for x 0.
fn hermite(points: &[[f64; 2]], x: f64, periodic: bool) -> f64 {
    let mut knots: Vec<[f64; 2]> = points.to_vec();
    if periodic {
        if knots.len() > 1 && knots[0][0] == 0.0 && knots[knots.len() - 1][0] == 1.0 {
            knots.pop();
        }
        if knots.len() == 1 {
            return knots[0][1];
        }
        let before: Vec<[f64; 2]> = knots[knots.len() - 2..]
            .iter()
            .map(|[x, y]| [x - 1.0, *y])
            .collect();
        let after: Vec<[f64; 2]> = knots[..2].iter().map(|[x, y]| [x + 1.0, *y]).collect();
        knots = before.into_iter().chain(knots).chain(after).collect();
    }
    match knots.as_slice() {
        [] => return 0.0,
        [[_, y]] => return *y,
        _ => {}
    }
    let count = knots.len();
    let widths: Vec<f64> = knots
        .windows(2)
        .map(|pair| pair[1][0] - pair[0][0])
        .collect();
    let slopes: Vec<f64> = knots
        .windows(2)
        .zip(&widths)
        .map(|(pair, width)| (pair[1][1] - pair[0][1]) / width)
        .collect();
    let mut tangents = vec![0.0; count];
    for index in 1..count - 1 {
        let [left, right] = [slopes[index - 1], slopes[index]];
        if left * right > 0.0 {
            let [h0, h1] = [widths[index - 1], widths[index]];
            let [w1, w2] = [2.0 * h1 + h0, h1 + 2.0 * h0];
            tangents[index] = (w1 + w2) / (w1 / left + w2 / right);
        }
    }
    let x = if periodic {
        x.rem_euclid(1.0)
    } else {
        x.clamp(knots[0][0], knots[count - 1][0])
    };
    let segment = knots
        .partition_point(|knot| knot[0] <= x)
        .clamp(1, count - 1)
        - 1;
    let [x0, y0] = knots[segment];
    let y1 = knots[segment + 1][1];
    let width = widths[segment];
    let t = (x - x0) / width;
    let [t2, t3] = [t * t, t * t * t];
    (2.0 * t3 - 3.0 * t2 + 1.0) * y0
        + (t3 - 2.0 * t2 + t) * width * tangents[segment]
        + (-2.0 * t3 + 3.0 * t2) * y1
        + (t3 - t2) * width * tangents[segment + 1]
}

/// The FX piecewise-linear curve at `x` (`fx_composition`'s evaluation).
fn fx_curve(points: &[HueCurvePoint], x: f64, periodic: bool) -> f64 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 0.5;
    };
    let mut x = x.clamp(0.0, 1.0);
    if periodic {
        x = x.rem_euclid(1.0);
        if (first.x != 0.0 || last.x != 1.0) && x < first.x {
            x += 1.0;
        }
    } else if x <= first.x {
        return first.y;
    }
    for pair in points.windows(2) {
        if x <= pair[1].x {
            return pair[0].y + (x - pair[0].x) / (pair[1].x - pair[0].x) * (pair[1].y - pair[0].y);
        }
    }
    if periodic && (first.x != 0.0 || last.x != 1.0) {
        let width = first.x + 1.0 - last.x;
        return last.y + (x - last.x) / width * (first.y - last.y);
    }
    last.y
}

/// The root of increasing `f` over -1 to 1 (an end when it has none).
fn bisect(f: impl Fn(f64) -> f64) -> f64 {
    let [mut low, mut high] = [-1.0, 1.0];
    if f(high) <= 0.0 {
        return high;
    }
    if f(low) >= 0.0 {
        return low;
    }
    for _ in 0..48 {
        let mid = 0.5 * (low + high);
        if f(mid) > 0.0 {
            high = mid;
        } else {
            low = mid;
        }
    }
    0.5 * (low + high)
}

/// The turn (radians, -π to π) about the grey axis that brings `colour`'s
/// display HSV hue closest to `hue` (turns).
fn turn_to(colour: [f64; 3], hue: f64) -> f64 {
    let error = |angle: f64| wrap(hsv_hue(rotate(colour, angle)) - hue).abs();
    let steps = 720;
    let step = 2.0 * std::f64::consts::PI / f64::from(steps);
    let mut best = (0.0, f64::INFINITY);
    for index in 0..=steps {
        let angle = -std::f64::consts::PI + f64::from(index) * step;
        let candidate = error(angle);
        if candidate < best.1 {
            best = (angle, candidate);
        }
    }
    let [mut low, mut high] = [best.0 - step, best.0 + step];
    for _ in 0..40 {
        let [a, b] = [low + (high - low) / 3.0, high - (high - low) / 3.0];
        if error(a) < error(b) {
            high = b;
        } else {
            low = a;
        }
    }
    (0.5 * (low + high)).clamp(-std::f64::consts::PI, std::f64::consts::PI)
}

fn wrap(turns: f64) -> f64 {
    (turns + 0.5).rem_euclid(1.0) - 0.5
}

fn luma(rgb: [f64; 3]) -> f64 {
    rgb.iter().zip(REC709).map(|(c, w)| c * w).sum()
}

fn chroma_length(rgb: [f64; 3]) -> f64 {
    let y = luma(rgb);
    rgb.iter().map(|c| (c - y) * (c - y)).sum::<f64>().sqrt()
}

/// HSV hue in turns (0 for a grey).
fn hsv_hue([r, g, b]: [f64; 3]) -> f64 {
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    if delta <= 0.0 {
        return 0.0;
    }
    let sector = if max == r {
        (g - b) / delta
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (sector / 6.0).rem_euclid(1.0)
}

fn hsv_saturation(rgb: [f64; 3]) -> f64 {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    if max <= 0.0 {
        return 0.0;
    }
    (max - rgb[0].min(rgb[1]).min(rgb[2])) / max
}

fn hsv_rgb(hue: f64, saturation: f64, value: f64) -> [f64; 3] {
    [0.0, 2.0 / 3.0, 1.0 / 3.0].map(|offset| {
        let channel = (((hue + offset).rem_euclid(1.0)) * 6.0 - 3.0).abs() - 1.0;
        value * (1.0 + (channel.clamp(0.0, 1.0) - 1.0) * saturation)
    })
}

/// The HSV hue of the linear-light colour, Lumetri's hue key.
fn linear_hue(rgb: [f64; 3]) -> f64 {
    hsv_hue(rgb.map(|c| c.clamp(0.0, 1.0).powf(GAMMA)))
}

/// Lumetri's saturation curves: linear chroma scaled by `gain` at constant
/// linear luminance.
fn chroma_gain(rgb: [f64; 3], gain: f64) -> [f64; 3] {
    let linear = rgb.map(|c| c.clamp(0.0, 1.0).powf(GAMMA));
    let y = luma(linear);
    linear.map(|c| {
        (y + (c - y) * gain.max(0.0))
            .clamp(0.0, 1.0)
            .powf(1.0 / GAMMA)
    })
}

/// Lumetri's Hue vs Hue: display chroma turned by `angle` about the grey
/// axis at constant display luma.
fn rotate(rgb: [f64; 3], angle: f64) -> [f64; 3] {
    let y = luma(rgb);
    let c = rgb.map(|value| value - y);
    let k = 1.0 / 3.0_f64.sqrt();
    let cross = [k * (c[2] - c[1]), k * (c[0] - c[2]), k * (c[1] - c[0])];
    let along = k * (c[0] + c[1] + c[2]);
    let (sin, cos) = angle.sin_cos();
    let turned: [f64; 3] =
        std::array::from_fn(|index| c[index] * cos + cross[index] * sin + k * along * (1.0 - cos));
    let shift = luma(turned);
    turned.map(|value| (y + value - shift).clamp(0.0, 1.0))
}

/// Lumetri's Hue vs Luma: `y` times the display luma added to each channel.
fn luma_gain(rgb: [f64; 3], y: f64) -> [f64; 3] {
    let lift = luma(rgb) * y;
    rgb.map(|c| (c + lift).clamp(0.0, 1.0))
}

/// Colours of display luma `l` and HSV saturation
/// [`REFERENCE_SATURATION`] at [`REFERENCE_HUES`] hues.
fn luma_colours(l: f64) -> Vec<[f64; 3]> {
    (0..REFERENCE_HUES)
        .map(|index| {
            let unit = hsv_rgb(
                index as f64 / REFERENCE_HUES as f64,
                REFERENCE_SATURATION,
                1.0,
            );
            let scale = l / luma(unit);
            unit.map(|c| (c * scale).clamp(0.0, 1.0))
        })
        .collect()
}

/// Colours of HSV saturation `s` and value [`REFERENCE_VALUE`] at
/// [`REFERENCE_HUES`] hues.
fn saturation_colours(s: f64) -> Vec<[f64; 3]> {
    (0..REFERENCE_HUES)
        .map(|index| hsv_rgb(index as f64 / REFERENCE_HUES as f64, s, REFERENCE_VALUE))
        .collect()
}

/// Mean display HSV saturation ratio of `colours` under Lumetri's chroma
/// gain `gain`.
fn mean_saturation_ratio(colours: &[[f64; 3]], gain: f64) -> f64 {
    colours
        .iter()
        .map(|&colour| hsv_saturation(chroma_gain(colour, gain)) / hsv_saturation(colour).max(1e-9))
        .sum::<f64>()
        / colours.len() as f64
}

/// Display chroma length per unit of HSV saturation at
/// [`REFERENCE_VALUE`], averaged over the reference hues: the Sat vs Sat
/// key of an FX saturation.
fn saturation_axis_scale() -> f64 {
    saturation_colours(1.0)
        .into_iter()
        .map(chroma_length)
        .sum::<f64>()
        / REFERENCE_HUES as f64
}

/// `curves` after the FX schema's own validation (point count, x order at
/// GPU precision, periodic seam).
fn validated(curves: HueCurves) -> Result<HueCurves, String> {
    serde_json::to_value(&curves)
        .and_then(serde_json::from_value)
        .map_err(|error| format!("fitted hueCurves are invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curves(kind: Curve, points: &[[f64; 2]]) -> HueSatCurves {
        let mut out = HueSatCurves::default();
        out.curves[kind as usize] = points.to_vec();
        out
    }

    /// Premiere 26.5.2 render (probe hc1, clip 1): a flat Hue vs Sat of -0.5
    /// halves the linear chroma, so pure red's display luma rises by 68.5
    /// codes; the model reproduces it.
    #[test]
    fn hue_vs_sat_scales_linear_chroma_at_constant_luminance() {
        let red = chroma_gain([1.0, 0.0, 0.0], 0.5);
        let rise = (luma(red) - luma([1.0, 0.0, 0.0])) * 255.0;
        assert!((rise - 68.5).abs() < 0.5, "{rise}");
    }

    /// Premiere 26.5.2 renders: a bump of y -0.6 at x 0.25 between zeros at
    /// 0.15 and 0.35 keys on linear-light hue through a monotone spline
    /// (measured effective chroma gains at the probe's hue columns).
    #[test]
    fn hue_curves_key_on_linear_light_hue_through_a_monotone_spline() {
        let bump = [
            [0.0, 0.0],
            [0.15, 0.0],
            [0.25, -0.6],
            [0.35, 0.0],
            [1.0, 0.0],
        ];
        for (x, measured) in [(0.25, -0.6), (0.2, -0.3), (0.5, 0.0), (0.1, 0.0)] {
            assert!((hermite(&bump, x, true) - measured).abs() < 1e-9);
        }
        // A display yellow-green (HSV 81°) lies at linear-light hue 0.2742.
        let colour = hsv_rgb(81.0 / 360.0, 1.0, 1.0);
        assert!(
            (linear_hue(colour) - 0.2742).abs() < 1e-3,
            "{}",
            linear_hue(colour)
        );
        // Flat ends: a two-point Luma vs Sat ramp is a smoothstep.
        let ramp = [[0.0, -1.0], [1.0, 0.0]];
        assert!((hermite(&ramp, 0.25, false) - (-1.0 + 0.15625)).abs() < 1e-9);
    }

    #[test]
    fn a_wrapping_hue_curve_continues_across_x_1() {
        let wrap = [[0.05, -0.6], [0.2, 0.0], [0.9, 0.0]];
        let near = hermite(&wrap, 0.999, true);
        let past = hermite(&wrap, 0.001, true);
        assert!((near - past).abs() < 0.02 && near < -0.1, "{near} {past}");
    }

    #[test]
    fn identity_and_saved_form_round_trip() {
        let mut curves = curves(Curve::HueVsHue, &[[0.0, 0.1], [0.5, -0.2], [1.0, 0.1]]);
        curves.curves[3] = vec![[0.0, -0.5], [1.0, 0.0]];
        let saved = curves.encode().expect("at most 32 points");
        assert_eq!(saved.each_ref().map(|(id, _)| *id), HUE_CURVE_IDS);
        let bytes: [Option<&[u8]>; 5] =
            std::array::from_fn(|index| Some(saved[index].1.as_slice()));
        assert_eq!(HueSatCurves::decode(bytes), Ok(curves));
        assert_eq!(HueSatCurves::decode([None; 5]), Ok(HueSatCurves::default()));
        assert!(HueSatCurves::default().is_identity());
    }

    #[test]
    fn decode_rejects_unmeasured_values() {
        let one = LumetriCurve {
            kind: 3,
            points: vec![[0.3, 0.2]],
        }
        .encode()
        .expect("one point");
        let empty = LumetriCurve {
            kind: 3,
            points: Vec::new(),
        }
        .encode()
        .expect("no point");
        let bytes = [Some(one.as_slice()), Some(&empty[..]), None, None, None];
        assert!(HueSatCurves::decode(bytes).is_err());
        let beyond = LumetriCurve {
            kind: 2,
            points: vec![[0.0, 0.0], [1.0, 1.5]],
        }
        .encode()
        .expect("two points");
        let bytes = [None, None, None, Some(beyond.as_slice()), None];
        assert!(HueSatCurves::decode(bytes).is_err());
    }

    /// Each curve imports to a valid FX curve of at most 16 points whose
    /// sense follows Lumetri's, and exports back to Lumetri points that
    /// reimport within 0.02.
    #[test]
    fn every_curve_converts_both_ways_and_round_trips() {
        let cases: [(Curve, &[[f64; 2]]); 6] = [
            (
                Curve::HueVsSat,
                &[
                    [0.0, 0.0],
                    [0.15, 0.0],
                    [0.25, -0.6],
                    [0.35, 0.0],
                    [1.0, 0.0],
                ],
            ),
            (Curve::HueVsHue, &[[0.0, 0.1], [1.0, 0.1]]),
            (
                Curve::HueVsLuma,
                &[[0.0, 0.0], [0.65, 0.0], [0.75, -0.3], [0.85, 0.0]],
            ),
            (Curve::LumaVsSat, &[[0.0, 0.0], [0.5, -0.8], [1.0, 0.0]]),
            (Curve::SatVsSat, &[[0.0, 0.0], [0.5, 0.5], [1.0, 0.0]]),
            (
                Curve::HueVsSat,
                &[
                    [0.0, 0.3],
                    [0.1, -0.4],
                    [0.3, 0.2],
                    [0.55, -0.7],
                    [0.8, 0.1],
                    [1.0, 0.3],
                ],
            ),
        ];
        for (kind, points) in cases {
            let lumetri = curves(kind, points);
            let fx = to_fx(&lumetri).expect("valid FX curves");
            let fx_points = [
                &fx.hue_vs_saturation,
                &fx.hue_vs_hue,
                &fx.hue_vs_luminance,
                &fx.luminance_vs_saturation,
                &fx.saturation_vs_saturation,
            ][kind as usize]
                .as_ref()
                .expect("the converted curve");
            assert!(fx_points.len() <= MAX_FX_POINTS);
            let back = from_fx(&fx);
            assert!(back.curves[kind as usize].len() <= super::super::CURVE_POINTS);
            let again = to_fx(&back).expect("valid FX curves");
            let again_points = [
                &again.hue_vs_saturation,
                &again.hue_vs_hue,
                &again.hue_vs_luminance,
                &again.luminance_vs_saturation,
                &again.saturation_vs_saturation,
            ][kind as usize]
                .as_ref()
                .expect("the reconverted curve");
            for index in 0..=100 {
                let x = f64::from(index) / 100.0;
                let [a, b] = [fx_points, again_points].map(|p| fx_curve(p, x, kind.periodic()));
                assert!((a - b).abs() < 0.02, "{kind:?} x {x}: {a} vs {b}");
            }
        }
        // Senses: desaturating, a negative hue turn and darkening.
        let sat = to_fx(&curves(Curve::HueVsSat, &[[0.0, -0.5], [1.0, -0.5]])).expect("fx");
        assert!(
            sat.hue_vs_saturation
                .expect("curve")
                .iter()
                .all(|p| p.y < 0.4)
        );
        let hue = to_fx(&curves(Curve::HueVsHue, &[[0.0, 0.1], [1.0, 0.1]])).expect("fx");
        assert!(hue.hue_vs_hue.expect("curve").iter().all(|p| p.y < 0.5));
        let dark = to_fx(&curves(Curve::HueVsLuma, &[[0.0, -0.2], [1.0, -0.2]])).expect("fx");
        assert!(
            dark.hue_vs_luminance
                .expect("curve")
                .iter()
                .all(|p| p.y < 0.5)
        );
    }
}
