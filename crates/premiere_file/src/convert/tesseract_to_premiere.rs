//! Map one shared editable document and its inspected media facts to sequence semantics.
use super::{
    background::{black_shape, identity_transform},
    effects,
    graphic::{premiere_path, scaled_path},
    keyframes,
    nested::{nest_candidate, LayerExport},
    packing::{PicturePacker, PicturePackingRecipe},
    premiere_to_tesseract::wipe_frame_reason,
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
            normalize_line_breaks, PrGraphicObject, PrJustification, PrRgb, PrTextDocument,
            PrTextFrame, PrTextStroke, PrTextTransform, PrVerticalAlign,
        },
        PrAnimatedProperty, PrBlendMode, PrKeyframeEasing, PrLinearWipe, PrMask, PrMatteChannel,
        PrMediaKind, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe, PrStaticCrop,
        PrStaticTransform, PrText, PrTrackMatte, PrVideoStream, PrVideoTrack, MOTION_PARAMS,
        TRANSFORM_SHUTTER_ANGLE,
    },
    {approximate, omit, Omission, OmissionScope},
};
use fx_schema::{
    animator::{
        AnimationGraph, AnimationGraphEntry, AnimatorData, PropertyKeyframeEasing,
        PropertyKeyframeTrack,
    },
    BlendMode, EditableFxCompositionDocument, FontAssetProperties, GroupLayer, ImageLayer,
    Justification, Layer, LayerData, LayerId, MaskMode, MediaSourceKind, PathMask, PropType,
    PropertyTarget, PropertyValue, RectLayer, ShapeLayer, TextDocument, TextLayer, Time,
    TimeRangeProperty, TimeRemapExtrapolation, TimeRemapProperty, TrackMatte, TrackMatteType,
    Transform, VerticalAlign, VideoLayer,
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
fn constant_playback_source_range(
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
            != i128::from(active_range.start.as_millis()) + i128::from(input_offset_ms)
        || i128::from(keys[1].time.as_millis())
            != i128::from(active_range.end().as_millis()) + i128::from(input_offset_ms)
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
    Ok((
        TimeRangeProperty::new(start, end.saturating_sub(start)),
        keys[1].value < keys[0].value,
    ))
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
    for key in track.keyframes() {
        let PropertyValue::Float(value) = key.value() else {
            return Err(unsupported(format!(
                "{property} keys must have float values"
            )));
        };
        let source_ticks = keyframes::source_ticks(source_in, key.layer_time().as_millis())?;
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
        let easing = keyframes::native_easing(key.easing())?;
        keys.push(PrScalarKeyframe {
            source_ticks,
            value: *value,
            easing,
        });
    }
    for pair in keys.windows(2) {
        if matches!(pair[1].easing, PrKeyframeEasing::CubicBezier { .. })
            && pair[0].value == pair[1].value
        {
            return Err(unsupported(format!(
                "{property} cubic easing between equal values cannot preserve Premiere velocity"
            )));
        }
    }
    // A key that starts a Hold is written with Hold (mode 4), whose in-handle
    // Premiere ignores; the first key has no arrival and the last starts none.
    if keys.windows(3).any(|keys| {
        keys[2].easing == PrKeyframeEasing::Hold
            && !keyframes::premiere_keeps_the_arrival_into_a_hold(keys[1].easing)
    }) {
        return Err(unsupported(format!(
            "{property} cubic easing into a key that starts a Hold cannot keep its in-handle, which Premiere ignores"
        )));
    }
    for omission in key_omissions {
        omissions.emit(omission);
    }
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
    Ok(PrLinearWipe {
        initial_completion: 100.0 - default_visible,
        completion,
        angle_degrees: wipe.angle_degrees,
        feather: wipe.feather,
    })
}

fn source_media<'a>(media: &'a mut BTreeMap<MediaId, PrMedia>, id: &MediaId) -> &'a mut PrMedia {
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
    /// A mask whose guide is a shape layer: Premiere's mask on the clip's
    /// intrinsic Opacity, which applies after every effect, as an FX mask on
    /// the stage group over the video does. FX applies a video's own mask
    /// before its effects, so a flat video exports one only without effects.
    Opacity {
        mask: PrMask,
        guide_id: LayerId,
    },
    /// A track matte whose source, a video or still beside the clip or under
    /// its stage group, exports as a clip that the export places on a track
    /// above every clip it keys ([`UNPLACED_MATTE_TRACK`]).
    TrackMatte {
        source: LayerId,
        channel: PrMatteChannel,
    },
}

impl CanonicalMask {
    /// The guide that draws the mask and paints nothing. A track matte source
    /// is a clip of its own, not a guide.
    pub(super) fn guide_id(&self) -> Option<LayerId> {
        match self {
            Self::Crop { guide_id, .. } | Self::Opacity { guide_id, .. } => Some(*guide_id),
            Self::LinearWipe(wipe) => Some(wipe.guide_id),
            Self::TrackMatte { .. } => None,
        }
    }
}

/// The matte track of a keyed clip until export places its source, which no
/// sequence has: the shared timeline validation rejects it, so a clip never
/// exports without its matte.
const UNPLACED_MATTE_TRACK: usize = usize::MAX;

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

/// The FX layers that export as one Premiere clip.
#[derive(Clone, Copy)]
pub(super) enum ClipLayers<'a> {
    /// A video layer with its own mask.
    Video(&'a VideoLayer),
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
            Self::Stage { group, .. } | Self::Nest(group) => group.id,
        }
    }

    fn transform(self) -> &'a Transform {
        match self {
            Self::Video(video) => &video.transform,
            Self::Stage { group, .. } | Self::Nest(group) => &group.transform,
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
            Self::Stage { group, .. } | Self::Nest(group) => group.playback.input_range(),
        }
    }

    fn is_hidden(self) -> bool {
        match self {
            Self::Video(video) => video.is_hidden,
            Self::Stage { group, .. } | Self::Nest(group) => group.is_hidden,
        }
    }

    fn blend_mode(self) -> BlendMode {
        match self {
            Self::Video(video) => video.blend_mode,
            Self::Stage { group, .. } | Self::Nest(group) => group.blend_mode,
        }
    }
}

