//! Nested-sequence placements: another sequence played as one clip.
//!
//! Premiere nests a sequence by placing a track item whose source is that
//! sequence. The placement plays the inner timeline from `in_ticks` to
//! `out_ticks` over its own range at one constant forward rate, normal speed
//! when both ranges have one length, and clips it to the placement. Where the
//! inner timeline has no picture the nest is transparent: the AME render of
//! the derived `premiere_isolated_nested_sequence` fixture shows the red outer
//! clip at outer 9-10 s. Each placement owns a copy of the inner timeline, so
//! both conversion directions edit copies independently. Copies of one native
//! sequence keep its GUID and share its media records.

use super::{
    FrameRate, MediaId, PrBlendMode, PrEffect, PrLinearWipe, PrMedia, PrPropertyAnimation,
    PrSequence, PrStaticCrop, PrStaticTransform, PrTrackMatte, PrVideoItem, PrVideoOccurrence,
    PrVideoTrack,
};
use crate::{
    format::{ensure_valid, invalid, Result},
    omit, Omission, OmissionScope,
};
use std::{collections::BTreeMap, ops::Range};

/// Sequence levels that one timeline may contain below itself.
/// The 47 staged corpus cases reach depth 3; this bounds recursive stack use.
pub(crate) const MAX_NEST_DEPTH: usize = 8;

/// One placement of another sequence on a video track.
#[derive(Debug, Clone)]
pub(crate) struct PrNestOccurrence {
    /// Native track item identity when loaded from Premiere.
    pub(crate) id: Option<String>,
    pub(crate) start_ticks: i64,
    pub(crate) end_ticks: i64,
    /// Inner-sequence time shown at `start_ticks`.
    pub(crate) in_ticks: i64,
    /// Inner-sequence time reached at `end_ticks`.
    pub(crate) out_ticks: i64,
    /// The placement's own edits, with the meaning of the `PrVideoOccurrence`
    /// fields of the same name; the frame they apply to is the nested
    /// sequence's canvas, and its effects apply after its Crop or Linear Wipe.
    /// Import reads them neutral: it omits a nest with any of them, except
    /// a Track Matte Key, which keys the nest's canvas-sized picture, a Blend
    /// Mode, which blends it, and its Motion and Motion keys without a Track
    /// Matte Key, which move it.
    pub(crate) transform: PrStaticTransform,
    pub(crate) opacity: f64,
    pub(crate) blend_mode: PrBlendMode,
    pub(crate) animations: Vec<PrPropertyAnimation>,
    pub(crate) crop: PrStaticCrop,
    pub(crate) linear_wipe: Option<PrLinearWipe>,
    pub(crate) track_matte: Option<PrTrackMatte>,
    pub(crate) effects: Vec<PrEffect>,
    /// Effective picture output, flattened from clip Enable and track output
    /// as for a media occurrence (`PrVideoOccurrence::enabled`).
    pub(crate) enabled: bool,
    /// The placed timeline. It has the outer canvas, and the outer frame
    /// rate unless the placement is retimed.
    pub(crate) sequence: PrSequence,
}

impl PrNestOccurrence {
    /// Whether the placement plays its inner window at another rate than its
    /// own range: the window from In to Out has another length.
    pub(crate) fn is_retimed(&self) -> bool {
        i128::from(self.out_ticks) - i128::from(self.in_ticks)
            != i128::from(self.end_ticks) - i128::from(self.start_ticks)
    }

    /// One group plus every inline copy it owns.
    pub(crate) fn expanded_occurrence_count(&self) -> usize {
        self.sequence.expanded_occurrence_count().saturating_add(1)
    }

    pub(crate) fn timeline_ticks(&self) -> Range<i64> {
        self.start_ticks..self.end_ticks
    }

    pub(crate) fn overlaps(&self, range: &Range<i64>) -> bool {
        self.start_ticks < range.end && range.start < self.end_ticks
    }

    /// Error and omission context: the native item, else the inner sequence name.
    pub(crate) fn record(&self) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| format!("nested sequence {:?}", self.sequence.name))
    }

    /// Checks one placement against the frame rate and canvas of its outer
    /// sequence. A retimed placement may nest another frame rate, and its
    /// saved In and Out may lie off both frame grids. Such a window trims no
    /// inner clip: the nest's group maps its placement onto the window
    /// instead.
    pub(crate) fn validate(
        &self,
        frame_rate: FrameRate,
        dimensions: [u32; 2],
        media: &BTreeMap<MediaId, PrMedia>,
    ) -> Result<()> {
        ensure_valid!(
            self.start_ticks >= 0
                && self.end_ticks > self.start_ticks
                && self.in_ticks >= 0
                && self.out_ticks > self.in_ticks,
            "invalid timeline/source ranges"
        );
        PrVideoOccurrence::validate_edits(
            self.opacity,
            self.transform,
            self.crop,
            &self.animations,
            self.linear_wipe.as_ref(),
            &self.effects,
        )?;
        // The placement stays on sequence frames. At normal speed so does its
        // window, at which the nest trims its inner clips, so no inner clip
        // is clipped to a sliver shorter than one frame.
        let retimed = self.is_retimed();
        for (name, ticks, framed) in [
            ("timeline start", self.start_ticks, true),
            ("timeline end", self.end_ticks, true),
            ("nested sequence in point", self.in_ticks, !retimed),
            ("nested sequence out point", self.out_ticks, !retimed),
        ] {
            ensure_valid!(
                !framed || ticks % frame_rate.ticks_per_frame() == 0,
                "{name} must align to a {frame_rate} sequence frame boundary"
            );
        }
        let inner = &self.sequence;
        ensure_valid!(
            inner.frame_rate == frame_rate || retimed,
            "nested sequence {:?} runs at {} inside a {frame_rate} sequence over a window as long as its placement; such a mixed-rate nest is not supported",
            inner.name,
            inner.frame_rate
        );
        ensure_valid!(
            inner.dimensions() == dimensions,
            "nested sequence {:?} canvas {}x{} differs from the outer {}x{} canvas",
            inner.name,
            inner.width,
            inner.height,
            dimensions[0],
            dimensions[1]
        );
        // Both Adobe-saved corpus nests end within their sequence. What
        // Premiere does with a longer placement is unverified, so a lengthened
        // group does not export and such a native placement does not import.
        ensure_valid!(
            self.out_ticks <= inner.end_ticks(),
            "out point {} ticks is past the {} tick end of nested sequence {:?}; a placement longer than its nested sequence is not supported",
            self.out_ticks,
            inner.end_ticks(),
            inner.name
        );
        inner.validate_timeline(media)
    }
}

