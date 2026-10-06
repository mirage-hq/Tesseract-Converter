//! Timeline rules shared by decoding, conversion, and encoding.

use super::{
    FrameRate, MediaId, PrAudioOccurrence, PrAudioStream, PrEffect, PrGraphic, PrKeyframeEasing,
    PrLinearWipe, PrMedia, PrPropertyAnimation, PrSequence, PrStaticCrop, PrStaticTransform,
    PrTimeRemap, PrTrackMatte, PrVideoItem, PrVideoOccurrence, PrVideoTrack,
};
use crate::format::{ensure_valid, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

// Native reading and writing share this bound: export can write a final-frame
// hold, and reimport must accept the same file. Source-in, timeline start, and
// timeline end each round to the nearest millisecond, at most 0.5 ms each.
// The two frame-snap errors sum to at most one sequence frame, which the check
// below already removes when it locates the final sample.
const END_SAMPLE_ROUNDING_TICKS: i64 = 3 * super::TICKS_PER_MILLISECOND / 2;

// Compatibility precision, not a claim about Adobe's internal rounding:
// tolerate one nanosecond of saved source-time residue without changing any
// authored endpoints or speed. This is unrelated to frame/sample admission.
const SOURCE_SPAN_PRECISION_TICKS: f64 = super::TICKS as f64 / 1_000_000_000.0;

/// Whether a source span agrees with a constant forward or reverse speed.
/// Premiere's placement and source endpoints share one tick domain, regardless
/// of frame rate. Admit one nanosecond of source-time serialization residue,
/// plus a conservative binary64 error bound; do not reconstruct the speed.
pub(crate) fn source_span_matches(timeline_ticks: i64, source_ticks: i64, rate: f64) -> bool {
    if timeline_ticks <= 0 || source_ticks <= 0 || !rate.is_finite() || rate == 0.0 {
        return false;
    }
    let expected = timeline_ticks as f64 * rate.abs();
    let actual = source_ticks as f64;
    if !expected.is_finite() {
        return false;
    }
    // Five rounded stages: timeline integer conversion, rate representation,
    // product, source integer conversion and subtraction. With unit roundoff
    // u = EPSILON / 2, their absolute errors sum to at most (6u + O(u²)) *
    // max(expected, actual). 8 * EPSILON conservatively covers this, use of
    // rounded magnitudes, and rounding of the bound/comparison itself. Since
    // actual >= 1 tick, it also dominates subnormal rate/product absolute error.
    // Multiply by the small factor first so a finite product cannot overflow
    // while computing its allowance.
    let arithmetic_error = (8.0 * f64::EPSILON) * expected.max(actual);
    let tolerance = SOURCE_SPAN_PRECISION_TICKS + arithmetic_error;
    let discrepancy = (actual - expected).abs();
    tolerance.is_finite() && discrepancy.is_finite() && discrepancy <= tolerance
}

/// Bounds a forward linear tail at the played endpoint, without rounding its slope.
/// An endpoint before the tail starts is bounded by the tail's first source value.
/// Callers separately check chronology, coverage and all preceding source keys.
pub(crate) fn linear_tail_within_media(
    input: Range<i128>,
    source: Range<i128>,
    played_end: i128,
    media_end: i128,
) -> bool {
    if input.start >= input.end
        || source.start < 0
        || source.start >= source.end
        || source.start > media_end
        || played_end > input.end
    {
        return false;
    }
    let bounded = || {
        let elapsed = played_end.max(input.start).checked_sub(input.start)?;
        let rise = source.end.checked_sub(source.start)?;
        let span = input.end.checked_sub(input.start)?;
        let available = media_end.checked_sub(source.start)?;
        Some(elapsed.checked_mul(rise)? <= available.checked_mul(span)?)
    };
    bounded().unwrap_or(false)
}

impl PrSequence {
    /// Saved placement/source grid, before any editable sampling approximation.
    pub(crate) fn native_frame_rate(&self) -> FrameRate {
        self.native_frame_ticks
            .and_then(FrameRate::from_sequence_ticks)
            .unwrap_or(self.frame_rate)
    }

    pub(crate) fn validate_occurrence_count(count: usize) -> Result<()> {
        ensure_valid!(count > 0, "requires at least one media occurrence");
        Ok(())
    }

    /// The video timeline end or the exact last converted audio end, whichever is later.
    pub(crate) fn end_ticks(&self) -> i64 {
        self.audio
            .iter()
            .map(|clip| clip.end_ticks)
            .fold(self.timeline_end_ticks, i64::max)
    }

    /// The last media occurrence end. The timeline may extend past it.
    pub(crate) fn occurrence_end_ticks(&self) -> i64 {
        self.video_items()
            .map(|item| item.timeline_ticks().end)
            .chain(self.audio.iter().map(|clip| clip.end_ticks))
            .chain(self.nest_occurrences().map(|nest| nest.end_ticks))
            .max()
            .unwrap_or(0)
    }

    /// Intervals up to the sequence end that no opaque media covers. Text
    /// graphics are transparent and an adjustment layer only alters the
    /// picture beneath it, so time either covers alone is still a gap.
    #[cfg(test)]
    pub(crate) fn gaps(&self, media: &BTreeMap<MediaId, PrMedia>) -> Vec<Range<i64>> {
        let mut ranges: Vec<_> = self
            .video_occurrences()
            // Disabled occurrences show no picture; the timeline end still counts them.
            .filter(|clip| clip.enabled)
            .filter(|clip| {
                media
                    .get(&clip.media)
                    .is_some_and(|facts| !facts.is_adjustment())
            })
            .map(PrVideoOccurrence::timeline_ticks)
            .collect();
        ranges.sort_by_key(|range| range.start);
        let mut end = 0;
        let mut gaps = Vec::new();
        for range in ranges {
            if end < range.start {
                gaps.push(end..range.start);
            }
            end = end.max(range.end);
        }
        // Sound, text or disabled clips past the last picture leave an uncovered tail.
        let sequence_end = self.end_ticks();
        if end < sequence_end {
            gaps.push(end..sequence_end);
        }
        gaps
    }

    pub(crate) fn validate_timeline(&self, media: &BTreeMap<MediaId, PrMedia>) -> Result<()> {
        Self::validate_occurrence_count(
            self.video_items().count() + self.nest_occurrences().count() + self.audio.len(),
        )?;
        ensure_valid!(
            self.width > 0 && self.height > 0,
            "sequence {:?}: dimensions must be positive",
            self.name
        );
        ensure_valid!(
            self.native_frame_ticks.is_none_or(|ticks| ticks > 0),
            "native sequence grid must be positive"
        );
        for (index, track) in self.video_tracks.iter().enumerate() {
            for item in &track.items {
                let checked = match item {
                    PrVideoItem::Media(clip) => {
                        let facts = media.get(&clip.media).ok_or_else(|| {
                            crate::format::invalid(format!(
                                "sequence {:?}: unknown media {}",
                                self.name, clip.media
                            ))
                        })?;
                        clip.validate_on_grid(
                            self.frame_rate,
                            self.native_frame_ticks
                                .unwrap_or(self.frame_rate.ticks_per_frame()),
                            facts,
                        )
                    }
                    PrVideoItem::Graphic(graphic) => graphic.validate(self.native_frame_rate()),
                    PrVideoItem::Capsule(capsule) => capsule
                        .validate()
                        .map_err(|error| crate::format::invalid(error.to_string())),
                };
                checked.map_err(|error| {
                    crate::format::invalid(format!(
                        "sequence {:?}, video track {index}, {}: {error}",
                        self.name,
                        item_label(item)
                    ))
                })?;
            }
            for pair in track.items.windows(2) {
                ensure_valid!(
                    pair[0].timeline_ticks().end <= pair[1].timeline_ticks().start,
                    "sequence {:?}, video track {index}, {} and {}: video occurrences must be ordered and non-overlapping on the same track",
                    self.name,
                    item_label(&pair[0]),
                    item_label(&pair[1])
                );
            }
            // Every Track Matte Key names a matte that the reader resolved or
            // the export placed; an unresolvable one would write a clip whole.
            let keyed = track
                .items
                .iter()
                .filter_map(PrVideoItem::media)
                .filter_map(|clip| {
                    Some((clip.id.as_deref(), clip.timeline_ticks(), clip.track_matte?))
                })
                .chain(track.nests.iter().filter_map(|nest| {
                    Some((nest.id.as_deref(), nest.timeline_ticks(), nest.track_matte?))
                }));
            for (id, range, matte) in keyed {
                check_track_matte(&self.video_tracks, index, range.clone(), matte).map_err(
                    |reason| {
                        crate::format::invalid(format!(
                            "sequence {:?}, video track {index}, {}: {reason}",
                            self.name,
                            id.unwrap_or("placement")
                        ))
                    },
                )?;
            }
        }
        for clip in &self.audio {
            let source = clip.record();
            let stream = media
                .get(&clip.media)
                .and_then(|facts| facts.audio.as_ref())
                .ok_or_else(|| {
                    crate::format::invalid(format!(
                        "sequence {:?}, audio {source}: unknown audio media",
                        self.name
                    ))
                })?;
            clip.validate(stream).map_err(|error| {
                crate::format::invalid(format!("sequence {:?}, audio {source}: {error}", self.name))
            })?;
        }
        self.validate_nests(media)?;
        // After the occurrences, so that a malformed clip reports its own error.
        let occurrence_end = self.occurrence_end_ticks();
        ensure_valid!(
            self.end_ticks() >= occurrence_end,
            "sequence {:?}: timeline end {} ms precedes the last media occurrence end {} ms; trim or delete the occurrences past the timeline end",
            self.name,
            nearest_millisecond(self.end_ticks()),
            nearest_millisecond(occurrence_end)
        );
        Ok(())
    }
}

impl PrAudioOccurrence {
    /// Checks one sound placement against its stream. Sound has no frame grid,
    /// so only the shared rounding allowance may pass the media end.
    pub(crate) fn validate(&self, stream: &PrAudioStream) -> Result<()> {
        ensure_valid!(
            self.source_channel
                .as_ref()
                .is_none_or(|selection| stream.channels == super::AudioChannels::Stereo
                    && selection.channel() < 2),
            "mono source-channel selection requires stereo media and channel 0 or 1"
        );
        let intrinsic_ticks = stream.intrinsic_ticks;
        ensure_valid!(
            self.start_ticks >= 0
                && self.end_ticks > self.start_ticks
                && self.in_ticks >= 0
                && self.out_ticks > self.in_ticks
                && self.in_ticks < intrinsic_ticks,
            "invalid timeline/source ranges"
        );
        ensure_valid!(
            self.playback_rate.is_finite() && self.playback_rate != 0.0,
            "audio playback rate must be finite and nonzero"
        );
        ensure_valid!(
            self.playback_rate != 1.0
                || self.end_ticks - self.start_ticks == self.out_ticks - self.in_ticks,
            "unit audio source and timeline durations differ"
        );
        if self.playback_rate != 1.0 {
            // Audio-only saved-clock consistency: allow one source millisecond
            // or 1% of the nominal source span, whichever is larger. Donor
            // clocks carry small non-sample-grid residuals; gross disagreements
            // must not silently turn a saved rate into a different editable one.
            let nominal = (self.end_ticks - self.start_ticks) as f64 * self.playback_rate.abs();
            let residual = ((self.out_ticks - self.in_ticks) as f64 - nominal).abs();
            let allowance = (super::TICKS_PER_MILLISECOND as f64).max(nominal * 0.01);
            ensure_valid!(nominal.is_finite() && residual <= allowance,
                "audio saved speed and source/timeline spans disagree beyond one source millisecond or 1% of the nominal span");
        }
        ensure_valid!(
            i128::from(self.out_ticks)
                <= i128::from(intrinsic_ticks) + i128::from(END_SAMPLE_ROUNDING_TICKS),
            "source range passes the media end and rounding allowance"
        );
        if let Some(volume) = &self.volume_keys {
            ensure_valid!(
                !volume.keys.is_empty(),
                "clip Volume requires at least one key"
            );
            // The gain multiplies every key, so each product must stay finite.
            ensure_valid!(
                volume.gain.is_finite()
                    && volume.gain >= 0.0
                    && volume
                        .keys
                        .iter()
                        .all(|key| key.value >= 0.0 && (key.value * volume.gain).is_finite()),
                "clip Volume keys must be finite, nonnegative gains"
            );
            ensure_valid!(
                volume
                    .keys
                    .windows(2)
                    .all(|pair| pair[0].source_ticks < pair[1].source_ticks)
                    && volume
                        .keys
                        .iter()
                        .all(|key| !matches!(key.easing, PrKeyframeEasing::CubicBezier { .. })),
                "clip Volume keys must be Linear or Hold, with strictly increasing source times"
            );
        }
        let fade_ticks = [&self.fade_in, &self.fade_out]
            .into_iter()
            .flatten()
            .try_fold(0_i64, |total, fade| {
                (fade.duration_ticks > 0)
                    .then(|| total.checked_add(fade.duration_ticks))
                    .flatten()
            });
        ensure_valid!(
            fade_ticks.is_some_and(|ticks| ticks <= self.end_ticks - self.start_ticks),
            "audio fades must lie within the placement without overlapping"
        );
        Ok(())
    }
}

fn item_label(item: &PrVideoItem) -> &str {
    match item {
        PrVideoItem::Media(clip) => clip.record(),
        PrVideoItem::Graphic(graphic) => graphic.id.as_deref().unwrap_or("graphic"),
        PrVideoItem::Capsule(capsule) => capsule.placement.id.as_deref().unwrap_or("Capsule"),
    }
}

/// The one native provider overlapping a consumer, on its own sequence clock.
/// `covered_range` bounds activation without changing either authored mapping.
#[derive(Debug)]
pub(crate) struct TrackMatteProvider<'a> {
    pub(crate) record: Option<&'a str>,
    pub(crate) range: Range<i64>,
    pub(crate) covered_range: Range<i64>,
}

