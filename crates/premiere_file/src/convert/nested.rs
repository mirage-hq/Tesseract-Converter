//! Nested sequences as groups of inline copies, in both directions.
//!
//! Import: each nested placement becomes one `GroupLayer` over the placement's
//! range. Its children are an independent copy of the part of the inner
//! timeline that the placement shows, on the group's clock, so the group moves
//! and trims as one unit. Inner gaps stay transparent because the inner black
//! canvas is not copied. Every copy refers to the shared media assets. The
//! placement's Track Matte Key becomes the group's track matte. A retimed
//! placement's children keep the inner clock instead, untrimmed, and its
//! group's linear playback maps the placement onto the inner window; its
//! Motion keys stay unconverted, as a retimed clip's do.
//!
//! Export: a group becomes a new inner sequence of its children, placed from
//! inner time zero over the group's range. The placement takes the group's
//! Motion, Opacity, their keys, its blend mode, its one Crop, Linear Wipe or
//! track matte, its effects and its Enable, as a clip takes its layer's. A
//! group with a background, a clock other than the plain one (a time remap or
//! another rate, as an imported retimed nest has), motion blur or another
//! mask has no exact nest and is omitted, and so is a group that exports no
//! picture.

use super::{
    background::{black_shape, identity_transform, plain_group},
    effects::{export_effects, EffectHost, EffectIdAllocator},
    packing::{PictureContainerToken, PicturePacker, SourceBoundaryToken},
    premiere_to_tesseract::{
        audio_layers, clip_transform, guide_layer, guide_mask, matte_layer, motion_tracks,
        set_tracks, tick_range, validate_time_range, video_layers, CompositionShutter,
    },
    tesseract_to_premiere::{
        animates_motion, canonical_mask, clip_omitted, export_layers, export_mask,
        export_motion_keys, export_transform, has_background, is_background_property,
        layer_animations, layer_tracks, stage_layers, stage_video, stage_video_spans_group,
        unexportable_clip, unexportable_motion, unplaced_track_matte, unsupported_image_masks,
        CanonicalMask, ClipLayers, MotionHost, WrittenAnimation,
    },
    timing::{frame_ticks_from_time, ticks_from_time, time_from_ticks},
};
use crate::{
    audio_media::SourceSound,
    error::{ensure, unsupported, BuildError, Result},
    export_loss::{omit_field, ExportField, OmissionSink},
    format::{FrameRate, MediaId, PrMedia, PrSequence, PrVideoItem},
    linked_compositions::LinkedCompositions,
    media::MediaFacts,
    schema::{
        PrAudioOccurrence, PrBlendMode, PrNestOccurrence, PrStaticTransform, PrVideoTrack,
        MAX_NEST_DEPTH,
    },
    {approximate, omit, Omission, OmissionScope},
};
use fx_schema::{
    animator::{PropertyKeyframeEasing, PropertyKeyframeTrack},
    AnimationGraph, AssetId, BlendMode, Duration, EffectData, EffectPayload, EffectRecord,
    FontAssetProperties, FxItemId, GroupLayer, Layer, LayerData, LayerEffect, LayerId,
    LayerPlayback, MotionBlurSettings, PropType, PropertyValue, Time, TimeRangeProperty,
};
use std::{collections::BTreeMap, ops::Range};

/// The root layer of each converted item and nest of one sequence, by track
/// index and timeline start, which a Track Matte Key on a lower track names.
#[derive(Debug, Clone, Copy)]
pub(super) enum ItemLayer {
    Plain(LayerId),
    /// The explicit outer physical-picture owner created by Stroke lowering.
    Stroke(LayerId),
}
impl ItemLayer {
    pub(super) fn id(self) -> LayerId {
        match self {
            Self::Plain(id) | Self::Stroke(id) => id,
        }
    }
}
pub(super) type ItemLayers = BTreeMap<(usize, i64), ItemLayer>;

/// Parent and document-wide identities for one inline layer list.
pub(super) struct LayerScope<'a, 'r, 'm> {
    pub(super) parent: Option<LayerId>,
    /// Index of this scope's first media layer. A layer id is its index plus one.
    pub(super) first_index: usize,
    /// First unassigned index, shared by every scope of one document.
    pub(super) next_index: &'a mut usize,
    /// Composition-unique effect ids, shared like `next_index`.
    pub(super) effect_ids: &'a mut EffectIdAllocator,
    /// The root layer of each item and nest of this list converted so far.
    pub(super) item_layers: ItemLayers,
    /// The composition's one motion-blur shutter, shared like `next_index`.
    pub(super) composition_shutter: &'a mut Option<CompositionShutter>,
    /// The sequence's linked compositions, whose pictures take identities
    /// from `next_index` and `effect_ids`; shared like them.
    pub(super) linked: &'a mut LinkedCompositions<'r, 'm>,
    /// Whether this scope's clock is the document clock: no enclosing nest
    /// group starts after the document start.
    pub(super) on_document_clock: bool,
}

impl<'a, 'r, 'm> LayerScope<'a, 'r, 'm> {
    pub(super) fn root(
        next_index: &'a mut usize,
        effect_ids: &'a mut EffectIdAllocator,
        composition_shutter: &'a mut Option<CompositionShutter>,
        linked: &'a mut LinkedCompositions<'r, 'm>,
    ) -> Self {
        Self {
            parent: None,
            first_index: 0,
            next_index,
            effect_ids,
            item_layers: ItemLayers::new(),
            composition_shutter,
            linked,
            on_document_clock: true,
        }
    }
}

/// Why a retimed nest's Motion keys are not converted, as a retimed clip's
/// are not: native keys count on the source clock, and where Premiere puts
/// them under a speed change is not pinned by an Adobe fixture.
const RETIMED_NEST_KEYS: &str = "keys on a retimed nested sequence occurrence are not converted";

/// The report of a nest whose group passes the blend modes of its layers
/// through: FX draws a group that neither blends nor is masked or matted
/// straight into its parent at the frames where it is at full Opacity and
/// renders no effect, where Premiere composites the nested sequence alone
/// before placing it.
const PASS_THROUGH_APPROXIMATION: &str = "the nested sequence composites alone in Premiere, but its FX group passes the blend modes of the layers inside it through to the layers below the group at the frames where its Opacity is full, so there they also blend with those where the nested sequence is not opaque (content-dependent error)";

