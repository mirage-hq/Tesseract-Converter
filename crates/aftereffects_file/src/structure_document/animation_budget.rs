//! Converter-local accounting for generated editable animation payloads.
//!
//! Tracks serialized graph entries and inline time-remap properties without an
//! aggregate project-size quota. Checked arithmetic still applies; this
//! accounting does not bound process RSS.

use std::{fmt, io};

#[cfg(test)]
use fx_schema::TimeRemapProperty;
use fx_schema::animator::{
    AnimationGraphEntry, AnimatorData, MAX_KEYFRAME_ID_BYTES, PropertyKeyframe,
};
use fx_schema::{
    LayerRefMap, PropertyAnimator, PropertyKeyframeEasing, PropertyTarget, PropertyValue,
    ShapePath, Time, TimeRemapExtrapolation,
};
use serde::Serialize;

const GRAPH_ENTRY_SEPARATOR_BYTES: usize = 1;
const INLINE_PROPERTY_ATTACHMENT_BYTES: usize = 1;
const PROPERTY_TRACK_PREFIX: &[u8] = br#"{"keyframes":["#;
const PROPERTY_TRACK_SUFFIX: &[u8] = b"]}";
const KEYFRAME_ANIMATOR_PREFIX: &[u8] = br#"{"type":"keyframes","keyframes":["#;
const KEYFRAME_ANIMATOR_SUFFIX: &[u8] = br#"],"enabled":true}"#;
const GRAPH_ENTRY_TARGET_PREFIX: &[u8] = br#"{"target":"#;
const GRAPH_ENTRY_ANIMATOR_SEPARATOR: &[u8] = b",\"animator\":";
const GRAPH_ENTRY_SUFFIX: &[u8] = b",\"dependencies\":[],\"layerRefs\":{}}";
const REMAP_PREFIX: &[u8] = br#"{"keyframes":["#;
const REMAP_SUFFIX_PREFIX: &[u8] = br#"],"before":"#;
const REMAP_AFTER_SEPARATOR: &[u8] = b",\"after\":";
const REMAP_SUFFIX: &[u8] = b"}";
const REMAP_KEY_ID_PREFIX: &[u8] = br#"{"id":"#;
const REMAP_KEY_TIME_SEPARATOR: &[u8] = b",\"time\":";
const REMAP_KEY_VALUE_SEPARATOR: &[u8] = b",\"value\":";
const REMAP_KEY_EASING_SEPARATOR: &[u8] = b",\"easing\":";
const REMAP_KEY_SUFFIX: &[u8] = b"}";

/// Single-owner accounting for converter-generated serialized animation data.
/// Test-only allowances exercise transactional failure without a production quota.
#[derive(Debug, Default)]
pub(super) struct AnimationBudget {
    used: usize,
    denials: usize,
    #[cfg(test)]
    limit: Option<usize>,
}

/// An owned transaction point. Consuming it in [`AnimationBudget::rollback`]
/// prevents one caller from restoring the same reservation twice.
#[derive(Debug)]
pub(super) struct AnimationCheckpoint {
    used: usize,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ReservationError {
    #[error(
        "animation budget has {remaining} bytes remaining, but {requested} bytes were requested"
    )]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only the test-only budget limit constructs it")
    )]
    Exhausted { requested: usize, remaining: usize },
    #[error("generated keyframe id has invalid UTF-8 byte length {actual}; expected 1..={maximum}")]
    InvalidGeneratedId { actual: usize, maximum: usize },
    #[error("estimated animation track is empty")]
    EmptyTrack,
    #[error("animation size arithmetic overflowed")]
    SizeOverflow,
    #[error("cannot release {released} animation bytes when only {used} bytes are reserved")]
    ReleaseUnderflow { released: usize, used: usize },
    #[error("animation value could not be sized with its persisted serializer: {0}")]
    Serialization(String),
}

impl AnimationBudget {
    #[cfg(test)]
    pub(super) fn with_limit(bytes: usize) -> Self {
        Self {
            limit: Some(bytes),
            ..Self::default()
        }
    }

    pub(super) fn checkpoint(&self) -> AnimationCheckpoint {
        AnimationCheckpoint { used: self.used }
    }

    pub(super) fn rollback(&mut self, checkpoint: AnimationCheckpoint) {
        debug_assert!(
            checkpoint.used <= self.used,
            "an animation checkpoint must precede the reservations it rolls back"
        );
        if checkpoint.used <= self.used {
            self.used = checkpoint.used;
        }
    }

