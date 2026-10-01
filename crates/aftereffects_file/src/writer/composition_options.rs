//! Checked composition-record options shared by root and precomposition writers.

use crate::schema::CompositionRecord;

use super::AepWriteError;

/// Native-integral composition settings lowered from the current FX document.
///
/// A value is passed explicitly to every generated composition so nested output
/// cannot fall back to source-project metadata or independently guessed defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompositionOptions {
    enabled: bool,
    shutter_angle: u16,
    shutter_phase: i32,
    samples_per_frame: i32,
    adaptive_sample_limit: i32,
}

impl CompositionOptions {
    /// Constructs settings already checked against AE's integral field widths.
    pub(crate) const fn motion_blur(
        enabled: bool,
        shutter_angle: u16,
        shutter_phase: i32,
        samples_per_frame: i32,
        adaptive_sample_limit: i32,
    ) -> Self {
        Self {
            enabled,
            shutter_angle,
            shutter_phase,
            samples_per_frame,
            adaptive_sample_limit,
        }
    }
}

/// Applies current composition options to a freshly constructed `cdta` record.
pub(crate) fn apply(
    record: &mut CompositionRecord,
    options: CompositionOptions,
) -> Result<(), AepWriteError> {
    record.set_motion_blur(
        options.enabled,
        options.shutter_angle,
        options.shutter_phase,
        options.samples_per_frame,
        options.adaptive_sample_limit,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{schema::CompositionRecord, timing::Duration24};

    use super::{CompositionOptions, apply};

    #[test]
    fn applies_typed_options_to_a_fresh_composition_record() {
        let mut record =
            CompositionRecord::empty_ae26(640, 360, Duration24::from_frames(24).unwrap()).unwrap();
        apply(
            &mut record,
            CompositionOptions::motion_blur(true, 225, -45, 32, 192),
        )
        .unwrap();

        assert_eq!(record.flags()[1] & 8, 8);
        assert_eq!(record.shutter_angle(), 225);
        assert_eq!(record.shutter_phase(), -45);
        assert_eq!(record.motion_blur_samples(), (32, 192));
    }
}
