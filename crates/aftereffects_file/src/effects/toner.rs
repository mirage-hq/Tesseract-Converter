//! Bounded static CC Toner lowering through existing editable FX controls.
//! The native local `parT` in the provenance fixture establishes sparse defaults.

use fx_schema::{
    ColorCurve, ColorCurvePoint, ColorCurves, ColorCurvesSemanticVersion, LayerEffect,
};

use super::native::DecodedEffect;
use crate::properties::{NumericProperty, NumericValueKind};

const PARAMETERS: [&str; 7] = [
    "CC Toner-0001",
    "CC Toner-0002",
    "CC Toner-0003",
    "CC Toner-0004",
    "CC Toner-0005",
    "CC Toner-0006",
    "CC Toner-0007",
];

pub(super) fn parameter_names(effect: &str) -> &'static [&'static str] {
    if effect == "CC Toner" {
        &PARAMETERS
    } else {
        &[]
    }
}

/// Defaults apply only to absent native controls, never malformed explicit values.
pub(super) fn default_parameter(effect: &str, name: &str) -> Option<NumericProperty> {
    if effect != "CC Toner" {
        return None;
    }
    let (values, value_kind) = match name {
        "CC Toner-0001" => (vec![1.0; 4], NumericValueKind::Color),
        "CC Toner-0002" => (
            vec![128.0 / 255.0, 100.0 / 255.0, 70.0 / 255.0, 1.0],
            NumericValueKind::Color,
        ),
        "CC Toner-0003" => (vec![0.0, 0.0, 0.0, 1.0], NumericValueKind::Color),
        "CC Toner-0004" => (vec![0.0], NumericValueKind::Continuous),
        "CC Toner-0005" => (vec![2.0], NumericValueKind::Integer),
        "CC Toner-0006" => (
            vec![192.0 / 255.0, 170.0 / 255.0, 120.0 / 255.0, 1.0],
            NumericValueKind::Color,
        ),
        "CC Toner-0007" => (
            vec![64.0 / 255.0, 50.0 / 255.0, 10.0 / 255.0, 1.0],
            NumericValueKind::Color,
        ),
        _ => return None,
    };
    Some(NumericProperty {
        values,
        value_kind,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
    })
}

pub(crate) struct LoweredToner {
    pub effects: [LayerEffect; 2],
    /// Fraction of the original source mixed into the toned output.
    pub original: f64,
}

fn control<'a>(source: &'a DecodedEffect, name: &str) -> Result<&'a NumericProperty, String> {
    let mut matches = source
        .parameters
        .iter()
        .filter(|parameter| parameter.match_name == name);
    let parameter = matches
        .next()
        .ok_or_else(|| format!("{name}: control missing"))?;
    if matches.next().is_some() {
        return Err(format!("{name}: duplicate control"));
    }
    let numeric = parameter
        .numeric
        .as_ref()
        .map_err(|error| format!("{name}: {error}"))?;
    if numeric.animated || numeric.expression_enabled || !numeric.keyframes.is_empty() {
        return Err(format!(
            "{name}: animated/enabled expression control is outside static Toner mapping"
        ));
    }
    Ok(numeric)
}

fn scalar(source: &DecodedEffect, name: &str) -> Result<f64, String> {
    let numeric = control(source, name)?;
    match numeric.values.as_slice() {
        [value] if value.is_finite() => Ok(*value),
        _ => Err(format!("{name}: finite scalar required")),
    }
}

fn color(source: &DecodedEffect, name: &str) -> Result<[f64; 3], String> {
    let numeric = control(source, name)?;
    match numeric.values.as_slice() {
        [r, g, b, a]
            if numeric.value_kind == NumericValueKind::Color
                && [r, g, b, a]
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(*value))
                && *a == 1.0 =>
        {
            Ok([*r, *g, *b])
        }
        _ => Err(format!("{name}: normalized opaque RGB color required")),
    }
}

pub(crate) fn lower(source: &DecodedEffect) -> Result<LoweredToner, String> {
    let mode = scalar(source, "CC Toner-0005")?;
    let names: &[&str] = match mode {
        2.0 => &["CC Toner-0003", "CC Toner-0002", "CC Toner-0001"],
        3.0 => &[
            "CC Toner-0003",
            "CC Toner-0007",
            "CC Toner-0002",
            "CC Toner-0006",
            "CC Toner-0001",
        ],
        _ => {
            return Err(format!(
                "Tones {mode}: only native Tritone (2) and Pentone (3) are mapped"
            ));
        }
    };
    let original = scalar(source, "CC Toner-0004")?;
    if !(0.0..=1.0).contains(&original) {
        return Err("Blend with Original: expected normalized 0..1".into());
    }
    let colors = names
        .iter()
        .map(|name| color(source, name))
        .collect::<Result<Vec<_>, _>>()?;
    let curve = |channel| {
        ColorCurve::new(
            colors
                .iter()
                .enumerate()
                .map(|(index, color)| {
                    ColorCurvePoint::new(index as f64 / (colors.len() - 1) as f64, color[channel])
                })
                .collect(),
        )
        .map_err(|error| error.to_string())
    };
    Ok(LoweredToner {
        original,
        effects: [
            LayerEffect::TintTritone {
                black_r: Some(0.0),
                black_g: Some(0.0),
                black_b: Some(0.0),
                white_r: Some(1.0),
                white_g: Some(1.0),
                white_b: Some(1.0),
                amount: Some(100.0),
            },
            LayerEffect::ColorCurves {
                semantic_version: ColorCurvesSemanticVersion::V1,
                curves: ColorCurves {
                    master: ColorCurve::identity(),
                    red: curve(0)?,
                    green: curve(1)?,
                    blue: curve(2)?,
                },
            },
        ],
    })
}
