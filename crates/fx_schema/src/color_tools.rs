//! Static version-one hue curves, channel mixing and selective color.
//!
//! These are Jerboa grades, not vendor-parity conversion contracts.

use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

fn version_one<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let version = u8::deserialize(d)?;
    if version != 1 {
        return Err(serde::de::Error::custom("semanticVersion must be 1"));
    }
    Ok(version)
}

fn bounded<'de, D: Deserializer<'de>>(d: D, bound: f64) -> Result<f64, D::Error> {
    let value = f64::deserialize(d)?;
    if !value.is_finite() || !(-bound..=bound).contains(&value) {
        return Err(serde::de::Error::custom(format!(
            "value must be finite and in -{bound}..={bound}"
        )));
    }
    Ok(value)
}
fn unit<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    bounded(d, 1.0)
}
fn weight<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    bounded(d, 2.0)
}

/// A normalized hue-curve control; y=0.5 is neutral for every curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "project_types.d.ts")]
pub struct HueCurvePoint {
    /// Normalized input coordinate.
    pub x: f64,
    /// Normalized adjustment coordinate.
    pub y: f64,
}

fn curve<'de, D: Deserializer<'de>>(
    d: D,
    periodic: bool,
) -> Result<Option<Vec<HueCurvePoint>>, D::Error> {
    let points = Option::<Vec<HueCurvePoint>>::deserialize(d)?;
    if let Some(points) = &points {
        if !(2..=16).contains(&points.len()) {
            return Err(serde::de::Error::custom("hue curve requires 2..=16 points"));
        }
        for (index, p) in points.iter().enumerate() {
            if !p.x.is_finite()
                || !p.y.is_finite()
                || !(0.0..=1.0).contains(&p.x)
                || !(0.0..=1.0).contains(&p.y)
            {
                return Err(serde::de::Error::custom(
                    "hue curve coordinates must be finite in 0..=1",
                ));
            }
            // The shader divides f32 coordinates; reject collapsed segments too.
            if index > 0 && p.x as f32 <= points[index - 1].x as f32 {
                return Err(serde::de::Error::custom(
                    "hue curve x must strictly increase at GPU precision",
                ));
            }
        }
        let first = points[0];
        let last = points[points.len() - 1];
        if periodic && first.x == 0.0 && last.x == 1.0 && first.y != last.y {
            return Err(serde::de::Error::custom(
                "periodic hue curve endpoints must have matching y",
            ));
        }
        if periodic && (first.x != 0.0 || last.x != 1.0) && (first.x as f32 + 1.0) <= last.x as f32
        {
            return Err(serde::de::Error::custom(
                "periodic hue curve wrap must have positive width at GPU precision",
            ));
        }
    }
    Ok(points)
}
fn periodic_curve<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<HueCurvePoint>>, D::Error> {
    curve(d, true)
}
fn linear_curve<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<HueCurvePoint>>, D::Error> {
    curve(d, false)
}

/// Piecewise-linear hue, saturation and Rec.709-luminance adjustments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "project_types.d.ts")]
pub struct HueCurves {
    /// Fixed version-one math; parameters are static.
    #[serde(deserialize_with = "version_one")]
    #[ts(type = "1")]
    pub semantic_version: u8,
    /// Hue shift, in turns y-0.5, keyed on the original hue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "periodic_curve"
    )]
    #[ts(optional)]
    pub hue_vs_hue: Option<Vec<HueCurvePoint>>,
    /// Saturation gain 2y keyed on the original hue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "periodic_curve"
    )]
    #[ts(optional)]
    pub hue_vs_saturation: Option<Vec<HueCurvePoint>>,
    /// Rec.709 luminance offset y-0.5 keyed on the original hue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "periodic_curve"
    )]
    #[ts(optional)]
    pub hue_vs_luminance: Option<Vec<HueCurvePoint>>,
    /// Saturation gain 2y keyed on original Rec.709 luminance.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "linear_curve"
    )]
    #[ts(optional)]
    pub luminance_vs_saturation: Option<Vec<HueCurvePoint>>,
    /// Saturation gain 2y keyed on original HSV saturation.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "linear_curve"
    )]
    #[ts(optional)]
    pub saturation_vs_saturation: Option<Vec<HueCurvePoint>>,
}
impl HueCurves {
    /// Neutral grades omit the render pass, preserving pixels exactly.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        [
            &self.hue_vs_hue,
            &self.hue_vs_saturation,
            &self.hue_vs_luminance,
            &self.luminance_vs_saturation,
            &self.saturation_vs_saturation,
        ]
        .into_iter()
        .all(|curve| curve.as_ref().is_none_or(|p| p.iter().all(|p| p.y == 0.5)))
    }
}

