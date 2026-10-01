//! Non-scalar/native-popup effect controls. Parent conversion paths call these
//! after applying affine mapping rows; neither path evaluates AE expressions.
use fx_schema::LayerEffect;
use serde_json::Value;

use super::native::DecodedEffect;
use crate::writer::effects::NativeEffect;

fn source<'a>(effect: &'a DecodedEffect, suffix: &str) -> Option<(&'a [f64], bool)> {
    let parameter = effect
        .parameters
        .iter()
        .find(|p| p.match_name.ends_with(suffix))?;
    let numeric = parameter.numeric.as_ref().ok()?;
    Some((
        numeric
            .keyframes
            .first()
            .map(|key| key.values.as_slice())
            .unwrap_or(&numeric.values),
        numeric.animated || numeric.expression_enabled,
    ))
}

fn scalar(effect: &DecodedEffect, suffix: &str, warnings: &mut Vec<String>) -> Option<f64> {
    let (values, animated) = source(effect, suffix)?;
    if animated {
        warnings.push(format!("{suffix}: only the initial value is representable by this static mapping; animation/expression omitted"));
    }
    values.first().copied().filter(|v| v.is_finite())
}

#[derive(Default)]
pub(crate) struct ImportReport {
    pub(crate) warnings: Vec<String>,
    consumed_suffixes: Vec<&'static str>,
    pub(crate) shadow_distance_scale: Option<[f64; 2]>,
}

impl ImportReport {
    fn consume(&mut self, suffix: &'static str) {
        self.consumed_suffixes.push(suffix);
    }

    pub(crate) fn consumed(&self, match_name: &str) -> bool {
        self.consumed_suffixes
            .iter()
            .any(|suffix| match_name.ends_with(suffix))
    }
}

// A fixed angle makes polar distance an affine map into the existing Vector2
// offset target; changing the angle would require a nonlinear coupled track.
fn shadow_distance_scale(native: &DecodedEffect) -> Option<[f64; 2]> {
    let parameter = |name: &str| {
        native
            .parameters
            .iter()
            .find(|p| p.match_name == name)?
            .numeric
            .as_ref()
            .ok()
    };
    let direction = parameter("ADBE Drop Shadow-0003")?;
    let distance = parameter("ADBE Drop Shadow-0004")?;
    if direction.animated
        || !direction.keyframes.is_empty()
        || direction.expression_present
        || direction.expression_enabled
        || direction.dimensions_separated
        || distance.expression_present
        || distance.expression_enabled
        || distance.dimensions_separated
        || (distance.animated && distance.keyframes.is_empty())
        || distance
            .keyframes
            .iter()
            .any(|key| key.values.len() != 1 || !key.values[0].is_finite())
    {
        return None;
    }
    let [angle] = direction.values.as_slice() else {
        return None;
    };
    if !angle.is_finite() {
        return None;
    }
    let initial = distance
        .keyframes
        .first()
        .map(|key| key.values.as_slice())
        .unwrap_or(&distance.values);
    let [initial] = initial else {
        return None;
    };
    if !initial.is_finite() {
        return None;
    }
    let theta = angle.to_radians();
    Some([theta.sin(), -theta.cos()])
}

fn set_native(effect: &mut NativeEffect, suffix: &str, component: usize, value: f64) {
    if let Some(slot) = effect
        .properties
        .iter_mut()
        .find(|p| p.match_name.ends_with(suffix))
        .and_then(|p| p.values.get_mut(component))
    {
        *slot = value;
    }
}

fn value(payload: &Value, name: &str) -> Option<f64> {
    payload.get(name)?.as_f64().filter(|v| v.is_finite())
}

/// Invert's other channels require different color-space/alpha operations.
/// Reject the occurrence before assigning IDs or importing any numeric tracks.
pub(crate) fn validate_import(native: &DecodedEffect) -> Result<(), &'static str> {
    if native.match_name != "ADBE Invert" {
        return Ok(());
    }
    match source(native, "-0001") {
        Some(([1.0], false)) => {}
        _ => return Err("only a static RGB Channel is representable"),
    }
    match source(native, "-0002") {
        Some(([blend], false)) if blend.is_finite() && (0.0..=100.0).contains(blend) => Ok(()),
        _ => Err("Blend With Original must be a static finite percentage in 0..100"),
    }
}

