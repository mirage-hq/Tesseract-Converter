//! Bounded fresh-record clocks for source-backed precomposition occurrences.
//!
//! Registration and the `FreshLayerRecord` setters are owned by the shared
//! writer core. This module deliberately returns typed record fields instead
//! of patching an `ldta` byte image.

use std::borrow::Cow;

use fx_schema::{
    PropertyKeyframeEasing, Time, TimeRangeProperty, TimeRemapExtrapolation, TimeRemapProperty,
};

use crate::rifx::Chunk;

pub(crate) use crate::schema::layer_records::{FreshLayerClockFields, NativeRational};

use super::{
    AepWriteError, KeyframeEasing, NumericKeyframe, NumericTrack, keyframes::PropertyClock, views,
};

const MILLIS_PER_SECOND: i128 = 1_000;

/// Exact nominal clock, independent native visibility, and optional authored Remap.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceClockPlan {
    pub(crate) record: FreshLayerClockFields,
    pub(crate) active_range: TimeRangeProperty,
    pub(crate) source_duration_millis: u64,
    visibility_record: Option<FreshLayerClockFields>,
    time_remap: Option<NumericTrack>,
}

impl SourceClockPlan {
    /// Preserves a canonical positive affine mapping when trimming/offsetting its
    /// visible window produces fractional source milliseconds. Native source
    /// in/out points stay rational; the editable mapping is never rounded.
    pub(crate) fn affine_windowed(
        active_range: TimeRangeProperty,
        input: TimeRangeProperty,
        output: TimeRangeProperty,
        input_offset_ms: i64,
        source_range: TimeRangeProperty,
        source_duration_millis: u64,
    ) -> Result<Self, AepWriteError> {
        let parent_delta = i128::from(input.duration.as_millis());
        let source_delta = i128::from(output.duration.as_millis());
        if active_range.duration.as_millis() == 0 || parent_delta == 0 || source_delta == 0 {
            return Err(AepWriteError::Invalid(
                "affine media clock requires positive spans",
            ));
        }
        let overflow = || AepWriteError::Invalid("affine media window calculation overflowed");
        let origin = i128::from(output.start.as_millis())
            .checked_mul(parent_delta)
            .ok_or_else(overflow)?;
        let endpoint = |time: Time| -> Result<i128, AepWriteError> {
            (i128::from(time.as_millis()) + i128::from(input_offset_ms)
                - i128::from(input.start.as_millis()))
            .checked_mul(source_delta)
            .and_then(|delta| origin.checked_add(delta))
            .ok_or_else(overflow)
        };
        let source_start = endpoint(active_range.start)?;
        let source_end = endpoint(active_range.end())?;
        let lower = i128::from(source_range.start.as_millis())
            .checked_mul(parent_delta)
            .ok_or_else(overflow)?;
        let upper = i128::from(source_range.end().as_millis())
            .checked_mul(parent_delta)
            .ok_or_else(overflow)?;
        if source_range.end().as_millis() > source_duration_millis
            || source_start < lower
            || source_end > upper
        {
            return Err(AepWriteError::Invalid(
                "affine media window leaves the authored source range",
            ));
        }
        let start_numerator = (i128::from(input.start.as_millis()) - i128::from(input_offset_ms))
            .checked_mul(source_delta)
            .and_then(|value| value.checked_sub(origin))
            .ok_or_else(overflow)?;
        let start_denominator = MILLIS_PER_SECOND
            .checked_mul(source_delta)
            .ok_or_else(overflow)?;
        let point_denominator = MILLIS_PER_SECOND
            .checked_mul(parent_delta)
            .ok_or_else(overflow)?;
        Ok(Self {
            record: FreshLayerClockFields {
                stretch: NativeRational::new(parent_delta, source_delta)?,
                start_time: NativeRational::new(start_numerator, start_denominator)?,
                in_point: NativeRational::new(source_start, point_denominator)?,
                out_point: NativeRational::new(source_end, point_denominator)?,
            },
            active_range,
            source_duration_millis,
            visibility_record: None,
            time_remap: None,
        })
    }

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
            visibility_record: None,
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
        Self::plan_time_remap(
            active_range,
            property,
            source_duration_millis,
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
        let plan = Self::time_remap_with_offset(active_range, property, source_duration_millis, 0)?;
        plan.time_remap_property_with_clock(clock)?;
        Ok(plan)
    }

