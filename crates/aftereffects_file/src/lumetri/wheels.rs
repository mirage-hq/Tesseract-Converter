//! Lumetri Color Wheels as an FX `colorCurves`.
//!
//! Measured in Premiere 26.5.2 (renders of in-house wheel values on an 8-bit
//! grey ramp with saturated, pastel and warm colour rows, Rec. 709 sequence,
//! 63 wheel settings): the wheels act on each channel alone (every coloured
//! pixel equals the grey ramp's output at its channel code), so the section
//! is three per-channel transfer curves. With the chroma direction
//! `c(hue) = hsv(hue, 1, 1) - Rec.709 luma` of a wheel `(hue, luma, amount)`:
//!
//! - Shadows lift `L = amount/2 * c + (luma - 0.5)/2`: `y = x + L (1 - x)`,
//!   where a negative `L` ramps in from black over `W = 0.5942 |L|^0.6199`
//!   (`1 - (1 - x/W)^1.1979`), so black stays black;
//! - Highlights gain `G = 1 + amount/2 * c + (luma - 0.5)/2`: `y * G`, where
//!   a gain above 1 ramps out towards white over `W = 0.635 (G - 1)^0.6483`
//!   (`1 - (1 - (1 - y)/W)^1.1987`);
//! - Midtones power `E = 1 - amount * c - (luma - 0.5)`: `clamp(y, 0, 1)^E`.
//!
//! The ramp widths and exponents are fitted; the model is at most 3.2 8-bit
//! codes from Premiere (mean 0.29) over the measured settings, worst near
//! black under a strong midtones power. The fourth saved wheel is inert in
//! a three-wheel Lumetri. Export writes FX `colorCurves` as Lumetri RGB
//! Curves, never wheels: FX has no wheels operation.
use super::{REC709, Wheels};
use fx_schema::{
    ColorCurve, ColorCurvePoint, ColorCurves, ColorCurvesSemanticVersion, LayerEffect,
    MAX_COLOR_CURVE_POINTS,
};

/// Samples of a curve at 8-bit input codes.
const SAMPLES: usize = 256;

/// Largest fitted deviation from the wheel model, in 8-bit codes.
const FIT_TOLERANCE_CODES: f64 = 1.0;

/// Fitted shadows ramp: width scale, width exponent, ramp exponent.
const SHADOWS_RAMP: [f64; 3] = [0.5942, 0.6199, 1.1979];
/// Fitted highlights ramp: width scale, width exponent, ramp exponent.
const HIGHLIGHTS_RAMP: [f64; 3] = [0.635, 0.6483, 1.1987];

pub const LUMETRI_WHEELS_APPROXIMATION: &str = "Lumetri Color Wheels map to colorCurves with an identity Master: Premiere 26.5.2 renders show the Shadows, Midtones and Highlights wheels acting per channel as a lift, a power and a gain, and a fitted model of that transfer (at most 3.2 8-bit codes from Premiere, mean 0.29, over 63 measured settings) is written as Red, Green and Blue curves of at most 32 linear points within one code, through which Premiere's RGB Curves spline also stays within one code; the wheels are no longer editable as wheels and export back as Lumetri RGB Curves";

/// The FX `colorCurves` following the wheels' per-channel transfer, or
/// why it is not representable: a midtones power outside the measured
/// positive range.
pub fn color_curves(wheels: &Wheels) -> Result<LayerEffect, String> {
    let channels = channel_transfers(wheels)?;
    let [red, green, blue] = channels.map(|table| fitted(&table));
    Ok(LayerEffect::ColorCurves {
        semantic_version: ColorCurvesSemanticVersion::V1,
        curves: ColorCurves {
            master: ColorCurve::identity(),
            red: red?,
            green: green?,
            blue: blue?,
        },
    })
}

