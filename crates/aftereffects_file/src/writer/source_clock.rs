//! Bounded fresh-record clocks for source-backed precomposition occurrences.
//!
//! Registration and the `FreshLayerRecord` setters are owned by the shared
//! writer core. This module deliberately returns typed record fields instead
//! of patching an `ldta` byte image.

use fx_schema::{PropertyKeyframeEasing, Time, TimeRangeProperty, TimeRemapProperty};

use crate::rifx::Chunk;

pub(crate) use crate::schema::layer_records::{FreshLayerClockFields, NativeRational};

use super::{AepWriteError, KeyframeEasing, NumericKeyframe, NumericTrack, views};

const MILLIS_PER_SECOND: i128 = 1_000;

/// Exact occurrence clock plus an optional authored native Time Remap track.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceClockPlan {
    pub(crate) record: FreshLayerClockFields,
    pub(crate) active_range: TimeRangeProperty,
    pub(crate) source_duration_millis: u64,
    time_remap: Option<NumericTrack>,
}

impl SourceClockPlan {
    /// Plans a positive affine parent-to-source mapping from two exact endpoint
    /// pairs. AE's relation is `parent = start + source * stretch`.
    pub(crate) fn affine(
        active_range: TimeRangeProperty,
        source_start: Time,
        source_end: Time,
        source_duration_millis: u64,
    ) -> Result<Self, AepWriteError> {
        let parent_delta = i128::from(active_range.duration.as_millis());
        let source_start_millis = i128::from(source_start.as_millis());
        let source_end_millis = i128::from(source_end.as_millis());
        let source_delta = source_end_millis - source_start_millis;
        if parent_delta <= 0 || source_delta <= 0 {
            return Err(AepWriteError::Invalid(
                "affine precomposition clock requires positive parent and source spans",
            ));
        }
        if source_end.as_millis() > source_duration_millis {
            return Err(AepWriteError::Invalid(
                "affine precomposition clock exceeds the explicit source duration",
            ));
        }

        let stretch = NativeRational::new(parent_delta, source_delta)?;
        let start_numerator = i128::from(active_range.start.as_millis())
            .checked_mul(source_delta)
            .and_then(|value| value.checked_sub(source_start_millis.checked_mul(parent_delta)?))
            .ok_or(AepWriteError::Invalid(
                "affine precomposition start-time calculation overflowed",
            ))?;
        let start_denominator =
            MILLIS_PER_SECOND
                .checked_mul(source_delta)
                .ok_or(AepWriteError::Invalid(
                    "affine precomposition start-time denominator overflowed",
                ))?;
        let record = FreshLayerClockFields {
            stretch,
            start_time: NativeRational::new(start_numerator, start_denominator)?,
            in_point: NativeRational::milliseconds(source_start_millis)?,
            out_point: NativeRational::milliseconds(source_end_millis)?,
        };
        Ok(Self {
            record,
            active_range,
            source_duration_millis,
            time_remap: None,
        })
    }

    /// Recognize a two-key linear map covering exactly the active interval.
    /// An affine native record avoids unsupported Time Remap extrapolation and
    /// key tick rounding, including millisecond-truncated imported audio ends.
    pub(crate) fn affine_keyframes(
        active_range: TimeRangeProperty,
        property: &TimeRemapProperty,
        source_duration_millis: u64,
    ) -> Option<Result<Self, AepWriteError>> {
        let [first, last] = property.keyframes() else {
            return None;
        };
        // TimeRemapKeyframe.easing describes the segment ARRIVING at that key.
        // The first easing is irrelevant to the segment first -> last; using it
        // here would silently turn an incoming Hold/Bezier segment into affine.
        if first.time != active_range.start
            || last.time != active_range.end()
            || last.easing != PropertyKeyframeEasing::Linear
        {
            return None;
        }
        Some(Self::affine(
            active_range,
            first.value,
            last.value,
            source_duration_millis,
        ))
    }