/// One channel mixer's output row applied to straight input RGB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "project_types.d.ts")]
pub struct ChannelMixerRow {
    /// Input red coefficient, -2..=2.
    #[serde(deserialize_with = "weight")]
    pub red: f64,
    /// Input green coefficient, -2..=2.
    #[serde(deserialize_with = "weight")]
    pub green: f64,
    /// Input blue coefficient, -2..=2.
    #[serde(deserialize_with = "weight")]
    pub blue: f64,
    /// Additive output constant, -1..=1.
    #[serde(deserialize_with = "unit")]
    pub constant: f64,
}
/// Static RGB matrix plus constants, clamped after mixing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "project_types.d.ts")]
pub struct ChannelMixer {
    /// Fixed version-one math; parameters are static.
    #[serde(deserialize_with = "version_one")]
    #[ts(type = "1")]
    pub semantic_version: u8,
    /// Red output row; also the common row in monochrome mode.
    pub red: ChannelMixerRow,
    /// Green output row.
    pub green: ChannelMixerRow,
    /// Blue output row.
    pub blue: ChannelMixerRow,
    /// Use the red row for every output, like AE Channel Mixer Monochrome.
    pub monochrome: bool,
}
impl ChannelMixer {
    /// Neutral grades omit the render pass, preserving pixels exactly.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        !self.monochrome
            && [self.red, self.green, self.blue]
                .into_iter()
                .enumerate()
                .all(|(i, row)| {
                    [row.red, row.green, row.blue]
                        .into_iter()
                        .enumerate()
                        .all(|(j, v)| v == if i == j { 1.0 } else { 0.0 })
                        && row.constant == 0.0
                })
    }
}
/// Ink adjustments per selective-color range; zero is neutral.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "project_types.d.ts")]
pub struct SelectiveColorAdjustment {
    /// Red-channel subtractive adjustment, -1..=1.
    #[serde(deserialize_with = "unit")]
    pub cyan: f64,
    /// Green-channel subtractive adjustment, -1..=1.
    #[serde(deserialize_with = "unit")]
    pub magenta: f64,
    /// Blue-channel subtractive adjustment, -1..=1.
    #[serde(deserialize_with = "unit")]
    pub yellow: f64,
    /// Common subtractive adjustment, -1..=1.
    #[serde(deserialize_with = "unit")]
    pub black: f64,
}
/// Jerboa's relative or absolute subtractive-ink approximation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum SelectiveColorMethod {
    /// Ink changes scale by value when adding ink, headroom when removing it.
    Relative,
    /// Ink changes are independent of channel value.
    Absolute,
}
/// Nine-range selective color, approximately related to AE/Photoshop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "project_types.d.ts")]
pub struct SelectiveColor {
    /// Fixed version-one math; parameters are static.
    #[serde(deserialize_with = "version_one")]
    #[ts(type = "1")]
    pub semantic_version: u8,
    /// Relative versus absolute subtractive ink changes.
    pub method: SelectiveColorMethod,
    /// Red-dominant pixels.
    pub reds: SelectiveColorAdjustment,
    /// Blue-minimum pixels.
    pub yellows: SelectiveColorAdjustment,
    /// Green-dominant pixels.
    pub greens: SelectiveColorAdjustment,
    /// Red-minimum pixels.
    pub cyans: SelectiveColorAdjustment,
    /// Blue-dominant pixels.
    pub blues: SelectiveColorAdjustment,
    /// Green-minimum pixels.
    pub magentas: SelectiveColorAdjustment,
    /// Bright pixels.
    pub whites: SelectiveColorAdjustment,
    /// Neutral/midtone pixels.
    pub neutrals: SelectiveColorAdjustment,
    /// Dark pixels.
    pub blacks: SelectiveColorAdjustment,
}
impl SelectiveColor {
    /// Range order shared by shader generation and reference evaluation.
    #[must_use]
    pub fn adjustments(&self) -> [SelectiveColorAdjustment; 9] {
        [
            self.reds,
            self.yellows,
            self.greens,
            self.cyans,
            self.blues,
            self.magentas,
            self.whites,
            self.neutrals,
            self.blacks,
        ]
    }
    /// Neutral grades omit the render pass, preserving pixels exactly.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.adjustments()
            .iter()
            .all(|adjustment| *adjustment == SelectiveColorAdjustment::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hue_curves_validate_counts_coordinates_order_and_seam() {
        for points in [
            json!([]),
            json!([{"x":0,"y":0.5}]),
            json!([{"x":0,"y":0.5},{"x":0,"y":0.5}]),
            json!([{"x":0,"y":-0.1},{"x":1,"y":0.5}]),
            json!([{"x":0,"y":0.2},{"x":1,"y":0.8}]),
        ] {
            assert!(serde_json::from_value::<HueCurves>(
                json!({"semanticVersion":1,"hueVsHue":points})
            )
            .is_err());
        }
        assert!(
            serde_json::from_value::<HueCurves>(json!({"semanticVersion":1}))
                .unwrap()
                .is_identity()
        );
    }
    #[test]
    fn mixer_and_ink_bounds_are_strict() {
        for (red, constant) in [(2.1, 0.0), (0.0, -1.1)] {
            assert!(serde_json::from_value::<ChannelMixerRow>(
                json!({"red":red,"green":0,"blue":0,"constant":constant})
            )
            .is_err());
        }
        assert!(serde_json::from_value::<SelectiveColorAdjustment>(
            json!({"cyan":1.1,"magenta":0,"yellow":0,"black":0})
        )
        .is_err());
        assert!(
            version_one(serde::de::value::U8Deserializer::<serde::de::value::Error>::new(2))
                .is_err()
        );
    }
}