/// The modelled wheel output of each channel at the 8-bit input codes.
fn channel_transfers(wheels: &Wheels) -> Result<[Vec<f64>; 3], String> {
    let [shadows, midtones, highlights, _] = wheels.wheels;
    let lift = offsets(shadows, 0.5);
    let gain = offsets(highlights, 0.5).map(|offset| 1.0 + offset);
    let power = offsets(midtones, 1.0).map(|offset| 1.0 - offset);
    if let Some(exponent) = power.iter().find(|exponent| **exponent < 0.2) {
        return Err(format!(
            "the Color Wheels midtones power {exponent:.3} is below the measured range (0.2 or more)"
        ));
    }
    Ok(std::array::from_fn(|channel| {
        (0..SAMPLES)
            .map(|code| {
                let x = code as f64 / (SAMPLES - 1) as f64;
                transfer(x, lift[channel], gain[channel], power[channel])
            })
            .collect()
    }))
}

/// A wheel's per-channel offset `scale * (amount * c(hue) + luma - 0.5)`:
/// 0.5 for the Shadows lift and Highlights gain, 1 for the Midtones power.
fn offsets([hue, luma, amount]: [f64; 3], scale: f64) -> [f64; 3] {
    let chroma = hue_chroma(hue);
    chroma.map(|component| scale * (amount * component + (luma - 0.5)))
}

/// `hsv(hue, 1, 1)` minus its Rec. 709 luma.
fn hue_chroma(hue: f64) -> [f64; 3] {
    let sector = hue.rem_euclid(360.0) / 60.0;
    let fraction = sector.fract();
    let rgb = match sector as u32 {
        0 => [1.0, fraction, 0.0],
        1 => [1.0 - fraction, 1.0, 0.0],
        2 => [0.0, 1.0, fraction],
        3 => [0.0, 1.0 - fraction, 1.0],
        4 => [fraction, 0.0, 1.0],
        _ => [1.0, 0.0, 1.0 - fraction],
    };
    let luma: f64 = rgb.iter().zip(REC709).map(|(c, w)| c * w).sum();
    rgb.map(|c| c - luma)
}

/// The fitted ramp `1 - (1 - min(u, 1))^exponent` for `u >= 0`.
fn ramp(u: f64, exponent: f64) -> f64 {
    1.0 - (1.0 - u.clamp(0.0, 1.0)).powf(exponent)
}

/// One channel of the wheel transfer at input `x`.
fn transfer(x: f64, lift: f64, gain: f64, power: f64) -> f64 {
    let [scale, width_exponent, ramp_exponent] = SHADOWS_RAMP;
    let lifted = if lift >= 0.0 {
        x + lift * (1.0 - x)
    } else {
        let width = scale * lift.abs().powf(width_exponent);
        x + lift * (1.0 - x) * ramp(x / width, ramp_exponent)
    };
    let [scale, width_exponent, ramp_exponent] = HIGHLIGHTS_RAMP;
    let gained = if gain <= 1.0 {
        lifted * gain
    } else {
        let width = scale * (gain - 1.0).powf(width_exponent);
        lifted + (gain - 1.0) * lifted * ramp((1.0 - lifted) / width, ramp_exponent)
    };
    gained.clamp(0.0, 1.0).powf(power)
}

pub const TONAL_COLOR_WHEELS_APPROXIMATION: &str = "tonalColor exports as Lumetri Color Wheels: each channel's lift, power and gain (the measured wheel model, at most 3.2 codes from Premiere 26.5.2) is least-squares fitted to tonalColor's output on a grey ramp and 720 HSV colours and the three per-channel vectors become the Shadows, Midtones and Highlights wheels (hue, luma, amount); tonalColor offsets chroma in fixed luma windows with a gamut fit that vanishes at black and white, where the wheels' lift and gain still act, and the wheels act per channel, so blacks, whites and coloured pixels differ most";

