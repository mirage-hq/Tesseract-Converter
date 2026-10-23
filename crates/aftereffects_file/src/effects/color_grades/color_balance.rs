//! After Effects Color Balance (`ADBE Color Balance 2`) as per-channel curves.
//!
//! AE 26.5 Color Balance is an exact per-channel transfer on eight-bit codes
//! (measured: a table read from a grey ramp predicts every colour of the ramp
//! with error 0, on 124 probe looks). Each channel is a Levels map with input
//! black `b`, input white `w` and gamma `g`:
//!
//! - Preserve Luminosity off: `b = max(0, -shadow)`, `w = 255 - max(0, highlight)`,
//!   `g = 2^(-(2 midtone + shadow + highlight) / 200)`.
//! - Preserve Luminosity on: shadows only raise black points
//!   (`b = max(shadows) - shadow`), highlights only lower white points
//!   (`w = 255 - (highlight - min(highlights))`), and midtones are a gamma
//!   `2^(-(midtone - (max + min of midtones) / 2) / 100)`.
//!
//! A brightening gamma (`g < 1`) does not follow `v^g` near black: AE limits
//! its slope there. The measured shape is reproduced by a cubic Hermite toe
//! below input code 32 whose slope at black is `1 + 5.8 (1 - g)` (an
//! empirical fit). A black point at or above the white point is a step to 255
//! at the white point. Against 156 measured AE tables (24 of them random
//! held-out looks) the model's mean error is 0.12 codes, 99.9 % of entries are
//! within 2 codes, and the worst entry (a steep ramp of fractional balances)
//! is 10 codes off. FX evaluates the result as editable piecewise-linear
//! `colorCurves`, which cannot keep the balances as separate controls.

use fx_schema::{ColorCurve, ColorCurves};

use crate::effects::curves;

/// Eight-bit code values per channel table.
const CODES: usize = 256;
/// Normalised input below which a brightening gamma uses the measured toe.
const TOE: f64 = 32.0 / 255.0;
/// Toe slope at black per unit of `1 - gamma` (fitted to AE renders).
const TOE_SLOPE: f64 = 5.8;

/// Static Color Balance controls in AE units (each balance -100..=100).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Balances {
    /// Shadow Red/Green/Blue Balance.
    pub shadows: [f64; 3],
    /// Midtone Red/Green/Blue Balance.
    pub midtones: [f64; 3],
    /// Highlight Red/Green/Blue Balance.
    pub highlights: [f64; 3],
    /// Preserve Luminosity checkbox.
    pub preserve_luminosity: bool,
}

/// One channel's Levels map on eight-bit codes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ChannelLevels {
    black: f64,
    white: f64,
    gamma: f64,
}

fn max(values: [f64; 3]) -> f64 {
    values.into_iter().fold(f64::NEG_INFINITY, f64::max)
}

fn min(values: [f64; 3]) -> f64 {
    values.into_iter().fold(f64::INFINITY, f64::min)
}

impl Balances {
    fn levels(&self) -> [ChannelLevels; 3] {
        std::array::from_fn(|channel| {
            let shadow = self.shadows[channel];
            let midtone = self.midtones[channel];
            let highlight = self.highlights[channel];
            if self.preserve_luminosity {
                let centre = (max(self.midtones) + min(self.midtones)) / 2.0;
                ChannelLevels {
                    black: max(self.shadows) - shadow,
                    white: 255.0 - (highlight - min(self.highlights)),
                    gamma: (-(midtone - centre) / 100.0).exp2(),
                }
            } else {
                ChannelLevels {
                    black: (-shadow).max(0.0),
                    white: 255.0 - highlight.max(0.0),
                    gamma: (-(2.0 * midtone + shadow + highlight) / 200.0).exp2(),
                }
            }
        })
    }

    /// AE's Red/Green/Blue transfer tables, rounded to eight-bit codes.
    pub(crate) fn tables(&self) -> [[u8; CODES]; 3] {
        self.levels().map(|levels| {
            std::array::from_fn(|code| {
                // The transfer is clamped to 0..=255, so the cast cannot truncate.
                levels.transfer(code as f64).round() as u8
            })
        })
    }