    fn plan_time_remap(
        active_range: TimeRangeProperty,
        property: &TimeRemapProperty,
        source_duration_millis: u64,
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
        let leading_hold = i128::from(first.time.as_millis()) > mapped_start;
        let trailing_hold = i128::from(last.time.as_millis()) < mapped_end;
        if (leading_hold && property.before() != TimeRemapExtrapolation::Hold)
            || (trailing_hold && property.after() != TimeRemapExtrapolation::Hold)
        {
            return Err(AepWriteError::Invalid(
                "native Time Remap authored key span must cover the active interval or hold outside it",
            ));
        }

        let active_start = i128::from(active_range.start.as_millis());
        let source_limit = source_duration_millis as f64;
        let mut native_keys =
            Vec::with_capacity(keys.len() + usize::from(leading_hold) + usize::from(trailing_hold));
        if leading_hold {
            native_keys.push(NumericKeyframe {
                time_millis: 0,
                values: vec![first.value.as_millis() as f64 / 1_000.0],
                easing: vec![KeyframeEasing::Hold],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            });
        }
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
            let easing = native_easing(key.easing)?;
            native_keys.push(NumericKeyframe {
                time_millis: local_millis,
                values: vec![key.value.as_millis() as f64 / 1_000.0],
                // Easing belongs to the segment arriving at this key. The
                // new head-to-first segment must hold, regardless of the
                // authored first key's otherwise unused incoming easing.
                easing: vec![if leading_hold && index == 0 {
                    KeyframeEasing::Hold
                } else {
                    easing
                }],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            });
        }

        if trailing_hold {
            native_keys.push(NumericKeyframe {
                time_millis: i64::try_from(mapped_end - mapped_start).map_err(|_| {
                    AepWriteError::Invalid(
                        "native Time Remap endpoint exceeds the signed clock range",
                    )
                })?,
                values: vec![last.value.as_millis() as f64 / 1_000.0],
                easing: vec![KeyframeEasing::Hold],
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
            visibility_record: None,
            time_remap: Some(NumericTrack { keys: native_keys }),
        })
    }

    #[must_use]
    pub(crate) const fn has_time_remap(&self) -> bool {
        self.time_remap.is_some()
    }

    /// Match FX's nearest-millisecond visibility predicate without changing the
    /// affine source map or the nominal clock used to serialize property keys.
    /// Continuous native footage sampling remains distinct from FX sampling.
    pub(crate) fn apply_rounded_millisecond_visibility(&mut self) -> Result<(), AepWriteError> {
        if self.has_time_remap() {
            return Err(AepWriteError::Invalid(
                "authored Time Remap visibility must retain its original clock",
            ));
        }
        let shift = |value: NativeRational| {
            // All inputs are bounded i32/u32 fields; the three-factor products
            // fit i128. Parent half a millisecond maps to 1/(2000 * stretch)
            // source seconds, including non-unit positive stretches.
            let scale = 2_000 * i128::from(self.record.stretch.numerator);
            NativeRational::new(
                i128::from(value.numerator) * scale
                    - i128::from(self.record.stretch.denominator) * i128::from(value.denominator),
                i128::from(value.denominator) * scale,
            )
        };
        let in_point = shift(self.record.in_point)?;
        let out_point = shift(self.record.out_point)?;
        if in_point.numerator < 0 {
            return Err(AepWriteError::Invalid(
                "rounded-millisecond visibility would require a negative source in-point",
            ));
        }
        self.visibility_record = Some(FreshLayerClockFields {
            in_point,
            out_point,
            ..self.record
        });
        Ok(())
    }

    /// Only the native visible envelope uses the correction; property rebasing
    /// and source-time evaluation deliberately continue to use `record`.
    pub(crate) fn native_record(&self) -> FreshLayerClockFields {
        self.visibility_record.unwrap_or(self.record)
    }

