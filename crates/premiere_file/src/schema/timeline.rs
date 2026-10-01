//! Timeline rules shared by decoding, conversion, and encoding.

use super::{
    FrameRate, MediaId, PrAudioOccurrence, PrAudioStream, PrEffect, PrGraphic, PrKeyframeEasing,
    PrLinearWipe, PrMedia, PrPropertyAnimation, PrSequence, PrStaticCrop, PrStaticTransform,
    PrTrackMatte, PrVideoItem, PrVideoOccurrence, PrVideoTrack,
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

impl PrSequence {
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
                        clip.validate(self.frame_rate, facts)
                    }
                    PrVideoItem::Graphic(graphic) => graphic.validate(self.frame_rate),
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
        ensure_valid!(
            self.end_ticks() % self.frame_rate.ticks_per_frame() == 0
                || self
                    .audio
                    .iter()
                    .any(|clip| clip.end_ticks == self.end_ticks()),
            "sequence {:?}: timeline end must align to a {} sequence frame boundary",
            self.name,
            self.frame_rate
        );
        Ok(())
    }
}

impl PrAudioOccurrence {
    /// Checks one sound placement against its stream. Sound has no frame grid,
    /// so only the shared rounding allowance may pass the media end.
    pub(crate) fn validate(&self, stream: &PrAudioStream) -> Result<()> {
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
            self.end_ticks - self.start_ticks == self.out_ticks - self.in_ticks,
            "retiming is not supported"
        );
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
        Ok(())
    }
}

fn item_label(item: &PrVideoItem) -> &str {
    match item {
        PrVideoItem::Media(clip) => clip.record(),
        PrVideoItem::Graphic(graphic) => graphic.id.as_deref().unwrap_or("graphic"),
    }
}