    /// Plans a bounded native Time Remap without relying on AE's unknown
    /// endpoint extrapolation, minimum, or clamp behavior.
    #[cfg(test)]
    pub(crate) fn time_remap(
        active_range: TimeRangeProperty,
        property: &TimeRemapProperty,
        source_duration_millis: u64,
    ) -> Result<Self, AepWriteError> {
        Self::time_remap_with_clock(
            active_range,
            property,
            source_duration_millis,
            super::keyframes::PropertyClock::DEFAULT,
        )
    }

    pub(crate) fn time_remap_with_offset(
        active_range: TimeRangeProperty,
        property: &TimeRemapProperty,
        source_duration_millis: u64,
        input_offset_ms: i64,
    ) -> Result<Self, AepWriteError> {
        Self::time_remap_with_clock_and_offset(
            active_range,
            property,
            source_duration_millis,
            super::keyframes::PropertyClock::DEFAULT,
            input_offset_ms,
        )
    }

    #[cfg(test)]
    pub(super) fn time_remap_with_clock(
        active_range: TimeRangeProperty,
        property: &TimeRemapProperty,
        source_duration_millis: u64,
        clock: super::keyframes::PropertyClock,
    ) -> Result<Self, AepWriteError> {
        Self::time_remap_with_clock_and_offset(
            active_range,
            property,
            source_duration_millis,
            clock,
            0,
        )
    }

    fn time_remap_with_clock_and_offset(
        active_range: TimeRangeProperty,
        property: &TimeRemapProperty,
        source_duration_millis: u64,
        clock: super::keyframes::PropertyClock,
        input_offset_ms: i64,
    ) -> Result<Self, AepWriteError> {
        if active_range.duration.as_millis() == 0 {
            return Err(AepWriteError::Invalid(
                "time-remapped precomposition active range is empty",
            ));
        }
        let keys = property.keyframes();
        let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
            return Err(AepWriteError::Invalid(
                "native Time Remap requires at least two authored keys",
            ));
        };
        if keys.len() < 2 {
            return Err(AepWriteError::Invalid(
                "one-key native Time Remap endpoint behavior is not established",
            ));
        }
        let mapped_start = i128::from(active_range.start.as_millis()) + i128::from(input_offset_ms);
        let mapped_end = i128::from(active_range.end().as_millis()) + i128::from(input_offset_ms);
        if i128::from(first.time.as_millis()) >= mapped_start
            || i128::from(last.time.as_millis()) <= mapped_end
        {
            return Err(AepWriteError::Invalid(
                "native Time Remap active interval must lie strictly inside the authored key span",
            ));
        }

        let active_start = i128::from(active_range.start.as_millis());
        let source_limit = source_duration_millis as f64;
        let mut native_keys = Vec::with_capacity(keys.len());
        for (index, key) in keys.iter().enumerate() {
            if key.value.as_millis() > source_duration_millis {
                return Err(AepWriteError::Invalid(
                    "native Time Remap key exceeds the explicit source duration",
                ));
            }
            if index > 0
                && !segment_control_hull_is_bounded(
                    keys[index - 1].value.as_millis() as f64,
                    key.value.as_millis() as f64,
                    key.easing,
                    source_limit,
                )
            {
                return Err(AepWriteError::Invalid(
                    "native Time Remap control hull can leave the explicit source domain",
                ));
            }
            let local_millis = i128::from(key.time.as_millis()) - mapped_start;
            let local_millis = i64::try_from(local_millis).map_err(|_| {
                AepWriteError::Invalid("native Time Remap key exceeds the signed clock range")
            })?;
            require_exact_native_key_time(local_millis, clock)?;
            native_keys.push(NumericKeyframe {
                time_millis: local_millis,
                values: vec![key.value.as_millis() as f64 / 1_000.0],
                easing: vec![native_easing(key.easing)?],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            });
        }

