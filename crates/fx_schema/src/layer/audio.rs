//! Canonical persisted audio-layer schema.
use crate::{
    AssetId, DonorCaptionPresentation, Duration, LayerId, LayerPlayback, TimeRangeProperty,
};
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};
use std::fmt;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, TS)]
#[serde(try_from = "f64")]
#[ts(type = "number")]
pub struct LinearGain(f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("linear gain must be finite and non-negative")]
pub struct LinearGainValidationError;

impl LinearGain {
    pub const ZERO: Self = Self(0.0);

    pub const UNITY: Self = Self(1.0);

    pub fn new(value: f64) -> Result<Self, LinearGainValidationError> {
        if !value.is_finite() || value < 0.0 {
            return Err(LinearGainValidationError);
        }
        Ok(Self(value))
    }

    pub const fn as_f64(self) -> f64 {
        self.0
    }
}

impl Default for LinearGain {
    fn default() -> Self {
        Self::UNITY
    }
}

impl TryFrom<f64> for LinearGain {
    type Error = LinearGainValidationError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<LinearGain> for f64 {
    fn from(gain: LinearGain) -> Self {
        gain.as_f64()
    }
}

/// Canonical AudioAutoDucking fields with reader-specific strictness.
#[doc(hidden)]
#[macro_export]
macro_rules! define_audioautoducking_schema {
    (reader: [$($reader:tt)*]) => {
        /// Caption-driven gain envelope applied after an audio layer's authored
        /// volume curve.
        ///
        /// This is deliberately separate from the layer's `AudioVolume` animator:
        /// fades, keyframes, and scripts produce the base gain, then the host audio
        /// mixer multiplies that gain by `ducked_gain` while speech is active.
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $($reader)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct AudioAutoDucking {
            /// Multiplier applied to the authored volume curve during speech.
            #[serde(default = "default_auto_duck_gain")]
            pub ducked_gain: LinearGain,
            /// Speech spans separated by at most this interval are treated as one
            /// continuous ducked span.
            #[serde(
                default = "default_auto_duck_merge_gap",
                rename = "mergeGapMs",
                deserialize_with = "deserialize_non_negative_duration"
            )]
            #[ts(type = "number", rename = "mergeGapMs")]
            pub merge_gap: Duration,
        }
    };
}

define_audioautoducking_schema! { reader: [] }

impl Default for AudioAutoDucking {
    fn default() -> Self {
        Self {
            ducked_gain: default_auto_duck_gain(),
            merge_gap: default_auto_duck_merge_gap(),
        }
    }
}

/// Default stored ducking gain.
pub const AUTO_DUCK_DEFAULT_GAIN: LinearGain = LinearGain(0.2);
/// Default stored speech-merge gap.
pub const AUTO_DUCK_DEFAULT_MERGE_GAP: Duration = Duration::from_millis(2_000);

const fn default_auto_duck_gain() -> LinearGain {
    AUTO_DUCK_DEFAULT_GAIN
}
const fn default_auto_duck_merge_gap() -> Duration {
    AUTO_DUCK_DEFAULT_MERGE_GAP
}

fn deserialize_non_negative_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Duration, D::Error> {
    struct NonNegativeDurationVisitor;
    impl NonNegativeDurationVisitor {
        fn from_f64<E: de::Error>(value: f64) -> Result<Duration, E> {
            if value < 0.0 {
                return Err(E::invalid_value(
                    de::Unexpected::Float(value),
                    &"a non-negative autoDucking mergeGapMs value",
                ));
            }
            Ok(Duration::from_millis_f64(value))
        }
    }
    impl Visitor<'_> for NonNegativeDurationVisitor {
        type Value = Duration;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a non-negative autoDucking mergeGapMs value")
        }
        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Duration::from_millis(value))
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
            if value < 0 {
                Err(E::invalid_value(de::Unexpected::Signed(value), &self))
            } else {
                Ok(Duration::from_millis(value as u64))
            }
        }
        fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E> {
            Ok(Duration::from_millis(
                u64::try_from(value).unwrap_or(u64::MAX),
            ))
        }
        fn visit_i128<E: de::Error>(self, value: i128) -> Result<Self::Value, E> {
            if value < 0 {
                Err(E::invalid_value(
                    de::Unexpected::Other("negative integer"),
                    &self,
                ))
            } else {
                Ok(Duration::from_millis(
                    u64::try_from(value).unwrap_or(u64::MAX),
                ))
            }
        }
        fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
            Self::from_f64(value)
        }
        fn visit_f32<E: de::Error>(self, value: f32) -> Result<Self::Value, E> {
            Self::from_f64(f64::from(value))
        }
    }
    deserializer.deserialize_any(NonNegativeDurationVisitor)
}