    pub(super) fn reserve(&mut self, bytes: usize) -> Result<(), ReservationError> {
        let Some(used) = self.used.checked_add(bytes) else {
            self.denials = self.denials.saturating_add(1);
            return Err(ReservationError::SizeOverflow);
        };
        #[cfg(test)]
        if let Some(limit) = self.limit
            && used > limit
        {
            self.denials = self.denials.saturating_add(1);
            return Err(ReservationError::Exhausted {
                requested: bytes,
                remaining: limit - self.used,
            });
        }
        self.used = used;
        Ok(())
    }

    pub(super) fn release(&mut self, bytes: usize) -> Result<(), ReservationError> {
        self.used = self
            .used
            .checked_sub(bytes)
            .ok_or(ReservationError::ReleaseUnderflow {
                released: bytes,
                used: self.used,
            })?;
        Ok(())
    }

    /// Atomically sums and reserves a coupled set of estimates.
    pub(super) fn reserve_all(
        &mut self,
        estimates: impl IntoIterator<Item = usize>,
    ) -> Result<usize, ReservationError> {
        let mut total = 0_usize;
        for estimate in estimates {
            let Some(next) = total.checked_add(estimate) else {
                self.denials = self.denials.saturating_add(1);
                return Err(ReservationError::SizeOverflow);
            };
            total = next;
        }
        self.reserve(total)?;
        Ok(total)
    }

    #[cfg(test)]
    pub(super) const fn remaining(&self) -> usize {
        self.limit
            .expect("remaining is used only with a test allowance")
            - self.used
    }

    #[cfg(test)]
    pub(super) const fn used(&self) -> usize {
        self.used
    }

    /// Failed accounting reservations, retained across rollback so callers can
    /// detect a failed expansion transaction.
    pub(super) const fn denials(&self) -> usize {
        self.denials
    }
}

/// Serialized-size metadata for a generated keyframe id, computed while the id
/// is formatted. This avoids retaining a generated `String` during preflight.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct GeneratedKeyframeIdSize {
    raw_bytes: usize,
    serialized_bytes: usize,
}

impl GeneratedKeyframeIdSize {
    pub(super) fn new(arguments: fmt::Arguments<'_>) -> Result<Self, ReservationError> {
        let mut counter = EscapedJsonStringCounter::new();
        if fmt::write(&mut counter, arguments).is_err() {
            return Err(ReservationError::SizeOverflow);
        }
        if counter.raw_bytes == 0 || counter.raw_bytes > MAX_KEYFRAME_ID_BYTES {
            return Err(ReservationError::InvalidGeneratedId {
                actual: counter.raw_bytes,
                maximum: MAX_KEYFRAME_ID_BYTES,
            });
        }
        Ok(Self {
            raw_bytes: counter.raw_bytes,
            serialized_bytes: counter.serialized_bytes,
        })
    }

    #[cfg(test)]
    pub(super) const fn raw_bytes(&self) -> usize {
        self.raw_bytes
    }

    pub(super) const fn serialized_bytes(&self) -> usize {
        self.serialized_bytes
    }
}

struct EscapedJsonStringCounter {
    raw_bytes: usize,
    // Opening and closing quotes are included.
    serialized_bytes: usize,
}

impl EscapedJsonStringCounter {
    const fn new() -> Self {
        Self {
            raw_bytes: 0,
            serialized_bytes: 2,
        }
    }
}

impl fmt::Write for EscapedJsonStringCounter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.raw_bytes = self.raw_bytes.checked_add(value.len()).ok_or(fmt::Error)?;
        for byte in value.bytes() {
            let encoded_bytes = match byte {
                b'"' | b'\\' | b'\x08' | b'\t' | b'\n' | b'\x0c' | b'\r' => 2,
                0x00..=0x1f => 6,
                _ => 1,
            };
            self.serialized_bytes = self
                .serialized_bytes
                .checked_add(encoded_bytes)
                .ok_or(fmt::Error)?;
        }
        Ok(())
    }
}

/// Allocation-free aggregate for one prospective property-keyframe track.
///
/// Prospective converted numeric keys may be built one at a time and measured
/// through `PropertyKeyframe::serialized_json_size`; copied keys use the
/// borrowed source key plus a replacement-id size and do not clone values.
#[derive(Debug, Default)]
pub(super) struct PropertyTrackEstimate {
    keyframe_bytes: usize,
    keyframe_count: usize,
}

