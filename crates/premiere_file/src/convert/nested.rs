//! Nested sequences as groups of inline copies, in both directions.
//!
//! Import: each nested placement becomes one `GroupLayer` over the placement's
//! range. Its children are an independent copy of the part of the inner
//! timeline that the placement shows, on the group's clock, so the group moves
//! and trims as one unit. Inner gaps stay transparent because the inner black
//! canvas is not copied. Every copy refers to the shared media assets. The
//! placement's Track Matte Key becomes the group's track matte. The children
//! of a retimed placement, and of one whose sequence has another frame rate,
//! keep the inner clock instead, untrimmed, and its group's playback
//! maps the placement onto the inner window; a retimed nest's Motion keys
//! stay unconverted, as a retimed clip's do. Bounded unit reverse uses a
//! separate picture Group so occurrence Motion/Opacity clocks remain forward
//! while descendants retain their authored clocks under the decreasing remap.
//! The placement's Motion places
//! its picture, the inner canvas, in the outer canvas, as a clip's places its
//! media frame, and its Opacity and Opacity keys fade the group whether or
//! not it moves.
//!
//! Export: a group becomes a new inner sequence of its children, placed from
//! inner time zero over the group's range. The placement takes the group's
//! Motion, Opacity, their keys, its blend mode, its one Crop, Linear Wipe or
//! track matte, its effects and its Enable, as a clip takes its layer's. A
//! group retains current children with local field/clock approximations when
//! native controls cannot reproduce them. Inseparable unsupported coverage
//! remains omitted. Unmapped Group motion blur is reported separately.
//! A group is also omitted if it exports no
//! picture, and the smallest group around a layer that export would place
//! but whose range the sequence grid collapses to no frame, which no
//! placement can hold.

use super::{
    background::{black_shape, identity_transform, plain_group},
    effects::{export_effects, import_nest_effects, EffectHost, EffectIdAllocator},
    packing::{PictureContainerToken, PicturePacker, SourceBoundaryToken},
    premiere_to_tesseract::{
        audio_layers, clip_transform, crop_rect, curved_transform_position_approximation,
        guide_layer, guide_mask, map_animation_graph_error, matte_layer, motion_tracks,
        opacity_mask, scalar_keys, set_tracks, shape_guide, staged_video_transform, tick_range,
        transform_stage_tracks, validate_time_range, video_layers, CompositionShutter,
    },
    tesseract_to_premiere::{
        animates_motion, canonical_mask, clip_omitted, consumed_layer_ids, export_layers,
        export_mask, export_motion_keys, export_transform, has_background, image_mask,
        is_background_property, layer_animations, layer_tracks, mask_guide_ids, stage_layers,
        stage_video, stage_video_spans_group, track_matte_sources, unplaced_track_matte,
        CanonicalMask, ClipLayers, MotionHost, WrittenAnimation,
    },
    timing::{
        duration_from_ticks, frame_ticks_from_time, linear_source_range, ticks_from_time,
        time_from_ticks,
    },
};
use crate::{
    audio_media::SourceSound,
    error::{ensure, unsupported, BuildError, Result},
    export_loss::{omit_field, ExportField, OmissionSink},
    format::{FrameRate, MediaId, PrMedia, PrSequence, PrVideoItem},
    linked_compositions::LinkedCompositions,
    media::MediaFacts,
    schema::{
        PrAnimatedProperty, PrAudioFade, PrAudioOccurrence, PrBlendMode, PrKeyframeEasing,
        PrNestOccurrence, PrStaticTransform, PrTimeRemap, PrVideoTrack, MAX_NEST_DEPTH,
    },
    {approximate, omit, Omission, OmissionKind, OmissionScope},
};
use fx_schema::{
    animator::{PropertyAnimator, PropertyKeyframeEasing, PropertyKeyframeTrack},
    AnimationGraph, AssetId, BlendMode, Duration, EffectData, EffectPayload, EffectRecord,
    FontAssetProperties, FxItemId, GroupLayer, ImageSource, Layer, LayerData, LayerEffect, LayerId,
    LayerPlayback, MotionBlurSettings, PropType, PropertyValue, Time, TimeRangeProperty,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

/// Display label only; current structure, clocks and source guide select
/// and validate a nested Transform stage independently of this spelling.
const NEST_TRANSFORM_STAGE: &str = "Premiere nested Transform ";
const NEST_TRANSFORM_APPROXIMATION: &str = "nested Transform is approximated as editable source-frame affine before Motion with source-canvas clipping; Geometry2 order, clipping and transparent pixels may differ; export uses ordinary Transform";
const NESTED_MATTE_CONTROLS_APPROXIMATION: &str = "nested matte Motion/Opacity is retained on its editable Group with source-canvas clipping where needed; native group sampling and edge/alpha fidelity are unmeasured";

/// One native-render-order stage, innermost first. Affine stages use existing
/// Group transforms; pixel effects use the unchanged nested-host effect mapping.
struct NestEffectStage<'a> {
    id: LayerId,
    parent: LayerId,
    transform: fx_schema::Transform,
    tracks: Vec<(fx_schema::Property, PropertyKeyframeTrack)>,
    effects: &'a [crate::schema::PrEffect],
    warnings: Vec<String>,
    affine: bool,
    coverage: Option<NestGeometry2Coverage>,
}

/// Stationary output coverage above one translating Geometry2 picture. The
/// guides use the nested source plane, not the parent's Motion canvas.
struct NestGeometry2Coverage {
    id: LayerId,
    parent: LayerId,
    masks: Vec<fx_schema::PathMask>,
    guides: Vec<Layer>,
    tracks: Vec<(fx_schema::PropertyTarget, PropertyKeyframeTrack)>,
    reports: Vec<Omission>,
}

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
pub(super) struct LayerScope<'a, 'm> {
    pub(super) picture_clocks: &'a crate::media::PictureClocks,
    pub(super) parent: Option<LayerId>,
    /// Enclosing groups, including generated stages, for bounded native export.
    pub(super) nesting_depth: usize,
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
    pub(super) linked: &'a mut LinkedCompositions<'m>,
    /// Whether this scope's clock is the document clock: no enclosing nest
    /// group starts after the document start.
    pub(super) on_document_clock: bool,
    /// The document's canvas in pixels, in whose frame FX draws an adjustment
    /// layer's effects at any nesting depth; the same in every scope of one
    /// document.
    pub(super) document_canvas: [u32; 2],
}

