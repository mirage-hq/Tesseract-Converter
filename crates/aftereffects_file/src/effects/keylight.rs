//! Bounded static `Keylight 906` profile, lowered to the existing editable
//! `CustomShader` with `keylight.wgsl` as its pixel model.
//!
//! [`CONTROLS`] is the supported parameter ABI: the native declaration types
//! and plugin defaults of Keylight (1.2), as recorded from its `pard`
//! declarations. A control resolves from its explicit record, else from the
//! instance's own declaration, else from this profile. AE omits the
//! declarations of some instances, so the profile supplies only genuinely
//! absent controls of an instance whose declaration table is missing or
//! readable; it never copies another instance's values. Another plugin build
//! that shares this match name is not established.
//!
//! The supported configuration is View Final Result, Soft Colour replacement
//! with the neutral Replace Colour, Source Alpha Normal, Clip Black 0, neutral
//! biases and no pre-blur, matte processing, colour correction, source crops
//! or Inside/Outside Mask. Screen Colour, Screen Gain, Screen Balance and
//! Clip White become editable shader parameters in native units. Any other
//! value, animation, enabled expression, undecodable or duplicated record or
//! declaration, unreadable declaration table, or unknown or incompatibly
//! declared control is a reason to omit the instance.

use fx_schema::{EffectParam, LayerEffect};

use super::native::{Declarations, DecodedEffect, DecodedParameter};
use crate::properties::{NumericProperty, NumericValueKind};

pub(crate) const MATCH_NAME: &str = "Keylight 906";

/// Diagnostic for every converted instance.
pub(crate) const APPROXIMATION: &str = "bounded static Keylight 906 profile converted to an editable CustomShader approximation (dominant-channel difference matte with Screen Gain and Balance, linear Clip White, screen subtraction, neutral Soft Colour and Source Alpha Normal) on display-referred SDR values; Keylight's exact matte, despill and Soft Colour equations, HDR or colour-managed behaviour, edge resampling and native alpha are unverified, and FX→AEP export omits custom shaders";

const DESCRIPTION: &str = "Converted from After Effects Keylight 906 (Keylight 1.2): View Final Result, Soft Colour replacement, Source Alpha Normal. An approximation of documented behaviour, not Keylight's unpublished algorithm: a difference matte on the Screen Colour's dominant channel, scaled by Screen Gain, where Screen Balance weights the two other channels as ordered in the Screen Colour; a linear Clip White remap with Clip Black 0; screen-colour subtraction; and a neutral Soft Colour replacement with Rec. 601 luminance where clipping raises the matte. Pixels with another dominant channel are unchanged. Values are display-referred SDR; HDR or colour-managed input is not supported.";

const WGSL: &str = include_str!("keylight.wgsl");

/// Smallest Screen Colour difference that the single-precision matte divides
/// by; `keylight.wgsl` passes pixels through below the same limit.
const MIN_SCREEN_DIFFERENCE: f64 = 1e-6;

/// Tolerance for comparing decoded values with a required plugin default.
const TOLERANCE: f64 = 1e-6;

/// AE `PF_ParamType` values in this profile's declarations.
const FIXED_SLIDER: u32 = 2;
const CHECKBOX: u32 = 4;
const COLOR: u32 = 5;
const POPUP: u32 = 7;
const NO_DATA: u32 = 9;
const PATH: u32 = 12;
const GROUP_START: u32 = 13;
const GROUP_END: u32 = 14;

#[derive(Clone, Copy, Debug)]
enum Rule {
    /// A group marker or button, or a control of a feature that the required
    /// values keep disabled: it cannot change the render.
    Inert,
    /// The shader implements only this plugin-default number.
    Number(f64),
    /// The shader implements only this plugin-default normalized colour.
    Colour([f64; 3]),
    /// An Inside/Outside Mask path selector, which must select no path.
    NoPath,
    /// Editable shader inputs with their plugin defaults.
    ScreenColour([f64; 3]),
    ScreenGain(f64),
    ScreenBalance(f64),
    ClipWhite(f64),
}

struct Control {
    suffix: &'static str,
    label: &'static str,
    kind: u32,
    rule: Rule,
}

const fn control(suffix: &'static str, label: &'static str, kind: u32, rule: Rule) -> Control {
    Control {
        suffix,
        label,
        kind,
        rule,
    }
}