/// The name prefix of a rectangle that import creates as the guide of a
/// Linear Wipe. Script preparation reads it to choose the guide's key
/// bindings; export selects Crop or Linear Wipe by the guide's shape.
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

/// The name prefix of the stage group that import builds for a clip's Crop,
/// Linear Wipe, Opacity mask or Transform. It tells a Transform stage from a
/// Premiere nest of one moved clip, which imports to the same shape and keeps
/// exporting as a nest; a renamed or editor-authored group is a nest.
pub(super) const STAGE_GROUP_NAME: &str = "Premiere stage ";

/// The one video child of an imported stage group ([`STAGE_GROUP_NAME`])
/// without masks, which exports as one clip whose Transform effect is the
/// video's transform when `unsupported_stage` accepts it, and as a nest
/// otherwise. Any other group exports as a nest.
pub(super) fn transform_stage_layers(group: &GroupLayer) -> Option<&Layer> {
    if !group.masks.is_empty() || !group.name.starts_with(STAGE_GROUP_NAME) {
        return None;
    }
    let [video] = group.layers.as_slice() else {
        return None;
    };
    match video.data() {
        LayerData::Video(_) => Some(video),
        LayerData::Media(media) if media.source.kind == MediaSourceKind::Video => Some(video),
        _ => None,
    }
}