impl<'a, 'm> LayerScope<'a, 'm> {
    pub(super) fn root(
        document_canvas: [u32; 2],
        next_index: &'a mut usize,
        effect_ids: &'a mut EffectIdAllocator,
        composition_shutter: &'a mut Option<CompositionShutter>,
        picture_clocks: &'a crate::media::PictureClocks,
        linked: &'a mut LinkedCompositions<'m>,
    ) -> Self {
        Self {
            picture_clocks,
            parent: None,
            nesting_depth: 0,
            first_index: 0,
            next_index,
            effect_ids,
            item_layers: ItemLayers::new(),
            composition_shutter,
            linked,
            on_document_clock: true,
            document_canvas,
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
/// at every frame. Import asks this of the group that it builds for a nest
/// and export of the group that it writes, so a round trip repeats the report.
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

/// How far the Opacity of `group` gets over its active range, which its
/// playback shows as `start..end` on the clock of its own keys, its content
/// clock (from zero on a plain clock). Without an Opacity animation its
/// static value decides. With a keyframe track, FX's value at each time of
/// the range is a key's value, a held value (before the first key, after the
/// last, over a Hold) or an interpolation. The range meets values that Hold
/// and Linear pieces take exactly: a key in the range, a held value, and a
/// Linear value where the range starts. It approaches, without taking, the
/// Linear value where the range ends. A cubic Bézier ease may pass both of
/// its keys: only the largest value of its control points, whose convex hull
/// holds the curve, bounds it. So Opacity is full when a value that the range
/// meets is 100; possibly full when only an approached value or an ease's
/// bound is; and below otherwise. Any other Opacity animation is possibly
/// full.
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
    let Ok(window) = linear_source_range(&group.playback) else {
        return OpacityReach::Possibly;
    };
    let start = window.start.as_millis() as f64;
    let end = window.end().as_millis() as f64;
    // The largest value that the range meets, and the largest that it
    // approaches or that bounds an ease.
    let (mut met, mut bound) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    if first.0 > start {
        met = met.max(first.1);
    }
    if last.0 < end {
        met = met.max(last.1);
    }
    for &(time, value, _) in &keys {
        if (start..end).contains(&time) {
            met = met.max(value);
        }
    }
    for pair in keys.windows(2) {
        let ((from_time, from, _), (to_time, to, easing)) = (pair[0], pair[1]);
        if from_time >= end || to_time <= start {
            continue;
        }
        let at = |time: f64| from + (to - from) * (time - from_time) / (to_time - from_time);
        match easing {
            PropertyKeyframeEasing::Hold => met = met.max(from),
            PropertyKeyframeEasing::Linear => {
                met = met.max(at(from_time.max(start)));
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
        PrVideoItem::Capsule(capsule) => blends(capsule.placement.blend_mode),
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
    scope: &mut LayerScope<'_, '_>,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, AssetId>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
    progress: Option<(fx_conv::ProgressPhase<'_>, &std::cell::Cell<usize>)>,
) -> Result<Vec<Layer>> {
    let mut groups = Vec::new();
    for nest in &project.video_tracks[track_index].nests {
        let _processed = super::premiere_to_tesseract::Processed { progress };
        if nest.playback_rate < 0.0 {
            let group = reverse_nest(project, nest, scope, media, asset_ids, dynamics, omissions)?;
            scope.item_layers.insert(
                (track_index, nest.start_ticks),
                ItemLayer::Plain(group.id()),
            );
            groups.push(group);
            continue;
        }
        // A keyed nest keeps default Motion (the reader omits one that moves),
        // so its matte stays a sibling of the group. An omitted nest takes no
        // id.
        let track_matte = match nest.track_matte {
            None => None,
            Some(matte) => {
                match matte_layer(
                    &scope.item_layers,
                    &project.video_tracks,
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
        // for a nest whose visible children move onto the group clock, else
        // the inner clock from In, on which the children keep their places:
        // over the window from In to Out, both ends rounded once, for a
        // retimed nest, and at unit rate for one at normal speed whose
        // sequence has another frame rate.
        let retimed = nest.is_retimed();
        let inner_clock = nest.plays_inner_clock(project.native_frame_rate());
        if nest.linear_wipe.is_some()
            && (inner_clock || nest.sequence.dimensions() != project.dimensions())
        {
            omit(
                omissions,
                OmissionScope::Occurrence,
                nest.record(),
                "nested Linear Wipe requires equal canvases and a unit-forward matching clock",
            );
            continue;
        }
        let content_window = if nest.time_remap.is_some() {
            // The frame guide is on the source clock, not the curve's input
            // In/Out. Cover the child domain without sampling the curve.
            tick_range(0, nest.sequence.end_ticks())?
        } else if retimed {
            tick_range(nest.in_ticks, nest.out_ticks)?
        } else if inner_clock {
            TimeRangeProperty::new(time_from_ticks(nest.in_ticks)?, active_range.duration)
        } else {
            TimeRangeProperty::new(Time::ZERO, active_range.duration)
        };
        let playback = if let Some(remap) = &nest.time_remap {
            let mapped = super::premiere_to_tesseract::mapped_time_remap(
                remap,
                nest.playback_rate,
                nest.in_ticks != 0 || nest.playback_rate != 1.0,
                nest.sequence.end_ticks(),
                active_range,
                nest.start_ticks,
            );
            mapped
                .map_err(|error| error.to_string())
                .and_then(|(property, offset)| {
                    LayerPlayback::remapped(active_range, property, offset)
                        .map_err(|error| error.to_string())
                })
        } else {
            LayerPlayback::linear(active_range, active_range, content_window, 0)
                .map_err(|error| error.to_string())
        };
        let playback = match playback {
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
        let key_origin = if inner_clock { 0 } else { nest.in_ticks };
        // Opacity keys that do not convert, as two native keys within one
        // millisecond do not, would leave the group at its static Opacity
        // where they fade it, so they omit the nest before it takes an id;
        // Motion keys that do not convert keep their static values.
        let opacity_error = nest
            .animations
            .iter()
            .filter(|animation| animation.property() == PrAnimatedProperty::Opacity)
            .find_map(|animation| {
                scalar_keys(animation, key_origin, group_id, PropType::Opacity).err()
            });
        if let Some(error) = opacity_error {
            omit(
                omissions,
                OmissionScope::Occurrence,
                nest.record(),
                format!(
                    "nested sequence Opacity keys not converted: {error}; its group would hold its static Opacity where they fade it"
                ),
            );
            continue;
        }
        let transform_stage = nest
            .effects
            .iter()
            .any(|effect| matches!(effect.params, crate::schema::PrEffectParams::Transform(_)));
        let stage = if nest.effects.is_empty() && nest.opacity_mask.is_none() {
            None
        } else {
            let prepare = || -> Result<_> {
                let stage_id = LayerId::new(*scope.next_index as u64 + 2);
                if !transform_stage {
                    return Ok(vec![NestEffectStage {
                        id: stage_id,
                        parent: group_id,
                        transform: identity_transform(),
                        tracks: Vec::new(),
                        effects: &nest.effects,
                        warnings: Vec::new(),
                        affine: false,
                        coverage: None,
                    }]);
                }
                if inner_clock
                    || !nest.crop.is_default()
                    || nest.linear_wipe.is_some()
                    || nest.opacity_mask.is_some()
                    || nest.track_matte.is_some()
                {
                    return Err(unsupported(
                        "nested Transform requires unit-forward matching clocks and no masks",
                    ));
                }
                let mut reports = Vec::new();
                let _ = motion_tracks(
                    &nest.animations,
                    key_origin,
                    None,
                    group_id,
                    None,
                    nest.sequence.dimensions(),
                    project.dimensions(),
                    &nest.record(),
                    &mut reports,
                );
                if !reports.is_empty() {
                    return Err(unsupported(
                        "nested Transform requires every intrinsic Motion/Opacity key to convert",
                    ));
                }
                let count = nest.effects.len();
                let mut next = *scope.next_index as u64 + 2;
                let mut parent = group_id;
                let mut layout = Vec::with_capacity(count);
                for position in (0..count).rev() {
                    let coverage = nest
                        .geometry2_masks
                        .get(&position)
                        .filter(|masks| !masks.is_empty())
                        .map(|_| {
                            let id = LayerId::new(next);
                            next += 1;
                            (id, parent)
                        });
                    let id = LayerId::new(next);
                    next += 1;
                    layout.push((id, coverage.map_or(parent, |(id, _)| id), coverage));
                    parent = id;
                }
                layout.reverse();
                let mut stages = Vec::with_capacity(count);
                for ((position, effect), (id, parent, coverage_owner)) in
                    nest.effects.iter().enumerate().zip(layout)
                {
                    let (transform, tracks, effects, warnings, affine) =
                        if let crate::schema::PrEffectParams::Transform(transform) = &effect.params
                        {
                            if coverage_owner.is_none() {
                                if let Some(reason) =
                                    crate::schema::nested_transform_import_canvas_reason(
                                        nest.sequence.dimensions(),
                                        project.dimensions(),
                                        &nest.transform,
                                        nest.opacity,
                                        &nest.animations,
                                        effect,
                                    )
                                {
                                    return Err(unsupported(reason));
                                }
                            }
                            let mut warnings = transform.approximations(&effect.animations);
                            if let Some(warning) = curved_transform_position_approximation(effect) {
                                warnings.push(format!(
                                    "{warning}; retained nested effect {}",
                                    position + 1
                                ));
                            }
                            (
                                staged_video_transform(transform, nest.sequence.dimensions())?,
                                transform_stage_tracks(
                                    effect,
                                    transform,
                                    nest.sequence.dimensions(),
                                    key_origin,
                                    id,
                                )?,
                                &[][..],
                                warnings,
                                true,
                            )
                        } else {
                            (
                                identity_transform(),
                                Vec::new(),
                                std::slice::from_ref(effect),
                                Vec::new(),
                                false,
                            )
                        };
                    let coverage = if let Some((coverage_id, coverage_parent)) = coverage_owner {
                        let mut masks = Vec::new();
                        let mut guides = Vec::new();
                        let mut mask_tracks = Vec::new();
                        let mut reports = Vec::new();
                        for mask in &nest.geometry2_masks[&position] {
                            let guide_id = LayerId::new(next);
                            let mask_id = FxItemId::new(next + 1);
                            next += 2;
                            let (path_mask, path) = opacity_mask(
                                mask,
                                mask_id,
                                guide_id,
                                nest.sequence.dimensions(),
                                &nest.record(),
                                &mut reports,
                            )?;
                            masks.push(path_mask);
                            mask_tracks.extend(
                                super::mask_animation::import_tracks(mask, mask_id, key_origin)
                                    .map_err(unsupported)?,
                            );
                            guides.push(Layer::from_data(&LayerData::Shape(shape_guide(
                                guide_id,
                                "Nested Geometry2 effect mask".into(),
                                Some(coverage_id),
                                content_window,
                                identity_transform(),
                                path,
                            )))?);
                        }
                        Some(NestGeometry2Coverage {
                            id: coverage_id,
                            parent: coverage_parent,
                            masks,
                            guides,
                            tracks: mask_tracks,
                            reports,
                        })
                    } else {
                        None
                    };
                    stages.push(NestEffectStage {
                        id,
                        parent,
                        transform,
                        tracks,
                        effects,
                        warnings,
                        affine,
                        coverage,
                    });
                }
                Ok(stages)
            };
            match prepare() {
                Ok(stage) => Some(stage),
                Err(error) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        nest.record(),
                        error.to_string(),
                    );
                    continue;
                }
            }
        };
        let stage_ids = stage.as_ref().map_or(0, |stages| {
            stages
                .iter()
                .map(|stage| {
                    1 + stage
                        .coverage
                        .as_ref()
                        .map_or(0, |coverage| 1 + 2 * coverage.masks.len())
                })
                .sum::<usize>()
        });
        // Outer guides follow every effect stage, output-coverage owner and
        // owned guide/mask pair, rather than assuming one picture-stage ID.
        let mut next_guide_id = *scope.next_index as u64 + 2 + stage_ids as u64;
        // Prepare before importing children or reserving identities. Native
        // tick keys may collide on the FX millisecond clock; keep siblings.
        let wipe_guide = match &nest.linear_wipe {
            Some(wipe) => {
                let guide_number = next_guide_id;
                let guide_id = LayerId::new(guide_number);
                match super::premiere_to_tesseract::linear_wipe_guide(
                    wipe,
                    key_origin,
                    guide_id,
                    nest.sequence.dimensions(),
                ) {
                    Ok(guide) => {
                        next_guide_id += 2;
                        Some((
                            guide_id,
                            FxItemId::new(guide_number + 1),
                            wipe.feather,
                            guide,
                        ))
                    }
                    Err(error) => {
                        omit(
                            omissions,
                            OmissionScope::Occurrence,
                            nest.record(),
                            format!("nested Linear Wipe not converted: {error}"),
                        );
                        continue;
                    }
                }
            }
            None => None,
        };
        // Opacity coverage belongs above the picture/effects stage, not on
        // that stage's input. Prepare controls before reserving IDs or children.
        let opacity_guide = if let Some(mask) = &nest.opacity_mask {
            let prepare = || -> Result<_> {
                ensure!(
                    !inner_clock && nest.sequence.dimensions() == project.dimensions()
                        && mask.path_keys.is_empty() && nest.crop.is_default()
                        && nest.linear_wipe.is_none() && nest.track_matte.is_none(),
                    "nested Opacity mask requires equal canvases, a static outline and a unit-forward matching clock without other masks"
                );
                let guide_id = LayerId::new(next_guide_id);
                let mask_id = FxItemId::new(next_guide_id + 1);
                let mut reports = Vec::new();
                let (path_mask, path) = opacity_mask(
                    mask,
                    mask_id,
                    guide_id,
                    nest.sequence.dimensions(),
                    &nest.record(),
                    &mut reports,
                )?;
                let tracks = super::mask_animation::import_tracks(mask, mask_id, key_origin)
                    .map_err(unsupported)?;
                Ok((guide_id, path_mask, path, tracks, reports))
            };
            match prepare() {
                Ok(guide) => Some(guide),
                Err(error) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        nest.record(),
                        format!("nested Opacity mask not converted: {error}"),
                    );
                    continue;
                }
            }
        } else {
            None
        };
        let matte_source = project.video_tracks[..track_index].iter().any(|track| {
            track
                .items
                .iter()
                .filter_map(PrVideoItem::media)
                .map(|clip| (clip.track_matte, clip.timeline_ticks()))
                .chain(
                    track
                        .nests
                        .iter()
                        .map(|consumer| (consumer.track_matte, consumer.timeline_ticks())),
                )
                .any(|(matte, range)| {
                    matte.is_some_and(|matte| matte.track_index == track_index)
                        && range == nest.timeline_ticks()
                })
        });
        let mut motion_reports = Vec::new();
        let imported_motion = motion_tracks(
            &nest.animations,
            key_origin,
            retimed.then_some(RETIMED_NEST_KEYS),
            group_id,
            None,
            nest.sequence.dimensions(),
            project.dimensions(),
            &nest.record(),
            &mut motion_reports,
        );
        // Holding failed matte keys at their static value can expose picture
        // outside the saved coverage. Ordinary picture Motion keeps its fallback.
        let failed_matte_motion = matte_source && !motion_reports.is_empty();
        omissions.extend(motion_reports);
        if failed_matte_motion {
            omit(
                omissions,
                OmissionScope::Occurrence,
                nest.record(),
                "matte Motion keys could not be retained; nested matte omitted to preserve coverage",
            );
            continue;
        }
        let content_parent = stage.as_ref().map_or(group_id, |stages| stages[0].id);
        *scope.next_index += 1
            + stage_ids
            + 2 * usize::from(wipe_guide.is_some())
            + 2 * usize::from(opacity_guide.is_some());
        scope
            .item_layers
            .insert((track_index, nest.start_ticks), ItemLayer::Plain(group_id));
        let content = if inner_clock {
            inner_clock_content(nest)
        } else {
            visible_content(nest, omissions)?
        };
        // A retimed nest's keys are not converted, so only its static Motion
        // can move it. Opacity keys fade it without moving it.
        let keyed = !retimed
            && nest
                .animations
                .iter()
                .any(|animation| animation.property() != PrAnimatedProperty::Opacity);
        // A moved nest shows its sequence's frame only, so its group is
        // clipped to that frame, which also keeps blends inside it. Default
        // Motion moves a frame of another size than the outer canvas too: it
        // centres it there.
        let (frame, canvas) = (nest.sequence.dimensions(), project.dimensions());
        let moved = nest.transform != PrStaticTransform::default() || keyed || frame != canvas;
        let first_index = *scope.next_index;
        *scope.next_index += content.video_items().count();
        let mut layers = video_layers(
            &content,
            LayerScope {
                picture_clocks: scope.picture_clocks,
                parent: Some(content_parent),
                nesting_depth: scope.nesting_depth + 1 + usize::from(stage.is_some()),
                first_index,
                next_index: &mut *scope.next_index,
                effect_ids: &mut *scope.effect_ids,
                item_layers: ItemLayers::new(),
                composition_shutter: &mut *scope.composition_shutter,
                linked: &mut *scope.linked,
                on_document_clock: scope.on_document_clock
                    && !inner_clock
                    && active_range.start == Time::ZERO,
                document_canvas: scope.document_canvas,
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
            Some(content_parent),
            scope.next_index,
            scope.effect_ids,
            scope.linked,
            media,
            asset_ids,
            dynamics,
            omissions,
            None,
        )?);
        // The placement's Opacity fades the group whether or not its Motion
        // moves it.
        let motion = clip_transform(&nest.transform, nest.opacity, frame, canvas)?;
        let mut transform = identity_transform();
        transform.opacity = motion.opacity;
        let mut masks = Vec::new();
        // Crop remains above the retained effect stage. Otherwise a spatial
        // effect could spread pixels back into the concealed crop region.
        let effects_before_coverage = (nest.linear_wipe.is_some() && nest.effects_above_mask != 0)
            || (!nest.effects.is_empty() && !nest.crop.is_default());
        let mut outer_guide = None;
        if moved || stage.is_some() || !nest.crop.is_default() || nest.linear_wipe.is_some() {
            transform = motion;
            // Crop clips the nest's own frame before Motion. The guide stays
            // editable, including when export normalizes the inner canvas.
            let (guide_id, mask_id, guide_transform, rect, feather) = match wipe_guide {
                Some((guide_id, mask_id, feather, (transform, property, track))) => {
                    set_tracks(
                        dynamics,
                        vec![(fx_schema::Property::new(guide_id, property), track)],
                    )?;
                    (
                        guide_id,
                        mask_id,
                        transform,
                        black_shape(frame[0], frame[1]),
                        feather,
                    )
                }
                None => {
                    let guide_id = LayerId::new(*scope.next_index as u64 + 1);
                    let mask_id = FxItemId::new(*scope.next_index as u64 + 2);
                    *scope.next_index += 2;
                    (
                        guide_id,
                        mask_id,
                        identity_transform(),
                        crop_rect(&nest.crop, frame),
                        0.0,
                    )
                }
            };
            let guide = Layer::from_data(&LayerData::Rect(guide_layer(
                guide_id,
                "Nested sequence frame".to_owned(),
                Some(if effects_before_coverage {
                    group_id
                } else {
                    content_parent
                }),
                content_window,
                guide_transform,
                rect,
            )))?;
            if effects_before_coverage {
                outer_guide = Some(guide);
            } else {
                layers.push(guide);
            }
            masks.push(guide_mask(mask_id, guide_id, feather));
        }
        // The placement's Motion moves its picture, the inner canvas, as a
        // clip's moves its media frame: Anchor Point and its keys scale by
        // the inner canvas, Position by the outer one. Its Motion and Opacity
        // keys count on the source clock, which a group clock starts at In
        // and the inner clock at inner zero.
        let record = nest.record();
        set_tracks(dynamics, imported_motion)?;
        if matte_source
            && (nest.transform != PrStaticTransform::default() || !nest.animations.is_empty())
        {
            approximate(omissions, &record, NESTED_MATTE_CONTROLS_APPROXIMATION);
        }
        if let Some(stages) = stage {
            let stacked_affine = transform_stage && stages.len() > 1;
            for (position, stage) in stages.into_iter().enumerate() {
                set_tracks(dynamics, stage.tracks)?;
                let (effects, effect_tracks) = super::effects::import_nest_effect_slice(
                    nest,
                    stage.effects,
                    stage.id,
                    key_origin,
                    scope.effect_ids,
                    omissions,
                );
                for (target, track) in effect_tracks {
                    dynamics
                        .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
                        .map_err(map_animation_graph_error)?;
                }
                let inner = GroupLayer {
                    parent: Some(stage.parent),
                    masks: if position == 0 && !effects_before_coverage {
                        std::mem::take(&mut masks)
                    } else {
                        Vec::new()
                    },
                    effects,
                    // This stage does not rebase the inner clock already selected
                    // by the outer Group; children, guides and keys retain it.
                    playback: LayerPlayback::linear(
                        content_window,
                        content_window,
                        content_window,
                        0,
                    )
                    .map_err(unsupported)?,
                    ..plain_group(
                        stage.id,
                        if stage.affine {
                            "Nested Transform picture"
                        } else {
                            "Nested sequence effects"
                        }
                        .into(),
                        content_window,
                        stage.transform,
                        layers,
                    )?
                };
                layers = vec![Layer::from_data(&LayerData::Group(inner))?];
                if let Some(coverage) = stage.coverage {
                    for (target, track) in coverage.tracks {
                        dynamics
                            .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
                            .map_err(map_animation_graph_error)?;
                    }
                    omissions.extend(coverage.reports);
                    layers.extend(coverage.guides);
                    let covered = GroupLayer {
                        parent: Some(coverage.parent),
                        masks: coverage.masks,
                        playback: LayerPlayback::linear(
                            content_window,
                            content_window,
                            content_window,
                            0,
                        )
                        .map_err(unsupported)?,
                        ..plain_group(
                            coverage.id,
                            "Nested Geometry2 output coverage".into(),
                            content_window,
                            identity_transform(),
                            layers,
                        )?
                    };
                    layers = vec![Layer::from_data(&LayerData::Group(covered))?];
                }
                if stage.affine && !stacked_affine {
                    approximate(omissions, nest.record(), NEST_TRANSFORM_APPROXIMATION);
                }
                for warning in stage.warnings {
                    approximate(omissions, nest.record(), warning);
                }
            }
            if stacked_affine {
                approximate(omissions, nest.record(), "ordered nested affine effects retain separate editable stages; intermediate raster bounds, clipping and native edge/alpha sampling may differ; stacked export is unestablished");
            }
        }
        if let Some(guide) = outer_guide {
            layers.push(guide);
        }
        if let Some((guide_id, mask, path, tracks, reports)) = opacity_guide {
            for (target, track) in tracks {
                dynamics
                    .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
                    .map_err(map_animation_graph_error)?;
            }
            omissions.extend(reports);
            layers.push(Layer::from_data(&LayerData::Shape(shape_guide(
                guide_id,
                "Nested Opacity mask".into(),
                Some(group_id),
                content_window,
                identity_transform(),
                path,
            )))?);
            masks.push(mask);
        }
        let group = GroupLayer {
            is_hidden: !nest.enabled,
            parent: scope.parent,
            blend_mode: nest.blend_mode.fx_mode(),
            track_matte,
            masks,
            playback,
            ..plain_group(
                group_id,
                if transform_stage {
                    format!("{NEST_TRANSFORM_STAGE}{}", nest.sequence.name)
                } else {
                    nest.sequence.name.clone()
                },
                active_range,
                transform,
                layers,
            )?
        };
        // The pass-through of the group as FX draws it, at its Opacity and
        // keys, which export reports of the same group.
        let pass_through =
            pass_through_report(&group, dynamics).filter(|_| blends_inside(&content));
        groups.push(Layer::from_data(&LayerData::Group(group))?);
        if let Some(warning) = nest.blend_mode.approximation() {
            approximate(omissions, nest.record(), warning);
        }
        if let Some(report) = pass_through {
            approximate(omissions, nest.record(), report);
        }
    }
    Ok(groups)
}

/// Keep occurrence keys on the increasing Clip clock; only the picture's
/// descendants use the decreasing reflected sequence clock. A single remapped
/// owner would incorrectly reverse its own Motion and Opacity along with them.
fn reverse_nest(
    project: &PrSequence,
    nest: &PrNestOccurrence,
    scope: &mut LayerScope<'_, '_>,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, AssetId>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Layer> {
    nest.validate(project.native_frame_rate(), media)?;
    ensure!(
        !nest.sequence.has_sound(),
        "reverse nested picture must not carry unpaired sound"
    );
    let active = tick_range(nest.start_ticks, nest.end_ticks)?;
    let local = TimeRangeProperty::new(Time::ZERO, active.duration);
    let window = nest.reverse_source_window()?;
    let source = tick_range(window.start, window.end)?;
    let canvas_window = tick_range(0, nest.sequence.end_ticks())?;
    let picture_playback = LayerPlayback::remapped(
        local,
        super::premiere_to_tesseract::constant_time_remap(true, local, source)?,
        0,
    )
    .map_err(unsupported)?;
    let owner_playback = LayerPlayback::linear(active, active, local, 0).map_err(unsupported)?;
    let owner_id = LayerId::new(*scope.next_index as u64 + 1);
    let effect_stage_id =
        (!nest.effects.is_empty()).then(|| LayerId::new(*scope.next_index as u64 + 2));
    let effect_stage_count = usize::from(effect_stage_id.is_some());
    let picture_id = LayerId::new((*scope.next_index + effect_stage_count) as u64 + 2);
    let guide_id = LayerId::new((*scope.next_index + effect_stage_count) as u64 + 3);
    let mask_id = FxItemId::new((*scope.next_index + effect_stage_count) as u64 + 4);
    let (frame, canvas) = (nest.sequence.dimensions(), project.dimensions());
    let mut reports = Vec::new();
    let tracks = motion_tracks(
        &nest.animations,
        nest.in_ticks,
        None,
        owner_id,
        None,
        frame,
        canvas,
        &nest.record(),
        &mut reports,
    );
    for mut report in reports {
        report
            .reason
            .push_str("; reverse occurrence static value and other parameters were kept");
        omissions.push(report);
    }
    // The reader silences this picture independently of the native sound items.
    // Descendant placements and animation stay untrimmed on their authored clock.
    *scope.next_index += 4 + effect_stage_count;
    let first_index = *scope.next_index;
    *scope.next_index += nest.sequence.video_items().count();
    let mut layers = video_layers(
        &nest.sequence,
        LayerScope {
            picture_clocks: scope.picture_clocks,
            parent: Some(picture_id),
            nesting_depth: scope.nesting_depth + 2 + effect_stage_count,
            first_index,
            next_index: &mut *scope.next_index,
            effect_ids: &mut *scope.effect_ids,
            item_layers: ItemLayers::new(),
            composition_shutter: &mut *scope.composition_shutter,
            linked: &mut *scope.linked,
            on_document_clock: false,
            document_canvas: scope.document_canvas,
        },
        media,
        asset_ids,
        dynamics,
        omissions,
        None,
    )?;
    layers.push(Layer::from_data(&LayerData::Rect(guide_layer(
        guide_id,
        "Nested sequence frame".into(),
        Some(picture_id),
        canvas_window,
        identity_transform(),
        crop_rect(&nest.crop, frame),
    )))?);
    let picture = GroupLayer {
        parent: Some(effect_stage_id.unwrap_or(owner_id)),
        playback: picture_playback,
        masks: vec![guide_mask(mask_id, guide_id, 0.0)],
        ..plain_group(
            picture_id,
            "Nested reverse picture".into(),
            local,
            identity_transform(),
            layers,
        )?
    };
    set_tracks(dynamics, tracks)?;
    let mut content = Layer::from_data(&LayerData::Group(picture))?;
    if let Some(stage_id) = effect_stage_id {
        let (effects, effect_tracks) =
            import_nest_effects(nest, stage_id, nest.in_ticks, scope.effect_ids, omissions);
        for (target, track) in effect_tracks {
            dynamics
                .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
                .map_err(map_animation_graph_error)?;
        }
        let stage = GroupLayer {
            parent: Some(owner_id),
            effects,
            ..plain_group(
                stage_id,
                "Nested sequence effects".into(),
                local,
                identity_transform(),
                vec![content],
            )?
        };
        content = Layer::from_data(&LayerData::Group(stage))?;
    }
    let owner = GroupLayer {
        parent: scope.parent,
        is_hidden: !nest.enabled,
        playback: owner_playback,
        blend_mode: nest.blend_mode.fx_mode(),
        ..plain_group(
            owner_id,
            nest.sequence.name.clone(),
            active,
            clip_transform(&nest.transform, nest.opacity, frame, canvas)?,
            vec![content],
        )?
    };
    if let Some(warning) = nest.blend_mode.approximation() {
        approximate(omissions, nest.record(), warning);
    }
    if let Some(warning) =
        pass_through_report(&owner, dynamics).filter(|_| blends_inside(&nest.sequence))
    {
        approximate(omissions, nest.record(), warning);
    }
    Ok(Layer::from_data(&LayerData::Group(owner))?)
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
    if origin != nest.start_ticks {
        approximate(
            omissions,
            nest.record(),
            format!(
                "nested placement origin {} ticks is normalized to {origin} ticks on the editable millisecond clock; child tick bounds retain the residual phase without snapping to sequence frames",
                nest.start_ticks
            ),
        );
    }
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
            let source = if clip.playback_rate == 1.0 {
                source
            } else {
                clip.source_part(
                    &(clip.start_ticks.max(window.start)..clip.end_ticks.min(window.end)),
                )?
            };
            for fade in clip.play_part(timeline, source)? {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    fade.id.as_deref().unwrap_or(clip.record()),
                    PrAudioFade::PARTLY_PLAYED,
                );
            }
            audio.push(clip);
        }
    }
    content.audio = audio;
    for track in &mut content.video_tracks {
        let mut items = Vec::with_capacity(track.items.len());
        for mut item in std::mem::take(&mut track.items) {
            let range = item.timeline_ticks();
            if range.start >= window.end || window.start >= range.end {
                continue;
            }
            let source_in = match &item {
                PrVideoItem::Media(clip) => clip.in_ticks,
                PrVideoItem::Graphic(graphic) => graphic.in_ticks,
                PrVideoItem::Capsule(capsule) => capsule.placement.in_ticks,
            };
            if let PrVideoItem::Media(clip) = &mut item {
                if clip.held_source_ticks().is_some() {
                    for (index, effect) in clip.effects.iter_mut().enumerate() {
                        if !effect.animations.is_empty()
                            && !matches!(
                                effect.params,
                                crate::schema::PrEffectParams::AlphaGlow { .. }
                                    | crate::schema::PrEffectParams::Transform(_)
                                    | crate::schema::PrEffectParams::PosterizeTime { .. }
                            )
                        {
                            effect.animations.clear();
                            approximate(omissions, clip.id.as_deref().unwrap_or("inner held picture"),
                                format!("Frame Hold effect {} keys omitted at stack position {}; authored base effect, picture and independent children retained", effect.spec().display_name, index + 1));
                        }
                    }
                }
            }
            let Some((timeline, mut source)) =
                visible_part(range.clone(), source_in, &window, shift)?
            else {
                continue;
            };
            match &mut item {
                PrVideoItem::Media(clip) => {
                    if let Some(held) = clip.held_source_ticks() {
                        // A measured explicit hold has no moving source clock.
                        // Keep its In and held instant; only shorten the input
                        // span and rebuild its canonical unit-speed hold keys.
                        let duration = timeline.end - timeline.start;
                        let out = clip.in_ticks.checked_add(duration).ok_or_else(|| {
                            unsupported(
                                "nested Frame Hold input span exceeds Premiere's tick range",
                            )
                        })?;
                        source = clip.in_ticks..out;
                        clip.time_remap =
                            Some(crate::schema::PrTimeRemap::frame_hold(held, duration));
                    } else if clip.playback_rate != 1.0 {
                        source = constant_source_part(
                            &range,
                            &(clip.in_ticks..clip.out_ticks),
                            &window,
                        )?;
                    }
                    if clip.held_source_ticks().is_none() {
                        if let Some(remap) = &mut clip.time_remap {
                            let delta =
                                source.start.checked_sub(clip.in_ticks).ok_or_else(|| {
                                    unsupported("nested remap trim exceeds input clock")
                                })?;
                            for key in &mut remap.keys {
                                key.timeline_ticks =
                                    key.timeline_ticks.checked_sub(delta).ok_or_else(|| {
                                        unsupported("nested remap key exceeds input clock")
                                    })?;
                            }
                            // Keep source samples/keys exactly; only the elapsed
                            // input origin moves with the visible trim.
                        }
                    }
                    (clip.start_ticks, clip.end_ticks) = (timeline.start, timeline.end);
                    (clip.in_ticks, clip.out_ticks) = (source.start, source.end);
                }
                PrVideoItem::Graphic(graphic) => {
                    (graphic.start_ticks, graphic.end_ticks) = (timeline.start, timeline.end);
                    graphic.in_ticks = source.start;
                }
                PrVideoItem::Capsule(capsule) => {
                    source = constant_source_part(
                        &range,
                        &(capsule.placement.in_ticks..capsule.source_out_ticks),
                        &window,
                    )?;
                    (capsule.placement.start_ticks, capsule.placement.end_ticks) =
                        (timeline.start, timeline.end);
                    capsule.placement.in_ticks = source.start;
                    capsule.source_out_ticks = source.end;
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

/// The part of a nest's inner timeline that its placement shows, on the
/// inner clock onto which its group maps the placement
/// ([`PrNestOccurrence::plays_inner_clock`]): each inner placement and sound
/// that shares time with the window from In to Out, unmoved and untrimmed,
/// since the group's window hides the rest. A retimed nest has no sound: an
/// audio item plays its nest at normal speed, so the reader pairs none with
/// it (`pair_sounds`).
fn inner_clock_content(nest: &PrNestOccurrence) -> PrSequence {
    // A nonlinear input window is not a source window. Keep the full authored
    // child timeline; Group playback and each child's half-open bounds gate it.
    if nest.time_remap.is_some() {
        return nest.sequence.clone();
    }
    let window = nest.in_ticks..nest.out_ticks;
    let shows = |range: Range<i64>| range.start < window.end && window.start < range.end;
    let mut content = nest.sequence.clone();
    content
        .audio
        .retain(|clip| shows(clip.start_ticks..clip.end_ticks));
    for track in &mut content.video_tracks {
        track.items.retain(|item| shows(item.timeline_ticks()));
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
    /// Longest independently authored duration for each shared video asset.
    pub(super) authored_video_durations: &'a BTreeMap<String, u64>,
    /// Preset video frames that were natural before source-frame preparation.
    pub(super) natural_frames: &'a BTreeSet<LayerId>,
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
    /// Absolute FX boundary inherited by the picture inside a sampled-end nest.
    /// Only children ending at that boundary inherit its half-open membership.
    pub(super) sampled_picture_end: Option<Time>,
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
    /// Product of inherited nest scale lower bounds for compound-edge diagnostics.
    pub(super) nest_scale: Option<f64>,
    /// The composition's motion blur, which a Transform stage's video
    /// exports as its Transform's own shutter.
    pub(super) motion_blur: MotionBlurSettings,
    /// Whether a Transform stage wrote that shutter, so that the
    /// composition's motion blur is not reported as lost.
    pub(super) motion_blur_written: &'a mut bool,
    /// Commit these field losses only if every enclosing native nest survives.
    pub(super) unmapped_group_motion_blur: &'a mut Vec<&'d GroupLayer>,
}

impl LayerExport<'_, '_> {
    /// Select the native half-open picture end without changing its authored FX boundary.
    pub(super) fn picture_end_ticks(&self, end: Time, mask: Option<&CanonicalMask>) -> Result<i64> {
        if self.samples_picture_end(end, mask)? {
            super::timed_images::sampled_end_ticks(self, end)
        } else {
            self.frame_ticks(end, "activeRange.end")
        }
    }

    fn samples_picture_end(&self, end: Time, mask: Option<&CanonicalMask>) -> Result<bool> {
        Ok(mask.is_some_and(CanonicalMask::samples_end)
            || inherits_sampled_end(self.origin, end, self.sampled_picture_end)?)
    }

    pub(super) fn boundary(&self) -> Result<SourceBoundaryToken> {
        self.boundary
            .ok_or_else(|| unsupported("picture placement has no source boundary"))
    }

    /// Snaps a list-local time as an absolute document time, then measures it
    /// from the snapped origin, so nested boundaries keep the top-level rule.
    pub(super) fn frame_ticks(&self, time: Time, context: &str) -> Result<i64> {
        grid_ticks(self.origin, self.frame_rate, time, context)
    }
}

fn absolute_picture_end(origin: Time, end: Time) -> Result<Time> {
    origin
        .checked_add_duration(Duration::from_millis(end.as_millis()))
        .ok_or_else(|| unsupported("picture end exceeds Premiere's tick range"))
}

fn inherits_sampled_end(origin: Time, end: Time, inherited: Option<Time>) -> Result<bool> {
    Ok(inherited == Some(absolute_picture_end(origin, end)?))
}

/// [`LayerExport::frame_ticks`] of `time` on a list whose time zero is document
/// time `origin`, on the `frame_rate` grid.
fn grid_ticks(origin: Time, frame_rate: FrameRate, time: Time, context: &str) -> Result<i64> {
    let absolute = origin
        .checked_add_duration(Duration::from_millis(time.as_millis()))
        .ok_or_else(|| unsupported(format!("{context} exceeds Premiere's tick range")))?;
    Ok(frame_ticks_from_time(absolute, frame_rate, context)?
        - frame_ticks_from_time(origin, frame_rate, context)?)
}

/// Why the nest of `group` cannot place one of its layers, if it cannot: on
/// the frame grid of `context`, in a sequence whose time zero is document time
/// `origin`, the layer's range snaps to no frame ([`grid_ticks`]), which
/// export rejects for a picture placement. Export omits this nest, the
/// smallest around the layer, before writing any of it. Only a layer that
/// export would place as a timed picture in the nest's sequence counts, as
/// its writer decides before it snaps the range; one that the writer omits
/// first keeps its own reason. Counted are a clip ([`clip_video`]); a group
/// that export writes as a nest ([`exported_nest`]), whose own layers count
/// when it exports, but not a stage group that export omits instead
/// ([`stage_exports_as_nest`]); a still that its masks, Motion, frame and
/// fit let its writer place ([`image_mask`],
/// [`still_facts`](super::still::still_facts)); an
/// adjustment clip; and a Color Matte
/// ([`exported_matte`](super::color_matte::exported_matte)). A track matte
/// source exports only above a clip or nest that it keys and over exactly
/// that consumer's range (`canonical_track_matte`), so the consumer counts in
/// its place. A sound keeps its milliseconds, and a graphic whose range
/// collapses is omitted alone. Every layer is checked, so an error that one
/// raises, such as a still's conflicting media facts, rejects the export
/// whichever layer collapses. `canvas` is the nest's source frame, which can
/// differ from the containing sequence size in `context`.
// Children ending at the inherited sampled boundary use its membership rule,
// exactly as their eventual picture writers do; all other ends round normally.
fn collapsed_child(
    group: &GroupLayer,
    origin: Time,
    sampled_picture_end: Option<Time>,
    context: &LayerExport<'_, '_>,
    canvas: [u32; 2],
) -> Result<Option<String>> {
    let layers = group.layers.as_slice();
    let dynamics = context.dynamics;
    // Mask guides paint nothing; the group's own guide is its child.
    let guides: BTreeSet<_> = mask_guide_ids(layers)
        .into_iter()
        .chain(group.masks.iter().filter_map(|mask| mask.layer))
        .collect();
    let consumed = consumed_layer_ids(layers, false);
    let matte_sources = track_matte_sources(layers);
    let collapse = |layer: &Layer| -> Result<Option<String>> {
        let placed = match layer.data() {
            _ if matte_sources.contains(&layer.id()) => false,
            LayerData::Audio(_) => false,
            LayerData::Adjustment(adjustment) => {
                super::adjustment::unexported_reason(adjustment, layers, dynamics, canvas).is_none()
            }
            LayerData::Image(image) => {
                let ImageSource::Asset(source) = &image.source;
                image_mask(image, layers, dynamics, canvas).is_ok()
                    && super::still::still_facts(source, context.media_facts)?.is_ok()
            }
            LayerData::Rect(rect) => {
                !guides.contains(&rect.id)
                    && super::color_matte::exported_matte(
                        rect,
                        Some(group.id),
                        &consumed,
                        context,
                        canvas,
                    )
                    .is_some()
            }
            LayerData::Group(child) => {
                clip_video(layer, layers, dynamics, canvas).is_some()
                    || (super::adjustment_geometry::is_stage(child)
                        || stage_video(child, dynamics).is_none()
                        || stage_exports_as_nest(child))
                        && exported_nest(layer, layers, dynamics, context.depth + 1, canvas)
                            .is_some()
            }
            _ => clip_video(layer, layers, dynamics, canvas).is_some(),
        };
        if !placed {
            return Ok(None);
        }
        let range = layer.active_range();
        let end = range
            .start
            .checked_add_duration(range.duration)
            .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
        let ticks = |time, field| grid_ticks(origin, context.frame_rate, time, field);
        let end_ticks = if inherits_sampled_end(origin, end, sampled_picture_end)? {
            super::timed_images::sampled_end_ticks_at(origin, context.frame_rate, end)?
        } else {
            ticks(end, "activeRange.end")?
        };
        let collapsed = end_ticks <= ticks(range.start, "activeRange.start")?;
        Ok(collapsed.then(|| {
            format!(
                "its layer {} ({:?}) activeRange {}..{} ms collapses to zero duration on the {} sequence grid",
                layer.id(),
                layer.name(),
                range.start.as_millis(),
                end.as_millis(),
                context.frame_rate
            )
        }))
    };
    let mut collapsed = None;
    for layer in layers {
        // An error names its layer, as the layer's writer would.
        let reason = collapse(layer).map_err(|source| BuildError::Context {
            context: format!("layer {} ({:?})", layer.id(), layer.name()),
            source: Box::new(source),
        })?;
        collapsed = collapsed.or(reason);
    }
    Ok(collapsed)
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
    if let LayerData::Group(group) = layer.data() {
        // Geometry2 stages always use a nest, including the Opacity fallback.
        if super::adjustment_geometry::is_stage(group) {
            return None;
        }
        match super::effect_mask::EffectScope::recognize(group, dynamics, canvas) {
            Ok(Some(scope)) => {
                return (!clip_omitted(ClipLayers::EffectScope(scope), layers, dynamics, canvas))
                    .then_some(scope.video())
            }
            Err(_) => return None,
            Ok(None) => {}
        }
    }
    // A stage group exports as one clip of its video.
    let (video, stage) = match layer.data() {
        LayerData::Group(group) => (stage_video(group, dynamics)?, Some(group)),
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

/// The direct-child provider named by a stage group's track matte, which
/// exports beside the group's clip when that clip exports
/// ([`clip_video`]).
fn stage_matte_source<'d>(
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
    group.layers.iter().find(|child| child.id() == matte.layer)
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
                    let stage = is_nested_transform_stage(group)
                        .then(|| nested_transform_stage(group, dynamics, canvas).ok())
                        .flatten();
                    let (content, frame) = stage
                        .as_ref()
                        .map_or((group, canvas), |stage| (stage.picture, stage.frame));
                    collect(&content.layers, dynamics, frame, depth + 1, videos);
                }
                None => match layer.data() {
                    LayerData::Adjustment(adjustment)
                        if super::adjustment::unexported_reason(
                            adjustment, layers, dynamics, canvas,
                        )
                        .is_none() =>
                    {
                        videos.push(layer);
                    }
                    _ => {
                        videos.extend(clip_video(layer, layers, dynamics, canvas));
                        videos.extend(
                            stage_matte_source(layer, layers, dynamics, canvas)
                                .filter(|source| matches!(source.data(), LayerData::Video(_))),
                        );
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
/// shape layers) nor a clip, and that [`unsupported_group`] accepts. Media
/// inspection reads the media of every such group, also of a stage group
/// that export omits whole instead ([`stage_exports_as_nest`]).
fn exported_nest<'d>(
    layer: &'d Layer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    depth: usize,
    canvas: [u32; 2],
) -> Option<&'d GroupLayer> {
    match layer.data() {
        // Mirror export's structural dispatch before the one-video shape
        // heuristic. The shared admission validates guide, controls and depth.
        LayerData::Group(group) if super::adjustment_geometry::is_stage(group) => {
            unsupported_group(group, layers, dynamics, depth, canvas)
                .is_none()
                .then_some(group)
        }
        LayerData::Group(group)
            if matches!(
                super::effect_mask::EffectScope::recognize(group, dynamics, canvas),
                Ok(None)
            ) && (is_nested_transform_stage(group)
                || super::graphic::graphic_objects(group, dynamics).is_none())
                && clip_video(layer, layers, dynamics, canvas).is_none()
                && nest_candidate(group)
                && unsupported_group(group, layers, dynamics, depth, canvas).is_none() =>
        {
            Some(group)
        }
        _ => None,
    }
}

/// Each exported layer list paired with its native source canvas: `layers`
/// and the content of every nest ([`exported_nest`]).
fn exported_lists<'d>(
    layers: &'d [Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> Vec<(&'d [Layer], [u32; 2])> {
    fn collect<'d>(
        layers: &'d [Layer],
        dynamics: &AnimationGraph,
        canvas: [u32; 2],
        depth: usize,
        lists: &mut Vec<(&'d [Layer], [u32; 2])>,
    ) {
        lists.push((layers, canvas));
        for layer in layers {
            if let Some(group) = exported_nest(layer, layers, dynamics, depth, canvas) {
                let stage = is_nested_transform_stage(group)
                    .then(|| nested_transform_stage(group, dynamics, canvas).ok())
                    .flatten();
                let (content, frame) = stage
                    .as_ref()
                    .map_or((group, canvas), |stage| (stage.picture, stage.frame));
                collect(&content.layers, dynamics, frame, depth + 1, lists);
            } else if let Some(source) = stage_matte_source(layer, layers, dynamics, canvas) {
                if let LayerData::Group(provider) = source.data() {
                    // Admission proved the bounded timed-image provider. Inspect
                    // precisely the same stills that its linked nest exports.
                    lists.push((&provider.layers, canvas));
                }
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
/// ([`exported_lists`]): a still whose mask, if any, is one Crop or Opacity
/// mask beside it in its list ([`image_mask`]), or the track matte source
/// under a stage group, beside the group's clip, which has no mask; never one
/// with an inseparable unsupported mask. Motion fields recover locally.
/// Media of an omitted image is never inspected, so
/// it cannot fail the export.
pub(crate) fn exported_image_layers<'d>(
    layers: &'d [Layer],
    dynamics: &'d AnimationGraph,
    canvas: [u32; 2],
) -> impl Iterator<Item = &'d Layer> {
    exported_lists(layers, dynamics, canvas)
        .into_iter()
        .flat_map(move |(list, canvas)| {
            list.iter().filter_map(move |layer| match layer.data() {
                LayerData::Image(image) => {
                    (image_mask(image, list, dynamics, canvas).is_ok()).then_some(layer)
                }
                LayerData::Group(group) => group.track_matte.as_ref().and_then(|matte| {
                    stage_layers(group)?;
                    group.layers.iter().find(|child| {
                        child.id() == matte.layer
                            && matches!(child.data(), LayerData::Image(image)
                                            if image.masks.is_empty()
                                                && image.track_matte.is_none()
                            )
                    })
                }),
                _ => None,
            })
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
    fn collect<'d>(
        layers: &'d [Layer],
        dynamics: &AnimationGraph,
        canvas: [u32; 2],
        depth: usize,
        output: &mut Vec<&'d Layer>,
    ) {
        for layer in layers {
            match layer.data() {
                LayerData::Audio(_) => output.push(layer),
                LayerData::Group(group) if super::audio_groups::sound_only(group) => {
                    collect(&group.layers, dynamics, canvas, depth + 1, output);
                }
                _ => {
                    if let Some(group) = exported_nest(layer, layers, dynamics, depth, canvas) {
                        let stage = is_nested_transform_stage(group)
                            .then(|| nested_transform_stage(group, dynamics, canvas).ok())
                            .flatten();
                        let (content, frame) = stage
                            .as_ref()
                            .map_or((group, canvas), |stage| (stage.picture, stage.frame));
                        collect(&content.layers, dynamics, frame, depth + 1, output);
                    }
                }
            }
        }
    }
    let mut output = Vec::new();
    collect(layers, dynamics, canvas, 0, &mut output);
    output.into_iter()
}

/// Whether a group that exports as no clip exports as a nest, if
/// [`unsupported_group`] accepts it: every group but a stage group whose video
/// does not play over the whole group ([`stage_video_spans_group`]), which is
/// omitted instead.
fn nest_candidate(group: &GroupLayer) -> bool {
    match stage_layers(group).map(super::video_data) {
        Some(Ok(Some(video))) => stage_video_spans_group(group, &video),
        _ => true,
    }
}

/// Whether a stage group whose one clip export omits ([`clip_omitted`])
/// exports as a nest instead, whose placement carries its mask: a nest
/// candidate ([`nest_candidate`]) without a track matte. A stage group's
/// track matte source is its child and a nest's is its sibling, so export
/// omits a keyed stage group whole, with its own reason.
pub(super) fn stage_exports_as_nest(group: &GroupLayer) -> bool {
    group.track_matte.is_none() && nest_candidate(group)
}

/// Borrowed content owner of the one-native-nest Transform representation.
/// Never fall back to a second rasterizing native nest for a recognized but
/// edited/unsupported stage.
struct NestedTransformStage<'a> {
    picture: &'a GroupLayer,
    guide: LayerId,
    frame: [u32; 2],
    effect: crate::schema::PrEffect,
    reports: Vec<Omission>,
}

/// A structurally isolated inner picture needs one native raster boundary,
/// not an additional Normal nest. Full geometry/clock/control admission follows
/// in `nested_transform_stage`; neither owner name establishes authority.
pub(super) fn is_nested_transform_stage(group: &GroupLayer) -> bool {
    matches!(group.layers.as_slice(), [child]
        if matches!(child.data(), LayerData::Group(picture)
            // Preserve the existing adjustment owner's native path. Its masked
            // affine shape must not be captured as a nested Transform picture.
            if !super::adjustment_geometry::is_stage(picture)
                && picture.effects.is_empty()
                && picture.track_matte.is_none()
                && !picture.masks.is_empty()
                && picture.masks.iter().any(|mask| mask.layer.is_some_and(|id|
                    picture.layers.iter().any(|layer|
                        layer.id() == id && matches!(layer.data(), LayerData::Rect(guide)
                            if guide.rect.position == [0.0; 2]
                                && guide.rect.roundness == 0.0))))))
}

// A failed native control mapping must not shrink a validated current source
// canvas to the parent canvas before the inner picture can move its pixels.
fn nested_transform_frame(
    group: &GroupLayer,
    dynamics: &AnimationGraph,
) -> Option<([u32; 2], LayerId)> {
    let [child] = group.layers.as_slice() else {
        return None;
    };
    let LayerData::Group(picture) = child.data() else {
        return None;
    };
    let [mask] = picture.masks.as_slice() else {
        return None;
    };
    let guide_id = mask.layer?;
    if *mask != guide_mask(mask.id, guide_id, 0.0)
        || layer_animations(dynamics, guide_id).next().is_some()
        || dynamics
            .entries()
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
        || consumed_layer_ids(&picture.layers, false).contains(&guide_id)
    {
        return None;
    }
    let guide = picture.layers.iter().find(|layer| layer.id() == guide_id)?;
    let LayerData::Rect(guide) = guide.data() else {
        return None;
    };
    let size = guide.rect.size;
    if size.iter().any(|value| {
        !value.is_finite() || *value <= 0.0 || *value > f64::from(u32::MAX) || value.fract() != 0.0
    }) {
        return None;
    }
    let frame = size.map(|value| value as u32);
    let local = TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration);
    let mut expected = guide_layer(
        guide_id,
        guide.name.clone(),
        Some(picture.id),
        local,
        identity_transform(),
        black_shape(frame[0], frame[1]),
    );
    expected.rect.fill_enabled = guide.rect.fill_enabled;
    (guide == &expected).then_some((frame, guide_id))
}

fn nested_transform_stage<'a>(
    group: &'a GroupLayer,
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> std::result::Result<NestedTransformStage<'a>, String> {
    let invalid = || {
        "nested Transform stage requires one unmasked Motion owner, one affine picture owner and a static whole-source-canvas guide".to_owned()
    };
    if !group.masks.is_empty() || group.track_matte.is_some() || !group.effects.is_empty() {
        return Err(invalid());
    }
    let [child] = group.layers.as_slice() else {
        return Err(invalid());
    };
    let LayerData::Group(picture) = child.data() else {
        return Err(invalid());
    };
    let local = TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration);
    let identity_clock =
        LayerPlayback::linear(local, local, local, 0).map_err(|e| e.to_string())?;
    if picture.parent != Some(group.id)
        || picture.is_hidden
        || picture.blend_mode != BlendMode::Normal
        || picture.track_matte.is_some()
        || !picture.effects.is_empty()
        || !picture.description.is_empty()
        || picture.playback != identity_clock
        || unsupported_group_fields(picture, dynamics).is_some()
        || picture
            .layers
            .iter()
            .any(|layer| layer.parent_id() != Some(picture.id))
    {
        return Err(invalid());
    }
    let (frame, guide_id) = nested_transform_frame(group, dynamics).ok_or_else(invalid)?;
    let mut reports = Vec::new();
    let effect = super::effects::export_layer_transform(
        MotionHost {
            id: picture.id,
            transform: &picture.transform,
        },
        dynamics,
        frame,
        0,
        None,
        &group.name,
        &mut reports,
    )
    .map_err(|reason| format!("nested Transform stage: {reason}"))?;
    if frame != canvas {
        let motion = &group.transform;
        if motion.skew != 0.0
            || motion.skew_axis != 0.0
            || motion.rotation_x != 0.0
            || motion.rotation_y != 0.0
            || motion.orientation != [0.0; 3]
            || motion.position.z().is_some_and(|z| z != 0.0)
        {
            return Err(format!(
                "nested Transform stage: differing-canvas nested Transform requires planar Motion without skew; source {frame:?}, placement {canvas:?}"
            ));
        }
        let native_motion = export_transform(
            ClipLayers::Nest(group).motion(),
            frame,
            canvas,
            dynamics,
            &group.name,
            &mut reports,
        );
        if let Some(reason) = crate::schema::nested_transform_canvas_reason(
            frame,
            canvas,
            &native_motion,
            motion.opacity.value(),
            layer_animations(dynamics, group.id).next().is_some(),
            &effect,
        ) {
            return Err(format!(
                "nested Transform stage: {reason}; source {frame:?}, placement {canvas:?}"
            ));
        }
    }
    Ok(NestedTransformStage {
        picture,
        guide: guide_id,
        frame,
        effect,
        reports,
    })
}

/// Generated Geometry2 nests must not be mistaken for their editable stage on
/// reimport. Use the same name for publication and the writer-bound admission.
fn nested_sequence_name(group: &GroupLayer) -> String {
    let name = if super::adjustment_geometry::is_stage(group) {
        format!("Adjustment Geometry2 nest ({})", group.name)
    } else if group.name.is_empty() {
        format!("Group {}", group.id)
    } else {
        group.name.clone()
    };
    name.chars().take(255).collect()
}

/// Exports one group as a nest or direct timed stills in `video_tracks`.
/// Returns a track index only for a native nest, so its caller can bind that
/// nest's matte relationships. Direct stills and omitted groups return `None`.
pub(super) fn export_group<'d>(
    group: &'d GroupLayer,
    layers: &[Layer],
    video_tracks: &mut Vec<PrVideoTrack>,
    context: &mut LayerExport<'_, 'd>,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<usize>> {
    export_group_at(group, layers, None, video_tracks, context, omissions)
}

/// A stage-child provider becomes a sibling nest on the consumer's clock.
/// The existing nest/packing path owns its contents and linked TrackMatte.
pub(super) fn export_group_at<'d>(
    group: &'d GroupLayer,
    layers: &[Layer],
    staged_window: Option<TimeRangeProperty>,
    video_tracks: &mut Vec<PrVideoTrack>,
    context: &mut LayerExport<'_, 'd>,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<usize>> {
    let record = format!("layer {} ({:?})", group.id, group.name);
    let label_length = group.name.chars().count()
        + if super::adjustment_geometry::is_stage(group) {
            "Adjustment Geometry2 nest ()".len()
        } else {
            0
        };
    if group.name.is_empty() || label_length > 255 {
        omit_field(omissions, group.id, ExportField::Metadata, &record,
            "Group name cannot be a native sequence label; generated/truncated the label to 1–255 characters while retaining its content");
    }
    let canvas = [context.width, context.height];
    if has_background(group)
        || layer_animations(context.dynamics, group.id)
            .any(|(property, _)| is_background_property(property))
    {
        omit_field(omissions, group.id, ExportField::Effects, &record,
            "Group background/padding was not exported; current children, masks, base controls and supported keys retained");
    }
    let retimed = !super::timing::is_plain_group_playback(&group.playback);
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
    let stage = if is_nested_transform_stage(group) {
        match nested_transform_stage(group, context.dynamics, canvas) {
            Ok(stage) => Some(stage),
            Err(reason) => {
                approximate(omissions, &record, format!(
                    "native nested Transform was not reconstructed: {reason}; retaining current children, masks and supported Motion through ordinary nested export; the additional canvas clipping/raster boundary and native sampling are an approximation"
                ));
                None
            }
        }
    } else {
        None
    };
    let is_geometry_stage = super::adjustment_geometry::is_stage(group);
    let opacity_fallback = is_geometry_stage
        && super::adjustment_geometry::has_authored_opacity(group, context.dynamics);
    let opacity_guide = if opacity_fallback {
        Some(
            super::adjustment_geometry::stage_guide(group, context.dynamics, canvas)
                .map_err(unsupported)?,
        )
    } else {
        None
    };
    let mut geometry_reports = Vec::new();
    if opacity_fallback {
        approximate(&mut geometry_reports, &record,
            "adjustment Geometry2 stage with authored Group opacity exported as an ordinary moved nest: Group affine controls and Opacity keys retained as native Motion/Opacity, not unmeasured adjustment Geometry2 opacity mix; native sampling and clipping fidelity remain unmeasured");
    }
    let geometry = if is_geometry_stage && !opacity_fallback {
        match super::adjustment_geometry::export_stage(
            group,
            context.dynamics,
            canvas,
            context.frame_rate.generator_in_ticks(),
            &mut geometry_reports,
        ) {
            Ok(geometry) => Some(geometry),
            Err(reason) => {
                approximate(&mut geometry_reports, &record, format!(
                    "native adjustment Geometry2 controls not reconstructed: {reason}; valid guide and current children retained through ordinary nested Motion/Opacity; sampling/clipping remain approximate"));
                None
            }
        }
    } else {
        None
    };
    let content = stage.as_ref().map_or(group, |stage| stage.picture);
    let frame = stage
        .as_ref()
        .map(|stage| stage.frame)
        .or_else(|| {
            is_nested_transform_stage(group)
                .then(|| nested_transform_frame(group, context.dynamics).map(|(frame, _)| frame))
                .flatten()
        })
        .unwrap_or(canvas);
    // Only a structural, disjoint Image group can bypass a nest. Matte sources
    // and explicit Transform stages retain their native placement boundary.
    let is_track_matte_source =
        staged_window.is_some() || track_matte_sources(layers).contains(&group.id);
    let timed_matte = is_track_matte_source
        && super::timed_images::is_matte_source(group, context.dynamics, canvas);
    if stage.is_none() && !is_geometry_stage && !is_track_matte_source {
        if let Some(images) = super::timed_images::export_group(group, context, omissions)? {
            let retained = !images.is_empty();
            for (clip, media) in images {
                context.media.entry(clip.media.clone()).or_insert(media);
                super::tesseract_to_premiere::place_item(
                    video_tracks,
                    PrVideoItem::Media(clip),
                    0,
                    context,
                )?;
            }
            if retained {
                retain_group_motion_blur(group, context);
            }
            return Ok(None);
        }
    }
    if stage.is_none()
        && !is_geometry_stage
        && !is_track_matte_source
        && super::timed_images::oversized_source(group, context)?
    {
        omit(omissions, OmissionScope::Occurrence, &record,
            "Image group cannot use direct still placement and its oversized source is not proved inside the native nest canvas; occurrence omitted rather than clipping pixels before Group Motion");
        return Ok(None);
    }
    let clip = ClipLayers::Nest(group);
    let mask = if geometry.is_some() {
        None
    } else {
        canonical_mask(clip, canvas, layers, context.dynamics, canvas[0], canvas[1])
            .map_err(unsupported)?
    };
    let window = staged_window.unwrap_or_else(|| group.playback.input_range());
    let active_end = window
        .start
        .checked_add_duration(window.duration)
        .ok_or_else(|| unsupported("group activeRange end exceeds Premiere's tick range"))?;
    let start_ticks = context.frame_ticks(window.start, "group playback.inputRange.start")?;
    let mut end_ticks = if timed_matte {
        super::timed_images::sampled_end_ticks(context, active_end)?
    } else {
        context.picture_end_ticks(active_end, mask.as_ref())?
    };
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
    let sampled_picture_end =
        if timed_matte || context.samples_picture_end(active_end, mask.as_ref())? {
            Some(
                context
                    .origin
                    .checked_add_duration(Duration::from_millis(active_end.as_millis()))
                    .ok_or_else(|| unsupported("picture end exceeds Premiere's tick range"))?,
            )
        } else {
            context.sampled_picture_end
        };
    let collapsed = if timed_matte {
        // Ordinary timed-image lowering diagnoses zero-frame samples individually.
        None
    } else {
        collapsed_child(content, origin, sampled_picture_end, context, frame).map_err(|source| {
            BuildError::Context {
                context: record.clone(),
                source: Box::new(source),
            }
        })?
    };
    if let Some(reason) = collapsed {
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
    // The placement plays from inner time zero, so its keys count from the
    // group's start. Its Crop is a percentage of its picture, the canvas.
    let (crop, linear_wipe) = export_mask(mask.as_ref(), context, 0, &record, omissions)?;
    let opacity_mask = match &mask {
        Some(CanonicalMask::Opacity { mask, .. }) => Some(mask.clone()),
        _ => None,
    };
    // The placement's Motion, which the clips inside do not follow; it is
    // reported after them.
    let mut motion_reports = Vec::new();
    let mut transform = export_transform(
        clip.motion(),
        frame,
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
    let animations = if geometry.is_some() {
        // G is an actual adjustment effect inside the nest, not outer Motion.
        transform = PrStaticTransform::default();
        motion_reports.clear();
        Vec::new()
    } else {
        export_motion_keys(
            &mut tracks,
            0,
            &mut transform,
            frame,
            canvas,
            &record,
            &mut motion_reports,
        )
        .0
    };
    // Equal-canvas identity Motion can use Premiere's centered default.
    // With differing canvases, equal normalized points need not be identity.
    let motion_animated = animates_motion(&animations);
    if frame == canvas
        && !motion_animated
        && transform.position == transform.anchor_point
        && transform.scale == [100.0; 2]
        && transform.rotation == 0.0
    {
        transform = PrStaticTransform::default();
    }
    if timed_matte {
        // Disjoint stills carry the provider's own Motion/Opacity inside this
        // neutral nest, through ordinary timed-image lowering. The consumer's
        // outer stage Motion therefore still moves both picture and coverage.
        transform = PrStaticTransform::default();
    }
    let sequence_name = nested_sequence_name(group);
    let child_container = context.packer.begin_container(
        context.boundary()?,
        &sequence_name,
        frame,
        context.frame_rate,
        0,
    )?;
    // Restore standalone nested-sound media if the group has no picture.
    let media = content
        .layers
        .iter()
        .any(|layer| matches!(layer.data(), LayerData::Audio(_)))
        .then(|| context.media.clone());
    let mut audio = Vec::new();
    let mut child_written = WrittenAnimation::default();
    let mut child_motion_blur_written = false;
    let mut child_group_motion_blur = Vec::new();
    let nest_scale = context
        .nest_scale
        .zip(super::graphic::smallest_scale(group, context))
        .map(|(outer, own)| outer * own);
    let mut inner = LayerExport {
        nest_scale,
        dynamics: context.dynamics,
        property_tracks: &mut *context.property_tracks,
        written: &mut child_written,
        media_facts: context.media_facts,
        authored_video_durations: context.authored_video_durations,
        natural_frames: context.natural_frames,
        audio_facts: context.audio_facts,
        fonts: context.fonts,
        audio: &mut audio,
        media: &mut *context.media,
        packer: &mut *context.packer,
        container: child_container,
        boundary: None,
        width: frame[0],
        height: frame[1],
        frame_rate: context.frame_rate,
        origin,
        sampled_picture_end,
        depth: context.depth + 1,
        canvas: None,
        group_guide: stage
            .as_ref()
            .map(|stage| stage.guide)
            .or_else(|| geometry.as_ref().map(|(guide, _)| *guide))
            .or(opacity_guide)
            .or_else(|| mask.as_ref().and_then(CanonicalMask::guide_id)),
        // The flattened picture owner still transforms its descendants.
        in_moved_nest: context.in_moved_nest
            || stage.is_some()
            || motion_animated
            || transform != PrStaticTransform::default(),
        motion_blur: context.motion_blur,
        motion_blur_written: &mut child_motion_blur_written,
        unmapped_group_motion_blur: &mut child_group_motion_blur,
    };
    let mut inner_tracks = if timed_matte {
        let frames = super::timed_images::export_matte_frames(group, &mut inner, omissions)?
            .ok_or_else(|| {
                unsupported("admitted timed matte no longer has timed image structure")
            })?;
        let mut tracks = Vec::new();
        for (id, clip, media) in frames {
            inner.boundary = Some(inner.packer.begin_boundary(inner.container, id)?);
            inner.media.entry(clip.media.clone()).or_insert(media);
            super::tesseract_to_premiere::place_item(
                &mut tracks,
                PrVideoItem::Media(clip),
                0,
                &mut inner,
            )?;
        }
        tracks
    } else {
        export_layers(&content.layers, Some(content.id), &mut inner, omissions).map_err(
            |source| BuildError::Context {
                context: record.clone(),
                source: Box::new(source),
            },
        )?
    };
    if let Some((guide_id, effect)) = &geometry {
        super::adjustment_geometry::place_exported(
            group,
            *guide_id,
            effect.clone(),
            &mut inner_tracks,
            &mut inner,
            omissions,
        )?;
    }
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
    if !empty && !retimed && end_ticks - start_ticks > inner_end {
        // Preserve all retained child picture/sound at its authored clock.
        // Any unsupported background/effect-only tail is a local loss, not a
        // reason to discard the owner or its already-converted children.
        end_ticks = start_ticks + inner_end;
        omissions.emit_field(
            Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Approximated,
                record: record.clone(),
                reason: "group extends past its exported children's end; retained its supported native picture at unchanged child times and left the remaining group interval transparent".into(),
            },
            group.id,
            ExportField::Placement,
        );
    }
    if empty && stage.is_some() {
        if let Some(media) = media {
            *context.media = media;
        }
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            "nested Transform stage has no exportable source picture",
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
                still: false,
                staged: false,
                nested: true,
                in_nest: true,
                transform: &group.transform,
                source_in: 0,
                video_keys: None,
                static_parameters_reason: None,
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
                reverse_source_duration: None,
                playback_rate: 1.0,
                time_remap: None,
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
                opacity_mask: opacity_mask.clone(),
                track_matte: unplaced_track_matte(mask.as_ref()),
                effects: pending_effects,
                geometry2_masks: Default::default(),
                effects_above_mask: 0,
                enabled: !group.is_hidden,
                sequence: PrSequence {
                    native_frame_ticks: None,
                    id: None,
                    name: sequence_name.clone(),
                    top_level: Some(false),
                    video_tracks: inner_tracks,
                    audio,
                    frame_rate: context.frame_rate,
                    width: frame[0],
                    height: frame[1],
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
    if geometry.is_some() {
        for property in context
            .property_tracks
            .get(&group.id)
            .into_iter()
            .flat_map(|tracks| tracks.keys())
        {
            context.written.record(group.id, *property);
        }
    }
    context.property_tracks.remove(&group.id);
    for report in geometry_reports.into_iter().chain(motion_reports) {
        omissions.emit(report);
    }
    if let Some(mask) = &opacity_mask {
        for warning in mask.approximations() {
            approximate(omissions, &record, warning);
        }
    }
    let remap = if retimed {
        match recovered_group_remap(group, inner_end, end_ticks - start_ticks) {
            Ok(remap) => {
                approximate(omissions, &record,
                    "Group playback retained as bounded native linear TimeRemapping over its authored child selection; non-linear easing/direction changes are approximated, current children and owner keys retained on the input clock");
                Some(remap)
            }
            Err(reason) => {
                omit(omissions, OmissionScope::Occurrence, &record,
                    format!("no representable child interval remains for Group playback: {reason}; independent siblings retained"));
                return Ok(None);
            }
        }
    } else {
        None
    };
    let mut nest = PrNestOccurrence {
        id: None,
        reverse_source_duration: None,
        playback_rate: 1.0,
        time_remap: remap,
        start_ticks,
        end_ticks,
        in_ticks: 0,
        out_ticks: end_ticks - start_ticks,
        transform,
        opacity: if timed_matte {
            100.0
        } else {
            group.transform.opacity.value()
        },
        blend_mode: PrBlendMode::from_fx_mode(group.blend_mode),
        animations,
        crop,
        linear_wipe,
        opacity_mask,
        track_matte: unplaced_track_matte(mask.as_ref()),
        effects: if let Some(stage) = &stage {
            vec![stage.effect.clone()]
        } else {
            export_effects(
                &group.effects,
                context.dynamics,
                EffectHost {
                    layer: group.id,
                    still: false,
                    staged: false,
                    nested: true,
                    in_nest: true,
                    transform: &group.transform,
                    source_in: 0,
                    video_keys: None,
                    static_parameters_reason: None,
                    frame: canvas,
                    canvas,
                },
                context.written,
                &record,
                omissions,
            )
        },
        geometry2_masks: Default::default(),
        effects_above_mask: 0,
        enabled: !group.is_hidden,
        sequence: PrSequence {
            native_frame_ticks: None,
            id: None,
            name: sequence_name,
            top_level: Some(false),
            video_tracks: inner_tracks,
            audio,
            frame_rate: context.frame_rate,
            width: frame[0],
            height: frame[1],
            timeline_end_ticks: 0,
        },
    };
    // Sequence duration remains derived from real child placements.
    nest.sequence.timeline_end_ticks = nest.sequence.occurrence_end_ticks();
    let pass_through =
        pass_through_report(group, context.dynamics).filter(|_| blends_inside(&nest.sequence));
    context
        .written
        .record_placement(group.id, &nest.animations, mask, nest.linear_wipe.as_ref());
    if let Some(stage) = &stage {
        for report in &stage.reports {
            omissions.emit(report.clone());
        }
        // The original Geometry2-to-affine approximation is reported on import.
        // Repeating it here would classify that prior normalization as a new
        // export loss and route this native stage to linked AE.
        for property in context
            .property_tracks
            .remove(&stage.picture.id)
            .unwrap_or_default()
            .into_keys()
        {
            context.written.record(stage.picture.id, property);
        }
    }
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
    context
        .unmapped_group_motion_blur
        .extend(child_group_motion_blur);
    retain_group_motion_blur(group, context);
    if let Some(stage) = &stage {
        // This owner is flattened into the retained native Geometry2 effect.
        retain_group_motion_blur(stage.picture, context);
    }
    if let Some(warning) = PrBlendMode::export_approximation(group.blend_mode) {
        approximate(omissions, &record, warning);
    }
    if let Some(report) = pass_through {
        approximate(omissions, &record, report);
    }
    Ok(Some(index))
}

/// Defer this field loss until the containing picture reaches root output.
/// Admission also serves media collection, so it must not reject this field.
pub(super) fn retain_group_motion_blur<'d>(
    group: &'d GroupLayer,
    context: &mut LayerExport<'_, 'd>,
) {
    if group.motion_blur {
        context.unmapped_group_motion_blur.push(group);
    }
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
    if super::adjustment_geometry::is_stage(group) {
        if let Err(reason) = super::adjustment_geometry::stage_guide(group, dynamics, canvas) {
            return Some(reason);
        }
        if !super::adjustment_geometry::has_authored_opacity(group, dynamics) {
            // Native-control recognition is not owner admission. A valid guide
            // can still carry current children through the ordinary mask path.
        }
        // Authored opacity uses the ordinary nest's existing animation and mask
        // admission below, not the adjustment effect's unmeasured mix control.
    }
    let clock_coverage = !super::timing::is_plain_group_playback(&group.playback)
        && (!group.masks.is_empty() || group.track_matte.is_some());
    clock_coverage.then(|| "retimed Group coverage cannot share the existing native mask/matte clock; inseparable masked picture omitted without exposing pixels".to_owned())
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
        (background, "group backgrounds are not supported"),
        (
            !super::timing::is_plain_group_playback(&group.playback),
            "group time remapping is not supported",
        ),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then(|| reason.to_owned()))
}

/// Why a group inside `depth` groups has no nest, if it has none.
pub(super) fn unsupported_nest_depth(depth: usize) -> Option<String> {
    (depth >= MAX_NEST_DEPTH)
        .then(|| format!("nesting deeper than {MAX_NEST_DEPTH} levels is not supported"))
}

/// Existing native linear remap carrier, bounded by actual child placements.
fn recovered_group_remap(group: &GroupLayer, inner_end: i64, duration: i64) -> Result<PrTimeRemap> {
    let selection = match group.playback.mapping() {
        fx_schema::LayerPlaybackMapping::Linear { .. } => {
            let range = group.playback.input_range();
            let start = super::timing::linear_source_ticks(
                &group.playback,
                ticks_from_time(range.start, "Group input")?,
            )?;
            let end = super::timing::linear_source_ticks(
                &group.playback,
                ticks_from_time(range.end(), "Group input")?,
            )?;
            start..end
        }
        fx_schema::LayerPlaybackMapping::TimeRemap { property } => {
            // Use existing affine-key evaluation first. Otherwise retain the
            // authored key-value selection, not an arbitrary zero-origin default.
            let range = group.playback.input_range();
            let endpoints = super::tesseract_to_premiere::constant_playback_source_range(
                property,
                range,
                TimeRangeProperty::new(Time::ZERO, duration_from_ticks(inner_end)?),
                group.playback.input_offset_ms(),
            );
            let selected = match endpoints {
                Ok((range, false)) => range,
                _ => {
                    match (
                        super::tesseract_to_premiere::linear_remap_source_time(
                            property,
                            range.start,
                            group.playback.input_offset_ms(),
                        ),
                        super::tesseract_to_premiere::linear_remap_source_time(
                            property,
                            range.end(),
                            group.playback.input_offset_ms(),
                        ),
                    ) {
                        (Ok(start), Ok(end)) if start != end => TimeRangeProperty::new(
                            start.min(end),
                            start.max(end).saturating_sub(start.min(end)),
                        ),
                        _ => {
                            // Fractional/unsupported endpoint evaluation retains
                            // only brackets consumed by this current window, not
                            // unused keys or historical source-selection tails.
                            let start = i128::from(range.start.as_millis())
                                + i128::from(group.playback.input_offset_ms());
                            let end = i128::from(range.end().as_millis())
                                + i128::from(group.playback.input_offset_ms());
                            let values = property
                                .keyframes()
                                .windows(2)
                                .filter(|pair| {
                                    i128::from(pair[0].time.as_millis()) < end
                                        && start < i128::from(pair[1].time.as_millis())
                                })
                                .flat_map(|pair| [pair[0].value, pair[1].value])
                                .collect::<Vec<_>>();
                            let first = values.iter().copied().min().ok_or_else(|| {
                                unsupported("Group remap has no consumed source keys")
                            })?;
                            let last = values.iter().copied().max().ok_or_else(|| {
                                unsupported("Group remap has no consumed source keys")
                            })?;
                            TimeRangeProperty::new(first, last.saturating_sub(first))
                        }
                    }
                }
            };
            ticks_from_time(selected.start, "Group source")?
                ..ticks_from_time(selected.end(), "Group source")?
        }
    };
    let start = selection.start.max(0);
    let end = selection.end.min(inner_end);
    ensure!(
        start < end && duration > 0,
        "Group source selection has no positive physical child span"
    );
    Ok(PrTimeRemap {
        keys: vec![
            crate::schema::PrTimeRemapKeyframe {
                timeline_ticks: 0,
                source_ticks: start,
                easing: PrKeyframeEasing::Linear,
            },
            crate::schema::PrTimeRemapKeyframe {
                timeline_ticks: duration,
                source_ticks: end,
                easing: PrKeyframeEasing::Linear,
            },
        ],
    })
}

#[cfg(test)]
#[path = "tests/nested.rs"]
mod tests;