/// Checks that a key has one enabled, unambiguous provider above its track.
/// A provider contained by the consumer is recoverable: import restricts the
/// consumer without extending the provider. A provider outside the consumer is
/// rejected because consuming it would hide its independently painting span.
pub(crate) fn check_track_matte(
    tracks: &[PrVideoTrack],
    track_index: usize,
    range: Range<i64>,
    matte: PrTrackMatte,
) -> std::result::Result<(), String> {
    track_matte_provider(tracks, track_index, range, matte).map(|_| ())
}

/// Resolves the actual provider span/identity, never the consumer's start.
/// Independent masked providers remain excluded: their generated guide/stage
/// owners cannot be substituted by an unclipped child or a guessed picture.
pub(crate) fn track_matte_provider(
    tracks: &[PrVideoTrack],
    track_index: usize,
    range: Range<i64>,
    matte: PrTrackMatte,
) -> std::result::Result<TrackMatteProvider<'_>, String> {
    let matte_track = tracks.get(matte.track_index).ok_or_else(|| {
        format!(
            "Track Matte Key names video track {}, which the sequence does not have",
            matte.track_index
        )
    })?;
    if matte.track_index <= track_index {
        return Err(format!(
            "Track Matte Key names video track {}, which is not above the clip's track {track_index}",
            matte.track_index
        ));
    }
    let overlaps = |other: &Range<i64>| other.start < range.end && range.start < other.end;
    if matte_track.items.iter().any(|item| matches!(item,
        PrVideoItem::Graphic(graphic) if overlaps(&graphic.timeline_ticks()) && graphic.objects.iter().any(|object|
            object.mask_source().is_some() || matches!(object, super::text::PrGraphicObject::Group(_))))) {
        return Err("Track Matte Key using a graphic with Mask with Shape/Text or SubGroups is unverified".to_owned());
    }
    if let Some(loss) = matte_track.items.iter().find_map(|item| match item {
        PrVideoItem::Graphic(graphic) if overlaps(&graphic.timeline_ticks()) => {
            graphic.effect_loss.as_ref()
        }
        _ => None,
    }) {
        return Err(format!(
            "Track Matte Key using a graphic with omitted Ramp ({}) is not converted: luma changes and graphic-host alpha coverage is unverified",
            loss.ramp_component
        ));
    }
    // Each matte candidate's range, Enable and independent coverage owners.
    let mut sources = matte_track
        .items
        .iter()
        .map(|item| match item {
            PrVideoItem::Media(clip) => (
                clip.timeline_ticks(),
                clip.enabled,
                !clip.crop.is_default()
                    || clip.linear_wipe.is_some()
                    || clip.opacity_mask.is_some()
                    || clip.track_matte.is_some(),
                clip.id.as_deref(),
            ),
            PrVideoItem::Graphic(graphic) => (
                graphic.timeline_ticks(),
                graphic.enabled,
                graphic.opacity_mask.is_some(),
                graphic.id.as_deref(),
            ),
            PrVideoItem::Capsule(capsule) => (
                capsule.placement.timeline_ticks(),
                capsule.placement.enabled,
                capsule.placement.opacity_mask.is_some(),
                capsule.placement.id.as_deref(),
            ),
        })
        .chain(matte_track.nests.iter().map(|nest| {
            (
                nest.timeline_ticks(),
                nest.enabled,
                nest.track_matte.is_some() || nest.opacity_mask.is_some(),
                nest.id.as_deref(),
            )
        }))
        .filter(|(source_range, ..)| overlaps(source_range));
    let (source_range, enabled, masked, record) = match (sources.next(), sources.next()) {
        (None, _) => {
            return Err(format!(
                "the matte track {} holds no clip over the clip's range",
                matte.track_index
            ))
        }
        (Some(_), Some(_)) => {
            return Err(format!(
                "the matte track {} holds more than one clip over the clip's range; only one unambiguous provider converts",
                matte.track_index
            ))
        }
        (Some(source), None) => source,
    };
    if source_range.start < range.start || source_range.end > range.end {
        return Err(format!(
            "the matte clip spans {}..{} ticks outside the clip's {}..{}; its independently painting interval cannot be consumed as an FX matte",
            source_range.start, source_range.end, range.start, range.end
        ));
    }
    if !enabled {
        return Err(
            "the matte clip is disabled or on a muted track; FX drops a hidden matte source and would show the clip whole"
                .to_owned(),
        );
    }
    if masked {
        return Err(
            "the matte clip has its own Crop, Linear Wipe, Opacity mask or Track Matte Key"
                .to_owned(),
        );
    }
    Ok(TrackMatteProvider {
        record,
        covered_range: source_range.start.max(range.start)..source_range.end.min(range.end),
        range: source_range,
    })
}