impl PropertyTrackEstimate {
    pub(super) fn push_prospective(
        &mut self,
        keyframe: &PropertyKeyframe,
    ) -> Result<(), ReservationError> {
        validate_keyframe_id(keyframe.id().as_str())?;
        let bytes = keyframe
            .serialized_json_size()
            .map_err(|error| ReservationError::Serialization(error.to_string()))?;
        self.push_keyframe_bytes(bytes)
    }

    /// Sizes a copied key without cloning its potentially large borrowed value.
    pub(super) fn push_copied(
        &mut self,
        source: &PropertyKeyframe,
        replacement_id: &GeneratedKeyframeIdSize,
    ) -> Result<(), ReservationError> {
        let source_bytes = source
            .serialized_json_size()
            .map_err(|error| ReservationError::Serialization(error.to_string()))?;
        let source_id_bytes = serialized_json_size(source.id())?;
        let without_source_id = source_bytes
            .checked_sub(source_id_bytes)
            .ok_or(ReservationError::SizeOverflow)?;
        let bytes = checked_add(without_source_id, replacement_id.serialized_bytes())?;
        self.push_keyframe_bytes(bytes)
    }

    fn push_keyframe_bytes(&mut self, bytes: usize) -> Result<(), ReservationError> {
        let keyframe_bytes = checked_add(self.keyframe_bytes, bytes)?;
        let keyframe_count = self
            .keyframe_count
            .checked_add(1)
            .ok_or(ReservationError::SizeOverflow)?;
        self.keyframe_bytes = keyframe_bytes;
        self.keyframe_count = keyframe_count;
        Ok(())
    }

    pub(super) fn track_serialized_bytes(&self) -> Result<usize, ReservationError> {
        if self.keyframe_count == 0 {
            return Err(ReservationError::EmptyTrack);
        }
        sequence_size(
            PROPERTY_TRACK_PREFIX.len(),
            self.keyframe_bytes,
            self.keyframe_count,
            PROPERTY_TRACK_SUFFIX.len(),
        )
    }

    pub(super) fn entry_serialized_bytes(
        &self,
        target: &PropertyTarget,
    ) -> Result<usize, ReservationError> {
        // Evaluate this independently rather than deriving it from the track
        // wrapper, because the animator flattens the same key array directly.
        self.track_serialized_bytes()?;
        let animator_bytes = sequence_size(
            KEYFRAME_ANIMATOR_PREFIX.len(),
            self.keyframe_bytes,
            self.keyframe_count,
            KEYFRAME_ANIMATOR_SUFFIX.len(),
        )?;
        graph_entry_size(target, animator_bytes)
    }

    pub(super) fn entry_reservation_bytes(
        &self,
        target: &PropertyTarget,
    ) -> Result<usize, ReservationError> {
        checked_add(
            self.entry_serialized_bytes(target)?,
            GRAPH_ENTRY_SEPARATOR_BYTES,
        )
    }
}

/// Exact serialized size of a constant graph entry without cloning `value`.
pub(super) fn constant_entry_serialized_bytes(
    target: &PropertyTarget,
    value: &PropertyValue,
) -> Result<usize, ReservationError> {
    let animator = BorrowedConstantAnimator {
        animator_type: "constant",
        value,
    };
    borrowed_entry_serialized_bytes(target, &animator)
}

pub(super) fn constant_entry_reservation_bytes(
    target: &PropertyTarget,
    value: &PropertyValue,
) -> Result<usize, ReservationError> {
    checked_add(
        constant_entry_serialized_bytes(target, value)?,
        GRAPH_ENTRY_SEPARATOR_BYTES,
    )
}

/// Exact size of an entry that reuses an already-built animator by borrowing
/// its persisted representation. This is suitable for constants and scripts.
pub(super) fn borrowed_animator_entry_serialized_bytes(
    target: &PropertyTarget,
    animator: &PropertyAnimator,
) -> Result<usize, ReservationError> {
    borrowed_entry_serialized_bytes(target, animator)
}

pub(super) fn borrowed_animator_entry_reservation_bytes(
    target: &PropertyTarget,
    animator: &PropertyAnimator,
) -> Result<usize, ReservationError> {
    checked_add(
        borrowed_animator_entry_serialized_bytes(target, animator)?,
        GRAPH_ENTRY_SEPARATOR_BYTES,
    )
}