/// The report of a group that may pass the blend modes of its layers
/// through: only an upper bound says that its Opacity gets to full
/// ([`OpacityReach::Possibly`]), or it has an effect that FX may render,
/// which isolates the group at the frames where it renders.
const POSSIBLE_PASS_THROUGH_APPROXIMATION: &str = "the nested sequence composites alone in Premiere, but its FX group passes the blend modes of the layers inside it through to the layers below the group at any frame, if there is one, where its Opacity is full and none of its effects isolates it; there they would also blend with those where the nested sequence is not opaque (content-dependent error)";

/// The pass-through report for `group`, if FX may draw it straight into its
/// parent at some time of its active range, as it draws the group of a
/// Normal, unmatted nest ([`PASS_THROUGH_APPROXIMATION`]). FX decides this
/// per frame: it draws a group offscreen at the frames where the group is
/// below full Opacity or one of its effects renders, and at every frame when
/// it blends, is masked or matted (`tree_renderer::walk`). Whether an effect
/// renders at a frame can depend on its values there (a blur of zero renders
/// nothing), which this does not decide: an effect that FX may render makes
/// the report conditional ([`POSSIBLE_PASS_THROUGH_APPROXIMATION`]). That is
/// a conservative bound, which also reports a group that its effect isolates
/// at every frame.
fn pass_through_report(group: &GroupLayer, dynamics: &AnimationGraph) -> Option<&'static str> {
    if group.blend_mode != BlendMode::Normal
        || !group.masks.is_empty()
        || group.track_matte.is_some()
    {
        return None;
    }
    match opacity_reach(group, dynamics) {
        OpacityReach::Below => None,
        OpacityReach::Full if !group.effects.iter().any(may_render_on_group) => {
            Some(PASS_THROUGH_APPROXIMATION)
        }
        OpacityReach::Full | OpacityReach::Possibly => Some(POSSIBLE_PASS_THROUGH_APPROXIMATION),
    }
}

/// Whether FX may render `effect` on a group, which isolates the group at
/// the frames where it renders. FX never renders a disabled effect, a person
/// or depth matte, which needs a media layer's own frame, Posterize Time,
/// which holds the group's clock instead, or an effect of a type that this
/// schema does not know, which it loads as unsupported.
fn may_render_on_group(effect: &EffectRecord) -> bool {
    let payload = match effect.data() {
        EffectData::Identified { enabled: false, .. } => return false,
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
    };
    !matches!(
        payload,
        EffectPayload::Known(
            LayerEffect::PersonMatte { .. }
                | LayerEffect::DepthMatte { .. }
                | LayerEffect::PosterizeTime { .. }
        ) | EffectPayload::Unknown(_)
    )
}

/// How far a group's Opacity gets over its active range.
enum OpacityReach {
    /// Below full at every time of the range.
    Below,
    /// Full at some time of the range.
    Full,
    /// Perhaps full: only a bound says so, which an ease may not reach.
    Possibly,
}

/// How far the Opacity of `group` gets over its active range `0..end` on the
/// group clock. Without an Opacity animation its static value decides. With
/// a keyframe track, FX's value at each time of the range is a key's value, a
/// held value (before the first key, after the last, over a Hold) or an
/// interpolation. The range meets values that Hold and Linear pieces take
/// exactly: a key in the range, a held value, and a Linear value where the
/// range starts. It approaches, without taking, the Linear value where the
/// range ends. A cubic Bézier ease may pass both of its keys: only the
/// largest value of its control points, whose convex hull holds the curve,
/// bounds it. So Opacity is full when a value that the range meets is 100;
/// possibly full when only an approached value or an ease's bound is; and
/// below otherwise. Any other Opacity animation is possibly full.
fn opacity_reach(group: &GroupLayer, dynamics: &AnimationGraph) -> OpacityReach {
    let Some(tracks) = layer_tracks(dynamics, group.id, |property| property == PropType::Opacity)
    else {
        return OpacityReach::Possibly;
    };
    let Some(track) = tracks.get(&PropType::Opacity) else {
        return if group.transform.opacity.value() >= 100.0 {
            OpacityReach::Full
        } else {
            OpacityReach::Below
        };
    };
    let mut keys = Vec::with_capacity(track.keyframes().len());
    for key in track.keyframes() {
        let PropertyValue::Float(value) = key.value() else {
            return OpacityReach::Possibly;
        };
        keys.push((key.layer_time().as_millis() as f64, *value, key.easing()));
    }
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return OpacityReach::Possibly;
    };
    let end = group.playback.input_range().duration.as_millis() as f64;
    // The largest value that the range meets, and the largest that it
    // approaches or that bounds an ease.
    let (mut met, mut bound) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    if first.0 > 0.0 {
        met = met.max(first.1);
    }
    if last.0 < end {
        met = met.max(last.1);
    }
    for &(time, value, _) in &keys {
        if (0.0..end).contains(&time) {
            met = met.max(value);
        }
    }
    for pair in keys.windows(2) {
        let ((from_time, from, _), (to_time, to, easing)) = (pair[0], pair[1]);
        if from_time >= end || to_time <= 0.0 {
            continue;
        }
        let at = |time: f64| from + (to - from) * (time - from_time) / (to_time - from_time);
        match easing {
            PropertyKeyframeEasing::Hold => met = met.max(from),
            PropertyKeyframeEasing::Linear => {
                met = met.max(at(from_time.max(0.0)));
                if to_time > end {
                    bound = bound.max(at(end));
                }
            }
            PropertyKeyframeEasing::CubicBezier { y1, y2, .. } => {
                bound = [y1, y2]
                    .into_iter()
                    .map(|y| from + (to - from) * y)
                    .fold(bound.max(from.max(to)), f64::max);
            }
        }
    }
    if met >= 100.0 {
        OpacityReach::Full
    } else if bound >= 100.0 {
        OpacityReach::Possibly
    } else {
        OpacityReach::Below
    }
}

