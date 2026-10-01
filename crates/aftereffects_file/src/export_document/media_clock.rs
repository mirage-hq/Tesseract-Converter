//! Exact source-clock planning for current FX media occurrences.
//!
//! This module is intentionally separate from dispatch. The shared export owner
//! must register it and must reject dynamic occurrence-owned Transform tracks
//! when [`MediaClockPlan::requires_source_owned_transform`] is true.

use fx_schema::{
    LayerPlayback, LayerPlaybackMapping, PropertyKeyframeEasing, Time, TimeRangeProperty,
    TimeRemapProperty,
};

use crate::writer::{AepWriteError, source_clock::SourceClockPlan};

/// A finalized moving-media clock and optional static native Time Remap value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MediaClockPlan {
    pub(crate) source_clock: SourceClockPlan,
    /// Source seconds for a static `ADBE Time Remapping` property.
    pub(crate) static_source_time_secs: Option<f64>,
    /// Property animation follows the source clock (native Time Remap, or an
    /// explicit FX remap even when its native clock is affine), so dynamic
    /// occurrence-owned properties cannot be rebased onto it.
    pub(crate) requires_source_owned_transform: bool,
}

pub(crate) fn plan_layer(
    playback: &LayerPlayback,
    source_range: TimeRangeProperty,
    static_source_time_secs: Option<f64>,
    source_duration_millis: u64,
) -> Result<MediaClockPlan, AepWriteError> {
    let active_range = playback.input_range();
    match playback.mapping() {
        LayerPlaybackMapping::Linear { input, output } => {
            let mapped = linear_output_range(playback, *input, *output)?;
            if mapped.start < source_range.start || mapped.end() > source_range.end() {
                return Err(AepWriteError::Invalid(
                    "linear media playback leaves the authored source range",
                ));
            }
            plan(
                active_range,
                mapped,
                None,
                static_source_time_secs,
                source_duration_millis,
            )
        }
        LayerPlaybackMapping::TimeRemap { property } => plan_with_offset(
            active_range,
            source_range,
            Some(property),
            static_source_time_secs,
            source_duration_millis,
            playback.input_offset_ms(),
        ),
    }
}

/// Canonical unit-rate Audio plays through intrinsic EOF; other mappings keep
/// the independent source selection and the existing native remap guards.
pub(crate) fn plan_audio_layer(
    playback: &LayerPlayback,
    source_range: TimeRangeProperty,
    source_duration_millis: u64,
) -> Result<MediaClockPlan, AepWriteError> {
    require_source_range(source_range, source_duration_millis)?;
    match playback.mapping() {
        LayerPlaybackMapping::Linear { input, output } if input.duration == output.duration => {
            let mapped = linear_output_range(playback, *input, *output)?;
            if mapped.start < source_range.start {
                return Err(AepWriteError::Invalid(
                    "linear media playback leaves the authored source range",
                ));
            }
            plan_unit_audio(playback.input_range(), mapped.start, source_duration_millis)
        }
        // Explicit-remap animation samples the mapped source clock. Keep the
        // affine native record, but require source-owned keys: the writer's
        // occurrence-local key rebase would apply the mapping a second time.
        LayerPlaybackMapping::TimeRemap { property } if playback.input_offset_ms() == 0 => {
            plan_audio(
                playback.input_range(),
                source_range,
                Some(property),
                source_duration_millis,
            )
            .map(|plan| MediaClockPlan {
                requires_source_owned_transform: true,
                ..plan
            })
        }
        _ => plan_layer(playback, source_range, None, source_duration_millis),
    }
}