/// Called after affine fields, before deserializing the edited FX payload.
/// Non-representable modes retain convertible siblings with explicit warnings.
pub(crate) fn import(native: &DecodedEffect, size: [f64; 2], payload: &mut Value) -> ImportReport {
    let mut report = ImportReport::default();
    match native.match_name.as_str() {
        "ADBE Invert" => report.consume("-0001"),
        "ADBE Fill" => {
            for (suffix, label) in [
                ("-0003", "Horizontal Feather"),
                ("-0004", "Vertical Feather"),
                ("-0006", "Invert"),
            ] {
                if let Some((values, animated)) = source(native, suffix) {
                    report.consume(suffix);
                    let nondefault = values
                        .first()
                        .is_some_and(|value| value.abs() > f64::EPSILON);
                    if animated || nondefault {
                        report.warnings.push(format!(
                            "{label} ({suffix}) cannot be represented by the Tint-based Fill approximation; control omitted"
                        ));
                    }
                }
            }
        }
        "ADBE Drop Shadow" => {
            if let Some((color, animated)) = source(native, "-0001") {
                if color.len() >= 4 {
                    payload["color"] = serde_json::json!([color[0], color[1], color[2], color[3]]);
                    report.consume("-0001");
                    if (color[3] - 1.0).abs() > f64::EPSILON {
                        report.warnings.push("Shadow Color alpha is not independently preserved; native Opacity supplies FX shadow alpha (combined-alpha fidelity unverified)".into());
                    }
                }
                if animated {
                    report.warnings.push("Shadow Color animation requires a typed color target; retain initial color only".into());
                }
            }
            if let Some(opacity) = scalar(native, "-0002", &mut report.warnings) {
                payload["color"][3] = serde_json::json!(opacity / 255.0);
                report.consume("-0002");
            }
            let direction = scalar(native, "-0003", &mut report.warnings);
            report.shadow_distance_scale = shadow_distance_scale(native);
            let distance = if report.shadow_distance_scale.is_some() {
                source(native, "-0004").and_then(|(values, _)| values.first().copied())
            } else {
                scalar(native, "-0004", &mut report.warnings)
            };
            if let (Some(angle), Some(distance)) = (direction, distance) {
                let theta = angle.to_radians();
                payload["offset"] =
                    serde_json::json!([distance * theta.sin(), -distance * theta.cos()]);
                report.consume("-0003");
                report.consume("-0004");
            }
            if let Some(shadow_only) = scalar(native, "-0006", &mut report.warnings) {
                report.consume("-0006");
                if shadow_only != 0.0 {
                    report.warnings.push(
                        "Shadow Only cannot be represented; normal shadow compositing retained"
                            .into(),
                    );
                }
            }
        }
        "CS Vignette" => {
            if let Some((center, animated)) = source(native, "-0003") {
                report.consume("-0003");
                let centered = center.len() >= 2
                    && (center[0] - size[0] * 0.5).abs() <= f64::EPSILON
                    && (center[1] - size[1] * 0.5).abs() <= f64::EPSILON;
                if animated || !centered {
                    report.warnings.push("-0003 Center animation/noncenter value cannot be represented by the centered FX vignette; centered replacement retained".into());
                }
            }
            if let Some((pin_highlights, animated)) = source(native, "-0005") {
                report.consume("-0005");
                let nonzero = pin_highlights
                    .first()
                    .is_some_and(|value| value.abs() > f64::EPSILON);
                if animated || nonzero {
                    report.warnings.push("-0005 Pin Highlights animation/nonzero value has no FX vignette control; highlight pinning omitted".into());
                }
            }
        }
        "ADBE Shift Channels" => {
            if let Some(route) = scalar(native, "-0001", &mut report.warnings) {
                report.consume("-0001");
                if route != 1.0 {
                    report
                        .warnings
                        .push("Take Alpha From has no FX channel route; alpha retained".into());
                }
            }
            for (suffix, field, own) in [
                ("-0002", "takeRedFrom", 2.0),
                ("-0003", "takeGreenFrom", 3.0),
                ("-0004", "takeBlueFrom", 4.0),
            ] {
                if let Some(route) = scalar(native, suffix, &mut report.warnings) {
                    report.consume(suffix);
                    if route == 9.0 {
                        payload[field] = serde_json::json!("fullOn");
                    } else if route == 10.0 {
                        payload[field] = serde_json::json!("fullOff");
                    } else if route != own {
                        report.warnings.push(format!("{suffix}: source route {route} cannot render with current FX backend; own channel retained"));
                    }
                }
            }
        }
        "ADBE OFMotionBlur" => {
            if let Some(mode) = scalar(native, "-0001", &mut report.warnings) {
                report.consume("-0001");
                payload["shutterControl"] =
                    serde_json::json!(if mode == 1.0 { "manual" } else { "automatic" });
                if mode != 1.0 && mode != 2.0 {
                    report
                        .warnings
                        .push("Unknown Shutter Control mode approximated as automatic".into());
                }
            }
        }
        "ADBE AIF Perlin Noise 3D" => {
            if let Some(mode) = scalar(native, "-0002", &mut report.warnings) {
                report.consume("-0002");
                if let Some(name) = ["block", "linear", "softLinear", "spline"]
                    .get((mode as usize).saturating_sub(1))
                    .filter(|_| mode.fract() == 0.0 && (1.0..=4.0).contains(&mode))
                {
                    payload["noiseType"] = serde_json::json!(name);
                } else {
                    report.warnings.push(format!(
                        "Noise Type {mode} unsupported; softLinear retained"
                    ));
                }
            }
            if let Some(mode) = scalar(native, "-0001", &mut report.warnings) {
                report.consume("-0001");
                let kind = [
                    (1.0, "basic"),
                    (3.0, "turbulentSmooth"),
                    (4.0, "turbulentBasic"),
                    (5.0, "turbulentSharp"),
                    (11.0, "max"),
                    (14.0, "rocky"),
                    (15.0, "cloudy"),
                    (18.0, "strings"),
                ]
                .iter()
                .find(|(id, _)| *id == mode);
                if let Some((_, name)) = kind {
                    payload["fractalType"] = serde_json::json!(name);
                } else {
                    report
                        .warnings
                        .push(format!("Fractal Type {mode} unsupported; basic retained"));
                }
            }
            if size[0] <= 0.0 {
                report
                    .warnings
                    .push("Noise Offset Turbulence requires positive content width".into());
            }
            if let Some(mode) = scalar(native, "-0026", &mut report.warnings) {
                report.consume("-0026");
                if mode != 2.0 {
                    report.warnings.push(
                        "Noise Blending Mode cannot be expressed by scalar FX blend; default retained"
                            .into(),
                    );
                }
            }
        }
        "ADBE Ripple" | "ADBE Wave Warp" => {
            let ripple = native.match_name == "ADBE Ripple";
            let suffix = if ripple { "-0005" } else { "-0003" };
            if let Some(width) = scalar(native, suffix, &mut report.warnings) {
                report.consume(suffix);
                if width > 0.0 && size[0] > 0.0 {
                    let mapped = if ripple {
                        std::f64::consts::TAU * size[0] / width
                    } else {
                        size[0] / width
                    };
                    payload[if ripple { "frequency" } else { "waveWidth" }] =
                        serde_json::json!(mapped);
                } else {
                    report.warnings.push(format!("{suffix}: positive wavelength and content width required; FX default retained"));
                }
            }
        }
        _ => {}
    }
    report
}