/// Whether a picture of `sequence`, one of its own layers, blends with what
/// lies below it.
fn blends_inside(sequence: &PrSequence) -> bool {
    let blends = |mode: PrBlendMode| mode.fx_mode() != BlendMode::Normal;
    sequence.video_items().any(|item| match item {
        PrVideoItem::Media(clip) => blends(clip.blend_mode),
        PrVideoItem::Graphic(graphic) => blends(graphic.blend_mode),
    }) || sequence
        .nest_occurrences()
        .any(|nest| blends(nest.blend_mode))
}

/// Creates one group per nested placement on a track, below the layers
/// already created in `scope`, and records each group in the scope.
#[allow(clippy::too_many_arguments)]
pub(super) fn track_nests(
    project: &PrSequence,
    track_index: usize,
    scope: &mut LayerScope<'_, '_, '_>,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, AssetId>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
    progress: Option<(fx_conv::ProgressPhase<'_>, &std::cell::Cell<usize>)>,
) -> Result<Vec<Layer>> {
    let mut groups = Vec::new();
    for nest in &project.video_tracks[track_index].nests {
        let _processed = super::premiere_to_tesseract::Processed { progress };
        // A nest keeps default Motion, so its matte stays a sibling of the
        // group. An omitted nest takes no id.
        let track_matte = match nest.track_matte {
            None => None,
            Some(matte) => {
                match matte_layer(
                    &scope.item_layers,
                    matte,
                    nest.timeline_ticks(),
                    track_index,
                    &nest.record(),
                    omissions,
                ) {
                    Ok(matte) => Some(matte),
                    Err(reason) => {
                        omit(omissions, OmissionScope::Occurrence, nest.record(), reason);
                        continue;
                    }
                }
            }
        };
        let active_range = tick_range(nest.start_ticks, nest.end_ticks)?;
        validate_time_range("active_range", active_range)?;
        // The group maps its placement onto its children's clock: from zero
        // for a nest at normal speed, whose visible children move onto the
        // group clock, else the inner clock over the window from In to Out,
        // both ends rounded once, on which the children keep their places.
        let retimed = nest.is_retimed();
        let content_window = if retimed {
            tick_range(nest.in_ticks, nest.out_ticks)?
        } else {
            TimeRangeProperty::new(Time::ZERO, active_range.duration)
        };
        let playback = match LayerPlayback::linear(active_range, active_range, content_window, 0) {
            Ok(playback) => playback,
            Err(reason) => {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    nest.record(),
                    format!("nested sequence clock not converted: {reason}"),
                );
                continue;
            }
        };
        let group_id = LayerId::new(*scope.next_index as u64 + 1);
        *scope.next_index += 1;
        scope
            .item_layers
            .insert((track_index, nest.start_ticks), ItemLayer::Plain(group_id));
        let content = if retimed {
            inner_clock_content(nest)
        } else {
            visible_content(nest, omissions)?
        };
        // A retimed nest's keys are not converted, so only its static Motion
        // can move it.
        let keyed = !retimed && !nest.animations.is_empty();
        // A moved nest shows its sequence's frame only, so its group is
        // clipped to that frame, which also keeps blends inside it.
        let moved = nest.transform != PrStaticTransform::default() || keyed;
        let passes_blends_through = !moved
            && nest.blend_mode.fx_mode() == BlendMode::Normal
            && track_matte.is_none()
            && blends_inside(&content);
        let first_index = *scope.next_index;
        *scope.next_index += content.video_items().count();
        let mut layers = video_layers(
            &content,
            LayerScope {
                parent: Some(group_id),
                first_index,
                next_index: &mut *scope.next_index,
                effect_ids: &mut *scope.effect_ids,
                item_layers: ItemLayers::new(),
                composition_shutter: &mut *scope.composition_shutter,
                linked: &mut *scope.linked,
                on_document_clock: scope.on_document_clock
                    && !retimed
                    && active_range.start == Time::ZERO,
            },
            media,
            asset_ids,
            dynamics,
            omissions,
            None,
        )?;
        // The reader folded the gain of the nest's audio item into this sound.
        layers.extend(audio_layers(
            &content.audio,
            Some(group_id),
            scope.next_index,
            media,
            asset_ids,
            dynamics,
            omissions,
            None,
        )?);
        let canvas = [project.width, project.height];
        let mut transform = identity_transform();
        let mut masks = Vec::new();
        if moved {
            transform = clip_transform(&nest.transform, nest.opacity, canvas, canvas)?;
            // The form a nest's Crop exports from: an Add mask whose guide is
            // a child of the group at the identity, here the whole frame.
            let guide_id = LayerId::new(*scope.next_index as u64 + 1);
            let mask_id = FxItemId::new(*scope.next_index as u64 + 2);
            *scope.next_index += 2;
            layers.push(Layer::from_data(&LayerData::Rect(guide_layer(
                guide_id,
                "Nested sequence frame".to_owned(),
                Some(group_id),
                content_window,
                identity_transform(),
                black_shape(canvas[0], canvas[1]),
            )))?);
            masks.push(guide_mask(mask_id, guide_id, 0.0));
        }
        // The placement's Motion moves its canvas-sized picture, as a clip's
        // moves a canvas-sized video: the reader admits only a nest with the
        // outer canvas, so the canvas is also the source frame that Anchor
        // Point keys scale by. Its keys count from In on the source clock,
        // the group clock at normal speed.
        let record = nest.record();
        set_tracks(
            dynamics,
            motion_tracks(
                &nest.animations,
                nest.in_ticks,
                retimed.then_some(RETIMED_NEST_KEYS),
                group_id,
                None,
                canvas,
                canvas,
                &record,
                omissions,
            ),
        )?;
        groups.push(Layer::from_data(&LayerData::Group(GroupLayer {
            is_hidden: !nest.enabled,
            parent: scope.parent,
            blend_mode: nest.blend_mode.fx_mode(),
            track_matte,
            masks,
            playback,
            ..plain_group(
                group_id,
                nest.sequence.name.clone(),
                active_range,
                transform,
                layers,
            )?
        }))?);
        if let Some(warning) = nest.blend_mode.approximation() {
            approximate(omissions, nest.record(), warning);
        }
        if passes_blends_through {
            approximate(omissions, nest.record(), PASS_THROUGH_APPROXIMATION);
        }
    }
    Ok(groups)
}

