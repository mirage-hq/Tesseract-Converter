//! Round absolute boundaries once; derive durations from the rounded endpoints.

use crate::{
    error::{unsupported, Result},
    format::FrameRate,
    schema::TICKS_PER_MILLISECOND,
};
use fx_schema::{Duration, Time};

fn round_half_up(numerator: i128, divisor: i128) -> Option<i128> {
    if numerator < 0 {
        return None;
    }
    numerator.checked_add(divisor / 2)?.checked_div(divisor)
}

/// `value / divisor` to the nearest whole number, ties away from zero, as
/// `keyframes::layer_millis` rounds; `divisor` is positive and even, so a tie
/// is exact.
pub(super) fn nearest(value: i128, divisor: i128) -> Option<i128> {
    if value < 0 {
        round_half_up(value.checked_neg()?, divisor)?.checked_neg()
    } else {
        round_half_up(value, divisor)
    }
}

/// `1 / rate` as an exact fraction `(numerator, denominator)` of positive
/// integers, so that a clock divided by a native speed stays exact until it
/// rounds. `None` unless `rate` is positive, finite and normal, and also when
/// the fraction exceeds `i128`.
pub(super) fn reciprocal(rate: f64) -> Option<(i128, i128)> {
    if !(rate.is_finite() && rate > 0.0) {
        return None;
    }
    // A normal `rate` is (2^52 + fraction) × 2^(biased - 1075): 1075 is the
    // exponent bias 1023 plus the 52 fraction bits.
    let bits = rate.to_bits();
    let biased = i32::try_from(bits >> 52)
        .ok()
        .filter(|&biased| biased > 0)?;
    let whole = (bits & ((1 << 52) - 1)) | (1 << 52);
    let zeros = whole.trailing_zeros();
    let exponent = (biased - 1075).checked_add_unsigned(zeros)?;
    let power = |exponent: i32| {
        let shift = u32::try_from(exponent.max(0)).ok()?;
        1_i128.checked_shl(shift).filter(|&value| value > 0)
    };
    let denominator = i128::from(whole >> zeros).checked_mul(power(exponent)?)?;
    Some((power(-exponent)?, denominator))
}

pub(super) fn time_from_ticks(ticks: i64) -> Result<Time> {
    let millis = round_half_up(i128::from(ticks), i128::from(TICKS_PER_MILLISECOND))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| unsupported("negative Adobe timestamp"))?;
    Ok(Time::from_millis(millis))
}

/// Rounds a length that starts at tick 0, such as a document or media end.
/// A clip duration must come from its rounded endpoints instead.
pub(super) fn duration_from_ticks(ticks: i64) -> Result<Duration> {
    Ok(Duration::from_millis(time_from_ticks(ticks)?.as_millis()))
}

pub(super) fn ticks_from_time(time: Time, context: &str) -> Result<i64> {
    i64::try_from(i128::from(time.as_millis()) * i128::from(TICKS_PER_MILLISECOND))
        .map_err(|_| unsupported(format!("{context} exceeds Premiere's tick range")))
}

pub(super) fn frame_ticks_from_time(
    time: Time,
    frame_rate: FrameRate,
    context: &str,
) -> Result<i64> {
    let ticks = i128::from(ticks_from_time(time, context)?);
    let frame = i128::from(frame_rate.ticks_per_frame());
    round_half_up(ticks, frame)
        .and_then(|count| count.checked_mul(frame))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| unsupported(format!("{context} exceeds Premiere's tick range")))
}

/// A plain group starts its child clock at zero and advances at unit speed.
pub(super) fn is_plain_group_playback(playback: &fx_schema::LayerPlayback) -> bool {
    match playback.mapping() {
        fx_schema::LayerPlaybackMapping::Linear { input, output } => {
            input.duration == output.duration
                && i128::from(output.start.as_millis())
                    + i128::from(playback.input_range().start.as_millis())
                    + i128::from(playback.input_offset_ms())
                    - i128::from(input.start.as_millis())
                    == 0
        }
        fx_schema::LayerPlaybackMapping::TimeRemap { .. } => false,
    }
}