/// The Shadows, Midtones and Highlights wheels whose per-channel transfer
/// best matches `tonal` (least squares over [`tonal_samples`]: an 8-bit
/// grey ramp and an HSV grid of 720 colours), and the mean and largest
/// remaining per-channel deviation over those samples in 8-bit codes; or
/// why a fitted wheel lies outside the saved ranges (an amount or luma
/// outside 0 to 1).
pub fn from_tonal_color(tonal: &fx_schema::TonalColor) -> Result<(Wheels, [f64; 2]), String> {
    let samples = tonal_samples();
    let targets: Vec<[f64; 3]> = samples.iter().map(|&rgb| tonal_color(tonal, rgb)).collect();
    let mut lift = [0.0; 3];
    let mut gain = [1.0; 3];
    let mut power = [1.0; 3];
    let [mut total, mut largest]: [f64; 2] = [0.0, 0.0];
    for channel in 0..3 {
        let error = |[l, g, p]: [f64; 3]| {
            if p < 0.2 || g <= 0.0 {
                return f64::INFINITY;
            }
            samples
                .iter()
                .zip(&targets)
                .map(|(input, target)| {
                    let difference = transfer(input[channel], l, g, p) - target[channel];
                    difference * difference
                })
                .sum::<f64>()
        };
        let [l, g, p] = nelder_mead(error, [0.0, 1.0, 1.0]);
        [lift[channel], gain[channel], power[channel]] = [l, g, p];
        for (input, target) in samples.iter().zip(&targets) {
            let codes = (transfer(input[channel], l, g, p) - target[channel]).abs() * 255.0;
            total += codes;
            largest = largest.max(codes);
        }
    }
    let shadows = wheel(lift.map(|value| value / 0.5), "Shadows")?;
    let highlights = wheel(gain.map(|value| (value - 1.0) / 0.5), "Highlights")?;
    let midtones = wheel(power.map(|value| 1.0 - value), "Midtones")?;
    Ok((
        Wheels {
            wheels: [shadows, midtones, highlights, [0.0, 0.5, 0.0]],
        },
        [total / (3 * samples.len()) as f64, largest],
    ))
}

/// The fit's inputs: 52 greys and the HSV colours of 24 hues, saturations
/// 0.15 to 0.9 and values 0.2 to 0.95 (fitting only greys left the chart
/// colours 10% further from tonalColor in an offline comparison).
fn tonal_samples() -> Vec<[f64; 3]> {
    let greys = (0..52).map(|step| [f64::from(step) / 51.0; 3]);
    let colours = (0..24).flat_map(|hue| {
        [0.15, 0.3, 0.5, 0.7, 0.9]
            .into_iter()
            .flat_map(move |saturation| {
                [0.2, 0.35, 0.5, 0.65, 0.8, 0.95]
                    .into_iter()
                    .map(move |value| {
                        let sector = f64::from(hue) / 4.0;
                        let fraction = sector.fract();
                        let [p, q, t] = [
                            value * (1.0 - saturation),
                            value * (1.0 - saturation * fraction),
                            value * (1.0 - saturation * (1.0 - fraction)),
                        ];
                        match sector as u32 {
                            0 => [value, t, p],
                            1 => [q, value, p],
                            2 => [p, value, t],
                            3 => [p, q, value],
                            4 => [t, p, value],
                            _ => [value, p, q],
                        }
                    })
            })
    });
    greys.chain(colours).collect()
}