/// The part of a nest's inner timeline that its placement shows, on the
/// group clock.
///
/// Inner time `t` maps to group time `t - in + start - origin`, where `origin`
/// is the placement start rounded to the millisecond, as the group start is.
/// Each moved boundary then rounds to the millisecond of its absolute outer
/// time, which is the top-level rule. A boundary less than half a millisecond
/// before the origin becomes zero, where it also rounds. Every kind of item
/// moves so; a still or graphic keeps its source clock with its in-point.
fn visible_content(nest: &PrNestOccurrence, omissions: &mut Vec<Omission>) -> Result<PrSequence> {
    let origin = ticks_from_time(time_from_ticks(nest.start_ticks)?, "nested sequence start")?;
    let shift = i128::from(nest.start_ticks) - i128::from(nest.in_ticks) - i128::from(origin);
    let window = nest.in_ticks..nest.out_ticks;
    let mut content = nest.sequence.clone();
    let mut audio = Vec::with_capacity(content.audio.len());
    for mut clip in std::mem::take(&mut content.audio) {
        if let Some((timeline, source)) = visible_part(
            clip.start_ticks..clip.end_ticks,
            clip.in_ticks,
            &window,
            shift,
        )? {
            (clip.start_ticks, clip.end_ticks) = (timeline.start, timeline.end);
            (clip.in_ticks, clip.out_ticks) = (source.start, source.end);
            audio.push(clip);
        }
    }
    content.audio = audio;
    for (track_index, track) in content.video_tracks.iter_mut().enumerate() {
        let mut items = Vec::with_capacity(track.items.len());
        for mut item in std::mem::take(&mut track.items) {
            let range = item.timeline_ticks();
            if range.start >= window.end || window.start >= range.end {
                continue;
            }
            let source_in = match &item {
                PrVideoItem::Media(clip) => clip.in_ticks,
                PrVideoItem::Graphic(graphic) => graphic.in_ticks,
            };
            if let PrVideoItem::Media(clip) = &item {
                if clip.time_remap.is_some() {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        format!(
                            "{}, inner video track {track_index}, {} ({}..{} ticks)",
                            nest.record(),
                            clip.record(),
                            range.start,
                            range.end
                        ),
                        "retimed inner clip not converted: trimming a source-time remapping inside a nest is not implemented",
                    );
                    continue;
                }
            }
            let Some((timeline, mut source)) =
                visible_part(range.clone(), source_in, &window, shift)?
            else {
                continue;
            };
            match &mut item {
                PrVideoItem::Media(clip) => {
                    if clip.playback_rate != 1.0 {
                        source = constant_source_part(
                            &range,
                            &(clip.in_ticks..clip.out_ticks),
                            &window,
                        )?;
                    }
                    (clip.start_ticks, clip.end_ticks) = (timeline.start, timeline.end);
                    (clip.in_ticks, clip.out_ticks) = (source.start, source.end);
                }
                PrVideoItem::Graphic(graphic) => {
                    (graphic.start_ticks, graphic.end_ticks) = (timeline.start, timeline.end);
                    graphic.in_ticks = source.start;
                }
            }
            items.push(item);
        }
        track.items = items;
        let mut nests = Vec::with_capacity(track.nests.len());
        for mut inner in std::mem::take(&mut track.nests) {
            let range = inner.timeline_ticks();
            if let Some((timeline, mut source)) =
                visible_part(range.clone(), inner.in_ticks, &window, shift)?
            {
                // A retimed inner nest keeps its rate over the part shown.
                if inner.is_retimed() {
                    source =
                        constant_source_part(&range, &(inner.in_ticks..inner.out_ticks), &window)?;
                }
                (inner.start_ticks, inner.end_ticks) = (timeline.start, timeline.end);
                (inner.in_ticks, inner.out_ticks) = (source.start, source.end);
                nests.push(inner);
            }
        }
        track.nests = nests;
    }
    Ok(content)
}

/// The part of a retimed nest's inner timeline that its placement shows, on
/// the inner clock onto which its group maps the placement: each inner
/// placement that shares time with the window from In to Out, unmoved and
/// untrimmed, since the group's window hides the rest. It has no sound: an
/// audio item plays its nest at normal speed, so the reader pairs none with
/// a retimed nest (`pair_sounds`).
fn inner_clock_content(nest: &PrNestOccurrence) -> PrSequence {
    let window = nest.in_ticks..nest.out_ticks;
    let mut content = nest.sequence.clone();
    for track in &mut content.video_tracks {
        track.items.retain(|item| {
            let range = item.timeline_ticks();
            range.start < window.end && window.start < range.end
        });
        track.nests.retain(|inner| inner.overlaps(&window));
    }
    content
}

/// Trim a constant-rate clip or retimed nest in its authored source span.
/// Reverse clips keep Premiere's backward source bounds; `clip_timing`
/// converts those to forward bounds. Integer interpolation preserves the full
/// endpoints and avoids changing the rate because the nest's trim was treated
/// as unit speed.
fn constant_source_part(
    timeline: &Range<i64>,
    source: &Range<i64>,
    window: &Range<i64>,
) -> Result<Range<i64>> {
    let duration = i128::from(timeline.end) - i128::from(timeline.start);
    let span = i128::from(source.end) - i128::from(source.start);
    ensure!(
        duration > 0 && span > 0,
        "invalid constant-rate nested clip span"
    );
    let boundary = |time: i64| {
        let offset = i128::from(time) - i128::from(timeline.start);
        i64::try_from(i128::from(source.start) + (offset * span + duration / 2) / duration)
            .map_err(|_| unsupported("nested source span exceeds Premiere's tick range"))
    };
    Ok(boundary(timeline.start.max(window.start))?..boundary(timeline.end.min(window.end))?)
}