/// Declare the compatibility audio reader's raw field projection.
///
/// Optional timing fields are intentionally retained until the product reader can
/// normalize either historical clocks or canonical windowed playback.
#[doc(hidden)]
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! define_wire_audio_layer_schema {
    ($emit:ident) => {
        $emit! {
            struct WireAudioLayer {
                id: LayerId,
                name: String,
                #[serde(default)]
                description: String,
                #[serde(default)]
                is_hidden: bool,
                parent: Option<LayerId>,
                #[serde(default, deserialize_with = "deserialize_optional_time_secs")]
                start_time: Option<Time>,
                #[serde(default = "crate::layer::default_wire_audio_volume")]
                volume: f64,
                #[serde(default)]
                auto_ducking: Option<AudioAutoDucking>,
                #[serde(default = "crate::layer::default_audio_window_ms")]
                window_ms: Duration,
                source: AudioSource,
                #[serde(default)]
                metadata: Option<AudioLayerMetadata>,
                #[serde(default)]
                captions_enabled: Option<bool>,
                #[serde(default)]
                caption_presentation: Option<DonorCaptionPresentation>,
                #[serde(
                    default,
                    deserialize_with = "super::wire::deserialize_legacy_range_presence"
                )]
                active_range: Option<TimeRangeProperty>,
                #[serde(
                    default,
                    deserialize_with = "super::wire::deserialize_legacy_range_presence"
                )]
                source_range: Option<TimeRangeProperty>,
                #[serde(
                    default,
                    deserialize_with = "super::wire::deserialize_playback_presence"
                )]
                playback: Option<serde_json::Value>,
                #[serde(default)]
                preserve_audio_pitch: bool,
                source_intrinsic_duration: Duration,
            }
        }
    };
}

/// Canonical persisted audio-layer fields; readers select only the legacy clock adapter.
#[doc(hidden)]
// The caller-qualified time-remap path preserves each reader's existing type and source spelling.
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! define_audio_layer_schema {
    (reader: [$($reader:tt)*], start_attrs: [$($start_attrs:tt)*]) => {
        /// Audio-only layer: contributes an audible clip to the export mix.
        ///
        /// AE models a pure audio import as a layer with only an *Audio* property
        /// group (Audio Levels) plus in/out timing — no Transform and no visual
        /// output. We mirror that: an `AudioLayer` produces no `scene::Node`; the
        /// host's audio-scene builder surfaces it into the `AudioScene` mix instead.
        ///
        /// Relationship to the read-only `AudioGain{Left,Right,Both}` properties: those
        /// *sample* an audio layer's source to drive **visual** reactivity (they make
        /// no sound); an `AudioLayer` is what makes audio actually **heard** in the
        /// export. The two are independent and can reference the same asset.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $($reader)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct AudioLayer {
            /// Stable composition-unique layer id — the id space referenced by
            /// `parent`, track mattes, path masks, and text `pathOptions`.
            pub id: LayerId,
            /// User-visible name for the layer. This is distinct from `description`.
            pub name: String,
            /// Optional user-visible description for the layer.
            #[serde(default, skip_serializing_if = "String::is_empty")]
            pub description: String,
            /// Whether this layer is excluded from all rendered and audible output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub is_hidden: bool,
            /// `LayerId` of the containing Group layer, or `null` for a root
            /// layer — mirrors the layer's position in the tree.
            #[serde(skip_serializing_if = "Option::is_none")]
            pub parent: Option<LayerId>,
            /// Legacy source-audio offset, mapped to the layer's start on the
            /// composition timeline. New documents use [`Self::source_range`], which
            /// can represent both offset and stretch. Still accepted on input for
            /// backwards compatibility, but omitted from canonical JSON because
            /// `sourceRange` owns audio source timing.
            $($start_attrs)*
            pub start_time: AudioStartTime,
            /// Linear gain applied to the source (AE "Audio Levels"); `1.0` = unity.
            #[serde(default = "default_audio_volume")]
            pub volume: LinearGain,
            /// Optional caption-driven gain envelope applied after the authored
            /// `volume` value or animator. Presence means auto-ducking is enabled.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub auto_ducking: Option<AudioAutoDucking>,
            /// Window centred on the current time over which this layer's
            /// `AudioGain{Left,Right,Both}` properties reduce the waveform to a
            /// scalar gain. Serialized as integer milliseconds (`Duration`'s
            /// transparent encoding), matching the previous `u32` wire format.
            #[serde(default = "default_audio_window_ms")]
            pub window_ms: Duration,
            /// Source audio asset reference.
            pub source: AudioSource,
            /// Audio role, source, and label metadata.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub metadata: Option<AudioLayerMetadata>,
            /// Explicit participation in the project's effective caption track.
            /// Unset voiceover layers retain the legacy default when their asset
            /// transcription resolves; unset music and sound-effect layers default
            /// off. Explicit presence wins for every audio modality.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub captions_enabled: Option<bool>,
            /// Sparse occurrence-local caption presentation keyed by canonical
            /// transcription coordinates. Projected effective-caption ids are never
            /// persisted here.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional)]
            pub caption_presentation: Option<DonorCaptionPresentation>,
            /// Source-audio span, independent of the visible playback window.
            pub source_range: TimeRangeProperty,
            /// Parent-clock window and editable source mapping.
            pub playback: LayerPlayback,
            /// Preserve perceived pitch during supported TimeRemap playback and export.
            /// Unsupported mappings retain ordinary resampling behavior.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub preserve_audio_pitch: bool,
            /// Intrinsic duration of the backing audio source asset.
            #[ts(type = "number")]
            pub source_intrinsic_duration: Duration,
        }
    };
}

