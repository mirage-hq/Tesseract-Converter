//! Map one shared editable document and its inspected media facts to sequence semantics.
use super::{
    background::{black_shape, identity_transform},
    effects,
    graphic::{premiere_path, scaled_path},
    keyframes,
    nested::{stage_exports_as_nest, LayerExport},
    packing::{PicturePacker, PicturePackingRecipe},
    premiere_to_tesseract::{mask_key_easing, wipe_frame_reason},
    text::{automatic_line_spacing, STROKE_WIDTH_RATIO},
    timing::{frame_ticks_from_time, ticks_from_time, time_from_ticks},
};
use crate::{
    error::{ensure, unsupported, BuildError, Result},
    export_loss::{
        omit_field, with_context, ExportContext, ExportField, ExportLossDomain, ExportLossSource,
        OmissionSink,
    },
    format::{
        FrameRate, MediaId, PrGraphic, PrMedia, PrProjectFile, PrSequence, PrVideoItem,
        PrVideoOccurrence,
    },
    media::MediaFacts,
    schema::{
        scalar_range,
        text::{
            normalize_line_breaks, PrGraphicObject, PrJustification, PrPathVertex, PrRgb,
            PrShapePath, PrTextDocument, PrTextFrame, PrTextStroke, PrTextTransform,
            PrVerticalAlign,
        },
        PrAnimatedProperty, PrBlendMode, PrKeyframeEasing, PrLinearWipe, PrMask, PrMaskPathKey,
        PrMatteChannel, PrMediaKind, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe,
        PrStaticCrop, PrStaticTransform, PrText, PrTrackMatte, PrVideoStream, PrVideoTrack,
        MOTION_PARAMS, TRANSFORM_SHUTTER_ANGLE,
    },
    {approximate, omit, Omission, OmissionScope},
};
use fx_schema::{
    animator::{
        AnimationGraph, AnimationGraphEntry, AnimatorData, PropertyKeyframeEasing,
        PropertyKeyframeTrack,
    },
    BlendMode, EditableFxCompositionDocument, EffectData, EffectPayload, FontAssetProperties,
    GroupLayer, ImageLayer, ImageSource, Justification, Layer, LayerData, LayerEffect, LayerId,
    MaskMode, MediaSourceKind, PathMask, PositiveRect, PropType, PropertyTarget, PropertyValue,
    RectLayer, ShapeLayer, ShapePath, TextDocument, TextLayer, Time, TimeRangeProperty,
    TimeRemapExtrapolation, TimeRemapProperty, TrackMatte, TrackMatteType, Transform,
    VerticalAlign, VideoLayer,
};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
};

struct Processed<'a> {
    progress: Option<(fx_conv::ProgressPhase<'a>, &'a Cell<usize>)>,
}

impl Drop for Processed<'_> {
    fn drop(&mut self) {
        if let Some((phase, count)) = self.progress {
            let completed = count.get() + 1;
            count.set(completed);
            phase.update(completed);
        }
    }
}

/// The source interval actually traversed by constant playback; sourceRange
/// bounds the selected keys rather than requiring traversal of the whole range.
pub(super) fn constant_playback_source_range(
    playback: &TimeRemapProperty,
    active_range: TimeRangeProperty,
    source_range: TimeRangeProperty,
    input_offset_ms: i64,
) -> Result<(TimeRangeProperty, bool)> {
    let keys = playback.keyframes();
    if playback.before() != TimeRemapExtrapolation::Inactive
        || playback.after() != TimeRemapExtrapolation::Inactive
        || keys.len() != 2
        || i128::from(keys[0].time.as_millis())
            > i128::from(active_range.start.as_millis()) + i128::from(input_offset_ms)
        || i128::from(keys[1].time.as_millis())
            < i128::from(active_range.end().as_millis()) + i128::from(input_offset_ms)
        || keys[1].easing != PropertyKeyframeEasing::Linear
    {
        return Err(unsupported(
            "only a bounded two-key linear constant TimeRemap can be exported",
        ));
    }
    let start = keys[0].value.min(keys[1].value);
    let end = keys[0].value.max(keys[1].value);
    ensure!(
        start < end,
        "constant TimeRemap source interval must be nonempty"
    );
    ensure!(
        start >= source_range.start && end <= source_range.end(),
        "TimeRemap endpoints extend beyond the authored source selection"
    );
    let selected_start = linear_remap_source_time(playback, active_range.start, input_offset_ms)?;
    let selected_end = linear_remap_source_time(playback, active_range.end(), input_offset_ms)?;
    ensure!(
        selected_start != selected_end,
        "constant TimeRemap selected source interval must be nonempty"
    );
    let start = selected_start.min(selected_end);
    let end = selected_start.max(selected_end);
    Ok((
        TimeRangeProperty::new(start, end.saturating_sub(start)),
        selected_end < selected_start,
    ))
}

/// Evaluate a selected linear segment without rounding a fractional source endpoint.
pub(super) fn linear_remap_source_time(
    playback: &TimeRemapProperty,
    time: Time,
    input_offset_ms: i64,
) -> Result<Time> {
    let shifted = i128::from(time.as_millis()) + i128::from(input_offset_ms);
    let shifted = u64::try_from(shifted)
        .map(Time::from_millis)
        .map_err(|_| unsupported("TimeRemap input offset exceeds the source clock"))?;
    let pair = playback
        .keyframes()
        .windows(2)
        .find(|pair| pair[0].time <= shifted && shifted <= pair[1].time)
        .ok_or_else(|| {
            unsupported("TimeRemap active window extends beyond its authored key span")
        })?;
    let relative = i128::from(shifted.as_millis() - pair[0].time.as_millis());
    let source_delta =
        i128::from(pair[1].value.as_millis()) - i128::from(pair[0].value.as_millis());
    let numerator = relative
        .checked_mul(source_delta)
        .ok_or_else(|| unsupported("TimeRemap interpolation exceeds the source clock"))?;
    let denominator = i128::from(pair[1].time.as_millis() - pair[0].time.as_millis());
    ensure!(
        numerator % denominator == 0,
        "TimeRemap window has fractional millisecond source endpoints"
    );
    let mapped = i128::from(pair[0].value.as_millis()) + numerator / denominator;
    u64::try_from(mapped)
        .map(Time::from_millis)
        .map_err(|_| unsupported("TimeRemap source endpoint exceeds the source clock"))
}

struct NativePlayback {
    source_range: TimeRangeProperty,
    reverse: bool,
    approximated: bool,
}

/// The source instant that `video` shows as an explicit Frame Hold: exactly
/// two equal keys, the second Linear, whose span contains the active window
/// after the signed input offset, on an instant of the half-open source
/// selection. The covered window never shows the extrapolation, so any
/// qualifies; other curves stay with [`native_playback_source_range`].
pub(super) fn held_source(video: &VideoLayer) -> Option<Time> {
    let keys = video.playback.time_remap()?.keyframes();
    if keys.len() != 2 {
        return None;
    }
    let (first, last) = (&keys[0], &keys[1]);
    let window = video.playback.input_range();
    let offset = i128::from(video.playback.input_offset_ms());
    let shifted = |time: Time| i128::from(time.as_millis()) + offset;
    let covers_window = i128::from(first.time.as_millis()) <= shifted(window.start)
        && i128::from(last.time.as_millis()) >= shifted(window.end());
    let held = first.value;
    let holds_selected_instant = last.value == held
        && last.easing == PropertyKeyframeEasing::Linear
        && video.source_range.start <= held
        && held < video.source_range.end();
    (covers_window && holds_selected_instant).then_some(held)
}

/// Retain a validated authored source selection when native playback cannot
/// express the curve. This changes timing, never asset identity or selection.
fn native_playback_source_range(
    playback: &TimeRemapProperty,
    active_range: TimeRangeProperty,
    source_range: TimeRangeProperty,
    input_offset_ms: i64,
) -> Result<NativePlayback> {
    if let Ok((source_range, reverse)) =
        constant_playback_source_range(playback, active_range, source_range, input_offset_ms)
    {
        return Ok(NativePlayback {
            source_range,
            reverse,
            approximated: false,
        });
    }
    ensure!(
        source_range.duration.as_millis() > 0,
        "authored source selection is empty"
    );
    let linear = playback.keyframes().windows(2).all(|pair| {
        pair[1].easing == PropertyKeyframeEasing::Linear && pair[1].value > pair[0].value
    });
    let endpoints = linear
        .then(|| {
            let start =
                linear_remap_source_time(playback, active_range.start, input_offset_ms).ok()?;
            let end =
                linear_remap_source_time(playback, active_range.end(), input_offset_ms).ok()?;
            (start >= source_range.start && end <= source_range.end() && start < end)
                .then_some(TimeRangeProperty::new(start, end.saturating_sub(start)))
        })
        .flatten();
    Ok(NativePlayback {
        source_range: endpoints.unwrap_or(source_range),
        reverse: false,
        approximated: true,
    })
}

pub(super) fn scale_tracks_match(x: &PropertyKeyframeTrack, y: &PropertyKeyframeTrack) -> bool {
    x.keyframes().len() == y.keyframes().len()
        && x.keyframes().iter().zip(y.keyframes()).all(|(x, y)| {
            x.layer_time() == y.layer_time()
                && x.value() == y.value()
                && x.easing() == y.easing()
                && x.spatial_in_tangent() == y.spatial_in_tangent()
                && x.spatial_out_tangent() == y.spatial_out_tangent()
        })
}

pub(super) fn export_position_keys(
    x: &PropertyKeyframeTrack,
    y: &PropertyKeyframeTrack,
    source_in: i64,
    dimensions: [u32; 2],
) -> Result<Vec<PrPointKeyframe>> {
    PropertyKeyframeTrack::validate_position_pair(x, y)
        .map_err(|error| unsupported(format!("Position keyframes cannot be exported: {error}")))?;
    let mut keys = Vec::with_capacity(x.keyframes().len());
    for (x, y) in x.keyframes().iter().zip(y.keyframes()) {
        let (PropertyValue::Float(x_value), PropertyValue::Float(y_value)) = (x.value(), y.value())
        else {
            return Err(unsupported("Position keys must have float values"));
        };
        if x.layer_time() != y.layer_time() || x.easing() != y.easing() {
            return Err(unsupported(
                "Position X/Y keys must have identical times and temporal easing",
            ));
        }
        let pair_tangent = |x: Option<f64>, y: Option<f64>, name: &str| match (x, y) {
            (Some(x), Some(y)) => Ok(Some([
                x / f64::from(dimensions[0]),
                y / f64::from(dimensions[1]),
            ])),
            (None, None) => Ok(None),
            _ => Err(unsupported(format!(
                "Position X/Y {name} spatial tangents must be paired"
            ))),
        };
        keys.push(PrPointKeyframe {
            source_ticks: keyframes::source_ticks(source_in, x.layer_time().as_millis())?,
            value: [
                *x_value / f64::from(dimensions[0]),
                *y_value / f64::from(dimensions[1]),
            ],
            easing: keyframes::native_easing(x.easing())?,
            spatial_in_tangent: pair_tangent(
                x.spatial_in_tangent(),
                y.spatial_in_tangent(),
                "incoming",
            )?,
            spatial_out_tangent: pair_tangent(
                x.spatial_out_tangent(),
                y.spatial_out_tangent(),
                "outgoing",
            )?,
        });
    }
    for pair in keys.windows(2) {
        let distance = crate::schema::spatial::segment_length(&pair[0], &pair[1])
            .ok_or_else(|| unsupported("Position spatial curve length is nonfinite"))?;
        if matches!(pair[1].easing, PrKeyframeEasing::CubicBezier { .. }) && distance == 0.0 {
            return Err(unsupported(
                "Position cubic easing on a stationary spatial segment cannot preserve Premiere velocity",
            ));
        }
    }
    Ok(keys)
}

/// The native keys of `track`, counted from `source_in`. A first key after
/// the source In exports as it is: Premiere holds it before its time, as FX
/// does, so the static value written as `StartKeyframe` never shows.
pub(super) fn export_scalar_keys(
    track: &PropertyKeyframeTrack,
    source_in: i64,
    property: &str,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Vec<PrScalarKeyframe>> {
    let mut keys = Vec::with_capacity(track.keyframes().len());
    let mut key_omissions = Vec::new();
    let spec = MOTION_PARAMS.iter().find(|spec| spec.name == property);
    for key in track.keyframes() {
        let PropertyValue::Float(value) = key.value() else {
            omit(&mut key_omissions, OmissionScope::Feature, record,
                format!("{property} nonnumeric key was skipped; other keys and static controls retained"));
            continue;
        };
        if !value.is_finite() || spec.is_some_and(|spec| !spec.holds(*value)) {
            omit(&mut key_omissions, OmissionScope::Feature, record,
                format!("{property} key at {} ms was not representable in the native control; other keys and static controls retained", key.layer_time().as_millis()));
            continue;
        }
        let source_ticks = match keyframes::source_ticks(source_in, key.layer_time().as_millis()) {
            Ok(time) => time,
            Err(error) => {
                omit(&mut key_omissions, OmissionScope::Feature, record,
                    format!("{property} key was skipped: {error}; other keys and static controls retained"));
                continue;
            }
        };
        if key.spatial_in_tangent().is_some() || key.spatial_out_tangent().is_some() {
            omit(
                &mut key_omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "{property} key at {} ms: spatial tangents were not exported",
                    key.layer_time().as_millis()
                ),
            );
        }
        let easing = match keyframes::native_easing(key.easing()) {
            Ok(easing) => easing,
            Err(error) => {
                approximate(
                    &mut key_omissions,
                    record,
                    format!("{property} key easing approximated as linear: {error}"),
                );
                PrKeyframeEasing::Linear
            }
        };
        keys.push(PrScalarKeyframe {
            source_ticks,
            value: *value,
            easing,
        });
    }
    for index in 1..keys.len() {
        let equal_cubic =
            keys[index].easing.bezier().is_some() && keys[index - 1].value == keys[index].value;
        let arrival_into_hold = keys
            .get(index + 1)
            .is_some_and(|next| next.easing == PrKeyframeEasing::Hold)
            && !keyframes::premiere_keeps_the_arrival_into_a_hold(keys[index].easing);
        let range = scalar_range(&[
            (keys[index - 1].value, None),
            (keys[index].value, keys[index].easing.bezier()),
        ]);
        let outside = spec.is_some_and(|spec| range.into_iter().any(|value| !spec.holds(value)));
        if equal_cubic || arrival_into_hold || outside {
            keys[index].easing = PrKeyframeEasing::Linear;
            approximate(&mut key_omissions, record,
                format!("{property} segment easing approximated as linear; valid key times and values retained"));
        }
    }
    for omission in key_omissions {
        omissions.emit(omission);
    }
    ensure!(
        !keys.is_empty(),
        "{property} has no representable keys; static control retained"
    );
    Ok(keys)
}

/// Why FX layer-clock keys cannot be written on a clip with `playback_rate`.
///
/// Native keys are on the source clock; how Premiere places them under
/// constant speed or reverse is not pinned by an Adobe fixture.
fn retimed_keys_reason(playback_rate: f64) -> Option<&'static str> {
    (playback_rate != 1.0).then_some("keys on a retimed or reversed clip are not converted")
}

/// Whether the written `animations` move the clip frame, which Crop and Linear
/// Wipe masks must follow and a Directional Blur inside a nest cannot. Keys
/// that were not written leave static Motion.
pub(super) fn animates_motion(animations: &[PrPropertyAnimation]) -> bool {
    animations
        .iter()
        .any(|animation| animation.property() != PrAnimatedProperty::Opacity)
}

/// Map a canonical Linear Wipe guide to the native effect on the clip's
/// source clock starting at `source_in`. The caller checks the clip frame
/// with [`wipe_frame_reason`].
fn export_linear_wipe(
    wipe: CanonicalLinearWipe,
    guide_tracks: Option<&BTreeMap<PropType, &PropertyKeyframeTrack>>,
    default_visible: f64,
    source_in: i64,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<PrLinearWipe> {
    let guide_tracks = guide_tracks
        .ok_or_else(|| unsupported("Linear Wipe guide is missing its scale animation"))?;
    let track = guide_tracks
        .get(&wipe.property_type)
        .ok_or_else(|| unsupported("Linear Wipe guide has the wrong scale animation"))?;
    if guide_tracks.len() != 1 {
        return Err(unsupported(
            "Linear Wipe guide has unsupported extra animation",
        ));
    }
    let mut completion = export_scalar_keys(
        track,
        source_in,
        "Linear Wipe completion",
        omissions,
        record,
    )?;
    for key in &mut completion {
        key.value = 100.0 - key.value;
        if !(0.0..=100.0).contains(&key.value) {
            return Err(unsupported(
                "Linear Wipe completion must remain between 0 and 100",
            ));
        }
    }
    // One key holds everywhere in FX. Write its current value as a native
    // constant, including when it was edited independently of the base scale.
    let initial_completion = if completion.len() == 1 {
        completion.remove(0).value
    } else {
        100.0 - default_visible
    };
    Ok(PrLinearWipe {
        initial_completion,
        completion,
        angle_degrees: wipe.angle_degrees,
        feather: wipe.feather,
    })
}

pub(super) fn source_media<'a>(
    media: &'a mut BTreeMap<MediaId, PrMedia>,
    id: &MediaId,
) -> &'a mut PrMedia {
    media.entry(id.clone()).or_insert_with(|| PrMedia {
        name: String::new(),
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: None,
        audio: None,
    })
}

#[cfg(test)]
mod position_tests {
    use super::*;
    use crate::schema::{TICKS, TICKS_PER_MILLISECOND};
    use fx_schema::{animator::PropertyKeyframe, KeyframeId, PropertyKeyframeEasing, TimeOffset};

    #[test]
    fn a_delayed_first_position_key_exports_with_its_own_value_and_source_tick() {
        let track = |axis, value| {
            PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
                KeyframeId::new(format!("position-{axis}")),
                TimeOffset::from_millis(500),
                PropertyValue::Float(value),
                PropertyKeyframeEasing::Linear,
            )])
            .unwrap()
        };
        // Premiere holds the key before its time, so the export needs no
        // static value to show there.
        let x = track("x", 0.03 * 1920.0);
        let y = track("y", 0.49 * 1080.0);
        let keys = export_position_keys(&x, &y, TICKS, [1920, 1080]).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].source_ticks, TICKS + 500 * TICKS_PER_MILLISECOND);
        assert!((keys[0].value[0] - 0.03).abs() < 1e-12);
        assert!((keys[0].value[1] - 0.49).abs() < 1e-12);
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CanonicalLinearWipe {
    guide_id: LayerId,
    property_type: PropType,
    angle_degrees: i16,
    feather: f64,
    /// The guide's static visible fraction on its wipe axis, 0 to 100.
    initial_visible: f64,
}

/// The Crop, Linear Wipe, Opacity mask or Track Matte Key that one clip's
/// mask or track matte exports as.
#[derive(Debug, Clone)]
pub(super) enum CanonicalMask {
    Crop {
        crop: PrStaticCrop,
        guide_id: LayerId,
    },
    LinearWipe(CanonicalLinearWipe),
    /// A shape guide or full-frame stage rectangle: Premiere's mask on the clip's
    /// intrinsic Opacity, which applies after every effect, as an FX mask on
    /// the stage group over the video does. FX applies a video's own mask
    /// before its effects, so a flat video retains the mask and locally omits
    /// those owner effects rather than writing them in the wrong order.
    Opacity {
        mask: PrMask,
        mask_id: fx_schema::FxItemId,
        /// The modern shape guide, absent for a legacy inline outline that is
        /// already authored in the masked layer's source-frame coordinates.
        guide_id: Option<LayerId>,
    },
    /// A track matte whose source, a video, still or bounded nest beside the clip
    /// (video/still also under its stage group), exports on a track
    /// above every clip it keys ([`UNPLACED_MATTE_TRACK`]).
    TrackMatte {
        source: LayerId,
        channel: PrMatteChannel,
        sampled_end: bool,
    },
}

impl CanonicalMask {
    /// Admission proved that this consumer and its timed-image provider share sampled ends.
    pub(super) fn samples_end(&self) -> bool {
        matches!(
            self,
            Self::TrackMatte {
                sampled_end: true,
                ..
            }
        )
    }

    /// The guide that draws the mask and paints nothing. A track matte source
    /// is a clip of its own, not a guide.
    pub(super) fn guide_id(&self) -> Option<LayerId> {
        match self {
            Self::Crop { guide_id, .. } => Some(*guide_id),
            Self::Opacity { guide_id, .. } => *guide_id,
            Self::LinearWipe(wipe) => Some(wipe.guide_id),
            Self::TrackMatte { .. } => None,
        }
    }
}

/// The matte track of a keyed clip until export places its source, which no
/// sequence has: the shared timeline validation rejects it, so a clip never
/// exports without its matte.
const UNPLACED_MATTE_TRACK: usize = usize::MAX;

/// Why a flat video's effects are omitted when its own mask becomes a native
/// Opacity mask. A stage group could preserve the order, but flat export must
/// not place the effects on the wrong side of coverage.
const VIDEO_OPACITY_MASK_EFFECT_ORDER_REASON: &str = "FX applies this video effect after the video's mask, while Premiere applies clip effects before an Opacity mask; retaining the mask on a flat clip requires omitting the effect";

/// The Track Matte Key of a keyed clip before its matte source is placed.
pub(super) fn unplaced_track_matte(mask: Option<&CanonicalMask>) -> Option<PrTrackMatte> {
    match mask {
        Some(CanonicalMask::TrackMatte { channel, .. }) => Some(PrTrackMatte {
            track_index: UNPLACED_MATTE_TRACK,
            channel: *channel,
        }),
        _ => None,
    }
}

/// The Crop and the Opacity mask that a clip's `mask` writes on its
/// occurrence, a video's or a still's; a Linear Wipe and a Track Matte Key
/// write fields of their own. An Opacity mask's feather and expansion
/// approximations are reported with the placement.
pub(super) fn crop_and_opacity_mask(
    mask: Option<&CanonicalMask>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> (PrStaticCrop, Option<PrMask>) {
    let (crop, opacity_mask) = match mask {
        Some(CanonicalMask::Crop { crop, .. }) => (*crop, None),
        Some(CanonicalMask::Opacity { mask, .. }) => (PrStaticCrop::default(), Some(mask.clone())),
        Some(CanonicalMask::LinearWipe(_) | CanonicalMask::TrackMatte { .. }) | None => {
            (PrStaticCrop::default(), None)
        }
    };
    if let Some(mask) = &opacity_mask {
        for warning in mask.approximations() {
            approximate(omissions, record, warning);
        }
    }
    (crop, opacity_mask)
}

/// The FX layers that export as one Premiere clip.
#[derive(Clone, Copy)]
pub(super) enum ClipLayers<'a> {
    /// A video layer with its own mask.
    Video(&'a VideoLayer),
    /// An independently validated standard-effect mask scope, not a Transform stage.
    EffectScope(super::effect_mask::EffectScope<'a>),
    /// A stage group over one video layer and the guide of the group's one
    /// mask or the source of its track matte. The clip takes Motion, Opacity,
    /// their keys, Enable and its range from the group, and its media, source
    /// range, playback and effects from the video.
    Stage {
        group: &'a GroupLayer,
        video: &'a VideoLayer,
    },
    /// A group that exports as a nested sequence. The placement takes Motion,
    /// Opacity, their keys, Enable, its range, the group's one mask and its
    /// effects from the group; its picture is the sequence canvas.
    Nest(&'a GroupLayer),
    /// A still's image layer with its own mask, which exports only as a Crop
    /// or an Opacity mask ([`image_mask`]); the still's clip is written by
    /// [`super::still::export_image_layer`].
    Image(&'a ImageLayer),
}

/// The layer whose transform and keys a clip's Motion and Opacity take: a
/// video, a stage or nest group, or a still's image layer.
#[derive(Clone, Copy)]
pub(super) struct MotionHost<'a> {
    pub(super) id: LayerId,
    pub(super) transform: &'a Transform,
}

impl<'a> MotionHost<'a> {
    /// A still's image layer, whose transform and keys its clip takes.
    pub(super) fn image(image: &'a ImageLayer) -> Self {
        Self {
            id: image.id,
            transform: &image.transform,
        }
    }
}

impl<'a> ClipLayers<'a> {
    /// The layer whose Motion and Opacity keys the clip takes.
    fn motion_id(self) -> LayerId {
        match self {
            Self::Video(video) => video.id,
            Self::EffectScope(scope) => scope.group().id,
            Self::Stage { group, .. } | Self::Nest(group) => group.id,
            Self::Image(image) => image.id,
        }
    }

    fn transform(self) -> &'a Transform {
        match self {
            Self::Video(video) => &video.transform,
            Self::EffectScope(scope) => &scope.group().transform,
            Self::Stage { group, .. } | Self::Nest(group) => &group.transform,
            Self::Image(image) => &image.transform,
        }
    }

    /// The layer whose transform and keys the clip's Motion and Opacity take.
    pub(super) fn motion(self) -> MotionHost<'a> {
        MotionHost {
            id: self.motion_id(),
            transform: self.transform(),
        }
    }

    fn active_range(self) -> TimeRangeProperty {
        match self {
            Self::Video(video) => video.playback.input_range(),
            Self::EffectScope(scope) => scope.group().playback.input_range(),
            Self::Stage { group, .. } | Self::Nest(group) => group.playback.input_range(),
            Self::Image(image) => image.active_range,
        }
    }

    fn is_hidden(self) -> bool {
        match self {
            Self::Video(video) => video.is_hidden,
            Self::EffectScope(scope) => scope.group().is_hidden,
            Self::Stage { group, .. } | Self::Nest(group) => group.is_hidden,
            Self::Image(image) => image.is_hidden,
        }
    }

    fn blend_mode(self) -> BlendMode {
        match self {
            Self::Video(video) => video.blend_mode,
            Self::EffectScope(scope) => scope.group().blend_mode,
            Self::Stage { group, .. } | Self::Nest(group) => group.blend_mode,
            Self::Image(image) => image.blend_mode,
        }
    }
}

/// Display prefix for imported Linear Wipe guides. Script preparation and
/// export both select ownership by current geometry, masks and clocks.
pub(super) const LINEAR_WIPE_GUIDE_PREFIX: &str = "Premiere Linear Wipe guide ";

/// The layer properties whose keys move a layer's frame.
const FRAME_PROPERTIES: [PropType; 15] = [
    PropType::PositionX,
    PropType::PositionY,
    PropType::PositionZ,
    PropType::Rotation,
    PropType::Skew,
    PropType::SkewAxis,
    PropType::RotationX,
    PropType::RotationY,
    PropType::OrientationX,
    PropType::OrientationY,
    PropType::OrientationZ,
    PropType::ScaleX,
    PropType::ScaleY,
    PropType::AnchorPointX,
    PropType::AnchorPointY,
];

/// The authored animations of the properties of `layer`, including those
/// that export omits.
pub(super) fn layer_animations(
    dynamics: &AnimationGraph,
    layer: LayerId,
) -> impl Iterator<Item = (PropType, &AnimationGraphEntry)> {
    dynamics.entries().iter().filter_map(move |entry| {
        entry
            .target
            .as_property()
            .filter(|property| property.layer_id() == layer)
            .map(|property| (property.property_type(), entry))
    })
}

/// The authored keys of `layer` on the properties that `select` accepts: one
/// enabled keyframe track without dependencies each, or `None` when one of
/// their animations is not.
pub(super) fn layer_tracks(
    dynamics: &AnimationGraph,
    layer: LayerId,
    select: impl Fn(PropType) -> bool,
) -> Option<BTreeMap<PropType, &PropertyKeyframeTrack>> {
    let mut tracks = BTreeMap::new();
    for (property, entry) in layer_animations(dynamics, layer) {
        if !select(property) {
            continue;
        }
        let AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } = entry.animator.data()
        else {
            return None;
        };
        if !entry.dependencies.is_empty() || tracks.insert(property, track).is_some() {
            return None;
        }
    }
    Some(tracks)
}