pub(super) fn linear_output_range(
    playback: &LayerPlayback,
    input: TimeRangeProperty,
    output: TimeRangeProperty,
) -> Result<TimeRangeProperty, AepWriteError> {
    let map = |sample: Time| -> Result<Time, AepWriteError> {
        let shifted = i128::from(sample.as_millis()) + i128::from(playback.input_offset_ms());
        let relative = shifted - i128::from(input.start.as_millis());
        let numerator = relative
            .checked_mul(i128::from(output.duration.as_millis()))
            .ok_or(AepWriteError::Invalid(
                "linear media playback mapping overflows",
            ))?;
        let denominator = i128::from(input.duration.as_millis());
        if numerator % denominator != 0 {
            return Err(AepWriteError::Invalid(
                "linear media playback maps the visible window to fractional milliseconds",
            ));
        }
        let mapped = i128::from(output.start.as_millis()) + numerator / denominator;
        let mapped = u64::try_from(mapped)
            .map_err(|_| AepWriteError::Invalid("linear media playback maps before source zero"))?;
        Ok(Time::from_millis(mapped))
    };
    let start = map(playback.input_range().start)?;
    let end = map(playback.input_range().end())?;
    if end <= start {
        return Err(AepWriteError::Invalid(
            "linear media playback must advance through source time",
        ));
    }
    Ok(TimeRangeProperty::new(start, end.saturating_sub(start)))
}

/// Preserve Audio's exact two-key affine remaps and unit-rate EOF policy.
/// Canonical callers validate the independent source selection before this
/// helper; do not infer audio stretch from its persisted duration.
pub(crate) fn plan_audio(
    active_range: TimeRangeProperty,
    source_range: TimeRangeProperty,
    playback: Option<&TimeRemapProperty>,
    source_duration_millis: u64,
) -> Result<MediaClockPlan, AepWriteError> {
    if let Some(property) = playback {
        require_playback_hull_in_source_range(property, source_range)?;
        if let Some(clock) =
            SourceClockPlan::affine_keyframes(active_range, property, source_duration_millis)
        {
            return Ok(MediaClockPlan {
                source_clock: clock?,
                static_source_time_secs: None,
                requires_source_owned_transform: false,
            });
        }
        return plan(
            active_range,
            source_range,
            playback,
            None,
            source_duration_millis,
        );
    }
    plan_unit_audio(active_range, source_range.start, source_duration_millis)
}

fn plan_unit_audio(
    active_range: TimeRangeProperty,
    source_start: Time,
    source_duration_millis: u64,
) -> Result<MediaClockPlan, AepWriteError> {
    // The mixer is silent after EOF, not stretched to fill the active span.
    // Retain the audible prefix rather than omitting the whole occurrence.
    let available = source_duration_millis.saturating_sub(source_start.as_millis());
    let audible_duration = active_range
        .duration
        .min(fx_schema::Duration::from_millis(available));
    let audible_range = TimeRangeProperty::new(active_range.start, audible_duration);
    let effective_range = TimeRangeProperty::new(source_start, audible_duration);
    plan(
        audible_range,
        effective_range,
        None,
        None,
        source_duration_millis,
    )
}

/// Plans the exact current media mapping without endpoint extrapolation.
pub(crate) fn plan(
    active_range: TimeRangeProperty,
    source_range: TimeRangeProperty,
    playback: Option<&TimeRemapProperty>,
    static_source_time_secs: Option<f64>,
    source_duration_millis: u64,
) -> Result<MediaClockPlan, AepWriteError> {
    plan_with_offset(
        active_range,
        source_range,
        playback,
        static_source_time_secs,
        source_duration_millis,
        0,
    )
}