/// The video that a group exports as one clip of, when the group has a stage
/// shape: a mask stage ([`stage_layers`]) or a Transform stage
/// ([`transform_stage_layers`]).
pub(super) fn stage_video(group: &GroupLayer) -> Option<&Layer> {
    stage_layers(group).or_else(|| transform_stage_layers(group))
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
        (group.motion_blur, "the group has motion blur"),
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
        // record again from its source In.
        let frame = source_frame(video).map_err(|error| error.to_string());
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
enum Guide<'a> {
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

    /// Why the guide cannot be one Premiere mask's outline beside the layer
    /// `noun`, whose `parent` and `range` it must share, if it cannot: every
    /// field but its geometry and transform must be neutral.
    fn unsupported(
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
/// video clip; a still exports no mask ([`unsupported_image_masks`]). A clip
/// whose mask it rejects is omitted whole, so no clip exports without its
/// mask. A video layer's guide is its sibling in `layers`, in the parent's
/// space; a stage or nest group's guide is its direct child in the clip's
/// frame. A track matte source is the sibling of a video or nest group, or
/// the direct child of a stage group ([`canonical_track_matte`]). `frame` is
/// the video's source frame, or a nest's canvas. The check reads every
/// authored animation, including those that export omits.
pub(super) fn canonical_mask(
    clip: ClipLayers<'_>,
    frame: [u32; 2],
    layers: &[Layer],
    dynamics: &AnimationGraph,
    width: u32,
    height: u32,
) -> std::result::Result<Option<CanonicalMask>, String> {
    let (masks, guides, track_matte) = match clip {
        ClipLayers::Video(video) => (&video.masks, layers, &video.track_matte),
        ClipLayers::Stage { group, .. } | ClipLayers::Nest(group) => {
            (&group.masks, group.layers.as_slice(), &group.track_matte)
        }
    };
    // The layer, named in the reasons, whose parent and range the guide
    // shares: the video, or a nest's group, over its whole range.
    let (noun, parent, range) = match clip {
        ClipLayers::Video(video) | ClipLayers::Stage { video, .. } => {
            ("video", video.parent, video.playback.input_range())
        }
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
    let mask = match masks.as_slice() {
        [] => return Ok(None),
        [mask] => mask,
        _ => return Err("several masks are not one Crop, Linear Wipe or Opacity mask".to_owned()),
    };
    let unsupported_mask = [
        (mask.mode != MaskMode::Add, "the mask mode is not Add"),
        (mask.legacy_path.is_some(), "the mask has an inline path"),
        (mask.expansion != 0.0, "the mask has an expansion"),
        (
            mask.feather[0] != mask.feather[1],
            "the mask feather differs between its axes",
        ),
        (
            dynamics
                .entries()
                .iter()
                .any(|entry| entry.target.fx_item_id() == Some(mask.id)),
            "the mask has keys",
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
    if let Some(reason) = guide.unsupported(noun, parent, range) {
        return Err(reason);
    }
    // A flat guide has its video's transform and the keys that move the
    // video's frame, and no other keys, so that the mask stays in the video's
    // frame at every time; a staged or nested guide is in the clip's frame.
    // Key ids differ. `label` names the guide in the reason.
    let in_video_frame = |guide: Guide<'_>, label: &str| match clip {
        ClipLayers::Video(video) => {
            let same_keys = match (
                layer_tracks(dynamics, guide.id(), |_| true),
                layer_tracks(dynamics, video.id, |property| {
                    FRAME_PROPERTIES.contains(&property)
                }),
            ) {
                (Some(guide), Some(video)) => {
                    guide.len() == video.len()
                        && guide.iter().all(|(property, guide)| {
                            video
                                .get(property)
                                .is_some_and(|video| scale_tracks_match(guide, video))
                        })
                }
                _ => false,
            };
            (guide.transform() == &video.transform && same_keys)
                .then_some(())
                .ok_or_else(|| {
                    format!("{label}'s transform or Motion keys differ from its video's")
                })
        }
        ClipLayers::Stage { .. } | ClipLayers::Nest(_) => {
            if guide.transform() != &identity_transform() {
                return Err(format!("{label} under the group is not at the identity"));
            }
            if layer_animations(dynamics, guide.id()).next().is_some() {
                return Err(format!("{label} under the group has keys"));
            }
            Ok(())
        }
    };
    let guide = match guide {
        Guide::Rect(guide) => guide,
        Guide::Shape(guide) => {
            match clip {
                ClipLayers::Nest(_) => {
                    return Err("an Opacity mask on a nested sequence is not converted".to_owned());
                }
                // The writer places Opacity after every effect, where Premiere
                // applies its mask; FX applies the video's own mask before
                // its effects, so only a stage group carries a mask over them.
                ClipLayers::Video(video) if !video.effects.is_empty() => {
                    return Err("the video has effects, which FX applies after its mask and Premiere before an Opacity mask; the mask needs a stage group".to_owned());
                }
                ClipLayers::Video(_) | ClipLayers::Stage { .. } => {}
            }
            in_video_frame(Guide::Shape(guide), "the Opacity mask guide")?;
            return canonical_opacity_mask(mask, guide, frame).map(Some);
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
            return Err("the video does not map its frame onto the canvas unchanged, so its Linear Wipe guide is not its frame".to_owned());
        }
        return Ok(Some(CanonicalMask::LinearWipe(wipe)));
    }
    // FX rounds the mask outline by the guide's roundness; a Crop is square.
    if guide.rect.roundness != 0.0 {
        return Err("the Crop guide has rounded corners".to_owned());
    }
    // Unkeyed, a guide and video that only translate may differ in anchor and
    // position: FX offsets each by position minus anchor.
    let origin = match (in_video_frame(Guide::Rect(guide), "the Crop guide"), clip) {
        (Ok(()), _) => guide.rect.position,
        (Err(_), ClipLayers::Video(video))
            if layer_animations(dynamics, guide.id).next().is_none()
                && only_translates(&guide.transform, guide.id, dynamics)
                && only_translates(&video.transform, video.id, dynamics) =>
        {
            let ([x, y], g, v) = (guide.rect.position, &guide.transform, &video.transform);
            [
                x + g.position.x() - g.anchor_point[0] - v.position.x() + v.anchor_point[0],
                y + g.position.y() - g.anchor_point[1] - v.position.y() + v.anchor_point[1],
            ]
        }
        (Err(reason), _) => return Err(reason),
    };
    guide_crop(guide, origin, frame, mask.feather[0])
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
/// opacity and inversion. A guide's paints are not drawn and not checked.
fn canonical_opacity_mask(
    mask: &PathMask,
    guide: &ShapeLayer,
    frame: [u32; 2],
) -> std::result::Result<CanonicalMask, String> {
    let content = &guide.shape;
    if content.round_corners.is_some()
        || content.offset_paths.is_some()
        || content.trim.is_some()
        || content.poly_star.is_some()
        || content.ellipse.is_some()
    {
        return Err("the Opacity mask guide has a path modifier or primitive".to_owned());
    }
    let [width, height] = frame.map(f64::from);
    let path = premiere_path(&scaled_path(&content.path, [1.0 / width, 1.0 / height]))
        .map_err(|error| format!("the Opacity mask guide path cannot be exported: {error}"))?;
    let mask = PrMask {
        path,
        feather: mask.feather[0],
        opacity: mask.opacity.value() * 100.0,
        inverted: mask.inverted,
    };
    mask.validate().map_err(|error| error.to_string())?;
    Ok(CanonicalMask::Opacity {
        mask,
        guide_id: guide.id,
    })
}

/// The Track Matte Key that `matte` on `clip` exports as, or why the clip is
/// omitted. Its source is a video that itself exports as a clip
/// ([`clip_omitted`]) or a still that exports with its coverage whole
/// ([`super::still::unsupported_matte_still`]; a still as content accepts
/// the loss of its effects, a matte does not, and a matte still stays at its
/// defaults), the two exporters whose omission is decided before they run: beside a
/// video or nest group in
/// `layers` over the clip's range, or the direct child of a stage group over
/// the group's range, not hidden, with no track matte or masks of its own.
/// Premiere keys the clip's sequence-sized source frame by the matte track's
/// sequence-sized output, so the source must be that frame
/// ([`sequence_sized_layer_is_clip_frame`]): a sibling source of a moved or
/// keyed flat clip stays fixed in FX while Premiere's Motion would move the
/// keyed picture (fixture G5), and only a stage group carries that
/// relationship. A Color Matte, graphic or nest source is not exported.
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
    let channel = match matte.mode {
        TrackMatteType::Alpha => PrMatteChannel::Alpha,
        TrackMatteType::AlphaInverted => PrMatteChannel::AlphaInverted,
        TrackMatteType::Luma => PrMatteChannel::Luma,
        TrackMatteType::LumaInverted => {
            return Err("the track matte mode is lumaInverted, which Premiere's Reverse with Matte Luma does not render: it gives the matte clip's zero-luma exterior full coverage where FX gives none".to_owned());
        }
    };
    // The list that holds the source and the parent and range it must have.
    let (siblings, parent, range) = match clip {
        ClipLayers::Video(video) => (layers, video.parent, video.playback.input_range()),
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
        _ => {
            return Err(format!(
            "the track matte source is a {} layer; only a video or still image source is exported",
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
            if matches!(clip, ClipLayers::Stage { .. }) && video.playback.time_remap().is_some() =>
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
        // Every other layer kind returned above.
        _ => None,
    };
    if let Some(reason) = unexported_source {
        return Err(reason);
    }
    Ok(CanonicalMask::TrackMatte {
        source: matte.layer,
        channel,
    })
}

/// The source frame and mask of a clip that export writes, or why export
/// omits the clip whole.
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
        ClipLayers::Video(video) | ClipLayers::Stage { video, .. } => source_frame(video)?,
        ClipLayers::Nest(_) => [width, height],
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
        || matches!(clip_mask(clip, layers, dynamics, canvas), Ok(Err(_)))
}

/// Why export omits the clip whose Motion `motion` hosts whole for a Scale or
/// Rotation that no native clip shows unchanged, if it does; a video, a group
/// and a still take the same rule. Premiere shows a Scale or Rotation
/// outside its Motion parameter's bounds ([`MOTION_PARAMS`]) as another
/// picture, so each static value, and each value that an authored constant or
/// keyframe animation takes, between its keys too ([`track_range`]) and also
/// in one that export omits, must lie inside them; a negative
/// Scale is a flip, which Motion cannot write.
pub(super) fn unexportable_motion(
    motion: MotionHost<'_>,
    dynamics: &AnimationGraph,
) -> Option<&'static str> {
    const SCALE: &[PropType] = &[PropType::ScaleX, PropType::ScaleY];
    let transform = motion.transform;
    let animated = |properties: &'static [PropType]| {
        layer_animations(dynamics, motion.id)
            .filter(move |(property, _)| properties.contains(property))
            .flat_map(|(_, entry)| {
                let curve = entry.animator.keyframe_track().and_then(track_range);
                let values = entry.animator.finite_value_range().unwrap_or_default();
                values
                    .into_iter()
                    .filter_map(|value| match value {
                        PropertyValue::Float(value) => Some(*value),
                        _ => None,
                    })
                    .chain(curve.into_iter().flatten())
            })
    };
    let holds = |property: PrAnimatedProperty, value: f64| {
        MOTION_PARAMS
            .iter()
            .any(|spec| spec.animation == Some(property) && spec.holds(value))
    };
    let scale = |value| holds(PrAnimatedProperty::UniformScale, value);
    let rotation = |value| holds(PrAnimatedProperty::Rotation, value);
    if transform
        .scale
        .into_iter()
        .chain(animated(SCALE))
        .any(|value| value < 0.0)
    {
        Some("flip (negative scale) was not exported")
    } else if !transform.scale.into_iter().all(scale) {
        Some("static scale exceeds Premiere's supported range")
    } else if !animated(SCALE).all(scale) {
        Some("Scale animation exceeds Premiere's supported range")
    } else if !rotation(transform.rotation) {
        Some("static rotation exceeds Premiere's supported range")
    } else if !animated(&[PropType::Rotation]).all(rotation) {
        Some("Rotation animation exceeds Premiere's supported range")
    } else {
        None
    }
}

/// Why export omits the clip that `clip` exports as whole for a value that no
/// native clip shows unchanged, if it does: a Scale or Rotation that its
/// Motion cannot show ([`unexportable_motion`]), or a playback that native
/// speed cannot carry. Native constant speed and
/// reverse carry only a bounded two-key linear playback
/// ([`constant_playback_source_range`]), so any other playback would show other
/// frames at unit speed, as would a `source.timeRemap` without playback whose
/// source range is longer or shorter than the active range: FX does not render
/// the remap and plays that range stretched. A stage group that this rejects
/// exports as a nest, whose placement or clip it rejects in turn.
pub(super) fn unexportable_clip(
    clip: ClipLayers<'_>,
    dynamics: &AnimationGraph,
) -> Option<&'static str> {
    if let Some(reason) = unexportable_motion(clip.motion(), dynamics) {
        Some(reason)
    } else if let ClipLayers::Video(video) | ClipLayers::Stage { video, .. } = clip {
        let remapped = match video.playback.time_remap() {
            Some(playback) => constant_playback_source_range(
                playback,
                video.playback.input_range(),
                video.source_range,
                video.playback.input_offset_ms(),
            )
            .is_err(),
            None => {
                video.source.time_remap.is_some()
                    && video.source_range.duration != video.playback.input_range().duration
            }
        };
        remapped.then_some(
            "time remapping was not exported: Premiere export writes constant speed only",
        )
    } else {
        None
    }
}

/// The least and greatest value that the keys of `track` take at any time:
/// their values, and where a cubic Bézier easing overshoots them between two
/// ([`scalar_range`]). `None` when the keys are not floats.
fn track_range(track: &PropertyKeyframeTrack) -> Option<[f64; 2]> {
    let keys = track
        .keyframes()
        .iter()
        .map(|key| {
            let PropertyValue::Float(value) = key.value() else {
                return None;
            };
            let bezier = match key.easing() {
                PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => Some([x1, y1, x2, y2]),
                PropertyKeyframeEasing::Hold | PropertyKeyframeEasing::Linear => None,
            };
            Some((*value, bezier))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(scalar_range(&keys))
}

/// Why export omits the still `image` whole for its masks or track matte, if
/// it does: a still exports no Crop, Linear Wipe or Track Matte Key, so an
/// image never exports without its mask. Export and media inspection share
/// this decision, as they share [`clip_mask`] for a video clip.
pub(super) fn unsupported_image_masks(image: &ImageLayer) -> Option<&'static str> {
    (!image.masks.is_empty() || image.track_matte.is_some())
        .then_some("a still image exports no Crop, Linear Wipe or Track Matte Key")
}

/// The Crop that a guide rectangle at `origin` in the video's frame draws: its edges as
/// percentages of `frame`.
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

/// The Scale track of `guide` when its shape, not its name, is a Linear Wipe
/// guide's: the black sequence-sized rectangle whose only animation is one
/// ScaleX or ScaleY key track that, for a flat guide, its video does not have
/// (a flat Crop guide repeats its video's keys).
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
        ClipLayers::Video(video) => layer_tracks(dynamics, video.id, |other| other == property)
            .and_then(|tracks| tracks.get(&property).copied())
            .is_some_and(|other| scale_tracks_match(track, other)),
        ClipLayers::Stage { .. } | ClipLayers::Nest(_) => false,
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

/// The integer source frame of a video layer, whose `sourceRect` must start
/// at the source origin.
pub(super) fn source_frame(video: &VideoLayer) -> Result<[u32; 2]> {
    let rect = video
        .source
        .frame_rect
        .ok_or_else(|| unsupported("sourceRect must cover the exact canvas"))?
        .get();
    if rect.x != 0.0 || rect.y != 0.0 {
        return Err(unsupported("sourceRect must start at the source origin"));
    }
    let side = |value: f64, name: &str| {
        u32::try_from(value as u64)
            .ok()
            .filter(|side| f64::from(*side) == value && *side > 0)
            .ok_or_else(|| unsupported(format!("sourceRect {name} must be a positive integer")))
    };
    Ok([side(rect.width, "width")?, side(rect.height, "height")?])
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
}

impl WrittenAnimation {
    /// Commits child evidence only after its containing placement is retained.
    pub(super) fn append(&mut self, child: &mut Self) {
        self.properties.append(&mut child.properties);
        self.effects.append(&mut child.effects);
    }

    /// Whether the exported project writes the keys of `target`.
    pub(crate) fn contains(&self, target: &PropertyTarget) -> bool {
        match target {
            PropertyTarget::LayerProperty(property) => self
                .properties
                .contains(&(property.layer_id(), property.property_type())),
            PropertyTarget::EffectProperty(target) => self.effects.contains(&target.effect_id()),
            PropertyTarget::FxItemProperty(_) => false,
        }
    }

    pub(super) fn record(&mut self, layer: LayerId, property: PropType) {
        self.properties.insert((layer, property));
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
    /// keys, and a Crop guide's frame tracks, which [`canonical_mask`] proved
    /// equal to the frame tracks of the placement's Motion.
    pub(super) fn record_placement(
        &mut self,
        owner: LayerId,
        animations: &[PrPropertyAnimation],
        mask: Option<CanonicalMask>,
        linear_wipe: Option<&PrLinearWipe>,
    ) {
        self.record_animations(owner, animations);
        match mask {
            Some(CanonicalMask::LinearWipe(wipe)) if linear_wipe.is_some() => {
                self.record(wipe.guide_id, wipe.property_type);
            }
            Some(CanonicalMask::Crop { guide_id, .. }) => {
                for property in FRAME_PROPERTIES {
                    if self.properties.contains(&(owner, property)) {
                        self.record(guide_id, property);
                    }
                }
            }
            // Opacity masks are static and do not write guide animation tracks,
            // and a track matte source is a clip of its own.
            Some(
                CanonicalMask::Opacity { .. }
                | CanonicalMask::LinearWipe(_)
                | CanonicalMask::TrackMatte { .. },
            )
            | None => {}
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
    pub(crate) canvas: Option<std::ops::Range<i64>>,
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
    for sequence in project.sequences() {
        super::background::validate_gap_coverage(
            sequence,
            &project.media,
            lowered.canvas.as_ref(),
        )?;
    }
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
        media_facts,
        audio_facts,
        fonts,
        frame_rate,
        omissions,
        fx_conv::Progress::default(),
    )
}

pub(crate) fn lower_document_with_progress(
    document: &EditableFxCompositionDocument,
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
    let keyed_text_layer_ids = keyed_text_layer_ids(composition.layers(), composition.dynamics());
    let anchor_point_layer_ids = anchor_point_layer_ids(composition.layers());
    let dimensions = document.dimensions();
    // `effects::export_effects` reports an animated parameter with its effect.
    let video_effect_ids =
        effects::video_effect_ids(composition, [dimensions.width, dimensions.height]);
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
        // exporter's coherent held point-alignment check.
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
                && keyed_text_layer_ids.contains(&property.layer_id());
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
        super::background::validate_black_canvas(rect, width, height, frame_rate)
            .ok()
            .map(|range| (rect.id, range))
    } else {
        None
    };
    let mut media: BTreeMap<MediaId, PrMedia> = BTreeMap::new();
    let mut audio = Vec::new();
    let mut written = WrittenAnimation::default();
    let mut packer = PicturePacker::new(&sequence_name, [width, height], frame_rate, 0);
    let root_container = packer.root();
    let mut motion_blur_written = false;
    let phase = progress.phase("lowering Premiere layers", "root layers", layers.len());
    let completed = Cell::new(0);
    let video_tracks = export_layers_with_progress(
        layers,
        None,
        &mut LayerExport {
            dynamics: composition.dynamics(),
            property_tracks: &mut property_tracks,
            written: &mut written,
            media_facts,
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
            depth: 0,
            canvas: canvas.as_ref().map(|(id, _)| *id),
            group_guide: None,
            in_moved_nest: false,
            motion_blur: composition.motion_blur(),
            motion_blur_written: &mut motion_blur_written,
        },
        omissions,
        Some((phase, &completed)),
    )?;
    progress.stage("assemble Premiere document");
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
            canvas: canvas.map(|(_, range)| range),
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
    // Gap coverage belongs to the final picture, after any supplied AE scopes
    // replace unsupported native content. Native-only emission checks it too.
    Ok(LoweredDocument {
        project: Some(PrProjectFile::from_sequences(vec![sequence], media)),
        written,
        packing,
        canvas: canvas.map(|(_, range)| range),
    })
}

/// Takes a sound's volume keys. Its other tracks stay, so that they are
/// reported as animation that was not exported; an entry left empty is
/// dropped, so that a sound with only volume keys reports nothing.
fn take_volume_track<'d>(
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
    let mut mask_guides: BTreeSet<_> = context.group_guide.into_iter().collect();
    // A track matte source exports after the clips that it keys, on a track
    // above every one of them ([`MatteConsumers`]); FX never draws it as
    // content of its own, so one that no exported clip keys is omitted.
    let matte_sources = track_matte_sources(layers);
    let mut consumers = MatteConsumers::default();
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
                if occurrence.volume_keys.is_some() {
                    context.written.record(sound.id, PropType::AudioVolume);
                }
                source_media(context.media, &occurrence.media).audio = Some(facts.clone());
                context.audio.push(occurrence);
            }
            continue;
        }
        // A group of text and shape layers is a graphic whose Vector Motion is
        // the group transform, and a stage group exports as one clip of its
        // video, below; other groups are nested sequences.
        let stage = match layer.data() {
            LayerData::Group(group) => {
                if let Some(objects) = super::graphic::graphic_objects(group) {
                    if let Some(graphic) = super::graphic::export_graphic_group(
                        group, &objects, &consumed, context, omissions,
                    ) {
                        place_item(
                            &mut video_tracks,
                            PrVideoItem::Graphic(graphic),
                            min_track,
                            context,
                        )?;
                    }
                    continue;
                }
                match stage_video(group)
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
                        if let (Some(track), Some(source)) = (placed, group.track_matte.as_ref()) {
                            consumers.add_nest(&video_tracks, source.layer, track);
                        }
                        continue;
                    }
                }
            }
            _ => None,
        };
        if let LayerData::Image(image) = layer.data() {
            let record = format!("layer {} ({:?})", image.id, image.name);
            if let Some(reason) = unsupported_image_masks(image) {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    &record,
                    format!("masks cannot be exported: {reason}; occurrence omitted"),
                );
                mask_guides.extend(image.masks.iter().filter_map(|mask| mask.layer));
                continue;
            }
            if let Some(reason) = unexportable_motion(MotionHost::image(image), context.dynamics) {
                omit(omissions, OmissionScope::Occurrence, &record, reason);
                continue;
            }
            let exported =
                super::still::export_image_layer(image, parent, context, omissions, &record)
                    .map_err(|source| BuildError::Context {
                        context: record,
                        source: Box::new(source),
                    })?;
            if let Some((occurrence, source)) = exported {
                context
                    .media
                    .entry(occurrence.media.clone())
                    .or_insert(source);
                let track = place_item(
                    &mut video_tracks,
                    PrVideoItem::Media(occurrence),
                    min_track,
                    context,
                )?;
                consumers.place_source(&mut video_tracks, image.id, track, context)?;
            }
            continue;
        }
        if let LayerData::Adjustment(adjustment) = layer.data() {
            let record = format!("layer {} ({:?})", adjustment.id, adjustment.name);
            let exported =
                super::adjustment::export_adjustment_layer(adjustment, context, omissions, &record)
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
            if sole_guides.contains(&shape.id) {
                continue;
            }
        }
        if let LayerData::Rect(rect) = layer.data() {
            if mask_guide_ids.contains(&rect.id) || context.canvas == Some(rect.id) {
                continue;
            }
            let record = format!("layer {} ({:?})", rect.id, rect.name);
            let exported = super::color_matte::export_rect_layer(
                rect, parent, &consumed, context, omissions, &record,
            )
            .map_err(|source| BuildError::Context {
                context: record,
                source: Box::new(source),
            })?;
            if let Some(item) = exported {
                place_item(&mut video_tracks, item, min_track, context)?;
            }
            continue;
        }
        let record = format!("layer {} ({:?})", layer.id(), layer.name());
        let (clip, video) = match (super::graphic::ObjectLayer::of(layer), &stage) {
            (_, Some((group, video))) => (ClipLayers::Stage { group, video }, &**video),
            // A text or shape layer of the list is a graphic of that object.
            (Some(object), None) => {
                if let Some(graphic) = super::graphic::export_object(
                    object, parent, &consumed, context, omissions, &record,
                ) {
                    place_item(
                        &mut video_tracks,
                        PrVideoItem::Graphic(graphic),
                        min_track,
                        context,
                    )?;
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
        let unexportable = unexportable_clip(clip, context.dynamics);
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
        // placement carries the mask, and its video's keys stay for the nest.
        // A nest's track matte source is its sibling, and a stage group's is
        // its child, so a keyed stage group keeps its own reason instead.
        if let (true, Some(&(group, _))) = (
            accepted.is_err() || unexportable.is_some(),
            stage
                .as_ref()
                .filter(|(group, _)| group.track_matte.is_none() && nest_candidate(group)),
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
        let (occurrence, source) = export_video_clip(
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
        })?;
        context.written.record_placement(
            clip.motion_id(),
            &occurrence.animations,
            mask,
            occurrence.linear_wipe.as_ref(),
        );
        // Every layer of one asset validated against the same inspected facts.
        source_media(context.media, &occurrence.media).video = Some(source);
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
        let track = place_item(
            &mut video_tracks,
            PrVideoItem::Media(occurrence),
            min_track,
            context,
        )?;
        consumers.place_source(&mut video_tracks, layer.id(), track, context)?;
        if let Some(matte_source) = matte_source {
            consumers.add_item(&video_tracks, matte_source, track);
            // A stage group's source is its child; it exports beside the
            // group's clip, over the group's range, above it.
            if let Some((group, _)) = &stage {
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
fn export_video_clip(
    clip: ClipLayers<'_>,
    video: &VideoLayer,
    parent: Option<LayerId>,
    matte_source: bool,
    ([source_width, source_height], mask): ([u32; 2], Option<CanonicalMask>),
    context: &mut LayerExport<'_, '_>,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<(PrVideoOccurrence, PrVideoStream)> {
    use fx_schema::MediaFit;
    let (width, height, media_facts) = (context.width, context.height, context.media_facts);
    // A stage group is the parent of its video, whose mask fields are
    // neutral; the group's description is reported with the video's.
    let (group, masks_exported, group_description) = match clip {
        ClipLayers::Video(_) => (parent, mask.is_some(), false),
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
        (
            !matches!(source.fit, MediaFit::Contain | MediaFit::None),
            ExportField::MediaFit,
        ),
        (source.time_remap.is_some(), ExportField::TimeRemap),
        (
            source.audio_enhancement.is_some(),
            ExportField::AudioEnhancement,
        ),
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
    let (crop, opacity_mask) = match &mask {
        Some(CanonicalMask::Crop { crop, .. }) => (*crop, None),
        Some(CanonicalMask::Opacity { mask, .. }) => (PrStaticCrop::default(), Some(mask.clone())),
        Some(CanonicalMask::LinearWipe(_) | CanonicalMask::TrackMatte { .. }) | None => {
            (PrStaticCrop::default(), None)
        }
    };
    if opacity_mask
        .as_ref()
        .is_some_and(|mask| mask.feather != 0.0)
    {
        approximate(omissions, record, crate::schema::MASK_FEATHER_APPROXIMATION);
    }
    let track_matte = unplaced_track_matte(mask.as_ref());
    let mut native_transform = export_transform(
        clip.motion(),
        [source_width, source_height],
        [width, height],
        context.dynamics,
        record,
        omissions,
    );
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
    let (source_frame_rate, source_duration_ticks) = facts.timing.supported()?;
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
    ensure!(
        (facts.width, facts.height) == (source_width, source_height),
        "packaged source must be supported video matching sourceRect dimensions"
    );
    let ClipTicks {
        start_ticks: active_start_ticks,
        end_ticks: active_end_ticks,
        source_start_ticks,
        in_ticks,
        out_ticks,
        playback_rate,
    } = clip_ticks(clip, video, source_duration_ticks, context)?;
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
                    format!(
                        "sourceIntrinsicDuration {} ms is the floor of the packaged MP4 duration (nearest {intrinsic_millis} ms); native media retains its exact source clock",
                        video.source_intrinsic_duration.as_millis()
                    ),
                );
            },
        );
    }
    let mut tracks = context
        .property_tracks
        .remove(&clip.motion_id())
        .unwrap_or_default();
    let retimed_keys = retimed_keys_reason(playback_rate);
    if let Some(reason) = retimed_keys {
        for property in std::mem::take(&mut tracks).into_keys() {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "{property:?} animation was not exported: {reason}; static values were kept"
                ),
            );
        }
    }
    let (animations, _) = export_motion_keys(
        &mut tracks,
        source_start_ticks,
        &mut native_transform,
        [source_width, source_height],
        [width, height],
        record,
        omissions,
    );
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
    let mut exported_effects = effects::export_effects(
        &video.effects,
        context.dynamics,
        effects::EffectHost {
            layer: video.id,
            staged: matches!(clip, ClipLayers::Stage { .. }),
            nested: context.in_moved_nest,
            transform: t,
            source_in: source_start_ticks,
            frame: [source_width, source_height],
            canvas: [width, height],
        },
        context.written,
        record,
        omissions,
    );
    // A Transform stage's video ends the chain with its Transform, which
    // takes the video's keys (`unsupported_stage` accepted it): it writes
    // every track of the video, as `export_transform_stage` rejects others.
    if transform_stage {
        exported_effects.push(
            effects::export_transform_stage(
                video,
                context.dynamics,
                [source_width, source_height],
                source_start_ticks,
                shutter_angle,
                record,
                omissions,
            )
            .map_err(unsupported)?,
        );
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
    Ok((
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
            time_remap: None,
            linear_wipe,
            opacity_mask,
            track_matte,
            enabled: !clip.is_hidden(),
            // Under a mask stage, every effect applies before the mask; a
            // Transform stage has no mask.
            effects_above_mask: match clip {
                ClipLayers::Video(_) | ClipLayers::Nest(_) => 0,
                ClipLayers::Stage { .. } if transform_stage => 0,
                ClipLayers::Stage { .. } => exported_effects.len(),
            },
            active_transforms: u8::from(transform_stage),
            effects: exported_effects,
            stroke: None,
        },
        PrVideoStream {
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks: source_duration_ticks,
            frame_rate: (source_frame_rate).into(),
            width: facts.width,
            height: facts.height,
            kind: PrMediaKind::Video {
                codec: Some(facts.codec),
                hdr_profile: facts.hdr_profile(),
            },
        },
    ))
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
        ClipLayers::Stage { group, video } => {
            match super::timing::relocate_playback(&video.playback, group.playback.input_range()) {
                Ok(playback) => super::audio::embedded(
                    &VideoLayer {
                        playback,
                        is_hidden: group.is_hidden,
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
        ClipLayers::Nest(_) => None,
    } {
        if sound.volume_keys.is_some() {
            context.written.record(video.id, PropType::AudioVolume);
        }
        source_media(context.media, &sound.media).audio = Some(facts.clone());
        context.audio.push(sound);
    }
}

/// Where an exported clip plays, in Premiere ticks.
struct ClipTicks {
    /// The clip's range on the sequence frame grid.
    start_ticks: i64,
    end_ticks: i64,
    /// The source time at the clip start, on the forward media clock of the
    /// clip's keys.
    source_start_ticks: i64,
    /// The native source bounds, which reverse playback measures backward
    /// from the media end.
    in_ticks: i64,
    out_ticks: i64,
    /// Negative for reverse playback.
    playback_rate: f64,
}

/// Where `clip`, whose video is `video`, plays on the sequence frame grid
/// and in its media, which lasts `intrinsic_ticks`: at native constant speed
/// or reverse, or at unit speed. A `source.timeRemap` that export keeps plays
/// its source range at unit speed.
fn clip_ticks(
    clip: ClipLayers<'_>,
    video: &VideoLayer,
    intrinsic_ticks: i64,
    context: &LayerExport<'_, '_>,
) -> Result<ClipTicks> {
    let frame_rate = context.frame_rate;
    let source = &video.source;
    // The clip's timeline range; a staged video's own range is [0, its
    // group's duration] on the group clock.
    let active_range = clip.active_range();
    let active_end = active_range
        .start
        .checked_add_duration(active_range.duration)
        .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
    let active_start_ticks = context.frame_ticks(active_range.start, "activeRange.start")?;
    let active_end_ticks = context.frame_ticks(active_end, "activeRange.end")?;
    ensure!(
        active_end_ticks > active_start_ticks,
        "activeRange {}..{} ms collapses to zero duration on the {frame_rate} sequence grid",
        active_range.start.as_millis(),
        active_end.as_millis()
    );
    let active_duration_ticks = active_end_ticks - active_start_ticks;
    let constant_playback = video
        .playback
        .time_remap()
        .map(|playback| {
            constant_playback_source_range(
                playback,
                video.playback.input_range(),
                video.source_range,
                video.playback.input_offset_ms(),
            )
        })
        .transpose()?;
    let mapped_source = match constant_playback {
        Some((range, _)) => range,
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
    // The shared timeline validator checks source coverage at sequence
    // sample times, including a final partial source frame.
    let intrinsic_millis = time_from_ticks(intrinsic_ticks)?.as_millis();
    let floored_millis = u64::try_from(intrinsic_ticks / crate::schema::TICKS_PER_MILLISECOND)
        .map_err(|_| unsupported("negative media duration"))?;
    ensure!(
        video.source_intrinsic_duration.as_millis() == intrinsic_millis
            || video.source_intrinsic_duration.as_millis() == floored_millis,
        "sourceIntrinsicDuration {} ms differs from the packaged MP4 duration {intrinsic_millis} ms{}",
        video.source_intrinsic_duration.as_millis(),
        super::replacement::active_output_context(source)
    );
    // Export omits a clip with any other playback ([`unexportable_clip`]).
    let playback_rate = if let Some((_, reverse)) = constant_playback {
        let rate = source_duration_ticks as f64 / active_duration_ticks as f64;
        if reverse {
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
    })
}

/// The static Motion of the clip whose Motion `motion` hosts, whose picture is
/// `frame` pixels: the position normalized to the canvas, the anchor to the
/// picture. What Motion cannot carry is reported; a clip whose Scale or
/// Rotation it cannot show is omitted before ([`unexportable_motion`]).
pub(super) fn export_transform(
    motion: MotionHost<'_>,
    [source_width, source_height]: [u32; 2],
    [width, height]: [u32; 2],
    dynamics: &AnimationGraph,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> PrStaticTransform {
    let t = motion.transform;
    let native_transform = PrStaticTransform {
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
/// A nest carries no Opacity mask (`canonical_mask` rejects one).
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
        Some(CanonicalMask::Opacity { .. }) => {
            return Err(unsupported(
                "an Opacity mask on a nested sequence is not converted",
            ))
        }
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

/// The layers among `layers` and their descendants whose Anchor Point keys
/// export as clip Motion keys ([`export_motion_keys`]): videos, stills and
/// groups that are no graphic, and the guide of a video's mask, which
/// repeats them. A graphic's objects and Vector Motion key no Anchor Point,
/// so the keys of text, shape, rectangle and graphic group layers stay
/// reported as animation that export omits.
fn anchor_point_layer_ids(layers: &[Layer]) -> BTreeSet<LayerId> {
    let mut ids = BTreeSet::new();
    for layer in layers {
        match layer.data() {
            LayerData::Video(_) | LayerData::Media(_) | LayerData::Image(_) => {
                ids.insert(layer.id());
                let (_, masks) = layer_consumers(layer);
                ids.extend(masks.iter().filter_map(|mask| mask.layer));
            }
            LayerData::Group(group) if super::graphic::graphic_objects(group).is_none() => {
                ids.insert(group.id);
                ids.extend(anchor_point_layer_ids(&group.layers));
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
fn consumed_layer_ids(layers: &[Layer], ancestor_hidden: bool) -> BTreeSet<LayerId> {
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
fn place_item(
    video_tracks: &mut Vec<PrVideoTrack>,
    item: PrVideoItem,
    min_track: usize,
    context: &mut LayerExport<'_, '_>,
) -> Result<usize> {
    // All assigned items are lower in the current paint stack.
    let index = video_tracks
        .iter()
        .rposition(|track| {
            track.items.iter().any(|lower| item.overlaps(lower))
                || track
                    .nests
                    .iter()
                    .any(|nest| nest.overlaps(&item.timeline_ticks()))
        })
        .map_or(0, |index| index + 1)
        .max(min_track);
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

/// The layers that a track matte of `layers`, or of their descendants, names
/// as its source.
fn track_matte_sources(layers: &[Layer]) -> BTreeSet<LayerId> {
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
        for consumer in self.consumers.remove(&source).unwrap_or_default() {
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
                        PrVideoItem::Graphic(_) => continue,
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
fn export_staged_matte(
    group: &GroupLayer,
    source: LayerId,
    parent: Option<LayerId>,
    video_tracks: &mut Vec<PrVideoTrack>,
    consumers: &mut MatteConsumers,
    context: &mut LayerExport<'_, '_>,
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
            })?;
            // The keys that the source's clip writes; it has no mask of its
            // own ([`canonical_track_matte`]), so no guide track either.
            context
                .written
                .record_animations(video.id, &occurrence.animations);
            source_media(context.media, &occurrence.media).video = Some(stream);
            place_item(
                video_tracks,
                PrVideoItem::Media(occurrence),
                min_track,
                context,
            )?
        }
        LayerData::Image(image) => {
            let image = ImageLayer {
                active_range: group.playback.input_range(),
                parent,
                ..image.clone()
            };
            let exported =
                super::still::export_image_layer(&image, parent, context, omissions, &record)
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
    let end_ticks = context.frame_ticks(end, "activeRange.end")?;
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
/// text and shape effects, which `effects::export_effects` and `text_shadow`
/// write or report one by one.
pub(super) fn unexported_layer_fields(
    layer: ClipLayer<'_>,
    group: Option<LayerId>,
    masks_exported: bool,
) -> [(bool, ExportField); 12] {
    use ClipLayer::{Image, Shape, Text, Video};
    let (description, parent, track_matte) = match layer {
        Video(l) => (&l.description, l.parent, &l.track_matte),
        Image(l) => (&l.description, l.parent, &l.track_matte),
        Text(l) => (&l.description, l.parent, &l.track_matte),
        Shape(l) => (&l.description, l.parent, &l.track_matte),
    };
    let (masks, effects, motion_blur) = match layer {
        Video(l) => (&l.masks, &[][..], l.motion_blur),
        Image(l) => (&l.masks, &l.effects[..], l.motion_blur),
        Text(l) => (&l.masks, &[][..], l.motion_blur),
        Shape(l) => (&l.masks, &[][..], l.motion_blur),
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
        (!effects.is_empty(), ExportField::Effects),
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
    if !layer.animators.is_empty() || layer.path_options.is_some() || layer.anchor_options.is_some()
    {
        return Err(unsupported(
            "text animators, path options and anchor options are unsupported",
        ));
    }
    if transform.scale[0] != transform.scale[1] {
        return Err(unsupported("text scale must be uniform"));
    }
    let (document, box_origin) = text_document(&layer.source_text, font)?;
    Ok(PrText {
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
            scale: transform.scale[0],
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
        opacity: 100.0,
        blend_mode: PrBlendMode::Normal,
        animations: Vec::new(),
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