const NEUTRAL: Rule = Rule::Colour([127.0 / 255.0; 3]);

use Rule::{Inert, NoPath, Number};

/// Every Keylight 906 control in native order (`-0000` and the built-in
/// parameter group are excluded by the native decoder).
const CONTROLS: [Control; 79] = [
    control("0001", "About", NO_DATA, Inert),
    control("0002", "View", POPUP, Number(11.0)), // Final Result
    control("0003", "Unpremultiply Result", CHECKBOX, Number(0.0)),
    control("0004", "Screen Colour", COLOR, Rule::ScreenColour([0.0; 3])),
    control("0005", "Screen Gain", FIXED_SLIDER, Rule::ScreenGain(100.0)),
    control(
        "0006",
        "Screen Balance",
        FIXED_SLIDER,
        Rule::ScreenBalance(50.0),
    ),
    control("0007", "Despill Bias", COLOR, NEUTRAL),
    control("0008", "Alpha Bias", COLOR, NEUTRAL),
    control("0009", "Lock Biases Together", CHECKBOX, Inert),
    control("0010", "Screen Pre-blur", FIXED_SLIDER, Number(0.0)),
    control("0011", "Screen Matte", GROUP_START, Inert),
    control("0012", "Clip Black", FIXED_SLIDER, Number(0.0)),
    control("0013", "Clip White", FIXED_SLIDER, Rule::ClipWhite(100.0)),
    control("0014", "Clip Rollback", FIXED_SLIDER, Number(0.0)),
    control("0015", "Screen Shrink/Grow", FIXED_SLIDER, Number(0.0)),
    control("0016", "Screen Softness", FIXED_SLIDER, Number(0.0)),
    control("0017", "Screen Despot Black", FIXED_SLIDER, Number(0.0)),
    control("0018", "Screen Despot White", FIXED_SLIDER, Number(0.0)),
    control("0019", "Replace Method", POPUP, Number(4.0)), // Soft Colour
    control("0020", "Replace Colour", COLOR, NEUTRAL),
    control("0021", "Screen Matte", GROUP_END, Inert),
    control("0022", "Inside Mask", GROUP_START, Inert),
    control("0023", "Inside Mask", PATH, NoPath),
    control("0024", "Inside Mask Softness", FIXED_SLIDER, Inert),
    control("0025", "Inside Mask Invert", CHECKBOX, Number(0.0)),
    control("0026", "Inside Mask Replace Method", POPUP, Inert),
    control("0027", "Inside Mask Replace Colour", COLOR, Inert),
    control("0028", "Source Alpha", POPUP, Number(3.0)), // Normal
    control("0029", "Inside Mask", GROUP_END, Inert),
    control("0030", "Outside Mask", GROUP_START, Inert),
    control("0031", "Outside Mask", PATH, NoPath),
    control("0032", "Outside Mask Softness", FIXED_SLIDER, Inert),
    control("0033", "Outside Mask Invert", CHECKBOX, Number(0.0)),
    control("0034", "Outside Mask", GROUP_END, Inert),
    control("0035", "Foreground Colour Correction", GROUP_START, Inert),
    control("0036", "Enable Colour Correction", CHECKBOX, Number(0.0)),
    control("0037", "Saturation", FIXED_SLIDER, Inert),
    control("0038", "Contrast", FIXED_SLIDER, Inert),
    control("0039", "Brightness", FIXED_SLIDER, Inert),
    control("0040", "Colour Suppression", GROUP_START, Inert),
    control("0041", "Suppress", POPUP, Inert),
    control("0042", "Suppression Balance", FIXED_SLIDER, Inert),
    control("0043", "Suppression Amount", FIXED_SLIDER, Inert),
    control("0044", "Colour Suppression", GROUP_END, Inert),
    control("0045", "Colour Balancing", GROUP_START, Inert),
    control("0046", "Hue", FIXED_SLIDER, Inert),
    control("0047", "Sat", FIXED_SLIDER, Inert),
    control("0048", "Colour Balance Wheel", NO_DATA, Inert),
    control("0049", "Colour Balancing", GROUP_END, Inert),
    control("0050", "Foreground Colour Correction", GROUP_END, Inert),
    control("0051", "Edge Colour Correction", GROUP_START, Inert),
    control(
        "0052",
        "Enable Edge Colour Correction",
        CHECKBOX,
        Number(0.0),
    ),
    control("0053", "Edge Hardness", FIXED_SLIDER, Inert),
    control("0054", "Edge Softness", FIXED_SLIDER, Inert),
    control("0055", "Edge Grow", FIXED_SLIDER, Inert),
    control("0056", "Edge Saturation", FIXED_SLIDER, Inert),
    control("0057", "Edge Contrast", FIXED_SLIDER, Inert),
    control("0058", "Edge Brightness", FIXED_SLIDER, Inert),
    control("0059", "Edge Colour Suppression", GROUP_START, Inert),
    control("0060", "Edge Suppress", POPUP, Inert),
    control("0061", "Edge Suppression Balance", FIXED_SLIDER, Inert),
    control("0062", "Edge Suppression Amount", FIXED_SLIDER, Inert),
    control("0063", "Edge Colour Suppression", GROUP_END, Inert),
    control("0064", "Edge Colour Balancing", GROUP_START, Inert),
    control("0065", "Edge Hue", FIXED_SLIDER, Inert),
    control("0066", "Edge Sat", FIXED_SLIDER, Inert),
    control("0067", "Edge Colour Balance Wheel", NO_DATA, Inert),
    control("0068", "Edge Colour Balancing", GROUP_END, Inert),
    control("0069", "Edge Colour Correction", GROUP_END, Inert),
    control("0070", "Source Crops", GROUP_START, Inert),
    control("0071", "X Method", POPUP, Inert),
    control("0072", "Y Method", POPUP, Inert),
    control("0073", "Edge Colour", COLOR, Inert),
    control("0074", "Edge Colour Alpha", FIXED_SLIDER, Inert),
    control("0075", "Crop Left", FIXED_SLIDER, Number(0.0)),
    control("0076", "Crop Right", FIXED_SLIDER, Number(100.0)),
    control("0077", "Crop Top", FIXED_SLIDER, Number(0.0)),
    control("0078", "Crop Bottom", FIXED_SLIDER, Number(100.0)),
    control("0079", "Source Crops", GROUP_END, Inert),
];