/// The part of one placement inside `window`: its moved timeline range and
/// its source range, or `None` when the window does not show it.
fn visible_part(
    range: Range<i64>,
    source_in: i64,
    window: &Range<i64>,
    shift: i128,
) -> Result<Option<(Range<i64>, Range<i64>)>> {
    let start = range.start.max(window.start);
    let end = range.end.min(window.end);
    if start >= end {
        return Ok(None);
    }
    let ticks = |value: i128| {
        i64::try_from(value)
            .map_err(|_| unsupported("nested placement exceeds Premiere's tick range"))
    };
    let source_start = ticks(i128::from(source_in) + i128::from(start) - i128::from(range.start))?;
    let source_end = ticks(i128::from(source_start) + i128::from(end) - i128::from(start))?;
    let timeline = ticks((i128::from(start) + shift).max(0))?..ticks(i128::from(end) + shift)?;
    Ok(Some((timeline, source_start..source_end)))
}

/// Current document inputs shared by the root layer list and every nested one.
pub(super) struct LayerExport<'a, 'd> {
    pub(super) dynamics: &'d AnimationGraph,
    pub(super) property_tracks:
        &'a mut BTreeMap<LayerId, BTreeMap<PropType, &'d PropertyKeyframeTrack>>,
    /// The animated properties whose keys the placed native owners write.
    pub(super) written: &'a mut WrittenAnimation,
    pub(super) audio_facts: &'a BTreeMap<String, SourceSound>,
    pub(super) fonts: &'a BTreeMap<String, FontAssetProperties>,
    pub(super) audio: &'a mut Vec<PrAudioOccurrence>,
    pub(super) media_facts: &'a BTreeMap<String, MediaFacts>,
    pub(super) media: &'a mut BTreeMap<MediaId, PrMedia>,
    /// Actual source-boundary recorder shared by every nested lowering scope.
    pub(super) packer: &'a mut PicturePacker,
    pub(super) container: PictureContainerToken,
    pub(super) boundary: Option<SourceBoundaryToken>,
    pub(super) width: u32,
    pub(super) height: u32,
    /// Sequence rate of the export, shared by every nested sequence.
    pub(super) frame_rate: FrameRate,
    /// Document time of this list's time zero.
    pub(super) origin: Time,
    /// Groups that contain this list.
    pub(super) depth: usize,
    /// The root rectangle that exports as the black canvas; nested lists have
    /// none.
    pub(super) canvas: Option<LayerId>,
    /// The guide of the mask that the placement of this list's nest exports,
    /// which paints nothing; the root list has none.
    pub(super) group_guide: Option<LayerId>,
    /// Whether the placement of a nest that holds this list writes Motion,
    /// which a Directional Blur on a clip in the list does not follow.
    pub(super) in_moved_nest: bool,
    /// The composition's motion blur, which a Transform stage's video
    /// exports as its Transform's own shutter.
    pub(super) motion_blur: MotionBlurSettings,
    /// Whether a Transform stage wrote that shutter, so that the
    /// composition's motion blur is not reported as lost.
    pub(super) motion_blur_written: &'a mut bool,
}

impl LayerExport<'_, '_> {
    pub(super) fn boundary(&self) -> Result<SourceBoundaryToken> {
        self.boundary
            .ok_or_else(|| unsupported("picture placement has no source boundary"))
    }

    /// Snaps a list-local time as an absolute document time, then measures it
    /// from the snapped origin, so nested boundaries keep the top-level rule.
    pub(super) fn frame_ticks(&self, time: Time, context: &str) -> Result<i64> {
        let absolute = self
            .origin
            .checked_add_duration(Duration::from_millis(time.as_millis()))
            .ok_or_else(|| unsupported(format!("{context} exceeds Premiere's tick range")))?;
        Ok(frame_ticks_from_time(absolute, self.frame_rate, context)?
            - frame_ticks_from_time(self.origin, self.frame_rate, context)?)
    }
}

/// The video of the clip that `layer`, one of `layers`, exports as: a video
/// layer, or a stage group's video. `None` for any other layer and for a clip
/// that export omits whole, or exports as a nest, for its stage group or mask
/// ([`clip_omitted`]). `canvas` is the sequence size.
fn clip_video<'d>(
    layer: &'d Layer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> Option<&'d Layer> {
    // A stage group exports as one clip of its video.
    let (video, stage) = match layer.data() {
        LayerData::Group(group) => (stage_video(group)?, Some(group)),
        _ => (layer, None),
    };
    let omitted = match super::video_data(video) {
        Ok(Some(data)) => {
            let clip = match stage {
                Some(group) => ClipLayers::Stage {
                    group,
                    video: &data,
                },
                None => ClipLayers::Video(&data),
            };
            clip_omitted(clip, layers, dynamics, canvas)
        }
        Ok(None) => return None,
        // Its inspection rejects the export, as export itself would.
        Err(_) => false,
    };
    (!omitted).then_some(video)
}

/// The video that a stage group's track matte names, its direct child, which
/// exports as a clip beside the group's clip when that clip exports
/// ([`clip_video`]).
fn stage_matte_video<'d>(
    layer: &'d Layer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> Option<&'d Layer> {
    let LayerData::Group(group) = layer.data() else {
        return None;
    };
    let matte = group.track_matte.as_ref()?;
    clip_video(layer, layers, dynamics, canvas)?;
    group
        .layers
        .iter()
        .find(|child| child.id() == matte.layer && matches!(child.data(), LayerData::Video(_)))
}

/// The video of each clip that export writes from a layer list and from every
/// group inside it that exports as a nest ([`clip_video`]), each such group,
/// whose placement writes its effects, and each adjustment layer that exports
/// as an adjustment clip. Media of an omitted layer is never inspected, so it
/// cannot fail the export.
pub(crate) fn exported_video_layers<'d>(
    layers: &'d [Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> Vec<&'d Layer> {
    fn collect<'d>(
        layers: &'d [Layer],
        dynamics: &AnimationGraph,
        canvas: [u32; 2],
        depth: usize,
        videos: &mut Vec<&'d Layer>,
    ) {
        for layer in layers {
            match exported_nest(layer, layers, dynamics, depth, canvas) {
                Some(group) => {
                    videos.push(layer);
                    collect(&group.layers, dynamics, canvas, depth + 1, videos);
                }
                None => match layer.data() {
                    LayerData::Adjustment(adjustment)
                        if super::adjustment::unexported_reason(adjustment).is_none() =>
                    {
                        videos.push(layer);
                    }
                    _ => {
                        videos.extend(clip_video(layer, layers, dynamics, canvas));
                        videos.extend(stage_matte_video(layer, layers, dynamics, canvas));
                    }
                },
            }
        }
    }
    let mut videos = Vec::new();
    collect(layers, dynamics, canvas, 0, &mut videos);
    videos
}

