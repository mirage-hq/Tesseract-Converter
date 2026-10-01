//! Checked time values used by the native AE writer.

use crate::schema::RecordError;

/// AE's nominal composition FPS is a 16.16 fixed-point number. Timeline
/// timestamps themselves use seconds (24,576 ticks per second), not frames.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameRate(u32);

impl FrameRate {
    pub(crate) fn new(fps: f64) -> Result<Self, RecordError> {
        if !fps.is_finite() || !(1.0 / 65_536.0..=240.0).contains(&fps) {
            return Err(RecordError::Invalid(
                "FPS must be finite and between 1/65536 and 240",
            ));
        }
        let fixed = (fps * 65_536.0).round() as u32;
        Ok(Self(fixed))
    }

    pub(crate) fn fps(self) -> f64 {
        f64::from(self.0) / 65_536.0
    }

    pub(crate) fn parts(self) -> (u16, u16) {
        ((self.0 >> 16) as u16, self.0 as u16)
    }

    pub(crate) fn frame_interval(self) -> (u32, u32) {
        if self.0 == 24 * 65_536 {
            (1024, 24_576)
        } else {
            // Reciprocal of the exact nominal 16.16 rate, not a second
            // independently rounded FPS approximation.
            (65_536, self.0)
        }
    }

    pub(crate) fn duration(self, millis: u64) -> Result<(u32, Duration24), RecordError> {
        let frames = (millis as f64 * self.fps() / 1000.0).ceil();
        if !frames.is_finite() || frames < 1.0 || frames > u32::MAX as f64 {
            return Err(RecordError::Invalid("invalid composition frame count"));
        }
        // A fractional tick cannot represent the exact frame boundary. Round
        // down so the encoded duration never extends into an extra frame.
        let frames = frames as u32;
        let ticks = u128::from(frames) * 24_576 * 65_536 / u128::from(self.0);
        let ticks = u32::try_from(ticks)
            .ok()
            .filter(|ticks| *ticks <= i32::MAX as u32)
            .ok_or(RecordError::Invalid(
                "duration exceeds the AE signed tick range",
            ))?;
        Ok((frames, Duration24::from_ticks(ticks)?))
    }
}

/// Positive duration in AE's 24,576-ticks-per-second timebase.
/// The historical name reflects the original 24fps-only writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Duration24(u32);

impl Duration24 {
    /// Converts 24fps frames to 1024 ticks per frame for the legacy writer API.
    pub(crate) fn from_frames(frames: u32) -> Result<Self, RecordError> {
        let ticks = frames
            .checked_mul(1024)
            .filter(|value| i32::try_from(*value).is_ok())
            .ok_or(RecordError::Invalid(
                "duration exceeds the AE signed tick range",
            ))?;
        if ticks == 0 {
            return Err(RecordError::Invalid("zero duration"));
        }
        Ok(Self(ticks))
    }
    /// Constructs a duration from 24,576 ticks per second (independent of FPS).
    pub(crate) fn from_ticks(ticks: u32) -> Result<Self, RecordError> {
        if ticks == 0 || i32::try_from(ticks).is_err() {
            return Err(RecordError::Invalid(
                "duration exceeds the AE signed tick range",
            ));
        }
        Ok(Self(ticks))
    }

    /// Returns the exact AE ticks for unsigned duration fields.
    pub(crate) const fn ticks(self) -> u32 {
        self.0
    }

    /// Returns the exact AE ticks for signed timeline fields.
    pub(crate) const fn signed_ticks(self) -> i32 {
        // The private constructor rejects values above i32::MAX.
        self.0 as i32
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration24, FrameRate};
    #[test]
    fn duration_boundaries() {
        assert!(Duration24::from_frames(0).is_err());
        let max = i32::MAX as u32 / 1024;
        assert_eq!(Duration24::from_frames(max).unwrap().ticks(), max * 1024);
        assert!(Duration24::from_frames(max + 1).is_err());
        assert!(Duration24::from_frames(u32::MAX).is_err());
        assert_eq!(
            Duration24::from_frames(max).unwrap().signed_ticks(),
            (max * 1024) as i32
        );
    }

    #[test]
    fn arbitrary_fixed_point_rates_use_exact_tick_quotients() {
        // Include rates whose frame boundary is not an integral AE tick.
        for fixed in [1, 65_537, 1_965_080, 3_932_161, 15_728_639] {
            let rate = FrameRate(fixed);
            for millis in [1, 1_001, 2_040, 10_001] {
                let result = rate.duration(millis);
                let frames = (millis as f64 * rate.fps() / 1000.0).ceil() as u32;
                let numerator = u128::from(frames) * 24_576 * 65_536;
                let expected = numerator / u128::from(fixed);
                if expected > i32::MAX as u128 {
                    assert!(result.is_err());
                } else {
                    let (actual_frames, duration) = result.unwrap();
                    assert_eq!(actual_frames, frames);
                    assert_eq!(u128::from(duration.ticks()), expected);
                    assert!(u128::from(duration.ticks()) * u128::from(fixed) <= numerator);
                }
            }
        }
    }

    #[test]
    fn fractional_frame_boundaries_do_not_gain_an_extra_frame() {
        for fps in [24.0, 25.0, 29.97, 30.0, 60.0, 240.0] {
            let rate = FrameRate::new(fps).unwrap();
            for millis in [1, 1_001, 2_040, 10_001] {
                let (frames, duration) = rate.duration(millis).unwrap();
                let (numerator, denominator) = rate.frame_interval();
                let boundary = u128::from(frames) * u128::from(numerator) * 24_576;
                let ticks = u128::from(duration.ticks());
                assert!(
                    ticks * u128::from(denominator) <= boundary,
                    "{fps}fps at {millis}ms"
                );
                assert!(
                    (ticks + 1) * u128::from(denominator) > boundary,
                    "{fps}fps at {millis}ms"
                );
            }
        }
    }
}
