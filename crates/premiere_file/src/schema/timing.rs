//! Sequence rates and positive native source frame durations in Adobe ticks.

use crate::error::{unsupported, Result};
use std::fmt;

/// Adobe ticks per second, shared by sequence and media timestamps.
pub(crate) const TICKS: i64 = 254_016_000_000;
pub(crate) const TICKS_PER_MILLISECOND: i64 = TICKS / 1000;

/// A tick time for messages, in seconds rounded to the nearest millisecond.
pub(crate) fn seconds(ticks: i64) -> String {
    let millisecond = i128::from(TICKS_PER_MILLISECOND);
    let millis = (i128::from(ticks) + millisecond / 2).div_euclid(millisecond);
    format!("{}.{:03}", millis.div_euclid(1000), millis.rem_euclid(1000))
}

/// One of the constant video frame rates that conversion supports.
///
/// Native sequence clocks retain their exact ticks; physical source durations use
/// `SourceFrameRate`. The listed rates remain the media/export policy allowlist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRate {
    /// 24000/1001 (23.976) fps.
    Fps24000Over1001,
    Fps24,
    Fps25,
    /// 30000/1001 (29.97) fps.
    Fps30000Over1001,
    Fps30,
    Fps50,
    /// 60000/1001 (59.94) fps.
    Fps60000Over1001,
    Fps60,
    /// An imported sequence clock with a positive tick period.
    Native(NativeFrameRate),
}

/// An admitted native sequence clock. Construction validates the editable clock bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeFrameRate {
    ticks_per_frame: i64,
}

impl FrameRate {
    pub(crate) const ALL: [Self; 8] = [
        Self::Fps24000Over1001,
        Self::Fps24,
        Self::Fps25,
        Self::Fps30000Over1001,
        Self::Fps30,
        Self::Fps50,
        Self::Fps60000Over1001,
        Self::Fps60,
    ];

    /// Frames per second as an exact numerator and denominator.
    pub(crate) const fn frames_per_second(self) -> (u64, u64) {
        match self {
            Self::Fps24000Over1001 => (24000, 1001),
            Self::Fps24 => (24, 1),
            Self::Fps25 => (25, 1),
            Self::Fps30000Over1001 => (30000, 1001),
            Self::Fps30 => (30, 1),
            Self::Fps50 => (50, 1),
            Self::Fps60000Over1001 => (60000, 1001),
            Self::Fps60 => (60, 1),
            Self::Native(rate) => (TICKS as u64, rate.ticks_per_frame as u64),
        }
    }

    /// Frame duration in Adobe ticks. The division is exact for every listed rate.
    pub const fn ticks_per_frame(self) -> i64 {
        if let Self::Native(rate) = self {
            return rate.ticks_per_frame;
        }
        let (numerator, denominator) = self.frames_per_second();
        TICKS * denominator as i64 / numerator as i64
    }

    /// Ticks of the whole frames in `seconds`, rounded down to a frame.
    pub(crate) const fn whole_frame_ticks(self, seconds: i64) -> i64 {
        seconds * TICKS / self.ticks_per_frame() * self.ticks_per_frame()
    }

    /// Source in-point of a new still, Color Matte or graphic placement: one
    /// hour into its synthetic clock, rounded down to a whole frame. Adobe-saved
    /// projects use 3600 s at 24, 25 and 30 fps (`visualizer_slideshow`,
    /// `cinemagraph`, `corporate_slideshow`) and 107 892 frames at 29.97 fps
    /// (`abstract_slideshow`, `credits`); the other rates are inferred.
    pub(crate) const fn generator_in_ticks(self) -> i64 {
        self.whole_frame_ticks(60 * 60)
    }

    pub(crate) fn from_ticks_per_frame(ticks: i64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|rate| rate.ticks_per_frame() == ticks)
    }

    /// Keeps a native sequence's exact frame grid without adding a media codec/rate.
    pub(crate) fn from_sequence_ticks(ticks: i64) -> Option<Self> {
        Self::from_ticks_per_frame(ticks).or_else(|| {
            (ticks > 0).then_some(Self::Native(NativeFrameRate {
                ticks_per_frame: ticks,
            }))
        })
    }

    /// Matches a sample duration of `units` on a clock of `timescale` units per second.
    pub(crate) fn from_seconds_per_frame(units: u32, timescale: u32) -> Option<Self> {
        if timescale == 0 {
            return None;
        }
        Self::ALL.into_iter().find(|rate| {
            let (numerator, denominator) = rate.frames_per_second();
            u64::from(units) * numerator == u64::from(timescale) * denominator
        })
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (numerator, denominator) = self.frames_per_second();
        if denominator == 1 {
            write!(f, "{numerator} fps")
        } else {
            write!(f, "{numerator}/{denominator} fps")
        }
    }
}

/// Positive native source frame duration, distinct from a sequence's rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceFrameRate(i64);

