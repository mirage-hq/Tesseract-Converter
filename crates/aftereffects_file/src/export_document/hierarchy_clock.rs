//! Clock normalization for source-backed native Group precompositions.
//!
//! This is an integration helper for `hierarchy::classify`; it is not active
//! until the shared export core registers it and enforces the returned
//! source-backed-precomposition requirement.

use fx_schema::{
    Duration, GroupLayer, LayerId, LayerPlayback, LayerPlaybackMapping, PropType, Time,
    TimeRangeProperty, animator::AnimationGraphEntry,
};

use super::effective_constant;
use crate::{
    timing::Duration24,
    writer::{AepWriteError, source_clock::SourceClockPlan},
};

/// Source-clock domains established by the hierarchy owner, never inferred
/// from the root composition duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChildClockDomains {
    /// Domain used by child static/default values and source-clock animators.
    pub(crate) default_domain: TimeRangeProperty,
    /// Child lifetime domain reachable by the occurrence clock.
    pub(crate) lifetime_domain: TimeRangeProperty,
}

/// An identity-clock Group view for geometry classification plus the separate
/// occurrence clock that restores the original parent-to-source mapping.
pub(crate) struct HierarchyClockPlan {
    geometry: GroupLayer,
    pub(crate) occurrence_clock: SourceClockPlan,
    pub(crate) source_duration: Duration24,
    pub(crate) source_duration_millis: u64,
}

impl HierarchyClockPlan {
    /// Input for the existing `hierarchy::classify`. The shared caller must
    /// require its precomposition branch; a Null parent never inherits time.
    pub(crate) const fn geometry(&self) -> &GroupLayer {
        &self.geometry
    }
}

/// Separates the Group's source clock from geometry without changing child
/// clocks; identity occurrences need only their finite reachable source domain.
pub(crate) fn plan(
    group: &GroupLayer,
    dynamics: &[AnimationGraphEntry],
    domains: ChildClockDomains,
    audio_only: bool,
) -> Result<HierarchyClockPlan, AepWriteError> {
    let source_duration_millis = checked_source_duration(domains)?;
    let source_duration = if audio_only {
        // Native composition storage is whole-frame, but audio sample durations
        // generally are not. Pad only the empty source tail; keep the original
        // occurrence clock, child ranges and geometry lifetime unchanged.
        let frames = (u128::from(source_duration_millis) * 24).div_ceil(1_000);
        Duration24::from_frames(u32::try_from(frames).map_err(|_| {
            AepWriteError::Invalid("audio precomposition duration exceeds native frame range")
        })?)?
    } else {
        duration24(source_duration_millis)?
    };
    let active_range = group.playback.input_range();
    let occurrence_clock = match group.playback.mapping() {
        LayerPlaybackMapping::Linear { input, output } => {
            let (source_start, source_end) = linear_endpoints(&group.playback, *input, *output)?;
            SourceClockPlan::affine(
                active_range,
                source_start,
                source_end,
                source_duration_millis,
            )?
        }
        LayerPlaybackMapping::TimeRemap { property }
            if super::group_has_root_identity_clock(group, active_range.end()) =>
        {
            SourceClockPlan::affine(
                active_range,
                Time::ZERO,
                Time::from_millis(active_range.duration.as_millis()),
                source_duration_millis,
            )?
        }
        LayerPlaybackMapping::TimeRemap { property } => {
            // Two linear keys on the visible interval can use the affine
            // occurrence path only when no input offset shifts their domain.
            if group.playback.input_offset_ms() == 0
                && let Some(clock) = SourceClockPlan::affine_keyframes(
                    active_range,
                    property,
                    source_duration_millis,
                )
            {
                // FX evaluates the Group's own keys on its content clock, while
                // native occurrence keys use parent-local time. Audio has no
                // Transform and retains the existing affine path.
                if !audio_only && has_any_dynamic_own_transform(dynamics, group.id) {
                    return Err(AepWriteError::Invalid(
                        "keyframed Group source clock cannot rebase Group-owned Transform animation",
                    ));
                }
                clock?
            } else {
                SourceClockPlan::time_remap_with_offset(
                    active_range,
                    property,
                    source_duration_millis,
                    group.playback.input_offset_ms(),
                )?
            }
        }
    };

    // `animator_time_inner` evaluates Group-owned keyframes/JS on the Group's
    // own remapped content clock. AE occurrence Transform keys use only the
    // occurrence start/stretch clock; native Time Remap affects the source,
    // not those occurrence properties. Constants are clock-independent.
    if occurrence_clock.has_time_remap() && has_dynamic_own_transform(dynamics, group.id) {
        return Err(AepWriteError::Invalid(
            "time-remapped Group Transform animation requires source-owned native properties",
        ));
    }

    let mut geometry = group.clone();
    let geometry_range =
        TimeRangeProperty::new(Time::ZERO, Duration::from_millis(source_duration_millis));
    geometry.playback = LayerPlayback::linear(geometry_range, geometry_range, geometry_range, 0)
        .map_err(AepWriteError::Invalid)?;
    Ok(HierarchyClockPlan {
        geometry,
        occurrence_clock,
        source_duration,
        source_duration_millis,
    })
}