        let record = FreshLayerClockFields {
            stretch: NativeRational::new(1, 1)?,
            start_time: NativeRational::milliseconds(active_start)?,
            in_point: NativeRational::new(0, 1)?,
            out_point: NativeRational::milliseconds(i128::from(active_range.duration.as_millis()))?,
        };
        Ok(Self {
            record,
            active_range,
            source_duration_millis,
            time_remap: Some(NumericTrack { keys: native_keys }),
        })
    }

    #[must_use]
    pub(crate) const fn has_time_remap(&self) -> bool {
        self.time_remap.is_some()
    }

    /// Rebases one occurrence-local property time through the finalized affine
    /// record clock. Native Time Remap callers must reject occurrence-owned
    /// dynamic properties separately because that mapping is nonlinear.
    pub(crate) fn source_time_millis(&self, occurrence_millis: i64) -> Result<i64, AepWriteError> {
        let stretch_numerator = i128::from(self.record.stretch.numerator);
        if stretch_numerator <= 0 {
            return Err(AepWriteError::Invalid(
                "property-key rebase requires positive native stretch",
            ));
        }
        let in_denominator = i128::from(self.record.in_point.denominator);
        let numerator = i128::from(self.record.in_point.numerator)
            .checked_mul(MILLIS_PER_SECOND)
            .and_then(|value| value.checked_mul(stretch_numerator))
            .and_then(|value| {
                i128::from(occurrence_millis)
                    .checked_mul(i128::from(self.record.stretch.denominator))
                    .and_then(|delta| delta.checked_mul(in_denominator))
                    .and_then(|delta| value.checked_add(delta))
            })
            .ok_or(AepWriteError::Invalid(
                "property-key clock rebase overflowed",
            ))?;
        let denominator =
            in_denominator
                .checked_mul(stretch_numerator)
                .ok_or(AepWriteError::Invalid(
                    "property-key clock denominator overflowed",
                ))?;
        if numerator % denominator != 0 {
            return Err(AepWriteError::Invalid(
                "property-key time is not exact in the finalized native source clock",
            ));
        }
        i64::try_from(numerator / denominator)
            .map_err(|_| AepWriteError::Invalid("property-key time exceeds native range"))
    }

    /// Rebases every key in one native track before property serialization.
    pub(crate) fn rebase_track_times(&self, track: &mut NumericTrack) -> Result<(), AepWriteError> {
        for key in &mut track.keys {
            key.time_millis = self.source_time_millis(key.time_millis)?;
        }
        Ok(())
    }

    /// Builds the known scalar animated-property grammar. Animated leaves omit
    /// `cdat`, so no static base value can become stale.
    #[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
    pub(crate) fn time_remap_property(&self) -> Result<Option<Chunk>, AepWriteError> {
        self.time_remap_property_with_clock(super::keyframes::PropertyClock::DEFAULT)
    }

    pub(super) fn time_remap_property_with_clock(
        &self,
        clock: super::keyframes::PropertyClock,
    ) -> Result<Option<Chunk>, AepWriteError> {
        self.time_remap
            .as_ref()
            .map(|track| {
                views::property_with_clock(views::ValueKind::Scalar, &[], None, Some(track), clock)
                    .map_err(AepWriteError::from)
            })
            .transpose()
    }

    /// Appends only the typed Time Remap property to a freshly constructed
    /// layer. The shared core must apply [`Self::record`] through its typed
    /// fresh-record setter before calling this helper.
    #[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
    pub(crate) fn append_time_remap_property(
        &self,
        layer: &mut Chunk,
    ) -> Result<(), AepWriteError> {
        self.append_time_remap_property_with_clock(layer, super::keyframes::PropertyClock::DEFAULT)
    }

    pub(super) fn append_time_remap_property_with_clock(
        &self,
        layer: &mut Chunk,
        clock: super::keyframes::PropertyClock,
    ) -> Result<(), AepWriteError> {
        let Some(property) = self.time_remap_property_with_clock(clock)? else {
            return Ok(());
        };
        let layer_children = layer.children_mut().ok_or(AepWriteError::Invalid(
            "source-clock target layer is not a fresh LIST",
        ))?;
        let mut roots = layer_children
            .iter_mut()
            .filter(|child| child.list_kind() == Some(*b"tdgp"));
        let root = roots.next().ok_or(AepWriteError::Invalid(
            "source-clock target layer has no property root",
        ))?;
        if roots.next().is_some() {
            return Err(AepWriteError::Invalid(
                "source-clock target layer has duplicate property roots",
            ));
        }
        let children = root.children_mut().ok_or(AepWriteError::Invalid(
            "source-clock property root is opaque",
        ))?;
        let end = children.last().ok_or(AepWriteError::Invalid(
            "source-clock property root is empty",
        ))?;
        if end.id() != *b"tdmn"
            || end.data_payload().is_none_or(|bytes| {
                let end = bytes
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(bytes.len());
                &bytes[..end] != b"ADBE Group End"
            })
        {
            return Err(AepWriteError::Invalid(
                "source-clock property root has no terminal group marker",
            ));
        }
        let index = children.len() - 1;
        children.insert(index, match_name("ADBE Time Remapping")?);
        children.insert(index + 1, property);
        Ok(())
    }
}