/// The group that `layer`, one of `layers` at nesting `depth`, is when it
/// exports as a nest: a group that is neither a graphic (a group of text and
/// shape layers) nor a clip, and that [`unsupported_group`] accepts.
fn exported_nest<'d>(
    layer: &'d Layer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    depth: usize,
    canvas: [u32; 2],
) -> Option<&'d GroupLayer> {
    match layer.data() {
        LayerData::Group(group)
            if super::graphic::graphic_objects(group).is_none()
                && clip_video(layer, layers, dynamics, canvas).is_none()
                && nest_candidate(group)
                && unsupported_group(group, layers, dynamics, depth, canvas).is_none() =>
        {
            Some(group)
        }
        _ => None,
    }
}

/// Each layer list that export writes: `layers` and the layers of every group
/// in it that exports as a nest ([`exported_nest`]).
fn exported_lists<'d>(
    layers: &'d [Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> Vec<&'d [Layer]> {
    fn collect<'d>(
        layers: &'d [Layer],
        dynamics: &AnimationGraph,
        canvas: [u32; 2],
        depth: usize,
        lists: &mut Vec<&'d [Layer]>,
    ) {
        lists.push(layers);
        for layer in layers {
            if let Some(group) = exported_nest(layer, layers, dynamics, depth, canvas) {
                collect(&group.layers, dynamics, canvas, depth + 1, lists);
            }
        }
    }
    let mut lists = Vec::new();
    collect(layers, dynamics, canvas, 0, &mut lists);
    lists
}

/// The video of each clip of the top-level `layers` that export writes, by
/// the clip's layer id ([`clip_video`]): only these clips export embedded
/// sound. Groups that export as nests are not clips.
pub(crate) fn exported_clip_videos<'d>(
    layers: &'d [Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> BTreeMap<LayerId, &'d Layer> {
    layers
        .iter()
        .filter_map(|layer| Some((layer.id(), clip_video(layer, layers, dynamics, canvas)?)))
        .collect()
}

/// The still images that export writes, in `layers` or a nest inside them
/// ([`exported_lists`]): a still, or the track matte source under a stage
/// group, beside the group's clip, and never one with a mask
/// ([`unsupported_image_masks`]) or with a Scale or Rotation that its Motion
/// cannot show ([`unexportable_motion`]). Media of an omitted image is never
/// inspected, so it cannot fail the export.
pub(crate) fn exported_image_layers<'d>(
    layers: &'d [Layer],
    dynamics: &'d AnimationGraph,
    canvas: [u32; 2],
) -> impl Iterator<Item = &'d Layer> {
    let exported = move |image| {
        unsupported_image_masks(image).is_none()
            && unexportable_motion(MotionHost::image(image), dynamics).is_none()
    };
    exported_lists(layers, dynamics, canvas)
        .into_iter()
        .flatten()
        .flat_map(move |layer| match layer.data() {
            LayerData::Image(image) => exported(image).then_some(layer),
            LayerData::Group(group) => group.track_matte.as_ref().and_then(|matte| {
                stage_layers(group)?;
                group.layers.iter().find(|child| {
                    child.id() == matte.layer
                        && matches!(child.data(), LayerData::Image(image) if exported(image))
                })
            }),
            _ => None,
        })
}

/// The audio layers of `layers` and of the nests inside them
/// ([`exported_lists`]), whose sound export inspects; a group that then
/// exports no picture is omitted with its sound.
pub(crate) fn exported_audio_layers<'d>(
    layers: &'d [Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> impl Iterator<Item = &'d Layer> {
    exported_lists(layers, dynamics, canvas)
        .into_iter()
        .flatten()
        .filter(|layer| matches!(layer.data(), LayerData::Audio(_)))
}

/// Whether a group that exports as no clip exports as a nest, if
/// [`unsupported_group`] accepts it: every group but a stage group whose video
/// does not play over the whole group ([`stage_video_spans_group`]), which is
/// omitted instead.
pub(super) fn nest_candidate(group: &GroupLayer) -> bool {
    match stage_layers(group).map(super::video_data) {
        Some(Ok(Some(video))) => stage_video_spans_group(group, &video),
        _ => true,
    }
}