impl PrStaticTransform {
    /// Checks the static Motion values against Premiere's parameter bounds.
    pub(crate) fn validate(&self) -> Result<()> {
        ensure_valid!(
            self.position
                .iter()
                .chain(&self.anchor_point)
                .all(|value| value.is_finite())
                && self
                    .scale
                    .iter()
                    .all(|value| (0.0..=10_000.0).contains(value))
                && (-32_768.0..=32_767.0).contains(&self.rotation),
            "static Motion values are outside Premiere's supported range"
        );
        Ok(())
    }
}

impl PrGraphic {
    /// The measured static mask frame does not cover a graphic moved by clip Motion.
    pub(crate) fn validate_clip_motion_mask(&self) -> Result<()> {
        ensure_valid!(
            self.clip_motion == PrStaticTransform::default() || self.opacity_mask.is_none(),
            "a graphic with clip Motion and a clip Opacity mask is not converted: its mask frame is unmeasured"
        );
        Ok(())
    }

    // Cadence selects samples; it does not constrain a graphic's tick bounds.
    pub(crate) fn validate(&self, _sequence_rate: FrameRate) -> Result<()> {
        ensure_valid!(
            self.start_ticks >= 0 && self.end_ticks > self.start_ticks,
            "invalid graphic timeline range"
        );
        if let Some(motion) = &self.vector_motion {
            motion.validate()?;
        }
        // The clip Motion and Opacity follow the video rules; the Motion has
        // no keys.
        self.clip_motion.validate()?;
        self.validate_clip_motion_mask()?;
        ensure_valid!(
            self.opacity.is_finite() && (0.0..=100.0).contains(&self.opacity),
            "graphic clip opacity must be finite and between 0 and 100"
        );
        if let Some(mask) = &self.opacity_mask {
            mask.validate()?;
        }
        for (index, animation) in self.animations.iter().enumerate() {
            let PrPropertyAnimation::Opacity(keys) = animation else {
                return Err(crate::format::invalid(
                    "graphic clip Motion keys are unsupported",
                ));
            };
            ensure_valid!(index == 0, "duplicate graphic clip Opacity animation");
            animation.validate_keys()?;
            ensure_valid!(
                keys.iter().all(|key| (0.0..=100.0).contains(&key.value))
                    && keys
                        .windows(2)
                        .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
                "graphic clip Opacity keys must be between 0 and 100 at strictly increasing times"
            );
        }
        ensure_valid!(!self.objects.is_empty(), "a graphic must have an object");
        self.objects
            .iter()
            .try_for_each(super::text::PrGraphicObject::validate)
    }
}

