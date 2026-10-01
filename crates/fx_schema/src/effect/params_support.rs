//! Canonical persisted effect support enums.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What the renderer substitutes when the segmenter produces an empty mask
/// (no person detected). Mirrors [`scene::SegmentationEmptyFallback`] with a
/// serde representation so `LayerEffect` round-trips through JSON.
///
/// The choice depends on what sits underneath the masked-subject layer in
/// the composition's render tree (see the scene-crate doc for the original
/// rationale).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum SegmentationEmptyFallback {
    /// Show the masked source layer as-is (white mask). Use when the layer
    /// underneath is a synthetic background, so falling back to "show
    /// everything" preserves the user's footage if the segmenter fails.
    #[default]
    White,
    /// Hide the masked-subject layer entirely (transparent mask). Use when
    /// the layer underneath is the same source video; otherwise the
    /// fallback would paint a duplicate over the bg overlays.
    Hide,
}

/// AE "Shutter Control" popup for [`super::LayerEffect::PixelMotionBlur`].
///
/// `Automatic` mirrors AE's behavior of inheriting the composition's
/// motion-blur settings; since fx compositions carry no comp-level
/// motion-blur configuration, it resolves to AE's composition defaults
/// (Shutter Angle 180°, 16 samples per frame). `Manual` honors the
/// effect's own `shutter_angle` / `shutter_samples`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum ShutterControl {
    /// Inherit AE's composition motion-blur defaults (180 degrees shutter,
    /// 16 samples), ignoring the effect's own `shutterAngle` /
    /// `shutterSamples`.
    #[default]
    Automatic,
    /// Honor the effect's `shutterAngle` (clamped 0..720) and
    /// `shutterSamples` (clamped 2..64).
    Manual,
}

/// Source selector for [`super::LayerEffect::ShiftChannels`], mirroring AE's
/// "Take <channel> From" dropdown (supported subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum ChannelSource {
    /// Keep the channel unchanged when it names its OWN output channel
    /// (`takeRedFrom: "red"`); cross-channel routing is not expressible and
    /// falls back to unchanged (warn-once).
    Red,
    /// See `Red` — keeps `takeGreenFrom` unchanged.
    Green,
    /// See `Red` — keeps `takeBlueFrom` unchanged.
    Blue,
    /// Force the output channel to its maximum (255 / white).
    FullOn,
    /// Force the output channel to 0 (black).
    FullOff,
}

impl ChannelSource {
    // serde per-field defaults: an omitted "take X from" keeps channel X.
    pub fn red() -> Self {
        ChannelSource::Red
    }
    pub fn green() -> Self {
        ChannelSource::Green
    }
    pub fn blue() -> Self {
        ChannelSource::Blue
    }
}

#[path = "params_support_declaration.rs"]
mod declaration;

crate::define_noise_popup_schema!();