impl NativeRational {
    fn new(numerator: i128, denominator: i128) -> Result<Self, AepWriteError> {
        if denominator <= 0 {
            return Err(AepWriteError::Invalid(
                "native source-clock rational has a nonpositive denominator",
            ));
        }
        let divisor = gcd(numerator.unsigned_abs(), denominator as u128);
        let numerator = numerator / divisor as i128;
        let denominator = denominator / divisor as i128;
        Ok(Self {
            numerator: i32::try_from(numerator)
                .map_err(|_| AepWriteError::Invalid("native source-clock numerator exceeds i32"))?,
            denominator: u32::try_from(denominator).map_err(|_| {
                AepWriteError::Invalid("native source-clock denominator exceeds u32")
            })?,
        })
    }

    fn milliseconds(milliseconds: i128) -> Result<Self, AepWriteError> {
        Self::new(milliseconds, MILLIS_PER_SECOND)
    }
}

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn require_exact_native_key_time(
    milliseconds: i64,
    clock: super::keyframes::PropertyClock,
) -> Result<(), AepWriteError> {
    let scaled = i128::from(milliseconds)
        .checked_mul(i128::from(clock.ticks()))
        .ok_or(AepWriteError::Invalid(
            "native Time Remap key tick calculation overflowed",
        ))?;
    if scaled % MILLIS_PER_SECOND != 0 || i32::try_from(scaled / MILLIS_PER_SECOND).is_err() {
        return Err(AepWriteError::Invalid(
            "native Time Remap key time is not exact in the established tick grammar",
        ));
    }
    Ok(())
}

fn native_easing(easing: PropertyKeyframeEasing) -> Result<KeyframeEasing, AepWriteError> {
    match easing {
        PropertyKeyframeEasing::Hold => Ok(KeyframeEasing::Hold),
        PropertyKeyframeEasing::Linear => Ok(KeyframeEasing::Linear),
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 }
            if [x1, y1, x2, y2].into_iter().all(f64::is_finite)
                && (0.0..=1.0).contains(&x1)
                && (0.0..=1.0).contains(&x2)
                && (x1 != 0.0 || y1 == 0.0)
                && (x2 != 1.0 || y2 == 1.0) =>
        {
            Ok(KeyframeEasing::CubicBezier { x1, y1, x2, y2 })
        }
        PropertyKeyframeEasing::CubicBezier { .. } => Err(AepWriteError::Invalid(
            "Time Remap cubic easing is not exact in the established native key grammar",
        )),
    }
}

fn segment_control_hull_is_bounded(
    from: f64,
    to: f64,
    easing: PropertyKeyframeEasing,
    source_limit: f64,
) -> bool {
    let controls = match easing {
        PropertyKeyframeEasing::Hold | PropertyKeyframeEasing::Linear => [from, from, to, to],
        PropertyKeyframeEasing::CubicBezier { y1, y2, .. } => {
            let delta = to - from;
            [from, from + delta * y1, from + delta * y2, to]
        }
    };
    controls
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=source_limit).contains(&value))
}

