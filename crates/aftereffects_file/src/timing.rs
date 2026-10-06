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

    #[cfg(test)]
    pub(crate) fn duration(self, millis: u64) -> Result<(u32, Duration24), RecordError> {
        self.duration_frames((millis as f64 * self.fps() / 1000.0).ceil())
    }

    /// Native composition authoring rounds the original seconds to the nearest
    /// frame, with positive half-frame ties upward. Do not feed this method the
    /// lossy FX millisecond projection or use it for child source coverage.
    /// Positive sub-half-frame inputs have a native null-frame endpoint, not a
    /// one-frame minimum. Source-duration constructors remain strictly positive.
    pub(crate) fn authored_duration(self, seconds: f64) -> Result<(u32, Duration24), RecordError> {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(RecordError::Invalid(
                "composition duration must be finite and positive",
            ));
        }
        let frames = (seconds * self.fps()).round();
        if frames == 0.0 {
            return Ok((0, Duration24(0)));
        }
        self.duration_frames(frames)
    }

    fn duration_frames(self, frames: f64) -> Result<(u32, Duration24), RecordError> {
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

/// Duration in AE's 24,576-ticks-per-second timebase. Source constructors require
/// positive ticks; root authoring may produce a native null-frame endpoint.
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
    fn authored_duration_preserves_native_null_frame_endpoint() {
        let rate = FrameRate::new(30.0).unwrap();
        // AE 26.5 constructor/setter controls, saved and reopened independently.
        for (seconds, frames) in [
            (0.001, 0),
            ((0.5 - 0.001) / 30.0, 0),
            (0.5 / 30.0, 1),
            ((0.5 + 0.001) / 30.0, 1),
            ((1.5 - 0.001) / 30.0, 1),
            (1.5 / 30.0, 2),
            ((1.5 + 0.001) / 30.0, 2),
        ] {
            let (actual_frames, duration) = rate.authored_duration(seconds).unwrap();
            assert_eq!(actual_frames, frames, "{seconds}s");
            assert_eq!(duration.ticks(), frames * 24_576 / 30);
        }
        for invalid in [
            0.0,
            -0.001,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
        ] {
            assert!(rate.authored_duration(invalid).is_err(), "{invalid}");
        }
        // Native root null-frame support does not relax source-duration constructors.
        assert!(Duration24::from_frames(0).is_err());
        assert!(Duration24::from_ticks(0).is_err());
    }

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