/// Exports one group, one of `layers`, as a nested sequence on the lowest
/// track of `video_tracks` above every overlapping lower placement, and
/// returns that track's index; or omits it when a nest cannot represent it
/// unchanged.
pub(super) fn export_group<'d>(
    group: &'d GroupLayer,
    layers: &[Layer],
    video_tracks: &mut Vec<PrVideoTrack>,
    context: &mut LayerExport<'_, 'd>,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<usize>> {
    let record = format!("layer {} ({:?})", group.id, group.name);
    let canvas = [context.width, context.height];
    if let Some(reason) = unsupported_group(group, layers, context.dynamics, context.depth, canvas)
    {
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            format!("group was not exported as a nested sequence: {reason}"),
        );
        return Ok(None);
    }
    if !group.description.is_empty() {
        omit_field(
            omissions,
            group.id,
            ExportField::Description,
            &record,
            "description was not exported",
        );
    }
    let window = group.playback.input_range();
    let active_end = window
        .start
        .checked_add_duration(window.duration)
        .ok_or_else(|| unsupported("group activeRange end exceeds Premiere's tick range"))?;
    let start_ticks = context.frame_ticks(window.start, "group playback.inputRange.start")?;
    let end_ticks = context.frame_ticks(active_end, "group activeRange.end")?;
    ensure!(
        end_ticks > start_ticks,
        "{record}: activeRange {}..{} ms collapses to zero duration on the {} sequence grid",
        window.start.as_millis(),
        active_end.as_millis(),
        context.frame_rate
    );
    let origin = context
        .origin
        .checked_add_duration(Duration::from_millis(window.start.as_millis()))
        .ok_or_else(|| unsupported("group start exceeds Premiere's tick range"))?;
    // The placement plays from inner time zero, so its keys count from the
    // group's start. Its Crop is a percentage of its picture, the canvas.
    let clip = ClipLayers::Nest(group);
    let mask = canonical_mask(clip, canvas, layers, context.dynamics, canvas[0], canvas[1])
        .map_err(unsupported)?;
    let (crop, linear_wipe) = export_mask(mask.as_ref(), context, 0, &record, omissions)?;
    // The placement's Motion, which the clips inside do not follow; it is
    // reported after them.
    let mut motion_reports = Vec::new();
    let mut transform = export_transform(
        clip.motion(),
        canvas,
        canvas,
        context.dynamics,
        &record,
        &mut motion_reports,
    );
    let mut tracks = context
        .property_tracks
        .get(&group.id)
        .cloned()
        .unwrap_or_default();
    let (animations, _) = export_motion_keys(
        &mut tracks,
        0,
        &mut transform,
        canvas,
        canvas,
        &record,
        &mut motion_reports,
    );
    // Motion that moves no pixel of the canvas-sized picture is Premiere's
    // default, which a plain nest writes.
    let motion_animated = animates_motion(&animations);
    if !motion_animated
        && transform.position == transform.anchor_point
        && transform.scale == [100.0; 2]
        && transform.rotation == 0.0
    {
        transform = PrStaticTransform::default();
    }
    let child_container = context.packer.begin_container(
        context.boundary()?,
        &group.name,
        [context.width, context.height],
        context.frame_rate,
        0,
    )?;
    // Restore standalone nested-sound media if the group has no picture.
    let media = group
        .layers
        .iter()
        .any(|layer| matches!(layer.data(), LayerData::Audio(_)))
        .then(|| context.media.clone());
    let mut audio = Vec::new();
    let mut child_written = WrittenAnimation::default();
    let mut child_motion_blur_written = false;
    let mut inner = LayerExport {
        dynamics: context.dynamics,
        property_tracks: &mut *context.property_tracks,
        written: &mut child_written,
        media_facts: context.media_facts,
        audio_facts: context.audio_facts,
        fonts: context.fonts,
        audio: &mut audio,
        media: &mut *context.media,
        packer: &mut *context.packer,
        container: child_container,
        boundary: None,
        width: context.width,
        height: context.height,
        frame_rate: context.frame_rate,
        origin,
        depth: context.depth + 1,
        canvas: None,
        group_guide: mask.as_ref().and_then(CanonicalMask::guide_id),
        in_moved_nest: context.in_moved_nest
            || motion_animated
            || transform != PrStaticTransform::default(),
        motion_blur: context.motion_blur,
        motion_blur_written: &mut child_motion_blur_written,
    };
    let mut inner_tracks = export_layers(&group.layers, Some(group.id), &mut inner, omissions)
        .map_err(|source| BuildError::Context {
            context: record.clone(),
            source: Box::new(source),
        })?;
    let empty = inner_tracks
        .iter()
        .all(|track| track.items.is_empty() && track.nests.is_empty());
    let inner_end = inner_tracks
        .iter()
        .flat_map(|track| {
            track
                .items
                .iter()
                .map(PrVideoItem::timeline_ticks)
                .map(|range| range.end)
                .chain(track.nests.iter().map(|nest| nest.end_ticks))
        })
        .chain(audio.iter().map(|sound| sound.end_ticks))
        .max()
        .unwrap_or(0);
    // A pending (picture-empty) container still owns the group's local clock.
    // Retained native sequences keep their original occurrence-derived end.
    let captured_end = if empty {
        end_ticks - start_ticks
    } else {
        inner_end
    };
    context
        .packer
        .finish_container(child_container, &mut inner_tracks, captured_end)?;
    if !empty && end_ticks - start_ticks > inner_end {
        // Do not manufacture a too-long native placement: it would fail the
        // whole project before a caller can replace this picture scope.
        // Unlike picture, nested sound has no independent replacement path.
        ensure!(
            audio.is_empty()
                && !inner_tracks
                    .iter()
                    .flat_map(|track| &track.nests)
                    .any(|nest| nest.sequence.has_sound()),
            "{record}: group extends past its exported children's end and contains nested audio; cannot omit its native sound"
        );
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            "group extends past its exported children's end; no supported native nested sequence was exported",
        );
        return Ok(None);
    }
    if empty {
        // Sound-only nests would render opaque black in Premiere.
        if !audio.is_empty() {
            if let Some(media) = media {
                *context.media = media;
            }
            omit(omissions, OmissionScope::Occurrence, &record,
                "a group with sound but no picture is not exported: Premiere draws a nested sequence without video as opaque black");
            return Ok(None);
        }
        // Retain an exact outer header only when its own lowering has no loss.
        // It remains a pending object, not fabricated native picture content.
        let mut pending_written = WrittenAnimation::default();
        let mut pending_omissions = Vec::new();
        let pending_effects = export_effects(
            &group.effects,
            context.dynamics,
            EffectHost {
                layer: group.id,
                staged: false,
                nested: true,
                transform: &group.transform,
                source_in: 0,
                frame: canvas,
                canvas,
            },
            &mut pending_written,
            &record,
            &mut pending_omissions,
        );
        if motion_reports.is_empty() && pending_omissions.is_empty() {
            let pending = PrNestOccurrence {
                id: None,
                start_ticks,
                end_ticks,
                in_ticks: 0,
                out_ticks: end_ticks - start_ticks,
                transform,
                opacity: group.transform.opacity.value(),
                blend_mode: PrBlendMode::from_fx_mode(group.blend_mode),
                animations,
                crop,
                linear_wipe,
                track_matte: unplaced_track_matte(mask.as_ref()),
                effects: pending_effects,
                enabled: !group.is_hidden,
                sequence: PrSequence {
                    id: None,
                    name: group.name.clone(),
                    top_level: Some(false),
                    video_tracks: inner_tracks,
                    audio,
                    frame_rate: context.frame_rate,
                    width: context.width,
                    height: context.height,
                    timeline_end_ticks: captured_end,
                },
            };
            context.packer.record_pending_nest(
                context.container,
                context.boundary()?,
                child_container,
                pending,
            )?;
        }
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            "group with no exportable video was not exported",
        );
        return Ok(None);
    }
    // A child's keys only reach native output if its containing nest survives.
    context.written.append(&mut child_written);
    *context.motion_blur_written |= child_motion_blur_written;
    context.property_tracks.remove(&group.id);
    for report in motion_reports {
        omissions.emit(report);
    }
    let mut nest = PrNestOccurrence {
        id: None,
        start_ticks,
        end_ticks,
        in_ticks: 0,
        out_ticks: end_ticks - start_ticks,
        transform,
        opacity: group.transform.opacity.value(),
        blend_mode: PrBlendMode::from_fx_mode(group.blend_mode),
        animations,
        crop,
        linear_wipe,
        track_matte: unplaced_track_matte(mask.as_ref()),
        effects: export_effects(
            &group.effects,
            context.dynamics,
            EffectHost {
                layer: group.id,
                staged: false,
                nested: true,
                transform: &group.transform,
                source_in: 0,
                frame: canvas,
                canvas,
            },
            context.written,
            &record,
            omissions,
        ),
        enabled: !group.is_hidden,
        sequence: PrSequence {
            id: None,
            name: group.name.clone(),
            top_level: Some(false),
            video_tracks: inner_tracks,
            audio,
            frame_rate: context.frame_rate,
            width: context.width,
            height: context.height,
            timeline_end_ticks: 0,
        },
    };
    // The inner sequence ends with its last placement, so a group longer than
    // its content still has no exact nest.
    nest.sequence.timeline_end_ticks = nest.sequence.occurrence_end_ticks();
    let pass_through =
        pass_through_report(group, context.dynamics).filter(|_| blends_inside(&nest.sequence));
    context
        .written
        .record_placement(group.id, &nest.animations, mask, nest.linear_wipe.as_ref());
    let range = nest.timeline_ticks();
    let index = video_tracks
        .iter()
        .rposition(|track| track.overlaps(&range))
        .map_or(0, |index| index + 1);
    if index == video_tracks.len() {
        video_tracks.push(PrVideoTrack {
            transitions: Vec::new(),
            items: Vec::new(),
            nests: Vec::new(),
        });
    }
    video_tracks[index].nests.push(nest);
    context.packer.record_nest(
        context.container,
        context.boundary()?,
        index,
        child_container,
    )?;
    if let Some(warning) = PrBlendMode::export_approximation(group.blend_mode) {
        approximate(omissions, &record, warning);
    }
    if let Some(report) = pass_through {
        approximate(omissions, &record, report);
    }
    Ok(Some(index))
}

