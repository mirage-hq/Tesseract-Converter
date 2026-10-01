//! Composition settings lowered from the canonical current FX document.

use fx_schema::MotionBlurSettings;

use crate::writer::{AepWriteError, CompositionOptions};

/// Converts current FX motion-blur settings without rounding native-integral
/// fields. Callers pass the result to both root and generated precompositions.
pub(super) fn from_motion_blur(
    settings: MotionBlurSettings,
) -> Result<CompositionOptions, AepWriteError> {
    Ok(CompositionOptions::motion_blur(
        settings.enabled,
        whole_u16(
            settings.shutter_angle.value(),
            "fractional motion-blur shutter angle cannot be represented by AE cdta",
        )?,
        whole_i32(
            settings.shutter_phase,
            "fractional motion-blur shutter phase cannot be represented by AE cdta",
        )?,
        whole_i32(
            settings.samples_per_frame.value(),
            "fractional motion-blur samples per frame cannot be represented by AE cdta",
        )?,
        whole_i32(
            settings.adaptive_sample_limit.value(),
            "fractional motion-blur adaptive sample limit cannot be represented by AE cdta",
        )?,
    ))
}

fn whole_u16(value: f64, error: &'static str) -> Result<u16, AepWriteError> {
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=f64::from(u16::MAX)).contains(&value) {
        return Err(AepWriteError::Invalid(error));
    }
    Ok(value as u16)
}

fn whole_i32(value: f64, error: &'static str) -> Result<i32, AepWriteError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < f64::from(i32::MIN)
        || value > f64::from(i32::MAX)
    {
        return Err(AepWriteError::Invalid(error));
    }
    Ok(value as i32)
}

#[cfg(test)]
mod tests {
    use fx_schema::MotionBlurSettings;

    use crate::writer::CompositionOptions;

    use super::from_motion_blur;

    fn settings(value: serde_json::Value) -> MotionBlurSettings {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn lowers_current_settings_without_changing_integer_values() {
        let options = from_motion_blur(settings(serde_json::json!({
            "enabled": true,
            "shutterAngle": 271,
            "shutterPhase": -137,
            "samplesPerFrame": 23,
            "adaptiveSampleLimit": 191
        })))
        .unwrap();

        assert_eq!(
            options,
            CompositionOptions::motion_blur(true, 271, -137, 23, 191)
        );
    }

    #[test]
    fn rejects_fractional_native_fields_instead_of_rounding() {
        let fractional_angle = settings(serde_json::json!({ "shutterAngle": 180.5 }));
        assert!(from_motion_blur(fractional_angle).is_err());

        let fractional_phase = settings(serde_json::json!({ "shutterPhase": -89.5 }));
        assert!(from_motion_blur(fractional_phase).is_err());
    }
}