impl PrVideoTrack {
    /// Whether an item or nested placement on this lane shares time with `range`.
    pub(crate) fn overlaps(&self, range: &Range<i64>) -> bool {
        self.items.iter().any(|item| {
            let item = item.timeline_ticks();
            item.start < range.end && range.start < item.end
        }) || self.nests.iter().any(|nest| nest.overlaps(range))
    }
}

impl PrSequence {
    /// Nested placements in track-major order, from the bottom track upward.
    pub(crate) fn nest_occurrences(&self) -> impl Iterator<Item = &PrNestOccurrence> {
        self.video_tracks.iter().flat_map(|track| &track.nests)
    }

    /// Whether a sound plays in this timeline or in a nest inside it. A nest
    /// plays the sound of its timeline through its one audio item, whose gain
    /// the reader folds into that sound.
    pub(crate) fn has_sound(&self) -> bool {
        !self.audio.is_empty()
            || self
                .nest_occurrences()
                .any(|nest| nest.sequence.has_sound())
    }

    /// Whether this timeline or a nested timeline places sound from `media`.
    pub(crate) fn uses_audio_media(&self, media: &MediaId) -> bool {
        self.audio.iter().any(|clip| &clip.media == media)
            || self
                .nest_occurrences()
                .any(|nest| nest.sequence.uses_audio_media(media))
    }

    /// Placements after every nest becomes one group of inline copies: each
    /// media or graphic item at any level, plus one group per nested placement.
    pub(crate) fn expanded_occurrence_count(&self) -> usize {
        self.nest_occurrences().fold(
            self.video_items().count() + self.audio.len(),
            |count, nest| count.saturating_add(nest.expanded_occurrence_count()),
        )
    }

    /// Media placed only inside nested sequences, in first-appearance order.
    pub(crate) fn nested_media(&self) -> Vec<&MediaId> {
        self.nest_occurrences()
            .flat_map(|nest| nest.sequence.media_in_order())
            .collect()
    }

    /// Drops nested media placements for which `omitted` returns a reason, and
    /// then nested placements left without content, reporting each.
    pub(crate) fn retain_nested_media(
        &mut self,
        omitted: &impl Fn(&MediaId) -> Option<String>,
        omissions: &mut Vec<Omission>,
    ) {
        for track in &mut self.video_tracks {
            track.nests.retain_mut(|nest| {
                let inner = &mut nest.sequence;
                for inner_track in &mut inner.video_tracks {
                    inner_track.items.retain(|item| {
                        let PrVideoItem::Media(clip) = item else {
                            return true;
                        };
                        match omitted(&clip.media) {
                            None => true,
                            Some(reason) => {
                                omit(omissions, OmissionScope::Occurrence, clip.record(), reason);
                                false
                            }
                        }
                    });
                }
                inner.audio.retain(|clip| match omitted(&clip.media) {
                    None => true,
                    Some(reason) => {
                        omit(omissions, OmissionScope::Occurrence, clip.record(), reason);
                        false
                    }
                });
                inner.retain_nested_media(omitted, omissions);
                let keep = inner.video_items().next().is_some()
                    || inner.nest_occurrences().next().is_some()
                    || !inner.audio.is_empty();
                if !keep {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        nest.record(),
                        "nested sequence has no convertible video occurrences",
                    );
                }
                keep
            });
        }
    }

    fn nest_depth(&self) -> usize {
        self.nest_occurrences()
            .map(|nest| nest.sequence.nest_depth().saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    /// Checks nested placements. A lane holds media and nested placements in
    /// one ordered, non-overlapping sequence.
    pub(crate) fn validate_nests(&self, media: &BTreeMap<MediaId, PrMedia>) -> Result<()> {
        ensure_valid!(
            self.nest_depth() <= MAX_NEST_DEPTH,
            "sequence {:?}: nesting deeper than {MAX_NEST_DEPTH} levels is not supported",
            self.name
        );
        for (index, track) in self.video_tracks.iter().enumerate() {
            for nest in &track.nests {
                nest.validate(self.frame_rate, self.dimensions(), media)
                    .map_err(|error| {
                        invalid(format!(
                            "sequence {:?}, video track {index}, {}: {error}",
                            self.name,
                            nest.record()
                        ))
                    })?;
            }
            let mut ranges: Vec<_> = track
                .items
                .iter()
                .map(PrVideoItem::timeline_ticks)
                .chain(track.nests.iter().map(PrNestOccurrence::timeline_ticks))
                .collect();
            ranges.sort_by_key(|range| range.start);
            ensure_valid!(
                track
                    .nests
                    .windows(2)
                    .all(|pair| pair[0].start_ticks < pair[1].start_ticks)
                    && ranges.windows(2).all(|pair| pair[0].end <= pair[1].start),
                "sequence {:?}, video track {index}: nested placements must be ordered and must not overlap other placements on the same track",
                self.name
            );
        }
        Ok(())
    }
}