fn match_name(value: &str) -> Result<Chunk, AepWriteError> {
    if value.len() > 40 || !value.is_ascii() {
        return Err(AepWriteError::Invalid(
            "invalid source-clock property match name",
        ));
    }
    let mut bytes = [0_u8; 40];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    Ok(Chunk::data(*b"tdmn", bytes)?)
}

#[cfg(test)]
mod tests {
    use fx_schema::{Duration, KeyframeId, TimeRemapExtrapolation, TimeRemapKeyframe};

    use super::*;

    fn key(
        id: &str,
        time_millis: u64,
        value_millis: u64,
        easing: PropertyKeyframeEasing,
    ) -> TimeRemapKeyframe {
        TimeRemapKeyframe {
            id: KeyframeId::new(id),
            time: Time::from_millis(time_millis),
            value: Time::from_millis(value_millis),
            easing,
        }
    }

    fn remap(keys: Vec<TimeRemapKeyframe>) -> TimeRemapProperty {
        TimeRemapProperty::new(
            keys,
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap()
    }

    #[test]
    fn remap_offsets_shift_native_key_times_without_changing_values_or_visibility() {
        let property = remap(vec![
            key("first", 0, 500, PropertyKeyframeEasing::Linear),
            key("last", 4_000, 3_500, PropertyKeyframeEasing::Linear),
        ]);
        let range = TimeRangeProperty::new(Time::from_millis(1_500), Duration::from_millis(1_000));
        for offset in [-1_000, 1_000] {
            let plan =
                SourceClockPlan::time_remap_with_offset(range, &property, 4_000, offset).unwrap();
            assert_eq!(plan.active_range, range);
            assert_eq!(
                plan.record.start_time,
                NativeRational::milliseconds(1_500).unwrap()
            );
            let keys = &plan.time_remap.as_ref().unwrap().keys;
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0].time_millis, -1_500 - offset);
            assert_eq!(keys[1].time_millis, 2_500 - offset);
            assert_eq!(keys[0].values, vec![0.5]);
            assert_eq!(keys[1].values, vec![3.5]);
        }
        assert!(SourceClockPlan::time_remap_with_offset(range, &property, 4_000, 1_500).is_err());
    }

    #[test]
    fn affine_plan_exposes_exact_fresh_record_rationals() {
        let plan = SourceClockPlan::affine(
            TimeRangeProperty::new(Time::from_millis(1_000), Duration::from_millis(2_000)),
            Time::from_millis(500),
            Time::from_millis(1_500),
            2_000,
        )
        .unwrap();

        assert_eq!(
            plan.record.stretch,
            NativeRational {
                numerator: 2,
                denominator: 1
            }
        );
        assert_eq!(
            plan.record.start_time,
            NativeRational {
                numerator: 0,
                denominator: 1
            }
        );
        assert_eq!(
            plan.record.in_point,
            NativeRational {
                numerator: 1,
                denominator: 2
            }
        );
        assert_eq!(
            plan.record.out_point,
            NativeRational {
                numerator: 3,
                denominator: 2
            }
        );
        assert!(!plan.has_time_remap());
        assert_eq!(plan.source_time_millis(0).unwrap(), 500);
        assert_eq!(plan.source_time_millis(1_000).unwrap(), 1_000);
    }

    #[test]
    fn affine_keyframes_uses_arriving_last_easing_not_departing_first() {
        let active = TimeRangeProperty::new(Time::from_millis(1_000), Duration::from_millis(2_000));
        let first_hold_last_linear = remap(vec![
            key("a", 1_000, 500, PropertyKeyframeEasing::Hold),
            key("b", 3_000, 1_500, PropertyKeyframeEasing::Linear),
        ]);
        assert!(
            SourceClockPlan::affine_keyframes(active, &first_hold_last_linear, 2_000)
                .unwrap()
                .is_ok()
        );

        let first_linear_last_hold = remap(vec![
            key("a", 1_000, 500, PropertyKeyframeEasing::Linear),
            key("b", 3_000, 1_500, PropertyKeyframeEasing::Hold),
        ]);
        assert!(
            SourceClockPlan::affine_keyframes(active, &first_linear_last_hold, 2_000).is_none()
        );
    }

    #[test]
    fn time_remap_requires_guard_keys_and_exact_native_key_times() {
        let active = TimeRangeProperty::new(Time::from_millis(1_000), Duration::from_millis(1_000));
        let guarded = remap(vec![
            key("a", 0, 0, PropertyKeyframeEasing::Linear),
            key("b", 1_250, 500, PropertyKeyframeEasing::Linear),
            key("c", 1_750, 750, PropertyKeyframeEasing::Linear),
            key("d", 3_000, 1_000, PropertyKeyframeEasing::Linear),
        ]);
        assert!(SourceClockPlan::time_remap(active, &guarded, 2_000).is_ok());

        let endpoint_dependent = remap(vec![
            key("a", 1_000, 0, PropertyKeyframeEasing::Linear),
            key("b", 2_000, 1_000, PropertyKeyframeEasing::Linear),
        ]);
        assert!(SourceClockPlan::time_remap(active, &endpoint_dependent, 2_000).is_err());

        let inexact_tick = remap(vec![
            key("a", 1, 0, PropertyKeyframeEasing::Linear),
            key("b", 1_251, 500, PropertyKeyframeEasing::Linear),
            key("c", 3_001, 1_000, PropertyKeyframeEasing::Linear),
        ]);
        assert!(SourceClockPlan::time_remap(active, &inexact_tick, 2_000).is_err());
    }

    #[test]
    fn thirty_fps_time_remap_changes_key_clock_not_source_values() {
        let plan = SourceClockPlan::time_remap_with_clock(
            TimeRangeProperty::new(Time::from_millis(1_000), Duration::from_millis(1_000)),
            &remap(vec![
                key("a", 0, 250, PropertyKeyframeEasing::Hold),
                key("b", 1_100, 700, PropertyKeyframeEasing::Hold),
                key("c", 3_000, 1_700, PropertyKeyframeEasing::Hold),
            ]),
            3000,
            super::super::keyframes::PropertyClock::for_rate(
                crate::timing::FrameRate::new(30.0).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let property = plan.time_remap_property_with_clock(clock).unwrap().unwrap();
        fn collect(chunk: &Chunk, descriptors: &mut Vec<u32>, keys: &mut Vec<i32>) {
            if chunk.id() == *b"tdb4" {
                let bytes = chunk.data_payload().unwrap();
                descriptors.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
            }
            if chunk.id() == *b"ldat" {
                let bytes = chunk.data_payload().unwrap();
                keys.extend(
                    bytes
                        .chunks_exact(48)
                        .map(|item| i32::from_be_bytes(item[..4].try_into().unwrap())),
                );
            }
            if let Some(children) = chunk.children() {
                for child in children {
                    collect(child, descriptors, keys);
                }
            }
        }
        let mut descriptors = Vec::new();
        let mut keys = Vec::new();
        collect(&property, &mut descriptors, &mut keys);
        assert_eq!(descriptors, vec![30_720]);
        assert!(
            keys.contains(&3072),
            "1100ms occurrence key becomes 100ms local"
        );
        assert_eq!(plan.time_remap.as_ref().unwrap().keys[1].values, vec![0.7]);
    }

    #[test]
    fn control_hull_rejects_source_overshoot_without_assuming_native_clamping() {
        assert!(!segment_control_hull_is_bounded(
            250.0,
            750.0,
            PropertyKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: -1.0,
                x2: 0.75,
                y2: 3.0,
            },
            1_000.0,
        ));
    }
}