/// Why a nest cannot represent `group`, one of `layers`, unchanged, if it
/// cannot. `depth` counts the groups that contain `group`, and `canvas` is
/// the sequence size.
fn unsupported_group(
    group: &GroupLayer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    depth: usize,
    canvas: [u32; 2],
) -> Option<String> {
    unsupported_group_fields(group, dynamics)
        .or_else(|| unexportable_clip(ClipLayers::Nest(group), dynamics).map(str::to_owned))
        .or_else(|| unsupported_group_animation(group, dynamics, canvas))
        .or_else(|| {
            let [width, height] = canvas;
            canonical_mask(
                ClipLayers::Nest(group),
                canvas,
                layers,
                dynamics,
                width,
                height,
            )
            .err()
        })
        .or_else(|| unsupported_nest_depth(depth))
}

/// Why a nest cannot represent `group` for a field of its own, if it cannot:
/// the part of [`unsupported_group`] that no track's keys can change, which
/// script preparation also reads. Only the presence of background animation
/// matters, not its kind.
pub(super) fn unsupported_group_fields(
    group: &GroupLayer,
    dynamics: &AnimationGraph,
) -> Option<String> {
    let background = has_background(group)
        || layer_animations(dynamics, group.id)
            .any(|(property, _)| is_background_property(property));
    [
        (
            group.name.is_empty() || group.name.chars().count() > 255,
            "the nested sequence name must have 1 to 255 characters",
        ),
        (background, "group backgrounds are not supported"),
        (
            !super::timing::is_plain_group_playback(&group.playback),
            "group time remapping is not supported",
        ),
        (group.motion_blur, "group motion blur is not supported"),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then(|| reason.to_owned()))
}

/// Why a group inside `depth` groups has no nest, if it has none.
pub(super) fn unsupported_nest_depth(depth: usize) -> Option<String> {
    (depth >= MAX_NEST_DEPTH)
        .then(|| format!("nesting deeper than {MAX_NEST_DEPTH} levels is not supported"))
}

/// Why the placement cannot carry the animation of `group`'s own properties,
/// if it cannot: each must be one enabled keyframe track without dependencies
/// ([`layer_tracks`]) that the placement's key export writes whole
/// ([`export_motion_keys`] from the Motion of [`export_transform`]). A dropped
/// key could show what the group's animation hides.
fn unsupported_group_animation(
    group: &GroupLayer,
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> Option<String> {
    let Some(mut tracks) = layer_tracks(dynamics, group.id, |_| true) else {
        return Some(
            "group animation that is not one enabled keyframe track without dependencies is not supported"
                .to_owned(),
        );
    };
    // Only a dropped track matters here; the export reports the rest.
    let reports = &mut Vec::new();
    let motion = ClipLayers::Nest(group).motion();
    let mut transform = export_transform(motion, canvas, canvas, dynamics, "", reports);
    let (_, dropped) =
        export_motion_keys(&mut tracks, 0, &mut transform, canvas, canvas, "", reports);
    dropped.or_else(|| {
        let property = tracks.into_keys().next()?;
        Some(format!("group {property:?} animation is not supported"))
    })
}

#[cfg(test)]
#[path = "tests/nested.rs"]
mod tests;