fn linear_endpoints(
    playback: &LayerPlayback,
    input: TimeRangeProperty,
    output: TimeRangeProperty,
) -> Result<(Time, Time), AepWriteError> {
    let map = |sample: Time| -> Result<Time, AepWriteError> {
        let shifted = i128::from(sample.as_millis()) + i128::from(playback.input_offset_ms());
        let relative = shifted - i128::from(input.start.as_millis());
        let numerator = relative
            .checked_mul(i128::from(output.duration.as_millis()))
            .ok_or(AepWriteError::Invalid("linear playback mapping overflows"))?;
        let denominator = i128::from(input.duration.as_millis());
        if numerator % denominator != 0 {
            return Err(AepWriteError::Invalid(
                "linear playback maps the visible window to fractional milliseconds",
            ));
        }
        let mapped = i128::from(output.start.as_millis()) + numerator / denominator;
        let mapped = u64::try_from(mapped)
            .map_err(|_| AepWriteError::Invalid("linear playback maps before source zero"))?;
        Ok(Time::from_millis(mapped))
    };
    Ok((
        map(playback.input_range().start)?,
        map(playback.input_range().end())?,
    ))
}

fn checked_source_duration(domains: ChildClockDomains) -> Result<u64, AepWriteError> {
    if domains.default_domain.start != Time::ZERO || domains.lifetime_domain.start != Time::ZERO {
        return Err(AepWriteError::Invalid(
            "precomposition child default/lifetime domains must be source-zero based",
        ));
    }
    let duration = domains
        .default_domain
        .end()
        .max(domains.lifetime_domain.end())
        .as_millis();
    if duration == 0 {
        return Err(AepWriteError::Invalid(
            "precomposition child default/lifetime domains are empty",
        ));
    }
    Ok(duration)
}

fn duration24(milliseconds: u64) -> Result<Duration24, AepWriteError> {
    const TICKS_PER_SECOND: u64 = 24_576;
    const MILLIS_PER_SECOND: u64 = 1_000;

    let tick_numerator = milliseconds
        .checked_mul(TICKS_PER_SECOND)
        .and_then(|value| value.checked_add(MILLIS_PER_SECOND - 1))
        .ok_or(AepWriteError::Invalid(
            "precomposition source duration tick calculation overflowed",
        ))?;
    // A source domain is semantic millisecond time, while AE composition
    // duration is a tick count. Round up by less than one tick so the native
    // container cannot clip the explicit source end. Import rounds back to the
    // same millisecond, so repeated FX -> AEP -> FX conversions do not grow it.
    let ticks = u32::try_from(tick_numerator / MILLIS_PER_SECOND).map_err(|_| {
        AepWriteError::Invalid("precomposition source duration exceeds native tick range")
    })?;
    Ok(Duration24::from_ticks(ticks)?)
}

fn has_dynamic_own_transform(entries: &[AnimationGraphEntry], owner: LayerId) -> bool {
    entries.iter().any(|entry| {
        entry.target.layer_id() == Some(owner)
            && entry.target.as_property().is_some_and(|property| {
                matches!(
                    property.property_type(),
                    PropType::AnchorPointX
                        | PropType::AnchorPointY
                        | PropType::PositionX
                        | PropType::PositionY
                        | PropType::ScaleX
                        | PropType::ScaleY
                        | PropType::Rotation
                        | PropType::Opacity
                )
            })
            && effective_constant(&entry.animator).is_none()
    })
}

