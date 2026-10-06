//! Nested-sequence placements: another sequence played as one clip.
//!
//! Premiere nests a sequence by placing a track item whose source is that
//! sequence. The placement plays the inner timeline from `in_ticks` to
//! `out_ticks` over its own range at one constant forward input rate, normal
//! speed when both ranges have one length, and clips it to the placement.
//! Bounded unit reverse instead reflects the saved input window about the
//! sequence source's OriginalDuration, retaining that origin separately.
//! With TimeRemapping these bound input to the curve, not the source picture. Its
//! picture is the inner canvas, which its Motion places in the outer canvas
//! as a clip's places its media frame, and the inner timeline keeps its own
//! frame rate. Where the inner timeline has no picture the nest is
//! transparent: the AME render of the derived
//! `premiere_isolated_nested_sequence` fixture shows the red outer clip at
//! outer 9-10 s. Each placement owns a copy of the inner timeline, so
//! both conversion directions edit copies independently. Copies of one native
//! sequence keep its GUID and share its media records.

use super::{
    FrameRate, MediaId, PrBlendMode, PrEffect, PrEffectParamKeys, PrEffectParams, PrKeyframeEasing,
    PrLinearWipe, PrMask, PrMedia, PrPropertyAnimation, PrSequence, PrStaticCrop,
    PrStaticTransform, PrTrackMatte, PrVideoItem, PrVideoOccurrence, PrVideoTrack,
};
use crate::{
    format::{ensure_valid, invalid, Result},
    omit, Omission, OmissionScope,
};
use std::{collections::BTreeMap, ops::Range};

/// Sequence levels that one timeline may contain below itself.
/// The 47 staged corpus cases reach depth 3; this bounds recursive stack use.
pub(crate) const MAX_NEST_DEPTH: usize = 8;

/// Differing-canvas Transform admission shared by native reading and editable
/// lowering. Native pixels establish source/source Geometry2 points and
/// source/parent Motion points only for static, positive uniform-scale and
/// translation on square-pixel 16:9 canvases. Equal-canvas admission is unchanged.
pub(crate) fn nested_transform_canvas_reason(
    frame: [u32; 2],
    canvas: [u32; 2],
    motion: &PrStaticTransform,
    opacity: f64,
    motion_keyed: bool,
    effect: &PrEffect,
) -> Option<&'static str> {
    if frame == canvas {
        return None;
    }
    let wide = |[width, height]: [u32; 2]| {
        width != 0 && height != 0 && u64::from(width) * 9 == u64::from(height) * 16
    };
    if !wide(frame) || !wide(canvas) {
        return Some("differing-canvas nested Transform requires proportional 16:9 canvases; other point bases are unmeasured");
    }
    if motion_keyed || !effect.animations.is_empty() {
        return Some("differing-canvas nested Transform requires static Motion/Opacity and Transform controls; keyed point/rotation semantics are unmeasured");
    }
    let PrEffectParams::Transform(transform) = &effect.params else {
        return Some("differing-canvas nested Transform requires affine parameters");
    };
    if motion.scale[0] <= 0.0
        || motion.scale[0] != motion.scale[1]
        || motion.rotation != 0.0
        || !transform.uniform_scale
        || transform.scale_height <= 0.0
        || transform.rotation != 0.0
        || transform.skew != 0.0
        || transform.skew_axis != 0.0
    {
        return Some("differing-canvas nested Transform requires positive uniform scale and translation without rotation or skew");
    }
    if opacity != 100.0 || transform.opacity != 100.0 {
        return Some("differing-canvas nested Transform requires full Motion and Transform Opacity; fractional alpha is unmeasured");
    }
    None
}