/// The video layer of a stage group: the guide of its one mask, a rectangle
/// or a shape, or the source of its track matte, is one of its two children
/// and the other is a video. Any other group exports as a nest.
pub(super) fn stage_layers(group: &GroupLayer) -> Option<&Layer> {
    // The one child that the group's mask or track matte consumes.
    let consumed = match (group.masks.as_slice(), &group.track_matte) {
        ([mask], None) => mask.layer?,
        ([], Some(matte)) => matte.layer,
        _ => return None,
    };
    let [first, second] = group.layers.as_slice() else {
        return None;
    };
    let is_guide = |layer: &Layer| {
        (group.track_matte.is_some()
            || matches!(layer.data(), LayerData::Rect(_) | LayerData::Shape(_)))
            && layer.id() == consumed
    };
    let video = match (is_guide(first), is_guide(second)) {
        (true, false) => second,
        (false, true) => first,
        _ => return None,
    };
    match video.data() {
        LayerData::Video(_) => Some(video),
        LayerData::Media(media) if media.source.kind == MediaSourceKind::Video => Some(video),
        _ => None,
    }
}

/// Display label only; export admission is independent of this spelling.
pub(super) const STAGE_GROUP_NAME: &str = "Premiere stage ";

/// A single-picture affine composition that one clip can carry without an
/// extra rasterizing nest. Validate the current owners, controls and clocks
/// before capturing an ordinary group; a one-child shape alone is not enough.
pub(super) fn transform_stage_layers<'a>(
    group: &'a GroupLayer,
    dynamics: &AnimationGraph,
) -> Option<&'a Layer> {
    if !group.masks.is_empty() {
        return None;
    }
    let [layer] = group.layers.as_slice() else {
        return None;
    };
    let video = super::video_data(layer).ok()??;
    // A centred native Motion can spell the spatial identity with nonzero
    // anchor/position. An ordinary identity nest around a moved/keyed child
    // retains its raster boundary. Only current Transform-only skew/blur can
    // require this lowerer when the outer owner is neutral; ordinary Opacity
    // must not be moved into unmeasured Transform mixing.
    let neutral = |t: &Transform, id: LayerId, motion_blur: bool| {
        t.position.xy_array() == t.anchor_point
            && t.scale == [100.0; 2]
            && t.rotation == 0.0
            && t.skew == 0.0
            && t.rotation_x == 0.0
            && t.rotation_y == 0.0
            && t.orientation == [0.0; 3]
            && t.position.z().is_none_or(|z| z == 0.0)
            && t.opacity.value() == 100.0
            && !motion_blur
            && layer_animations(dynamics, id).next().is_none()
    };
    let transform_only = video.transform.skew != 0.0 || video.motion_blur;
    if neutral(&video.transform, video.id, video.motion_blur)
        || (neutral(&group.transform, group.id, group.motion_blur)
            && !transform_only
            && group.track_matte.is_none())
        || video.source.fit != fx_schema::MediaFit::Stretch
        || video.source.frame_rect.is_none()
        || video.source.input_transform.is_some()
        || video.source.time_remap.is_some()
    {
        return None;
    }
    // Invalid single-picture matte ownership is still a stage candidate, so
    // clip admission can omit just it instead of rasterizing an invalid nest.
    (group.track_matte.is_some() || unsupported_stage(group, &video, dynamics).is_none())
        .then_some(layer)
}

/// The video that a group exports as one clip of, when the group has a stage
/// shape: a mask stage ([`stage_layers`]) or a Transform stage
/// ([`transform_stage_layers`]).
pub(super) fn stage_video<'a>(
    group: &'a GroupLayer,
    dynamics: &AnimationGraph,
) -> Option<&'a Layer> {
    stage_layers(group).or_else(|| transform_stage_layers(group, dynamics))
}

/// Whether `group` has a static background: a fill, or a padding or corner
/// radius, which shape the fill. No clip, graphic or nest carries one.
pub(super) fn has_background(group: &GroupLayer) -> bool {
    [
        group.padding_top,
        group.padding_right,
        group.padding_bottom,
        group.padding_left,
        group.corner_radius_top_left,
        group.corner_radius_top_right,
        group.corner_radius_bottom_right,
        group.corner_radius_bottom_left,
    ]
    .iter()
    .any(|value| value.value() != 0.0)
        || !group.fills.is_empty()
}

/// Whether `property` keys one of the padding and corner radius fields of
/// [`has_background`].
pub(super) fn is_background_property(property: PropType) -> bool {
    matches!(
        property,
        PropType::PaddingTop
            | PropType::PaddingRight
            | PropType::PaddingBottom
            | PropType::PaddingLeft
            | PropType::CornerRadiusTopLeft
            | PropType::CornerRadiusTopRight
            | PropType::CornerRadiusBottomRight
            | PropType::CornerRadiusBottomLeft
    )
}

/// Why a stage group with this video does not export as one clip, if it does
/// not: every field of the group or its video that the clip cannot carry must
/// be neutral. `canonical_mask` checks the mask and its guide. Under a mask
/// stage the video is at the identity without keys or motion blur; under a
/// Transform stage (a group without masks or a track matte) the video's
/// transform and keys must export as one Transform effect
/// ([`effects::export_transform_stage`]), whose shutter carries the video's
/// motion blur (`export_video_clip`), and a video at the identity without
/// keys is no Transform stage.
fn unsupported_stage(
    group: &GroupLayer,
    video: &VideoLayer,
    dynamics: &AnimationGraph,
) -> Option<String> {
    let mask_stage = !group.masks.is_empty() || group.track_matte.is_some();
    // The clip takes the group's Motion and Opacity keys.
    let group_keys = layer_animations(dynamics, group.id).any(|(property, _)| {
        !matches!(
            property,
            PropType::Opacity
                | PropType::PositionX
                | PropType::PositionY
                | PropType::AnchorPointX
                | PropType::AnchorPointY
                | PropType::Rotation
                | PropType::ScaleX
                | PropType::ScaleY
        )
    });
    // Keys on the video's effects export with its effects.
    let video_keys = dynamics
        .entries()
        .iter()
        .any(|entry| entry.target.layer_id() == Some(video.id));
    [
        (!group.effects.is_empty(), "the group has effects"),
        (
            !super::timing::is_plain_group_playback(&group.playback),
            "the group has a nonidentity content clock",
        ),
        (has_background(group), "the group has a background"),
        (
            group_keys,
            "the group has keys other than Position, Anchor Point, Rotation, Scale and Opacity",
        ),
        (
            mask_stage && video.transform != identity_transform(),
            "the video's transform or opacity is not the identity",
        ),
        (mask_stage && video_keys, "the video has keys"),
        (
            video_keys && held_source(video).is_some(),
            "the video holds a frame and has keys",
        ),
        (video.is_hidden, "the video is hidden"),
        (
            video.blend_mode != BlendMode::Normal,
            "the video has a blend mode",
        ),
        (video.track_matte.is_some(), "the video has a track matte"),
        (!video.masks.is_empty(), "the video has masks"),
        (
            video.corner_radius.is_some(),
            "the video has a corner radius",
        ),
        (mask_stage && video.motion_blur, "the video has motion blur"),
        (video.start_time.is_some(), "the video has a start time"),
        (video.placement.is_some(), "the video has a placement"),
        (
            !stage_video_spans_group(group, video),
            "the video's range is not the group's",
        ),
        (
            video.parent != Some(group.id),
            "the video's parent is not the group",
        ),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then_some(reason.to_owned()))
    .or_else(|| {
        if mask_stage {
            return None;
        }
        // The same construction as the export's, on the video's clock and
        // without the shutter, which never rejects; the export builds the
        // record again from the video's key origin.
        let frame = source_frame(video.source.frame_rect).map_err(|error| error.to_string());
        frame
            .and_then(|frame| {
                effects::export_transform_stage(
                    video,
                    dynamics,
                    frame,
                    0,
                    None,
                    "",
                    &mut Vec::new(),
                )
            })
            .err()
    })
}

/// Whether the video of a stage group plays over exactly the group's range,
/// as one clip needs. A stage group that one clip cannot carry exports as a
/// nest only when it does: a nest of a video that ends early would outlast its
/// nested sequence, which rejects the whole export.
pub(super) fn stage_video_spans_group(group: &GroupLayer, video: &VideoLayer) -> bool {
    video.playback.input_range()
        == TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration)
}

/// A mask guide: the rectangle of a Crop or Linear Wipe, or the shape of an
/// Opacity mask.
#[derive(Clone, Copy)]
pub(super) enum Guide<'a> {
    Rect(&'a RectLayer),
    Shape(&'a ShapeLayer),
}

impl<'a> Guide<'a> {
    fn of(layer: &'a Layer) -> Option<Self> {
        match layer.data() {
            LayerData::Rect(rect) => Some(Self::Rect(rect)),
            LayerData::Shape(shape) => Some(Self::Shape(shape)),
            _ => None,
        }
    }

    fn id(self) -> LayerId {
        match self {
            Self::Rect(rect) => rect.id,
            Self::Shape(shape) => shape.id,
        }
    }

    fn transform(self) -> &'a Transform {
        match self {
            Self::Rect(rect) => &rect.transform,
            Self::Shape(shape) => &shape.transform,
        }
    }

    /// Whether keys on `property` move the guide's frame: every property but
    /// a shape guide's outline, whose keys are its mask's own
    /// ([`guide_outline_keys`]).
    fn frames(self, property: PropType) -> bool {
        matches!(self, Self::Rect(_)) || property != PropType::ShapePath
    }

    /// Why the guide cannot be one Premiere mask's outline beside the layer
    /// `noun`, whose `parent` and `range` it must share, if it cannot: every
    /// field but its geometry and transform must be neutral.
    pub(super) fn unsupported(
        self,
        noun: &str,
        parent: Option<LayerId>,
        range: TimeRangeProperty,
    ) -> Option<String> {
        let (
            is_hidden,
            guide_parent,
            active_range,
            blend_mode,
            track_matte,
            masks,
            effects,
            motion_blur,
        ) = match self {
            Self::Rect(l) => (
                l.is_hidden,
                l.parent,
                l.active_range,
                l.blend_mode,
                &l.track_matte,
                &l.masks,
                &l.effects,
                l.motion_blur,
            ),
            Self::Shape(l) => (
                l.is_hidden,
                l.parent,
                l.active_range,
                l.blend_mode,
                &l.track_matte,
                &l.masks,
                &l.effects,
                l.motion_blur,
            ),
        };
        [
            (is_hidden, "the guide is hidden".to_owned()),
            (
                guide_parent != parent,
                format!("the guide is not the {noun}'s sibling"),
            ),
            (
                active_range != range,
                format!("the guide's range differs from the {noun}'s"),
            ),
            (
                blend_mode != BlendMode::Normal,
                "the guide has a blend mode".to_owned(),
            ),
            (
                track_matte.is_some(),
                "the guide has a track matte".to_owned(),
            ),
            (!masks.is_empty(), "the guide has masks".to_owned()),
            (!effects.is_empty(), "the guide has effects".to_owned()),
            (motion_blur, "the guide has motion blur".to_owned()),
        ]
        .into_iter()
        .find_map(|(unsupported, reason)| unsupported.then_some(reason))
    }
}

/// What the mask or track matte of one clip exports as: nothing without
/// either, one Crop, one Linear Wipe, one Opacity mask or one Track Matte Key,
/// or the reason it cannot export. This is the one mask-export check of a
/// clip, a still's too, which keeps only a Crop or an Opacity mask
/// ([`image_mask`]). A clip whose mask it rejects is omitted whole, so no clip
/// exports without its mask. A video or image layer's guide is its sibling in
/// `layers`, in the parent's space; a stage or nest group's guide is its
/// direct child in the clip's frame. A track matte source is the sibling of a
/// video or nest group, or the direct child of a stage group
/// ([`canonical_track_matte`]). `frame` is the video's source frame, a still's
/// pixel frame, or a nest's canvas. The check reads every authored animation,
/// including those that export omits.
pub(super) fn canonical_mask(
    clip: ClipLayers<'_>,
    frame: [u32; 2],
    layers: &[Layer],
    dynamics: &AnimationGraph,
    width: u32,
    height: u32,
) -> std::result::Result<Option<CanonicalMask>, String> {
    let (masks, guides, track_matte) = match clip {
        ClipLayers::EffectScope(_) => return Ok(None),
        ClipLayers::Video(video) => (&video.masks, layers, &video.track_matte),
        ClipLayers::Stage { group, .. } | ClipLayers::Nest(group) => {
            (&group.masks, group.layers.as_slice(), &group.track_matte)
        }
        ClipLayers::Image(image) => (&image.masks, layers, &image.track_matte),
    };
    // The layer, named in the reasons, whose parent and range the guide
    // shares: the video or still, or a nest's group, over its whole range.
    let (noun, parent, range) = match clip {
        ClipLayers::EffectScope(_) => return Ok(None),
        ClipLayers::Video(video) | ClipLayers::Stage { video, .. } => {
            ("video", video.parent, video.playback.input_range())
        }
        ClipLayers::Image(image) => ("still", image.parent, image.active_range),
        ClipLayers::Nest(group) => (
            "group",
            Some(group.id),
            TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration),
        ),
    };
    if let Some(matte) = track_matte {
        if !masks.is_empty() {
            return Err("a track matte with masks on one clip is not converted; FX's intersection of the two is unverified against Premiere".to_owned());
        }
        return canonical_track_matte(clip, matte, layers, dynamics, frame, [width, height])
            .map(Some);
    }
    // Stills retain their existing static-mask profile; numeric animation
    // and expansion are admitted only on the established video/graphic hosts.
    if matches!(clip, ClipLayers::Image(_)) {
        if let [mask] = masks.as_slice() {
            if mask.expansion != 0.0 {
                return Err("the mask has an expansion".to_owned());
            }
            if dynamics
                .entries()
                .iter()
                .any(|entry| entry.target.fx_item_id() == Some(mask.id))
            {
                return Err("the mask has keys".to_owned());
            }
        }
    }
    // A pre-guide inline outline is already in the masked video's own source
    // coordinates. Route only the static, plain-video form through the same
    // validated native Opacity-mask record as a modern static shape guide.
    if let [mask] = masks.as_slice() {
        if let Some(path) = &mask.legacy_path {
            let ClipLayers::Video(video) = clip else {
                return Err(
                    "a legacy inline Opacity mask is supported only on a plain video".to_owned(),
                );
            };
            let bounded_identity_viewport = video.source.frame_rect.is_some_and(|rect| {
                let rect = rect.get();
                rect.x != 0.0 || rect.y != 0.0
            }) && matches!(video.source.fit,
                fx_schema::MediaFit::Custom { scale, content_center }
                    if scale.get() == [1.0, 1.0]
                        && content_center.get()
                            == [f64::from(width) / 2.0, f64::from(height) / 2.0])
                && frame == [width, height];
            if !bounded_identity_viewport {
                return Err("a legacy inline Opacity mask requires the bounded identity-Custom non-origin viewport route".to_owned());
            }
            if mask.layer.is_some() {
                return Err("a legacy inline Opacity mask also names a guide layer".to_owned());
            }
            if mask.mode != MaskMode::Add {
                return Err("the legacy inline mask mode is not Add".to_owned());
            }
            if mask.feather[0] != mask.feather[1] {
                return Err("the legacy inline mask feather differs between its axes".to_owned());
            }
            if super::mask_animation::has_tracks(dynamics, mask.id) {
                return Err("the legacy inline Opacity mask has numeric keys".to_owned());
            }
            let native = PrMask {
                raster: None,
                path: mask_outline(path, frame)?,
                path_keys: Vec::new(),
                feather: mask.feather[0],
                feather_keys: Vec::new(),
                expansion: mask.expansion,
                expansion_keys: Vec::new(),
                // The pre-guide inline representation stores this control in
                // native percent units, unlike the normalized modern form.
                opacity: mask.opacity.value(),
                opacity_keys: Vec::new(),
                inverted: mask.inverted,
            };
            native.validate().map_err(|error| error.to_string())?;
            return Ok(Some(CanonicalMask::Opacity {
                mask: native,
                mask_id: mask.id,
                guide_id: None,
            }));
        }
    }
    let Some((mask, guide)) = one_mask_guide(masks, guides, (noun, parent, range), dynamics)?
    else {
        return Ok(None);
    };
    // A flat guide has its video's (or still's) transform and the keys that move
    // its frame, and no other keys, so that the mask stays in the picture's
    // frame at every time; a staged or nested guide is in the clip's frame.
    // A shape guide's outline keys are its mask's own (`guide_outline_keys`).
    // Key ids differ. The guide's keys count from the video's visible start
    // on a clock that FX does not hold inside the clip, so the video's keys
    // must count from there too, and FX must not hold the video's clock
    // before its last key ([`VideoKeyClock`]). `label` names the guide in
    // the reason.
    let in_video_frame = |guide: Guide<'_>, label: &str| match clip {
        ClipLayers::Video(_) | ClipLayers::Image(_) => {
            let video_clock = match clip {
                ClipLayers::Video(video) => VideoKeyClock::of(video),
                _ => None,
            };
            let (same_keys, other_clock) = match (
                layer_tracks(dynamics, guide.id(), |property| {
                    matches!(clip, ClipLayers::Image(_)) || guide.frames(property)
                }),
                layer_tracks(dynamics, clip.motion_id(), |property| {
                    FRAME_PROPERTIES.contains(&property)
                }),
            ) {
                (Some(guide), Some(video)) => (
                    guide.len() == video.len()
                        && guide.iter().all(|(property, guide)| {
                            video
                                .get(property)
                                .is_some_and(|video| scale_tracks_match(guide, video))
                        }),
                    !video.is_empty()
                        && video_clock.is_some_and(|clock| {
                            !clock.counts_from_visible_start()
                                || video.values().any(|track| !clock.exact(track))
                        }),
                ),
                _ => (false, false),
            };
            if guide.transform() != clip.transform() || !same_keys {
                Err(format!(
                    "{label}'s transform or Motion keys differ from its {noun}'s"
                ))
            } else if other_clock {
                Err(format!(
                    "{label} repeats its video's Motion keys on another clock"
                ))
            } else {
                Ok(())
            }
        }
        ClipLayers::Stage { .. } | ClipLayers::Nest(_) | ClipLayers::EffectScope(_) => {
            if guide.transform() != &identity_transform() {
                let coverage = if matches!(guide, Guide::Rect(_))
                    && guide.transform().opacity.value() != 100.0
                {
                    format!("; Crop cannot encode the guide's {}% opacity without changing current masked coverage", guide.transform().opacity.value())
                } else {
                    String::new()
                };
                return Err(format!(
                    "{label} under the group is not at the identity{coverage}"
                ));
            }
            if layer_animations(dynamics, guide.id()).any(|(property, _)| guide.frames(property)) {
                return Err(format!("{label} under the group has keys"));
            }
            Ok(())
        }
    };
    let guide = match guide {
        Guide::Rect(guide) => guide,
        Guide::Shape(guide) => {
            // The video whose clip may write the guide's outline keys.
            let video = match clip {
                ClipLayers::EffectScope(_) => return Ok(None),
                ClipLayers::Nest(group) => {
                    if !group.effects.is_empty() {
                        return Err("nested Opacity mask requires effects below its owner: FX masks a Group before its effects, whereas Premiere masks their output".to_owned());
                    }
                    if layer_animations(dynamics, guide.id)
                        .any(|(property, _)| property == PropType::ShapePath)
                    {
                        return Err(
                            "nested Opacity mask requires a static guide outline".to_owned()
                        );
                    }
                    None
                }
                // A still keeps its static mask but never writes outline keys.
                ClipLayers::Image(_) => None,
                // A flat video's conflicting owner effects are omitted later,
                // at the shared effect/Opacity-mask ordering boundary. A stage
                // already puts its group mask after the child's effects.
                ClipLayers::Video(video) | ClipLayers::Stage { video, .. } => Some(video),
            };
            in_video_frame(Guide::Shape(guide), "the Opacity mask guide")?;
            let path_keys = match video {
                Some(video) => guide_outline_keys(guide, video, dynamics, frame)?,
                None => Vec::new(),
            };
            if super::mask_animation::has_tracks(dynamics, mask.id) {
                if let Some(reason) = video.and_then(mask_path_clock_reason) {
                    return Err(format!("numeric Opacity mask keys cannot export: {reason}"));
                }
            }
            let mut native = canonical_opacity_mask(mask, guide, path_keys, frame)?;
            super::mask_animation::export_tracks(&mut native, mask.id, dynamics, 0)?;
            return Ok(Some(CanonicalMask::Opacity {
                mask: native,
                mask_id: mask.id,
                guide_id: Some(guide.id),
            }));
        }
    };
    // A Crop or wipe hides nothing that its rectangle shows.
    let unsupported_rect_mask = [
        (mask.inverted, "the mask is inverted"),
        (mask.opacity.value() != 1.0, "the mask opacity is not 1"),
    ];
    if let Some(reason) = unsupported_rect_mask
        .into_iter()
        .find_map(|(unsupported, reason)| unsupported.then_some(reason))
    {
        return Err(reason.to_owned());
    }
    if let Some(track) = wipe_track(clip, guide, dynamics, width, height) {
        let wipe = canonical_linear_wipe(guide, track, mask.feather[0], width, height)?;
        // A staged or nested guide is the group's direct child.
        let in_clip_frame = matches!(clip, ClipLayers::Stage { .. } | ClipLayers::Nest(_));
        if !sequence_sized_layer_is_clip_frame(
            clip,
            frame,
            [width, height],
            in_clip_frame,
            dynamics,
        ) {
            return Err(format!("the {noun} does not map its frame onto the canvas unchanged, so its Linear Wipe guide is not its frame"));
        }
        return Ok(Some(CanonicalMask::LinearWipe(wipe)));
    }
    // FX rounds the mask outline by the guide's roundness; a Crop is square.
    if guide.rect.roundness != 0.0 {
        return Err("the Crop guide has rounded corners".to_owned());
    }
    // Unkeyed, a guide and video (or still) that only translate may differ in
    // anchor and position: FX offsets each by position minus anchor.
    let origin = match (in_video_frame(Guide::Rect(guide), "the Crop guide"), clip) {
        (Ok(()), _) => guide.rect.position,
        (Err(_), ClipLayers::Video(_) | ClipLayers::Image(_))
            if layer_animations(dynamics, guide.id).next().is_none()
                && only_translates(&guide.transform, guide.id, dynamics)
                && only_translates(clip.transform(), clip.motion_id(), dynamics) =>
        {
            let ([x, y], g, v) = (guide.rect.position, &guide.transform, clip.transform());
            [
                x + g.position.x() - g.anchor_point[0] - v.position.x() + v.anchor_point[0],
                y + g.position.y() - g.anchor_point[1] - v.position.y() + v.anchor_point[1],
            ]
        }
        (Err(reason), _) => return Err(reason),
    };
    let crop = guide_crop(guide, origin, frame, mask.feather[0])?;
    if matches!(clip, ClipLayers::Stage { .. })
        && origin == [0.0, 0.0]
        && guide.rect.size == frame.map(f64::from)
        && mask.feather == [0.0, 0.0]
    {
        // Zero Crop is elided by the writer, but a stage's effects can draw
        // beyond the source frame. Keep this authored boundary after them.
        return Ok(Some(CanonicalMask::Opacity {
            mask: PrMask {
                raster: None,
                path: PrShapePath {
                    vertices: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
                        .map(|point| PrPathVertex {
                            point,
                            in_tangent: point,
                            out_tangent: point,
                            smooth: false,
                        })
                        .to_vec(),
                    closed: true,
                },
                path_keys: Vec::new(),
                feather: 0.0,
                feather_keys: Vec::new(),
                expansion: 0.0,
                expansion_keys: Vec::new(),
                opacity: 100.0,
                opacity_keys: Vec::new(),
                inverted: false,
            },
            mask_id: mask.id,
            guide_id: Some(guide.id),
        }));
    }
    Ok(crop)
}