fn plan_with_offset(
    active_range: TimeRangeProperty,
    source_range: TimeRangeProperty,
    playback: Option<&TimeRemapProperty>,
    static_source_time_secs: Option<f64>,
    source_duration_millis: u64,
    input_offset_ms: i64,
) -> Result<MediaClockPlan, AepWriteError> {
    require_source_range(source_range, source_duration_millis)?;
    if playback.is_some() && static_source_time_secs.is_some() {
        return Err(AepWriteError::Invalid(
            "media playback and static source Time Remap cannot both own the source clock",
        ));
    }

    if let Some(property) = playback {
        require_playback_hull_in_source_range(property, source_range)?;
        return Ok(MediaClockPlan {
            source_clock: SourceClockPlan::time_remap_with_offset(
                active_range,
                property,
                source_duration_millis,
                input_offset_ms,
            )?,
            static_source_time_secs: None,
            requires_source_owned_transform: true,
        });
    }

    if let Some(source_secs) = static_source_time_secs {
        let source_limit_secs = source_duration_millis as f64 / 1_000.0;
        let source_start_secs = source_range.start.as_millis() as f64 / 1_000.0;
        let source_end_secs = source_range.end().as_millis() as f64 / 1_000.0;
        if !source_secs.is_finite()
            || !(0.0..=source_limit_secs).contains(&source_secs)
            || !(source_start_secs..=source_end_secs).contains(&source_secs)
            || active_range.duration.as_millis() > source_duration_millis
        {
            return Err(AepWriteError::Invalid(
                "static source Time Remap is outside the authored/source domain",
            ));
        }
        return Ok(MediaClockPlan {
            // Static Time Remap owns the sampled source value. This affine plan
            // establishes only an exact occurrence-local property clock.
            source_clock: SourceClockPlan::affine(
                active_range,
                Time::ZERO,
                Time::from_millis(active_range.duration.as_millis()),
                source_duration_millis,
            )?,
            static_source_time_secs: Some(source_secs),
            requires_source_owned_transform: true,
        });
    }

    Ok(MediaClockPlan {
        source_clock: SourceClockPlan::affine(
            active_range,
            source_range.start,
            source_range.end(),
            source_duration_millis,
        )?,
        static_source_time_secs: None,
        requires_source_owned_transform: false,
    })
}

fn require_source_range(
    source_range: TimeRangeProperty,
    source_duration_millis: u64,
) -> Result<(), AepWriteError> {
    if source_range.duration.as_millis() == 0
        || source_range.end().as_millis() > source_duration_millis
    {
        return Err(AepWriteError::Invalid(
            "media source range is empty or exceeds the interpreted source duration",
        ));
    }
    Ok(())
}