/// An admitted instance and the supported controls it did not store.
pub(crate) struct Lowered {
    pub effect: LayerEffect,
    /// Labels of absent controls resolved from the profile's plugin defaults.
    pub defaulted: Vec<&'static str>,
}

/// Resolved editable inputs in native units.
struct Settings {
    screen: [f64; 3],
    gain: f64,
    balance: f64,
    clip_white: f64,
}

/// Lower one decoded native occurrence, or return why it is outside the
/// supported profile. The caller owns identity, enable state and diagnostics.
pub(crate) fn lower(source: &DecodedEffect) -> Result<Lowered, String> {
    if source.match_name != MATCH_NAME {
        return Err(format!("{} is not {MATCH_NAME}", source.match_name));
    }
    // Profile defaults may stand in only for controls that are known to be
    // absent, never for declarations the decoder could not read.
    if source.declarations == Declarations::Unreadable {
        return Err("its own parameter declaration table is duplicated or malformed, so its controls cannot be checked against the supported profile".into());
    }
    for parameter in &source.parameters {
        let control = find(parameter).ok_or_else(|| {
            format!(
                "{}: control outside the supported Keylight 906 profile",
                parameter.match_name
            )
        })?;
        match &parameter.declared_kind {
            Err(error) => {
                return Err(format!(
                    "{}: unreadable declaration ({error})",
                    context(control)
                ));
            }
            Ok(Some(kind)) if *kind != control.kind => {
                return Err(format!(
                    "{}: declared parameter type {kind} is incompatible with the supported type {}",
                    context(control),
                    control.kind
                ));
            }
            Ok(_) => {}
        }
    }
    let mut defaulted = Vec::new();
    let (mut screen, mut gain, mut balance, mut clip_white) = (None, None, None, None);
    for control in &CONTROLS {
        let parameter = source
            .parameters
            .iter()
            .find(|parameter| suffix(parameter) == Some(control.suffix));
        match control.rule {
            Rule::Inert => {}
            Rule::Number(expected) => {
                let value = number(control, parameter, expected, &mut defaulted)?;
                if (value - expected).abs() > TOLERANCE {
                    return Err(unsupported(control, value, expected));
                }
            }
            Rule::Colour(expected) => {
                let value = colour(control, parameter, expected, &mut defaulted)?;
                if value
                    .iter()
                    .zip(expected)
                    .any(|(a, b)| (a - b).abs() > TOLERANCE)
                {
                    return Err(unsupported(control, value, expected));
                }
            }
            Rule::NoPath => match parameter {
                None => defaulted.push(control.label),
                Some(parameter) if parameter.unset_path => {}
                Some(_) => {
                    return Err(format!(
                        "{}: an explicit or undecodable mask selector is outside the supported profile",
                        context(control)
                    ));
                }
            },
            Rule::ScreenColour(default) => {
                screen = Some(colour(control, parameter, default, &mut defaulted)?);
            }
            // Ranges are the declared valid ranges of the native sliders.
            Rule::ScreenGain(default) => {
                let value = number(control, parameter, default, &mut defaulted)?;
                gain = Some(within(control, value, 0.0, 5000.0)?);
            }
            Rule::ScreenBalance(default) => {
                let value = number(control, parameter, default, &mut defaulted)?;
                balance = Some(within(control, value, 0.0, 100.0)?);
            }
            Rule::ClipWhite(default) => {
                let value = number(control, parameter, default, &mut defaulted)?;
                if value <= 0.0 {
                    return Err(format!(
                        "{} = {value} does not exceed the supported Clip Black 0",
                        context(control)
                    ));
                }
                clip_white = Some(within(control, value, 0.0, 100.0)?);
            }
        }
    }
    let settings = Settings {
        screen: screen.expect("the profile declares Screen Colour"),
        gain: gain.expect("the profile declares Screen Gain"),
        balance: balance.expect("the profile declares Screen Balance"),
        clip_white: clip_white.expect("the profile declares Clip White"),
    };
    let Some(screen_difference) = screen_difference(settings.screen, settings.balance / 100.0)
    else {
        return Err(format!(
            "Screen Colour {:?} has no unique dominant channel to key",
            settings.screen
        ));
    };
    if screen_difference <= MIN_SCREEN_DIFFERENCE {
        return Err(format!(
            "Screen Colour {:?} exceeds its other channels by only {screen_difference}",
            settings.screen
        ));
    }
    Ok(Lowered {
        effect: shader(source.index, &settings),
        defaulted,
    })
}