    /// The tables as editable FX curves (identity Master), each within one
    /// code of its table.
    pub(crate) fn curves(&self) -> Result<ColorCurves, String> {
        let [red, green, blue] = self.tables();
        let fit = |table: &[u8; CODES]| {
            curves::fit(table).map_err(|error| format!("Color Balance curve fit failed ({error})"))
        };
        Ok(ColorCurves {
            master: ColorCurve::identity(),
            red: fit(&red)?,
            green: fit(&green)?,
            blue: fit(&blue)?,
        })
    }
}

impl ChannelLevels {
    /// Output code for input `code`, both in 0..=255.
    fn transfer(self, code: f64) -> f64 {
        let span = self.white - self.black;
        let input = if span > 0.0 {
            ((code - self.black) / span).clamp(0.0, 1.0)
        } else if code >= self.white {
            1.0
        } else {
            0.0
        };
        255.0 * gamma(input, self.gamma).clamp(0.0, 1.0)
    }
}

/// `input^gamma`, with AE's slope-limited toe for a brightening gamma.
fn gamma(input: f64, gamma: f64) -> f64 {
    if gamma >= 1.0 || input >= TOE {
        return input.powf(gamma);
    }
    // Cubic Hermite from (0, 0) with slope `toe_slope` to the power curve at
    // TOE, matching its value and slope there.
    let toe_slope = 1.0 + TOE_SLOPE * (1.0 - gamma);
    let end = TOE.powf(gamma);
    let end_slope = gamma * end / TOE;
    let t = input / TOE;
    let (t2, t3) = (t * t, t * t * t);
    (t3 - 2.0 * t2 + t) * toe_slope * TOE
        + (3.0 * t2 - 2.0 * t3) * end
        + (t3 - t2) * end_slope * TOE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sample input codes of the measured tables below.
    const SAMPLES: [usize; 12] = [1, 4, 8, 16, 32, 64, 96, 128, 160, 192, 224, 250];
    const IDENTITY: [u8; 12] = [1, 4, 8, 16, 32, 64, 96, 128, 160, 192, 224, 250];

    fn balances(pairs: &[(&str, f64)], preserve_luminosity: bool) -> Balances {
        let mut balances = Balances {
            preserve_luminosity,
            ..Balances::default()
        };
        for (name, value) in pairs {
            let (range, channel) = name.split_at(1);
            let channel = "rgb".find(channel).expect("channel letter");
            match range {
                "s" => balances.shadows[channel] = *value,
                "m" => balances.midtones[channel] = *value,
                "h" => balances.highlights[channel] = *value,
                _ => panic!("range letter"),
            }
        }
        balances
    }

    /// AE 26.5 (8 bpc) Color Balance tables read from renders of an in-house
    /// grey ramp: headless-adobe `render_aep` jobs 11930–11935 (probe comps
    /// cbp1–cbp6) and the lane G wave-6 `cb_mixed` comp (the fixture's look).
    fn assert_matches_ae(balances: Balances, measured: [[u8; 12]; 3]) {
        let tables = balances.tables();
        for (channel, expected) in measured.iter().enumerate() {
            for (sample, &code) in SAMPLES.iter().enumerate() {
                let model = i32::from(tables[channel][code]);
                let native = i32::from(expected[sample]);
                assert!(
                    (model - native).abs() <= 1,
                    "{balances:?} channel {channel} code {code}: model {model}, AE {native}"
                );
            }
        }
    }

    #[test]
    fn preserve_luminosity_off_is_a_levels_map_with_a_toe_measured_in_after_effects() {
        // Midtone Red +100: gamma 0.5 with the slope-limited toe.
        assert_matches_ae(
            balances(&[("mr", 100.0)], false),
            [
                [4, 16, 30, 55, 90, 128, 156, 181, 202, 221, 239, 252],
                IDENTITY,
                IDENTITY,
            ],
        );
        // Shadow Red -25: input black 25 and gamma 2^(25/200).
        assert_matches_ae(
            balances(&[("sr", -25.0)], false),
            [
                [0, 0, 0, 0, 6, 37, 71, 106, 143, 180, 218, 248],
                IDENTITY,
                IDENTITY,
            ],
        );
        // Highlight Red +50: input white 205 and gamma 2^(-50/200).
        assert_matches_ae(
            balances(&[("hr", 50.0)], false),
            [
                [2, 9, 17, 30, 54, 96, 134, 171, 207, 241, 255, 255],
                IDENTITY,
                IDENTITY,
            ],
        );
    }

    #[test]
    fn preserve_luminosity_on_normalises_each_range_as_measured_in_after_effects() {
        // Shadows only raise black points: max(shadows) - shadow.
        assert_matches_ae(
            balances(&[("sr", 30.0), ("sg", -30.0)], true),
            [
                IDENTITY,
                [0, 0, 0, 0, 0, 5, 47, 89, 131, 173, 214, 248],
                [0, 0, 0, 0, 2, 39, 75, 111, 147, 184, 220, 249],
            ],
        );
        // Midtones are centred on (max + min) / 2.
        assert_matches_ae(
            balances(&[("mr", 50.0), ("mg", -20.0), ("mb", 10.0)], true),
            [
                [2, 9, 17, 29, 50, 86, 118, 148, 177, 204, 230, 251],
                [0, 1, 3, 7, 18, 43, 73, 105, 140, 177, 216, 249],
                [1, 4, 7, 15, 30, 61, 93, 125, 158, 190, 223, 250],
            ],
        );
        // Highlights only lower white points: highlight - min(highlights).
        assert_matches_ae(
            balances(&[("hr", 50.0), ("hg", -20.0), ("hb", 10.0)], true),
            [
                [1, 6, 11, 22, 44, 88, 132, 176, 221, 255, 255, 255],
                IDENTITY,
                [1, 5, 9, 18, 36, 73, 109, 145, 181, 218, 254, 255],
            ],
        );
        // The color_ae_color_balance fixture look, all three ranges at once.
        assert_matches_ae(
            balances(&[("sb", 40.0), ("mr", 30.0), ("hg", -25.0)], true),
            [
                [0, 0, 0, 0, 0, 39, 85, 127, 169, 209, 248, 255],
                [0, 0, 0, 0, 0, 22, 57, 94, 133, 173, 214, 248],
                [1, 3, 6, 13, 28, 62, 96, 133, 170, 209, 247, 255],
            ],
        );
        // Black point 100 and white point 155 leave a narrow ramp.
        assert_matches_ae(
            balances(&[("sr", -100.0), ("hr", 100.0)], true),
            [
                [0, 0, 0, 0, 0, 0, 0, 130, 255, 255, 255, 255],
                IDENTITY,
                IDENTITY,
            ],
        );
        // Black point 200 above white point 55: a step to 255 at code 55
        // (validation render, headless-adobe job 11944).
        let crossed = balances(
            &[("sr", -100.0), ("sg", 100.0), ("hr", 100.0), ("hg", -100.0)],
            true,
        )
        .tables();
        assert_eq!(
            &crossed[0][50..60],
            &[0, 0, 0, 0, 0, 255, 255, 255, 255, 255]
        );
        assert_eq!(crossed[0][255], 255);
    }

    #[test]
    fn neutral_balances_are_identity_curves() {
        let curves = Balances::default().curves().unwrap();
        assert!(curves.is_identity());
    }

    #[test]
    fn every_fitted_curve_stays_within_one_code_of_its_table() {
        let balances = balances(
            &[
                ("sr", -72.0),
                ("sb", 51.0),
                ("mg", 94.0),
                ("mb", -78.0),
                ("hr", 32.0),
                ("hg", -58.0),
            ],
            true,
        );
        let curves = balances.curves().unwrap();
        assert!(curves.master.is_identity());
        let tables = balances.tables();
        for (curve, table) in [&curves.red, &curves.green, &curves.blue]
            .iter()
            .zip(&tables)
        {
            let points = curve.points();
            for (code, expected) in table.iter().enumerate() {
                let x = code as f64 / 255.0;
                let pair = points
                    .windows(2)
                    .find(|pair| x <= pair[1].x())
                    .expect("curve covers 0..=1");
                let fraction = (x - pair[0].x()) / (pair[1].x() - pair[0].x());
                let y = (pair[0].y() + fraction * (pair[1].y() - pair[0].y())) * 255.0;
                assert!(
                    (y - f64::from(*expected)).abs() <= 1.0 + 1e-9,
                    "code {code}: curve {y}, table {expected}"
                );
            }
        }
    }
}