/// Whole milliseconds, nearest with ties forward, for tick positions in messages.
fn nearest_millisecond(ticks: i64) -> i64 {
    ticks
        .saturating_add(super::TICKS_PER_MILLISECOND / 2)
        .div_euclid(super::TICKS_PER_MILLISECOND)
}

impl PrVideoOccurrence {
    /// Checks one occurrence against its sequence frame rate and media facts.
    /// The media keeps its own frame rate.
    pub(crate) fn validate(&self, sequence_rate: FrameRate, facts: &PrMedia) -> Result<()> {
        self.validate_on_grid(sequence_rate, sequence_rate.ticks_per_frame(), facts)
    }

    /// Curve-only admission. Required placement, media, edits and grid checks
    /// remain in `validate_on_grid`; a rejected curve is not a valid base clock.
    pub(crate) fn validate_time_remap_curve(
        &self,
        remap: &PrTimeRemap,
        facts: &super::PrVideoStream,
    ) -> Result<()> {
        let intrinsic_ticks = facts
            .interpreted_duration()
            .map_err(|error| crate::format::invalid(error.to_string()))?;
        let active_duration = self
            .end_ticks
            .checked_sub(self.start_ticks)
            .ok_or_else(|| crate::format::invalid("timeline duration exceeds tick range"))?;
        let source_duration = self
            .out_ticks
            .checked_sub(self.in_ticks)
            .ok_or_else(|| crate::format::invalid("source duration exceeds tick range"))?;
        let span_at_rate =
            source_span_matches(active_duration, source_duration, self.playback_rate);
        // Key times are input ticks after In. An explicit FrameHold maps
        // the whole unit-speed placement to one source instant. A native
        // curve, with increasing source times, plays its input from In to
        // Out at the forward speed (`after_source_in`); Premiere's clock
        // from another In or speed is measured on physical video.
        let frame_hold = remap.held_source_ticks(active_duration).is_some();
        if frame_hold {
            ensure_valid!(
                self.playback_rate == 1.0,
                "constant playback combined with TimeRemapping is unsupported"
            );
            ensure_valid!(
                self.out_ticks - self.in_ticks == active_duration,
                "trimmed TimeRemapping clips are not supported"
            );
        } else {
            // Unit-speed remap input keeps exact placement length; other
            // speeds use the shared source-span consistency precision.
            let span_matches = if self.playback_rate == 1.0 {
                self.out_ticks - self.in_ticks == active_duration
            } else {
                span_at_rate
            };
            ensure_valid!(
                self.playback_rate > 0.0 && span_matches,
                "TimeRemapping In to Out must match the clip's forward playback rate"
            );
            ensure_valid!(
                !self.remaps_from_in_or_speed()
                    || matches!(facts.kind, super::PrMediaKind::Video { .. }),
                "TimeRemapping from a source In or at another speed is converted only on physical video"
            );
        }
        ensure_valid!(
            matches!(remap.keys.as_slice(), [first, .., last]
                if first.timeline_ticks <= 0
                    && last.timeline_ticks >= self.out_ticks - self.in_ticks),
            "TimeRemapping keys do not cover the placement"
        );
        // Saved curves can keep an unused control key past the media end.
        // Only a final linear tail is admitted, and only while its played
        // part stays in bounds. Keep the key: it defines the tail's slope.
        let bounded_tail = matches!(remap.keys.as_slice(), [.., previous, last]
        if !frame_hold
            && matches!(facts.kind, super::PrMediaKind::Video { .. })
            && last.source_ticks > intrinsic_ticks
            && last.easing == PrKeyframeEasing::Linear
            && linear_tail_within_media(
                i128::from(previous.timeline_ticks)..i128::from(last.timeline_ticks),
                i128::from(previous.source_ticks)..i128::from(last.source_ticks),
                i128::from(self.out_ticks - self.in_ticks),
                i128::from(intrinsic_ticks),
            ));
        ensure_valid!(
            remap.keys.len() >= 2
                && remap.keys.iter().enumerate().all(|(index, key)| {
                    ((0..=intrinsic_ticks).contains(&key.source_ticks)
                        || (bounded_tail && index == remap.keys.len() - 1))
                        && (!frame_hold || key.source_ticks < intrinsic_ticks)
                })
                && remap.keys.windows(2).all(|pair| {
                    pair[0].timeline_ticks < pair[1].timeline_ticks
                        && (frame_hold || pair[0].source_ticks < pair[1].source_ticks)
                }),
            "invalid or unsupported TimeRemapping curve"
        );
        // A last key at the media end, like the one that Premiere 26.5.1
        // appends, ends a segment that is unmeasured from another In or
        // speed: In to Out must end by the key before it.
        let plays_media_end_segment = matches!(remap.keys.as_slice(), [.., penultimate, last]
            if last.source_ticks == intrinsic_ticks
                && penultimate.timeline_ticks < self.out_ticks - self.in_ticks);
        ensure_valid!(
            !(self.remaps_from_in_or_speed() && plays_media_end_segment),
            "TimeRemapping from a source In or at another speed plays past the key before the curve's media-end key"
        );
        Ok(())
    }