/// The one mask of `masks` and its guide among `guides`, or why no Premiere
/// clip carries them: one plain Add mask without an inline path or unequal
/// feather axes, over a rectangle or shape guide
/// beside the layer `noun` with its `parent` and `range`
/// ([`Guide::unsupported`]); `None` without masks.
fn one_mask_guide<'a>(
    masks: &'a [PathMask],
    guides: &'a [Layer],
    (noun, parent, range): (&str, Option<LayerId>, TimeRangeProperty),
    dynamics: &AnimationGraph,
) -> std::result::Result<Option<(&'a PathMask, Guide<'a>)>, String> {
    let mask = match masks {
        [] => return Ok(None),
        [mask] => mask,
        _ => return Err("several masks are not one Crop, Linear Wipe or Opacity mask".to_owned()),
    };
    let unsupported_mask = [
        (mask.mode != MaskMode::Add, "the mask mode is not Add"),
        (mask.legacy_path.is_some(), "the mask has an inline path"),
        (
            mask.feather[0] != mask.feather[1],
            "the mask feather differs between its axes",
        ),
    ];
    if let Some(reason) = unsupported_mask
        .into_iter()
        .find_map(|(unsupported, reason)| unsupported.then_some(reason))
    {
        return Err(reason.to_owned());
    }
    let guide = mask
        .layer
        .and_then(|guide_id| guides.iter().find(|layer| layer.id() == guide_id))
        .and_then(Guide::of)
        .ok_or_else(|| format!("the mask guide is not a rectangle or shape beside the {noun}"))?;
    if matches!(guide, Guide::Rect(_)) {
        if mask.expansion != 0.0 {
            return Err("the mask has an expansion".to_owned());
        }
        if super::mask_animation::has_tracks(dynamics, mask.id) {
            return Err("the mask has keys".to_owned());
        }
    }
    if let Some(reason) = guide.unsupported(noun, parent, range) {
        return Err(reason);
    }
    Ok(Some((mask, guide)))
}

/// Whether `layer` only translates its content: a 2D transform at Scale 100 and
/// Rotation 0, without skew or 3D rotation, and no key that moves its frame.
fn only_translates(transform: &Transform, layer: LayerId, dynamics: &AnimationGraph) -> bool {
    !transform.position.is_3d()
        && transform.scale == [100.0, 100.0]
        && transform.rotation == 0.0
        && transform.skew == 0.0
        && transform.skew_axis == 0.0
        && transform.rotation_x == 0.0
        && transform.rotation_y == 0.0
        && transform.orientation == [0.0; 3]
        && !layer_animations(dynamics, layer)
            .any(|(property, _)| FRAME_PROPERTIES.contains(&property))
}

/// Whether a sequence-sized layer bound to `clip` is `clip`'s source frame,
/// which Premiere's Linear Wipe and Track Matte Key both take as the
/// sequence canvas: the source `frame` is the `canvas`, and the layer is
/// `in_clip_frame` (a stage or nest group's direct child, which the group's
/// Motion moves with the clip) or a sibling in the parent's space that
/// `clip` maps onto the canvas unchanged: anchor at position, no 3D, Scale
/// 100, no rotation or skew and no Motion keys. One check for both masks,
/// so that neither exports a mask whose relationship to the frame Premiere
/// would change.
fn sequence_sized_layer_is_clip_frame(
    clip: ClipLayers<'_>,
    frame: [u32; 2],
    canvas: [u32; 2],
    in_clip_frame: bool,
    dynamics: &AnimationGraph,
) -> bool {
    let t = clip.transform();
    frame == canvas
        && (in_clip_frame
            || t.anchor_point == [t.position.x(), t.position.y()]
                && only_translates(t, clip.motion_id(), dynamics))
}

/// The Opacity mask that `mask` with the shape guide `guide`, in the clip's
/// `frame`, exports as: one closed contour with no path modifier or
/// primitive, in unit fractions of the frame, with the mask's feather,
/// opacity and inversion, and the guide's outline keys as `path_keys`
/// ([`guide_outline_keys`]), whose first outline is then the mask's. A
/// guide's paints are not drawn and not checked.
fn canonical_opacity_mask(
    mask: &PathMask,
    guide: &ShapeLayer,
    path_keys: Vec<PrMaskPathKey>,
    frame: [u32; 2],
) -> std::result::Result<PrMask, String> {
    let content = &guide.shape;
    if content.round_corners.is_some()
        || content.offset_paths.is_some()
        || content.trim.is_some()
        || content.poly_star.is_some()
        || content.ellipse.is_some()
    {
        return Err("the Opacity mask guide has a path modifier or primitive".to_owned());
    }
    let path = match path_keys.first() {
        Some(first) => first.path.clone(),
        None => mask_outline(&content.path, frame)?,
    };
    let mask = PrMask {
        raster: None,
        feather_keys: Vec::new(),
        expansion: mask.expansion,
        expansion_keys: Vec::new(),
        opacity_keys: Vec::new(),
        path,
        path_keys,
        feather: mask.feather[0],
        opacity: mask.opacity.value() * 100.0,
        inverted: mask.inverted,
    };
    mask.validate().map_err(|error| error.to_string())?;
    Ok(mask)
}

/// The Premiere outline of the Opacity mask guide path `path`, in unit
/// fractions of the clip's `frame`.
fn mask_outline(path: &ShapePath, frame: [u32; 2]) -> std::result::Result<PrShapePath, String> {
    let [width, height] = frame.map(f64::from);
    premiere_path(&scaled_path(path, [1.0 / width, 1.0 / height]))
        .map_err(|error| format!("the Opacity mask guide path cannot be exported: {error}"))
}

/// The Mask Path keys of the outline keys of the Opacity mask guide `guide`
/// in the clip's `frame`, none when its outline is static, or why export
/// cannot write them: only the clip of a flat or staged `video` writes them,
/// on the clock of [`mask_path_clock_reason`].
fn guide_outline_keys(
    guide: &ShapeLayer,
    video: &VideoLayer,
    dynamics: &AnimationGraph,
    frame: [u32; 2],
) -> std::result::Result<Vec<PrMaskPathKey>, String> {
    let tracks = layer_tracks(dynamics, guide.id, |property| {
        property == PropType::ShapePath
    })
    .ok_or("the Opacity mask guide's outline animation is not one keyframe track")?;
    let Some(track) = tracks.get(&PropType::ShapePath) else {
        return Ok(Vec::new());
    };
    if let Some(reason) = mask_path_clock_reason(video) {
        return Err(format!(
            "the Opacity mask guide's outline keys are not exported: {reason}"
        ));
    }
    mask_path_keys(track, frame)
}

/// Why the Mask Path keys of `video`'s clip cannot be written on Premiere's
/// source clock, if they cannot. The guide counts its keys from the clip
/// start and Premiere from the media start. A nonzero source In is added
/// when the occurrence is written. A changed authored input window, speed,
/// reverse, hold or remap can separate a flat guide's repeated Motion clock
/// from the video's own clock, which this mapping does not resolve.
fn mask_path_clock_reason(video: &VideoLayer) -> Option<&'static str> {
    if video.source.time_remap.is_some() {
        return Some("the video's source is time-remapped");
    }
    let playback = &video.playback;
    match playback.mapping() {
        fx_schema::LayerPlaybackMapping::TimeRemap { .. } => {
            Some("the video is retimed, reversed, held or time-remapped")
        }
        fx_schema::LayerPlaybackMapping::Linear { input, output }
            if input.duration != output.duration =>
        {
            Some("the video plays at another speed")
        }
        fx_schema::LayerPlaybackMapping::Linear { input, .. } => (*input != playback.input_range()
            || playback.input_offset_ms() != 0)
            .then_some("the video does not play over exactly its authored input range"),
    }
}

/// The Mask Path keys of the Opacity mask guide outline keys `track` in a
/// clip's `frame`, on the owner-local clock that [`mask_path_clock_reason`]
/// admits. `export_video_clip` adds its source origin. Premiere's keys store no
/// interpolation: Premiere draws the interval before each key after the
/// first by the outlines' vertex counts ([`mask_key_easing`]), so each such
/// key must have the easing that draws the same, as the keys that import
/// writes do. Unmeasured: the native reopen and render of written keys.
fn mask_path_keys(
    track: &PropertyKeyframeTrack,
    frame: [u32; 2],
) -> std::result::Result<Vec<PrMaskPathKey>, String> {
    let mut keys = Vec::with_capacity(track.keyframes().len());
    let mut paths = Vec::with_capacity(track.keyframes().len());
    for key in track.keyframes() {
        let PropertyValue::Path(path) = key.value() else {
            return Err("the Opacity mask guide's outline keys are not paths".to_owned());
        };
        keys.push(PrMaskPathKey {
            source_ticks: keyframes::source_ticks(0, key.layer_time().as_millis())
                .map_err(|error| error.to_string())?,
            path: mask_outline(path, frame)?,
        });
        paths.push(path);
    }
    for (index, (pair, paths)) in keys.windows(2).zip(paths.windows(2)).enumerate() {
        let native = mask_key_easing((&pair[0].path, paths[0]), (&pair[1].path, paths[1]))
            .map_err(|reason| {
                format!(
                    "the Opacity mask guide's outline keys at source ticks {} and {} {reason}",
                    pair[0].source_ticks, pair[1].source_ticks
                )
            })?;
        let key = &track.keyframes()[index + 1];
        if key.easing() != native {
            let (easing, drawn) = match native {
                PropertyKeyframeEasing::Hold => (
                    "Hold",
                    "holds the earlier outline, since their vertex counts differ",
                ),
                _ => (
                    "Linear",
                    "moves each vertex linearly, since their vertex counts match",
                ),
            };
            return Err(format!(
                "the Opacity mask guide's outline key at {} ms is not {easing}: Premiere's Mask Path keys store no interpolation, and between those outlines Premiere {drawn}",
                key.layer_time().as_millis()
            ));
        }
    }
    Ok(keys)
}

/// The clip Opacity mask of the graphic `group` among its `siblings`, the
/// list that holds it: the one mask of [`one_mask_guide`] over a shape guide
/// beside the group, at the identity and without keys, in the sequence
/// `frame` ([`canonical_opacity_mask`]); `None` without masks. Premiere
/// applies a graphic's clip Opacity mask after its Vector Motion, in the
/// sequence frame (fixture `feature_graphic_masks_d_26_5`, probe d1), as FX
/// applies a mask whose guide shares the group's parent; a guide inside the
/// group would move with the group's transform.
pub(super) fn graphic_opacity_mask(
    group: &GroupLayer,
    siblings: &[Layer],
    dynamics: &AnimationGraph,
    frame: [u32; 2],
    source_in: i64,
) -> std::result::Result<Option<PrMask>, String> {
    let mut native = graphic_mask(
        &group.masks,
        siblings,
        ("graphic", group.parent, group.playback.input_range()),
        dynamics,
        frame,
    )?;
    if let Some(mask) = &mut native {
        super::mask_animation::export_tracks(mask, group.masks[0].id, dynamics, source_in)?;
    }
    Ok(native)
}

/// A static graphic-frame mask over an identity sibling guide, after its owner's transform.
pub(super) fn graphic_mask(
    masks: &[PathMask],
    siblings: &[Layer],
    placement: (&str, Option<LayerId>, TimeRangeProperty),
    dynamics: &AnimationGraph,
    frame: [u32; 2],
) -> std::result::Result<Option<PrMask>, String> {
    let Some((mask, guide)) = one_mask_guide(masks, siblings, placement, dynamics)? else {
        return Ok(None);
    };
    let Guide::Shape(guide) = guide else {
        return Err("the graphic's mask guide is not a shape".to_owned());
    };
    if guide.transform != identity_transform() {
        return Err("the graphic's mask guide is not at the identity".to_owned());
    }
    if layer_animations(dynamics, guide.id).next().is_some() {
        return Err("the graphic's mask guide has keys".to_owned());
    }
    canonical_opacity_mask(mask, guide, Vec::new(), frame).map(Some)
}

/// The Track Matte Key that `matte` on `clip` exports as, or why the clip is
/// omitted. Its source is a video that itself exports as a clip
/// ([`clip_omitted`]), a still that exports with its coverage whole
/// ([`super::still::unsupported_matte_still`]; a matte still stays at its
/// defaults, without the effects that a still as content exports) or a
/// static shape that exports as the graphic of one Shape
/// ([`super::graphic::unsupported_matte_shape`]), the exporters whose
/// omission is decided before they run: beside a
/// video or nest group in
/// `layers` over the clip's range, or the direct child of a stage group over
/// the group's range, not hidden, with no track matte or masks of its own.
/// Premiere keys the clip's sequence-sized source frame by the matte track's
/// sequence-sized output, so the source must be that frame
/// ([`sequence_sized_layer_is_clip_frame`]): a sibling source of a moved or
/// keyed flat clip stays fixed in FX while Premiere's Motion would move the
/// keyed picture (fixture G5), and only a stage group carries that
/// relationship. A Color Matte or text source is not exported; a nest
/// source exports only in its bounded form ([`nested_matte_source_reason`]).
/// `alphaInverted` is Premiere's Reverse with Matte Alpha; `lumaInverted`
/// has no native form ([`PrMatteChannel`]). `frame` is the clip's source
/// frame and `canvas` the sequence size.
fn canonical_track_matte(
    clip: ClipLayers<'_>,
    matte: &TrackMatte,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    frame: [u32; 2],
    canvas: [u32; 2],
) -> std::result::Result<CanonicalMask, String> {
    // The list that holds the source and the parent and range it must have.
    let (siblings, parent, range) = match clip {
        ClipLayers::EffectScope(_) => return Err("effect scope cannot own a track matte".into()),
        ClipLayers::Video(video) => (layers, video.parent, video.playback.input_range()),
        ClipLayers::Image(image) => (layers, image.parent, image.active_range),
        ClipLayers::Nest(group) => (layers, group.parent, group.playback.input_range()),
        ClipLayers::Stage { group, .. } => (
            group.layers.as_slice(),
            Some(group.id),
            TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration),
        ),
    };
    // A flat video's or nest group's source is its sibling in the parent's
    // space; a stage group's source is its direct child.
    let in_clip_frame = matches!(clip, ClipLayers::Stage { .. });
    if !sequence_sized_layer_is_clip_frame(clip, frame, canvas, in_clip_frame, dynamics) {
        return Err("the clip does not map its sequence-sized frame onto the canvas unchanged, so its sibling track matte source is not the frame that Premiere keys; a moved or keyed clip needs a stage group with the source as its child".to_owned());
    }
    canonical_matte_source(
        matte,
        siblings,
        parent,
        range,
        dynamics,
        canvas,
        in_clip_frame,
    )
}

/// Resolve the current editable binding without changing the source's visibility.
/// Rect fills share this source contract with video and nested fills.
pub(super) fn canonical_matte_source(
    matte: &TrackMatte,
    siblings: &[Layer],
    parent: Option<LayerId>,
    range: TimeRangeProperty,
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
    staged: bool,
) -> std::result::Result<CanonicalMask, String> {
    let channel = match matte.mode {
        TrackMatteType::Alpha => PrMatteChannel::Alpha,
        TrackMatteType::AlphaInverted => PrMatteChannel::AlphaInverted,
        TrackMatteType::Luma => PrMatteChannel::Luma,
        TrackMatteType::LumaInverted => {
            return Err("the track matte mode is lumaInverted, which Premiere's Reverse with Matte Luma does not render: it gives the matte clip's zero-luma exterior full coverage where FX gives none".to_owned());
        }
    };
    let source = siblings
        .iter()
        .find(|layer| layer.id() == matte.layer)
        .ok_or_else(|| "the track matte source is not beside the clip".to_owned())?;
    let (is_hidden, source_parent, source_range, own_matte, own_masks) = match source.data() {
        LayerData::Video(video) => (
            video.is_hidden,
            video.parent,
            video.playback.input_range(),
            &video.track_matte,
            &video.masks,
        ),
        LayerData::Image(image) => (
            image.is_hidden,
            image.parent,
            image.active_range,
            &image.track_matte,
            &image.masks,
        ),
        LayerData::Group(group) => (
            group.is_hidden,
            group.parent,
            group.playback.input_range(),
            &group.track_matte,
            &group.masks,
        ),
        // A stage group's child source exports as a clone beside the group's
        // clip ([`export_staged_matte`]), which relocates video playback only.
        LayerData::Shape(_) if staged => {
            return Err("the track matte source is a Shape layer under a stage group; a shape source is exported beside a flat video or nest group only".to_owned());
        }
        LayerData::Shape(shape) => (
            shape.is_hidden,
            shape.parent,
            shape.active_range,
            &shape.track_matte,
            &shape.masks,
        ),
        _ => {
            return Err(format!(
            "the track matte source is a {} layer; only a video, still image, graphic shape or bounded nested source is exported",
            source.layer_type_name()
        ))
        }
    };
    let unsupported_source = [
        (is_hidden, "the track matte source is hidden"),
        (
            source_parent != parent,
            "the track matte source's parent is not the clip's",
        ),
        (
            source_range != range,
            "the track matte source's range differs from the clip's; only a source spanning exactly the clip's range converts",
        ),
        (
            own_matte.is_some() || !own_masks.is_empty(),
            "the track matte source has its own track matte or masks",
        ),
    ];
    if let Some(reason) = unsupported_source
        .into_iter()
        .find_map(|(unsupported, reason)| unsupported.then_some(reason))
    {
        return Err(reason.to_owned());
    }
    let unexported_source = match source.data() {
        // A stage child's playback keys are on the group clock; the clone
        // beside the group's clip reads the sequence clock.
        LayerData::Video(video)
            if staged && video.playback.time_remap().is_some() =>
        {
            Some("the stage group's track matte source plays at another speed or is time-remapped; its playback keys are on the group clock, and the source clip beside the group's clip would read the sequence clock".to_owned())
        }
        LayerData::Video(video) => {
            clip_omitted(ClipLayers::Video(video), siblings, dynamics, canvas)
                .then(|| "the track matte source is not exported".to_owned())
        }
        LayerData::Image(image) => {
            super::still::unsupported_matte_still(image, (canvas[0], canvas[1]), dynamics)
        }
        LayerData::Group(group) => nested_matte_source_reason(group, dynamics, canvas, staged),
        LayerData::Shape(shape) => super::graphic::unsupported_matte_shape(shape, dynamics),
        // Every other layer kind returned above.
        _ => None,
    };
    if let Some(reason) = unexported_source {
        return Err(reason);
    }
    Ok(CanonicalMask::TrackMatte {
        source: matte.layer,
        channel,
        sampled_end: matches!(source.data(), LayerData::Group(group)
            if super::timed_images::is_matte_source(group, dynamics, canvas)),
    })
}

/// The source frame and mask of a clip that export writes, or why export
/// omits the clip whole.
/// A nested matte retains one neutral physical video or bounded timed Images.
/// Keep arbitrary stacks, child geometry, remapping and effects outside admission.
fn nested_matte_source_reason(
    group: &GroupLayer,
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
    staged: bool,
) -> Option<String> {
    if super::timed_images::is_matte_source(group, dynamics, canvas) {
        return None;
    }
    if group
        .layers
        .iter()
        .any(|child| matches!(child.data(), LayerData::Image(_)))
    {
        return Some("the supplied matte requires disjoint, visible, neutral full-canvas Images ending at its Group window; only static Group Motion/Opacity and plain timing are supported".to_owned());
    }
    let reason = "the nested track matte source must be an unkeyed, neutral group of one full-frame video without effects, masks or sound";
    if staged
        || super::nested::unsupported_group_fields(group, dynamics).is_some()
        || group.transform.opacity.value() != 100.0
        || group.blend_mode != BlendMode::Normal
        || !group.effects.is_empty()
        || layer_animations(dynamics, group.id).next().is_some()
        || !sequence_sized_layer_is_clip_frame(
            ClipLayers::Nest(group),
            canvas,
            canvas,
            false,
            dynamics,
        )
    {
        return Some(reason.to_owned());
    }
    let [child] = group.layers.as_slice() else {
        return Some(reason.to_owned());
    };
    let Ok(Some(video)) = super::video_data(child) else {
        return Some(reason.to_owned());
    };
    if video.corner_radius.is_some_and(|radius| radius != 0.0) {
        return Some("the nested track matte source video has rounded corners".to_owned());
    }
    if video.placement.is_some() {
        return Some("the nested track matte source video has a placement override".to_owned());
    }
    let unit_rate = matches!(video.playback.mapping(),
        fx_schema::LayerPlaybackMapping::Linear { input, output } if input.duration == output.duration);
    if !unit_rate || video.source.time_remap.is_some() {
        return Some("the nested track matte source video is retimed".to_owned());
    }
    let range = TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration);
    if video.parent != Some(group.id)
        || video.is_hidden
        || video.playback.input_range() != range
        || video.blend_mode != BlendMode::Normal
        || video.transform.opacity.value() != 100.0
        || !video.effects.is_empty()
        || !video.masks.is_empty()
        || video.track_matte.is_some()
        || video.motion_blur
        || super::audio::audible(&video, None)
        || layer_animations(dynamics, video.id).next().is_some()
        || source_frame(video.source.frame_rect).ok() != Some(canvas)
        || !sequence_sized_layer_is_clip_frame(
            ClipLayers::Video(&video),
            canvas,
            canvas,
            false,
            dynamics,
        )
        || clip_omitted(ClipLayers::Video(&video), &group.layers, dynamics, canvas)
    {
        return Some(reason.to_owned());
    }
    None
}

type ClipMask = std::result::Result<([u32; 2], Option<CanonicalMask>), String>;