type AudioStartTime = Option<crate::ScalarProperty>;
define_audio_layer_schema! {
    reader: [],
    start_attrs: [#[serde(default, skip_serializing_if = "Option::is_none")]]
}

pub const fn default_audio_volume() -> LinearGain {
    LinearGain::UNITY
}

pub const fn default_wire_audio_volume() -> f64 {
    LinearGain::UNITY.as_f64()
}

pub fn default_audio_window_ms() -> Duration {
    Duration::from_millis(50)
}

/// Declare the source with the caller's enhancement reader.
#[doc(hidden)]
#[macro_export]
macro_rules! define_audiosource_schema {
    () => {
        /// Source reference for an `AudioLayer`.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        #[ts(export_to = "project_types.d.ts")]
        pub struct AudioSource {
            /// Captions asset identifier for the source audio.
            pub asset_id: AssetId,
            /// Optional enhancement state and asset lineage for editor actions.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            #[ts(optional = nullable)]
            pub enhancement: Option<AudioSourceEnhancement>,
        }
    };
}

define_audiosource_schema! {}

impl AudioSource {
    /// Asset that renders for the current enhancement state.
    #[must_use]
    pub fn active_asset_id(&self) -> &AssetId {
        self.enhancement
            .as_ref()
            .filter(|enhancement| enhancement.enabled)
            .map_or(&self.asset_id, |enhancement| &enhancement.enhanced_asset_id)
    }
}

/// Canonical AudioSourceEnhancement fields with reader-specific strictness.
#[doc(hidden)]
#[macro_export]
macro_rules! define_audiosourceenhancement_schema {
    (reader: [$($reader:tt)*]) => {
        /// Enhancement state and the generated asset needed to apply it.
        /// `AudioSource::asset_id` remains the original asset. Legacy AI Edit
        /// does not distinguish denoise from other enhanced-audio production, so the
        /// model records the accurate, general enhancement operation.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase" $($reader)*)]
        #[ts(export_to = "project_types.d.ts")]
        pub struct AudioSourceEnhancement {
            /// Whether this layer currently uses the enhanced output.
            #[serde(default, skip_serializing_if = "std::ops::Not::not")]
            pub enabled: bool,
            /// Generated enhanced-audio asset.
            pub enhanced_asset_id: AssetId,
        }
    };
}

define_audiosourceenhancement_schema! { reader: [] }

/// Audio role, source, and label metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct AudioLayerMetadata {
    /// What kind of audio the clip is (music / sound effect / voiceover).
    pub modality: AudioLayerModality,
    /// Where the clip came from (generated / uploaded / stock).
    pub source: AudioLayerSource,
    /// Whether AI Edit added this sound as part of a shot style.
    /// Independent of asset provenance; style cleanup uses this marker and the clip start.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_ai_edit: bool,
    /// User-visible source label, such as a generation prompt or original file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts", rename_all = "camelCase")]
pub enum AudioLayerModality {
    Music,
    SoundEffect,
    Voiceover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts", rename_all = "camelCase")]
pub enum AudioLayerSource {
    Generated,
    Uploaded,
    Stock,
}

#[cfg(test)]
mod linear_gain_tests {
    #![allow(clippy::unwrap_used)]

    use serde_json::json;

    use super::*;

    #[test]
    fn linear_gain_accepts_mute_unity_and_amplification() {
        assert_eq!(LinearGain::new(0.0).unwrap(), LinearGain::ZERO);
        assert_eq!(LinearGain::new(1.0).unwrap(), LinearGain::UNITY);
        assert_eq!(LinearGain::new(1.5).unwrap().as_f64(), 1.5);
        assert_eq!(default_audio_volume(), LinearGain::UNITY);
        assert_eq!(default_wire_audio_volume(), 1.0);
    }

    #[test]
    fn linear_gain_rejects_negative_and_non_finite_values() {
        assert!(LinearGain::new(-0.1).is_err());
        assert!(LinearGain::new(f64::NAN).is_err());
        assert!(LinearGain::new(f64::INFINITY).is_err());
        assert!(LinearGain::new(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn linear_gain_wire_format_remains_a_plain_number() {
        let gain: LinearGain = serde_json::from_value(json!(1.5)).unwrap();
        assert_eq!(gain.as_f64(), 1.5);
        assert_eq!(serde_json::to_value(gain).unwrap(), json!(1.5));
        assert!(serde_json::from_value::<LinearGain>(json!(-0.1)).is_err());
    }
}