    /// Validate saved clip boundaries on the native grid, independently of the
    /// output sampling cadence used to import an approximated sequence.
    pub(crate) fn validate_on_grid(
        &self,
        sequence_rate: FrameRate,
        frame_ticks: i64,
        facts: &PrMedia,
    ) -> Result<()> {
        let stream = facts
            .video
            .as_ref()
            .ok_or_else(|| crate::format::invalid("source has no video stream"))?;
        self.validate_playback_on_grid(sequence_rate, frame_ticks, stream, self.time_remap.as_ref())
    }

    /// Check the independently saved constant clock without cloning or changing
    /// the occurrence. Optional-curve recovery must pass every ordinary base,
    /// property and native-grid check before publishing a fallback.
    pub(crate) fn validate_base_on_grid(
        &self,
        sequence_rate: FrameRate,
        frame_ticks: i64,
        facts: &super::PrVideoStream,
    ) -> Result<()> {
        self.validate_playback_on_grid(sequence_rate, frame_ticks, facts, None)
    }

    fn validate_playback_on_grid(
        &self,
        sequence_rate: FrameRate,
        frame_ticks: i64,
        facts: &super::PrVideoStream,
        remap: Option<&PrTimeRemap>,
    ) -> Result<()> {
        ensure_valid!(frame_ticks > 0, "native sequence grid must be positive");
        let intrinsic_ticks = facts
            .interpreted_duration()
            .map_err(|error| crate::format::invalid(error.to_string()))?;
        ensure_valid!(
            self.start_ticks >= 0
                && self.end_ticks > self.start_ticks
                && self.in_ticks >= 0
                && self.out_ticks > self.in_ticks
                && self.in_ticks < intrinsic_ticks,
            "invalid timeline/source ranges"
        );
        ensure_valid!(
            self.playback_rate.is_finite() && self.playback_rate != 0.0,
            "playback rate must be finite and nonzero"
        );
        let active_duration = self.end_ticks - self.start_ticks;
        let span_at_rate = source_span_matches(
            active_duration,
            self.out_ticks - self.in_ticks,
            self.playback_rate,
        );
        if let Some(remap) = remap {
            self.validate_time_remap_curve(remap, facts)?;
        } else {
            // A still has no media clock: Premiere 26.5.1 keeps a still's
            // 5 s source span when its placement is lengthened, and shows the
            // still over the whole placement (the Source Graphic save's grey
            // backdrop, 10 s, grey on every sampled AME frame to 9.97 s). At
            // unit forward rate its source clock then runs past the saved
            // Out, which the media-end check below bounds.
            let lengthened_still = facts.kind.is_still()
                && self.playback_rate == 1.0
                && self.out_ticks - self.in_ticks < active_duration;
            // A normal-forward still also keeps its saved span when shortened
            // (images/nests inner item123: 5 s source on a 2 s placement).
            ensure_valid!(
                span_at_rate || (facts.kind.is_still() && self.playback_rate == 1.0),
                "source span does not match the constant playback rate"
            );
            let source_end = if lengthened_still {
                self.in_ticks.checked_add(active_duration)
            } else {
                Some(self.out_ticks)
            };
            // Source-out is exclusive. A final partial frame can hold the
            // last source image; also admit the bounded ms-rounding error.
            ensure_valid!(
                source_end.is_some_and(|end| {
                    i128::from(end) - i128::from(sequence_rate.ticks_per_frame())
                        <= i128::from(intrinsic_ticks) + i128::from(END_SAMPLE_ROUNDING_TICKS)
                }),
                "source range requires a frame past the media end and rounding allowance"
            );
        }
        ensure_valid!(
            remap.is_none()
                || self.animations.is_empty()
                || (self.has_media_clock_rotation()
                    && matches!(facts.kind, super::PrMediaKind::Video { .. })),
            "Motion animation combined with TimeRemapping is unsupported"
        );
        Self::validate_edits(
            self.opacity,
            self.transform,
            self.crop,
            &self.animations,
            self.linear_wipe.as_ref(),
            &self.effects,
        )?;
        for (name, ticks) in [
            ("timeline start", self.start_ticks),
            ("timeline end", self.end_ticks),
        ] {
            ensure_valid!(
                ticks % frame_ticks == 0,
                "{name} must align to a frame boundary on the native {frame_ticks}-tick grid (editable sequence rate {sequence_rate})"
            );
        }
        Ok(())
    }