/// The source frame and mask of the clip that `clip` exports as, or why
/// export omits the clip whole: a stage group field (`unsupported_stage`) or
/// its mask (`canonical_mask`). Export and media inspection share this
/// decision. An error is a `sourceRect` that rejects the export.
fn clip_mask(
    clip: ClipLayers<'_>,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    [width, height]: [u32; 2],
) -> Result<ClipMask> {
    if let ClipLayers::Stage { group, video } = clip {
        if let Some(reason) = unsupported_stage(group, video, dynamics) {
            return Ok(Err(reason));
        }
    }
    let frame = match clip {
        ClipLayers::Video(video) => match source_frame(video.source.frame_rect) {
            Ok(frame) => frame,
            Err(error)
                if video.source.frame_rect.is_some_and(|frame| {
                    let frame = frame.get();
                    frame.x != 0.0 || frame.y != 0.0
                }) =>
            {
                if video.masks.is_empty() && video.track_matte.is_none() {
                    source_frame_dimensions(video.source.frame_rect)?
                } else if video.track_matte.is_none()
                    && matches!(video.masks.as_slice(), [mask]
                        if mask.legacy_path.is_some() && mask.layer.is_none())
                    && matches!(video.source.fit,
                        fx_schema::MediaFit::Custom { scale, content_center }
                            if scale.get() == [1.0, 1.0]
                                && content_center.get()
                                    == [f64::from(width) / 2.0, f64::from(height) / 2.0])
                {
                    // Legacy inline mask coordinates are already in this
                    // canvas-sized decoded source frame. Media inspection
                    // later proves the packaged frame has this exact size.
                    [width, height]
                } else {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        },
        ClipLayers::Stage { video, .. } => source_frame(video.source.frame_rect)?,
        ClipLayers::Nest(_) => [width, height],
        ClipLayers::EffectScope(_) => return Ok(Ok(([width, height], None))),
        // A masked still's pixel frame, which only its `sourceRect` names
        // before its media is inspected ([`image_mask`]).
        ClipLayers::Image(image) => {
            let ImageSource::Asset(source) = &image.source;
            match source_frame(source.frame_rect) {
                Ok(frame) => frame,
                Err(_) => return Ok(Err("the still's sourceRect is not its pixel frame at the origin, in which its mask draws".to_owned())),
            }
        }
    };
    Ok(canonical_mask(clip, frame, layers, dynamics, width, height).map(|mask| (frame, mask)))
}

/// Whether export omits `clip` whole, as [`clip_mask`] and
/// [`unexportable_clip`] decide; its media is then not inspected. `layers`
/// holds a video layer's siblings and `canvas` is the sequence size.
pub(super) fn clip_omitted(
    clip: ClipLayers<'_>,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> bool {
    unexportable_clip(clip, dynamics).is_some()
        || unsupported_ramp_dependencies(clip, layers).is_some()
        || matches!(clip_mask(clip, layers, dynamics, canvas), Ok(Err(_)))
}

/// A native ramp approximation cannot preserve an unverified mask clock or
/// expose a foreground when its alpha source or effect is unsupported.
/// Exact two-key playback keeps the existing effect, matte and mask rules.
fn unsupported_ramp_dependencies(clip: ClipLayers<'_>, layers: &[Layer]) -> Option<&'static str> {
    let video = match clip {
        ClipLayers::Video(video) | ClipLayers::Stage { video, .. } => video,
        // A nest's or still's clip has no media playback ramp.
        ClipLayers::Nest(_) | ClipLayers::Image(_) | ClipLayers::EffectScope(_) => return None,
    };
    let playback = video.playback.time_remap()?;
    if super::time_remap::native_ramp(video).is_none()
        && super::playback_segments::segmented_remap(video).is_none()
        && !native_playback_source_range(
            playback,
            video.playback.input_range(),
            video.source_range,
            video.playback.input_offset_ms(),
        )
        .is_ok_and(|playback| playback.approximated)
    {
        return None;
    }
    if video.effects.iter().any(|record| {
        let (enabled, payload) = match record.data() {
            EffectData::Identified {
                enabled, effect, ..
            } => (*enabled, effect),
            EffectData::Legacy(effect) => (true, effect),
        };
        enabled
            && matches!(
                payload,
                EffectPayload::Unknown(_)
                    | EffectPayload::Known(
                        LayerEffect::CustomShader { .. }
                            | LayerEffect::PersonMatte { .. }
                            | LayerEffect::DepthMatte { .. }
                            | LayerEffect::LumaKey { .. }
                            | LayerEffect::SimpleChoker { .. }
                            | LayerEffect::Unsupported(_)
                    )
            )
    }) {
        return Some("time remapping was not exported: native forward-ramp export cannot retain unsupported alpha-compositing effects");
    }
    fn consumed(layers: &[Layer], id: LayerId) -> bool {
        layers.iter().any(|layer| {
            let (matte, masks) = layer_consumers(layer);
            matte.as_ref().is_some_and(|matte| matte.layer == id)
                || masks.iter().any(|mask| mask.layer == Some(id))
                || layer
                    .child_layers()
                    .is_some_and(|children| consumed(children, id))
        })
    }
    (matches!(clip, ClipLayers::Stage { .. })
        || video.track_matte.is_some()
        || !video.masks.is_empty()
        || consumed(layers, video.id))
    .then_some("time remapping was not exported: native forward-ramp export requires an unmasked video outside mask and track-matte dependencies")
}

/// Why export omits a clip whose coverage clock or playback cannot use an
/// existing native carrier. Cosmetic Motion losses are diagnosed locally.
/// Native constant speed and reverse carry
/// bounded two-key linear playback, and an explicit Frame Hold two equal keys
/// over the window ([`held_source`]); forward linear ramps retain their selected
/// endpoint interval with an explicit approximation ([`native_playback_source_range`]).
/// Plateau-constrained nonlinear ramps keep native Speed keys ([`super::time_remap`]).
/// Other playback can retain a bounded authored source selection with a
/// diagnostic. Legacy `source.timeRemap` does not override current playback.
/// A stage group that this rejects
/// exports as a nest, whose placement or clip it rejects in turn.
pub(super) fn unexportable_clip(
    clip: ClipLayers<'_>,
    dynamics: &AnimationGraph,
) -> Option<&'static str> {
    let affine_reason = match clip {
        ClipLayers::Image(_) | ClipLayers::EffectScope(_) => None,
        ClipLayers::Video(video) => unsupported_affine_animation(video, None, dynamics),
        ClipLayers::Stage { group, video } => {
            unsupported_affine_animation(video, Some(group), dynamics)
        }
        // A rejected stage must not re-enter as a nest with the same unsafe keys.
        ClipLayers::Nest(group) => stage_layers(group).and_then(|layer| {
            super::video_data(layer)
                .ok()
                .flatten()
                .and_then(|video| unsupported_affine_animation(&video, Some(group), dynamics))
        }),
    };
    if affine_reason.is_some() {
        return affine_reason;
    }
    if let ClipLayers::Video(video) | ClipLayers::Stage { video, .. } = clip {
        video.playback.time_remap().and_then(|playback| {
            let unsupported = held_source(video).is_none()
                && super::time_remap::native_ramp(video).is_none()
                && super::playback_segments::segmented_remap(video).is_none()
                && native_playback_source_range(
                    playback,
                    video.playback.input_range(),
                    video.source_range,
                    video.playback.input_offset_ms(),
                )
                .is_err();
            unsupported.then_some(
                "time remapping was not exported: playback is outside the native speed, frame-hold and plateau-constrained ramp subsets",
            )
        })
    } else {
        None
    }
}

fn unsupported_affine_animation(
    video: &VideoLayer,
    group: Option<&GroupLayer>,
    dynamics: &AnimationGraph,
) -> Option<&'static str> {
    if !matches!(video.playback.mapping(), fx_schema::LayerPlaybackMapping::Linear { input, output }
        if input.duration != output.duration)
    {
        return None;
    }
    // Motion/effect property losses lower locally. Only coverage whose
    // provider needs the changed affine clock remains inseparable here.
    let keyed = dynamics.entries().iter().any(|entry| {
        let layer = entry.target.layer_id();
        let item = entry.target.fx_item_id();
        let coverage = |masks: &[PathMask]| {
            masks
                .iter()
                .any(|mask| item == Some(mask.id) || (mask.layer.is_some() && mask.layer == layer))
        };
        coverage(&video.masks)
            || video
                .track_matte
                .as_ref()
                .is_some_and(|matte| layer == Some(matte.layer))
            || group.is_some_and(|group| {
                coverage(&group.masks)
                    || group
                        .track_matte
                        .as_ref()
                        .is_some_and(|matte| layer == Some(matte.layer))
            })
    });
    keyed.then_some(
        "nonunit affine picture cannot carry its animated mask/matte provider on the existing native coverage clock",
    )
}

/// The Crop or Opacity mask that the one mask of the still `image` exports
/// as, with its guide beside the image in `layers`, or why export omits the
/// still whole: a still exports no Linear Wipe or Track Matte Key, and a Crop
/// or an Opacity mask only as [`clip_mask`] accepts it, so an image never
/// exports without its mask. `None` without a mask. Export, media inspection
/// and a nest's collapse check share this decision, as export and inspection
/// share [`clip_mask`] for a video clip; `canvas` is the sequence size.
pub(super) fn image_mask(
    image: &ImageLayer,
    layers: &[Layer],
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> std::result::Result<Option<CanonicalMask>, String> {
    const NO_WIPE_OR_MATTE: &str = "a still image exports no Linear Wipe or Track Matte Key";
    if image.track_matte.is_some() {
        return Err(NO_WIPE_OR_MATTE.to_owned());
    }
    if image.masks.is_empty() {
        return Ok(None);
    }
    let accepted = clip_mask(ClipLayers::Image(image), layers, dynamics, canvas)
        // No still's `sourceRect` rejects the export: a still without its
        // frame is omitted.
        .map_err(|error| error.to_string())?;
    let (_, mask) = accepted?;
    match mask {
        Some(CanonicalMask::Crop { .. } | CanonicalMask::Opacity { .. }) => Ok(mask),
        Some(CanonicalMask::LinearWipe(_) | CanonicalMask::TrackMatte { .. }) | None => {
            Err(NO_WIPE_OR_MATTE.to_owned())
        }
    }
}

/// The Crop that a guide rectangle at `origin` in the video's or still's frame
/// draws: its edges as percentages of `frame`.
fn guide_crop(
    guide: &RectLayer,
    [x, y]: [f64; 2],
    frame: [u32; 2],
    feather: f64,
) -> std::result::Result<Option<CanonicalMask>, String> {
    let [source_width, source_height] = frame.map(f64::from);
    let [crop_width, crop_height] = guide.rect.size;
    let crop = PrStaticCrop {
        left: x / source_width * 100.0,
        top: y / source_height * 100.0,
        right: (source_width - x - crop_width) / source_width * 100.0,
        bottom: (source_height - y - crop_height) / source_height * 100.0,
        edge_feather: feather,
    };
    crop.validate().map_err(|error| error.to_string())?;
    Ok(Some(CanonicalMask::Crop {
        crop,
        guide_id: guide.id,
    }))
}

/// Pre-bake ownership checks the current cardinal rectangle and its actual
/// mask consumer. It does not evaluate a script or retain imported origin;
/// the writer revalidates the fitted track before emitting Linear Wipe.
pub(super) fn scripted_wipe_axis(
    guide: &RectLayer,
    layers: &[Layer],
    group_owner: Option<&GroupLayer>,
    dynamics: &AnimationGraph,
    [width, height]: [u32; 2],
) -> Option<PropType> {
    let mut entries = layer_animations(dynamics, guide.id);
    let (property, entry) = entries.next()?;
    if entries.next().is_some()
        || !matches!(property, PropType::ScaleX | PropType::ScaleY)
        || !entry.dependencies.is_empty()
    {
        return None;
    }
    let accepts = |clip: ClipLayers<'_>,
                   masks: &[PathMask],
                   guides: &[Layer],
                   frame,
                   parent,
                   range,
                   in_clip_frame| {
        let Ok(Some((mask, Guide::Rect(current)))) =
            one_mask_guide(masks, guides, ("picture", parent, range), dynamics)
        else {
            return false;
        };
        current.id == guide.id
            && !mask.inverted
            && mask.opacity.value() == 1.0
            && linear_wipe_geometry(guide, property, mask.feather[0], width, height).is_ok()
            && sequence_sized_layer_is_clip_frame(
                clip,
                frame,
                [width, height],
                in_clip_frame,
                dynamics,
            )
    };
    if let Some(group) = group_owner {
        if group.track_matte.is_some() {
            return None;
        }
        let local = TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration);
        let accepted = if let Some(layer) = stage_layers(group) {
            let video = super::video_data(layer).ok()??;
            accepts(
                ClipLayers::Stage {
                    group,
                    video: &video,
                },
                &group.masks,
                layers,
                source_frame(video.source.frame_rect).ok()?,
                Some(group.id),
                local,
                true,
            )
        } else {
            accepts(
                ClipLayers::Nest(group),
                &group.masks,
                layers,
                [width, height],
                Some(group.id),
                local,
                true,
            )
        };
        return accepted.then_some(property);
    }
    layers
        .iter()
        .any(|layer| {
            let Ok(Some(video)) = super::video_data(layer) else {
                return false;
            };
            video.track_matte.is_none()
                && source_frame(video.source.frame_rect).is_ok_and(|frame| {
                    accepts(
                        ClipLayers::Video(&video),
                        &video.masks,
                        layers,
                        frame,
                        video.parent,
                        video.playback.input_range(),
                        false,
                    )
                })
        })
        .then_some(property)
}

/// The Scale track of a whole-canvas guide that its flat video does not
/// repeat: a repeated frame track is a Crop, not independent wipe completion.
fn wipe_track<'d>(
    clip: ClipLayers<'_>,
    guide: &RectLayer,
    dynamics: &'d AnimationGraph,
    width: u32,
    height: u32,
) -> Option<(PropType, &'d PropertyKeyframeTrack)> {
    if guide.rect != black_shape(width, height) {
        return None;
    }
    let tracks = layer_tracks(dynamics, guide.id, |_| true)?;
    let (&property, &track) = tracks.first_key_value().filter(|_| tracks.len() == 1)?;
    let video_track = match clip {
        ClipLayers::Video(_) | ClipLayers::Image(_) => {
            layer_tracks(dynamics, clip.motion_id(), |other| other == property)
                .and_then(|tracks| tracks.get(&property).copied())
                .is_some_and(|other| scale_tracks_match(track, other))
        }
        ClipLayers::Stage { .. } | ClipLayers::Nest(_) | ClipLayers::EffectScope(_) => false,
    };
    (matches!(property, PropType::ScaleX | PropType::ScaleY) && !video_track)
        .then_some((property, track))
}

/// The cardinal Linear Wipe that a sequence-sized guide with one Scale track
/// of wipe keys, and no other animation, draws, or why it draws none.
fn canonical_linear_wipe(
    guide: &RectLayer,
    (property_type, track): (PropType, &PropertyKeyframeTrack),
    feather: f64,
    width: u32,
    height: u32,
) -> std::result::Result<CanonicalLinearWipe, &'static str> {
    if !feather.is_finite() || feather < 0.0 {
        return Err("the Linear Wipe feather is negative or not finite");
    }
    if track.keyframes().is_empty()
        || !track.keyframes().iter().all(|key| {
            matches!(key.value(), PropertyValue::Float(value) if (0.0..=100.0).contains(value))
        })
    {
        return Err("the Linear Wipe keys are not visible fractions from 0 to 100");
    }
    linear_wipe_geometry(guide, property_type, feather, width, height)
}

/// Static cardinal geometry shared by key export and pre-bake script ownership.
fn linear_wipe_geometry(
    guide: &RectLayer,
    property_type: PropType,
    feather: f64,
    width: u32,
    height: u32,
) -> std::result::Result<CanonicalLinearWipe, &'static str> {
    if guide.rect != black_shape(width, height) || !feather.is_finite() || feather < 0.0 {
        return Err("the Linear Wipe guide is not a whole-canvas rectangle with finite feather");
    }
    let anchor = guide.transform.anchor_point;
    let angle_degrees = match property_type {
        PropType::ScaleX if anchor == [0.0, 0.0] => 270,
        PropType::ScaleX if anchor == [f64::from(width), 0.0] => 90,
        PropType::ScaleY if anchor == [0.0, 0.0] => 180,
        PropType::ScaleY if anchor == [0.0, f64::from(height)] => 0,
        _ => return Err("the Linear Wipe guide is not a cardinal wipe"),
    };
    // The wipe edge scales from the frame corner that the anchor names, so
    // the guide's position is that corner.
    let mut expected = identity_transform();
    expected.anchor_point = anchor;
    expected.position = fx_schema::Position::xy(anchor[0], anchor[1]);
    let initial_visible = match property_type {
        PropType::ScaleX => guide.transform.scale[0],
        _ => guide.transform.scale[1],
    };
    if !(0.0..=100.0).contains(&initial_visible) {
        return Err("the Linear Wipe guide's initial scale is outside 0 to 100");
    }
    expected.scale = match property_type {
        PropType::ScaleX => [initial_visible, 100.0],
        _ => [100.0, initial_visible],
    };
    if guide.transform != expected {
        return Err("the Linear Wipe guide is not a cardinal wipe");
    }
    Ok(CanonicalLinearWipe {
        guide_id: guide.id,
        property_type,
        angle_degrees,
        feather,
        initial_visible,
    })
}

/// The positive integer dimensions that a `sourceRect` names.
fn source_frame_dimensions(frame_rect: Option<PositiveRect>) -> Result<[u32; 2]> {
    let rect = frame_rect
        .ok_or_else(|| unsupported("sourceRect must cover the exact canvas"))?
        .get();
    let side = |value: f64, name: &str| {
        u32::try_from(value as u64)
            .ok()
            .filter(|side| f64::from(*side) == value && *side > 0)
            .ok_or_else(|| unsupported(format!("sourceRect {name} must be a positive integer")))
    };
    Ok([side(rect.width, "width")?, side(rect.height, "height")?])
}

/// The integer source frame that a video's or still's `sourceRect`,
/// `frame_rect`, names; it must start at the source origin.
pub(super) fn source_frame(frame_rect: Option<PositiveRect>) -> Result<[u32; 2]> {
    let rect = frame_rect
        .ok_or_else(|| unsupported("sourceRect must cover the exact canvas"))?
        .get();
    if rect.x != 0.0 || rect.y != 0.0 {
        return Err(unsupported("sourceRect must start at the source origin"));
    }
    source_frame_dimensions(frame_rect)
}

/// The animated FX properties whose keys an exported project writes.
///
/// Export records each one where it places the native owner of the keys, so
/// that script preparation can report a baked track that export discarded
/// instead of counting it as written. A written effect writes the keys of
/// every animated parameter it has.
#[derive(Debug, Default)]
pub(crate) struct WrittenAnimation {
    properties: BTreeSet<(LayerId, PropType)>,
    effects: BTreeSet<fx_schema::EffectId>,
    mask_properties: BTreeSet<(fx_schema::FxItemId, String)>,
}

impl WrittenAnimation {
    /// Commits child evidence only after its containing placement is retained.
    pub(super) fn append(&mut self, child: &mut Self) {
        self.properties.append(&mut child.properties);
        self.effects.append(&mut child.effects);
        self.mask_properties.append(&mut child.mask_properties);
    }

    /// Whether the exported project writes the keys of `target`.
    pub(crate) fn contains(&self, target: &PropertyTarget) -> bool {
        match target {
            PropertyTarget::LayerProperty(property) => self
                .properties
                .contains(&(property.layer_id(), property.property_type())),
            PropertyTarget::EffectProperty(target) => self.effects.contains(&target.effect_id()),
            PropertyTarget::FxItemProperty(target) => self
                .mask_properties
                .contains(&(target.item_id(), target.property_name().to_owned())),
        }
    }

    pub(super) fn record(&mut self, layer: LayerId, property: PropType) {
        self.properties.insert((layer, property));
    }

    pub(super) fn record_mask(&mut self, id: fx_schema::FxItemId, mask: &PrMask) {
        for (name, keys) in mask.numeric_keys() {
            if !keys.is_empty() {
                self.mask_properties.insert((id, name.to_owned()));
            }
        }
    }

    pub(super) fn record_effect(&mut self, effect: fx_schema::EffectId) {
        self.effects.insert(effect);
    }

    /// Records the FX tracks of `layer` that the native `animations` write:
    /// a Position, Anchor Point or uniform Scale key writes both axes, and a
    /// Scale Width key the X axis.
    pub(super) fn record_animations(&mut self, layer: LayerId, animations: &[PrPropertyAnimation]) {
        for animation in animations {
            let properties: &[PropType] = match animation {
                PrPropertyAnimation::Opacity(_) => &[PropType::Opacity],
                PrPropertyAnimation::Rotation(_) => &[PropType::Rotation],
                PrPropertyAnimation::Position(_) => &[PropType::PositionX, PropType::PositionY],
                PrPropertyAnimation::AnchorPoint(_) => {
                    &[PropType::AnchorPointX, PropType::AnchorPointY]
                }
                PrPropertyAnimation::UniformScale(_) => &[PropType::ScaleX, PropType::ScaleY],
                PrPropertyAnimation::ScaleWidth(_) => &[PropType::ScaleX],
            };
            for property in properties {
                self.record(layer, *property);
            }
        }
    }

    /// Records the keys that one placement of `owner` writes, and the guide
    /// tracks that its written `mask` represents: a Linear Wipe's completion
    /// keys, a Crop or Opacity mask guide's frame tracks, which
    /// [`canonical_mask`] proved equal to the frame tracks of the placement's
    /// Motion, and an Opacity mask guide's outline keys, which its mask's Mask
    /// Path keys write ([`guide_outline_keys`]).
    pub(super) fn record_placement(
        &mut self,
        owner: LayerId,
        animations: &[PrPropertyAnimation],
        mask: Option<CanonicalMask>,
        linear_wipe: Option<&PrLinearWipe>,
    ) {
        self.record_animations(owner, animations);
        if let Some(CanonicalMask::Opacity {
            mask,
            mask_id,
            guide_id,
        }) = &mask
        {
            self.record_mask(*mask_id, mask);
            if !mask.path_keys.is_empty() {
                if let Some(guide_id) = guide_id {
                    self.record(*guide_id, PropType::ShapePath);
                }
            }
        }
        match mask {
            Some(CanonicalMask::LinearWipe(wipe)) if linear_wipe.is_some() => {
                self.record(wipe.guide_id, wipe.property_type);
            }
            Some(CanonicalMask::Crop { guide_id, .. })
            | Some(CanonicalMask::Opacity {
                guide_id: Some(guide_id),
                ..
            }) => {
                for property in FRAME_PROPERTIES {
                    if self.properties.contains(&(owner, property)) {
                        self.record(guide_id, property);
                    }
                }
            }
            Some(CanonicalMask::Opacity { guide_id: None, .. }) => {}
            // A Linear Wipe that is not written has no completion keys, and a
            // track matte source is a clip of its own.
            Some(CanonicalMask::LinearWipe(_) | CanonicalMask::TrackMatte { .. }) | None => {}
        }
    }
}

/// One exported project and the animated FX properties whose keys it writes.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct ExportedProject {
    pub(crate) project: PrProjectFile,
    pub(crate) written: WrittenAnimation,
}

/// Native lowering and its animation evidence, even without a project.
pub(crate) struct LoweredDocument {
    pub(crate) project: Option<PrProjectFile>,
    pub(crate) written: WrittenAnimation,
    pub(crate) packing: PicturePackingRecipe,
}

/// [`export_document`] for checks that read only the project.
#[cfg(test)]
pub(crate) fn tesseract_to_premiere(
    document: &EditableFxCompositionDocument,
    media_facts: &BTreeMap<String, MediaFacts>,
    audio_facts: &BTreeMap<String, crate::audio_media::SourceSound>,
    fonts: &BTreeMap<String, FontAssetProperties>,
    frame_rate: FrameRate,
    omissions: &mut Vec<Omission>,
) -> Result<PrProjectFile> {
    export_document(
        document,
        media_facts,
        audio_facts,
        fonts,
        frame_rate,
        omissions,
    )
    .map(|exported| exported.project)
}

/// Lower a prepared document while preserving ordinary export's empty-content error.
#[cfg(test)]
pub(crate) fn export_document(
    document: &EditableFxCompositionDocument,
    media_facts: &BTreeMap<String, MediaFacts>,
    audio_facts: &BTreeMap<String, crate::audio_media::SourceSound>,
    fonts: &BTreeMap<String, FontAssetProperties>,
    frame_rate: FrameRate,
    omissions: &mut Vec<Omission>,
) -> Result<ExportedProject> {
    let lowered = lower_document(
        document,
        media_facts,
        audio_facts,
        fonts,
        frame_rate,
        omissions,
    )?;
    let project = lowered
        .project
        .ok_or_else(|| no_native_content(omissions))?;
    Ok(ExportedProject {
        project,
        written: lowered.written,
    })
}

pub(crate) fn no_native_content(omissions: &[Omission]) -> BuildError {
    unsupported(format!(
        "no convertible video or audio layers; no Premiere project published:\n{}",
        omissions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// Shared native lowering; inspection can retain losses when no native content survives.
#[cfg(test)]
pub(crate) fn lower_document(
    document: &EditableFxCompositionDocument,
    media_facts: &BTreeMap<String, MediaFacts>,
    audio_facts: &BTreeMap<String, crate::audio_media::SourceSound>,
    fonts: &BTreeMap<String, FontAssetProperties>,
    frame_rate: FrameRate,
    omissions: &mut dyn OmissionSink,
) -> Result<LoweredDocument> {
    lower_document_with_progress(
        document,
        &BTreeSet::new(),
        media_facts,
        audio_facts,
        fonts,
        frame_rate,
        omissions,
        fx_conv::Progress::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_document_with_progress(
    document: &EditableFxCompositionDocument,
    natural_frames: &BTreeSet<LayerId>,
    media_facts: &BTreeMap<String, MediaFacts>,
    audio_facts: &BTreeMap<String, crate::audio_media::SourceSound>,
    fonts: &BTreeMap<String, FontAssetProperties>,
    frame_rate: FrameRate,
    omissions: &mut dyn OmissionSink,
    progress: fx_conv::Progress<'_>,
) -> Result<LoweredDocument> {
    let composition = document.composition();
    for key in document.unknown_field_names() {
        omit(
            omissions,
            OmissionScope::Feature,
            "document",
            format!("unknown field {key} was not exported"),
        );
    }
    for (path, _) in composition.unknown_fields() {
        omit(
            omissions,
            OmissionScope::Feature,
            "composition",
            format!("unknown field {path} was not exported"),
        );
    }
    let mut property_tracks: BTreeMap<LayerId, BTreeMap<PropType, &PropertyKeyframeTrack>> =
        BTreeMap::new();
    let stroke_width_ids = stroke_width_ids(composition.layers());
    let keyed_text_layer_ids = keyed_text_layer_ids(composition.layers(), composition.dynamics());
    let anchor_point_layer_ids =
        anchor_point_layer_ids(composition.layers(), composition.dynamics());
    let outline_guide_ids = nested_mask_guide_ids(composition.layers());
    let dimensions = document.dimensions();
    // One native media record is shared by every occurrence of an asset. Use
    // the longest independently authored source extent before traversal so a
    // shorter declaration cannot overwrite or truncate that shared record.
    let mut authored_video_durations = BTreeMap::new();
    for layer in super::nested::exported_video_layers(
        composition.layers(),
        composition.dynamics(),
        [dimensions.width, dimensions.height],
    ) {
        let Some(video) = super::video_data(layer)? else {
            continue;
        };
        let asset_id = super::active_asset_id(&video.source).as_str().to_owned();
        let duration = video.source_intrinsic_duration.as_millis();
        authored_video_durations
            .entry(asset_id)
            .and_modify(|known: &mut u64| *known = (*known).max(duration))
            .or_insert(duration);
    }
    // `effects::export_effects` reports an animated parameter with its effect.
    let video_effect_ids = effects::video_effect_ids(
        composition,
        media_facts,
        [dimensions.width, dimensions.height],
    );
    let mask_ids = mask_item_ids(composition.layers());
    for entry in composition.dynamics().entries() {
        omissions.replace_context(ExportContext {
            source: ExportLossSource::Property(entry.target.clone()),
            domain: if matches!(&entry.target, PropertyTarget::LayerProperty(property)
                if property.property_type() == PropType::AudioVolume)
            {
                ExportLossDomain::Audio
            } else {
                ExportLossDomain::Unclassified
            },
        });
        if matches!(
            &entry.target,
            PropertyTarget::EffectProperty(target)
                if video_effect_ids.contains(&target.effect_id())
        ) {
            continue;
        }
        if entry
            .target
            .fx_item_id()
            .is_some_and(|id| mask_ids.contains(&id))
        {
            // The mask classifier consumes or rejects the entire owner.
            continue;
        }
        if matches!(&entry.target, PropertyTarget::FxItemProperty(target)
            if target.property_name() == super::graphic::TEXT_ANIMATOR_STROKE_WIDTH
                && stroke_width_ids.contains(&target.item_id()))
        {
            // The graphic exporter validates this animator's independent Hold
            // track and reconstructs each Source Text width from base + delta.
            continue;
        }
        let PropertyTarget::LayerProperty(property) = &entry.target else {
            omit(
                omissions,
                OmissionScope::Feature,
                "composition",
                "unsupported animation target was not exported",
            );
            continue;
        };
        // The Source Text fields of a text layer whose text is keyed export
        // as Source Text keys (`graphic::source_text_keys`); on a text layer
        // with static text, and on other layers, they have no mapping. Anchor
        // Point keys export only as a clip's Motion keys.
        // A keyed text's local Y anchor is admitted only to the graphic
        // exporter's coherent held point-alignment check. A mask guide's
        // outline keys export with its mask, or go with the clip that
        // cannot write them ([`guide_outline_keys`]).
        let exportable = matches!(
            property.property_type(),
            PropType::Opacity
                | PropType::PositionX
                | PropType::PositionY
                | PropType::Rotation
                | PropType::ScaleX
                | PropType::ScaleY
                | PropType::AudioVolume
        ) || matches!(
            property.property_type(),
            PropType::AnchorPointX | PropType::AnchorPointY
        ) && anchor_point_layer_ids.contains(&property.layer_id())
            || (super::graphic::source_text_field(property.property_type()).is_some()
                || property.property_type() == PropType::AnchorPointY)
                && keyed_text_layer_ids.contains(&property.layer_id())
            || property.property_type() == PropType::ShapePath
                && outline_guide_ids.contains(&property.layer_id());
        if !exportable || !entry.dependencies.is_empty() {
            omit(omissions, OmissionScope::Feature, format!("layer {}", property.layer_id()), "only independent Opacity, paired Position, Rotation, uniform Scale, audio volume and text Source Text keyframes can be exported; animation was omitted");
            continue;
        }
        let fx_schema::animator::AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } = entry.animator.data()
        else {
            omit(
                omissions,
                OmissionScope::Feature,
                format!("layer {}", property.layer_id()),
                "unsupported or disabled animation was not exported",
            );
            continue;
        };
        let tracks = property_tracks.entry(property.layer_id()).or_default();
        match tracks.entry(property.property_type()) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(track);
            }
            std::collections::btree_map::Entry::Occupied(_) => omit(
                omissions,
                OmissionScope::Feature,
                format!("layer {}", property.layer_id()),
                format!(
                    "duplicate {:?} animation was not exported",
                    property.property_type()
                ),
            ),
        }
    }
    omissions.replace_context(ExportContext {
        source: ExportLossSource::Document,
        domain: ExportLossDomain::Unclassified,
    });
    let sequence_name = composition.name();
    if sequence_name.is_empty() {
        return Err(unsupported("composition.name must be nonempty"));
    }
    let sequence_name = sequence_name.to_owned();
    if document
        .background_color()
        .is_some_and(|color| color != [0.0, 0.0, 0.0, 1.0])
    {
        omit(
            omissions,
            OmissionScope::Feature,
            "document",
            "background color was not exported (using black)",
        );
    }
    let (width, height) = (dimensions.width, dimensions.height);
    let document_end = Time::from_millis(document.duration().as_millis());
    // A rectangle that a mask references is a guide, never a matte or canvas.
    let mask_guide_ids = mask_guide_ids(composition.layers());
    let layers = composition.layers();
    // Guides do not paint, so the canvas is the bottommost other layer. Any
    // other bottommost rectangle converts with the other layers.
    let canvas = if let Some(LayerData::Rect(rect)) = layers
        .iter()
        .rev()
        .find(|layer| !mask_guide_ids.contains(&layer.id()))
        .map(Layer::data)
    {
        super::background::validate_black_canvas(rect, width, height)
            .is_ok()
            .then_some(rect.id)
    } else {
        None
    };
    let mut media: BTreeMap<MediaId, PrMedia> = BTreeMap::new();
    let mut audio = Vec::new();
    let mut written = WrittenAnimation::default();
    let mut packer = PicturePacker::new(&sequence_name, [width, height], frame_rate, 0);
    let root_container = packer.root();
    let mut motion_blur_written = false;
    let mut unmapped_group_motion_blur = Vec::new();
    let phase = progress.phase("lowering Premiere layers", "root layers", layers.len());
    let completed = Cell::new(0);
    let video_tracks = export_layers_with_progress(
        layers,
        None,
        &mut LayerExport {
            nest_scale: Some(1.0),
            dynamics: composition.dynamics(),
            property_tracks: &mut property_tracks,
            written: &mut written,
            media_facts,
            authored_video_durations: &authored_video_durations,
            natural_frames,
            audio_facts,
            fonts,
            audio: &mut audio,
            media: &mut media,
            packer: &mut packer,
            container: root_container,
            boundary: None,
            width,
            height,
            frame_rate,
            origin: Time::ZERO,
            sampled_picture_end: None,
            depth: 0,
            canvas,
            group_guide: None,
            in_moved_nest: false,
            motion_blur: composition.motion_blur(),
            motion_blur_written: &mut motion_blur_written,
            unmapped_group_motion_blur: &mut unmapped_group_motion_blur,
        },
        omissions,
        Some((phase, &completed)),
    )?;
    progress.stage("assemble Premiere document");
    for group in unmapped_group_motion_blur {
        omit_field(
            omissions,
            group.id,
            ExportField::MotionBlur,
            format!("layer {} ({:?})", group.id, group.name),
            "group motion blur was not exported; supported Group controls and child content retained without Group motion blur",
        );
    }
    for (layer_id, _) in property_tracks {
        omissions.replace_context(ExportContext {
            source: ExportLossSource::LayerSubtree(layer_id),
            domain: ExportLossDomain::Unclassified,
        });
        omit(
            omissions,
            OmissionScope::Feature,
            format!("layer {layer_id}"),
            "animation on an omitted or unsupported layer was not exported",
        );
    }
    omissions.replace_context(ExportContext {
        source: ExportLossSource::Document,
        domain: ExportLossDomain::Unclassified,
    });
    if composition.motion_blur() != Default::default() && !motion_blur_written {
        omit(
            omissions,
            OmissionScope::Feature,
            "composition",
            "motion blur was not exported",
        );
    }
    let has_native_content = !video_tracks.is_empty() || !audio.is_empty();
    let exact_document_end = ticks_from_time(document_end, "document.duration")?;
    let document_end_ticks =
        if audio.iter().map(|clip| clip.end_ticks).max() == Some(exact_document_end) {
            exact_document_end
        } else {
            frame_ticks_from_time(document_end, frame_rate, "document.duration")?
        };
    let mut sequence = PrSequence {
        native_frame_ticks: None,
        id: None,
        name: sequence_name,
        top_level: Some(true),
        video_tracks,
        audio,
        frame_rate,
        width,
        height,
        timeline_end_ticks: document_end_ticks,
    };
    // A trailing tail exports as the sequence work area and stays black in the
    // sequence. Occurrences are never extended or trimmed to the document end;
    // an end before the last occurrence exports that occurrence end instead.
    sequence.timeline_end_ticks = document_end_ticks.max(sequence.occurrence_end_ticks());
    packer.finish_container(
        root_container,
        &mut sequence.video_tracks,
        sequence.timeline_end_ticks,
    )?;
    let packing = packer.finish();
    if !has_native_content {
        return Ok(LoweredDocument {
            project: None,
            written,
            packing,
        });
    }
    sequence.validate_timeline(&media)?;
    // Premiere renders and this reader reimports only to the last occurrence.
    if sequence.occurrence_end_ticks() != document_end_ticks {
        omit(
            omissions,
            OmissionScope::Feature,
            "document.duration",
            format!(
                "duration differs from the last occurrence; exported duration is {} ticks",
                sequence.occurrence_end_ticks()
            ),
        );
    }
    Ok(LoweredDocument {
        project: Some(PrProjectFile::from_sequences(vec![sequence], media)),
        written,
        packing,
    })
}

/// Takes a sound's volume keys. Its other tracks stay, so that they are
/// reported as animation that was not exported; an entry left empty is
/// dropped, so that a sound with only volume keys reports nothing.
pub(super) fn take_volume_track<'d>(
    property_tracks: &mut BTreeMap<LayerId, BTreeMap<PropType, &'d PropertyKeyframeTrack>>,
    layer_id: LayerId,
) -> Option<&'d PropertyKeyframeTrack> {
    let tracks = property_tracks.get_mut(&layer_id)?;
    let volume = tracks.remove(&PropType::AudioVolume);
    if tracks.is_empty() {
        property_tracks.remove(&layer_id);
    }
    volume
}

