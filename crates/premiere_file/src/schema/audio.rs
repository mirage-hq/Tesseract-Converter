//! Ordinary mono/stereo audio streams and their timeline placements.

use super::{records, MediaId, PrScalarKeyframe};
use crate::error::{unsupported, Result};
use fx_schema::LinearGain;
use serde::Deserialize;
use std::ops::Range;

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
    /// Converter-owned clock of a prepared full raw source; never serialized.
    pub(crate) prepared_clock: Option<crate::audio_media::DelayedAudioClock>,
    pub(crate) intrinsic_ticks: i64,
    pub(crate) channels: AudioChannels,
    pub(crate) sample_rate: u32,
}

/// Converter-owned channel extraction, never serialized into editable FX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrAudioSourceChannel {
    /// A genuine mono selector must not fall back to the unselected stereo mix.
    Mono(usize),
    /// Fill Right may be omitted without losing baseline stereo sound. The
    /// native filter record is retained only for the contextual omission.
    FillRight(String),
}

impl PrAudioSourceChannel {
    pub(crate) fn channel(&self) -> usize {
        match self {
            Self::Mono(channel) => *channel,
            Self::FillRight(_) => 0,
        }
    }
}

/// One sound placement. Video and audio from the same source are separate placements.
#[derive(Debug, Clone)]
pub(crate) struct PrAudioOccurrence {
    /// Import extracts unchanged full-source samples before building editable
    /// audio layers, with distinct fallback rules for selectors and inserts.
    pub(crate) source_channel: Option<PrAudioSourceChannel>,
    pub(crate) id: Option<String>,
    pub(crate) media: MediaId,
    pub(crate) start_ticks: i64,
    pub(crate) end_ticks: i64,
    pub(crate) in_ticks: i64,
    pub(crate) out_ticks: i64,
    /// Signed saved speed: source bounds remain ordered even for reverse.
    /// Editable conversion rounds the independent endpoints, not their product.
    pub(crate) playback_rate: f64,
    /// Canonical native ON flag, absent in the canonical OFF representation.
    pub(crate) preserve_audio_pitch: bool,
    /// Static gain of every stage, linear with 1.0 at 0 dB. With keys, it
    /// holds the clip Level's value before its keys.
    pub(crate) volume: LinearGain,
    pub(crate) volume_keys: Option<PrVolumeKeys>,
    /// The fade that starts the placement and the fade that ends it.
    pub(crate) fade_in: Option<PrAudioFade>,
    pub(crate) fade_out: Option<PrAudioFade>,
}

/// Existing source-map pitch implementation bounds, coupled to
/// `crates/audio_renderer/src/pitch_preserve.rs` MIN_RATE/MAX_RATE. Reverse
/// and rates outside this interval retain the flag but use mapped resampling.
pub(crate) const RUNTIME_PITCH_RATES: std::ops::RangeInclusive<f64> = 0.25..=4.0;

impl PrAudioOccurrence {
    /// Only unit-forward pitch-OFF uses gain times relative to the layer In.
    /// Retiming or pitch-ON selects an absolute source-clock TimeRemap instead.
    pub(crate) fn uses_layer_clock(&self) -> bool {
        self.playback_rate == 1.0 && !self.preserve_audio_pitch
    }

    /// Error and omission context: the native item, else its media identity.
    pub(crate) fn record(&self) -> &str {
        self.id.as_deref().unwrap_or(self.media.as_str())
    }

    /// Whether the placement's volume changes over it: by Level keys or a fade.
    pub(crate) fn has_volume_animation(&self) -> bool {
        self.volume_keys.is_some() || self.fade_in.is_some() || self.fade_out.is_some()
    }

    /// Maps the placement clock through its saved source endpoints. Native audio
    /// endpoints can differ from the serialized speed's product; keeping both
    /// endpoints avoids accumulating that disagreement when trimming a sound.
    pub(crate) fn source_at(&self, timeline_ticks: i64) -> Result<i64> {
        let duration = i128::from(self.end_ticks) - i128::from(self.start_ticks);
        if duration <= 0 {
            return Err(unsupported("invalid audio placement duration"));
        }
        let span = i128::from(self.out_ticks) - i128::from(self.in_ticks);
        let elapsed = i128::from(timeline_ticks) - i128::from(self.start_ticks);
        let product = elapsed * span;
        let offset = (product.abs() + duration / 2) / duration * product.signum();
        // Premiere's shared Clip clock advances through In..Out in either
        // direction. Reverse bounds are measured backward from the media end.
        let source = i128::from(self.in_ticks) + offset;
        i64::try_from(source).map_err(|_| unsupported("audio source clock overflows"))
    }

    pub(crate) fn source_part(&self, timeline: &Range<i64>) -> Result<Range<i64>> {
        let start = self.source_at(timeline.start)?;
        let end = self.source_at(timeline.end)?;
        Ok(start.min(end)..start.max(end))
    }

