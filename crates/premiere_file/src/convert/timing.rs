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