/// Exports one layer list, bottom first, into video tracks of a sequence
/// whose time zero is `context.origin`. Layers must have `parent` as parent.
pub(super) fn export_layers<'d>(
    layers: &'d [Layer],
    parent: Option<LayerId>,
    context: &mut LayerExport<'_, 'd>,
    omissions: &mut dyn OmissionSink,
) -> Result<Vec<PrVideoTrack>> {
    let previous = omissions.context().cloned();
    let result = export_layers_inner(layers, parent, context, omissions, None);
    if let Some(previous) = previous {
        omissions.replace_context(previous);
    }
    result
}

fn export_layers_with_progress<'d>(
    layers: &'d [Layer],
    parent: Option<LayerId>,
    context: &mut LayerExport<'_, 'd>,
    omissions: &mut dyn OmissionSink,
    progress: Option<(fx_conv::ProgressPhase<'_>, &Cell<usize>)>,
) -> Result<Vec<PrVideoTrack>> {
    let previous = omissions.context().cloned();
    let result = export_layers_inner(layers, parent, context, omissions, progress);
    if let Some(previous) = previous {
        omissions.replace_context(previous);
    }
    result
}

fn export_layers_inner<'d>(
    layers: &'d [Layer],
    parent: Option<LayerId>,
    context: &mut LayerExport<'_, 'd>,
    omissions: &mut dyn OmissionSink,
    progress: Option<(fx_conv::ProgressPhase<'_>, &Cell<usize>)>,
) -> Result<Vec<PrVideoTrack>> {
    let (width, height) = (context.width, context.height);
    let mut video_tracks: Vec<PrVideoTrack> = Vec::new();
    let mask_guide_ids: BTreeSet<_> = mask_guide_ids(layers)
        .into_iter()
        .chain(context.group_guide)
        .collect();
    let sole_guides = sole_mask_guides(layers);
    // No group hides the root list, and a nested list exports into its nest's
    // sequence, whose placement carries the group's Enable: both count their
    // texts as shown unless the texts or groups inside the list hide them.
    let consumed = consumed_layer_ids(layers, false);
    let videos = layers
        .iter()
        .filter_map(|layer| super::video_data(layer).transpose())
        .map(|video| video.map(|video| (video.id, video)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    // The guides of every clip whose mask exported or omitted it, and of the
    // mask that the nest of this list exports.
    // Rect guides are already excluded from paint by `mask_guide_ids` above.
    // Register adjustment consumption up front too, independent of layer order.
    let mut mask_guides: BTreeSet<_> = context
        .group_guide
        .into_iter()
        .chain(layers.iter().filter_map(|layer| {
            match layer.data() {
                LayerData::Adjustment(adjustment)
                    if super::adjustment::unexported_reason(
                        adjustment,
                        layers,
                        context.dynamics,
                        [width, height],
                    )
                    .is_none() =>
                {
                    adjustment.masks.first().and_then(|mask| mask.layer)
                }
                _ => None,
            }
        }))
        .collect();
    // A track matte source exports after the clips that it keys, on a track
    // above every one of them ([`MatteConsumers`]); FX never draws it as
    // content of its own, so one that no exported clip keys is omitted.
    let matte_sources = track_matte_sources(layers);
    let mut consumers = MatteConsumers::default();
    let no_consumers = BTreeSet::new();
    let content = layers
        .iter()
        .rev()
        .filter(|layer| !matte_sources.contains(&layer.id()));
    let sources = layers
        .iter()
        .rev()
        .filter(|layer| matte_sources.contains(&layer.id()));
    for layer in content.chain(sources) {
        let _processed = Processed { progress };
        context.boundary = Some(
            context
                .packer
                .begin_boundary(context.container, layer.id())?,
        );
        omissions.replace_context(ExportContext {
            source: ExportLossSource::LayerSubtree(layer.id()),
            domain: if matches!(layer.data(), LayerData::Audio(_)) {
                ExportLossDomain::Audio
            } else {
                ExportLossDomain::Unclassified
            },
        });
        // The track that the layer's clip must clear: a source lies above
        // its consumers.
        let min_track = if matte_sources.contains(&layer.id()) {
            match consumers.min_track(layer.id()) {
                Some(min_track) => min_track,
                None => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        format!("layer {} ({:?})", layer.id(), layer.name()),
                        "track matte source of no exported clip was not exported; FX draws it only through the clips that it keys",
                    );
                    continue;
                }
            }
        } else {
            0
        };
        // A nest's sound plays on its sequence's audio tracks.
        if let LayerData::Audio(sound) = layer.data() {
            if let Some((occurrence, facts)) = super::audio::layer(
                sound,
                parent,
                take_volume_track(context.property_tracks, sound.id),
                context.audio_facts,
                omissions,
            )? {
                if occurrence.has_volume_animation() {
                    context.written.record(sound.id, PropType::AudioVolume);
                }
                source_media(context.media, &occurrence.media).audio = Some(facts.clone());
                context.audio.push(occurrence);
            }
            continue;
        }
        if let LayerData::Group(group) = layer.data() {
            if super::cross_dissolve::export_group(
                group,
                &mut video_tracks,
                &consumed,
                context,
                omissions,
            )? {
                continue;
            }
            if super::audio_groups::sound_only(group) {
                omissions.replace_context(ExportContext {
                    source: ExportLossSource::LayerSubtree(layer.id()),
                    domain: ExportLossDomain::Audio,
                });
                super::audio_groups::export(group, context, omissions)?;
                continue;
            }
        }
        // A group of text and shape layers is a graphic whose Vector Motion is
        // the group transform, and a stage group exports as one clip of its
        // video, below; other groups are nested sequences.
        let effect_scope = match layer.data() {
            LayerData::Group(group) => match super::effect_mask::EffectScope::recognize(
                group,
                context.dynamics,
                [width, height],
            ) {
                Ok(scope) => scope,
                Err(reason) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        format!("layer {}", group.id),
                        format!("effect mask scope was not exported: {reason}"),
                    );
                    continue;
                }
            },
            _ => None,
        };
        let stage = match layer.data() {
            LayerData::Group(group) => {
                // Recognized picture stages own their native boundary, even
                // with one identity or trimmed video child.
                if super::nested::is_nested_transform_stage(group)
                    || super::adjustment_geometry::is_stage(group)
                {
                    super::nested::export_group(
                        group,
                        layers,
                        &mut video_tracks,
                        context,
                        omissions,
                    )?;
                    continue;
                }
                if let Some(objects) = super::graphic::graphic_objects(group, context.dynamics) {
                    if let Some(graphic) = super::graphic::export_graphic_group(
                        group, &objects, layers, &consumed, context, omissions,
                    ) {
                        place_item(
                            &mut video_tracks,
                            PrVideoItem::Graphic(graphic),
                            min_track,
                            context,
                        )?;
                        super::nested::retain_group_motion_blur(group, context);
                    }
                    continue;
                }
                match effect_scope
                    .map(|scope| scope.video())
                    .or_else(|| stage_video(group, context.dynamics))
                    .map(super::video_data)
                    .transpose()?
                    .flatten()
                {
                    Some(video) => Some((group, video)),
                    None => {
                        let placed = super::nested::export_group(
                            group,
                            layers,
                            &mut video_tracks,
                            context,
                            omissions,
                        )?;
                        if let Some(track) = placed {
                            if let Some(source) = &group.track_matte {
                                consumers.add_nest(&video_tracks, source.layer, track);
                            }
                            let location = super::packing::PlacementLocation::Nest {
                                track,
                                position: video_tracks[track].nests.len() - 1,
                            };
                            consumers.bind_source(
                                &mut video_tracks,
                                group.id,
                                location,
                                context,
                            )?;
                        }
                        continue;
                    }
                }
            }
            _ => None,
        };
        if let LayerData::Image(image) = layer.data() {
            let record = format!("layer {} ({:?})", image.id, image.name);
            let mask = match image_mask(image, layers, context.dynamics, [width, height]) {
                Ok(mask) => mask,
                Err(reason) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        &record,
                        format!("masks cannot be exported: {reason}; occurrence omitted"),
                    );
                    mask_guides.extend(image.masks.iter().filter_map(|mask| mask.layer));
                    continue;
                }
            };
            mask_guides.extend(mask.as_ref().and_then(CanonicalMask::guide_id));
            let exported = super::still::export_image_layer(
                image,
                parent,
                mask.as_ref(),
                context,
                omissions,
                &record,
            )
            .map_err(|source| BuildError::Context {
                context: record,
                source: Box::new(source),
            })?;
            if let Some((occurrence, source)) = exported {
                context
                    .media
                    .entry(occurrence.media.clone())
                    .or_insert(source);
                let animations = occurrence.animations.clone();
                let track = super::cross_dissolve::place_picture(
                    &mut video_tracks,
                    PrVideoItem::Media(occurrence),
                    min_track,
                    image.id,
                    context,
                )?;
                // Only a placed still writes its keys, and the guide of its
                // mask the keys that it repeats.
                context
                    .written
                    .record_placement(image.id, &animations, mask, None);
                consumers.place_source(&mut video_tracks, image.id, track, context)?;
            }
            continue;
        }
        if let LayerData::Adjustment(adjustment) = layer.data() {
            let record = format!("layer {} ({:?})", adjustment.id, adjustment.name);
            let exported = super::adjustment::export_adjustment_layer(
                adjustment, layers, context, omissions, &record,
            )
            .map_err(|source| BuildError::Context {
                context: record,
                source: Box::new(source),
            })?;
            if let Some((occurrence, source)) = exported {
                context
                    .media
                    .entry(occurrence.media.clone())
                    .or_insert(source);
                place_item(
                    &mut video_tracks,
                    PrVideoItem::Media(occurrence),
                    min_track,
                    context,
                )?;
            }
            continue;
        }
        // A shape that one mask alone consumes is that mask's guide, which FX
        // never paints; a shape with another use is reported as a graphic
        // that another layer uses.
        if let LayerData::Shape(shape) = layer.data() {
            if sole_guides.contains(&shape.id)
                || (context.group_guide == Some(shape.id) && !matte_sources.contains(&shape.id))
            {
                continue;
            }
        }
        if let LayerData::Rect(rect) = layer.data() {
            if mask_guide_ids.contains(&rect.id) || context.canvas == Some(rect.id) {
                continue;
            }
            let record = format!("layer {} ({:?})", rect.id, rect.name);
            let exported = super::color_matte::export_rect_layer(
                rect, parent, &consumed, layers, context, omissions, &record,
            )
            .map_err(|source| BuildError::Context {
                context: record,
                source: Box::new(source),
            })?;
            if let Some(item) = exported {
                let track = super::cross_dissolve::place_picture(
                    &mut video_tracks,
                    item,
                    min_track,
                    rect.id,
                    context,
                )?;
                if let Some(matte) = &rect.track_matte {
                    consumers.add_item(&video_tracks, matte.layer, track);
                }
            }
            continue;
        }
        let record = format!("layer {} ({:?})", layer.id(), layer.name());
        let (clip, video) = match (super::graphic::ObjectLayer::of(layer), &stage) {
            (_, Some((group, video))) => (
                effect_scope.map_or(ClipLayers::Stage { group, video }, ClipLayers::EffectScope),
                &**video,
            ),
            // A text or shape layer of the list is a graphic of that object.
            // A shape that clips key ([`canonical_matte_source`]) exports
            // for them alone, above them, as a still source does: `consumed`
            // names it, which would refuse it as content of its own.
            (Some(object), None) => {
                let keys_clips = matte_sources.contains(&layer.id());
                let content_consumers = if keys_clips { &no_consumers } else { &consumed };
                if let Some(graphic) = super::graphic::export_object(
                    object,
                    parent,
                    content_consumers,
                    context,
                    omissions,
                    &record,
                ) {
                    let track = place_item(
                        &mut video_tracks,
                        PrVideoItem::Graphic(graphic),
                        min_track,
                        context,
                    )?;
                    if keys_clips {
                        consumers.place_source(&mut video_tracks, layer.id(), track, context)?;
                    }
                }
                continue;
            }
            // Both modern and historical video records use the same data view.
            // Other layers are reported after every guide is known.
            _ => match videos.get(&layer.id()) {
                Some(video) => (ClipLayers::Video(video), &**video),
                None => continue,
            },
        };
        // Root embedded sound is planned from the source layer independently
        // of whether picture-only masks/effects can emit a native occurrence.
        // Nested sound remains an explicit unsupported case in
        // `export_clip_sound`; stage-group sound keeps its existing group-clock
        // handling after the stage picture is admitted.
        let volume_track = context
            .property_tracks
            .get_mut(&video.id)
            .and_then(|tracks| tracks.remove(&PropType::AudioVolume));
        if matches!(clip, ClipLayers::Video(_)) {
            with_context(
                omissions,
                ExportContext {
                    source: ExportLossSource::Layer(video.id),
                    domain: ExportLossDomain::Audio,
                },
                |omissions| {
                    export_clip_sound(
                        clip,
                        video,
                        volume_track,
                        parent,
                        context,
                        &record,
                        omissions,
                    )
                },
            );
        }
        if let Some(MediaFacts::UnsupportedVideo(facts)) = context
            .media_facts
            .get(super::active_asset_id(&video.source).as_str())
        {
            omissions.emit_field(
                Omission {
                    scope: OmissionScope::Occurrence,
                    kind: crate::OmissionKind::Omitted,
                    record: record.clone(),
                    reason: format!("{}; native picture omitted, original media retained for editable AE fallback", facts.reason),
                },
                video.id,
                ExportField::PictureMedia,
            );
            mask_guides.extend(video.masks.iter().filter_map(|mask| mask.layer));
            continue;
        }
        let unexportable = unexportable_clip(clip, context.dynamics)
            .or_else(|| unsupported_ramp_dependencies(clip, layers));
        // Inspection skips a picture with unsupported playback or Motion.
        // An omitted natural frame therefore has no media dimensions to
        // resolve. Keep that omission before asking for its mask geometry;
        // a stage still takes the existing stage-to-nest path below.
        if video.source.frame_rect.is_none() && matches!(clip, ClipLayers::Video(_)) {
            if let Some(reason) = unexportable {
                omit(omissions, OmissionScope::Occurrence, &record, reason);
                mask_guides.extend(video.masks.iter().filter_map(|mask| mask.layer));
                continue;
            }
        }
        let accepted =
            clip_mask(clip, layers, context.dynamics, [width, height]).map_err(|source| {
                BuildError::Context {
                    context: record.clone(),
                    source: Box::new(source),
                }
            })?;
        // A stage group that one clip cannot carry exports as a nest, whose
        // placement carries the mask, and its video's keys stay for the nest;
        // a keyed stage group keeps its own reason instead.
        if let (true, Some(&(group, _))) = (
            effect_scope.is_none() && (accepted.is_err() || unexportable.is_some()),
            stage
                .as_ref()
                .filter(|(group, _)| stage_exports_as_nest(group)),
        ) {
            let placed =
                super::nested::export_group(group, layers, &mut video_tracks, context, omissions)?;
            if let (Some(track), Some(source)) = (placed, group.track_matte.as_ref()) {
                consumers.add_nest(&video_tracks, source.layer, track);
            }
            continue;
        }
        let ([source_width, source_height], mask) = match accepted {
            Ok(accepted) => accepted,
            // Any other stage group is omitted whole, so its video never
            // exports without its mask.
            Err(reason) if matches!(clip, ClipLayers::Stage { .. }) => {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    &record,
                    format!("stage group was not exported as one clip: {reason}"),
                );
                continue;
            }
            Err(reason) => {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    &record,
                    format!("masks cannot be exported: {reason}; occurrence omitted"),
                );
                mask_guides.extend(video.masks.iter().filter_map(|mask| mask.layer));
                continue;
            }
        };
        mask_guides.extend(mask.as_ref().and_then(CanonicalMask::guide_id));
        // A stage group with such a value went to the nest above: every stage
        // group that `clip_mask` accepts is a nest candidate.
        if let Some(reason) = unexportable {
            omit(omissions, OmissionScope::Occurrence, &record, reason);
            continue;
        }
        let matte_source = match &mask {
            Some(CanonicalMask::TrackMatte { source, .. }) => Some(*source),
            _ => None,
        };
        let Some((occurrence, source)) = export_video_clip(
            clip,
            video,
            parent,
            matte_sources.contains(&layer.id()),
            ([source_width, source_height], mask.clone()),
            context,
            &record,
            omissions,
        )
        .map_err(|source| BuildError::Context {
            context: format!("layer {} ({:?})", layer.id(), layer.name()),
            source: Box::new(source),
        })?
        else {
            continue;
        };
        let descriptor_conflict = {
            let slot = &mut source_media(context.media, &occurrence.media).video;
            match slot {
                Some(existing) if existing != &source => true,
                Some(_) => false,
                None => {
                    *slot = Some(source.clone());
                    false
                }
            }
        };
        if descriptor_conflict {
            omit_field(
                omissions,
                video.id,
                ExportField::PictureMedia,
                &record,
                "occurrence requires video metadata that conflicts with its shared native media descriptor; occurrence omitted",
            );
            continue;
        }
        let parts = retimed_video_items(
            video,
            &occurrence,
            &source,
            context.frame_rate,
            &record,
            omissions,
        )?;
        context.written.record_placement(
            clip.motion_id(),
            &occurrence.animations,
            mask,
            occurrence.linear_wipe.as_ref(),
        );
        if !matches!(clip, ClipLayers::Video(_)) {
            with_context(
                omissions,
                ExportContext {
                    source: ExportLossSource::Layer(video.id),
                    domain: ExportLossDomain::Audio,
                },
                |omissions| {
                    export_clip_sound(
                        clip,
                        video,
                        volume_track,
                        parent,
                        context,
                        &record,
                        omissions,
                    )
                },
            );
        }
        let item = PrVideoItem::Media(occurrence);
        // Reserve one track for the entire authored placement. Every segment
        // must stay on that track, including when this is a matte source.
        let min_track = if parts.is_some() {
            item_track(&video_tracks, &item, min_track)
        } else {
            min_track
        };
        let mut picture_retained = false;
        for item in parts.unwrap_or_else(|| vec![item]) {
            let track = super::cross_dissolve::place_picture(
                &mut video_tracks,
                item,
                min_track,
                clip.motion_id(),
                context,
            )?;
            consumers.place_source(&mut video_tracks, layer.id(), track, context)?;
            picture_retained = true;
            if let Some(matte_source) = matte_source {
                consumers.add_item(&video_tracks, matte_source, track);
            }
        }
        if let Some((group, _)) = stage.as_ref().filter(|_| picture_retained) {
            super::nested::retain_group_motion_blur(group, context);
        }
        // A stage group's source is its child; it exports once beside all of
        // the group's segments, over the group's whole range, above them.
        if let (Some(matte_source), Some((group, _))) = (matte_source, &stage) {
            export_staged_matte(
                group,
                matte_source,
                parent,
                &mut video_tracks,
                &mut consumers,
                context,
                omissions,
            )?;
        }
    }
    for layer in layers.iter().rev() {
        omissions.replace_context(ExportContext {
            source: ExportLossSource::LayerSubtree(layer.id()),
            domain: ExportLossDomain::Unclassified,
        });
        let handled_layer = match layer.data() {
            LayerData::Audio(_)
            | LayerData::Image(_)
            | LayerData::Text(_)
            | LayerData::Shape(_)
            | LayerData::Group(_)
            | LayerData::Adjustment(_) => true,
            LayerData::Rect(_) => !mask_guide_ids.contains(&layer.id()),
            _ => false,
        };
        if !videos.contains_key(&layer.id()) && !handled_layer && !mask_guides.contains(&layer.id())
        {
            omit(
                omissions,
                OmissionScope::Occurrence,
                format!("layer {} ({:?})", layer.id(), layer.name()),
                "unsupported layer type was not exported",
            );
        }
    }
    // A guide's keys belong to the mask that consumed it.
    for guide_id in mask_guides {
        context.property_tracks.remove(&guide_id);
    }
    Ok(video_tracks)
}