/// Exact reservation for a copied keyframe animator. The complete canonical
/// animator is sized first so authored enabled/disabled state is included, then
/// only each serialized key id is replaced by its generated counterpart.
pub(super) fn copied_animator_entry_reservation_bytes(
    target: &PropertyTarget,
    animator: &PropertyAnimator,
) -> Result<usize, ReservationError> {
    let AnimatorData::Keyframes { track, .. } = animator.data() else {
        return borrowed_animator_entry_reservation_bytes(target, animator);
    };
    let mut estimate = PropertyTrackEstimate::default();
    let mut entry_bytes = borrowed_entry_serialized_bytes(target, animator.data())?;
    for (index, key) in track.keyframes().iter().enumerate() {
        let replacement_id =
            GeneratedKeyframeIdSize::new(format_args!("aep-copy-{target}-{index}"))?;
        estimate.push_copied(key, &replacement_id)?;
        entry_bytes = entry_bytes
            .checked_sub(serialized_json_size(key.id())?)
            .ok_or(ReservationError::SizeOverflow)?;
        entry_bytes = checked_add(entry_bytes, replacement_id.serialized_bytes())?;
    }
    estimate.track_serialized_bytes()?;
    checked_add(entry_bytes, GRAPH_ENTRY_SEPARATOR_BYTES)
}

pub(super) fn committed_entry_reservation_bytes(
    entry: &AnimationGraphEntry,
) -> Result<usize, ReservationError> {
    checked_add(serialized_json_size(entry)?, GRAPH_ENTRY_SEPARATOR_BYTES)
}

/// Exact serialized size of an already-built entry. Intended for final test
/// assertions, not as a post-allocation substitute for preflight estimation.
#[cfg(test)]
pub(super) fn committed_entry_serialized_bytes(
    entry: &AnimationGraphEntry,
) -> Result<usize, ReservationError> {
    serialized_json_size(entry)
}

pub(super) fn path_constant_entry_reservation_bytes(
    target: &PropertyTarget,
    path: &ShapePath,
) -> Result<usize, ReservationError> {
    let value = BorrowedPathValue::Path(path);
    let animator = BorrowedConstantAnimator {
        animator_type: "constant",
        value: &value,
    };
    checked_add(
        borrowed_entry_serialized_bytes(target, &animator)?,
        GRAPH_ENTRY_SEPARATOR_BYTES,
    )
}