/// Checks the matte of a placement on track `track_index` over `range` whose
/// Track Matte Key is `matte`, or says why the key does not convert: the
/// matte track is a track of `tracks` strictly above the placement's, and its
/// one item or nest over `range` spans exactly `range` and is enabled.
/// Premiere shows a matte clip outside the placement's range, which FX never
/// does, and FX drops a hidden matte source and shows the placement whole. A
/// matte item with its own Crop, Linear Wipe, Opacity mask or Track Matte Key
/// converts as more than one layer or another keyed clip, which neither
/// direction pairs with a consumer. The reader and the model validation share
/// this one rule.
pub(crate) fn check_track_matte(
    tracks: &[PrVideoTrack],
    track_index: usize,
    range: Range<i64>,
    matte: PrTrackMatte,
) -> std::result::Result<(), String> {
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
    // Each matte candidate's range, Enable and whether it is masked itself.
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
            ),
            PrVideoItem::Graphic(graphic) => (graphic.timeline_ticks(), graphic.enabled, false),
        })
        .chain(matte_track.nests.iter().map(|nest| {
            (
                nest.timeline_ticks(),
                nest.enabled,
                nest.track_matte.is_some(),
            )
        }))
        .filter(|(source_range, _, _)| overlaps(source_range));
    let (source_range, enabled, masked) = match (sources.next(), sources.next()) {
        (None, _) => {
            return Err(format!(
                "the matte track {} holds no clip over the clip's range",
                matte.track_index
            ))
        }
        (Some(_), Some(_)) => {
            return Err(format!(
                "the matte track {} holds more than one clip over the clip's range; only one matte clip spanning exactly that range converts",
                matte.track_index
            ))
        }
        (Some(source), None) => source,
    };
    if source_range != range {
        return Err(format!(
            "the matte clip spans {}..{} ticks, not the clip's {}..{}; only a matte clip spanning exactly the clip's range converts",
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
    Ok(())
}

impl PrGraphic {
    pub(crate) fn validate(&self, sequence_rate: FrameRate) -> Result<()> {
        ensure_valid!(
            self.start_ticks >= 0 && self.end_ticks > self.start_ticks,
            "invalid graphic timeline range"
        );
        for (name, ticks) in [
            ("timeline start", self.start_ticks),
            ("timeline end", self.end_ticks),
        ] {
            ensure_valid!(
                ticks % sequence_rate.ticks_per_frame() == 0,
                "{name} must align to a {sequence_rate} sequence frame boundary"
            );
        }
        if let Some(motion) = &self.vector_motion {
            motion.validate()?;
        }
        // The clip Opacity follows the video Opacity rules, and the clip's
        // Motion keeps its default.
        ensure_valid!(
            self.opacity.is_finite() && (0.0..=100.0).contains(&self.opacity),
            "graphic clip opacity must be finite and between 0 and 100"
        );
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
        let facts = facts
            .video
            .as_ref()
            .ok_or_else(|| crate::format::invalid("source has no video stream"))?;
        ensure_valid!(
            self.start_ticks >= 0
                && self.end_ticks > self.start_ticks
                && self.in_ticks >= 0
                && self.out_ticks > self.in_ticks
                && self.in_ticks < facts.intrinsic_ticks,
            "invalid timeline/source ranges"
        );
        ensure_valid!(
            self.playback_rate.is_finite() && self.playback_rate != 0.0,
            "playback rate must be finite and nonzero"
        );
        let active_duration = self.end_ticks - self.start_ticks;
        if let Some(remap) = &self.time_remap {
            // An explicit FrameHold maps the whole placement to one source
            // instant. Native variable-speed curves still require increasing
            // source times and an untrimmed clip.
            let frame_hold = matches!(remap.keys.as_slice(), [first, last]
                if first.timeline_ticks == 0
                    && last.timeline_ticks == active_duration
                    && first.source_ticks == last.source_ticks
                    && first.easing == super::PrKeyframeEasing::Linear
                    && last.easing == super::PrKeyframeEasing::Linear);
            ensure_valid!(
                self.playback_rate == 1.0,
                "constant playback combined with TimeRemapping is unsupported"
            );
            ensure_valid!(
                self.out_ticks - self.in_ticks == active_duration
                    && (frame_hold || self.in_ticks == 0 && self.out_ticks == active_duration),
                "trimmed TimeRemapping clips are not supported"
            );
            ensure_valid!(
                remap.keys.len() >= 2
                    && remap
                        .keys
                        .first()
                        .is_some_and(|key| key.timeline_ticks <= 0)
                    && remap
                        .keys
                        .last()
                        .is_some_and(|key| key.timeline_ticks >= active_duration)
                    && remap.keys.iter().all(|key| {
                        (0..=facts.intrinsic_ticks).contains(&key.source_ticks)
                            && (!frame_hold || key.source_ticks < facts.intrinsic_ticks)
                    })
                    && remap.keys.windows(2).all(|pair| {
                        pair[0].timeline_ticks < pair[1].timeline_ticks
                            && (frame_hold || pair[0].source_ticks < pair[1].source_ticks)
                    }),
                "invalid or unsupported TimeRemapping curve"
            );
        } else {
            let source_duration = (self.out_ticks - self.in_ticks) as f64;
            let expected_source_duration = active_duration as f64 * self.playback_rate.abs();
            // Adobe truncates the floating-point product to integer ticks. The pinned
            // 0.905x reverse fixture differs by two ticks after decimal reconstruction.
            let source_span_matches = (source_duration - expected_source_duration).abs() <= 4.0;
            ensure_valid!(
                source_span_matches,
                "source span does not match the constant playback rate"
            );
            // Source-out is exclusive. A final partial frame can hold the
            // last source image; also admit the bounded ms-rounding error.
            ensure_valid!(
                i128::from(self.out_ticks) - i128::from(sequence_rate.ticks_per_frame())
                    <= i128::from(facts.intrinsic_ticks) + i128::from(END_SAMPLE_ROUNDING_TICKS),
                "source range requires a frame past the media end and rounding allowance"
            );
        }
        ensure_valid!(
            self.time_remap.is_none() || self.animations.is_empty(),
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
                ticks % sequence_rate.ticks_per_frame() == 0,
                "{name} must align to a {sequence_rate} sequence frame boundary"
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
        ensure_valid!(
            transform
                .position
                .iter()
                .chain(&transform.anchor_point)
                .all(|value| value.is_finite())
                && transform
                    .scale
                    .iter()
                    .all(|value| (0.0..=10_000.0).contains(value))
                && (-32_768.0..=32_767.0).contains(&transform.rotation),
            "static Motion values are outside Premiere's supported range"
        );
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
                !wipe.completion.is_empty()
                    && wipe
                        .completion
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