/// The native clip that `clip` exports as, with the facts of its source
/// media. `video` is the clip's video; its source is `source_width` by
/// `source_height` pixels and [`clip_mask`] accepted its `mask`. `parent` is
/// the group whose nest holds the clip, and `matte_source` whether the clip
/// is the track matte source of the clips that it keys. What the clip cannot
/// carry is reported.
#[allow(clippy::too_many_arguments)]
pub(super) fn export_video_clip(
    clip: ClipLayers<'_>,
    video: &VideoLayer,
    parent: Option<LayerId>,
    matte_source: bool,
    ([source_width, source_height], mask): ([u32; 2], Option<CanonicalMask>),
    context: &mut LayerExport<'_, '_>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<(PrVideoOccurrence, PrVideoStream)>> {
    use fx_schema::MediaFit;
    let (width, height, media_facts) = (context.width, context.height, context.media_facts);
    // A stage group is the parent of its video, whose mask fields are
    // neutral; the group's description is reported with the video's.
    let (group, masks_exported, group_description) = match clip {
        ClipLayers::Video(_) | ClipLayers::Image(_) => (parent, mask.is_some(), false),
        ClipLayers::EffectScope(scope) => (
            Some(scope.group().id),
            false,
            !scope.group().description.is_empty(),
        ),
        ClipLayers::Stage { group, .. } | ClipLayers::Nest(group) => {
            (Some(group.id), false, !group.description.is_empty())
        }
    };
    // A Transform stage's video exports its motion blur as the Transform's
    // own shutter at the composition's angle when the composition blurs
    // within the Shutter Angle bounds;
    // otherwise the motion blur is reported.
    let transform_stage = matches!(clip, ClipLayers::Stage { group, .. } if group.masks.is_empty() && group.track_matte.is_none());
    let blur = context.motion_blur;
    let shutter_angle = (transform_stage && video.motion_blur && blur.enabled)
        .then_some(blur.shutter_angle.value())
        .filter(|angle| {
            *angle > 0.0
                && TRANSFORM_SHUTTER_ANGLE
                    .value_range()
                    .is_some_and(|bounds| bounds.contains(angle))
        });
    for (changed, field) in unexported_layer_fields(ClipLayer::Video(video), group, masks_exported)
    {
        if changed && !(field == ExportField::MotionBlur && shutter_angle.is_some()) {
            omit_field(
                omissions,
                video.id,
                field,
                record,
                format!("{field} was not exported"),
            );
        }
    }
    if let Some(group_id) = group.filter(|_| group_description) {
        omit_field(
            omissions,
            group_id,
            ExportField::Description,
            record,
            "description was not exported",
        );
    }
    let t = clip.transform();
    // A track matte source writes Normal and reports its blend mode: how
    // Premiere's Track Matte Key reads a blended matte clip is unmeasured.
    let (blend_mode, blend_approximation) = match clip.blend_mode() {
        mode if !matte_source => (
            PrBlendMode::from_fx_mode(mode),
            PrBlendMode::export_approximation(mode),
        ),
        BlendMode::Normal => (PrBlendMode::Normal, None),
        _ => {
            omit_field(
                omissions,
                clip.motion_id(),
                ExportField::BlendMode,
                record,
                "unsupported blend mode was not exported (using normal)",
            );
            (PrBlendMode::Normal, None)
        }
    };
    let source = &video.source;
    for (changed, detail) in [
        (source.time_remap.is_some(), ExportField::TimeRemap),
        (
            source.input_transform.is_some(),
            ExportField::InputTransform,
        ),
    ] {
        if changed {
            omit_field(
                omissions,
                video.id,
                detail,
                record,
                format!("{detail} was not exported"),
            );
        }
    }
    if let Some(enhancement) = &source.audio_enhancement {
        let output = enhancement.enhanced_asset_id.as_str();
        let reason = if enhancement.enabled {
            format!("embedded audio selects active enhancement output {output:?} independently of the picture; the enhancement toggle and original sound lineage were not exported")
        } else {
            format!("inactive audio enhancement output {output:?} and the enhancement toggle were not exported")
        };
        omit_field(
            omissions,
            video.id,
            ExportField::AudioEnhancement,
            record,
            reason,
        );
    }
    let (mut crop, mut opacity_mask) = crop_and_opacity_mask(mask.as_ref(), record, omissions);
    let track_matte = unplaced_track_matte(mask.as_ref());
    let asset_id = super::replacement::active_video_asset(source, record, omissions)
        .as_str()
        .to_owned();
    if asset_id.is_empty() {
        return Err(unsupported("source.assetId must be nonempty"));
    }
    let MediaFacts::Video(facts) = media_facts
        .get(&asset_id)
        .ok_or_else(|| unsupported(format!("missing inspected media for asset {asset_id:?}")))?
    else {
        return Err(unsupported(format!(
            "asset {asset_id:?} is used with conflicting media kinds across layers"
        )));
    };
    let (source_frame_rate, physical_duration_ticks) = facts.timing.source_clock()?;
    let (source_duration_ticks, occurrence_duration_ticks) = if facts
        .timing
        .authored_source_duration
    {
        let authored_millis = video.source_intrinsic_duration.as_millis();
        let shared_millis = context
            .authored_video_durations
            .get(&asset_id)
            .copied()
            .unwrap_or(authored_millis);
        if shared_millis != authored_millis {
            with_context(
                omissions,
                ExportContext {
                    source: ExportLossSource::Layer(video.id),
                    domain: ExportLossDomain::Metadata,
                },
                |omissions| {
                    approximate(
                            omissions,
                            record,
                            format!(
                                "sourceIntrinsicDuration {authored_millis} ms conflicts with another occurrence of shared asset {asset_id:?}; using its longest authored extent {shared_millis} ms for the one native media descriptor"
                            ),
                        )
                },
            );
        }
        let shared_duration = ticks_from_time(
            Time::from_millis(shared_millis),
            "shared authored sourceIntrinsicDuration",
        )?;
        let occurrence_duration = ticks_from_time(
            Time::from_millis(authored_millis),
            "occurrence sourceIntrinsicDuration",
        )?;
        ensure!(
            shared_duration > 0 && occurrence_duration > 0,
            "authored sourceIntrinsicDuration must be positive"
        );
        approximate(
                omissions,
                format!("asset {asset_id:?}"),
                "original media presentation edits retained unchanged; physical packets establish the native source rate while sourceIntrinsicDuration supplies its authored presentation duration",
            );
        (shared_duration, occurrence_duration)
    } else {
        (physical_duration_ticks, physical_duration_ticks)
    };
    if occurrence_duration_ticks != source_duration_ticks
        && (source.time_remap.is_some()
            || !matches!(video.playback.mapping(),
                fx_schema::LayerPlaybackMapping::Linear { input, output }
                    if input.duration == output.duration))
    {
        omit_field(
            omissions,
            video.id,
            ExportField::PictureMedia,
            record,
            "occurrence has a conflicting sourceIntrinsicDuration and reverse or retimed playback whose end-relative bounds cannot use the shared native media descriptor; occurrence omitted",
        );
        return Ok(None);
    }
    // Import admits a quarter-turn source; export writes no orientation, so
    // the native stream it writes stays Identity.
    ensure!(
        facts.orientation == crate::schema::VideoOrientation::Identity,
        "rotated source video cannot be exported"
    );
    // Import's one warning per file, re-emitted once per packaged asset.
    if let Some(colour) = facts.colour {
        approximate(
            omissions,
            format!("asset {asset_id:?}"),
            colour.passthrough_warning(),
        );
    }
    let identity_custom_fit = matches!(source.fit,
        MediaFit::Custom { scale, content_center }
            if scale.get() == [1.0, 1.0]
                && content_center.get() == [f64::from(width) / 2.0, f64::from(height) / 2.0]
                && (facts.width, facts.height) == (width, height));
    if !matches!(
        source.fit,
        MediaFit::Contain | MediaFit::None | MediaFit::Stretch
    ) && !identity_custom_fit
    {
        omit_field(
            omissions,
            video.id,
            ExportField::MediaFit,
            record,
            "MediaFit was not exported",
        );
    }
    let source_rect = source.frame_rect.map(|frame| frame.get());
    let non_origin_source_rect = source_rect.filter(|rect| rect.x != 0.0 || rect.y != 0.0);
    let (source_width, source_height) = if let Some(rect) = non_origin_source_rect {
        // Crop and the existing Opacity mask are independent native controls.
        // Other masks or matte-source use retain their established rejection.
        ensure!(
            !matte_source && matches!(mask.as_ref(), None | Some(CanonicalMask::Opacity { .. })),
            "non-origin sourceRect cannot be used by this mask or track matte"
        );
        let contained = rect.x >= 0.0
            && rect.y >= 0.0
            && rect.x + rect.width <= f64::from(facts.width)
            && rect.y + rect.height <= f64::from(facts.height);
        if identity_custom_fit && contained {
            let source_crop = PrStaticCrop {
                left: rect.x / f64::from(facts.width) * 100.0,
                top: rect.y / f64::from(facts.height) * 100.0,
                right: (f64::from(facts.width) - rect.x - rect.width) / f64::from(facts.width)
                    * 100.0,
                bottom: (f64::from(facts.height) - rect.y - rect.height) / f64::from(facts.height)
                    * 100.0,
                edge_feather: 0.0,
            };
            source_crop.validate()?;
            debug_assert!(crop.is_default());
            crop = source_crop;
        } else {
            approximate(
                omissions,
                record,
                format!(
                    "non-origin sourceRect ({}, {}, {}x{}) was not exported; using the packaged {}x{} frame",
                    rect.x, rect.y, rect.width, rect.height, facts.width, facts.height
                ),
            );
        }
        (facts.width, facts.height)
    } else {
        ensure!(
            (facts.width, facts.height) == (source_width, source_height),
            "packaged source must be supported video matching sourceRect dimensions"
        );
        (source_width, source_height)
    };
    let mut native_transform = export_transform(
        clip.motion(),
        [source_width, source_height],
        [width, height],
        context.dynamics,
        record,
        omissions,
    );
    let natural = context.natural_frames.contains(&video.id);
    let fitting = super::video_fitting::VideoFitting::new(facts.pixel_aspect, source.fit, natural);
    if fitting.is_some() {
        approximate(omissions, record,
            "display-pixel fitting folded into editable native scale and anchor; source-space effect kernels and mask geometry may differ");
    } else if !natural
        && source.fit == MediaFit::None
        && !facts
            .pixel_aspect
            .agrees(crate::schema::records::PixelAspectRatio::SQUARE)
    {
        omit_field(
            omissions,
            video.id,
            ExportField::MediaFit,
            record,
            "unscaled non-square media fit was not exported (using coded-frame Stretch)",
        );
    }
    let clip_ticks = clip_ticks(clip, video, source_duration_ticks, mask.as_ref(), context)?;
    if occurrence_duration_ticks != source_duration_ticks
        && (clip_ticks.in_ticks < 0
            || clip_ticks.out_ticks > occurrence_duration_ticks
            || clip_ticks.out_ticks > source_duration_ticks)
    {
        omit_field(
            omissions,
            video.id,
            ExportField::PictureMedia,
            record,
            "forward occurrence selects beyond its own sourceIntrinsicDuration or the shared native media descriptor; occurrence omitted",
        );
        return Ok(None);
    }
    let ClipTicks {
        start_ticks: active_start_ticks,
        end_ticks: active_end_ticks,
        source_start_ticks,
        in_ticks,
        out_ticks,
        playback_rate,
        retiming,
    } = clip_ticks;
    if let crate::media::SampleClock::Irregular { media_end } = facts.timing.clock {
        let delta = i128::from(source_duration_ticks) * i128::from(facts.timing.timescale)
            - i128::from(crate::schema::TICKS) * i128::from(media_end);
        approximate(omissions, format!("asset {asset_id:?}"), format!(
            "irregular source nominal descriptor {} ticks/frame from {} physical samples over {}/{} seconds; descriptor duration deviation {}/{} seconds (nearest tick per sample); original bytes and sample timestamps unchanged, not a CFR grid; editable speed/reverse/hold retained with source-time and sequence-grid rounding; nominal/VFR intermediate frame sampling remains unverified",
            source_frame_rate.ticks_per_frame(), facts.timing.sample_count, media_end, facts.timing.timescale,
            delta, i128::from(crate::schema::TICKS) * i128::from(facts.timing.timescale)
        ));
    }
    if let Some(mask) = &mut opacity_mask {
        // Guides and mask items animate from the clip start, including signed
        // times before a trimmed window. Native mask keys use the media clock.
        let times = mask
            .path_keys
            .iter_mut()
            .map(|key| &mut key.source_ticks)
            .chain(
                mask.feather_keys
                    .iter_mut()
                    .map(|key| &mut key.source_ticks),
            )
            .chain(
                mask.expansion_keys
                    .iter_mut()
                    .map(|key| &mut key.source_ticks),
            )
            .chain(
                mask.opacity_keys
                    .iter_mut()
                    .map(|key| &mut key.source_ticks),
            );
        for time in times {
            *time = time
                .checked_add(source_start_ticks)
                .ok_or_else(|| unsupported("mask key exceeds the source clock"))?;
        }
    }
    if matches!(retiming, ClipRetiming::ApproximatedRamp) {
        omissions.emit_field(
            Omission {
                scope: OmissionScope::Feature,
                kind: crate::OmissionKind::Approximated,
                record: record.into(),
                reason: "time remapping was not exported faithfully: retained the bounded authored source selection at constant speed; unsupported curve detail was not reproduced".into(),
            },
            video.id,
            ExportField::TimeRemap,
        );
    }
    if matches!(video.playback.mapping(), fx_schema::LayerPlaybackMapping::Linear { input, output }
        if input.duration != output.duration)
    {
        approximate(omissions, record,
            "affine picture clock normalized to physical source and effective native clip speed; original interpretation and speed controls are not restored separately");
    }
    let intrinsic_millis = time_from_ticks(source_duration_ticks)?.as_millis();
    if video.source_intrinsic_duration.as_millis() != intrinsic_millis {
        with_context(
            omissions,
            ExportContext {
                source: ExportLossSource::Layer(video.id),
                domain: ExportLossDomain::Metadata,
            },
            |omissions| {
                approximate(
                    omissions,
                    record,
                    if video.source_intrinsic_duration.as_millis()
                        == u64::try_from(
                            source_duration_ticks / crate::schema::TICKS_PER_MILLISECOND,
                        )
                        .unwrap_or(u64::MAX)
                    {
                        format!("sourceIntrinsicDuration {} ms is the floor of the packaged MP4 duration (nearest {intrinsic_millis} ms); native media retains its exact source clock", video.source_intrinsic_duration.as_millis())
                    } else {
                        format!("stale sourceIntrinsicDuration {} ms differs from inspected current media duration {intrinsic_millis} ms; native media and bounded source selection retain the exact inspected clock", video.source_intrinsic_duration.as_millis())
                    },
                );
            },
        );
    }
    // The video's own keys count from where its key clock reads zero; a stage
    // group's Motion keys, like a guide's keys, count from the clip start.
    let video_keys = VideoKeyClock::of(video);
    let video_in = match video_keys {
        Some(clock) => clock.origin_ticks(source_start_ticks)?,
        None => source_start_ticks,
    };
    let (motion_in, motion_keys) = match clip {
        ClipLayers::Video(_) => (video_in, video_keys),
        ClipLayers::Image(_)
        | ClipLayers::Stage { .. }
        | ClipLayers::Nest(_)
        | ClipLayers::EffectScope(_) => (source_start_ticks, None),
    };
    let mut tracks = context
        .property_tracks
        .remove(&clip.motion_id())
        .unwrap_or_default();
    // Keys under an approximated ramp or a hold follow FX's remapped source
    // clock, which no native key clock is proven to carry.
    let source_clock_keys = match &retiming {
        ClipRetiming::Speed => None,
        ClipRetiming::Ramp(_) => Some("keys on a nonlinear TimeRemap ramp are not converted"),
        ClipRetiming::Segmented => Some("keys on segmented TimeRemap playback are not converted"),
        ClipRetiming::ApproximatedRamp => {
            Some("keys on a TimeRemap ramp approximated at constant speed are not converted")
        }
        ClipRetiming::FrameHold => Some("keys on an explicit Frame Hold are not converted"),
    };
    // Imported nonunit physical video uses an absolute media-time remap.
    // Effect tracks on that owner already use source time, unlike Motion on
    // an outer stage or keys on a nonunit affine authored input clock.
    let media_clock_effects = matches!(retiming, ClipRetiming::Speed)
        && playback_rate != 1.0
        && matches!(
            video.playback.mapping(),
            fx_schema::LayerPlaybackMapping::TimeRemap { .. }
        );
    let time_remap = match retiming {
        ClipRetiming::Ramp(curve) => Some(curve),
        ClipRetiming::FrameHold | ClipRetiming::Segmented => Some(
            crate::schema::PrTimeRemap::frame_hold(in_ticks, active_end_ticks - active_start_ticks),
        ),
        ClipRetiming::Speed | ClipRetiming::ApproximatedRamp => None,
    };
    let retimed_keys = source_clock_keys.or_else(|| retimed_keys_reason(playback_rate));
    tracks.retain(|property, track| {
        let reason = retimed_keys.or_else(|| {
            motion_keys
                .filter(|clock| !clock.exact(track))
                .map(|_| HELD_CLOCK_REASON)
        });
        if let Some(reason) = reason {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "{property:?} animation was not exported: {reason}; static values were kept"
                ),
            );
        }
        reason.is_none()
    });
    let (mut animations, _) = export_motion_keys(
        &mut tracks,
        motion_in,
        &mut native_transform,
        [source_width, source_height],
        [width, height],
        record,
        omissions,
    );
    if let Some(fitting) = fitting.filter(|_| !transform_stage) {
        if fitting.motion(&mut native_transform, &mut animations) {
            omit(omissions, OmissionScope::Feature, record,
                "Scale animation was not exported: non-square fitting requires nonuniform two-axis Motion keys; fitted static scale was kept");
        }
    }
    let motion_animated = animates_motion(&animations);
    // A wipe that cannot convert stops the export: no clip exports without its wipe.
    let linear_wipe = if let Some(CanonicalMask::LinearWipe(wipe)) = &mask {
        let reason = retimed_keys.or_else(|| {
            // A stage group's guide is in its video's frame, which the
            // group's Motion moves as Premiere moves a wiped clip.
            let ClipLayers::Video(_) = clip else {
                return None;
            };
            wipe_frame_reason(
                [source_width, source_height],
                [width, height],
                t.scale,
                t.rotation,
                t.anchor_point == t.position.xy_array(),
                motion_animated,
            )
        });
        if let Some(reason) = reason {
            return Err(unsupported(format!(
                "Linear Wipe cannot be exported: {reason}"
            )));
        }
        Some(export_linear_wipe(
            *wipe,
            context.property_tracks.get(&wipe.guide_id),
            wipe.initial_visible,
            source_start_ticks,
            omissions,
            record,
        )?)
    } else {
        None
    };
    // Scope effects run in the identity source plane; the group owns Motion.
    let scope_transform = identity_transform();
    let effect_transform = if matches!(clip, ClipLayers::EffectScope(_)) {
        &scope_transform
    } else {
        t
    };
    let scope_tail = match clip {
        ClipLayers::EffectScope(scope) => Some(
            scope
                .export_tail(context.dynamics, source_start_ticks)
                .map_err(unsupported)?,
        ),
        _ => None,
    };
    let mut exported_effects = if matches!(clip, ClipLayers::Video(_)) && opacity_mask.is_some() {
        effects::omit_unexported_effects(
            &video.effects,
            VIDEO_OPACITY_MASK_EFFECT_ORDER_REASON,
            record,
            omissions,
        );
        Vec::new()
    } else {
        effects::export_effects(
            &video.effects,
            context.dynamics,
            effects::EffectHost {
                layer: video.id,
                still: false,
                staged: matches!(clip, ClipLayers::Stage { .. }),
                nested: context.in_moved_nest,
                in_nest: context.depth > 0,
                transform: effect_transform,
                source_in: if media_clock_effects { 0 } else { video_in },
                video_keys,
                static_parameters_reason: source_clock_keys,
                frame: [source_width, source_height],
                canvas: [width, height],
            },
            context.written,
            record,
            omissions,
        )
    };
    if media_clock_effects
        && exported_effects
            .iter()
            .any(|effect| !effect.animations.is_empty())
    {
        approximate(
            omissions,
            record,
            format!(
                "{}; export writes independent occurrence effect chains, not shared Source effects",
                effects::RETIMED_EFFECT_CLOCK_APPROXIMATION
            ),
        );
    }
    if let Some((tail, mut written, notes)) = scope_tail {
        exported_effects.extend(tail);
        context.written.append(&mut written);
        for note in notes {
            omissions.emit(note);
        }
    }
    // A Transform stage's video ends the chain with its Transform, which
    // takes the video's keys (`unsupported_stage` accepted it): it writes
    // every track of the video, as `export_transform_stage` rejects others.
    if transform_stage {
        let mut stage = effects::export_transform_stage(
            video,
            context.dynamics,
            [source_width, source_height],
            video_in,
            shutter_angle,
            record,
            omissions,
        )
        .map_err(unsupported)?;
        if let Some(fitting) = fitting {
            fitting.stage(&mut stage);
        }
        exported_effects.push(stage);
        if shutter_angle.is_some() {
            *context.motion_blur_written = true;
            if blur.shutter_phase != 0.0 {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!(
                        "motion blur shutter phase {}° was not exported: a Transform's own shutter has no phase",
                        blur.shutter_phase
                    ),
                );
            }
        }
        for property in context
            .property_tracks
            .remove(&video.id)
            .unwrap_or_default()
            .into_keys()
        {
            context.written.record(video.id, property);
        }
    }
    if let Some(warning) = blend_approximation {
        approximate(omissions, record, warning);
    }
    Ok(Some((
        PrVideoOccurrence {
            id: None,
            media: MediaId(asset_id),
            start_ticks: active_start_ticks,
            end_ticks: active_end_ticks,
            in_ticks,
            out_ticks,
            playback_rate,
            frame_blending: video.frame_blending.as_ref().and_then(|value| match value {
                fx_schema::layer::FrameBlendingData::Boolean(false) => None,
                fx_schema::layer::FrameBlendingData::Boolean(true) => {
                    Some(fx_schema::FrameBlendingMode::Simple)
                }
                fx_schema::layer::FrameBlendingData::Mode(mode) => Some(*mode),
            }),
            opacity: t.opacity.value(),
            blend_mode,
            transform: native_transform,
            crop,
            animations,
            time_remap,
            linear_wipe,
            opacity_mask,
            track_matte,
            enabled: !clip.is_hidden(),
            // Under a mask stage, every effect applies before the mask; a
            // Transform stage has no mask.
            effects_above_mask: match clip {
                ClipLayers::Video(_)
                | ClipLayers::Nest(_)
                | ClipLayers::Image(_)
                | ClipLayers::EffectScope(_) => 0,
                ClipLayers::Stage { .. } if transform_stage => 0,
                ClipLayers::Stage { .. } => exported_effects.len(),
            },
            active_transforms: u8::from(transform_stage),
            source_effects: None,
            effects: exported_effects,
            stroke: None,
        },
        PrVideoStream {
            pixel_aspect: Default::default(),
            interpretation: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks: source_duration_ticks,
            frame_rate: source_frame_rate,
            width: facts.width,
            height: facts.height,
            kind: PrMediaKind::Video {
                codec: Some(facts.codec),
                hdr_profile: facts.hdr_profile(),
            },
        },
    )))
}

/// Exports the embedded sound of the clip `clip`, whose video is `video` and
/// whose volume keys are `volume_track`. A nested sequence has no sound, so
/// an audible clip inside the nest of the group `parent` is reported.
fn export_clip_sound(
    clip: ClipLayers<'_>,
    video: &VideoLayer,
    volume_track: Option<&PropertyKeyframeTrack>,
    parent: Option<LayerId>,
    context: &mut LayerExport<'_, '_>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) {
    if let Some(parent) = parent {
        if super::audio::audible(video, volume_track) {
            omit(
                omissions,
                OmissionScope::Feature,
                format!("group {parent}, {record}"),
                "embedded clip sound inside a nested sequence is not exported",
            );
        }
    } else if let Some((sound, facts)) = match clip {
        ClipLayers::Video(video) => {
            super::audio::embedded(video, volume_track, context.audio_facts, omissions)
        }
        // The sound plays over the group's range and follows its Enable.
        ClipLayers::Stage { .. } | ClipLayers::EffectScope(_) => {
            match super::timing::relocate_playback(&video.playback, clip.active_range()) {
                Ok(playback) => super::audio::embedded(
                    &VideoLayer {
                        playback,
                        is_hidden: clip.is_hidden(),
                        ..video.clone()
                    },
                    volume_track,
                    context.audio_facts,
                    omissions,
                ),
                Err(error) => {
                    omit(omissions, OmissionScope::Feature, record, error.to_string());
                    None
                }
            }
        }
        // A nested sequence or still has no sound.
        ClipLayers::Nest(_) | ClipLayers::Image(_) => None,
    } {
        if sound.has_volume_animation() {
            context.written.record(video.id, PropType::AudioVolume);
        }
        source_media(context.media, &sound.media).audio = Some(facts.clone());
        context.audio.push(sound);
    }
}

/// Why export omits a key track that [`VideoKeyClock::exact`] rejects.
pub(super) const HELD_CLOCK_REASON: &str =
    "a key lies outside the authored input range where FX holds the video's key clock";

/// The clock of a video's own keys (a flat clip's Motion and Opacity, its
/// effect parameters, a Transform stage's tracks) while they advance with
/// its picture at unit speed. FX animates them on the authored input clock
/// of an affine mapping, which a trim or a move of the visible window does
/// not reset, and on the source clock under an exact unit TimeRemap. A stage
/// group's keys and a mask guide's count from the clip start instead.
#[derive(Debug, Clone, Copy)]
pub(super) struct VideoKeyClock {
    /// The key time, in milliseconds, at the video's visible start.
    start: i128,
    /// The key times, in milliseconds, between which native keys show the
    /// values that FX shows: FX holds an affine clock at an end of its
    /// authored input range that the visible window passes, while native
    /// media keeps running there.
    exact: [i128; 2],
}