fn suffix(parameter: &DecodedParameter) -> Option<&str> {
    parameter
        .match_name
        .strip_prefix(MATCH_NAME)?
        .strip_prefix('-')
}

fn find(parameter: &DecodedParameter) -> Option<&'static Control> {
    let suffix = suffix(parameter)?;
    CONTROLS.iter().find(|control| control.suffix == suffix)
}

fn context(control: &Control) -> String {
    format!("{} ({})", control.label, control.suffix)
}

fn unsupported(
    control: &Control,
    value: impl std::fmt::Debug,
    expected: impl std::fmt::Debug,
) -> String {
    format!(
        "{} = {value:?}; only the plugin default {expected:?} is supported",
        context(control)
    )
}

/// The instance's static record, or `None` when it stores no such control.
fn stored<'a>(
    control: &Control,
    parameter: Option<&'a DecodedParameter>,
) -> Result<Option<&'a NumericProperty>, String> {
    let Some(parameter) = parameter else {
        return Ok(None);
    };
    let numeric = parameter
        .numeric
        .as_ref()
        .map_err(|error| format!("{}: {error}", context(control)))?;
    if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
        return Err(format!(
            "{}: animated or expression-driven; only static Keylight is supported",
            context(control)
        ));
    }
    Ok(Some(numeric))
}

/// A static number, or the plugin default for an absent control.
fn number(
    control: &Control,
    parameter: Option<&DecodedParameter>,
    default: f64,
    defaulted: &mut Vec<&'static str>,
) -> Result<f64, String> {
    let Some(numeric) = stored(control, parameter)? else {
        defaulted.push(control.label);
        return Ok(default);
    };
    match numeric.values.as_slice() {
        [value] if value.is_finite() => Ok(*value),
        values => Err(format!("{}: malformed number {values:?}", context(control))),
    }
}