fn require_playback_hull_in_source_range(
    property: &TimeRemapProperty,
    source_range: TimeRangeProperty,
) -> Result<(), AepWriteError> {
    let start = source_range.start.as_millis() as f64;
    let end = source_range.end().as_millis() as f64;
    let keys = property.keyframes();
    for (index, key) in keys.iter().enumerate() {
        let value = key.value.as_millis() as f64;
        if !(start..=end).contains(&value) {
            return Err(AepWriteError::Invalid(
                "media Time Remap key leaves the authored source range",
            ));
        }
        if index == 0 {
            continue;
        }
        let from = keys[index - 1].value.as_millis() as f64;
        let controls = match key.easing {
            PropertyKeyframeEasing::Hold | PropertyKeyframeEasing::Linear => {
                [from, from, value, value]
            }
            PropertyKeyframeEasing::CubicBezier { y1, y2, .. } => {
                let delta = value - from;
                [from, from + delta * y1, from + delta * y2, value]
            }
        };
        if controls
            .into_iter()
            .any(|control| !control.is_finite() || !(start..=end).contains(&control))
        {
            return Err(AepWriteError::Invalid(
                "media Time Remap control hull leaves the authored source range",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use fx_schema::{Duration, KeyframeId, TimeRemapExtrapolation, TimeRemapKeyframe};

    use super::*;

    fn range(start: u64, duration: u64) -> TimeRangeProperty {
        TimeRangeProperty::new(Time::from_millis(start), Duration::from_millis(duration))
    }

    fn remap(points: &[(u64, u64)]) -> TimeRemapProperty {
        TimeRemapProperty::new(
            points
                .iter()
                .enumerate()
                .map(|(index, &(time, value))| TimeRemapKeyframe {
                    id: KeyframeId::new(format!("k{index}")),
                    time: Time::from_millis(time),
                    value: Time::from_millis(value),
                    easing: PropertyKeyframeEasing::Linear,
                })
                .collect(),
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap()
    }

    #[test]
    fn affine_media_clock_preserves_trim_and_stretch_exactly() {
        let planned = plan(range(1_000, 2_000), range(500, 1_000), None, None, 4_000).unwrap();
        assert_eq!(planned.source_clock.record.stretch.numerator, 2);
        assert_eq!(planned.source_clock.record.in_point.numerator, 1);
        assert_eq!(planned.source_clock.record.in_point.denominator, 2);
        assert!(!planned.requires_source_owned_transform);
    }

    #[test]
    fn playback_keeps_strict_guard_keys_and_source_range_hull() {
        let accepted = remap(&[(0, 500), (1_250, 750), (1_750, 1_000), (3_000, 1_250)]);
        assert!(
            plan(
                range(1_000, 1_000),
                range(500, 1_000),
                Some(&accepted),
                None,
                2_000
            )
            .is_ok()
        );

        let endpoint_dependent = remap(&[(1_000, 500), (2_000, 1_500)]);
        assert!(
            plan(
                range(1_000, 1_000),
                range(500, 1_000),
                Some(&endpoint_dependent),
                None,
                2_000
            )
            .is_err()
        );
    }

    #[test]
    fn static_source_time_is_single_owned_and_source_bounded() {
        let planned = plan(
            range(500, 1_000),
            range(250, 1_500),
            None,
            Some(0.75),
            2_000,
        )
        .unwrap();
        assert_eq!(planned.static_source_time_secs, Some(0.75));
        assert!(planned.requires_source_owned_transform);
        assert!(plan(range(500, 1_000), range(250, 1_500), None, Some(2.5), 2_000).is_err());
    }

    fn record_clock(plan: &MediaClockPlan) -> [(i32, u32); 4] {
        let record = &plan.source_clock.record;
        [
            record.start_time,
            record.in_point,
            record.out_point,
            record.stretch,
        ]
        .map(|value| (value.numerator, value.denominator))
    }

    #[test]
    fn canonical_unit_audio_maps_independent_windows_and_offsets_before_eof_clipping() {
        for (offset, start, duration, record) in [
            (-100, 600, 3_400, [(3, 5), (3, 5), (4, 1), (1, 1)]),
            (100, 800, 3_200, [(2, 5), (4, 5), (4, 1), (1, 1)]),
        ] {
            let playback = LayerPlayback::linear(
                range(1_200, 5_000),
                range(1_000, 8_000),
                range(500, 8_000),
                offset,
            )
            .unwrap();
            let planned = plan_audio_layer(&playback, range(500, 1_000), 4_000).unwrap();
            assert_eq!(record_clock(&planned), record);
            assert_eq!(planned.source_clock.active_range, range(1_200, duration));
            assert_eq!(planned.source_clock.source_time_millis(0).unwrap(), start);
            assert_eq!(
                planned
                    .source_clock
                    .source_time_millis(duration as i64)
                    .unwrap(),
                4_000
            );
            assert!(!planned.source_clock.has_time_remap());
            assert!(!planned.requires_source_owned_transform);
        }
    }

    #[test]
    fn canonical_audio_requires_source_lower_bound_and_valid_selection_before_shortcuts() {
        let before_selection = LayerPlayback::linear(
            range(1_000, 2_000),
            range(1_000, 2_000),
            range(400, 2_000),
            0,
        )
        .unwrap();
        assert!(matches!(
            plan_audio_layer(&before_selection, range(500, 1_000), 4_000),
            Err(AepWriteError::Invalid(
                "linear media playback leaves the authored source range"
            ))
        ));
        let linear = LayerPlayback::linear(
            range(1_000, 2_000),
            range(1_000, 2_000),
            range(500, 2_000),
            0,
        )
        .unwrap();
        let keyed = LayerPlayback::remapped(
            range(1_000, 2_000),
            remap(&[(1_000, 500), (3_000, 1_500)]),
            0,
        )
        .unwrap();
        for source in [range(500, 0), range(500, 4_000)] {
            for playback in [&linear, &keyed] {
                assert!(matches!(
                    plan_audio_layer(playback, source, 4_000),
                    Err(AepWriteError::Invalid(
                        "media source range is empty or exceeds the interpreted source duration"
                    ))
                ));
            }
        }
    }

    #[test]
    fn nonunit_audio_linear_keeps_source_selection_and_fractional_mapping_guards() {
        let playback = LayerPlayback::linear(
            range(1_000, 2_000),
            range(1_000, 2_000),
            range(500, 1_000),
            0,
        )
        .unwrap();
        let planned = plan_audio_layer(&playback, range(500, 1_000), 4_000).unwrap();
        assert_eq!(record_clock(&planned), [(0, 1), (1, 2), (3, 2), (2, 1)]);
        let outside_selection = LayerPlayback::linear(
            range(1_000, 2_000),
            range(1_000, 2_000),
            range(500, 1_500),
            0,
        )
        .unwrap();
        assert!(matches!(
            plan_audio_layer(&outside_selection, range(500, 1_000), 4_000),
            Err(AepWriteError::Invalid(
                "linear media playback leaves the authored source range"
            ))
        ));
        let fractional =
            LayerPlayback::linear(range(1_001, 1), range(1_000, 3), range(500, 1), 0).unwrap();
        assert!(matches!(
            plan_audio_layer(&fractional, range(500, 1_000), 4_000),
            Err(AepWriteError::Invalid(
                "linear media playback maps the visible window to fractional milliseconds"
            ))
        ));
    }

    #[test]
    fn canonical_keyed_audio_keeps_affine_records_and_full_source_control_hull_guards() {
        let playback = LayerPlayback::remapped(
            range(1_000, 2_000),
            remap(&[(1_000, 500), (3_000, 1_500)]),
            0,
        )
        .unwrap();
        let planned = plan_audio_layer(&playback, range(500, 1_000), 4_000).unwrap();
        assert_eq!(record_clock(&planned), [(0, 1), (1, 2), (3, 2), (2, 1)]);
        assert!(!planned.source_clock.has_time_remap());
        assert!(planned.requires_source_owned_transform);
        let outside_selection = LayerPlayback::remapped(
            range(1_000, 2_000),
            remap(&[(1_000, 600), (3_000, 1_600)]),
            0,
        )
        .unwrap();
        assert!(matches!(
            plan_audio_layer(&outside_selection, range(500, 1_000), 4_000),
            Err(AepWriteError::Invalid(
                "media Time Remap key leaves the authored source range"
            ))
        ));
        let mut keys = remap(&[(0, 500), (4_000, 1_500)]).keyframes().to_vec();
        keys[1].easing = PropertyKeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: 0.2,
            x2: 0.7,
            y2: 0.8,
        };
        let property = TimeRemapProperty::new(
            keys,
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap();
        for offset in [0, 500] {
            let playback =
                LayerPlayback::remapped(range(1_000, 1_000), property.clone(), offset).unwrap();
            let planned = plan_audio_layer(&playback, range(500, 1_000), 4_000).unwrap();
            assert!(planned.source_clock.has_time_remap());
            assert!(planned.requires_source_owned_transform);
            // Canonical easing controls lie in [0,1], so this curve's full
            // control hull stays inside its authored key-value bounds.
            assert!(matches!(
                plan_audio_layer(&playback, range(500, 900), 4_000),
                Err(AepWriteError::Invalid(
                    "media Time Remap key leaves the authored source range"
                ))
            ));
        }
    }

    #[test]
    fn nonzero_audio_remap_offset_retains_native_keys_and_shifted_span_guards() {
        let playback =
            LayerPlayback::remapped(range(1_000, 1_000), remap(&[(0, 500), (4_000, 2_500)]), 500)
                .unwrap();
        let planned = plan_audio_layer(&playback, range(500, 2_000), 4_000).unwrap();
        assert!(planned.source_clock.has_time_remap());
        assert!(planned.requires_source_owned_transform);
        assert_eq!(record_clock(&planned), [(1, 1), (0, 1), (1, 1), (1, 1)]);
        assert_eq!(planned.source_clock.active_range, range(1_000, 1_000));
        // These endpoints match visibility only before the offset. An affine
        // shortcut would incorrectly bypass the retained native key-span guard.
        let shifted_past_keys = LayerPlayback::remapped(
            range(1_000, 2_000),
            remap(&[(1_000, 500), (3_000, 1_500)]),
            500,
        )
        .unwrap();
        assert!(plan_audio_layer(&shifted_past_keys, range(500, 2_000), 4_000).is_err());
    }
}