impl VideoKeyClock {
    /// The key clock of `video`, or `None` when its authored playback is
    /// retimed, reversed or ramped: its keys then keep counting from the
    /// source In under the existing retiming rules.
    pub(super) fn of(video: &VideoLayer) -> Option<Self> {
        let playback = &video.playback;
        let window = playback.input_range();
        match playback.mapping() {
            fx_schema::LayerPlaybackMapping::Linear { input, output }
                if input.duration == output.duration =>
            {
                let start = i128::from(window.start.as_millis())
                    + i128::from(playback.input_offset_ms())
                    - i128::from(input.start.as_millis());
                let held_end = i128::from(input.duration.as_millis());
                let shown_end = start + i128::from(window.duration.as_millis());
                Some(Self {
                    start,
                    exact: [
                        if start < 0 { 0 } else { i128::MIN },
                        if shown_end > held_end {
                            held_end
                        } else {
                            i128::MAX
                        },
                    ],
                })
            }
            fx_schema::LayerPlaybackMapping::Linear { .. } => None,
            // An exported remap's keys span the window and select source
            // inside the source range, where FX holds this clock, so FX never
            // holds it on the clip.
            fx_schema::LayerPlaybackMapping::TimeRemap { property } => {
                let (source, reverse) = constant_playback_source_range(
                    property,
                    window,
                    video.source_range,
                    playback.input_offset_ms(),
                )
                .ok()?;
                (!reverse && source.duration == window.duration).then(|| Self {
                    start: i128::from(source.start.as_millis()),
                    exact: [i128::MIN, i128::MAX],
                })
            }
        }
    }

    /// The native media tick at key time zero, on a clip whose source In is
    /// `source_in`.
    pub(super) fn origin_ticks(self, source_in: i64) -> Result<i64> {
        let origin =
            i128::from(source_in) - self.start * i128::from(crate::schema::TICKS_PER_MILLISECOND);
        i64::try_from(origin)
            .map_err(|_| unsupported("the video's key clock exceeds Premiere's tick range"))
    }

    /// Whether native keys show every value of `track` that FX shows.
    pub(super) fn exact(self, track: &PropertyKeyframeTrack) -> bool {
        let [least, greatest] = self.exact;
        track
            .keyframes()
            .iter()
            .all(|key| (least..=greatest).contains(&i128::from(key.layer_time().as_millis())))
    }

    /// Whether the clock reads zero at the video's visible start, as the
    /// clock of a guide beside it does.
    pub(super) fn counts_from_visible_start(self) -> bool {
        self.start == 0
    }
}

/// Where an exported clip plays, in Premiere ticks.
struct ClipTicks {
    /// The clip's range on the sequence frame grid.
    start_ticks: i64,
    end_ticks: i64,
    /// The source time at the clip start, on the forward media clock, from
    /// which a stage group's and a guide's keys count. A video's own keys
    /// count from its [`VideoKeyClock`] when its authored playback has unit
    /// rate. A native ramp stores its first source key here, not its value
    /// at In; ramp property keys are omitted, and stages/guides are excluded.
    source_start_ticks: i64,
    /// The native source bounds, which reverse playback measures backward
    /// from the media end.
    in_ticks: i64,
    out_ticks: i64,
    /// Negative for reverse playback.
    playback_rate: f64,
    retiming: ClipRetiming,
}

/// What an exported clip's native rate alone does not say about its playback.
enum ClipRetiming {
    /// Unit, constant or reverse speed carries the playback.
    Speed,
    /// A forward ramp retained as constant speed, for typed hybrid routing.
    ApproximatedRamp,
    /// A forward native Speed curve with its plateau-constrained handles.
    Ramp(crate::schema::PrTimeRemap),
    /// A template whose clock is replaced by editable speed/hold segments.
    Segmented,
    /// An explicit Frame Hold on one source instant ([`held_source`]).
    FrameHold,
}

/// Where `clip`, whose video is `video`, plays on the sequence frame grid
/// and in its media, which lasts `intrinsic_ticks`: as an explicit Frame Hold,
/// at native constant speed or reverse, a bounded forward ramp approximation,
/// or at unit speed. A `source.timeRemap` that export keeps plays
/// its source range at unit speed.
fn clip_ticks(
    clip: ClipLayers<'_>,
    video: &VideoLayer,
    intrinsic_ticks: i64,
    mask: Option<&CanonicalMask>,
    context: &LayerExport<'_, '_>,
) -> Result<ClipTicks> {
    let frame_rate = context.frame_rate;
    // The clip's timeline range; a staged video's own range is [0, its
    // group's duration] on the group clock.
    let active_range = clip.active_range();
    let active_end = active_range
        .start
        .checked_add_duration(active_range.duration)
        .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
    let active_start_ticks = context.frame_ticks(active_range.start, "activeRange.start")?;
    let active_end_ticks = context.picture_end_ticks(active_end, mask)?;
    ensure!(
        active_end_ticks > active_start_ticks,
        "activeRange {}..{} ms collapses to zero duration on the {frame_rate} sequence grid",
        active_range.start.as_millis(),
        active_end.as_millis()
    );
    let active_duration_ticks = active_end_ticks - active_start_ticks;
    // The shared timeline validator checks source coverage at sequence
    // sample times, including a final partial source frame.
    // Use the inspected physical clock, not the authored duration cache.
    // Selected source samples remain bounded by native timeline validation.
    if let Some(held) = held_source(video) {
        // The placement shows `held` at unit speed. Out may pass the media
        // end: the timeline validator requires only the held tick before it.
        let in_ticks = ticks_from_time(held, "held source instant")?;
        ensure!(
            in_ticks < intrinsic_ticks,
            "held source instant {} ms is not before the media end",
            held.as_millis()
        );
        let out_ticks = in_ticks
            .checked_add(active_duration_ticks)
            .ok_or_else(|| unsupported("held source end exceeds Premiere's tick range"))?;
        return Ok(ClipTicks {
            start_ticks: active_start_ticks,
            end_ticks: active_end_ticks,
            source_start_ticks: in_ticks,
            in_ticks,
            out_ticks,
            playback_rate: 1.0,
            retiming: ClipRetiming::FrameHold,
        });
    }
    if let fx_schema::LayerPlaybackMapping::Linear { input, output } = video.playback.mapping() {
        // Preserve the established whole-ms source-span/snap route. Only
        // fractional source endpoints need the exact-tick affine extension;
        // neither source names nor hidden import provenance select this path.
        if input.duration != output.duration
            && super::timing::linear_source_range(&video.playback).is_err()
        {
            ensure!(
                active_start_ticks == ticks_from_time(active_range.start, "affine placement")?
                    && ticks_from_time(context.origin, "affine parent origin")?
                        % frame_rate.ticks_per_frame()
                        == 0,
                "nonunit affine picture requires an exact sequence-grid placement origin"
            );
            let parent_start = ticks_from_time(video.playback.input_range().start, "affine input")?;
            let parent_end = parent_start
                .checked_add(active_duration_ticks)
                .ok_or_else(|| unsupported("affine input end exceeds Premiere's tick range"))?;
            let in_ticks = super::timing::linear_source_ticks(&video.playback, parent_start)?;
            let out_ticks = super::timing::linear_source_ticks(&video.playback, parent_end)?;
            ensure!(
                in_ticks >= ticks_from_time(video.source_range.start, "source selection")?
                    && out_ticks
                        <= ticks_from_time(video.source_range.end(), "source selection end")?,
                "affine playback window extends beyond the authored source selection"
            );
            ensure!(
                in_ticks >= 0 && in_ticks < intrinsic_ticks && out_ticks > in_ticks,
                "affine picture has invalid physical source bounds"
            );
            let last = super::timing::linear_source_ticks(
                &video.playback,
                parent_end - frame_rate.ticks_per_frame(),
            )?;
            ensure!(
                i128::from(last)
                    <= i128::from(intrinsic_ticks)
                        + 3 * i128::from(crate::schema::TICKS_PER_MILLISECOND) / 2,
                "affine picture requires a sample past the physical media end"
            );
            return Ok(ClipTicks {
                start_ticks: active_start_ticks,
                end_ticks: active_end_ticks,
                source_start_ticks: in_ticks,
                in_ticks,
                out_ticks,
                playback_rate: output.duration.as_millis() as f64
                    / input.duration.as_millis() as f64,
                retiming: ClipRetiming::Speed,
            });
        }
    }
    if let Some(ramp) = super::time_remap::native_ramp(video) {
        ensure!(
            ramp.curve
                .keys
                .iter()
                .all(|key| key.source_ticks <= intrinsic_ticks),
            "TimeRemap key exceeds the packaged media duration"
        );
        return Ok(ClipTicks {
            start_ticks: active_start_ticks,
            end_ticks: active_end_ticks,
            source_start_ticks: ramp.curve.keys[0].source_ticks,
            in_ticks: ramp.in_ticks,
            out_ticks: ramp.out_ticks,
            playback_rate: (ramp.out_ticks - ramp.in_ticks) as f64 / active_duration_ticks as f64,
            retiming: ClipRetiming::Ramp(ramp.curve),
        });
    }
    if super::playback_segments::segmented_remap(video).is_some() {
        // Build the shared static appearance once. This hold is a template,
        // never an emitted substitute for the authored playback.
        return Ok(ClipTicks {
            start_ticks: active_start_ticks,
            end_ticks: active_end_ticks,
            source_start_ticks: 0,
            in_ticks: 0,
            out_ticks: active_duration_ticks,
            playback_rate: 1.0,
            retiming: ClipRetiming::Segmented,
        });
    }
    let native_playback = video
        .playback
        .time_remap()
        .map(|playback| {
            native_playback_source_range(
                playback,
                video.playback.input_range(),
                video.source_range,
                video.playback.input_offset_ms(),
            )
        })
        .transpose()?;
    let mapped_source = match &native_playback {
        Some(playback) => playback.source_range,
        None => super::timing::linear_source_range(&video.playback)?,
    };
    ensure!(
        mapped_source.start >= video.source_range.start
            && mapped_source.end() <= video.source_range.end(),
        "playback window extends beyond the authored source selection"
    );
    let source_end = mapped_source
        .start
        .checked_add_duration(mapped_source.duration)
        .ok_or_else(|| unsupported("sourceRange end exceeds Premiere's tick range"))?;
    let source_start_ticks = ticks_from_time(mapped_source.start, "mapped source start")?;
    let mut source_end_ticks = ticks_from_time(source_end, "sourceRange.end")?;
    let source_duration_ticks = source_end_ticks
        .checked_sub(source_start_ticks)
        .ok_or_else(|| unsupported("sourceRange duration underflows"))?;
    let retiming = if native_playback
        .as_ref()
        .is_some_and(|playback| playback.approximated)
    {
        ClipRetiming::ApproximatedRamp
    } else {
        ClipRetiming::Speed
    };
    // Export omits a clip with any other playback ([`unexportable_clip`]).
    let playback_rate = if let Some(playback) = native_playback {
        let rate = source_duration_ticks as f64 / active_duration_ticks as f64;
        if playback.reverse {
            -rate
        } else {
            rate
        }
    } else if matches!(video.playback.mapping(),
        fx_schema::LayerPlaybackMapping::Linear { input, output } if input.duration != output.duration
    ) {
        source_duration_ticks as f64 / active_duration_ticks as f64
    } else {
        // Persisted millisecond endpoints can straddle a sequence-frame snap.
        // Unit-speed clips keep the snapped active duration rather than
        // accidentally introducing a tiny retime.
        source_end_ticks = source_start_ticks
            .checked_add(active_duration_ticks)
            .ok_or_else(|| unsupported("sourceRange end exceeds Premiere's tick range"))?;
        1.0
    };
    // Native reverse bounds are measured backward from the media end.
    let reverse = playback_rate < 0.0;
    let in_ticks = if reverse {
        intrinsic_ticks
            .checked_sub(source_end_ticks)
            .ok_or_else(|| unsupported("reverse source end exceeds intrinsic duration"))?
    } else {
        source_start_ticks
    };
    let out_ticks = if reverse {
        intrinsic_ticks
            .checked_sub(source_start_ticks)
            .ok_or_else(|| unsupported("reverse source start exceeds intrinsic duration"))?
    } else {
        source_end_ticks
    };
    Ok(ClipTicks {
        start_ticks: active_start_ticks,
        end_ticks: active_end_ticks,
        source_start_ticks,
        in_ticks,
        out_ticks,
        playback_rate,
        retiming,
    })
}

/// The static Motion of the clip whose Motion `motion` hosts, whose picture is
/// `frame` pixels: the position normalized to the canvas, the anchor to the
/// picture. Unsupported Motion fields/keys recover locally with diagnostics.
pub(super) fn export_transform(
    motion: MotionHost<'_>,
    [source_width, source_height]: [u32; 2],
    [width, height]: [u32; 2],
    dynamics: &AnimationGraph,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> PrStaticTransform {
    let t = motion.transform;
    let mut native_transform = PrStaticTransform {
        position: [
            t.position.x() / f64::from(width),
            t.position.y() / f64::from(height),
        ],
        anchor_point: [
            t.anchor_point[0] / f64::from(source_width),
            t.anchor_point[1] / f64::from(source_height),
        ],
        scale: t.scale,
        rotation: t.rotation,
    };
    for (axis, scale) in native_transform.scale.iter_mut().enumerate() {
        let recovered = if scale.is_finite() {
            scale.abs().min(10000.0)
        } else {
            100.0
        };
        if *scale != recovered {
            approximate(omissions, record, format!(
                "Scale axis {axis} approximated using a valid native magnitude {recovered}; picture and other controls retained"));
            *scale = recovered;
        }
    }
    if !MOTION_PARAMS.iter().any(|spec| {
        spec.animation == Some(PrAnimatedProperty::Rotation)
            && spec.holds(native_transform.rotation)
    }) {
        native_transform.rotation = if t.rotation.is_finite() {
            t.rotation.rem_euclid(360.0)
        } else {
            0.0
        };
        approximate(omissions, record,
            "Rotation normalized to a valid equivalent static angle; picture and other controls retained");
    }
    for (changed, detail) in [
        (t.skew != 0.0 || t.skew_axis != 0.0, "skew"),
        (
            t.rotation_x != 0.0 || t.rotation_y != 0.0 || t.orientation != [0.0, 0.0, 0.0],
            "3D rotation",
        ),
        // FX's camera scales a layer at depth z; a clip has no depth.
        (
            t.position.z().is_some_and(|z| z != 0.0)
                || layer_animations(dynamics, motion.id)
                    .any(|(property, _)| property == PropType::PositionZ),
            "3D position",
        ),
    ] {
        if changed {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("{detail} was not exported"),
            );
        }
    }
    native_transform
}