/// Called after affine fields have populated the native property list.
/// Static polar/reciprocal mappings cannot represent continuous key animation.
pub(crate) fn export(
    effect: &LayerEffect,
    size: [f64; 2],
    native: &mut NativeEffect,
) -> Vec<String> {
    let Ok(payload) = serde_json::to_value(effect) else {
        return vec!["FX effect cannot be serialized for native controls".into()];
    };
    let mut warnings = Vec::new();
    match native.match_name.as_str() {
        "ADBE Glo2" => {
            // FX composites its glow premultiplied-over the source. The native
            // catalog's Add operation washes out bright, colored source pixels.
            set_native(native, "-0006", 0, 2.0);
            warnings.push("Glow Operation set to Normal instead of native Add to approximate FX source compositing; halo color and brightness may still differ".into());
        }
        "ADBE Drop Shadow" => {
            native.enabled &= payload
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if payload
                .get("blendMode")
                .and_then(Value::as_str)
                .is_some_and(|mode| mode != "normal")
            {
                warnings.push(
                    "Shadow blendMode approximated with native Drop Shadow's normal compositing"
                        .into(),
                );
            }
            if let Some(color) = payload.get("color").and_then(Value::as_array) {
                for (index, channel) in color.iter().take(3).enumerate() {
                    if let Some(v) = channel.as_f64() {
                        set_native(native, "-0001", index, v);
                    }
                }
                if let Some(alpha) = color.get(3).and_then(Value::as_f64) {
                    set_native(native, "-0002", 0, alpha * 255.0);
                }
            }
            if let Some(offset) = payload.get("offset").and_then(Value::as_array)
                && let (Some(x), Some(y)) = (
                    offset.first().and_then(Value::as_f64),
                    offset.get(1).and_then(Value::as_f64),
                )
            {
                set_native(
                    native,
                    "-0003",
                    0,
                    x.atan2(-y).to_degrees().rem_euclid(360.0),
                );
                set_native(native, "-0004", 0, x.hypot(y));
            }
            if value(&payload, "spreadRadius").is_some_and(|v| v != 0.0) {
                warnings.push(
                    "Shadow spreadRadius omitted; AE Drop Shadow has no separate spread".into(),
                );
            }
        }
        "ADBE Shift Channels" => {
            for (suffix, field, own) in [
                ("-0002", "takeRedFrom", "red"),
                ("-0003", "takeGreenFrom", "green"),
                ("-0004", "takeBlueFrom", "blue"),
            ] {
                match payload.get(field).and_then(Value::as_str).unwrap_or(own) {
                    "fullOn" => set_native(native, suffix, 0, 9.0),
                    "fullOff" => set_native(native, suffix, 0, 10.0),
                    route if route == own => {},
                    route => warnings.push(format!("{field}: cross-channel {route} not rendered by FX; native own route retained")),
                }
            }
        }
        "ADBE OFMotionBlur" => {
            let mode = payload
                .get("shutterControl")
                .and_then(Value::as_str)
                .unwrap_or("automatic");
            set_native(native, "-0001", 0, if mode == "manual" { 1.0 } else { 2.0 });
        }
        "VISINF Grain Implant" => {
            for property in &mut native.properties {
                if !matches!(
                    property.match_name.as_str(),
                    "VISINF Grain Implant-0007"
                        | "VISINF Grain Implant-0008"
                        | "VISINF Grain Implant-0013"
                        | "VISINF Grain Implant-0030"
                        | "VISINF Grain Implant-0130"
                        | "VISINF Grain Implant-0028"
                ) {
                    property.values.fill(0.0);
                    property.animation = None;
                }
            }
            // Add Grain stores untouched instance state as zeroes even though its
            // parameter descriptors and scripting API expose nonzero UI defaults.
            // Keep the independently authored Preview replacement and its guide box;
            // Adobe derives the zero-valued region from the cropped native owner.
            set_native(native, "-0028", 0, 1.0);
        }
        "ADBE AIF Perlin Noise 3D" => {
            let noise = match payload
                .get("noiseType")
                .and_then(Value::as_str)
                .unwrap_or("softLinear")
            {
                "block" => 1.0,
                "linear" => 2.0,
                "spline" => 4.0,
                _ => 3.0,
            };
            set_native(native, "-0002", 0, noise);
            let fractal = match payload
                .get("fractalType")
                .and_then(Value::as_str)
                .unwrap_or("basic")
            {
                "turbulentSmooth" => 3.0,
                "turbulentBasic" => 4.0,
                "turbulentSharp" => 5.0,
                "max" => 11.0,
                "rocky" => 14.0,
                "cloudy" => 15.0,
                "strings" => 18.0,
                _ => 1.0,
            };
            set_native(native, "-0001", 0, fractal);
        }
        "ADBE Ripple" | "ADBE Wave Warp" => {
            let ripple = native.match_name == "ADBE Ripple";
            if let Some(frequency) = value(&payload, if ripple { "frequency" } else { "waveWidth" })
            {
                if frequency > 0.0 && size[0] > 0.0 {
                    set_native(
                        native,
                        if ripple { "-0005" } else { "-0003" },
                        0,
                        size[0] * if ripple { std::f64::consts::TAU } else { 1.0 } / frequency,
                    );
                } else {
                    warnings.push(
                        "Positive FX frequency and layer width required for native wavelength"
                            .into(),
                    );
                }
            }
        }
        _ => {}
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::mapping;
    #[test]
    fn every_mapped_effect_has_deserializable_defaults() {
        for mapped in mapping::mappings() {
            let payload = mapping::default_effect(mapped.fx_type);
            assert!(
                serde_json::from_value::<LayerEffect>(payload).is_ok(),
                "{}",
                mapped.fx_type
            );
        }
    }
    #[test]
    fn fixed_shadow_direction_supports_any_finite_angle_and_rejects_coupled_controls() {
        let controls = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/cosmic-self-mask-controls.rifx"),
            |_| false,
        )
        .unwrap();
        let (effects, _) = super::super::native::read_effects(controls.chunks(), [3840., 2160.]);
        let original = &effects[0];
        let mut generic = original.clone();
        generic
            .parameters
            .iter_mut()
            .find(|p| p.match_name == "ADBE Drop Shadow-0003")
            .unwrap()
            .numeric
            .as_mut()
            .unwrap()
            .values = vec![45.];
        let scale = shadow_distance_scale(&generic).unwrap();
        assert!((scale[0] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!((scale[1] + std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        for case in 0..6 {
            let mut effect = generic.clone();
            let name = if case < 3 {
                "ADBE Drop Shadow-0003"
            } else {
                "ADBE Drop Shadow-0004"
            };
            let numeric = effect
                .parameters
                .iter_mut()
                .find(|p| p.match_name == name)
                .unwrap()
                .numeric
                .as_mut()
                .unwrap();
            match case {
                0 => numeric.animated = true,
                1 => numeric.expression_enabled = true,
                2 => numeric.values = vec![f64::NAN],
                3 => numeric.expression_present = true,
                4 => numeric.keyframes[0].values = vec![1., 2.],
                _ => numeric.keyframes[0].values = vec![f64::INFINITY],
            }
            assert_eq!(shadow_distance_scale(&effect), None, "case {case}");
        }
    }
}