/// Exact source interval selected by an affine window, independent of source trimming.
pub(super) fn linear_source_range(
    playback: &fx_schema::LayerPlayback,
) -> Result<fx_schema::TimeRangeProperty> {
    let fx_schema::LayerPlaybackMapping::Linear { input, output } = playback.mapping() else {
        return Err(unsupported(
            "an editable Time Remap requires native key conversion",
        ));
    };
    let map = |time: Time| -> Result<Time> {
        let relative = i128::from(time.as_millis()) + i128::from(playback.input_offset_ms())
            - i128::from(input.start.as_millis());
        let numerator = relative * i128::from(output.duration.as_millis());
        let denominator = i128::from(input.duration.as_millis());
        if numerator % denominator != 0 {
            return Err(unsupported(
                "affine window has fractional millisecond source endpoints",
            ));
        }
        let mapped = i128::from(output.start.as_millis()) + numerator / denominator;
        u64::try_from(mapped)
            .map(Time::from_millis)
            .map_err(|_| unsupported("affine source endpoint exceeds the source clock"))
    };
    let start = map(playback.input_range().start)?;
    let end = map(playback.input_range().end())?;
    Ok(fx_schema::TimeRangeProperty::new(
        start,
        end.saturating_sub(start),
    ))
}

/// Evaluate continuous affine source time directly at the native tick boundary.
/// This picture-only path deliberately does not widen the audio millisecond clock.
pub(super) fn linear_source_ticks(
    playback: &fx_schema::LayerPlayback,
    parent_ticks: i64,
) -> Result<i64> {
    let fx_schema::LayerPlaybackMapping::Linear { input, output } = playback.mapping() else {
        return Err(unsupported("expected affine picture playback"));
    };
    let invalid = || unsupported("affine source endpoint is not representable in native ticks");
    let relative = i128::from(parent_ticks)
        .checked_add(
            (i128::from(playback.input_offset_ms()) - i128::from(input.start.as_millis()))
                .checked_mul(i128::from(TICKS_PER_MILLISECOND))
                .ok_or_else(invalid)?,
        )
        .ok_or_else(invalid)?;
    let numerator = relative
        .checked_mul(i128::from(output.duration.as_millis()))
        .ok_or_else(invalid)?;
    let denominator = i128::from(input.duration.as_millis());
    if denominator == 0 || numerator % denominator != 0 {
        return Err(invalid());
    }
    let anchor = i128::from(output.start.as_millis())
        .checked_mul(i128::from(TICKS_PER_MILLISECOND))
        .ok_or_else(invalid)?;
    i64::try_from(
        anchor
            .checked_add(numerator / denominator)
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())
}