/// The Crop and Linear Wipe that a nest placement's `mask` exports as. The
/// wipe takes its keys from its guide's Scale track, counted from `source_in`.
/// A nest's Opacity coverage is selected by `export_group_at`, which reports
/// its approximations only for a retained placement.
pub(super) fn export_mask(
    mask: Option<&CanonicalMask>,
    context: &LayerExport<'_, '_>,
    source_in: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<(PrStaticCrop, Option<PrLinearWipe>)> {
    Ok(match mask {
        None => (PrStaticCrop::default(), None),
        Some(CanonicalMask::Crop { crop, .. }) => (*crop, None),
        Some(CanonicalMask::LinearWipe(wipe)) => {
            let wipe = export_linear_wipe(
                *wipe,
                context.property_tracks.get(&wipe.guide_id),
                wipe.initial_visible,
                source_in,
                omissions,
                record,
            )?;
            (PrStaticCrop::default(), Some(wipe))
        }
        Some(CanonicalMask::Opacity { .. }) => (PrStaticCrop::default(), None),
        // The nest's track matte is written once its source is placed.
        Some(CanonicalMask::TrackMatte { .. }) => (PrStaticCrop::default(), None),
    })
}

/// The Anchor Point keys of the FX `AnchorPointX` and `AnchorPointY` tracks
/// `x` and `y`, counted from `source_in`, each point a fraction of the
/// clip's `frame` as the static anchor is ([`export_transform`]). A native
/// key holds both coordinates, so the axes need the same key times and
/// easing.
fn export_anchor_point_keys(
    x: &PropertyKeyframeTrack,
    y: &PropertyKeyframeTrack,
    source_in: i64,
    frame: [u32; 2],
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Vec<PrPointKeyframe>> {
    let x_keys = export_scalar_keys(x, source_in, "Anchor Point X", omissions, record)?;
    let y_keys = export_scalar_keys(y, source_in, "Anchor Point Y", omissions, record)?;
    ensure!(
        x_keys.len() == y_keys.len()
            && x_keys
                .iter()
                .zip(&y_keys)
                .all(|(x, y)| x.source_ticks == y.source_ticks && x.easing == y.easing),
        "Anchor Point X/Y keys must have identical times and temporal easing"
    );
    let [width, height] = frame.map(f64::from);
    Ok(x_keys
        .into_iter()
        .zip(y_keys)
        .map(|(x, y)| PrPointKeyframe {
            source_ticks: x.source_ticks,
            value: [x.value / width, y.value / height],
            easing: x.easing,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        })
        .collect())
}

/// `animation` when its keys take the one measured form of their property
/// ([`PrPropertyAnimation::unmeasured_form`]).
fn measured(animation: PrPropertyAnimation) -> Result<PrPropertyAnimation> {
    match animation.unmeasured_form() {
        Some(reason) => Err(unsupported(reason)),
        None => Ok(animation),
    }
}

/// The Opacity, Rotation, Position, Anchor Point and Scale keys among
/// `tracks`, the keys of the layer whose Motion the clip takes, counted from
/// `source_in`; each track that cannot export is reported, and
/// the first one's reason is returned. Position keys are fractions of the
/// `canvas` and Anchor Point keys of the clip's `frame`, as their static
/// values are ([`export_transform`]), and each sets its static value. Scale
/// keys export as uniform Scale when both axes have the same keys over equal
/// static scales, and as Scale Width keys when the X axis alone has keys,
/// which turns Uniform Scale off whatever the static scales. The tracks of
/// other properties stay in `tracks`.
pub(super) fn export_motion_keys(
    tracks: &mut BTreeMap<PropType, &PropertyKeyframeTrack>,
    source_in: i64,
    native_transform: &mut PrStaticTransform,
    frame: [u32; 2],
    [width, height]: [u32; 2],
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> (Vec<PrPropertyAnimation>, Option<String>) {
    let mut rejected = None;
    let mut reject = |omissions: &mut dyn OmissionSink, reason: String| {
        omit(omissions, OmissionScope::Feature, record, reason.clone());
        rejected.get_or_insert(reason);
    };
    let mut animations = Vec::new();
    if let Some(opacity) = tracks.remove(&PropType::Opacity) {
        match export_scalar_keys(opacity, source_in, "Opacity", omissions, record) {
            Ok(keys) => animations.push(PrPropertyAnimation::Opacity(keys)),
            Err(error) => reject(
                omissions,
                format!("Opacity animation was not exported: {error}"),
            ),
        }
    }
    if let Some(rotation) = tracks.remove(&PropType::Rotation) {
        match export_scalar_keys(rotation, source_in, "Rotation", omissions, record) {
            Ok(keys) => animations.push(PrPropertyAnimation::Rotation(keys)),
            Err(error) => reject(
                omissions,
                format!("Rotation animation was not exported: {error}"),
            ),
        }
    }
    let position_x = tracks.remove(&PropType::PositionX);
    let position_y = tracks.remove(&PropType::PositionY);
    match (position_x, position_y) {
        (Some(x), Some(y)) => match export_position_keys(x, y, source_in, [width, height]) {
            Ok(keys) => {
                if let Some(first) = keys.first() {
                    native_transform.position = first.value;
                }
                animations.push(PrPropertyAnimation::Position(keys));
            }
            Err(error) => reject(
                omissions,
                format!("Position animation was not exported: {error}"),
            ),
        },
        (None, None) => {}
        _ => reject(
            omissions,
            "unpaired Position keyframes were not exported".to_owned(),
        ),
    }
    let anchor_x = tracks.remove(&PropType::AnchorPointX);
    let anchor_y = tracks.remove(&PropType::AnchorPointY);
    match (anchor_x, anchor_y) {
        (Some(x), Some(y)) => {
            match export_anchor_point_keys(x, y, source_in, frame, omissions, record)
                .and_then(|keys| measured(PrPropertyAnimation::AnchorPoint(keys)))
            {
                Ok(animation) => {
                    if let Some(first) = animation.point_keys().and_then(<[_]>::first) {
                        native_transform.anchor_point = first.value;
                    }
                    animations.push(animation);
                }
                Err(error) => reject(
                    omissions,
                    format!("Anchor Point animation was not exported: {error}"),
                ),
            }
        }
        (None, None) => {}
        _ => reject(
            omissions,
            "unpaired Anchor Point keyframes were not exported".to_owned(),
        ),
    }
    let x = tracks.remove(&PropType::ScaleX);
    let y = tracks.remove(&PropType::ScaleY);
    match (x, y) {
        (Some(x), Some(y))
            if native_transform.scale[0] == native_transform.scale[1]
                && scale_tracks_match(x, y) =>
        {
            match export_scalar_keys(x, source_in, "Scale", omissions, record) {
                Ok(keys) => animations.push(PrPropertyAnimation::UniformScale(keys)),
                Err(error) => reject(
                    omissions,
                    format!("Scale animation was not exported: {error}"),
                ),
            }
        }
        (Some(x), None) => match export_scalar_keys(x, source_in, "Scale Width", omissions, record)
            .and_then(|keys| measured(PrPropertyAnimation::ScaleWidth(keys)))
        {
            Ok(animation) => animations.push(animation),
            Err(error) => reject(
                omissions,
                format!("Scale Width animation was not exported: {error}"),
            ),
        },
        (None, None) => {}
        _ => reject(
            omissions,
            "nonuniform or unpaired Scale keyframes were not exported".to_owned(),
        ),
    }
    (animations, rejected)
}

/// The layers that a mask of `layers` references, whatever kind of layer the
/// mask is on. FX never paints a mask guide.
pub(super) fn mask_guide_ids(layers: &[Layer]) -> BTreeSet<LayerId> {
    layers
        .iter()
        .flat_map(|layer| match layer.data() {
            LayerData::Media(media) => media.masks.as_slice(),
            LayerData::Text(text) => text.masks.as_slice(),
            LayerData::Video(video) => video.masks.as_slice(),
            LayerData::Image(image) => image.masks.as_slice(),
            LayerData::Rect(rect) => rect.masks.as_slice(),
            LayerData::Shape(shape) => shape.masks.as_slice(),
            LayerData::Group(group) => group.masks.as_slice(),
            LayerData::BooleanOperation(boolean) => boolean.masks.as_slice(),
            LayerData::Adjustment(adjustment) => adjustment.masks.as_slice(),
            LayerData::Pag(_) | LayerData::Audio(_) | LayerData::AiEdit(_) => &[],
        })
        .filter_map(|mask| mask.layer)
        .collect()
}

/// The layers that a mask of `layers` or of their descendants references.
fn nested_mask_guide_ids(layers: &[Layer]) -> BTreeSet<LayerId> {
    let mut ids = mask_guide_ids(layers);
    for layer in layers {
        if let Some(children) = layer.data().child_layers() {
            ids.extend(nested_mask_guide_ids(children));
        }
    }
    ids
}

fn mask_item_ids(layers: &[Layer]) -> BTreeSet<fx_schema::FxItemId> {
    let mut ids = BTreeSet::new();
    for layer in layers {
        let masks = match layer.data() {
            LayerData::Media(v) => &v.masks,
            LayerData::Text(v) => &v.masks,
            LayerData::Video(v) => &v.masks,
            LayerData::Image(v) => &v.masks,
            LayerData::Rect(v) => &v.masks,
            LayerData::Shape(v) => &v.masks,
            LayerData::Group(v) => &v.masks,
            LayerData::BooleanOperation(v) => &v.masks,
            LayerData::Adjustment(v) => &v.masks,
            LayerData::Pag(_) | LayerData::Audio(_) | LayerData::AiEdit(_) => continue,
        };
        ids.extend(masks.iter().map(|mask| mask.id));
        if let Some(children) = layer.data().child_layers() {
            ids.extend(mask_item_ids(children));
        }
    }
    ids
}

/// The text layers among `layers` and their descendants whose text content
/// `dynamics` keys: only their Source Text fields export as Source Text keys.
/// Keyed style fields of a text layer with static text keep the static text,
/// with the animation reported as omitted.
fn keyed_text_layer_ids(layers: &[Layer], dynamics: &AnimationGraph) -> BTreeSet<LayerId> {
    let text_layer_ids = text_layer_ids(layers);
    dynamics
        .entries()
        .iter()
        .filter_map(|entry| match &entry.target {
            PropertyTarget::LayerProperty(property)
                if property.property_type() == PropType::TextContent
                    && text_layer_ids.contains(&property.layer_id()) =>
            {
                Some(property.layer_id())
            }
            _ => None,
        })
        .collect()
}

/// Only the bounded width-only text animator has a Source Text key carrier.
fn stroke_width_ids(layers: &[Layer]) -> BTreeSet<fx_schema::FxItemId> {
    let mut ids = BTreeSet::new();
    for layer in layers {
        if let LayerData::Text(text) = layer.data() {
            if let Ok(Some(animator)) = super::graphic::stroke_width_animator(text) {
                ids.insert(animator.id);
            }
        }
        if let Some(children) = layer.data().child_layers() {
            ids.extend(stroke_width_ids(children));
        }
    }
    ids
}

/// The layers among `layers` and their descendants whose Anchor Point keys
/// export as clip Motion keys ([`export_motion_keys`]): videos, stills and
/// groups that are no graphic, and the guide of a video's mask, which
/// repeats them. A graphic's objects and Vector Motion key no Anchor Point,
/// so the keys of text, shape, rectangle and graphic group layers stay
/// reported as animation that export omits.
fn anchor_point_layer_ids(layers: &[Layer], dynamics: &AnimationGraph) -> BTreeSet<LayerId> {
    let mut ids = BTreeSet::new();
    for layer in layers {
        match layer.data() {
            LayerData::Video(_) | LayerData::Media(_) | LayerData::Image(_) => {
                ids.insert(layer.id());
                let (_, masks) = layer_consumers(layer);
                ids.extend(masks.iter().filter_map(|mask| mask.layer));
            }
            LayerData::Group(group)
                if super::graphic::graphic_objects(group, dynamics).is_none() =>
            {
                ids.insert(group.id);
                ids.extend(anchor_point_layer_ids(&group.layers, dynamics));
            }
            // No other layer exports as a clip with Motion.
            _ => {}
        }
    }
    ids
}

/// The text layers among `layers` and their descendants.
fn text_layer_ids(layers: &[Layer]) -> BTreeSet<LayerId> {
    let mut ids = BTreeSet::new();
    for layer in layers {
        if let LayerData::Text(text) = layer.data() {
            ids.insert(text.id);
        }
        if let Some(children) = layer.data().child_layers() {
            ids.extend(text_layer_ids(children));
        }
    }
    ids
}

/// The layers of `layers` that exactly one mask of `layers` references and
/// no track matte does: a guide with no other use, which export consumes with
/// the clip whose mask it outlines.
fn sole_mask_guides(layers: &[Layer]) -> BTreeSet<LayerId> {
    let mut mask_uses: BTreeMap<LayerId, usize> = BTreeMap::new();
    let mut matte_sources = BTreeSet::new();
    for layer in layers {
        let (track_matte, masks) = layer_consumers(layer);
        matte_sources.extend(track_matte.iter().map(|matte| matte.layer));
        for guide in masks.iter().filter_map(|mask| mask.layer) {
            *mask_uses.entry(guide).or_default() += 1;
        }
    }
    mask_uses
        .into_iter()
        .filter(|(guide, uses)| *uses == 1 && !matte_sources.contains(guide))
        .map(|(guide, _)| guide)
        .collect()
}

/// The track matte and masks of `layer`, which consume the layers they name.
fn layer_consumers(layer: &Layer) -> (&Option<TrackMatte>, &[PathMask]) {
    match layer.data() {
        LayerData::Text(l) => (&l.track_matte, &l.masks),
        LayerData::Video(l) => (&l.track_matte, &l.masks),
        LayerData::Media(l) => (&l.track_matte, &l.masks),
        LayerData::Image(l) => (&l.track_matte, &l.masks),
        LayerData::Rect(l) => (&l.track_matte, &l.masks),
        LayerData::Shape(l) => (&l.track_matte, &l.masks),
        LayerData::Group(l) => (&l.track_matte, &l.masks),
        LayerData::BooleanOperation(l) => (&l.track_matte, &l.masks),
        LayerData::Adjustment(l) => (&l.track_matte, &l.masks),
        LayerData::Pag(_) | LayerData::Audio(_) | LayerData::AiEdit(_) => (&None, &[]),
    }
}

/// The layers that a layer of `layers`, or of their descendants, uses as its
/// track matte source or mask guide, and the path layer of each text that FX
/// shows. FX consumes a text's path only while neither the text's own flag
/// nor a group around it hides the text (`collect_layer`), and otherwise
/// paints the path layer as content of its own; `ancestor_hidden` says
/// whether a group around `layers` hides them.
pub(super) fn consumed_layer_ids(layers: &[Layer], ancestor_hidden: bool) -> BTreeSet<LayerId> {
    let mut consumed = BTreeSet::new();
    for layer in layers {
        let (track_matte, masks) = layer_consumers(layer);
        consumed.extend(track_matte.iter().map(|matte| matte.layer));
        consumed.extend(masks.iter().filter_map(|mask| mask.layer));
        if let LayerData::Text(text) = layer.data() {
            if !(ancestor_hidden || text.is_hidden) {
                consumed.extend(text.path_options.as_ref().map(|path| path.path_layer));
            }
        }
        if let Some(children) = layer.child_layers() {
            let hidden = matches!(layer.data(), LayerData::Group(group) if group.is_hidden);
            consumed.extend(consumed_layer_ids(children, ancestor_hidden || hidden));
        }
    }
    consumed
}

/// Places an item on the lowest track, at least `min_track`, above every item
/// it overlaps, and returns that track's index.
pub(super) fn place_item(
    video_tracks: &mut Vec<PrVideoTrack>,
    item: PrVideoItem,
    min_track: usize,
    context: &mut LayerExport<'_, '_>,
) -> Result<usize> {
    let index = item_track(video_tracks, &item, min_track);
    while index >= video_tracks.len() {
        video_tracks.push(PrVideoTrack {
            items: Vec::new(),
            transitions: Vec::new(),
            nests: Vec::new(),
        });
    }
    video_tracks[index].items.push(item);
    context
        .packer
        .record_item(context.container, context.boundary()?, index)?;
    Ok(index)
}

fn item_track(video_tracks: &[PrVideoTrack], item: &PrVideoItem, min_track: usize) -> usize {
    // All assigned items are lower in the current paint stack.
    video_tracks
        .iter()
        .rposition(|track| {
            track.items.iter().any(|lower| item.overlaps(lower))
                || track
                    .nests
                    .iter()
                    .any(|nest| nest.overlaps(&item.timeline_ticks()))
        })
        .map_or(0, |index| index + 1)
        .max(min_track)
}

fn retimed_video_items(
    video: &VideoLayer,
    occurrence: &PrVideoOccurrence,
    source: &PrVideoStream,
    frame_rate: FrameRate,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<Vec<PrVideoItem>>> {
    let (segments, reason) = if let Some(segments) =
        super::playback_segments::segmented(video, occurrence, source, frame_rate)?
    {
        (segments, "TimeRemap retained as editable speed and Frame Hold segments; cuts snap to sequence frames, nonlinear legs use endpoint lines, ineligible source intervals stay empty, endpoint holds preserve valid samples, source-frame selection can differ, and sub-frame edits may differ")
    } else if occurrence.playback_rate < 0.0 && occurrence.time_remap.is_none() {
        (super::playback_segments::reverse(occurrence, source, frame_rate)?,
            "reverse playback retained as an editable boundary approximation; source-frame selection can differ, endpoint Frame Holds can split the clip, and sub-frame edits may differ")
    } else {
        return Ok(None);
    };
    omissions.emit_field(
        Omission {
            scope: OmissionScope::Feature,
            kind: crate::OmissionKind::Approximated,
            record: record.into(),
            reason: reason.into(),
        },
        video.id,
        ExportField::TimeRemap,
    );
    Ok(Some(
        segments
            .into_iter()
            .map(|segment| PrVideoItem::Media(segment.apply(occurrence)))
            .collect(),
    ))
}

/// The layers that a track matte of `layers`, or of their descendants, names
/// as its source.
pub(super) fn track_matte_sources(layers: &[Layer]) -> BTreeSet<LayerId> {
    let mut sources = BTreeSet::new();
    for layer in layers {
        sources.extend(layer_consumers(layer).0.iter().map(|matte| matte.layer));
        if let Some(children) = layer.child_layers() {
            sources.extend(track_matte_sources(children));
        }
    }
    sources
}

/// One exported placement that a track matte source keys, where
/// `export_layers` placed it; positions hold until the tracks are sorted.
#[derive(Clone, Copy)]
enum MatteConsumer {
    Item { track: usize, position: usize },
    Nest { track: usize, position: usize },
}

/// The exported placements that each track matte source of one layer list
/// keys, until the source is placed above them all and their Track Matte Keys
/// name its track.
#[derive(Default)]
struct MatteConsumers {
    consumers: BTreeMap<LayerId, Vec<MatteConsumer>>,
}

impl MatteConsumers {
    /// Records the last item placed on `track` as a consumer of `source`.
    fn add_item(&mut self, video_tracks: &[PrVideoTrack], source: LayerId, track: usize) {
        let position = video_tracks[track].items.len() - 1;
        self.consumers
            .entry(source)
            .or_default()
            .push(MatteConsumer::Item { track, position });
    }

    /// Records the last nest placed on `track` as a consumer of `source`.
    fn add_nest(&mut self, video_tracks: &[PrVideoTrack], source: LayerId, track: usize) {
        let position = video_tracks[track].nests.len() - 1;
        self.consumers
            .entry(source)
            .or_default()
            .push(MatteConsumer::Nest { track, position });
    }

    /// The lowest track strictly above every placement that `source` keys, if
    /// any exported placement keys it.
    fn min_track(&self, source: LayerId) -> Option<usize> {
        self.consumers
            .get(&source)?
            .iter()
            .map(|consumer| match consumer {
                MatteConsumer::Item { track, .. } | MatteConsumer::Nest { track, .. } => track + 1,
            })
            .max()
    }

    /// Records that `source` was placed on `source_track`: every placement
    /// that keys it names that track.
    fn place_source(
        &mut self,
        video_tracks: &mut [PrVideoTrack],
        source: LayerId,
        source_track: usize,
        context: &mut LayerExport<'_, '_>,
    ) -> Result<()> {
        use super::packing::PlacementLocation;
        let source_location = PlacementLocation::Item {
            track: source_track,
            position: video_tracks[source_track].items.len() - 1,
        };
        self.bind_source(video_tracks, source, source_location, context)
    }

    fn bind_source(
        &mut self,
        video_tracks: &mut [PrVideoTrack],
        source: LayerId,
        source_location: super::packing::PlacementLocation,
        context: &mut LayerExport<'_, '_>,
    ) -> Result<()> {
        use super::packing::PlacementLocation;
        let source_track = match source_location {
            PlacementLocation::Item { track, .. } | PlacementLocation::Nest { track, .. } => track,
        };
        let consumers = self.consumers.remove(&source).unwrap_or_default();
        let consumer_range = |consumer: &MatteConsumer| match *consumer {
            MatteConsumer::Item { track, position } => {
                video_tracks[track].items[position].timeline_ticks()
            }
            MatteConsumer::Nest { track, position } => {
                video_tracks[track].nests[position].timeline_ticks()
            }
        };
        if let Some(range) = consumers.first().map(consumer_range).filter(|range| {
            consumers
                .iter()
                .all(|consumer| consumer_range(consumer) == *range)
        }) {
            // A retained picture can end before its authored transparent tail.
            // Trim only a shared unit-clock matte's unused end, never its keys,
            // start/source clock or the coverage of any other consumer.
            if let PlacementLocation::Item { track, position } = source_location {
                if let PrVideoItem::Media(matte) = &mut video_tracks[track].items[position] {
                    if matte.start_ticks == range.start
                        && matte.end_ticks > range.end
                        && matte.playback_rate == 1.0
                        && matte.time_remap.is_none()
                        && matte.out_ticks - matte.in_ticks == matte.end_ticks - matte.start_ticks
                    {
                        matte.out_ticks -= matte.end_ticks - range.end;
                        matte.end_ticks = range.end;
                    }
                }
            }
        }
        for consumer in consumers {
            let consumer_location = match consumer {
                MatteConsumer::Item { track, position } => {
                    PlacementLocation::Item { track, position }
                }
                MatteConsumer::Nest { track, position } => {
                    PlacementLocation::Nest { track, position }
                }
            };
            let track_matte = match consumer {
                MatteConsumer::Item { track, position } => {
                    match &mut video_tracks[track].items[position] {
                        PrVideoItem::Media(clip) => &mut clip.track_matte,
                        PrVideoItem::Graphic(_) | PrVideoItem::Capsule(_) => continue,
                    }
                }
                MatteConsumer::Nest { track, position } => {
                    &mut video_tracks[track].nests[position].track_matte
                }
            };
            if let Some(matte) = track_matte {
                matte.track_index = source_track;
                context.packer.record_matte(
                    context.container,
                    consumer_location,
                    source_location,
                )?;
            }
        }
        Ok(())
    }
}

/// Exports the child `source` of the stage group `group`, the source of the
/// group's track matte, as a clip of the list whose parent is `parent`, over
/// the group's range and above the group's clip; its sound is not exported.
fn export_staged_matte<'d>(
    group: &'d GroupLayer,
    source: LayerId,
    parent: Option<LayerId>,
    video_tracks: &mut Vec<PrVideoTrack>,
    consumers: &mut MatteConsumers,
    context: &mut LayerExport<'_, 'd>,
    omissions: &mut dyn OmissionSink,
) -> Result<()> {
    let (width, height) = (context.width, context.height);
    let min_track = consumers
        .min_track(source)
        .ok_or_else(|| unsupported("the stage group's clip was not placed"))?;
    let source = group
        .layers
        .iter()
        .find(|layer| layer.id() == source)
        .ok_or_else(|| unsupported("the track matte source is not under its stage group"))?;
    let record = format!("layer {} ({:?})", source.id(), source.name());
    let track = match source.data() {
        LayerData::Video(video) => {
            let video = VideoLayer {
                playback: super::timing::relocate_playback(
                    &video.playback, group.playback.input_range(),
                )?,
                parent,
                ..video.clone()
            };
            let volume_track = context
                .property_tracks
                .get_mut(&video.id)
                .and_then(|tracks| tracks.remove(&PropType::AudioVolume));
            if super::audio::audible(&video, volume_track) {
                with_context(
                    omissions,
                    ExportContext {
                        source: ExportLossSource::Layer(video.id),
                        domain: ExportLossDomain::Audio,
                    },
                    |omissions| {
                        omit(
                            omissions,
                            OmissionScope::Feature,
                            &record,
                            "embedded audio of a staged track matte source was not exported",
                        )
                    },
                );
            }
            let clip = ClipLayers::Video(&video);
            let accepted = clip_mask(clip, &group.layers, context.dynamics, [width, height])?
                .map_err(|reason| {
                    unsupported(format!(
                        "{record}: the staged track matte source cannot be exported: {reason}"
                    ))
                })?;
            let (occurrence, stream) = export_video_clip(
                clip, &video, parent, true, accepted, context, &record, omissions,
            )
            .map_err(|error| BuildError::Context {
                context: record.clone(),
                source: Box::new(error),
            })?.ok_or_else(|| unsupported(
                "staged track matte source has no supported unit irregular clock; its consumer cannot be exported without the matte",
            ))?;
            // The keys that the source's clip writes; it has no mask of its
            // own ([`canonical_track_matte`]), so no guide track either.
            context
                .written
                .record_animations(video.id, &occurrence.animations);
            let parts = retimed_video_items(
                &video, &occurrence, &stream, context.frame_rate, &record, omissions,
            )?;
            source_media(context.media, &occurrence.media).video = Some(stream);
            let item = PrVideoItem::Media(occurrence);
            let min_track = if parts.is_some() {
                item_track(video_tracks, &item, min_track)
            } else {
                min_track
            };
            let mut track = min_track;
            for item in parts.unwrap_or_else(|| vec![item]) {
                track = place_item(video_tracks, item, min_track, context)?;
            }
            track
        }
        LayerData::Image(image) => {
            let image = ImageLayer {
                active_range: group.playback.input_range(),
                parent,
                ..image.clone()
            };
            // A matte source has no mask of its own ([`canonical_track_matte`]).
            let exported =
                super::still::export_image_layer(&image, parent, None, context, omissions, &record)
                    .map_err(|error| BuildError::Context {
                        context: record.clone(),
                        source: Box::new(error),
                    })?;
            // A still that its own rules omit is reported there and stays
            // unplaced, so its keyed clip fails the timeline validation
            // ([`UNPLACED_MATTE_TRACK`]).
            let Some((occurrence, media)) = exported else {
                return Ok(());
            };
            context
                .media
                .entry(occurrence.media.clone())
                .or_insert(media);
            place_item(
                video_tracks,
                PrVideoItem::Media(occurrence),
                min_track,
                context,
            )?
        }
        LayerData::Group(provider) => {
            let Some(track) = super::nested::export_group_at(
                provider,
                &group.layers,
                Some(group.playback.input_range()),
                video_tracks,
                context,
                omissions,
            )? else {
                return Ok(());
            };
            let location = super::packing::PlacementLocation::Nest {
                track,
                position: video_tracks[track].nests.len() - 1,
            };
            return consumers.bind_source(video_tracks, source.id(), location, context);
        }
        _ => {
            return Err(unsupported(format!(
                "{record}: a staged track matte source that is not a video or still image is not exported"
            )))
        }
    };
    consumers.place_source(video_tracks, source.id(), track, context)?;
    Ok(())
}

/// Snaps a layer's active range to the sequence frame grid, by the rule of
/// its list ([`LayerExport::frame_ticks`]).
fn active_range_ticks(
    range: &TimeRangeProperty,
    context: &LayerExport<'_, '_>,
) -> Result<std::ops::Range<i64>> {
    let end = range
        .start
        .checked_add_duration(range.duration)
        .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
    let start_ticks = context.frame_ticks(range.start, "activeRange.start")?;
    let end_ticks = context.picture_end_ticks(end, None)?;
    ensure!(
        end_ticks > start_ticks,
        "activeRange {}..{} ms collapses to zero duration on the {} sequence grid",
        range.start.as_millis(),
        end.as_millis(),
        context.frame_rate
    );
    Ok(start_ticks..end_ticks)
}

/// A layer that exports as one Premiere clip.
#[derive(Clone, Copy)]
pub(super) enum ClipLayer<'a> {
    Video(&'a VideoLayer),
    Image(&'a ImageLayer),
    Text(&'a TextLayer),
    Shape(&'a ShapeLayer),
}

/// The layer fields that no Premiere clip carries, each with whether `layer`
/// sets it, in report order ("<detail> was not exported"). A parent other
/// than `group`, whose nested sequence receives the clip, is reported. Masks
/// or a track matte that the clip exports as its Crop, Linear Wipe, Opacity
/// mask or Track Matte Key (`masks_exported`) are not reported, nor are video,
/// image, text and shape effects, which `effects::export_effects` and
/// `text_shadow` write or report one by one.
pub(super) fn unexported_layer_fields(
    layer: ClipLayer<'_>,
    group: Option<LayerId>,
    masks_exported: bool,
) -> [(bool, ExportField); 11] {
    use ClipLayer::{Image, Shape, Text, Video};
    let (description, parent, track_matte) = match layer {
        Video(l) => (&l.description, l.parent, &l.track_matte),
        Image(l) => (&l.description, l.parent, &l.track_matte),
        Text(l) => (&l.description, l.parent, &l.track_matte),
        Shape(l) => (&l.description, l.parent, &l.track_matte),
    };
    let (masks, motion_blur) = match layer {
        Video(l) => (&l.masks, l.motion_blur),
        Image(l) => (&l.masks, l.motion_blur),
        Text(l) => (&l.masks, l.motion_blur),
        Shape(l) => (&l.masks, l.motion_blur),
    };
    let (corner_radius, placement, captions) = match layer {
        Video(l) => (l.corner_radius, l.placement.is_some(), l.captions_enabled),
        Image(l) => (l.corner_radius, l.placement.is_some(), l.captions_enabled),
        Text(_) | Shape(_) => (None, false, None),
    };
    let (metadata, audio_pitch, caption_presentation) = match layer {
        Video(l) => (&l.metadata, l.preserve_audio_pitch, &l.caption_presentation),
        Image(_) | Text(_) | Shape(_) => (&None, false, &None),
    };
    [
        (!description.is_empty(), ExportField::Description),
        (metadata.is_some(), ExportField::Metadata),
        (parent != group, ExportField::ParentGrouping),
        (
            track_matte.is_some() && !masks_exported,
            ExportField::TrackMatte,
        ),
        (!masks.is_empty() && !masks_exported, ExportField::Masks),
        (corner_radius.is_some(), ExportField::CornerRadius),
        (audio_pitch, ExportField::AudioPitchPreservation),
        (placement, ExportField::Placement),
        (
            caption_presentation.is_some(),
            ExportField::CaptionPresentation,
        ),
        (motion_blur, ExportField::MotionBlur),
        (captions == Some(true), ExportField::Captions),
    ]
}

/// Report the blend mode of an object layer inside a graphic group, which
/// exports blending normally: a Premiere graphic blends as a whole (the
/// graphic of one ungrouped layer carries that layer's blend mode).
pub(super) fn omit_object_blend(
    layer: LayerId,
    blend_mode: BlendMode,
    omissions: &mut dyn OmissionSink,
    record: &str,
) {
    if blend_mode != BlendMode::Normal {
        omit_field(
            omissions,
            layer,
            ExportField::BlendMode,
            record,
            "unsupported blend mode was not exported (using normal)",
        );
    }
}

/// Map one FX text layer, a root layer or a layer of the root graphic group
/// `group`, to a native graphic text from its current values.
///
/// Layer properties that video layers also omit are reported as features. An
/// error means the text itself cannot become a single-style graphic.
pub(super) fn text_object(
    layer: &TextLayer,
    group: Option<LayerId>,
    font: String,
    dynamics: &AnimationGraph,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<PrText> {
    let transform = &layer.transform;
    let shared = unexported_layer_fields(ClipLayer::Text(layer), group, false);
    for (changed, detail) in shared.into_iter().chain([
        (
            transform.skew != 0.0 || transform.skew_axis != 0.0,
            ExportField::Skew,
        ),
        (
            transform.rotation_x != 0.0
                || transform.rotation_y != 0.0
                || transform.orientation != [0.0; 3]
                || transform.position.z().is_some(),
            ExportField::Rotation3d,
        ),
    ]) {
        if changed {
            omit_field(
                omissions,
                layer.id,
                detail,
                record,
                format!("{detail} was not exported"),
            );
        }
    }
    omit_object_blend(layer.id, layer.blend_mode, omissions, record);
    if layer.path_options.is_some() || layer.anchor_options.is_some() {
        return Err(unsupported(
            "text path options and anchor options are unsupported",
        ));
    }
    let source_text = super::graphic::stroke_width_source(layer, dynamics)?;
    let (document, box_origin) = text_document(&source_text, font)?;
    Ok(PrText {
        horizontal_scale: (transform.scale[0] != transform.scale[1]).then_some(transform.scale[0]),
        mask_source: None,
        name: layer.name.clone(),
        document,
        // Premiere's box starts at the layer origin, so the FX box
        // offset moves into the anchor; the rendered placement is unchanged.
        transform: PrTextTransform {
            position: [transform.position.x(), transform.position.y()],
            anchor: [
                transform.anchor_point[0] - box_origin[0],
                transform.anchor_point[1] - box_origin[1],
            ],
            scale: transform.scale[1],
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        },
        animations: Vec::new(),
        source_text_keys: Vec::new(),
    })
}

/// Map one FX text document, with the PostScript `font` of its family and
/// style, to a native Source Text document and the origin of its box, or
/// the reason it cannot become single-style graphic text.
pub(super) fn text_document(
    doc: &TextDocument,
    font: String,
) -> Result<(PrTextDocument, [f64; 2])> {
    if doc.font_variations.is_some()
        || doc.baseline_shift != 0.0
        || doc.underline
        || doc.strikethrough
        || (doc.apply_stroke && doc.stroke_over_fill)
    {
        return Err(unsupported(
            "text font variations, baseline shift, underline, strikethrough and stroke over fill are unsupported",
        ));
    }
    let native_size = native_float(doc.font_size.value(), "font size")?;
    let fill = doc
        .apply_fill
        .then(|| rgb(doc.fill_color, "text fill"))
        .transpose()?;
    let stroke = match (doc.apply_stroke, doc.stroke_color) {
        // The FX renderer draws no stroke without a color.
        (true, Some(color)) => Some(PrTextStroke {
            color: rgb(color, "text stroke")?,
            width: native_float(
                doc.stroke_width.value() / STROKE_WIDTH_RATIO,
                "stroke width",
            )?,
        }),
        _ => None,
    };
    // Import approximates native outside-only paint. Export keeps its existing
    // narrower contract rather than claiming that approximation round-trips.
    ensure!(
        fill.is_some() || stroke.is_none(),
        "outline-only text (stroke without fill) is unsupported"
    );
    let leading = match &doc.leading {
        None => 0.0,
        Some(leading) => native_float(
            leading.value() - automatic_line_spacing(native_size),
            "leading",
        )?,
    };
    // A box needs both its size and position; otherwise the FX renderer draws point text.
    let (frame, box_origin) = match (doc.box_text, doc.box_size, doc.box_position) {
        (true, Some([box_width, box_height]), Some(origin)) => {
            if doc.box_first_baseline.is_some() {
                return Err(unsupported(
                    "box text with an authored first baseline is unsupported",
                ));
            }
            let vertical = match (doc.vertical_align, &doc.leading) {
                (Some(VerticalAlign::Top), _) | (None, None) => PrVerticalAlign::Top,
                (Some(VerticalAlign::Center), _) => PrVerticalAlign::Center,
                (Some(VerticalAlign::Bottom), _) => PrVerticalAlign::Bottom,
                (None, Some(_)) => {
                    return Err(unsupported(
                        "box text with explicit leading needs an explicit vertical alignment",
                    ))
                }
            };
            (
                PrTextFrame::Box {
                    width: native_float(box_width, "box width")?,
                    height: native_float(box_height, "box height")?,
                    vertical,
                },
                origin,
            )
        }
        _ => (
            PrTextFrame::Point {
                vertical: PrVerticalAlign::Top,
            },
            [0.0, 0.0],
        ),
    };
    Ok((
        PrTextDocument {
            text: normalize_line_breaks(&doc.text),
            font,
            size: native_size,
            fill,
            stroke,
            shadow: None,
            all_caps: doc.all_caps,
            tracking: native_float(doc.tracking, "tracking")?,
            leading,
            justification: match doc.justification {
                Justification::Left => PrJustification::Left,
                Justification::Right => PrJustification::Right,
                Justification::Center => PrJustification::Center,
                Justification::Justify => PrJustification::Justify,
            },
            frame,
            background: None,
        },
        box_origin,
    ))
}

/// A graphic of `objects` over `range`, where export places every graphic:
/// at the generator in-point of its rate.
pub(super) fn graphic_of(
    objects: Vec<PrGraphicObject>,
    range: &TimeRangeProperty,
    enabled: bool,
    context: &LayerExport<'_, '_>,
) -> Result<PrGraphic> {
    let active = active_range_ticks(range, context)?;
    Ok(PrGraphic {
        id: None,
        start_ticks: active.start,
        end_ticks: active.end,
        in_ticks: context.frame_rate.generator_in_ticks(),
        vector_motion: None,
        clip_motion: PrStaticTransform::default(),
        opacity: 100.0,
        blend_mode: PrBlendMode::Normal,
        animations: Vec::new(),
        opacity_mask: None,
        effect_loss: None,
        objects,
        enabled,
    })
}

/// Premiere text and shape paints are opaque 8-bit RGB; channels round to
/// the nearest step.
pub(super) fn rgb(color: [f64; 4], paint: &str) -> Result<PrRgb> {
    if color[3] != 1.0 || !color.iter().all(|channel| (0.0..=1.0).contains(channel)) {
        return Err(unsupported(format!(
            "{paint} must be an opaque color with channels in 0..=1"
        )));
    }
    Ok(PrRgb(
        [color[0], color[1], color[2]].map(|channel| (channel * 255.0).round() as u8),
    ))
}

/// Premiere stores text measurements as 32-bit floats.
fn native_float(value: f64, field: &str) -> Result<f32> {
    let narrowed = value as f32;
    if !narrowed.is_finite() {
        return Err(unsupported(format!(
            "text {field} is outside Premiere's range"
        )));
    }
    Ok(narrowed)
}

#[cfg(test)]
#[path = "tests/tesseract_to_premiere.rs"]
mod tests;
