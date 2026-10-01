//! Ordinary mono/stereo audio streams and their timeline placements.

use super::{records, MediaId, PrScalarKeyframe};
use crate::error::{unsupported, Result};
use fx_schema::LinearGain;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioChannels {
    Mono,
    Stereo,
}

impl AudioChannels {
    /// Effective gain of a source mixed onto Premiere's measured static
    /// centered stereo route. The writer emits that same route.
    pub(crate) const fn centered_stereo_gain(self) -> f64 {
        match self {
            Self::Mono => std::f64::consts::FRAC_1_SQRT_2,
            Self::Stereo => 1.0,
        }
    }

    pub(crate) const fn count(self) -> usize {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
        }
    }

    /// Native `AudioChannelLayout` JSON.
    pub(crate) const fn layout(self) -> &'static str {
        match self {
            Self::Mono => records::MONO,
            Self::Stereo => records::STEREO,
        }
    }

    /// Native `ChannelType`.
    pub(crate) const fn channel_type(self) -> &'static str {
        match self {
            Self::Mono => "0",
            Self::Stereo => "1",
        }
    }

    /// `FilterMatchName` of the intrinsic clip Volume.
    pub(crate) const fn volume_match_name(self) -> &'static str {
        match self {
            Self::Mono => "Internal Volume Mono",
            Self::Stereo => "Internal Volume Stereo",
        }
    }

    /// Native `ChannelConfigData` of an intrinsic clip filter.
    pub(crate) const fn filter_channel_config(self) -> &'static str {
        match self {
            Self::Mono => {
                r#"{"in":[{"layout":[0],"name":"Mono In","type":0}],"out":[{"layout":[0],"name":"Mono Out","type":0}]}"#
            }
            Self::Stereo => {
                r#"{"in":[{"layout":[100,101],"name":"Stereo In","type":0}],"out":[{"layout":[100,101],"name":"Stereo Out","type":0}]}"#
            }
        }
    }

    pub(crate) fn parse(layout: &str) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Channel {
            channellabel: u32,
        }
        /// Projects up to Premiere 13 wrap the same channel array.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wrapped {
            #[serde(rename = "SerializerWrappedObject")]
            channels: Vec<Channel>,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Layout {
            Channels(Vec<Channel>),
            Wrapped(Wrapped),
        }
        let (Layout::Channels(channels) | Layout::Wrapped(Wrapped { channels })) =
            serde_json::from_str(layout)?;
        match channels.as_slice() {
            [Channel { channellabel: 0 }] => Ok(Self::Mono),
            [Channel { channellabel: 100 }, Channel { channellabel: 101 }] => Ok(Self::Stereo),
            _ => Err(unsupported(
                "only ordinary mono/stereo channel layouts are supported",
            )),
        }
    }
}

/// Layout of Premiere's intrinsic clip Volume. Both store a normalized
/// `Level`, and a `Level` without a value is 1.0; they differ in the value
/// that plays at 0 dB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrVolumeLayout {
    /// `[Mute, Level]`, written by Premiere 25 and 26 (project versions 43 and 45).
    Current,
    /// `[Bypass, Level]`, written up to Premiere 13 (project versions 25 to 36).
    Legacy,
}

impl PrVolumeLayout {
    /// The `Level` and Channel Volume value that plays at 0 dB, as pinned by
    /// AME renders: 10^(-15/20) as Premiere 26.5.1 writes it, and 0.5.
    pub(crate) const fn unity(self) -> f64 {
        match self {
            Self::Current => 0.177_827_939_391,
            Self::Legacy => 0.5,
        }
    }
}

/// Audio-stream facts of one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrAudioStream {
    pub(crate) intrinsic_ticks: i64,
    pub(crate) channels: AudioChannels,
    pub(crate) sample_rate: u32,
}

/// One sound placement. Video and audio from the same source are separate placements.
#[derive(Debug, Clone)]
pub(crate) struct PrAudioOccurrence {
    pub(crate) id: Option<String>,
    pub(crate) media: MediaId,
    pub(crate) start_ticks: i64,
    pub(crate) end_ticks: i64,
    pub(crate) in_ticks: i64,
    pub(crate) out_ticks: i64,
    /// Static gain of every stage, linear with 1.0 at 0 dB. With keys, it
    /// holds the clip Level's value before its keys.
    pub(crate) volume: LinearGain,
    pub(crate) volume_keys: Option<PrVolumeKeys>,
}

impl PrAudioOccurrence {
    /// Error and omission context: the native item, else its media identity.
    pub(crate) fn record(&self) -> &str {
        self.id.as_deref().unwrap_or(self.media.as_str())
    }
}

/// Keyed clip Volume `Level`, with key times on the source clock.
#[derive(Debug, Clone)]
pub(crate) struct PrVolumeKeys {
    /// Level gains, linear with 1.0 at 0 dB, each with the easing that
    /// arrives at it: Linear or Hold.
    pub(crate) keys: Vec<PrScalarKeyframe>,
    /// Gain of the other stages (Clip Gain, source level, faders, mutes and
    /// the measured centered mono mix),
    /// which multiplies every key.
    pub(crate) gain: f64,
}