/// Encode an exact interpreted-to-physical affine clock in independent domains.
/// Integer-ms eligibility encloses, rather than rounds away, fractional samples.
pub(super) fn interpreted_playback(
    clock: crate::media::InterpretedPictureClock,
    window: fx_schema::TimeRangeProperty,
    source_in: i64,
    placement_start: i64,
) -> Result<(fx_schema::LayerPlayback, fx_schema::TimeRangeProperty)> {
    use fx_schema::{LayerPlayback, TimeRangeProperty};
    let invalid = || {
        unsupported("interpreted affine origin is unrepresentable or exceeds the exact clock range")
    };
    let (n, d) = clock.ratio();
    let ticks = i128::from(TICKS_PER_MILLISECOND);
    let intercept = i128::from(source_in)
        .checked_sub(i128::from(placement_start))
        .and_then(|value| value.checked_mul(n))
        .ok_or_else(invalid)?;
    if intercept % ticks != 0 {
        return Err(invalid());
    }
    let c = intercept / ticks;
    // d*b + n*offset = c. Euclid supplies the inverse of d modulo n;
    // the nonnegative b keeps the authored output anchor a valid Time.
    let (mut a, mut b, mut x, mut y) = (d, n, 1_i128, 0_i128);
    while b != 0 {
        let quotient = a / b;
        (a, b) = (b, a % b);
        (x, y) = (
            y,
            x.checked_sub(quotient.checked_mul(y).ok_or_else(invalid)?)
                .ok_or_else(invalid)?,
        );
    }
    let anchor = c
        .rem_euclid(n)
        .checked_mul(x.rem_euclid(n))
        .ok_or_else(invalid)?
        .rem_euclid(n);
    let offset = c
        .checked_sub(d.checked_mul(anchor).ok_or_else(invalid)?)
        .ok_or_else(invalid)?
        / n;
    let at = |time: Time| -> Result<i128> {
        i128::from(time.as_millis())
            .checked_mul(n)
            .and_then(|v| v.checked_add(c))
            .filter(|&v| v >= 0)
            .ok_or_else(invalid)
    };
    let lo = at(window.start)? / d;
    let hi = at(window.end())?.checked_add(d - 1).ok_or_else(invalid)? / d;
    let unsigned = |value| u64::try_from(value).map_err(|_| invalid());
    let source_range = TimeRangeProperty::new(
        Time::from_millis(unsigned(lo)?),
        Duration::from_millis(unsigned(hi.checked_sub(lo).ok_or_else(invalid)?)?),
    );
    let playback = LayerPlayback::linear(
        window,
        TimeRangeProperty::new(Time::ZERO, Duration::from_millis(unsigned(d)?)),
        TimeRangeProperty::new(
            Time::from_millis(unsigned(anchor)?),
            Duration::from_millis(unsigned(n)?),
        ),
        i64::try_from(offset).map_err(|_| invalid())?,
    )
    .map_err(unsupported)?;
    Ok((playback, source_range))
}