/// [`has_dynamic_own_transform`] over every Transform channel, including the 3D
/// and skew channels that the writer checks only for native Time Remap.
fn has_any_dynamic_own_transform(entries: &[AnimationGraphEntry], owner: LayerId) -> bool {
    entries.iter().any(|entry| {
        entry.target.layer_id() == Some(owner)
            && entry
                .target
                .as_property()
                .is_some_and(|property| super::is_transform_property(property.property_type()))
            && effective_constant(&entry.animator).is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_keys_with_input_offset_preserve_source_clock() {
        let group: GroupLayer = serde_json::from_value(serde_json::json!({
            "id": 1, "name": "offset", "layers": [],
            "transform": {
                "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
                "rotation": 0, "opacity": 100
            },
            "playback": {
                "type": "windowed", "inputRange": {"start": 0, "duration": 1000},
                "inputOffsetMs": 500,
                "mapping": {"type": "timeRemap", "property": {
                    "keyframes": [
                        {"id": "a", "time": 0, "value": 0, "easing": {"type": "linear"}},
                        {"id": "b", "time": 2000, "value": 2000, "easing": {"type": "linear"}}
                    ],
                    "before": "inactive", "after": "inactive"
                }}
            }
        }))
        .expect("valid offset group");
        let domain = TimeRangeProperty::new(Time::ZERO, Duration::from_millis(2000));
        let planned = plan(
            &group,
            &[],
            ChildClockDomains {
                default_domain: domain,
                lifetime_domain: domain,
            },
            false,
        )
        .expect("native source-clock plan");
        let LayerPlaybackMapping::TimeRemap { property } = group.playback.mapping() else {
            panic!("expected authored remap");
        };
        assert!(planned.occurrence_clock.has_time_remap());
        assert_eq!(
            planned.occurrence_clock,
            SourceClockPlan::time_remap_with_offset(
                group.playback.input_range(),
                property,
                2000,
                500
            )
            .expect("offset native remap"),
            "identity keys with a nonzero input offset must retain their native remap"
        );
    }

    #[test]
    fn source_duration_is_the_explicit_union_of_default_and_lifetime_domains() {
        let domains = ChildClockDomains {
            default_domain: TimeRangeProperty::new(Time::ZERO, Duration::from_millis(2_000)),
            lifetime_domain: TimeRangeProperty::new(Time::ZERO, Duration::from_millis(3_000)),
        };
        assert_eq!(checked_source_duration(domains).unwrap(), 3_000);
        assert_eq!(duration24(3_000).unwrap().ticks(), 72 * 1_024);
    }

    #[test]
    fn affine_rate_requires_an_exact_millisecond_endpoint_ratio() {
        let input = TimeRangeProperty::new(Time::ZERO, Duration::from_millis(2_000));
        let output = TimeRangeProperty::new(Time::ZERO, Duration::from_millis(3_000));
        let playback = LayerPlayback::linear(input, input, output, 0).unwrap();
        assert_eq!(
            linear_endpoints(&playback, input, output).unwrap(),
            (Time::ZERO, Time::from_millis(3_000))
        );
        let window = TimeRangeProperty::new(Time::ZERO, Duration::from_millis(1_001));
        let playback = LayerPlayback::linear(window, input, output, 0).unwrap();
        assert!(linear_endpoints(&playback, input, output).is_err());
    }

    #[test]
    fn source_duration_never_falls_back_to_a_root_duration() {
        let shifted = ChildClockDomains {
            default_domain: TimeRangeProperty::new(
                Time::from_millis(1),
                Duration::from_millis(1_000),
            ),
            lifetime_domain: TimeRangeProperty::new(Time::ZERO, Duration::from_millis(1_000)),
        };
        assert!(checked_source_duration(shifted).is_err());
    }

    #[test]
    fn non_frame_aligned_source_durations_round_up_with_stable_milliseconds() {
        const SOURCE_DURATIONS: [u64; 7] = [12_551, 2_449, 2_043, 2_026, 2_020, 2_410, 1_001];

        for original_millis in SOURCE_DURATIONS {
            let mut imported_millis = original_millis;
            for _ in 0..3 {
                let ticks = duration24(imported_millis).unwrap().ticks();
                assert_eq!(u64::from(ticks), (imported_millis * 24_576).div_ceil(1_000));
                imported_millis = Duration::from_secs(f64::from(ticks) / 24_576.0).as_millis();
            }
            assert_eq!(imported_millis, original_millis);
        }
    }

    #[test]
    fn source_duration_retains_native_signed_tick_limit() {
        let largest_millis = (i32::MAX as u64 * 1_000) / 24_576;
        assert!(duration24(largest_millis).is_ok());
        assert!(duration24(largest_millis + 1).is_err());
        assert!(duration24(u64::MAX).is_err());
    }

    #[test]
    fn disabled_transform_track_is_clock_independent_effective_constant() {
        use fx_schema::animator::{
            AnimatorData, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeEasing,
            PropertyKeyframeTrack,
        };
        use fx_schema::{PropertyTarget, PropertyValue, TimeOffset};

        let owner = LayerId::new(8_100);
        let track = PropertyKeyframeTrack::new(vec![
            PropertyKeyframe::new(
                KeyframeId::new("disabled-clock-a"),
                TimeOffset::ZERO,
                PropertyValue::Float(10.0),
                PropertyKeyframeEasing::Linear,
            ),
            PropertyKeyframe::new(
                KeyframeId::new("disabled-clock-b"),
                TimeOffset::from_millis(1_000),
                PropertyValue::Float(20.0),
                PropertyKeyframeEasing::Linear,
            ),
        ])
        .unwrap();
        let animator = PropertyAnimator::from_data(&AnimatorData::Keyframes {
            track,
            enabled: false,
            disabled_value: Some(PropertyValue::Float(333.0)),
        })
        .unwrap();
        let entry = AnimationGraphEntry {
            target: PropertyTarget::layer(owner, PropType::PositionX),
            animator,
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };

        assert!(!has_dynamic_own_transform(&[entry], owner));
    }
}