/// Import-only equal-height translation and same-width taller rotation envelopes.
/// Both use source/source Geometry2 before source/parent Motion and retain the
/// pre-effect source guide without a stationary post-effect clip. Native reading
/// separately requires Geometry2; ordinary Transform and export keep the old gate.
/// Translation changes no colour/opacity control, so intrinsic Opacity retains
/// the existing nested Motion mapping rather than a new effect-opacity rule.
pub(crate) fn nested_transform_import_canvas_reason(
    frame: [u32; 2],
    canvas: [u32; 2],
    motion: &PrStaticTransform,
    opacity: f64,
    animations: &[PrPropertyAnimation],
    effect: &PrEffect,
) -> Option<&'static str> {
    let reason = nested_transform_canvas_reason(
        frame,
        canvas,
        motion,
        opacity,
        !animations.is_empty(),
        effect,
    );
    reason?;
    let PrEffectParams::Transform(transform) = &effect.params else {
        return reason;
    };
    let bounded_easing = |easing| match easing {
        PrKeyframeEasing::Linear => true,
        PrKeyframeEasing::CubicBezier { y1, y2, .. } => {
            (0.0..=1.0).contains(&y1) && (0.0..=1.0).contains(&y2)
        }
        PrKeyframeEasing::Hold => false,
    };
    let motion_keys = animations.iter().all(|animation| match animation {
        PrPropertyAnimation::Position(keys) => {
            keys.iter()
                .all(|key| key.value[0] == 0.5 && bounded_easing(key.easing))
                && super::spatial::curved_segment(keys).is_none()
        }
        PrPropertyAnimation::UniformScale(keys) => keys
            .iter()
            .all(|key| key.value >= 0.0 && bounded_easing(key.easing)),
        PrPropertyAnimation::Opacity(keys) => keys
            .iter()
            .all(|key| (0.0..=100.0).contains(&key.value) && bounded_easing(key.easing)),
        PrPropertyAnimation::AnchorPoint(_)
        | PrPropertyAnimation::Rotation(_)
        | PrPropertyAnimation::ScaleWidth(_) => false,
    });
    let [animation] = effect.animations.as_slice() else {
        return reason;
    };
    if let PrEffectParamKeys::Scalar(keys) = &animation.keys {
        let rotation = animation.param.id == super::TRANSFORM_ROTATION.id
            && keys.len() >= 2
            && keys.iter().all(|key| {
                key.value.is_finite()
                    && matches!(
                        key.easing,
                        PrKeyframeEasing::Linear | PrKeyframeEasing::CubicBezier { .. }
                    )
            });
        let admitted = frame[0] > 0
            && frame[0] == canvas[0]
            && canvas[1] > 0
            && frame[1] > canvas[1]
            && motion.anchor_point == [0.5; 2]
            && motion.position[0] == 0.5
            && motion.scale[0] > 0.0
            && motion.scale[0] == motion.scale[1]
            && motion.rotation == 0.0
            && opacity == 100.0
            && motion_keys
            && animations
                .iter()
                .all(|animation| matches!(animation, PrPropertyAnimation::Position(_)))
            && effect.enabled
            && effect.mask.is_none()
            && transform.anchor_point == transform.position
            && transform.uniform_scale
            && transform.scale_height == 100.0
            && transform.scale_width == 100.0
            && transform.skew == 0.0
            && transform.skew_axis == 0.0
            && transform.opacity == 100.0
            && transform.motion_blur_shutter_angle().is_none()
            && !transform.bicubic_sampling
            && rotation;
        return if admitted { None } else { reason };
    }
    let PrEffectParamKeys::Point(keys) = &animation.keys else {
        return reason;
    };
    let translated = animation.param.id == super::TRANSFORM_POSITION.id
        && keys.len() >= 2
        && keys.first().is_some_and(|key| key.value == [0.5; 2])
        && keys
            .iter()
            .all(|key| key.value[1] == 0.5 && bounded_easing(key.easing))
        && super::spatial::curved_segment(keys).is_none();
    let admitted = frame[0] > 0
        && frame[0] < canvas[0]
        && frame[1] > 0
        && frame[1] == canvas[1]
        && motion.anchor_point[0] == 0.5
        && motion.position[0] == 0.5
        && motion.scale[0] >= 0.0
        && motion.scale[0] == motion.scale[1]
        && motion.rotation == 0.0
        && opacity == 100.0
        && motion_keys
        && effect.enabled
        && effect.mask.is_none()
        && transform.anchor_point == [0.5; 2]
        && transform.position == [0.5; 2]
        && transform.uniform_scale
        && transform.scale_height == 100.0
        && transform.scale_width == 100.0
        && transform.rotation == 0.0
        && transform.skew == 0.0
        && transform.skew_axis == 0.0
        && transform.opacity == 100.0
        && transform.motion_blur_shutter_angle().is_none()
        && !transform.bicubic_sampling
        && translated;
    if admitted {
        None
    } else {
        reason
    }
}