#[cfg(test)]
pub(super) fn committed_remap_serialized_bytes(
    remap: &TimeRemapProperty,
) -> Result<usize, ReservationError> {
    serialized_json_size(remap)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BorrowedConstantAnimator<'a, V> {
    #[serde(rename = "type")]
    animator_type: &'static str,
    value: &'a V,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
enum BorrowedPathValue<'a> {
    Path(&'a ShapePath),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BorrowedGraphEntry<'a, A> {
    target: &'a PropertyTarget,
    animator: &'a A,
    dependencies: &'a [PropertyTarget],
    #[serde(skip_serializing_if = "Option::is_none")]
    random_seed_target: Option<&'a PropertyTarget>,
    layer_refs: &'a LayerRefMap,
}

fn borrowed_entry_serialized_bytes(
    target: &PropertyTarget,
    animator: &impl Serialize,
) -> Result<usize, ReservationError> {
    let dependencies = [];
    let layer_refs = LayerRefMap::default();
    serialized_json_size(&BorrowedGraphEntry {
        target,
        animator,
        dependencies: &dependencies,
        random_seed_target: None,
        layer_refs: &layer_refs,
    })
}

fn graph_entry_size(
    target: &PropertyTarget,
    animator_bytes: usize,
) -> Result<usize, ReservationError> {
    let mut bytes = GRAPH_ENTRY_TARGET_PREFIX.len();
    bytes = checked_add(bytes, serialized_json_size(target)?)?;
    bytes = checked_add(bytes, GRAPH_ENTRY_ANIMATOR_SEPARATOR.len())?;
    bytes = checked_add(bytes, animator_bytes)?;
    checked_add(bytes, GRAPH_ENTRY_SUFFIX.len())
}

/// Allocation-free aggregate for one prospective inline time-remap property.
#[derive(Debug, Default)]
pub(super) struct TimeRemapEstimate {
    keyframe_bytes: usize,
    keyframe_count: usize,
}

impl TimeRemapEstimate {
    pub(super) fn push_key(
        &mut self,
        id: &GeneratedKeyframeIdSize,
        time: Time,
        value: Time,
        easing: PropertyKeyframeEasing,
    ) -> Result<(), ReservationError> {
        let mut bytes = REMAP_KEY_ID_PREFIX.len();
        bytes = checked_add(bytes, id.serialized_bytes())?;
        bytes = checked_add(bytes, REMAP_KEY_TIME_SEPARATOR.len())?;
        bytes = checked_add(bytes, serialized_json_size(&time)?)?;
        bytes = checked_add(bytes, REMAP_KEY_VALUE_SEPARATOR.len())?;
        bytes = checked_add(bytes, serialized_json_size(&value)?)?;
        bytes = checked_add(bytes, REMAP_KEY_EASING_SEPARATOR.len())?;
        bytes = checked_add(bytes, serialized_json_size(&easing)?)?;
        bytes = checked_add(bytes, REMAP_KEY_SUFFIX.len())?;

        let keyframe_bytes = checked_add(self.keyframe_bytes, bytes)?;
        let keyframe_count = self
            .keyframe_count
            .checked_add(1)
            .ok_or(ReservationError::SizeOverflow)?;
        self.keyframe_bytes = keyframe_bytes;
        self.keyframe_count = keyframe_count;
        Ok(())
    }

    pub(super) fn serialized_bytes(
        &self,
        before: TimeRemapExtrapolation,
        after: TimeRemapExtrapolation,
    ) -> Result<usize, ReservationError> {
        if self.keyframe_count == 0 {
            return Err(ReservationError::EmptyTrack);
        }
        let mut suffix_bytes = REMAP_SUFFIX_PREFIX.len();
        suffix_bytes = checked_add(suffix_bytes, serialized_json_size(&before)?)?;
        suffix_bytes = checked_add(suffix_bytes, REMAP_AFTER_SEPARATOR.len())?;
        suffix_bytes = checked_add(suffix_bytes, serialized_json_size(&after)?)?;
        suffix_bytes = checked_add(suffix_bytes, REMAP_SUFFIX.len())?;
        sequence_size(
            REMAP_PREFIX.len(),
            self.keyframe_bytes,
            self.keyframe_count,
            suffix_bytes,
        )
    }

    pub(super) fn reservation_bytes(
        &self,
        before: TimeRemapExtrapolation,
        after: TimeRemapExtrapolation,
    ) -> Result<usize, ReservationError> {
        checked_add(
            self.serialized_bytes(before, after)?,
            INLINE_PROPERTY_ATTACHMENT_BYTES,
        )
    }
}

fn validate_keyframe_id(id: &str) -> Result<(), ReservationError> {
    if id.is_empty() || id.len() > MAX_KEYFRAME_ID_BYTES {
        return Err(ReservationError::InvalidGeneratedId {
            actual: id.len(),
            maximum: MAX_KEYFRAME_ID_BYTES,
        });
    }
    Ok(())
}

fn sequence_size(
    prefix_bytes: usize,
    item_bytes: usize,
    item_count: usize,
    suffix_bytes: usize,
) -> Result<usize, ReservationError> {
    let separators = item_count.saturating_sub(1);
    let bytes = checked_add(prefix_bytes, item_bytes)?;
    let bytes = checked_add(bytes, separators)?;
    checked_add(bytes, suffix_bytes)
}

fn checked_add(left: usize, right: usize) -> Result<usize, ReservationError> {
    left.checked_add(right)
        .ok_or(ReservationError::SizeOverflow)
}

fn serialized_json_size(value: &(impl Serialize + ?Sized)) -> Result<usize, ReservationError> {
    let mut writer = CountingWriter::default();
    let result = serde_json::to_writer(&mut writer, value);
    if writer.overflowed {
        return Err(ReservationError::SizeOverflow);
    }
    result
        .map(|()| writer.bytes)
        .map_err(|error| ReservationError::Serialization(error.to_string()))
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
    overflowed: bool,
}

impl io::Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(bytes) = self.bytes.checked_add(buffer.len()) else {
            self.overflowed = true;
            return Err(io::Error::other("serialized animation size overflow"));
        };
        self.bytes = bytes;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use fx_schema::animator::{AnimationGraphEntry, KeyframeId, PropertyKeyframeTrack};
    use fx_schema::layer::{ShapePath, ShapePathCommand};
    use fx_schema::{
        FxItemId, LayerId, PropType, TimeOffset, TimeRemapKeyframe, TimeRemapProperty,
    };

    use super::*;

    fn entry(target: PropertyTarget, animator: PropertyAnimator) -> AnimationGraphEntry {
        AnimationGraphEntry {
            target,
            animator,
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: LayerRefMap::default(),
        }
    }

    fn assert_keyframed_entry_parity(target: PropertyTarget, keyframe: PropertyKeyframe) {
        let mut estimate = PropertyTrackEstimate::default();
        estimate.push_prospective(&keyframe).unwrap();
        let estimated_track = estimate.track_serialized_bytes().unwrap();
        let track = PropertyKeyframeTrack::new(vec![keyframe]).unwrap();
        assert_eq!(estimated_track, serde_json::to_vec(&track).unwrap().len());
        let actual = entry(target.clone(), PropertyAnimator::keyframes(track));
        assert_eq!(
            estimate.entry_serialized_bytes(&target).unwrap(),
            serde_json::to_vec(&actual).unwrap().len()
        );
        assert_eq!(
            estimate.entry_reservation_bytes(&target).unwrap(),
            serde_json::to_vec(&actual).unwrap().len() + GRAPH_ENTRY_SEPARATOR_BYTES
        );
    }

    #[test]
    fn default_animation_accounting_has_no_aggregate_size_quota() {
        let mut budget = AnimationBudget::default();
        budget.reserve(64 * 1024 * 1024 + 1).unwrap();
        let checkpoint = budget.checkpoint();
        budget.reserve(1).unwrap();
        budget.rollback(checkpoint);
        assert_eq!(budget.used(), 64 * 1024 * 1024 + 1);
        assert_eq!(
            budget.reserve(usize::MAX),
            Err(ReservationError::SizeOverflow)
        );
        assert_eq!(budget.used(), 64 * 1024 * 1024 + 1);
        assert_eq!(budget.denials(), 1);
    }

    #[test]
    fn failed_reservations_and_overflow_are_atomic() {
        let mut budget = AnimationBudget::with_limit(10);
        budget.reserve(4).unwrap();
        let checkpoint = budget.checkpoint();
        let remaining = budget.remaining();

        assert_eq!(
            budget.reserve(remaining + 1),
            Err(ReservationError::Exhausted {
                requested: remaining + 1,
                remaining,
            })
        );
        assert_eq!(budget.remaining(), remaining);
        assert_eq!(budget.denials(), 1);

        assert_eq!(
            budget.reserve_all([usize::MAX, 1]),
            Err(ReservationError::SizeOverflow)
        );
        assert_eq!(budget.remaining(), remaining);
        assert_eq!(budget.denials(), 2);

        budget.reserve(remaining).unwrap();
        assert_eq!(budget.used(), 10);
        assert_eq!(
            budget.release(11),
            Err(ReservationError::ReleaseUnderflow {
                released: 11,
                used: 10,
            })
        );
        assert_eq!(budget.used(), 10);
        budget.rollback(checkpoint);
        assert_eq!(budget.remaining(), remaining);
        assert_eq!(budget.denials(), 2);
    }

    #[test]
    fn releasing_discarded_entry_keeps_inline_remap_reservation() {
        let mut remap = TimeRemapEstimate::default();
        for (index, time) in [Time::ZERO, Time::from_secs(1.0)].into_iter().enumerate() {
            let id = GeneratedKeyframeIdSize::new(format_args!("remap-{index}")).unwrap();
            remap
                .push_key(&id, time, time, PropertyKeyframeEasing::Linear)
                .unwrap();
        }
        let remap_bytes = remap
            .reservation_bytes(
                TimeRemapExtrapolation::Inactive,
                TimeRemapExtrapolation::Inactive,
            )
            .unwrap();
        let discarded_bytes = 37;
        let mut budget = AnimationBudget::with_limit(remap_bytes + discarded_bytes);
        budget.reserve(remap_bytes).unwrap();
        budget.reserve(discarded_bytes).unwrap();
        budget.release(discarded_bytes).unwrap();
        assert_eq!(budget.used(), remap_bytes);
    }

    #[test]
    fn track_estimate_has_no_serialized_size_quota() {
        let mut estimate = PropertyTrackEstimate::default();
        estimate.push_keyframe_bytes(1024 * 1024).unwrap();
        assert!(estimate.track_serialized_bytes().unwrap() > 1024 * 1024);
    }

    #[test]
    fn generated_id_counter_matches_serde_escaping_and_enforces_byte_limit() {
        let escaped = "quote-\" slash-\\ controls-\u{0008}\t\n\u{000c}\r\u{0001} snow-雪";
        let size = GeneratedKeyframeIdSize::new(format_args!("prefix-{escaped}")).unwrap();
        let actual = serde_json::to_vec(&format!("prefix-{escaped}"))
            .unwrap()
            .len();
        assert_eq!(size.raw_bytes(), format!("prefix-{escaped}").len());
        assert_eq!(size.serialized_bytes(), actual);

        let maximum = "x".repeat(MAX_KEYFRAME_ID_BYTES);
        let maximum_size = GeneratedKeyframeIdSize::new(format_args!("{maximum}")).unwrap();
        assert_eq!(maximum_size.raw_bytes(), MAX_KEYFRAME_ID_BYTES);
        assert_eq!(
            maximum_size.serialized_bytes(),
            serde_json::to_vec(&maximum).unwrap().len()
        );

        let oversized = "x".repeat(MAX_KEYFRAME_ID_BYTES + 1);
        assert_eq!(
            GeneratedKeyframeIdSize::new(format_args!("{oversized}")),
            Err(ReservationError::InvalidGeneratedId {
                actual: MAX_KEYFRAME_ID_BYTES + 1,
                maximum: MAX_KEYFRAME_ID_BYTES,
            })
        );
    }

    #[test]
    fn prospective_keyframe_entries_match_actual_serde_for_supported_values() {
        assert_keyframed_entry_parity(
            PropertyTarget::layer(LayerId::new(1), PropType::Opacity),
            PropertyKeyframe::new(
                KeyframeId::new("float"),
                TimeOffset::from_millis(-1250),
                PropertyValue::Float(-12.5),
                PropertyKeyframeEasing::Linear,
            ),
        );
        assert_keyframed_entry_parity(
            PropertyTarget::layer(LayerId::new(2), PropType::RectSize),
            PropertyKeyframe::new(
                KeyframeId::new("vector"),
                TimeOffset::from_millis(0),
                PropertyValue::Vector2([1920.25, 1080.5]),
                PropertyKeyframeEasing::Hold,
            ),
        );
        assert_keyframed_entry_parity(
            PropertyTarget::layer(LayerId::new(3), PropType::FillColor),
            PropertyKeyframe::new(
                KeyframeId::new("color"),
                TimeOffset::from_millis(250),
                PropertyValue::Color([0.125, 0.25, 0.5, 0.75]),
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.1,
                    y1: -0.25,
                    x2: 0.8,
                    y2: 1.5,
                },
            ),
        );
        assert_keyframed_entry_parity(
            PropertyTarget::fx_item(FxItemId::new(4), "stroke\"join\\mode"),
            PropertyKeyframe::new(
                KeyframeId::new("string-\"\\\n"),
                TimeOffset::from_millis(500),
                PropertyValue::String("round\"\\\n雪".into()),
                PropertyKeyframeEasing::Hold,
            ),
        );
        assert_keyframed_entry_parity(
            PropertyTarget::layer(LayerId::new(5), PropType::PositionX),
            PropertyKeyframe::new(
                KeyframeId::new("spatial"),
                TimeOffset::from_millis(750),
                PropertyValue::Float(50.0),
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.25,
                    y1: 0.1,
                    x2: 0.75,
                    y2: 0.9,
                },
            )
            .with_spatial_tangents(Some(-12.25), Some(18.5)),
        );
    }

    #[test]
    fn constants_and_borrowed_path_animators_match_actual_serde() {
        let target = PropertyTarget::fx_item(FxItemId::new(9), "path\"copy");
        let value = PropertyValue::Path(ShapePath {
            commands: vec![
                ShapePathCommand::MoveTo {
                    x: 1.25,
                    y: -2.5,
                    mirror: None,
                    corner_radius: None,
                },
                ShapePathCommand::CubicTo {
                    c1x: 3.0,
                    c1y: 4.0,
                    c2x: 5.0,
                    c2y: 6.0,
                    x: 7.0,
                    y: 8.0,
                    mirror: None,
                    corner_radius: None,
                },
                ShapePathCommand::Close,
            ],
        });
        let animator = PropertyAnimator::constant(value.clone()).unwrap();
        let actual = entry(target.clone(), animator.clone());
        let actual_bytes = serde_json::to_vec(&actual).unwrap().len();

        assert_eq!(
            constant_entry_serialized_bytes(&target, &value).unwrap(),
            actual_bytes
        );
        assert_eq!(
            borrowed_animator_entry_serialized_bytes(&target, &animator).unwrap(),
            actual_bytes
        );
        assert_eq!(
            constant_entry_reservation_bytes(&target, &value).unwrap(),
            actual_bytes + GRAPH_ENTRY_SEPARATOR_BYTES
        );
        assert_eq!(
            borrowed_animator_entry_reservation_bytes(&target, &animator).unwrap(),
            actual_bytes + GRAPH_ENTRY_SEPARATOR_BYTES
        );
        assert_eq!(
            committed_entry_serialized_bytes(&actual).unwrap(),
            actual_bytes
        );
    }

    #[test]
    fn copied_keys_are_sized_from_borrowed_values_and_replacement_ids() {
        let target = PropertyTarget::layer(LayerId::new(42), PropType::PositionX);
        let source_keys = [
            PropertyKeyframe::new(
                KeyframeId::new("source-a"),
                TimeOffset::from_millis(-500),
                PropertyValue::Float(10.25),
                PropertyKeyframeEasing::Linear,
            )
            .with_spatial_tangents(Some(-1.0), Some(2.5)),
            PropertyKeyframe::new(
                KeyframeId::new("source-b"),
                TimeOffset::from_millis(1250),
                PropertyValue::Float(20.75),
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: -0.1,
                    x2: 0.9,
                    y2: 1.2,
                },
            )
            .with_spatial_tangents(Some(-3.5), Some(4.0)),
        ];
        let mut estimate = PropertyTrackEstimate::default();
        for (index, source) in source_keys.iter().enumerate() {
            let id =
                GeneratedKeyframeIdSize::new(format_args!("aep-copy-{target}-{index}")).unwrap();
            estimate.push_copied(source, &id).unwrap();
        }

        let copied = source_keys
            .iter()
            .enumerate()
            .map(|(index, source)| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("aep-copy-{target}-{index}")),
                    source.layer_time(),
                    source.value().clone(),
                    source.easing(),
                )
                .with_spatial_tangents(source.spatial_in_tangent(), source.spatial_out_tangent())
            })
            .collect();
        let actual = entry(
            target.clone(),
            PropertyAnimator::keyframes(PropertyKeyframeTrack::new(copied).unwrap()),
        );
        assert_eq!(
            estimate.entry_serialized_bytes(&target).unwrap(),
            serde_json::to_vec(&actual).unwrap().len()
        );
    }

    #[test]
    fn time_remap_estimate_matches_actual_serde_including_exact_maximum_time() {
        const MAX_EXACT_MILLIS: u64 = (1_u64 << 53) - 1;
        let specifications = [
            (
                "aep-remap-1-0",
                Time::ZERO,
                Time::ZERO,
                PropertyKeyframeEasing::Linear,
            ),
            (
                "aep-clip-1-1",
                Time::from_millis(1_000),
                Time::from_millis(500),
                PropertyKeyframeEasing::Hold,
            ),
            (
                "aep-time-1-2",
                Time::from_millis(MAX_EXACT_MILLIS),
                Time::from_millis(MAX_EXACT_MILLIS),
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.125,
                    y1: 0.25,
                    x2: 0.875,
                    y2: 0.75,
                },
            ),
        ];
        let mut estimate = TimeRemapEstimate::default();
        let mut keys = Vec::new();
        for (id, time, value, easing) in specifications {
            let id_size = GeneratedKeyframeIdSize::new(format_args!("{id}")).unwrap();
            estimate.push_key(&id_size, time, value, easing).unwrap();
            keys.push(TimeRemapKeyframe {
                id: KeyframeId::new(id),
                time,
                value,
                easing,
            });
        }
        let remap = TimeRemapProperty::new(
            keys,
            TimeRemapExtrapolation::Inactive,
            TimeRemapExtrapolation::Inactive,
        )
        .unwrap();
        let actual = serde_json::to_vec(&remap).unwrap().len();
        assert_eq!(
            estimate
                .serialized_bytes(
                    TimeRemapExtrapolation::Inactive,
                    TimeRemapExtrapolation::Inactive,
                )
                .unwrap(),
            actual
        );
        assert_eq!(
            estimate
                .reservation_bytes(
                    TimeRemapExtrapolation::Inactive,
                    TimeRemapExtrapolation::Inactive,
                )
                .unwrap(),
            actual + INLINE_PROPERTY_ATTACHMENT_BYTES
        );
    }
}