/// A static normalized RGB colour, or the plugin default for an absent control.
fn colour(
    control: &Control,
    parameter: Option<&DecodedParameter>,
    default: [f64; 3],
    defaulted: &mut Vec<&'static str>,
) -> Result<[f64; 3], String> {
    let Some(numeric) = stored(control, parameter)? else {
        defaulted.push(control.label);
        return Ok(default);
    };
    match numeric.values.as_slice() {
        [r, g, b, a]
            if numeric.value_kind == NumericValueKind::Color
                && [r, g, b, a]
                    .iter()
                    .all(|value| (0.0..=1.0).contains(*value)) =>
        {
            Ok([*r, *g, *b])
        }
        values => Err(format!("{}: malformed colour {values:?}", context(control))),
    }
}

fn within(control: &Control, value: f64, min: f64, max: f64) -> Result<f64, String> {
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(format!(
            "{} = {value} is outside its declared range {min}..={max}",
            context(control)
        ))
    }
}

/// `D(S)` for screen colour `S` and balance `b`: its strictly dominant channel
/// minus `b` times the smaller and `1 - b` times the larger other channel.
fn screen_difference(screen: [f64; 3], balance: f64) -> Option<f64> {
    let primary = (0..3)
        .find(|&channel| (0..3).all(|other| other == channel || screen[channel] > screen[other]))?;
    let [a, b] = [(primary + 1) % 3, (primary + 2) % 3].map(|channel| screen[channel]);
    Some(screen[primary] - (balance * a.min(b) + (1.0 - balance) * a.max(b)))
}

fn shader(index: usize, settings: &Settings) -> LayerEffect {
    LayerEffect::CustomShader {
        name: format!("Keylight (AE effect {index})"),
        description: DESCRIPTION.into(),
        wgsl: WGSL.into(),
        params: vec![
            param(
                "screen.colorR",
                "Keylight Screen Colour red, normalized display-referred value.",
                0.0,
                1.0,
                settings.screen[0],
            ),
            param(
                "screen.colorG",
                "Keylight Screen Colour green, normalized display-referred value.",
                0.0,
                1.0,
                settings.screen[1],
            ),
            param(
                "screen.colorB",
                "Keylight Screen Colour blue, normalized display-referred value.",
                0.0,
                1.0,
                settings.screen[2],
            ),
            // The native slider shows 0..200 within a valid 0..5000 range.
            param(
                "screenGain",
                "Keylight Screen Gain, percent: scales the dominant-channel difference; above 100 removes more screen.",
                0.0,
                settings.gain.max(200.0),
                settings.gain,
            ),
            param(
                "screenBalance",
                "Keylight Screen Balance, percent: 100 compares the dominant channel with the smaller other Screen Colour channel, 0 with the larger, 50 with their average.",
                0.0,
                100.0,
                settings.balance,
            ),
            param(
                "clipWhite",
                "Keylight Clip White, percent: raw matte coverage at or above this becomes opaque; Clip Black is 0.",
                0.0,
                100.0,
                settings.clip_white,
            ),
        ],
        texture_inputs: Vec::new(),
    }
}

fn param(name: &str, description: &str, min: f64, max: f64, default: f64) -> EffectParam {
    EffectParam {
        name: name.into(),
        description: description.into(),
        min,
        max,
        default,
    }
}

#[cfg(test)]
mod tests {
    use super::{CONTROLS, screen_difference};

    #[test]
    fn profile_lists_each_native_control_once_in_order() {
        let suffixes: Vec<_> = CONTROLS.iter().map(|control| control.suffix).collect();
        let expected: Vec<_> = (1..=79).map(|index| format!("{index:04}")).collect();
        assert_eq!(suffixes, expected);
    }

    #[test]
    fn screen_difference_uses_the_screen_channel_order_for_balance() {
        // Green screen, red smaller than blue: balance 1 compares with red.
        assert_eq!(screen_difference([0.1, 0.8, 0.3], 1.0), Some(0.8 - 0.1));
        assert_eq!(screen_difference([0.1, 0.8, 0.3], 0.0), Some(0.8 - 0.3));
        // The same rule for red and blue screens.
        assert_eq!(screen_difference([0.9, 0.2, 0.4], 1.0), Some(0.9 - 0.2));
        assert_eq!(screen_difference([0.3, 0.1, 0.7], 0.0), Some(0.7 - 0.3));
        for tied in [[0.5, 0.5, 0.1], [0.4, 0.4, 0.4], [0.2, 0.6, 0.6]] {
            assert_eq!(screen_difference(tied, 0.5), None, "{tied:?}");
        }
    }
}