impl SourceFrameRate {
    pub(crate) fn from_ticks_per_frame(ticks: i64) -> Result<Self> {
        if ticks <= 0 {
            return Err(unsupported("source frame duration must be positive"));
        }
        Ok(Self(ticks))
    }

    pub(crate) const fn ticks_per_frame(self) -> i64 {
        self.0
    }

    pub(crate) fn supported(self) -> Option<FrameRate> {
        FrameRate::from_ticks_per_frame(self.0)
    }
}

impl From<FrameRate> for SourceFrameRate {
    fn from(rate: FrameRate) -> Self {
        Self(rate.ticks_per_frame())
    }
}

impl fmt::Display for SourceFrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.supported() {
            Some(rate) => rate.fmt(f),
            None => write!(f, "{} ticks per frame", self.0),
        }
    }
}

/// Native image orientation limited to the quarter turns already decoded by
/// the player's container display matrix. Mirroring has no supported mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum VideoOrientation {
    #[default]
    Identity,
    Clockwise,
    HalfTurn,
    CounterClockwise,
}

impl VideoOrientation {
    /// The orientation of a native `OriginalImageOrientationType`, an EXIF
    /// orientation code: 1 normal, 3 a 180° turn, 6 a 90° clockwise turn and
    /// 8 a 90° counterclockwise turn. The mirrored codes have no mapping.
    pub(crate) fn from_native(value: &str) -> Result<Self> {
        match value {
            "1" => Ok(Self::Identity),
            "6" => Ok(Self::Clockwise),
            "3" => Ok(Self::HalfTurn),
            "8" => Ok(Self::CounterClockwise),
            _ => Err(unsupported(
                "source orientation must be an unmirrored quarter turn",
            )),
        }
    }

    /// The EXIF orientation code of [`Self::from_native`].
    pub(crate) fn native(self) -> &'static str {
        match self {
            Self::Identity => "1",
            Self::Clockwise => "6",
            Self::HalfTurn => "3",
            Self::CounterClockwise => "8",
        }
    }

    /// The displayed size of a `width` × `height` encoded picture.
    pub(crate) fn display_dimensions(self, width: u32, height: u32) -> [u32; 2] {
        match self {
            Self::Clockwise | Self::CounterClockwise => [height, width],
            Self::Identity | Self::HalfTurn => [width, height],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_sequence_clocks_keep_exact_ticks_without_widening_media_rates() {
        for ticks in [
            1,
            TICKS_PER_MILLISECOND - 1,
            8_511_237_907,
            2 * TICKS,
            i64::MAX,
        ] {
            let rate = FrameRate::from_sequence_ticks(ticks).unwrap();
            assert_eq!(rate.ticks_per_frame(), ticks);
            assert_eq!(rate.frames_per_second(), (TICKS as u64, ticks as u64));
            assert!(FrameRate::from_ticks_per_frame(ticks).is_none());
        }
        for ticks in [-1, 0] {
            assert!(FrameRate::from_sequence_ticks(ticks).is_none());
        }
        for rate in FrameRate::ALL {
            assert_eq!(
                FrameRate::from_sequence_ticks(rate.ticks_per_frame()),
                Some(rate)
            );
        }
    }

    #[test]
    fn exact_rates_match_native_ticks_and_container_timescales() {
        for (frame_rate, ticks, units, timescale) in [
            (FrameRate::Fps24000Over1001, 10_594_584_000, 1001, 24000),
            (FrameRate::Fps24, 10_584_000_000, 512, 12288),
            (FrameRate::Fps25, 10_160_640_000, 512, 12800),
            (FrameRate::Fps30000Over1001, 8_475_667_200, 1001, 30000),
            (FrameRate::Fps30, 8_467_200_000, 512, 15360),
            (FrameRate::Fps50, 5_080_320_000, 256, 12800),
            (FrameRate::Fps60000Over1001, 4_237_833_600, 1001, 60000),
            (FrameRate::Fps60, 4_233_600_000, 256, 15360),
        ] {
            let (num, den) = frame_rate.frames_per_second();
            assert_eq!(TICKS as u64 * den % num, 0);
            assert_eq!(frame_rate.ticks_per_frame(), ticks);
            assert_eq!(FrameRate::from_ticks_per_frame(ticks), Some(frame_rate));
            assert_eq!(
                FrameRate::from_seconds_per_frame(units, timescale),
                Some(frame_rate)
            );
        }
        for ticks in [0, -1, 123, i64::MAX] {
            assert_eq!(FrameRate::from_ticks_per_frame(ticks), None);
        }
        for (units, timescale) in [(0, 0), (1, 0), (0, 30), (1, 27), (1000, 23976)] {
            assert_eq!(FrameRate::from_seconds_per_frame(units, timescale), None);
        }
        assert_eq!(FrameRate::Fps30000Over1001.to_string(), "30000/1001 fps");
        assert_eq!(FrameRate::Fps30.to_string(), "30 fps");
    }
}