/// `tonalColor` v1 (`fx_composition`'s `tonal_color.wgsl`) of one opaque
/// display colour.
fn tonal_color(tonal: &fx_schema::TonalColor, rgb: [f64; 3]) -> [f64; 3] {
    let smoothstep = |edge0: f64, edge1: f64, x: f64| {
        let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let global = [tonal.global_red, tonal.global_green, tonal.global_blue];
    let ranges = [
        [tonal.shadows_red, tonal.shadows_green, tonal.shadows_blue],
        [
            tonal.midtones_red,
            tonal.midtones_green,
            tonal.midtones_blue,
        ],
        [
            tonal.highlights_red,
            tonal.highlights_green,
            tonal.highlights_blue,
        ],
    ];
    let base = rgb.map(|c| c.clamp(0.0, 1.0));
    let light: f64 = base
        .iter()
        .zip(REC709)
        .map(|(c, w)| c * w)
        .sum::<f64>()
        .clamp(0.0, 1.0);
    let shadow = 1.0 - smoothstep(0.25, 0.5, light);
    let highlight = smoothstep(0.5, 0.75, light);
    let weights = [shadow, 1.0 - shadow - highlight, highlight];
    let delta: [f64; 3] = std::array::from_fn(|channel| {
        global[channel]
            + weights
                .iter()
                .zip(&ranges)
                .map(|(weight, range)| weight * range[channel])
                .sum::<f64>()
    });
    let luma: f64 = delta.iter().zip(REC709).map(|(d, w)| d * w).sum();
    let chroma = delta.map(|d| d - luma);
    let fit = chroma
        .iter()
        .zip(base)
        .map(|(&c, value)| {
            if c > 0.0 {
                (1.0 - value) / c
            } else if c < 0.0 {
                -value / c
            } else {
                1.0
            }
        })
        .fold(1.0, f64::min)
        .clamp(0.0, 1.0);
    std::array::from_fn(|channel| base[channel] + fit * chroma[channel])
}

/// The wheel `(hue, luma, amount)` whose per-channel offset
/// `amount * c(hue) + luma - 0.5` ([`offsets`] at scale 1) is `offset`.
fn wheel(offset: [f64; 3], name: &str) -> Result<[f64; 3], String> {
    let luma_offset: f64 = offset.iter().zip(REC709).map(|(o, w)| o * w).sum();
    let chroma = offset.map(|o| o - luma_offset);
    let length = chroma.iter().map(|c| c * c).sum::<f64>().sqrt();
    let luma = 0.5 + luma_offset;
    if length < 1e-9 {
        return range_checked([0.0, luma, 0.0], name);
    }
    // c(hue) sweeps every direction of the zero-luma plane once.
    let alignment = |hue: f64| {
        let direction = hue_chroma(hue);
        let size = direction.iter().map(|c| c * c).sum::<f64>().sqrt();
        direction
            .iter()
            .zip(chroma)
            .map(|(d, c)| d * c)
            .sum::<f64>()
            / (size * length)
    };
    let mut hue = (0..3600)
        .map(|step| f64::from(step) / 10.0)
        .max_by(|a, b| alignment(*a).total_cmp(&alignment(*b)))
        .unwrap_or(0.0);
    let mut step = 0.05;
    for _ in 0..30 {
        hue = [hue - step, hue, hue + step]
            .into_iter()
            .max_by(|a, b| alignment(*a).total_cmp(&alignment(*b)))
            .unwrap_or(hue);
        step *= 0.5;
    }
    let direction = hue_chroma(hue);
    let amount = length / direction.iter().map(|c| c * c).sum::<f64>().sqrt();
    range_checked([hue.rem_euclid(360.0), luma, amount], name)
}

fn range_checked(wheel: [f64; 3], name: &str) -> Result<[f64; 3], String> {
    let [_, luma, amount] = wheel;
    if !(0.0..=1.0).contains(&luma) || !(0.0..=1.0).contains(&amount) {
        return Err(format!(
            "the fitted {name} wheel (luma {luma:.3}, amount {amount:.3}) lies outside Lumetri's 0 to 1"
        ));
    }
    Ok(wheel)
}

/// Nelder–Mead minimum of `f` from `start` (initial steps 0.1).
fn nelder_mead(f: impl Fn([f64; 3]) -> f64, start: [f64; 3]) -> [f64; 3] {
    let mut simplex: Vec<([f64; 3], f64)> = (0..4)
        .map(|vertex| {
            let mut point = start;
            if vertex > 0 {
                point[vertex - 1] += 0.1;
            }
            (point, f(point))
        })
        .collect();
    let blend = |a: [f64; 3], b: [f64; 3], t: f64| -> [f64; 3] {
        std::array::from_fn(|index| a[index] + t * (b[index] - a[index]))
    };
    for _ in 0..2000 {
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
        if (simplex[3].1 - simplex[0].1).abs() < 1e-14 {
            break;
        }
        let centroid: [f64; 3] =
            std::array::from_fn(|index| simplex[..3].iter().map(|v| v.0[index]).sum::<f64>() / 3.0);
        let worst = simplex[3];
        let reflected = blend(centroid, worst.0, -1.0);
        let reflected_value = f(reflected);
        if reflected_value < simplex[0].1 {
            let expanded = blend(centroid, worst.0, -2.0);
            let expanded_value = f(expanded);
            simplex[3] = if expanded_value < reflected_value {
                (expanded, expanded_value)
            } else {
                (reflected, reflected_value)
            };
        } else if reflected_value < simplex[2].1 {
            simplex[3] = (reflected, reflected_value);
        } else {
            let contracted = blend(centroid, worst.0, 0.5);
            let contracted_value = f(contracted);
            if contracted_value < worst.1 {
                simplex[3] = (contracted, contracted_value);
            } else {
                let best = simplex[0].0;
                for vertex in &mut simplex[1..] {
                    vertex.0 = blend(best, vertex.0, 0.5);
                    vertex.1 = f(vertex.0);
                }
            }
        }
    }
    simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
    simplex[0].0
}

/// The piecewise-linear curve within [`FIT_TOLERANCE_CODES`] of `table`
/// (values at the 8-bit input codes) whose points Premiere's natural cubic
/// spline also follows within that tolerance, so the curve exports as native
/// Lumetri RGB Curves unchanged: greedy knot insertion at the worst code.
fn fitted(table: &[f64]) -> Result<ColorCurve, String> {
    let scale = (SAMPLES - 1) as f64;
    let x = |code: usize| code as f64 / scale;
    let mut knots = vec![0, SAMPLES - 1];
    loop {
        let points: Vec<[f64; 2]> = knots.iter().map(|&code| [x(code), table[code]]).collect();
        let mut worst: Option<(usize, f64)> = None;
        for segment in knots.windows(2) {
            let [start, end] = [segment[0], segment[1]];
            for code in start + 1..end {
                let fraction = (code - start) as f64 / (end - start) as f64;
                let linear = table[start] + fraction * (table[end] - table[start]);
                let spline = super::rgb_curves::spline_value(&points, x(code));
                let error = (linear - table[code]).abs().max((spline - linear).abs()) * scale;
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
                "a Color Wheels channel needs more than {MAX_COLOR_CURVE_POINTS} linear points within one 8-bit code"
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
            .map(|code| ColorCurvePoint::new(x(code), table[code]))
            .collect(),
    )
    .map_err(|error| format!("fitted Color Wheels curve: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The saved neutral wheel.
    const NEUTRAL: [f64; 3] = [0.0, 0.5, 0.0];

    fn wheels(shadows: [f64; 3], midtones: [f64; 3], highlights: [f64; 3]) -> Wheels {
        Wheels {
            wheels: [shadows, midtones, highlights, NEUTRAL],
        }
    }

    /// Premiere 26.5.2 renders (8-bit RGB codes) of in-house wheel values on
    /// the grey ramp, at input codes 0, 16, 64, 128, 192, 240 and 255: probes
    /// w2 clip 0 (Shadows luma 0), w2 clip 7 (Highlights luma 1), lane D
    /// clip 7 (Midtones 0 degrees amount 1), w4 clip 3 (Shadows 120 degrees
    /// 0.5, Midtones 240 degrees 0.5, Highlights 0 degrees 0.5) and w4 clip
    /// 7 (all four wheels moved).
    #[allow(clippy::type_complexity)]
    const MEASURED: [(Wheels, [[f64; 3]; 7]); 5] = [
        (
            Wheels {
                wheels: [[0.0, 0.0, 0.0], NEUTRAL, NEUTRAL, NEUTRAL],
            },
            [
                [0.0; 3], [0.0; 3], [17.0; 3], [96.0; 3], [176.0; 3], [236.0; 3], [255.0; 3],
            ],
        ),
        (
            Wheels {
                wheels: [NEUTRAL, NEUTRAL, [0.0, 1.0, 0.0], NEUTRAL],
            },
            [
                [0.0; 3], [20.0; 3], [80.0; 3], [160.0; 3], [239.0; 3], [255.0; 3], [255.0; 3],
            ],
        ),
        (
            Wheels {
                wheels: [NEUTRAL, [0.0, 0.5, 1.0], NEUTRAL, NEUTRAL],
            },
            [
                [0.0, 0.0, 0.0],
                [142.0, 9.0, 9.0],
                [190.0, 48.0, 48.0],
                [220.0, 111.0, 111.0],
                [240.0, 181.0, 181.0],
                [252.0, 237.0, 237.0],
                [255.0, 255.0, 255.0],
            ],
        ),
        (
            Wheels {
                wheels: [
                    [120.0, 0.5, 0.5],
                    [240.0, 0.5, 0.5],
                    [0.0, 0.5, 0.5],
                    NEUTRAL,
                ],
            },
            [
                [0.0, 16.0, 0.0],
                [1.0, 29.0, 10.0],
                [33.0, 70.0, 78.0],
                [123.0, 127.0, 154.0],
                [215.0, 184.0, 206.0],
                [254.0, 228.0, 238.0],
                [255.0, 241.0, 248.0],
            ],
        ),
        (
            Wheels {
                wheels: [
                    [30.0, 0.6, 0.4],
                    [150.0, 0.4, 0.3],
                    [270.0, 0.7, 0.5],
                    [60.0, 0.8, 0.9],
                ],
            },
            [
                [23.0, 9.0, 0.0],
                [35.0, 25.0, 4.0],
                [80.0, 73.0, 54.0],
                [151.0, 138.0, 144.0],
                [230.0, 204.0, 240.0],
                [255.0, 249.0, 255.0],
                [255.0, 255.0, 255.0],
            ],
        ),
    ];

    #[test]
    fn wheel_model_and_fit_follow_premiere_renders() {
        for (wheels, measured) in MEASURED {
            let tables = channel_transfers(&wheels).unwrap();
            let LayerEffect::ColorCurves { curves, .. } = color_curves(&wheels).unwrap() else {
                panic!("colorCurves expected");
            };
            assert!(curves.master.is_identity());
            let fitted = [&curves.red, &curves.green, &curves.blue];
            for (code, expected) in [0, 16, 64, 128, 192, 240, 255].into_iter().zip(measured) {
                for channel in 0..3 {
                    let model = tables[channel][code] * 255.0;
                    assert!(
                        (model - expected[channel]).abs() <= 3.2,
                        "{wheels:?} code {code} channel {channel}: model {model}, Premiere {}",
                        expected[channel]
                    );
                    let linear = linear_value(fitted[channel], code as f64 / 255.0) * 255.0;
                    assert!(
                        (linear - model).abs() <= FIT_TOLERANCE_CODES + 1e-9,
                        "{wheels:?} code {code} channel {channel}: fitted {linear}, model {model}"
                    );
                    let points: Vec<[f64; 2]> = fitted[channel]
                        .points()
                        .iter()
                        .map(|point| [point.x(), point.y()])
                        .collect();
                    let spline =
                        super::super::rgb_curves::spline_value(&points, code as f64 / 255.0)
                            * 255.0;
                    assert!(
                        (spline - linear).abs() <= FIT_TOLERANCE_CODES + 1e-9,
                        "{wheels:?} code {code} channel {channel}: spline {spline}, fitted {linear}"
                    );
                }
            }
        }
    }

    #[test]
    fn neutral_wheels_are_the_identity_and_hue_zero_pushes_red() {
        let tables = channel_transfers(&wheels(NEUTRAL, NEUTRAL, NEUTRAL)).unwrap();
        for table in &tables {
            for (code, value) in table.iter().enumerate() {
                assert!((value * 255.0 - code as f64).abs() < 1e-9);
            }
        }
        let [red, green, blue] = hue_chroma(0.0);
        assert!(red > 0.0 && green < 0.0 && blue < 0.0);
        let [red, green, blue] = hue_chroma(120.0);
        assert!(green > 0.0 && red < 0.0 && blue < 0.0);
        let luma: f64 = hue_chroma(200.0)
            .iter()
            .zip(REC709)
            .map(|(c, w)| c * w)
            .sum();
        assert!(luma.abs() < 1e-12);
    }

    fn tonal(shadows: [f64; 3], midtones: [f64; 3], highlights: [f64; 3]) -> fx_schema::TonalColor {
        serde_json::from_value(serde_json::json!({
            "semanticVersion": 1,
            "globalRed": 0.0, "globalGreen": 0.0, "globalBlue": 0.0,
            "shadowsRed": shadows[0], "shadowsGreen": shadows[1], "shadowsBlue": shadows[2],
            "midtonesRed": midtones[0], "midtonesGreen": midtones[1], "midtonesBlue": midtones[2],
            "highlightsRed": highlights[0], "highlightsGreen": highlights[1], "highlightsBlue": highlights[2],
        }))
        .expect("valid tonalColor")
    }

    /// A neutral tonalColor fits neutral wheels; a warm-shadows /
    /// cool-highlights grade fits wheels pushing the matching hues whose
    /// model stays within a few codes of tonalColor on the grey ramp, and
    /// the wheels round-trip through the import model.
    #[test]
    fn tonal_color_fits_wheels_on_greys_and_colours() {
        let (wheels, [mean, _]) =
            from_tonal_color(&tonal([0.0; 3], [0.0; 3], [0.0; 3])).expect("neutral");
        assert!(wheels.is_neutral() || wheels.wheels[..3].iter().all(|w| w[2] < 1e-3));
        assert!(mean < 0.1, "{mean}");
        let graded = tonal([0.08, 0.0, -0.08], [0.0, 0.03, 0.0], [-0.06, 0.0, 0.06]);
        let (wheels, [mean, largest]) = from_tonal_color(&graded).expect("in range");
        let [shadows, midtones, highlights, fourth] = wheels.wheels;
        assert_eq!(fourth, [0.0, 0.5, 0.0]);
        // Warm shadows push towards red/orange, cool highlights towards blue.
        assert!(!(60.0..300.0).contains(&shadows[0]), "{shadows:?}");
        assert!((150.0..270.0).contains(&highlights[0]), "{highlights:?}");
        assert!((60.0..180.0).contains(&midtones[0]), "{midtones:?}");
        // tonalColor's gamut fit vanishes at black and white, where a
        // wheel's lift and gain still act, and it shifts chroma by luma
        // window while the wheels act per channel: the fit removes only
        // part of the effect.
        let samples = tonal_samples();
        let effect = samples
            .iter()
            .map(|&rgb| {
                let out = tonal_color(&graded, rgb);
                (0..3).map(|c| (out[c] - rgb[c]).abs()).sum::<f64>() * 255.0 / 3.0
            })
            .sum::<f64>()
            / samples.len() as f64;
        assert!(
            mean < 0.85 * effect && largest < 40.0,
            "{mean} {largest} {effect}"
        );
        assert!(color_curves(&wheels).is_ok());
    }

    #[test]
    fn a_midtones_power_below_the_measured_range_is_rejected() {
        let error = color_curves(&wheels(NEUTRAL, [240.0, 1.0, 1.0], NEUTRAL)).unwrap_err();
        assert!(error.contains("below the measured range"), "{error}");
    }

    fn linear_value(curve: &ColorCurve, x: f64) -> f64 {
        let points = curve.points();
        let index = points.windows(2).position(|pair| x <= pair[1].x()).unwrap();
        let (a, b) = (points[index], points[index + 1]);
        a.y() + (x - a.x()) / (b.x() - a.x()) * (b.y() - a.y())
    }
}