/// One placement of another sequence on a video track.
#[derive(Debug, Clone)]
pub(crate) struct PrNestOccurrence {
    /// Native track item identity when loaded from Premiere.
    pub(crate) id: Option<String>,
    pub(crate) start_ticks: i64,
    pub(crate) end_ticks: i64,
    /// Input time at `start_ticks`; inner-sequence time without a remap.
    pub(crate) in_ticks: i64,
    /// Input time at `end_ticks`; inner-sequence time without a remap.
    pub(crate) out_ticks: i64,
    /// Signed input-clock speed. Reverse In/Out stay on the saved Clip clock.
    pub(crate) playback_rate: f64,
    /// Saved sequence-source OriginalDuration, retained only for reverse so its
    /// input window can reflect independently of the current inner duration.
    pub(crate) reverse_source_duration: Option<i64>,
    /// Source curve with signed input key times after In, before division by speed.
    pub(crate) time_remap: Option<super::PrTimeRemap>,
    /// The placement's own edits, with the meaning of the `PrVideoOccurrence`
    /// fields of the same name; the frame they apply to is the nested
    /// sequence's canvas, and effects retain their side of Linear Wipe.
    /// Import reads them neutral except a standalone static zero-feather Crop,
    /// a Track Matte Key on a nest of the outer canvas, which keys its
    /// canvas-sized picture, a Blend Mode, which blends it, its Opacity and,
    /// unless the nest is retimed, Opacity keys, which fade it, and its Motion
    /// and Motion keys without a Track Matte Key, which move it. Crop clips
    /// the nested canvas before Motion; Motion Crop remains unsupported. One
    /// static vector intrinsic Opacity mask also converts on equal canvases
    /// with unit-forward matching clocks, without another mask or Transform.
    pub(crate) transform: PrStaticTransform,
    pub(crate) opacity: f64,
    pub(crate) blend_mode: PrBlendMode,
    pub(crate) animations: Vec<PrPropertyAnimation>,
    pub(crate) crop: PrStaticCrop,
    pub(crate) linear_wipe: Option<PrLinearWipe>,
    /// Intrinsic Opacity coverage, after occurrence effects and before Motion.
    pub(crate) opacity_mask: Option<PrMask>,
    pub(crate) track_matte: Option<PrTrackMatte>,
    /// Occurrence effects in native render order, after the inner picture and
    /// before intrinsic Motion. Import reuses clip mappings on a picture Group;
    /// intrinsic Opacity coverage has a separate owner above it. Effect-owned
    /// masks and combinations with Crop or Track Matte remain excluded. The
    /// separate unmasked Transform path retains one affine effect; differing
    /// canvases require the static measured envelope. Edited native export
    /// remains without independent Adobe proof.
    pub(crate) effects: Vec<PrEffect>,
    /// Retained effects before Linear Wipe, using the clip mask-boundary count.
    pub(crate) effects_above_mask: usize,
    /// Effective picture output, flattened from clip Enable and track output
    /// as for a media occurrence (`PrVideoOccurrence::enabled`).
    pub(crate) enabled: bool,
    /// The placed timeline, at its own frame rate and canvas.
    pub(crate) sequence: PrSequence,
}

impl PrNestOccurrence {
    /// Whether the placement plays its inner window at another rate than its
    /// own range: the window from In to Out has another length.
    pub(crate) fn is_retimed(&self) -> bool {
        self.playback_rate < 0.0
            || self.time_remap.is_some()
            || i128::from(self.out_ticks) - i128::from(self.in_ticks)
                != i128::from(self.end_ticks) - i128::from(self.start_ticks)
    }