    /// Native AV Linear Opacity and planar Linear/Hold Scale/Position followers
    /// first quantize occurrence time, then map ticks through the affine source clock.
    /// Callers must guard the independently proved property/interpolation profile;
    /// this helper does not admit arbitrary keyed properties or cubic segments.
    pub(super) fn affine_property_source_units(
        &self,
        occurrence_millis: i64,
        clock: PropertyClock,
    ) -> Result<i32, AepWriteError> {
        if self.has_time_remap() || self.record.stretch.numerator <= 0 {
            return Err(AepWriteError::Invalid(
                "scalar native ticks require positive affine clock",
            ));
        }
        let occurrence_units = i128::from(clock.units(occurrence_millis)?);
        let stretch = i128::from(self.record.stretch.numerator);
        let in_denominator = i128::from(self.record.in_point.denominator);
        let numerator = i128::from(self.record.in_point.numerator)
            .checked_mul(i128::from(clock.ticks()))
            .and_then(|value| value.checked_mul(stretch))
            .and_then(|value| {
                occurrence_units
                    .checked_mul(i128::from(self.record.stretch.denominator))
                    .and_then(|delta| delta.checked_mul(in_denominator))
                    .and_then(|delta| value.checked_add(delta))
            })
            .ok_or(AepWriteError::Invalid(
                "scalar native tick rebase overflowed",
            ))?;
        let denominator = in_denominator
            .checked_mul(stretch)
            .ok_or(AepWriteError::Invalid(
                "scalar native tick denominator overflowed",
            ))?;
        let adjusted = if numerator >= 0 {
            numerator.checked_add(denominator / 2)
        } else {
            numerator.checked_sub(denominator / 2)
        }
        .ok_or(AepWriteError::Invalid(
            "scalar native tick quantization overflowed",
        ))?;
        i32::try_from(adjusted / denominator)
            .map_err(|_| AepWriteError::Invalid("scalar native ticks exceed native range"))
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
                let mut native = Cow::Borrowed(track);
                let end = i128::from(self.active_range.duration.as_millis());
                if let Some(last) = track.keys.last()
                    && i128::from(last.time_millis) == end
                    && i128::from(clock.units(last.time_millis)?) * MILLIS_PER_SECOND
                        < end * i128::from(clock.ticks())
                {
                    // The native out-point remains exact. If nearest-tick
                    // authoring moves its endpoint inward, keep the final
                    // source value through an invisible Hold guard instead of
                    // relying on unestablished post-key extrapolation.
                    let mut guard = last.clone();
                    guard.time_millis =
                        guard
                            .time_millis
                            .checked_add(1)
                            .ok_or(AepWriteError::Invalid(
                                "native Time Remap endpoint guard overflows",
                            ))?;
                    guard.easing.fill(KeyframeEasing::Hold);
                    if i128::from(clock.units(guard.time_millis)?) * MILLIS_PER_SECOND
                        < end * i128::from(clock.ticks())
                    {
                        return Err(AepWriteError::Invalid(
                            "native Time Remap endpoint guard does not cover the active interval",
                        ));
                    }
                    native.to_mut().keys.push(guard);
                }
                // AE's keyed Time Remap leaf ends with the source-domain
                // bounds, even when every key value lies inside that range.
                // Omitting them makes Adobe skip the entire layer section.
                views::property_with_clock(
                    views::ValueKind::TimeRemap,
                    &[],
                    Some((0.0, self.source_duration_millis as f64 / 1_000.0)),
                    Some(native.as_ref()),
                    clock,
                )
                .map_err(AepWriteError::from)
            })
            .transpose()
    }

    /// Validate with the same selected clock and grammar as final serialization
    /// before committing the owner, retaining layer-local omission on failure.
    /// Returns the largest authored-key time rounding error in seconds.
    pub(crate) fn validate_time_remap_at_rate(
        &self,
        rate: crate::timing::FrameRate,
    ) -> Result<f64, AepWriteError> {
        let Some(track) = &self.time_remap else {
            return Ok(0.0);
        };
        let clock = super::keyframes::PropertyClock::for_rate(rate)?;
        self.time_remap_property_with_clock(clock)?;
        let mut residual = 0_u128;
        for key in &track.keys {
            let rounded = i128::from(clock.units(key.time_millis)?) * MILLIS_PER_SECOND;
            let authored = i128::from(key.time_millis) * i128::from(clock.ticks());
            residual = residual.max((rounded - authored).unsigned_abs());
        }
        // Nearest-tick rounding bounds this integer residual to 500.
        Ok(residual as f64 / (f64::from(clock.ticks()) * 1_000.0))
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
        // Native AE places Time Remap before Transform, not after Effects.
        // The property is a layer clock and must be the first leaf in the
        // root's authored property sequence.
        let index = children
            .iter()
            .position(|child| child.id() == *b"tdmn")
            .ok_or(AepWriteError::Invalid(
                "source-clock property root has no first property",
            ))?;
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
    fn rounded_visibility_keeps_nominal_property_clock_for_positive_stretches() {
        for (parent_span, source_span) in [(125, 125), (250, 125), (125, 250)] {
            let mut plan = SourceClockPlan::affine(
                TimeRangeProperty {
                    start: Time::from_millis(10_042),
                    duration: Duration::from_millis(parent_span),
                },
                Time::from_millis(800),
                Time::from_millis(800 + source_span),
                4_000,
            )
            .unwrap();
            let nominal = plan.record;
            let last_key = i64::try_from(parent_span).unwrap();
            let before = [
                plan.source_time_millis(0).unwrap(),
                plan.source_time_millis(last_key).unwrap(),
            ];
            plan.apply_rounded_millisecond_visibility().unwrap();
            assert_eq!(plan.record, nominal);
            assert_eq!(plan.native_record().start_time, nominal.start_time);
            assert_eq!(plan.native_record().stretch, nominal.stretch);
            assert_eq!(
                [
                    plan.source_time_millis(0).unwrap(),
                    plan.source_time_millis(last_key).unwrap(),
                ],
                before
            );
            let native = plan.native_record();
            let fraction =
                |value: NativeRational| f64::from(value.numerator) / f64::from(value.denominator);
            let begin =
                fraction(native.start_time) + fraction(native.stretch) * fraction(native.in_point);
            let end =
                fraction(native.start_time) + fraction(native.stretch) * fraction(native.out_point);
            assert!((begin - 10.0415).abs() < 1e-12);
            assert!((end - (10.0415 + parent_span as f64 / 1_000.0)).abs() < 1e-12);
        }
    }

    #[test]
    fn rounded_visibility_preserves_nominal_clock_when_source_head_is_unavailable() {
        let mut plan = SourceClockPlan::affine(
            TimeRangeProperty {
                start: Time::from_millis(10_042),
                duration: Duration::from_millis(125),
            },
            Time::ZERO,
            Time::from_millis(125),
            4_000,
        )
        .unwrap();
        let original = plan.clone();
        assert!(plan.apply_rounded_millisecond_visibility().is_err());
        assert_eq!(plan, original);
    }

    #[test]
    fn rounded_visibility_does_not_rewrite_authored_remap_keys_or_guard_clock() {
        let mut plan = SourceClockPlan::time_remap(
            TimeRangeProperty {
                start: Time::from_millis(10_042),
                duration: Duration::from_millis(125),
            },
            &remap(vec![
                key("a", 10_042, 800, PropertyKeyframeEasing::Linear),
                key("b", 10_167, 925, PropertyKeyframeEasing::Linear),
            ]),
            4_000,
        )
        .unwrap();
        let original = plan.clone();
        assert!(plan.apply_rounded_millisecond_visibility().is_err());
        assert_eq!(plan, original);
    }

    #[test]
    fn time_remap_writes_the_source_bounds_required_by_native_ae() {
        fn find_remap(chunks: &[Chunk]) -> Option<&Chunk> {
            for pair in chunks.windows(2) {
                if pair[0].id() == *b"tdmn"
                    && pair[0]
                        .data_payload()
                        .is_some_and(|name| name.starts_with(b"ADBE Time Remapping"))
                {
                    return Some(&pair[1]);
                }
            }
            chunks
                .iter()
                .filter_map(Chunk::children)
                .find_map(find_remap)
        }
        // Independently Adobe-authored held movie: an eight-second source,
        // with two authored keys at 0 and 2 seconds holding source time 0.75.
        let native = crate::rifx::Rifx::parse_with(
            include_bytes!(
                "../../tests/fixtures/media_native_panel/native/fx-export-media-static-remap.aep"
            ),
            |_| false,
        )
        .unwrap();
        let reference = find_remap(native.chunks()).unwrap().children().unwrap();
        let plan = SourceClockPlan::time_remap(
            TimeRangeProperty::new(Time::ZERO, Duration::from_millis(2_000)),
            &remap(vec![
                key("in", 0, 750, PropertyKeyframeEasing::Linear),
                key("out", 2_000, 750, PropertyKeyframeEasing::Linear),
            ]),
            8_000,
        )
        .unwrap();
        let property = plan
            .time_remap_property_with_clock(super::super::keyframes::PropertyClock::DEFAULT)
            .unwrap()
            .unwrap();
        let actual = property.children().unwrap();
        for tag in [*b"tdum", *b"tduM"] {
            assert_eq!(
                crate::properties::data(actual, tag).unwrap(),
                crate::properties::data(reference, tag).unwrap(),
                "native Time Remap source bound {tag:?}"
            );
        }
        let keys = crate::properties::read_numeric(actual).unwrap().keyframes;
        assert_eq!(
            keys.iter()
                .map(|key| (key.time_secs, key.values[0]))
                .collect::<Vec<_>>(),
            [(0.0, 0.75), (2.0, 0.75)]
        );
    }

    // Unmodified authored curves from Worlds A, archive SHA256
    // 7c775d8c6ce20d3b2571717192c5663e5b8d93940c1b937c892b495b66aafe8e.
    // Media admission and Adobe fidelity are separate from this clock assertion.
    #[test]
    fn worlds_endpoint_ramps_keep_all_keys_including_the_one_millisecond_jump() {
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        for (duration, points) in [
            (
                3_500,
                &[
                    (0, 0),
                    (800, 1500),
                    (1650, 3000),
                    (2000, 3250),
                    (2800, 4500),
                    (3500, 5958),
                ][..],
            ),
            (
                6_000,
                &[
                    (0, 0),
                    (1725, 1750),
                    (1746, 1770),
                    (1747, 2333),
                    (2450, 3036),
                    (2750, 3756),
                    (3250, 3831),
                    (6000, 5958),
                ][..],
            ),
        ] {
            let property = remap(
                points
                    .iter()
                    .map(|&(time, value)| {
                        key(
                            &time.to_string(),
                            time,
                            value,
                            PropertyKeyframeEasing::Linear,
                        )
                    })
                    .collect(),
            );
            let range = TimeRangeProperty::new(Time::ZERO, Duration::from_millis(duration));
            let plan = SourceClockPlan::time_remap_with_clock(range, &property, 6042, clock)
                .expect("bounded endpoint-aligned production ramp");
            let native = plan.time_remap_property_with_clock(clock).unwrap().unwrap();
            let native = crate::properties::read_numeric(native.children().unwrap()).unwrap();
            assert_eq!(native.keyframes.len(), points.len());
            for (key, &(time, value)) in native.keyframes.iter().zip(points) {
                assert!((key.time_secs - time as f64 / 1000.0).abs() <= 0.5 / 30_720.0);
                assert_eq!(key.values, [value as f64 / 1000.0]);
                assert_eq!((key.in_interpolation, key.out_interpolation), (1, 1));
            }
            assert!(
                native
                    .keyframes
                    .windows(2)
                    .all(|keys| keys[0].time_secs < keys[1].time_secs)
            );
            assert_eq!(plan.active_range, range);
            assert_eq!(plan.record.in_point, NativeRational::new(0, 1).unwrap());
            assert_eq!(
                plan.record.out_point,
                NativeRational::milliseconds(i128::from(duration)).unwrap()
            );
        }
    }

    #[test]
    fn inward_rounded_endpoint_keeps_exact_out_point_with_an_invisible_hold_guard() {
        let rate = crate::timing::FrameRate::new(30.0).unwrap();
        let clock = super::super::keyframes::PropertyClock::for_rate(rate).unwrap();
        let active = TimeRangeProperty::new(Time::ZERO, Duration::from_millis(1_746));
        let property = remap(vec![
            key("a", 0, 0, PropertyKeyframeEasing::Linear),
            key("b", 1_745, 1_207, PropertyKeyframeEasing::Linear),
            key("c", 1_746, 1_770, PropertyKeyframeEasing::Linear),
        ]);
        let plan = SourceClockPlan::time_remap_with_clock(active, &property, 6_042, clock).unwrap();
        let native = plan.time_remap_property_with_clock(clock).unwrap().unwrap();
        let native = crate::properties::read_numeric(native.children().unwrap()).unwrap();
        assert_eq!(native.keyframes.len(), 4);
        assert_eq!(native.keyframes[2].values, [1.770]);
        assert_eq!(native.keyframes[3].values, [1.770]);
        assert_eq!(native.keyframes[2].out_interpolation, 3);
        assert!(native.keyframes[2].time_secs < 1.746);
        assert!(native.keyframes[3].time_secs > 1.746);
        assert!(native.keyframes[1].time_secs < native.keyframes[2].time_secs);
        assert_eq!(plan.active_range, active);
        assert_eq!(
            plan.record.out_point,
            NativeRational::milliseconds(1_746).unwrap()
        );
        let rounding = plan.validate_time_remap_at_rate(rate).unwrap();
        assert!(rounding > 0.0 && rounding <= 0.5 / 30_720.0);
        assert_eq!(
            property.keyframes().len(),
            3,
            "editable FX input stays unchanged"
        );
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
        assert!(SourceClockPlan::time_remap_with_offset(range, &property, 4_000, 1_501).is_err());
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
    fn time_remap_covers_endpoints_and_quantizes_subtick_keys_without_extrapolation() {
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
        assert!(SourceClockPlan::time_remap(active, &endpoint_dependent, 2_000).is_ok());

        let inexact_tick = remap(vec![
            key("a", 1, 0, PropertyKeyframeEasing::Linear),
            key("b", 1_251, 500, PropertyKeyframeEasing::Linear),
            key("c", 3_001, 1_000, PropertyKeyframeEasing::Linear),
        ]);
        assert!(SourceClockPlan::time_remap(active, &inexact_tick, 2_000).is_ok());

        let uncovered = remap(vec![
            key("a", 1_001, 0, PropertyKeyframeEasing::Linear),
            key("b", 2_000, 1_000, PropertyKeyframeEasing::Linear),
        ]);
        assert!(SourceClockPlan::time_remap(active, &uncovered, 2_000).is_err());
    }

    #[test]
    fn held_tail_of_source_remap_keeps_visible_endpoint_and_original_keys() {
        // Video 8357 of the private IG master has a 3271ms visible window,
        // 196 authored keys ending at 3266ms, and after: Hold. Only its final
        // five milliseconds need an endpoint guard; the source remains intact.
        let active = TimeRangeProperty::new(Time::ZERO, Duration::from_millis(3_271));
        let property = TimeRemapProperty::new(
            vec![
                key("start", 0, 2_868, PropertyKeyframeEasing::Linear),
                key("last", 3_266, 8_168, PropertyKeyframeEasing::Linear),
            ],
            TimeRemapExtrapolation::Hold,
            TimeRemapExtrapolation::Hold,
        )
        .unwrap();
        let plan = SourceClockPlan::time_remap(active, &property, 15_000).unwrap();
        let track = &plan.time_remap.as_ref().unwrap().keys;
        assert_eq!(track.len(), 3);
        assert_eq!(track[1].time_millis, 3_266);
        assert_eq!(track[1].values, [8.168]);
        assert_eq!(track[2].time_millis, 3_271);
        assert_eq!(track[2].values, [8.168]);
        assert_eq!(track[2].easing, [KeyframeEasing::Hold]);
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let native = plan.time_remap_property_with_clock(clock).unwrap().unwrap();
        let native = crate::properties::read_numeric(native.children().unwrap()).unwrap();
        assert_eq!(native.keyframes.len(), 4); // inward tick rounding guard
        assert_eq!(native.keyframes[2].values, [8.168]);
        assert!(native.keyframes[3].time_secs > 3.271);
        assert_eq!(property.keyframes().len(), 2);

        let unsupported = TimeRemapProperty::new(
            property.keyframes().to_vec(),
            TimeRemapExtrapolation::Hold,
            TimeRemapExtrapolation::Continue,
        )
        .unwrap();
        assert!(SourceClockPlan::time_remap(active, &unsupported, 15_000).is_err());
    }

    #[test]
    fn held_head_uses_shifted_window_without_admitting_other_extrapolation() {
        let active = TimeRangeProperty::new(Time::from_millis(1_000), Duration::from_millis(500));
        let property = TimeRemapProperty::new(
            vec![
                key("a", 1_300, 700, PropertyKeyframeEasing::Linear),
                key("b", 1_700, 900, PropertyKeyframeEasing::Linear),
            ],
            TimeRemapExtrapolation::Hold,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap();
        let plan = SourceClockPlan::time_remap_with_offset(active, &property, 1_000, 100).unwrap();
        let track = &plan.time_remap.as_ref().unwrap().keys;
        assert_eq!(track.len(), 3);
        assert_eq!(track[0].time_millis, 0);
        assert_eq!(track[0].values, [0.7]);
        assert_eq!(track[1].time_millis, 200);
        assert_eq!(track[1].easing, [KeyframeEasing::Hold]);
        assert_eq!(track[2].time_millis, 600);
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let native = plan.time_remap_property_with_clock(clock).unwrap().unwrap();
        let native = crate::properties::read_numeric(native.children().unwrap()).unwrap();
        assert_eq!(native.keyframes[1].in_interpolation, 3);
        assert_eq!(
            property.keyframes()[0].easing,
            PropertyKeyframeEasing::Linear
        );
        let unsupported = TimeRemapProperty::new(
            property.keyframes().to_vec(),
            TimeRemapExtrapolation::Continue,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap();
        assert!(SourceClockPlan::time_remap_with_offset(active, &unsupported, 1_000, 100).is_err());
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
        // The Adobe-authored timing_time_remap_hold.aep stores 1 for a keyed
        // Time Remap leaf, and 3 for a static one.
        let flags = property
            .children()
            .unwrap()
            .iter()
            .find(|child| child.id() == *b"tdsb")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(flags, 1_u32.to_be_bytes());
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
    fn keyed_time_remap_includes_adobe_source_domain_bounds() {
        // The independently Adobe-authored timing_time_remap_linear.aep has
        // tdum = 0 and tduM = 2 after the two-key list. Without these bounds,
        // AE reports "skipped sections: 1" and rejects an otherwise valid AEP.
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let plan = SourceClockPlan::time_remap_with_clock(
            TimeRangeProperty::new(Time::from_millis(1_000), Duration::from_millis(1_000)),
            &remap(vec![
                key("a", 1_000, 250, PropertyKeyframeEasing::Linear),
                key("b", 2_000, 1_700, PropertyKeyframeEasing::Linear),
            ]),
            2_000,
            clock,
        )
        .unwrap();
        let property = plan.time_remap_property_with_clock(clock).unwrap().unwrap();
        let children = property.children().unwrap();
        let bounds = &children[children.len() - 2..];
        assert_eq!(bounds[0].id(), *b"tdum");
        assert_eq!(bounds[0].data_payload().unwrap(), 0.0_f64.to_be_bytes());
        assert_eq!(bounds[1].id(), *b"tduM");
        assert_eq!(bounds[1].data_payload().unwrap(), 2.0_f64.to_be_bytes());

        let adobe = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/pr4442_native/sources/timing_time_remap_linear.aep"
        ))
        .unwrap();
        let adobe_leaf = adobe
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                crate::structure::ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .flat_map(|comp| &comp.layers)
            .flat_map(|layer| &layer.content)
            .filter(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .map(|chunk| chunk.children().unwrap())
            .flat_map(|root| root.windows(2))
            .find_map(|pair| {
                (pair[0].id() == *b"tdmn"
                    && pair[0]
                        .data_payload()
                        .is_some_and(|name| name.starts_with(b"ADBE Time Remapping\0"))
                    && pair[1].children().is_some_and(|property| {
                        property
                            .iter()
                            .any(|chunk| chunk.list_kind() == Some(*b"list"))
                    }))
                .then_some(&pair[1])
            })
            .unwrap();
        let adobe_children = adobe_leaf.children().unwrap();
        let adobe_bounds = &adobe_children[adobe_children.len() - 2..];
        for (authored, exported) in adobe_bounds.iter().zip(bounds) {
            assert_eq!(authored.id(), exported.id());
            assert_eq!(authored.data_payload(), exported.data_payload());
        }
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