    /// Maps an ordered source boundary back to the placement clock.
    pub(crate) fn timeline_at(&self, source_ticks: i64) -> Result<i64> {
        let span = i128::from(self.out_ticks) - i128::from(self.in_ticks);
        if span <= 0 {
            return Err(unsupported("invalid audio source duration"));
        }
        let elapsed = i128::from(source_ticks) - i128::from(self.in_ticks);
        let product = elapsed * (i128::from(self.end_ticks) - i128::from(self.start_ticks));
        let offset = (product.abs() + span / 2) / span * product.signum();
        i64::try_from(i128::from(self.start_ticks) + offset)
            .map_err(|_| unsupported("audio placement clock overflows"))
    }

    /// Plays only the part `source` of the placement's source range, over
    /// `timeline`. A fade whose whole source span the part plays still starts
    /// or ends it; any other fade is dropped, whichever edge of the part
    /// trims it. Returns the dropped fades of which the part still plays
    /// some: no fade represents what is left of them.
    pub(crate) fn play_part(
        &mut self,
        timeline: Range<i64>,
        source: Range<i64>,
    ) -> Result<Vec<PrAudioFade>> {
        let spans = [
            self.fade_in
                .as_ref()
                .map(|fade| {
                    self.source_part(&(self.start_ticks..self.start_ticks + fade.duration_ticks))
                })
                .transpose()?,
            self.fade_out
                .as_ref()
                .map(|fade| {
                    self.source_part(&(self.end_ticks - fade.duration_ticks..self.end_ticks))
                })
                .transpose()?,
        ];
        let mut cut = Vec::new();
        for (fade, span) in [
            (&mut self.fade_in, &spans[0]),
            (&mut self.fade_out, &spans[1]),
        ] {
            let Some(span) = span else {
                continue;
            };
            if source.start <= span.start && span.end <= source.end {
                continue;
            }
            let partly_played = span.start.max(source.start) < span.end.min(source.end);
            cut.extend(fade.take().filter(|_| partly_played));
        }
        (self.start_ticks, self.end_ticks) = (timeline.start, timeline.end);
        (self.in_ticks, self.out_ticks) = (source.start, source.end);
        Ok(cut)
    }
}

/// Bounded Custom Fade parameter: four incoming and six outgoing controls have
/// native render evidence. Other integers interpolate the parameter model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CustomFadeShape(i8);

impl CustomFadeShape {
    pub(crate) fn new(value: i64) -> Option<Self> {
        i8::try_from(value)
            .ok()
            .filter(|value| (-23..=29).contains(value))
            .map(Self)
    }

    pub(crate) const fn value(self) -> i8 {
        self.0
    }
}

/// Premiere's ordinary audio transition curves and bounded one-sided Custom Fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrFadeCurve {
    ConstantGain,
    ConstantPower,
    ExponentialFade,
    /// Saved bounded shape with absent type. Zero is squared sine, not Constant
    /// Gain. One-sided only; export uses editable Volume keys, not native Custom.
    Custom(CustomFadeShape),
}

impl PrFadeCurve {
    pub(crate) const ALL: [Self; 3] = [
        Self::ConstantGain,
        Self::ConstantPower,
        Self::ExponentialFade,
    ];

    /// Native `MatchName` and English `DisplayName`.
    pub(crate) const fn match_name(self) -> &'static str {
        match self {
            Self::ConstantGain => "Constant Gain",
            Self::ConstantPower => "Constant Power",
            Self::ExponentialFade => "Exponential Fade",
            Self::Custom(_) => "Custom Fade",
        }
    }

    /// The shortest fade, in milliseconds of the Tesseract clock, whose keys
    /// stay distinct there (`convert::audio::fade_keys`).
    pub(crate) const fn shortest_millis(self) -> i64 {
        match self {
            Self::ConstantGain => 1,
            Self::ConstantPower | Self::Custom(_) => 39,
            Self::ExponentialFade => 2,
        }
    }

    /// `FadeShapeType` and `FadeShapeValue` as Premiere 26.5.1 writes them;
    /// it writes neither for Constant Power.
    pub(crate) const fn fade_shape(self) -> Option<(i64, i64)> {
        match self {
            Self::ConstantGain => Some((0, 0)),
            Self::ConstantPower | Self::Custom(_) => None,
            Self::ExponentialFade => Some((0, -35)),
        }
    }
}

/// A fade at one edge of a sound placement, which plays its whole span: a
/// one-sided audio transition, or one half of a crossfade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrAudioFade {
    /// The native transition, for reports; absent on export.
    pub(crate) id: Option<String>,
    pub(crate) curve: PrFadeCurve,
    pub(crate) duration_ticks: i64,
}

impl PrAudioFade {
    /// A keyed clip Level may hold no key this close to a fade but one at the
    /// fade's full-level edge: conversion keys both on the millisecond grid,
    /// where two roundings could otherwise merge keys.
    pub(crate) const LEVEL_KEY_MARGIN_MILLIS: i64 = 2;

    /// Why a fade that [`PrAudioOccurrence::play_part`] returns is dropped.
    pub(crate) const PARTLY_PLAYED: &'static str =
        "audio fade not converted: the nested sequence plays only part of it";
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
