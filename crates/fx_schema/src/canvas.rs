//! Canvas geometry and persisted composition-level motion-blur controls.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{NonNegativeProperty, PositiveProperty, ScalarProperty};

/// Pixel dimensions for a composition canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TS)]
#[ts(export_to = "project_types.d.ts")]
pub struct Dimensions {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
}

impl Dimensions {
    /// Creates a width/height pair.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

impl From<(u32, u32)> for Dimensions {
    fn from(value: (u32, u32)) -> Self {
        Self::new(value.0, value.1)
    }
}

/// After Effects composition-level controls for standard transform motion blur.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct MotionBlurSettings {
    /// Master composition switch. Individual layers must also opt in.
    #[serde(default)]
    pub enabled: bool,
    /// Exposure duration in degrees (`0..=720`; `360` is one frame).
    #[serde(default = "default_motion_blur_shutter_angle")]
    pub shutter_angle: NonNegativeProperty,
    /// Exposure offset in degrees (`-360..=360`). AE defaults to `-90`.
    #[serde(default = "default_motion_blur_shutter_phase")]
    pub shutter_phase: ScalarProperty,
    /// Minimum temporal samples per frame (`2..=64`).
    #[serde(default = "default_motion_blur_samples_per_frame")]
    pub samples_per_frame: PositiveProperty,
    /// Maximum adaptive temporal samples (`16..=256`).
    #[serde(default = "default_motion_blur_adaptive_sample_limit")]
    pub adaptive_sample_limit: PositiveProperty,
}

impl<'de> Deserialize<'de> for MotionBlurSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            #[serde(default)]
            enabled: bool,
            #[serde(default = "default_motion_blur_shutter_angle")]
            shutter_angle: NonNegativeProperty,
            #[serde(default = "default_motion_blur_shutter_phase")]
            shutter_phase: ScalarProperty,
            #[serde(default = "default_motion_blur_samples_per_frame")]
            samples_per_frame: PositiveProperty,
            #[serde(default = "default_motion_blur_adaptive_sample_limit")]
            adaptive_sample_limit: PositiveProperty,
        }

        let wire = Wire::deserialize(deserializer)?;
        let shutter_angle = wire.shutter_angle.value();
        let samples_per_frame = wire.samples_per_frame.value();
        let adaptive_sample_limit = wire.adaptive_sample_limit.value();
        if shutter_angle > 720.0 {
            return Err(serde::de::Error::custom(
                "motionBlur.shutterAngle must be within 0..=720",
            ));
        }
        if !(-360.0..=360.0).contains(&wire.shutter_phase) {
            return Err(serde::de::Error::custom(
                "motionBlur.shutterPhase must be within -360..=360",
            ));
        }
        if !(2.0..=64.0).contains(&samples_per_frame) || samples_per_frame.fract() != 0.0 {
            return Err(serde::de::Error::custom(
                "motionBlur.samplesPerFrame must be a whole number within 2..=64",
            ));
        }
        if !(16.0..=256.0).contains(&adaptive_sample_limit) || adaptive_sample_limit.fract() != 0.0
        {
            return Err(serde::de::Error::custom(
                "motionBlur.adaptiveSampleLimit must be a whole number within 16..=256",
            ));
        }
        Ok(Self {
            enabled: wire.enabled,
            shutter_angle: wire.shutter_angle,
            shutter_phase: wire.shutter_phase,
            samples_per_frame: wire.samples_per_frame,
            adaptive_sample_limit: wire.adaptive_sample_limit,
        })
    }
}

impl MotionBlurSettings {
    /// Whether the optional wire record can be omitted without losing state.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl Default for MotionBlurSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            shutter_angle: default_motion_blur_shutter_angle(),
            shutter_phase: default_motion_blur_shutter_phase(),
            samples_per_frame: default_motion_blur_samples_per_frame(),
            adaptive_sample_limit: default_motion_blur_adaptive_sample_limit(),
        }
    }
}

fn default_motion_blur_shutter_angle() -> NonNegativeProperty {
    NonNegativeProperty::new(180.0).expect("180 is finite and non-negative")
}

fn default_motion_blur_shutter_phase() -> ScalarProperty {
    -90.0
}

fn default_motion_blur_samples_per_frame() -> PositiveProperty {
    PositiveProperty::new(16.0).expect("16 is finite and positive")
}

fn default_motion_blur_adaptive_sample_limit() -> PositiveProperty {
    PositiveProperty::new(128.0).expect("128 is finite and positive")
}