/// Places the same authored content window on another parent clock.
/// Mapping keys and source selection stay unchanged; only the input origin moves.
pub(super) fn relocate_playback(
    playback: &fx_schema::LayerPlayback,
    window: fx_schema::TimeRangeProperty,
) -> Result<fx_schema::LayerPlayback> {
    let offset = i128::from(playback.input_offset_ms())
        + i128::from(playback.input_range().start.as_millis())
        - i128::from(window.start.as_millis());
    let offset = i64::try_from(offset)
        .map_err(|_| unsupported("relocated playback offset exceeds the clock range"))?;
    match playback.mapping() {
        fx_schema::LayerPlaybackMapping::Linear { input, output } => {
            fx_schema::LayerPlayback::linear(window, *input, *output, offset)
        }
        fx_schema::LayerPlaybackMapping::TimeRemap { property } => {
            fx_schema::LayerPlayback::remapped(window, property.clone(), offset)
        }
    }
    .map_err(unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ffmpeg-library")]
    #[test]
    fn interpreted_affine_origins_are_exact_or_rejected() {
        use crate::schema::{SourceFrameRate, SourceInterpretation};
        use std::io::Cursor;
        let bytes = include_bytes!("../../tests/fixtures/video-120000-over-1001fps.mp4");
        let file = crate::media::inspect_video_media(
            Cursor::new(bytes),
            Cursor::new(bytes),
            bytes.len() as u64,
        )
        .unwrap();
        let mut source = crate::tests::support::video_media()
            .into_values()
            .next()
            .unwrap()
            .video
            .unwrap();
        source.frame_rate = SourceFrameRate::from_ticks_per_frame(2_118_936_594).unwrap();
        source.intrinsic_ticks = 6 * 2_118_936_594;
        source.interpretation = SourceInterpretation::Rate(FrameRate::Fps30.into());
        let clock = crate::media::InterpretedPictureClock::bind(&source, &file).unwrap();
        let window =
            fx_schema::TimeRangeProperty::new(Time::from_millis(33), Duration::from_millis(100));
        let frame = FrameRate::Fps30.ticks_per_frame();
        assert!(interpreted_playback(clock, window, 0, frame)
            .unwrap_err()
            .to_string()
            .contains("origin"));
        // Equal fractional source/placement origins cancel exactly.
        let (playback, _) = interpreted_playback(clock, window, frame, frame).unwrap();
        assert_eq!(
            linear_source_ticks(&playback, 4 * crate::schema::TICKS).unwrap(),
            254_270_016_000
        );
        let window =
            fx_schema::TimeRangeProperty::new(Time::from_millis(1000), Duration::from_millis(100));
        let (playback, _) = interpreted_playback(clock, window, 0, crate::schema::TICKS).unwrap();
        assert_eq!(
            linear_source_ticks(&playback, crate::schema::TICKS).unwrap(),
            0
        );
        assert_eq!(
            linear_source_ticks(
                &playback,
                crate::schema::TICKS + 100 * TICKS_PER_MILLISECOND
            )
            .unwrap(),
            6_356_750_400
        );
        assert!(
            interpreted_playback(clock, window, 0, 2 * crate::schema::TICKS).is_err(),
            "negative source sample"
        );
    }

    #[test]
    fn rounding_bounds_and_shared_boundaries_hold_at_every_frame_rate() {
        for frame_rate in FrameRate::ALL {
            let frame = frame_rate.ticks_per_frame();
            let mut previous = Time::ZERO;
            for index in 0..=10000 {
                let ticks = index * frame;
                let rounded = time_from_ticks(ticks).unwrap();
                assert!(
                    (i128::from(rounded.as_millis()) * i128::from(TICKS_PER_MILLISECOND)
                        - i128::from(ticks))
                    .abs()
                        <= i128::from(TICKS_PER_MILLISECOND / 2)
                );
                if index > 0 {
                    assert!(rounded > previous);
                }
                previous = rounded;
            }
        }
        for millis in 0..=10000 {
            let time = Time::from_millis(millis);
            let ticks = frame_ticks_from_time(time, FrameRate::Fps30, "test").unwrap();
            let exact = ticks_from_time(time, "test").unwrap();
            assert!((ticks - exact).abs() <= FrameRate::Fps30.ticks_per_frame() / 2);
        }
    }

    #[test]
    fn relocated_window_preserves_independent_affine_mapping_and_source_time() {
        use fx_schema::{LayerPlayback, TimeRangeProperty};
        let range = |start, duration| {
            TimeRangeProperty::new(Time::from_millis(start), Duration::from_millis(duration))
        };
        let original = LayerPlayback::linear(
            range(1_000, 1_000),
            range(0, 4_000),
            range(2_000, 8_000),
            500,
        )
        .unwrap();
        let relocated = relocate_playback(&original, range(0, 1_000)).unwrap();
        assert_eq!(relocated.mapping(), original.mapping());
        assert_eq!(relocated.input_offset_ms(), 1_500);
        assert_eq!(
            linear_source_range(&relocated).unwrap(),
            range(5_000, 2_000)
        );
        assert_eq!(
            linear_source_range(&original).unwrap(),
            linear_source_range(&relocated).unwrap()
        );
        assert!(!is_plain_group_playback(&relocated));
    }

    #[test]
    fn affine_fractional_endpoint_is_diagnosed_instead_of_rounded() {
        use fx_schema::{LayerPlayback, TimeRangeProperty};
        let range = |duration| TimeRangeProperty::new(Time::ZERO, Duration::from_millis(duration));
        let playback = LayerPlayback::linear(range(1_001), range(2_000), range(3_000), 0).unwrap();
        assert!(linear_source_range(&playback).is_err());
    }

    #[test]
    fn ties_go_forward_and_overflow_rejects() {
        assert_eq!(time_from_ticks(127_135_008_000).unwrap().as_millis(), 501);
        assert_eq!(
            frame_ticks_from_time(Time::from_millis(50), FrameRate::Fps30, "test").unwrap(),
            16_934_400_000
        );
        assert_eq!(
            ticks_from_time(Time::from_millis(967), "test").unwrap(),
            245_633_472_000
        );
        assert!(time_from_ticks(-1).is_err());
        assert!(time_from_ticks(i64::MAX).is_ok());
        assert!(ticks_from_time(Time::from_millis(u64::MAX), "test").is_err());
        assert!(
            frame_ticks_from_time(Time::from_millis(u64::MAX), FrameRate::Fps30, "test").is_err()
        );
    }
}