    /// Checks the Motion, Opacity, keys, Crop, Linear Wipe and effects of a
    /// media placement; a nest placement shares these rules.
    pub(super) fn validate_edits(
        opacity: f64,
        transform: PrStaticTransform,
        crop: PrStaticCrop,
        animations: &[PrPropertyAnimation],
        linear_wipe: Option<&PrLinearWipe>,
        effects: &[PrEffect],
    ) -> Result<()> {
        ensure_valid!(
            opacity.is_finite() && (0.0..=100.0).contains(&opacity),
            "opacity must be finite and between 0 and 100"
        );
        transform.validate()?;
        crop.validate()?;
        let mut animated_properties = BTreeSet::new();
        for animation in animations {
            ensure_valid!(
                animated_properties.insert(animation.property()),
                "duplicate {:?} animation",
                animation.property()
            );
            animation.validate_keys()?;
            if let PrPropertyAnimation::Opacity(keys) = animation {
                ensure_valid!(
                    keys.iter().all(|key| (0.0..=100.0).contains(&key.value)),
                    "opacity animation values must be between 0 and 100"
                );
            }
        }
        if let Some(wipe) = linear_wipe {
            ensure_valid!(
                matches!(wipe.angle_degrees, 0 | 90 | 180 | 270)
                    && (0.0..=100.0).contains(&wipe.initial_completion)
                    && wipe.feather.is_finite()
                    && (0.0..=32_000.0).contains(&wipe.feather),
                "Linear Wipe angle or feather is unsupported"
            );
            ensure_valid!(
                wipe.completion
                    .iter()
                    .all(|key| (0.0..=100.0).contains(&key.value))
                    && wipe
                        .completion
                        .windows(2)
                        .all(|pair| pair[0].source_ticks < pair[1].source_ticks),
                "Linear Wipe completion must have bounded, increasing keys"
            );
        }
        for effect in effects {
            effect.validate()?;
        }
        Ok(())
    }
}