    pub(crate) fn reverse_source_window(&self) -> Result<Range<i64>> {
        let duration = self.reverse_source_duration.ok_or_else(|| {
            invalid("reverse nested sequence has no saved source OriginalDuration")
        })?;
        ensure_valid!(
            duration > 0
                && self.in_ticks >= 0
                && self.out_ticks > self.in_ticks
                && self.out_ticks <= duration,
            "reverse nested sequence input window exceeds saved OriginalDuration"
        );
        Ok(duration - self.out_ticks..duration - self.in_ticks)
    }

    /// Whether the placement plays its inner timeline on the inner clock in
    /// a sequence at `frame_rate`: a retimed placement, and one whose
    /// sequence has another frame rate, whose In and Out a trim on the outer
    /// frames can put between inner frames. Such a window trims no inner
    /// clip: the nest's group maps its placement onto the window instead.
    pub(crate) fn plays_inner_clock(&self, frame_rate: FrameRate) -> bool {
        self.is_retimed() || self.sequence.frame_rate != frame_rate
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

    /// Checks bounded, coherent placement/source clocks. Tick bounds can lie
    /// between samples, including after a parent origin is rounded to milliseconds.
    pub(crate) fn validate(
        &self,
        frame_rate: FrameRate,
        media: &BTreeMap<MediaId, PrMedia>,
    ) -> Result<()> {
        ensure_valid!(
            self.start_ticks >= 0
                && self.end_ticks > self.start_ticks
                && self.in_ticks >= 0
                && self.out_ticks > self.in_ticks,
            "invalid timeline/source ranges"
        );
        ensure_valid!(
            self.effects_above_mask == 0 || self.effects_above_mask == self.effects.len(),
            "nested effects must stay on one side of Linear Wipe"
        );
        PrVideoOccurrence::validate_edits(
            self.opacity,
            self.transform,
            self.crop,
            &self.animations,
            self.linear_wipe.as_ref(),
            &self.effects,
        )?;
        if let Some(mask) = &self.opacity_mask {
            mask.validate()?;
            ensure_valid!(
                mask.raster.is_none() && mask.path_keys.is_empty()
                    && self.crop.is_default() && self.linear_wipe.is_none()
                    && self.track_matte.is_none() && self.playback_rate == 1.0
                    && self.time_remap.is_none() && self.sequence.frame_rate == frame_rate,
                "nested Opacity mask requires a static vector outline, a unit-forward matching clock and no other masks"
            );
        }
        let inner = &self.sequence;
        if self.playback_rate < 0.0 {
            ensure_valid!(
                self.playback_rate == -1.0
                    && self.time_remap.is_none()
                    && inner.frame_rate == frame_rate
                    && super::source_span_matches(
                        self.end_ticks - self.start_ticks,
                        self.out_ticks - self.in_ticks,
                        self.playback_rate,
                    ),
                "reverse nested sequence requires unit reverse playback and matching frame clocks"
            );
            let window = self.reverse_source_window()?;
            ensure_valid!(
                window.end <= inner.end_ticks(),
                "reflected reverse nested sequence window exceeds the current inner timeline"
            );
            ensure_valid!(
                window.start % inner.frame_rate.ticks_per_frame() == 0
                    && window.end % inner.frame_rate.ticks_per_frame() == 0,
                "reflected reverse nested sequence window must align to inner frame boundaries"
            );
            ensure_valid!(
                self.effects.iter().all(|effect| !matches!(effect.params, PrEffectParams::Transform(_)))
                    && (self.effects.is_empty() || self.crop.is_default())
                    && self.linear_wipe.is_none()
                    && self.opacity_mask.is_none()
                    && self.track_matte.is_none(),
                "reverse nested Transform and masked occurrence effect/coverage clocks are not converted"
            );
        } else {
            ensure_valid!(
                self.reverse_source_duration.is_none(),
                "forward nested sequence cannot carry a reverse source duration"
            );
        }
        // Both Adobe-saved corpus nests end within their sequence. What
        // Premiere does with a longer placement is unverified, so a lengthened
        // group does not export and such a native placement does not import.
        ensure_valid!(
            self.playback_rate < 0.0 || self.time_remap.is_some()
                || self.out_ticks <= inner.end_ticks(),
            "out point {} ticks is past the {} tick end of nested sequence {:?}; a placement longer than its nested sequence is not supported",
            self.out_ticks,
            inner.end_ticks(),
            inner.name
        );
        if let Some(remap) = &self.time_remap {
            ensure_valid!(
                self.playback_rate > 0.0
                    && super::source_span_matches(
                        self.end_ticks - self.start_ticks,
                        self.out_ticks - self.in_ticks,
                        self.playback_rate,
                    ),
                "nested TimeRemapping requires a matching forward input window"
            );
            let end = inner.end_ticks();
            let duration = self.out_ticks - self.in_ticks;
            let bounded_tail = matches!(remap.keys.as_slice(), [.., previous, last]
            if last.source_ticks > end
                && last.easing == super::PrKeyframeEasing::Linear
                && super::linear_tail_within_media(
                    i128::from(previous.timeline_ticks)..i128::from(last.timeline_ticks),
                    i128::from(previous.source_ticks)..i128::from(last.source_ticks),
                    i128::from(duration), i128::from(end),
                ));
            ensure_valid!(
                matches!(remap.keys.as_slice(), [first, .., last]
                    if first.timeline_ticks <= 0 && last.timeline_ticks >= duration)
                    && remap.keys.len() >= 2
                    && remap.keys.iter().enumerate().all(|(index, key)| {
                        (0..=end).contains(&key.source_ticks)
                            || (bounded_tail && index + 1 == remap.keys.len())
                    })
                    && remap.keys.windows(2).all(|pair| {
                        pair[0].timeline_ticks < pair[1].timeline_ticks
                            && pair[0].source_ticks < pair[1].source_ticks
                    })
                    && (remap.ramp_modes().is_some()
                        || remap
                            .keys
                            .iter()
                            .all(|key| key.easing == super::PrKeyframeEasing::Linear)),
                "invalid, uncovered or out-of-bounds nested TimeRemapping curve"
            );
        }
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
                nest.validate(self.native_frame_rate(), media)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{
        PrEffectParamAnimation, PrPointKeyframe, PrScalarKeyframe, PrTransform, TRANSFORM_POSITION,
    };

    fn point(value: [f64; 2], ticks: i64) -> PrPointKeyframe {
        PrPointKeyframe {
            source_ticks: ticks,
            value,
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        }
    }
    fn scalar(value: f64, ticks: i64) -> PrScalarKeyframe {
        PrScalarKeyframe {
            source_ticks: ticks,
            value,
            easing: PrKeyframeEasing::Linear,
        }
    }
    fn profile() -> (PrStaticTransform, Vec<PrPropertyAnimation>, PrEffect) {
        let motion = PrStaticTransform {
            anchor_point: [0.5, 1.02],
            position: [0.5, 0.82],
            scale: [0.0; 2],
            ..PrStaticTransform::default()
        };
        let animations = vec![
            PrPropertyAnimation::Position(vec![
                point([0.5, 0.82], 0),
                point([0.5, 0.74], super::super::TICKS),
            ]),
            PrPropertyAnimation::UniformScale(vec![
                scalar(0.0, 0),
                scalar(68.0, super::super::TICKS),
            ]),
            PrPropertyAnimation::Opacity(vec![scalar(100.0, 0), scalar(0.0, super::super::TICKS)]),
        ];
        let effect = PrEffect {
            enabled: true,
            mask: None,
            params: PrEffectParams::Transform(PrTransform {
                anchor_point: [0.5; 2],
                position: [0.5; 2],
                uniform_scale: true,
                scale_height: 100.0,
                scale_width: 100.0,
                skew: 0.0,
                skew_axis: 0.0,
                rotation: 0.0,
                opacity: 100.0,
                composition_shutter_angle: true,
                shutter_angle: 0.0,
                bicubic_sampling: false,
            }),
            animations: vec![PrEffectParamAnimation {
                param: &TRANSFORM_POSITION,
                keys: PrEffectParamKeys::Point(vec![
                    point([0.5; 2], 0),
                    point([-0.04, 0.5], super::super::TICKS),
                ]),
            }],
        };
        (motion, animations, effect)
    }
    fn tall_rotation_profile() -> (PrStaticTransform, Vec<PrPropertyAnimation>, PrEffect) {
        let (_, _, mut effect) = profile();
        let motion = PrStaticTransform {
            position: [0.5, -0.1],
            scale: [89.0; 2],
            ..PrStaticTransform::default()
        };
        let animations = vec![PrPropertyAnimation::Position(vec![
            point([0.5, -0.1], 0),
            point([0.5, 1.0], super::super::TICKS),
        ])];
        let PrEffectParams::Transform(transform) = &mut effect.params else {
            unreachable!()
        };
        transform.anchor_point = [0.3, 1.6];
        transform.position = transform.anchor_point;
        transform.rotation = 170.0;
        let mut first = scalar(170.0, 0);
        first.easing = PrKeyframeEasing::CubicBezier {
            x1: 0.35,
            y1: -0.02,
            x2: 0.0,
            y2: 1.002,
        };
        effect.animations = vec![PrEffectParamAnimation {
            param: &super::super::TRANSFORM_ROTATION,
            keys: PrEffectParamKeys::Scalar(vec![first, scalar(0.0, super::super::TICKS)]),
        }];
        (motion, animations, effect)
    }

    #[test]
    fn tall_canvas_keyed_rotation_admits_import_without_widening_export() {
        let (motion, animations, effect) = tall_rotation_profile();
        assert!(nested_transform_import_canvas_reason(
            [1920, 2900],
            [1920, 1080],
            &motion,
            100.0,
            &animations,
            &effect
        )
        .is_none());
        assert!(nested_transform_canvas_reason(
            [1920, 2900],
            [1920, 1080],
            &motion,
            100.0,
            true,
            &effect
        )
        .is_some());
    }

    #[test]
    fn tall_canvas_keyed_rotation_refuses_other_aspects_and_intrinsic_controls() {
        let (motion, animations, effect) = tall_rotation_profile();
        for (frame, canvas) in [
            ([1080, 2900], [1920, 1080]),
            ([1920, 960], [1920, 1080]),
            ([1920, 2900], [1920, 0]),
        ] {
            assert!(nested_transform_import_canvas_reason(
                frame,
                canvas,
                &motion,
                100.0,
                &animations,
                &effect
            )
            .is_some());
        }
        for case in 0..6 {
            let mut changed = motion;
            match case {
                0 => changed.anchor_point[1] = 0.4,
                1 => changed.position[0] = 0.4,
                2 => changed.scale = [0.0; 2],
                3 => changed.scale = [89.0, 90.0],
                4 => changed.rotation = 1.0,
                _ => changed.scale = [-89.0; 2],
            }
            assert!(
                nested_transform_import_canvas_reason(
                    [1920, 2900],
                    [1920, 1080],
                    &changed,
                    100.0,
                    &animations,
                    &effect
                )
                .is_some(),
                "case{case}"
            );
        }
        let mut curved = animations.clone();
        let PrPropertyAnimation::Position(keys) = &mut curved[0] else {
            unreachable!()
        };
        keys[0].spatial_out_tangent = Some([0.1, 0.0]);
        assert!(nested_transform_import_canvas_reason(
            [1920, 2900],
            [1920, 1080],
            &motion,
            100.0,
            &curved,
            &effect
        )
        .is_some());
        let mut scaled = animations;
        scaled.push(PrPropertyAnimation::UniformScale(vec![scalar(89.0, 0)]));
        assert!(nested_transform_import_canvas_reason(
            [1920, 2900],
            [1920, 1080],
            &motion,
            100.0,
            &scaled,
            &effect
        )
        .is_some());
    }

    #[test]
    fn tall_canvas_keyed_rotation_refuses_unmeasured_effect_controls() {
        let (motion, animations, effect) = tall_rotation_profile();
        for case in 0..9 {
            let mut changed = effect.clone();
            let PrEffectParams::Transform(transform) = &mut changed.params else {
                unreachable!()
            };
            match case {
                0 => transform.position[1] += 0.1,
                1 => transform.opacity = 50.0,
                2 => transform.scale_height = 120.0,
                3 => transform.skew = 1.0,
                4 => transform.bicubic_sampling = true,
                5 => {
                    transform.composition_shutter_angle = false;
                    transform.shutter_angle = 90.0;
                }
                6 => changed.enabled = false,
                7 => changed.animations[0].param = &TRANSFORM_POSITION,
                _ => {
                    let PrEffectParamKeys::Scalar(keys) = &mut changed.animations[0].keys else {
                        unreachable!()
                    };
                    keys[0].easing = PrKeyframeEasing::Hold;
                }
            }
            assert!(
                nested_transform_import_canvas_reason(
                    [1920, 2900],
                    [1920, 1080],
                    &motion,
                    100.0,
                    &animations,
                    &changed
                )
                .is_some(),
                "case{case}"
            );
        }
    }

    #[test]
    fn square_canvas_keyed_translation_admits_import_without_widening_export() {
        let (motion, animations, effect) = profile();
        assert!(nested_transform_import_canvas_reason(
            [1080, 1080],
            [1920, 1080],
            &motion,
            100.0,
            &animations,
            &effect
        )
        .is_none());
        assert!(nested_transform_canvas_reason(
            [1080, 1080],
            [1920, 1080],
            &motion,
            100.0,
            true,
            &effect
        )
        .is_some());
    }
    #[test]
    fn square_canvas_keyed_translation_keeps_other_aspects_and_motion_guards() {
        let (motion, animations, effect) = profile();
        for (frame, canvas) in [
            ([1080, 1080], [1920, 720]),
            ([1920, 1080], [1080, 1080]),
            ([1920, 2900], [1920, 1080]),
        ] {
            assert!(nested_transform_import_canvas_reason(
                frame,
                canvas,
                &motion,
                100.0,
                &animations,
                &effect
            )
            .is_some());
        }
        for (index, mut changed) in [motion; 3].into_iter().enumerate() {
            match index {
                0 => changed.rotation = 1.0,
                1 => changed.scale = [1.0, 2.0],
                _ => changed.anchor_point[0] = 0.4,
            }
            assert!(nested_transform_import_canvas_reason(
                [1080, 1080],
                [1920, 1080],
                &changed,
                100.0,
                &animations,
                &effect
            )
            .is_some());
        }
        let mut changed = animations;
        changed.push(PrPropertyAnimation::AnchorPoint(vec![point([0.5; 2], 0)]));
        assert!(nested_transform_import_canvas_reason(
            [1080, 1080],
            [1920, 1080],
            &motion,
            100.0,
            &changed,
            &effect
        )
        .is_some());
    }
    #[test]
    fn square_canvas_keyed_translation_refuses_curves_and_unmeasured_effect_controls() {
        let (motion, animations, effect) = profile();
        for case in 0..6 {
            let mut changed = effect.clone();
            let PrEffectParams::Transform(transform) = &mut changed.params else {
                unreachable!()
            };
            match case {
                0 => transform.rotation = 1.0,
                1 => transform.opacity = 50.0,
                2 => transform.scale_height = 120.0,
                3 => transform.bicubic_sampling = true,
                4 | 5 => {
                    let PrEffectParamKeys::Point(keys) = &mut changed.animations[0].keys else {
                        unreachable!()
                    };
                    if case == 4 {
                        keys[0].spatial_out_tangent = Some([0.0, 0.1]);
                    } else {
                        keys[1].easing = PrKeyframeEasing::Hold;
                    }
                }
                _ => unreachable!(),
            }
            assert!(
                nested_transform_import_canvas_reason(
                    [1080, 1080],
                    [1920, 1080],
                    &motion,
                    100.0,
                    &animations,
                    &changed
                )
                .is_some(),
                "case{case}"
            );
        }
    }
}
