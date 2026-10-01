//! Build canonical editable documents without runtime mutation dependencies.

#[path = "pop.rs"]
mod pop;
#[path = "stroke.rs"]
mod stroke;

use super::{
    background::{black_shape, identity_transform, plain_group},
    effects, fonts,
    graphic::{fx_path, scaled_path},
    keyframes,
    nested::{self, ItemLayer, ItemLayers, LayerScope},
    tesseract_to_premiere::LINEAR_WIPE_GUIDE_PREFIX,
    text::STROKE_WIDTH_RATIO,
    timing::{duration_from_ticks, time_from_ticks},
};
use crate::{
    error::{unsupported, BuildError, CreationError, EditableBuildError, Result},
    format::{MediaId, PrGraphic, PrMedia, PrSequence, PrVideoItem, PrVideoOccurrence},
    linked_compositions::LinkedCompositions,
    schema::{
        text::{PrJustification, PrRgb, PrTextFrame, PrTextTransform, PrVerticalAlign},
        MaskBoundary, PrAnimatedProperty, PrAudioOccurrence, PrEffect, PrEffectParamKeys,
        PrKeyframeEasing, PrLinearWipe, PrMatteChannel, PrMediaKind, PrNestOccurrence,
        PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe, PrStaticTransform, PrText,
        PrTimeRemap, PrTrackMatte, PrTransform, PrVideoStream, PrVolumeKeys, TransformOwner,
        TRANSFORM_OPACITY, TRANSFORM_POSITION, TRANSFORM_ROTATION, TRANSFORM_SCALE_HEIGHT,
        TRANSFORM_SCALE_WIDTH, TRANSFORM_SHUTTER_ANGLE,
    },
    {approximate, omit, Omission, OmissionScope},
};
use fx_schema::{
    animator::{
        AnimationGraphError, PropertyKeyframe, PropertyKeyframeEasing, PropertyKeyframeTrack,
    },
    AnimationGraph, AssetId, AudioLayer, AudioSource, BlendMode, CompositionId, Dimensions,
    Duration, EditableFxCompositionDocument, FXComposition, FxItemId, GroupLayer, Justification,
    KeyframeId, Layer, LayerData, LayerId, LinearGain, MaskMode, MediaFit, MotionBlurSettings,
    NonNegativeProperty, PathMask, PercentageProperty, Position, PositiveProperty, PositiveRect,
    PropType, Property, PropertyAnimator, PropertyValue, RectBounds, RectLayer, RectShape,
    ShapeContent, ShapeLayer, ShapePath, TextDocument, TextLayer, Time, TimeOffset,
    TimeRangeProperty, TimeRemapExtrapolation, TimeRemapKeyframe, TimeRemapProperty, TrackMatte,
    TrackMatteType, Transform, VerticalAlign, VideoLayer, VideoSource,
};
use std::{cell::Cell, collections::BTreeMap, ops::Range, sync::Arc};

pub(super) struct Processed<'a> {
    pub(super) progress: Option<(fx_conv::ProgressPhase<'a>, &'a Cell<usize>)>,
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

/// The animation graph rejects a keyframe id that two tracks share anywhere in
/// the composition, so each id names the layer that owns its track.
pub(super) fn keyframe_id(layer_id: LayerId, name: &str, index: usize) -> KeyframeId {
    KeyframeId::new(format!("premiere-{name}-{layer_id}-{index}"))
}

/// Why a matte clip at another speed or with Time Remapping is not moved
/// under a stage group (export rejects the mirror, a stage child source with
/// playback keys): its playback keys are on the sequence clock, and a stage
/// group's child reads the group clock, which starts at the clip. Translating
/// them is unmeasured (follow-up: staged mattes with speed changes).
const STAGED_RETIMED_MATTE_REASON: &str = "a Track Matte Key whose matte clip plays at another speed or is time-remapped is not converted on a clip that its Motion or effects stage; the matte's playback keys are on the sequence clock and a stage group's child reads the group clock";

/// Why native keys on `clip` cannot be imported onto the FX layer clock.
///
/// Native keys are on the source clock; they equal layer time only under unit
/// forward playback. How Premiere places keys under constant speed, reverse or
/// Time Remapping is not pinned by an Adobe fixture, so those keys fail closed.
fn retimed_keys_reason(clip: &PrVideoOccurrence) -> Option<&'static str> {
    (clip.playback_rate != 1.0 || clip.time_remap.is_some())
        .then_some("keys on a retimed, reversed or time-remapped clip are not converted")
}

/// Why the canvas-fixed Linear Wipe guide cannot represent a clip's wipe on
/// export (import stages such a clip); `motion_animated` is whether the
/// converted keys move the clip frame.
///
/// Premiere applies Linear Wipe in the clip's own frame before Motion. The
/// guide covers the canvas, so only a canvas-sized source whose unanimated
/// Motion is the identity (unit scale, no rotation, anchor on its position)
/// keeps the same wipe edge.
pub(super) fn wipe_frame_reason(
    source: [u32; 2],
    canvas: [u32; 2],
    scale: [f64; 2],
    rotation: f64,
    anchor_on_position: bool,
    motion_animated: bool,
) -> Option<&'static str> {
    if source != canvas {
        Some("the source frame differs from the canvas")
    } else if scale != [100.0, 100.0] || rotation != 0.0 || !anchor_on_position {
        Some("static Motion moves the clip frame off the canvas")
    } else if motion_animated {
        Some("Motion is animated")
    } else {
        None
    }
}

fn wipe_keys(
    wipe: &PrLinearWipe,
    source_in: i64,
    guide_id: LayerId,
    property_type: PropType,
) -> Result<PropertyKeyframeTrack> {
    let mut keys = Vec::with_capacity(wipe.completion.len());
    for (index, key) in wipe.completion.iter().enumerate() {
        let millis = keyframes::layer_millis(key.source_ticks, source_in)?;
        keys.push(PropertyKeyframe::new(
            keyframe_id(guide_id, "linear-wipe", index),
            TimeOffset::from_millis(millis),
            PropertyValue::Float(100.0 - key.value),
            keyframes::fx_easing(key.easing),
        ));
    }
    let track = PropertyKeyframeTrack::new(keys).map_err(|error| {
        unsupported(format!(
            "Premiere Linear Wipe keyframe times/values cannot be imported: {error}"
        ))
    })?;
    ensure_wipe_axis(property_type)?;
    Ok(track)
}

/// The static transform, animated scale axis and completion keys of the
/// canvas-sized guide that reveals `wipe`.
fn linear_wipe_guide(
    wipe: &PrLinearWipe,
    source_in: i64,
    guide_id: LayerId,
    canvas: [u32; 2],
) -> Result<(Transform, PropType, PropertyKeyframeTrack)> {
    let visible = 100.0 - wipe.initial_completion;
    let [width, height] = canvas.map(f64::from);
    // Each guide scales from the canvas edge where the wipe starts.
    let (property_type, edge, scale) = match wipe.angle_degrees {
        0 => (PropType::ScaleY, [0.0, height], [100.0, visible]),
        90 => (PropType::ScaleX, [width, 0.0], [visible, 100.0]),
        180 => (PropType::ScaleY, [0.0, 0.0], [100.0, visible]),
        270 => (PropType::ScaleX, [0.0, 0.0], [visible, 100.0]),
        _ => return Err(unsupported("non-cardinal Linear Wipe angle")),
    };
    let track = wipe_keys(wipe, source_in, guide_id, property_type)?;
    let transform = Transform {
        anchor_point: edge,
        position: Position::xy(edge[0], edge[1]),
        scale,
        ..identity_transform()
    };
    Ok((transform, property_type, track))
}

fn ensure_wipe_axis(property_type: PropType) -> Result<()> {
    if matches!(property_type, PropType::ScaleX | PropType::ScaleY) {
        Ok(())
    } else {
        Err(unsupported("unexpected Linear Wipe guide property"))
    }
}

pub(super) fn scalar_keys(
    animation: &PrPropertyAnimation,
    source_in: i64,
    layer_id: LayerId,
    property_type: PropType,
) -> Result<PropertyKeyframeTrack> {
    let native_keys = animation
        .scalar_keys()
        .ok_or_else(|| unsupported("unexpected point-valued Motion property"))?;
    scalar_property_keys(native_keys, source_in, layer_id, property_type)
}

/// The FX key track of the scalar layer property `property_type` from native
/// keys on the source clock from `source_in`, as Motion keys convert.
fn scalar_property_keys(
    native_keys: &[PrScalarKeyframe],
    source_in: i64,
    layer_id: LayerId,
    property_type: PropType,
) -> Result<PropertyKeyframeTrack> {
    let mut keys = Vec::with_capacity(native_keys.len());
    let name = match property_type {
        PropType::Opacity => "opacity",
        PropType::Rotation => "rotation",
        PropType::ScaleX => "scale-x",
        PropType::ScaleY => "scale-y",
        _ => return Err(unsupported("unexpected scalar property")),
    };
    for (index, key) in native_keys.iter().enumerate() {
        let millis = keyframes::layer_millis(key.source_ticks, source_in)?;
        let easing = keyframes::fx_easing(key.easing);
        keys.push(PropertyKeyframe::new(
            keyframe_id(layer_id, name, index),
            TimeOffset::from_millis(millis),
            PropertyValue::Float(key.value),
            easing,
        ));
    }
    // Never collapse distinct native times that round to the same local millisecond.
    PropertyKeyframeTrack::new(keys).map_err(|error| {
        unsupported(format!(
            "Premiere keyframe times/values cannot be imported: {error}"
        ))
    })
}

/// Clip Volume keys as `AudioVolume` keys. The other stages scale each value;
/// the easing of a Linear segment is fitted to its own Level values, because
/// Premiere's curve depends on them.
fn volume_keys(
    keys: &PrVolumeKeys,
    source_in: i64,
    layer_id: LayerId,
) -> Result<PropertyKeyframeTrack> {
    let mut output = Vec::with_capacity(keys.keys.len());
    for (index, key) in keys.keys.iter().enumerate() {
        let easing = match (index.checked_sub(1), key.easing) {
            (_, PrKeyframeEasing::Hold) => PropertyKeyframeEasing::Hold,
            (Some(previous), _) => {
                super::audio::fitted_level_easing(keys.keys[previous].value, key.value)
            }
            (None, _) => PropertyKeyframeEasing::Linear,
        };
        output.push(PropertyKeyframe::new(
            keyframe_id(layer_id, "volume", index),
            TimeOffset::from_millis(keyframes::layer_millis(key.source_ticks, source_in)?),
            PropertyValue::Float(key.value * keys.gain),
            easing,
        ));
    }
    PropertyKeyframeTrack::new(output).map_err(|error| {
        unsupported(format!(
            "Premiere volume keyframe times/values cannot be imported: {error}"
        ))
    })
}

fn time_remap_property(remap: &PrTimeRemap, start_ticks: i64) -> Result<TimeRemapProperty> {
    let keys = remap
        .keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let parent_ticks = start_ticks.checked_add(key.timeline_ticks).ok_or_else(|| {
                unsupported("TimeRemapping parent time exceeds Premiere's tick range")
            })?;
            let parent_millis = u64::try_from(keyframes::layer_millis(parent_ticks, 0)?)
                .map_err(|_| unsupported("TimeRemapping parent time must be non-negative"))?;
            let source_millis = u64::try_from(keyframes::layer_millis(key.source_ticks, 0)?)
                .map_err(|_| unsupported("TimeRemapping source time must be non-negative"))?;
            Ok(TimeRemapKeyframe {
                id: KeyframeId::new(format!("premiere-time-remap-{index}")),
                time: Time::from_millis(parent_millis),
                value: Time::from_millis(source_millis),
                easing: keyframes::fx_easing(key.easing),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    TimeRemapProperty::new(
        keys,
        TimeRemapExtrapolation::Continue,
        TimeRemapExtrapolation::Continue,
    )
    .map_err(|error| unsupported(format!("TimeRemapping cannot be imported: {error}")))
}

/// The identity-rate playback of a linked clip's picture group, over its
/// `range` on the document clock: the group's parent clock onto its clip
/// clock, as the group's start offset already maps it.
///
/// The FX runtime evaluates keyframed animation under time remapping on a
/// chain that starts at the first Group with `playback`, from the document
/// clock, without the start offsets of the Groups above it; its render walk
/// does apply them. This seed starts that chain at the picture group, whose
/// parent clock is the document clock, so the composition's own remapped
/// clocks (its source group and After Effects source clocks) keep their
/// placement time.
fn document_clock_seed(range: TimeRangeProperty) -> Result<TimeRemapProperty> {
    constant_time_remap(
        false,
        range,
        TimeRangeProperty::new(Time::ZERO, range.duration),
    )
}

/// `playback`, whose key times are on a clock on which its clip starts at
/// `start`, with its key times on the clip's own clock, which starts at zero.
fn on_clip_clock(playback: &TimeRemapProperty, start: Time) -> Result<TimeRemapProperty> {
    let keyframes = playback
        .keyframes()
        .iter()
        .map(|key| {
            let time = key
                .time
                .checked_sub(start)
                .ok_or_else(|| unsupported("a playback key precedes its clip"))?;
            Ok(TimeRemapKeyframe {
                time: Time::ZERO + time,
                ..key.clone()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    TimeRemapProperty::new(keyframes, playback.before(), playback.after()).map_err(|error| {
        unsupported(format!(
            "playback cannot be imported on the clip clock: {error}"
        ))
    })
}

fn constant_time_remap(
    reverse: bool,
    active_range: TimeRangeProperty,
    source_range: TimeRangeProperty,
) -> Result<TimeRemapProperty> {
    let (first_source, last_source) = if reverse {
        (source_range.end(), source_range.start)
    } else {
        (source_range.start, source_range.end())
    };
    let keyframes = vec![
        TimeRemapKeyframe {
            id: KeyframeId::new("premiere-playback-start"),
            time: active_range.start,
            value: first_source,
            easing: PropertyKeyframeEasing::Linear,
        },
        TimeRemapKeyframe {
            id: KeyframeId::new("premiere-playback-end"),
            time: active_range.end(),
            value: last_source,
            easing: PropertyKeyframeEasing::Linear,
        },
    ];
    TimeRemapProperty::new(
        keyframes,
        TimeRemapExtrapolation::Inactive,
        TimeRemapExtrapolation::Inactive,
    )
    .map_err(|error| unsupported(format!("constant playback cannot be imported: {error}")))
}

/// The FX layer properties that the keys of a native point property import
/// onto: per axis, the property and the name that its keyframe ids carry
/// ([`keyframe_id`]), so that a layer's Position and Anchor Point keys never
/// share an id.
struct PointTracks {
    /// The native property, in reasons.
    name: &'static str,
    axes: [(PropType, &'static str); 2],
}

const POSITION_TRACKS: PointTracks = PointTracks {
    name: "Position",
    axes: [
        (PropType::PositionX, "position-x"),
        (PropType::PositionY, "position-y"),
    ],
};

const ANCHOR_POINT_TRACKS: PointTracks = PointTracks {
    name: "Anchor Point",
    axes: [
        (PropType::AnchorPointX, "anchor-point-x"),
        (PropType::AnchorPointY, "anchor-point-y"),
    ],
};

pub(super) fn position_tracks(
    animation: &PrPropertyAnimation,
    source_in: i64,
    layer_id: LayerId,
    dimensions: [u32; 2],
) -> Result<(PropertyKeyframeTrack, PropertyKeyframeTrack)> {
    let PrPropertyAnimation::Position(native_keys) = animation else {
        return Err(unsupported("unexpected non-Position Motion property"));
    };
    let [(_, x), (_, y)] = point_property_tracks(
        native_keys,
        source_in,
        layer_id,
        &POSITION_TRACKS,
        dimensions,
    )?;
    Ok((x, y))
}

/// The FX tracks of the two axes of `point`, a native point property, from
/// its keys on the source clock from `source_in`, each coordinate scaled by
/// `dimensions`, as Motion Position keys convert.
fn point_property_tracks(
    native_keys: &[PrPointKeyframe],
    source_in: i64,
    layer_id: LayerId,
    point: &PointTracks,
    dimensions: [u32; 2],
) -> Result<[(PropType, PropertyKeyframeTrack); 2]> {
    let mut x_keys = Vec::with_capacity(native_keys.len());
    let mut y_keys = Vec::with_capacity(native_keys.len());
    let [(x_property, x_name), (y_property, y_name)] = point.axes;
    for (index, key) in native_keys.iter().enumerate() {
        let millis = keyframes::layer_millis(key.source_ticks, source_in)?;
        let easing = keyframes::fx_easing(key.easing);
        for (axis, output, name) in [(0, &mut x_keys, x_name), (1, &mut y_keys, y_name)] {
            let scale = f64::from(dimensions[axis]);
            output.push(
                PropertyKeyframe::new(
                    keyframe_id(layer_id, name, index),
                    TimeOffset::from_millis(millis),
                    PropertyValue::Float(key.value[axis] * scale),
                    easing,
                )
                .with_spatial_tangents(
                    (index > 0)
                        .then_some(key.spatial_in_tangent)
                        .flatten()
                        .map(|tangent| tangent[axis] * scale),
                    (index + 1 < native_keys.len())
                        .then_some(key.spatial_out_tangent)
                        .flatten()
                        .map(|tangent| tangent[axis] * scale),
                ),
            );
        }
    }
    let name = point.name;
    let x = PropertyKeyframeTrack::new(x_keys).map_err(|error| {
        unsupported(format!(
            "Premiere {name} X keyframes cannot be imported: {error}"
        ))
    })?;
    let y = PropertyKeyframeTrack::new(y_keys).map_err(|error| {
        unsupported(format!(
            "Premiere {name} Y keyframes cannot be imported: {error}"
        ))
    })?;
    PropertyKeyframeTrack::validate_position_pair(&x, &y).map_err(|error| {
        unsupported(format!(
            "Premiere {name} keyframe pair cannot be imported: {error}"
        ))
    })?;
    Ok([(x_property, x), (y_property, y)])
}

/// The staged video's transform from the clip's one Transform effect
/// (`PrVideoOccurrence::transform_stage`) on a `source`-pixel frame: both
/// points are the effect's, in source pixels (Oracle run E11 T7); the scale is
/// Scale Height on both axes under Uniform Scale (T3); the rotation is the
/// effect's, clockwise (T4); the skew is the effect's and the skew axis
/// Skew Axis − 90° under a skew, else 0 ([`PrTransform::fx_skew_axis`],
/// the convention that the export gate measured); the opacity is the
/// effect's, which FX blends in sRGB. The
/// group's Motion and clip Opacity apply after it (T10), so the two
/// opacities multiply.
fn staged_video_transform(transform: &PrTransform, source: [u32; 2]) -> Result<Transform> {
    let [width, height] = source.map(f64::from);
    Ok(Transform {
        anchor_point: [
            transform.anchor_point[0] * width,
            transform.anchor_point[1] * height,
        ],
        position: Position::xy(
            transform.position[0] * width,
            transform.position[1] * height,
        ),
        scale: transform.scale(),
        rotation: transform.rotation,
        skew: transform.skew,
        skew_axis: transform.fx_skew_axis(),
        opacity: PercentageProperty::new(transform.opacity)
            .ok_or_else(|| unsupported("Transform Opacity must be between 0 and 100"))?,
        ..identity_transform()
    })
}

/// The FX composition's one motion blur, which the first imported clip that
/// blurs sets: a Transform with motion blur
/// ([`PrTransform::motion_blur_shutter_angle`]),
/// or a linked composition that enables its own. A later clip that requests
/// other settings blurs with these and is reported.
pub(super) struct CompositionShutter {
    /// The composition's enabled motion blur.
    settings: MotionBlurSettings,
    /// The record of the clip that set it.
    clip: String,
}

impl CompositionShutter {
    /// A Transform's motion blur at `angle` with phase 0, the exposure
    /// starting at the frame time (Premiere's phase is unmeasured), and the
    /// default sample counts.
    fn transform(angle: f64) -> Result<MotionBlurSettings> {
        Ok(MotionBlurSettings {
            enabled: true,
            shutter_angle: NonNegativeProperty::new(angle)
                .ok_or_else(|| unsupported("Transform Shutter Angle must be non-negative"))?,
            shutter_phase: 0.0,
            ..MotionBlurSettings::default()
        })
    }

    /// Records the motion blur that clip `record` requests in `slot`, and
    /// reports the clip when an earlier one set other settings.
    fn request(
        slot: &mut Option<Self>,
        settings: MotionBlurSettings,
        record: &str,
        omissions: &mut Vec<Omission>,
    ) {
        let shutter = slot.get_or_insert_with(|| Self {
            settings,
            clip: record.to_owned(),
        });
        let set = shutter.settings;
        if set == settings {
            return;
        }
        let angle_only = MotionBlurSettings {
            shutter_angle: settings.shutter_angle,
            ..set
        } == settings;
        let message = if angle_only {
            format!(
                "composition shutter set to {}° by clip {}; clip {record} requested {}°",
                set.shutter_angle.value(),
                shutter.clip,
                settings.shutter_angle.value()
            )
        } else {
            let describe = |settings: MotionBlurSettings| {
                format!(
                    "shutter {}°, phase {}°, {} to {} samples per frame",
                    settings.shutter_angle.value(),
                    settings.shutter_phase,
                    settings.samples_per_frame.value(),
                    settings.adaptive_sample_limit.value()
                )
            };
            format!(
                "composition motion blur set by clip {} ({}); clip {record} requested {}",
                shutter.clip,
                describe(set),
                describe(settings)
            )
        };
        approximate(omissions, record, message);
    }
}

/// The staged video's tracks from the keys of its Transform `effect`, on the
/// video's clock from source In `source_in` as Motion keys are (E11 T9):
/// Position keys become the `PositionX`/`PositionY` pair in `source` pixels,
/// Scale Height keys the `ScaleY` track and, under Uniform Scale, the `ScaleX`
/// track too, Scale Width keys the `ScaleX` track without Uniform Scale,
/// Rotation keys the `Rotation` track and Opacity keys the `Opacity` track.
/// Shutter Angle keys and Scale Width keys under Uniform Scale have no track
/// ([`PrTransform::approximations`] and
/// [`PrTransform::unimported_scale_width_keys`] report them); the reader
/// rejects every other keyed parameter.
fn transform_stage_tracks(
    effect: &PrEffect,
    transform: &PrTransform,
    source: [u32; 2],
    source_in: i64,
    layer_id: LayerId,
) -> Result<Vec<(Property, PropertyKeyframeTrack)>> {
    let mut tracks = Vec::new();
    for animation in &effect.animations {
        let param = animation.param;
        let scalar_keys = |native_keys: &[PrScalarKeyframe], property_type| -> Result<_> {
            Ok((
                Property::new(layer_id, property_type),
                scalar_property_keys(native_keys, source_in, layer_id, property_type)?,
            ))
        };
        match (&animation.keys, param.id) {
            (PrEffectParamKeys::Point(keys), id) if id == TRANSFORM_POSITION.id => {
                let axes =
                    point_property_tracks(keys, source_in, layer_id, &POSITION_TRACKS, source)?;
                tracks.extend(
                    axes.map(|(property, track)| (Property::new(layer_id, property), track)),
                );
            }
            (PrEffectParamKeys::Scalar(keys), id) if id == TRANSFORM_SCALE_HEIGHT.id => {
                if transform.uniform_scale {
                    tracks.push(scalar_keys(keys, PropType::ScaleX)?);
                }
                tracks.push(scalar_keys(keys, PropType::ScaleY)?);
            }
            (PrEffectParamKeys::Scalar(keys), id) if id == TRANSFORM_SCALE_WIDTH.id => {
                if !transform.uniform_scale {
                    tracks.push(scalar_keys(keys, PropType::ScaleX)?);
                }
            }
            (PrEffectParamKeys::Scalar(keys), id) if id == TRANSFORM_ROTATION.id => {
                tracks.push(scalar_keys(keys, PropType::Rotation)?);
            }
            (PrEffectParamKeys::Scalar(keys), id) if id == TRANSFORM_OPACITY.id => {
                tracks.push(scalar_keys(keys, PropType::Opacity)?);
            }
            (PrEffectParamKeys::Scalar(_), id) if id == TRANSFORM_SHUTTER_ANGLE.id => {}
            _ => {
                return Err(unsupported(format!(
                    "keyframed Transform {} has no layer property",
                    param.label
                )));
            }
        }
    }
    Ok(tracks)
}

/// The document of `project` without linked compositions: their clips are
/// omitted, as a sequence whose AEPs are unresolved has them.
#[cfg(test)]
pub(crate) fn premiere_to_tesseract(
    project: &PrSequence,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    omissions: &mut Vec<Omission>,
) -> Result<EditableFxCompositionDocument> {
    sequence_document(
        project,
        media,
        asset_ids,
        &mut LinkedCompositions::default(),
        omissions,
    )
}

/// The editable document of `project`, whose packaged media has `asset_ids`
/// and whose linked compositions `linked` imports.
#[cfg(test)]
pub(crate) fn sequence_document(
    project: &PrSequence,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    linked: &mut LinkedCompositions<'_, '_>,
    omissions: &mut Vec<Omission>,
) -> Result<EditableFxCompositionDocument> {
    sequence_document_with_progress(
        project,
        media,
        asset_ids,
        linked,
        omissions,
        fx_conv::Progress::default(),
    )
}

pub(crate) fn sequence_document_with_progress(
    project: &PrSequence,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    linked: &mut LinkedCompositions<'_, '_>,
    omissions: &mut Vec<Omission>,
    progress: fx_conv::Progress<'_>,
) -> Result<EditableFxCompositionDocument> {
    let occurrence_count = project.video_items().count();
    let phase = progress.phase(
        "importing Premiere clips",
        "clips",
        occurrence_count
            + project.audio.len()
            + project
                .video_tracks
                .iter()
                .map(|track| track.nests.len())
                .sum::<usize>(),
    );
    let end = project.end_ticks();
    let duration = duration_from_ticks(end)?;
    if project.width == 0 || project.height == 0 {
        return Err(CreationError::MissingDimensions.into());
    }
    if duration.is_zero() {
        return Err(
            EditableBuildError::invalid_input("duration", "must be greater than zero").into(),
        );
    }

    let mut next_index = occurrence_count;
    let mut effect_ids = effects::EffectIdAllocator::default();
    let mut composition_shutter = None;
    let mut dynamics = AnimationGraph::new();
    let completed = Cell::new(0);
    let mut layers = video_layers(
        project,
        LayerScope::root(
            &mut next_index,
            &mut effect_ids,
            &mut composition_shutter,
            linked,
        ),
        media,
        asset_ids,
        &mut dynamics,
        omissions,
        Some((phase, &completed)),
    )?;
    let sounds = audio_layers(
        &project.audio,
        None,
        &mut next_index,
        media,
        asset_ids,
        &mut dynamics,
        omissions,
        Some((phase, &completed)),
    )?;
    progress.stage("assemble Tesseract document");
    let canvas = RectLayer {
        id: LayerId::new(next_index as u64 + 1),
        name: "Premiere black canvas".into(),
        description: String::new(),
        is_hidden: false,
        parent: None,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range: tick_range(0, end)?,
        effects: Vec::new(),
        motion_blur: false,
        transform: identity_transform(),
        rect: black_shape(project.width, project.height),
    };

    layers.extend(sounds);
    validate_time_range("active_range", canvas.active_range)?;
    layers.push(Layer::from_data(&fx_schema::LayerData::Rect(canvas))?);

    let mut composition = FXComposition::try_from_parts(
        CompositionId::new("main"),
        project.name.clone(),
        dynamics,
        layers,
    )
    .map_err(EditableBuildError::from)?;
    if let Some(shutter) = &composition_shutter {
        composition
            .set_motion_blur(shutter.settings)
            .map_err(EditableBuildError::from)?;
    }
    let document = EditableFxCompositionDocument::new(
        Dimensions::new(project.width, project.height),
        duration,
        None,
        composition,
    )?;
    report_unpackaged_fonts(project, omissions);
    Ok(document)
}

/// Editable audio layers of `sounds`, children of `parent`, with the next
/// ids of `next_index` and their Volume keys in `dynamics`. A key track that
/// cannot import is reported, and its sound keeps its placement at zero
/// gain: the value before the keys would play through their silent
/// intervals.
#[allow(clippy::too_many_arguments)]
pub(super) fn audio_layers(
    sounds: &[PrAudioOccurrence],
    parent: Option<LayerId>,
    next_index: &mut usize,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
    progress: Option<(fx_conv::ProgressPhase<'_>, &Cell<usize>)>,
) -> Result<Vec<Layer>> {
    let mut layers = Vec::with_capacity(sounds.len());
    for (index, clip) in sounds.iter().enumerate() {
        let _processed = Processed { progress };
        let source = media
            .get(&clip.media)
            .and_then(|source| source.audio.as_ref())
            .ok_or_else(|| unsupported("audio occurrence references unknown media"))?;
        let asset_id = asset_ids
            .get(&clip.media)
            .ok_or_else(|| unsupported("audio occurrence has no asset ID"))?;
        // Sound follows the same boundary rounding as picture.
        let active_range = tick_range(clip.start_ticks, clip.end_ticks)?;
        let source_range =
            TimeRangeProperty::new(time_from_ticks(clip.in_ticks)?, active_range.duration);
        let layer_id = LayerId::new(*next_index as u64 + 1);
        *next_index += 1;
        let mut volume = clip.volume;
        if let Some(keys) = &clip.volume_keys {
            match volume_keys(keys, clip.in_ticks, layer_id) {
                Ok(track) => dynamics
                    .set_property(
                        Property::new(layer_id, PropType::AudioVolume),
                        PropertyAnimator::keyframes(track),
                        Vec::new(),
                    )
                    .map_err(map_animation_graph_error)?,
                Err(error) => {
                    volume = LinearGain::ZERO;
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        clip.record(),
                        format!(
                            "volume animation was not imported: {error}; the sound was kept at zero gain"
                        ),
                    );
                }
            }
        }
        let sound = AudioLayer {
            id: layer_id,
            name: format!("Premiere audio {}", index + 1),
            description: String::new(),
            is_hidden: false,
            parent,
            start_time: None,
            volume,
            auto_ducking: None,
            window_ms: fx_schema::default_audio_window_ms(),
            source: AudioSource {
                asset_id: asset_id.clone(),
                enhancement: None,
            },
            metadata: None,
            captions_enabled: None,
            caption_presentation: None,
            source_range,
            playback: fx_schema::LayerPlayback::linear(
                active_range,
                active_range,
                TimeRangeProperty::new(source_range.start, active_range.duration),
                0,
            )
            .map_err(unsupported)?,
            preserve_audio_pitch: false,
            source_intrinsic_duration: duration_from_ticks(source.intrinsic_ticks)?,
        };
        validate_time_range("playback.inputRange", sound.playback.input_range())?;
        validate_time_range("source_range", sound.source_range)?;
        layers.push(Layer::from_data(&fx_schema::LayerData::Audio(sound))?);
    }
    Ok(layers)
}

pub(super) fn video_layers(
    project: &PrSequence,
    mut scope: LayerScope<'_, '_, '_>,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
    progress: Option<(fx_conv::ProgressPhase<'_>, &Cell<usize>)>,
) -> Result<Vec<Layer>> {
    let mut layers = Vec::with_capacity(project.video_items().count() * 2);
    let mut offset = scope.first_index + project.video_items().count();
    // How many placements key each matte, by the matte's track and start: a
    // stage group takes only a matte of its own.
    let mut matte_consumers: BTreeMap<(usize, i64), usize> = BTreeMap::new();
    for track in &project.video_tracks {
        let keyed = track
            .items
            .iter()
            .filter_map(PrVideoItem::media)
            .filter_map(|clip| Some((clip.track_matte?, clip.start_ticks)))
            .chain(
                track
                    .nests
                    .iter()
                    .filter_map(|nest| Some((nest.track_matte?, nest.start_ticks))),
            );
        for (matte, start) in keyed {
            *matte_consumers
                .entry((matte.track_index, start))
                .or_default() += 1;
        }
    }
    // Tracks are visited from the top, so a matte, on a track above its clip,
    // is in `scope.item_layers` before the clip that it keys.
    for (track_index, track) in project.video_tracks.iter().enumerate().rev() {
        offset -= track.items.len();
        for (local_index, item) in track.items.iter().enumerate() {
            let _processed = Processed { progress };
            let index = offset + local_index;
            let layer_id = LayerId::new(index as u64 + 1);
            let item_key = (track_index, item.timeline_ticks().start);
            let clip = match item {
                PrVideoItem::Media(clip) => clip,
                PrVideoItem::Graphic(graphic) => {
                    let root = super::graphic::import_graphic(
                        graphic,
                        project.dimensions(),
                        layer_id,
                        index,
                        &mut scope,
                        dynamics,
                        omissions,
                    )?;
                    scope
                        .item_layers
                        .insert(item_key, ItemLayer::Plain(root.id()));
                    layers.push(root);
                    continue;
                }
            };
            // Generator media has no asset: a matte becomes an editable
            // rectangle, an adjustment an FX adjustment layer.
            match media
                .get(&clip.media)
                .and_then(|source| source.video.as_ref())
                .map(|video| video.kind)
            {
                Some(PrMediaKind::Adjustment) => {
                    layers.extend(super::adjustment::import_adjustment(
                        clip,
                        layer_id,
                        index,
                        [project.width, project.height],
                        &mut scope,
                        dynamics,
                        omissions,
                    )?);
                    continue;
                }
                Some(PrMediaKind::ColorMatte(matte)) => {
                    effects::omit_effects(clip, "Color Matte", omissions);
                    let mut rect = super::color_matte::rect_layer(
                        project,
                        matte,
                        tick_range(clip.start_ticks, clip.end_ticks)?,
                        LayerId::new(index as u64 + 1),
                        format!("Premiere color matte {}", index + 1),
                        !clip.enabled,
                    );
                    rect.parent = scope.parent;
                    rect.blend_mode = clip.blend_mode.fx_mode();
                    rect.transform.opacity = PercentageProperty::new(clip.opacity)
                        .ok_or_else(|| unsupported("Premiere opacity must be between 0 and 100"))?;
                    validate_time_range("active_range", rect.active_range)?;
                    layers.push(Layer::from_data(&fx_schema::LayerData::Rect(rect))?);
                    scope
                        .item_layers
                        .insert(item_key, ItemLayer::Plain(layer_id));
                    if let Some(warning) = clip.blend_mode.approximation() {
                        approximate(omissions, clip.record(), warning);
                    }
                    continue;
                }
                // Only a composition of an exact AEP imports; it is never
                // flattened to video or chosen by name.
                Some(PrMediaKind::AfterEffectsComposition(_))
                    if !scope.linked.contains(&clip.media) =>
                {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        clip.record(),
                        "linked After Effects composition was not resolved to a composition of an exact AEP",
                    );
                    continue;
                }
                Some(
                    PrMediaKind::AfterEffectsComposition(_)
                    | PrMediaKind::Video { .. }
                    | PrMediaKind::Still { .. },
                )
                | None => {}
            }
            let source = media
                .get(&clip.media)
                .and_then(|source| source.video.as_ref())
                .ok_or_else(|| unsupported("video occurrence references unknown media"))?;
            if source.kind.is_still() {
                let asset_id = asset_ids
                    .get(&clip.media)
                    .ok_or_else(|| unsupported("still occurrence has no asset ID"))?;
                effects::omit_effects(clip, "still image", omissions);
                let canvas = [project.width, project.height];
                let mut image = super::still::image_layer(
                    source,
                    asset_id,
                    layer_id,
                    format!("Premiere still {}", index + 1),
                    tick_range(clip.start_ticks, clip.end_ticks)?,
                    !clip.enabled,
                    clip_transform(
                        &clip.transform,
                        clip.opacity,
                        [source.width, source.height],
                        canvas,
                    )?,
                )?;
                image.parent = scope.parent;
                image.blend_mode = clip.blend_mode.fx_mode();
                validate_time_range("active_range", image.active_range)?;
                layers.push(Layer::from_data(&fx_schema::LayerData::Image(image))?);
                // A still has no mask guide; its keys import as a video's do.
                let tracks = motion_tracks(
                    &clip.animations,
                    clip.in_ticks,
                    retimed_keys_reason(clip),
                    layer_id,
                    None,
                    [source.width, source.height],
                    canvas,
                    clip.record(),
                    omissions,
                );
                set_tracks(dynamics, tracks)?;
                scope
                    .item_layers
                    .insert(item_key, ItemLayer::Plain(layer_id));
                if let Some(warning) = clip.blend_mode.approximation() {
                    approximate(omissions, clip.record(), warning);
                }
                continue;
            }
            let picture = match source.kind {
                PrMediaKind::AfterEffectsComposition(_) => ClipPicture::Linked,
                _ => ClipPicture::Asset(
                    asset_ids
                        .get(&clip.media)
                        .ok_or_else(|| unsupported("video occurrence has no asset ID"))?,
                ),
            };
            let record = clip.record();
            let matte = match clip.track_matte {
                None => None,
                Some(matte) => {
                    match matte_layer(
                        &scope.item_layers,
                        matte,
                        clip.timeline_ticks(),
                        track_index,
                        record,
                        omissions,
                    ) {
                        Ok(matte) => Some(matte),
                        Err(reason) => {
                            omit(omissions, OmissionScope::Occurrence, record, reason);
                            continue;
                        }
                    }
                }
            };
            let matte_shared = clip
                .track_matte
                .is_some_and(|matte| matte_consumers[&(matte.track_index, clip.start_ticks)] > 1);
            let root = import_video_clip(
                VideoClip {
                    clip,
                    source,
                    picture,
                    index,
                    track_index,
                    matte,
                    matte_shared,
                    static_matte: !matte_shared && measured_static_matte(project, clip, media),
                },
                project,
                &mut scope,
                &mut layers,
                dynamics,
                omissions,
            )?;
            if let Some(root) = root {
                scope.item_layers.insert(item_key, root);
            }
        }
        layers.extend(nested::track_nests(
            project,
            track_index,
            &mut scope,
            media,
            asset_ids,
            dynamics,
            omissions,
            progress,
        )?);
        import_transitions(
            project,
            &scope,
            track_index,
            &layers,
            media,
            &matte_consumers,
            dynamics,
            omissions,
        );
    }
    omit_unconsumed_mattes(project, &scope.item_layers, &mut layers, omissions);
    Ok(layers)
}

/// The layer that `layer`'s track matte consumes.
fn consumed_matte(layer: &Layer) -> Option<LayerId> {
    match layer.data() {
        LayerData::Video(video) => video.track_matte.as_ref(),
        LayerData::Group(group) => group.track_matte.as_ref(),
        _ => None,
    }
    .map(|matte| matte.layer)
}

/// Drops the root layer of each matte clip whose keyed placement was omitted
/// here and that no layer consumes, recording the omitted placement: Premiere
/// does not draw a matte clip while an active key names its track (fixture
/// G1b), and FX hides a matte source only through the layer that consumes it.
/// The reader consumed the mattes of the placements it omitted
/// (`consume_claimed_mattes`); this pass takes those omitted while they
/// converted. Tracks are visited from the bottom, so a dropped matte layer
/// that keys its own matte drops that one in turn.
fn omit_unconsumed_mattes(
    project: &PrSequence,
    item_layers: &ItemLayers,
    layers: &mut Vec<Layer>,
    omissions: &mut Vec<Omission>,
) {
    // The item's record as its own omissions name it.
    let record_at = |track: usize, start: i64| {
        let track = &project.video_tracks[track];
        track
            .items
            .iter()
            .find(|item| item.timeline_ticks().start == start)
            .map(|item| match item {
                PrVideoItem::Media(clip) => clip.record().to_owned(),
                PrVideoItem::Graphic(graphic) => graphic.id.clone().unwrap_or_default(),
            })
            .or_else(|| {
                track
                    .nests
                    .iter()
                    .find(|nest| nest.start_ticks == start)
                    .map(PrNestOccurrence::record)
            })
            .unwrap_or_default()
    };
    for (track_index, track) in project.video_tracks.iter().enumerate() {
        let keyed = track
            .items
            .iter()
            .filter_map(PrVideoItem::media)
            .filter_map(|clip| Some((clip.start_ticks, clip.track_matte?)))
            .chain(
                track
                    .nests
                    .iter()
                    .filter_map(|nest| Some((nest.start_ticks, nest.track_matte?))),
            );
        for (start, matte) in keyed {
            let placed = item_layers
                .get(&(track_index, start))
                .is_some_and(|root| layers.iter().any(|layer| layer.id() == root.id()));
            // A matte clip that was not converted left no layer to drop.
            let Some(matte_layer) = item_layers
                .get(&(matte.track_index, start))
                .map(|item| item.id())
            else {
                continue;
            };
            if placed
                || layers
                    .iter()
                    .any(|layer| consumed_matte(layer) == Some(matte_layer))
            {
                continue;
            }
            let count = layers.len();
            layers.retain(|layer| layer.id() != matte_layer);
            if layers.len() < count {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    record_at(matte.track_index, start),
                    format!(
                        "matte source of the omitted clip {} was not converted: Premiere does not draw a track-matte source",
                        record_at(track_index, start)
                    ),
                );
            }
        }
    }
}

/// Why FX `luma` approximates Matte Luma (fixture G2): reported on the keyed
/// placement, once per key.
const LUMA_MATTE_APPROXIMATION: &str = "Matte Luma is approximated: Premiere weights the matte's encoded RGB by Rec. 601 (measured), FX by Rec. 709; exact for a neutral matte, up to about 11 levels apart on a saturated colour matte";

/// The FX track matte of the placement `record` on `track_index` over `range`
/// whose Track Matte Key is `matte`: the mode and the root layer of the matte
/// item, which the reader resolved ([`crate::schema::check_track_matte`]).
/// Or why the placement is omitted: FX would show it whole without its matte.
pub(super) fn matte_layer(
    item_layers: &ItemLayers,
    matte: PrTrackMatte,
    range: Range<i64>,
    track_index: usize,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> std::result::Result<TrackMatte, String> {
    let layer = item_layers
        .get(&(matte.track_index, range.start))
        .map(|item| item.id())
        .ok_or_else(|| {
            format!(
                "track {track_index}, range {}..{} ticks: the matte clip on track {} was not converted; occurrence omitted",
                range.start, range.end, matte.track_index
            )
        })?;
    let mode = match matte.channel {
        PrMatteChannel::Alpha => TrackMatteType::Alpha,
        PrMatteChannel::AlphaInverted => TrackMatteType::AlphaInverted,
        PrMatteChannel::Luma => {
            approximate(omissions, record, LUMA_MATTE_APPROXIMATION);
            TrackMatteType::Luma
        }
    };
    Ok(TrackMatte { mode, layer })
}

/// `layer`, a root layer over a stage group's range, as the group's direct
/// child: on the group clock, where the group's Motion moves it with the
/// video. Its keys stay layer-local, and a group keeps its own children.
fn into_stage(
    layer: &Layer,
    group_id: LayerId,
    duration: Duration,
    _stage_on_document_clock: bool,
) -> Result<Layer> {
    let mut data = layer.data().clone();
    let window = TimeRangeProperty::new(Time::ZERO, duration);
    match &mut data {
        LayerData::Video(video) => {
            video.parent = Some(group_id);
            video.playback = super::timing::relocate_playback(&video.playback, window)?;
            return Ok(Layer::from_data(&data)?);
        }
        LayerData::Group(group) => {
            group.parent = Some(group_id);
            group.playback = super::timing::relocate_playback(&group.playback, window)?;
            return Ok(Layer::from_data(&data)?);
        }
        _ => {}
    }
    let (parent, active_range) = match &mut data {
        LayerData::Image(image) => (&mut image.parent, &mut image.active_range),
        LayerData::Rect(rect) => (&mut rect.parent, &mut rect.active_range),
        LayerData::Shape(shape) => (&mut shape.parent, &mut shape.active_range),
        LayerData::Text(text) => (&mut text.parent, &mut text.active_range),
        _ => {
            return Err(unsupported(format!(
                "a {} layer is not an imported matte",
                layer.layer_type_name()
            )));
        }
    };
    *parent = Some(group_id);
    *active_range = TimeRangeProperty::new(Time::ZERO, duration);
    Ok(Layer::from_data(&data)?)
}

/// A one-sided dissolve that imports as two editable Opacity keys on its
/// retained picture: from zero at a head's cut up to the picture's static
/// Opacity at the transition end, or from that Opacity down to zero at a
/// tail's cut. The keys replace the static value, so the authored Opacity
/// applies once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpacityDissolve {
    /// The measured default Film Impact Dissolve, on either side of a
    /// physical still, video or linked composition: a smoothstep.
    FilmImpact,
    /// A default Cross Dissolve (Legacy), only as the incoming-only head of a
    /// static Color Matte: a linear ramp, because its native curve is
    /// unmeasured.
    LegacyMatteHead,
}

impl OpacityDissolve {
    /// The name in the ramp's keyframe ids ([`keyframe_id`]).
    fn key_name(self) -> &'static str {
        match self {
            Self::FilmImpact => "film-impact-dissolve",
            Self::LegacyMatteHead => "cross-dissolve",
        }
    }

    /// The incoming easing of the ramp's second key.
    fn easing(self) -> PropertyKeyframeEasing {
        match self {
            Self::FilmImpact => PropertyKeyframeEasing::CubicBezier {
                x1: 1.0 / 3.0,
                y1: 0.0,
                x2: 2.0 / 3.0,
                y2: 1.0,
            },
            Self::LegacyMatteHead => PropertyKeyframeEasing::Linear,
        }
    }

    /// The report of a converted ramp.
    fn approximation(self) -> &'static str {
        match self {
            Self::FilmImpact => "Film Impact default one-sided dissolve retained as editable smoothstep opacity; temporal curve and SDR encoded-value compositing approximate the measured native linear-light fade",
            Self::LegacyMatteHead => "Cross Dissolve (Legacy) head retained as editable linear opacity from 0 to the Color Matte's static Opacity; the native Legacy curve, frame phase and compositing space are unmeasured",
        }
    }
}

/// The two measured Film Impact profiles own separate editable properties:
/// Dissolve opacity and Pop geometry. A default Cross Dissolve (Legacy)
/// converts only as the head of a static Color Matte
/// ([`OpacityDissolve::LegacyMatteHead`]). All remain declared approximations.
/// `matte_consumers` counts the placements that key each matte item, by the
/// matte's track and start, as [`video_layers`] counts them.
#[expect(clippy::too_many_arguments)]
fn import_transitions(
    project: &PrSequence,
    scope: &LayerScope<'_, '_, '_>,
    track_index: usize,
    layers: &[Layer],
    media: &BTreeMap<MediaId, PrMedia>,
    matte_consumers: &BTreeMap<(usize, i64), usize>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) {
    use crate::schema::PrVideoTransitionKind;
    let track = &project.video_tracks[track_index];
    pop::import_graphics(
        track,
        project.frame_rate,
        scope,
        track_index,
        layers,
        dynamics,
        omissions,
    );
    for transition in &track.transitions {
        if transition.kind == PrVideoTransitionKind::FilmImpactPop {
            if transition.outgoing_clip.is_some() && transition.incoming_clip.is_some() {
                continue;
            }
            match pop::tracks(
                transition,
                track,
                project.frame_rate,
                scope,
                track_index,
                layers,
                media,
            )
            .and_then(|tracks| set_tracks(dynamics, tracks))
            {
                Ok(()) => approximate(
                    omissions,
                    &transition.id,
                    "Film Impact default Pop retained as editable sampled scale/position; native early motion blur/fade and fitted curve remain approximate",
                ),
                Err(reason) => omit(
                    omissions,
                    OmissionScope::Feature,
                    &transition.id,
                    reason.to_string(),
                ),
            }
            continue;
        }
        let dissolve = match transition.kind {
            PrVideoTransitionKind::FilmImpactDissolve => OpacityDissolve::FilmImpact,
            PrVideoTransitionKind::CrossDissolve => OpacityDissolve::LegacyMatteHead,
            // Converted above.
            PrVideoTransitionKind::FilmImpactPop => continue,
        };
        let converted = (|| -> Result<()> {
            let (id, tail) = match (
                dissolve,
                transition.outgoing_clip.as_deref(),
                transition.incoming_clip.as_deref(),
            ) {
                (OpacityDissolve::FilmImpact, Some(id), None) => (id, true),
                (_, None, Some(id)) => (id, false),
                (OpacityDissolve::FilmImpact, ..) => {
                    return Err(unsupported("only a one-sided dissolve converts"))
                }
                (OpacityDissolve::LegacyMatteHead, ..) => {
                    return Err(unsupported("only an incoming-only head converts"))
                }
            };
            let clip = track
                .items
                .iter()
                .filter_map(PrVideoItem::media)
                .find(|clip| clip.id.as_deref() == Some(id))
                .ok_or_else(|| {
                    unsupported("one-sided dissolve has no retained physical occurrence")
                })?;
            let source_kind = media
                .get(&clip.media)
                .and_then(|media| media.video.as_ref())
                .map(|source| source.kind);
            let picture = match dissolve {
                OpacityDissolve::FilmImpact => matches!(
                    source_kind,
                    Some(
                        PrMediaKind::Video { .. }
                            | PrMediaKind::Still { .. }
                            | PrMediaKind::AfterEffectsComposition(_)
                    )
                ),
                OpacityDissolve::LegacyMatteHead => {
                    matches!(source_kind, Some(PrMediaKind::ColorMatte(_)))
                }
            };
            if dissolve == OpacityDissolve::LegacyMatteHead {
                if !picture {
                    return Err(unsupported(
                        "only a Color Matte's head converts; the native Legacy curve is unmeasured on other pictures",
                    ));
                }
                // A dissolving matte source is unmeasured, and the matte of a
                // keyed clip that conversion omits is dropped after the
                // transitions (`omit_unconsumed_mattes`).
                if matte_consumers.contains_key(&(track_index, clip.start_ticks)) {
                    return Err(unsupported(
                        "a Track Matte Key uses this Color Matte as its matte; a dissolving matte source is unmeasured",
                    ));
                }
            }
            if !picture
                || scope.parent.is_some()
                || clip.blend_mode != crate::schema::PrBlendMode::Normal
                || clip.playback_rate != 1.0
                || clip.time_remap.is_some()
                || !clip.crop.is_default()
                || clip.linear_wipe.is_some()
                || clip.opacity_mask.is_some()
                || clip.track_matte.is_some()
                || clip.active_transforms != 0
                || clip
                    .animations
                    .iter()
                    .any(|animation| animation.property() == PrAnimatedProperty::Opacity)
            {
                return Err(unsupported(
                    "one-sided dissolve requires an unstaged picture on the document clock with Normal blend, unit playback, and no animated opacity, masks or Transform stage",
                ));
            }
            let inside_clip = if tail {
                transition.cut_ticks == transition.end_ticks
                    && transition.end_ticks == clip.end_ticks
                    && transition.start_ticks >= clip.start_ticks
            } else {
                transition.cut_ticks == transition.start_ticks
                    && transition.start_ticks == clip.start_ticks
                    && transition.end_ticks <= clip.end_ticks
            };
            if !inside_clip {
                return Err(unsupported(
                    "one-sided dissolve must lie inside its linked clip and meet the cut at its boundary",
                ));
            }
            if track
                .transitions
                .iter()
                .filter(|other| {
                    other.kind == transition.kind
                        && (other.outgoing_clip.as_deref() == Some(id)
                            || other.incoming_clip.as_deref() == Some(id))
                })
                .count()
                != 1
            {
                return Err(unsupported(
                    "multiple dissolves target the same clip opacity",
                ));
            }
            let owner = scope
                .item_layers
                .get(&(track_index, clip.start_ticks))
                .copied()
                .ok_or_else(|| unsupported("one-sided dissolve picture was omitted"))?;
            let layer_id = owner.id();
            if !layers.iter().any(|layer| {
                layer.id() == layer_id
                    && match (dissolve, layer.data()) {
                        (
                            OpacityDissolve::FilmImpact,
                            LayerData::Image(_) | LayerData::Video(_),
                        )
                        | (OpacityDissolve::LegacyMatteHead, LayerData::Rect(_)) => true,
                        (OpacityDissolve::FilmImpact, LayerData::Group(_)) => {
                            matches!(source_kind, Some(PrMediaKind::AfterEffectsComposition(_)))
                                || matches!(owner, ItemLayer::Stroke(_))
                        }
                        _ => false,
                    }
            }) {
                return Err(unsupported(
                    "one-sided dissolve requires a retained picture opacity owner",
                ));
            }
            let origin = time_from_ticks(clip.start_ticks)?.as_millis();
            let start = time_from_ticks(transition.start_ticks)?.as_millis();
            let end = time_from_ticks(transition.end_ticks)?.as_millis();
            if start >= end {
                return Err(unsupported(
                    "one-sided dissolve collapses after millisecond rounding",
                ));
            }
            let local = |time: u64| -> Result<TimeOffset> {
                let millis = time
                    .checked_sub(origin)
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or_else(|| {
                        unsupported("one-sided dissolve local time exceeds the supported range")
                    })?;
                Ok(TimeOffset::from_millis(millis))
            };
            let keys = PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    keyframe_id(layer_id, dissolve.key_name(), 0),
                    local(start)?,
                    PropertyValue::Float(if tail { clip.opacity } else { 0.0 }),
                    PropertyKeyframeEasing::Linear,
                ),
                PropertyKeyframe::new(
                    keyframe_id(layer_id, dissolve.key_name(), 1),
                    local(end)?,
                    PropertyValue::Float(if tail { 0.0 } else { clip.opacity }),
                    dissolve.easing(),
                ),
            ])
            .map_err(|error| unsupported(format!("one-sided dissolve opacity keys: {error}")))?;
            dynamics
                .set_property(
                    Property::new(layer_id, PropType::Opacity),
                    PropertyAnimator::keyframes(keys),
                    Vec::new(),
                )
                .map_err(map_animation_graph_error)?;
            Ok(())
        })();
        match converted {
            Ok(()) => approximate(omissions, &transition.id, dissolve.approximation()),
            Err(reason) => {
                // An unconverted built-in dissolve keeps its detection report.
                let reason = match dissolve {
                    OpacityDissolve::FilmImpact => reason.to_string(),
                    OpacityDissolve::LegacyMatteHead => format!(
                        "Cross Dissolve detected; editable transition opacity/topology is not converted: {reason}"
                    ),
                };
                omit(
                    omissions,
                    OmissionScope::Feature,
                    &transition.id,
                    format!(
                        "range {}..{} ticks, cut {}: {reason}",
                        transition.start_ticks, transition.end_ticks, transition.cut_ticks
                    ),
                );
            }
        }
    }
}

/// What a video clip's picture layer shows.
#[derive(Clone, Copy)]
enum ClipPicture<'a> {
    /// A packaged video asset, as a video layer.
    Asset(&'a AssetId),
    /// The editable picture of the clip's linked After Effects composition,
    /// as a group on the clip clock whose content plays on the composition
    /// clock ([`LinkedCompositions`]).
    Linked,
}

/// A video clip to import and the media that it plays.
struct VideoClip<'a> {
    clip: &'a PrVideoOccurrence,
    source: &'a PrVideoStream,
    picture: ClipPicture<'a>,
    /// The clip's layer index; its layer id is one more.
    index: usize,
    /// The clip's track, bottom first.
    track_index: usize,
    /// The clip's Track Matte Key, naming the matte's root layer, which is
    /// already in the clip's sibling list.
    matte: Option<TrackMatte>,
    /// Whether another placement keys the same matte, which then stays a
    /// sibling of every clip that keys it.
    matte_shared: bool,
    /// A4 measured only an unshared canvas-sized still at default static Motion.
    static_matte: bool,
}

/// Why a keyed picture's Transform is omitted when its matte is outside the
/// form that A4 measured ([`measured_static_matte`]); the Track Matte Key
/// itself converts as without the Transform.
const UNMEASURED_MATTE_TRANSFORM_REASON: &str = "Transform with Track Matte Key requires a canvas-sized still matte, unshared and at default static Motion, opaque, normal blend and unretimed (measured A4); Transform omitted, existing Track Matte Key retained";

/// Whether the matte that `clip` keys is an unshared canvas-sized still at
/// default static Motion, the only matte A4 measured a Transform against.
fn measured_static_matte(
    project: &PrSequence,
    clip: &PrVideoOccurrence,
    media: &BTreeMap<MediaId, PrMedia>,
) -> bool {
    let Some(matte) = clip.track_matte else {
        return false;
    };
    let Some(source) = project.video_tracks[matte.track_index]
        .items
        .iter()
        .filter_map(PrVideoItem::media)
        .find(|source| source.timeline_ticks() == clip.timeline_ticks())
    else {
        return false;
    };
    source.transform == Default::default()
        && source.animations.is_empty()
        && source.effects.is_empty()
        && source.opacity == 100.0
        && source.blend_mode == crate::schema::PrBlendMode::Normal
        && source.playback_rate == 1.0
        && source.time_remap.is_none()
        && media
            .get(&source.media)
            .and_then(|source| source.video.as_ref())
            .is_some_and(|source| {
                source.kind.is_still()
                    && [source.width, source.height] == [project.width, project.height]
            })
}

/// Appends the layers of one video clip to `siblings`: its video (or linked
/// composition group), the guide of its Crop, Linear Wipe or Opacity mask
/// and, for a staged mask, the stage group that holds them and the matte moved
/// under it. Its keys go to `dynamics`. Returns the clip's root layer; an
/// omitted clip has no layers and takes no ids.
fn import_video_clip(
    video: VideoClip<'_>,
    project: &PrSequence,
    scope: &mut LayerScope<'_, '_, '_>,
    siblings: &mut Vec<Layer>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Option<ItemLayer>> {
    let VideoClip {
        clip,
        source,
        picture,
        index,
        track_index,
        matte,
        matte_shared,
        static_matte,
    } = video;
    // The decoder applies the container's quarter turn. Motion, masks and
    // sourceRect operate on its displayed picture without a second rotation.
    let mut displayed_source = source.clone();
    [displayed_source.width, displayed_source.height] = source.display_dimensions();
    let source = &displayed_source;
    // Deny only the newly measured Transform when the matte is shared or outside A4.
    // Every consumer sees the same remaining clip, so the ordinary matte path
    // keeps its existing ownership, clocks and omission behavior.
    let without_transform = clip
        .transform_stage(
            [source.width, source.height],
            [project.width, project.height],
        )
        .ok()
        .flatten()
        .filter(|(_, _, owner)| *owner == TransformOwner::KeyedPicture && !static_matte)
        .map(|(effect, _, _)| {
            let mut fallback = clip.clone();
            fallback.remove_effect(effect);
            omit(
                omissions,
                OmissionScope::Feature,
                clip.record(),
                UNMEASURED_MATTE_TRANSFORM_REASON,
            );
            fallback
        });
    let clip = without_transform.as_ref().unwrap_or(clip);
    let layer_id = LayerId::new(index as u64 + 1);
    let boundary = clip
        .mask_boundary([source.width, source.height], [project.width, project.height])
        .and_then(|boundary| {
            let Some(matte) = matte.as_ref().filter(|_| boundary == MaskBoundary::Staged)
            else {
                return Ok(boundary);
            };
            // A stage group moves its matte under itself, where another clip's
            // key could not evaluate it on that clip's clock.
            if matte_shared {
                return Err("a Track Matte Key whose matte keys another clip too is not converted on a clip that its Motion or effects stage");
            }
            // The matte's playback keys are on the sequence clock; under the
            // group they would read the group clock, which starts at the clip.
            let retimed = siblings.iter().any(|layer| {
                layer.id() == matte.layer
                    && matches!(layer.data(), LayerData::Video(video) if video.playback.time_remap().is_some())
            });
            if retimed {
                return Err(STAGED_RETIMED_MATTE_REASON);
            }
            Ok(boundary)
        });
    let boundary = match boundary {
        Ok(boundary) => boundary,
        Err(reason) => {
            omit(
                omissions,
                OmissionScope::Occurrence,
                clip.record(),
                format!(
                    "track {track_index}, range {}..{} ticks: {reason}; occurrence omitted",
                    clip.start_ticks, clip.end_ticks
                ),
            );
            return Ok(None);
        }
    };
    let frame = PositiveRect::new(RectBounds::from_size(
        source.width.into(),
        source.height.into(),
    ))
    .ok_or_else(|| unsupported("video dimensions must be positive"))?;
    let record = clip.record();
    let Some(ClipTiming {
        active_range,
        video_range,
        source_range,
        source_intrinsic_duration,
        playback,
    }) = clip_timing(clip, source, boundary, record, omissions)?
    else {
        return Ok(None);
    };
    let retimed_keys = retimed_keys_reason(clip);
    let transform = clip_transform(
        &clip.transform,
        clip.opacity,
        [source.width, source.height],
        [project.width, project.height],
    )?;
    let mut masks = Vec::new();
    let mut guides = Vec::new();
    let mut tracks = Vec::new();
    // An omitted clip takes no id.
    let first_index = *scope.next_index;
    // The guide and mask take their ids as for a flat clip, so ids stay
    // the same without a stage group; the group takes the next one.
    let mask_ids =
        (!clip.crop.is_default() || clip.linear_wipe.is_some() || clip.opacity_mask.is_some())
            .then(|| {
                let ids = (
                    LayerId::new(*scope.next_index as u64 + 1),
                    FxItemId::new(*scope.next_index as u64 + 2),
                );
                *scope.next_index += 2;
                ids
            });
    let group_id = (boundary == MaskBoundary::Staged).then(|| {
        let group_id = LayerId::new(*scope.next_index as u64 + 1);
        *scope.next_index += 1;
        group_id
    });
    // Whether the stage group's clock, and the picture's parent clock, is the
    // document clock: a stage group or nest that starts later offsets it.
    let stage_on_document_clock = scope.on_document_clock && active_range.start == Time::ZERO;
    let picture_on_document_clock = match group_id {
        Some(_) => stage_on_document_clock,
        None => scope.on_document_clock,
    };
    // E11's Transform belongs to the video; A4's belongs to the whole
    // Alpha-keyed picture, including its static matte.
    let transform_stage = clip
        .transform_stage(
            [source.width, source.height],
            [project.width, project.height],
        )
        .ok()
        .flatten()
        .map(|(index, effect_transform, owner)| (&clip.effects[index], effect_transform, owner));
    let motion_blur_angle =
        transform_stage.and_then(|(_, transform, _)| transform.motion_blur_shutter_angle());
    // A stage group takes the clip's place, range, visibility, Motion
    // and Opacity. Under it, the video and its mask guide are in the
    // video's frame, or the video is placed by its Transform.
    let (video_parent, video_transform, video_hidden) = match (group_id, transform_stage) {
        (Some(group_id), Some((_, effect_transform, TransformOwner::Video))) => (
            Some(group_id),
            staged_video_transform(effect_transform, [source.width, source.height])?,
            false,
        ),
        (Some(group_id), None | Some((_, _, TransformOwner::KeyedPicture))) => {
            (Some(group_id), identity_transform(), false)
        }
        (None, _) => (scope.parent, transform, !clip.enabled),
    };
    // A stage group carries the Motion and Opacity keys. A flat Crop or
    // Opacity mask guide repeats the video's Motion keys, so that the mask
    // stays in the video's frame at every time.
    let motion_owner = group_id.unwrap_or(layer_id);
    let frame_guide = mask_ids
        .filter(|_| group_id.is_none() && (!clip.crop.is_default() || clip.opacity_mask.is_some()))
        .map(|(guide_id, _)| guide_id);
    let mut layer_tracks = Vec::new();
    // A linked clip's Transform blur, which it requests once its picture forms.
    let mut linked_transform_blur = None;
    // Transform keys follow the Motion keys' clock rules, on their measured
    // owner: the video (E11) or the keyed-picture group (A4).
    if let Some((effect, effect_transform, owner)) = transform_stage {
        if let Some(reason) = effect_transform.unretained_skew_axis() {
            omit(omissions, OmissionScope::Feature, record, reason);
        }
        for warning in effect_transform.approximations(&effect.animations) {
            approximate(omissions, record, warning);
        }
        if let Some(reason) = effect_transform.unimported_scale_width_keys(&effect.animations) {
            omit(omissions, OmissionScope::Feature, record, reason);
        }
        if let Some(angle) = motion_blur_angle {
            let settings = CompositionShutter::transform(angle)?;
            match picture {
                ClipPicture::Asset(_) => CompositionShutter::request(
                    scope.composition_shutter,
                    settings,
                    record,
                    omissions,
                ),
                ClipPicture::Linked => linked_transform_blur = Some(settings),
            }
        }
        let converted = match retimed_keys {
            Some(reason) if !effect.animations.is_empty() => Err(reason.to_owned()),
            _ => transform_stage_tracks(
                effect,
                effect_transform,
                [source.width, source.height],
                clip.in_ticks,
                match owner {
                    TransformOwner::Video => layer_id,
                    TransformOwner::KeyedPicture => group_id.expect("a keyed picture stages"),
                },
            )
            .map_err(|error| error.to_string()),
        };
        match converted {
            Ok(tracks) => layer_tracks.extend(tracks),
            Err(reason) => omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("Transform animation was not imported: {reason}; static values were kept"),
            ),
        }
    }
    layer_tracks.extend(motion_tracks(
        &clip.animations,
        clip.in_ticks,
        retimed_keys,
        motion_owner,
        frame_guide,
        [source.width, source.height],
        [project.width, project.height],
        record,
        omissions,
    ));
    if let (false, Some((guide_id, mask_id))) = (clip.crop.is_default(), mask_ids) {
        if clip.crop.edge_feather != 0.0 {
            approximate(
                omissions,
                record,
                "Crop Edge Feather is approximated by an FX mask; Premiere feather visuals are not preserved exactly",
            );
        }
        let source_width = f64::from(source.width);
        let source_height = f64::from(source.height);
        let mut rect = black_shape(source.width, source.height);
        rect.position = [
            source_width * clip.crop.left / 100.0,
            source_height * clip.crop.top / 100.0,
        ];
        rect.size = [
            source_width * (100.0 - clip.crop.left - clip.crop.right) / 100.0,
            source_height * (100.0 - clip.crop.top - clip.crop.bottom) / 100.0,
        ];
        guides.push(Layer::from_data(&fx_schema::LayerData::Rect(guide_layer(
            guide_id,
            format!("Premiere Crop guide {}", index + 1),
            video_parent,
            video_range,
            // Premiere applies this standard Crop before its fixed Motion
            // effect, so the editable guide must share the video's
            // transform, and a flat guide also its Motion keys (above).
            video_transform,
            rect,
        )))?);
        // FX masks cannot represent negative feather widths. Keep the
        // crop geometry and report the missing feather above.
        masks.push(guide_mask(
            mask_id,
            guide_id,
            clip.crop.edge_feather.max(0.0),
        ));
    }
    if let (Some(wipe), Some((guide_id, mask_id))) = (&clip.linear_wipe, mask_ids) {
        // The classifier stages every wipe whose clip frame moves, so
        // the canvas-sized guide shares the video's unmoved frame.
        let guide = match retimed_keys {
            Some(reason) => Err(unsupported(reason)),
            None => linear_wipe_guide(
                wipe,
                clip.in_ticks,
                guide_id,
                [project.width, project.height],
            ),
        };
        // The clip is not imported opaque without its wipe.
        let (transform, property_type, track) = match guide {
            Ok(guide) => guide,
            Err(error) => {
                *scope.next_index = first_index;
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    record,
                    format!("Linear Wipe was not imported: {error}"),
                );
                return Ok(None);
            }
        };
        guides.push(Layer::from_data(&fx_schema::LayerData::Rect(guide_layer(
            guide_id,
            format!("{LINEAR_WIPE_GUIDE_PREFIX}{}", index + 1),
            video_parent,
            video_range,
            transform,
            black_shape(project.width, project.height),
        )))?);
        masks.push(guide_mask(mask_id, guide_id, wipe.feather));
        tracks.push((Property::new(guide_id, property_type), track));
    }
    if let (Some(mask), Some((guide_id, mask_id))) = (&clip.opacity_mask, mask_ids) {
        if mask.feather != 0.0 {
            approximate(omissions, record, crate::schema::MASK_FEATHER_APPROXIMATION);
        }
        // The corpus stores the outline in unit fractions of the source frame;
        // the guide draws it in source pixels, in the
        // video's frame like the Crop guide.
        guides.push(Layer::from_data(&fx_schema::LayerData::Shape(
            shape_guide(
                guide_id,
                format!("Premiere Opacity mask {}", index + 1),
                video_parent,
                video_range,
                video_transform,
                scaled_path(
                    &fx_path(&mask.path),
                    [f64::from(source.width), f64::from(source.height)],
                ),
            ),
        ))?);
        let mut path_mask = guide_mask(mask_id, guide_id, mask.feather);
        path_mask.inverted = mask.inverted;
        path_mask.opacity = NonNegativeProperty::new(mask.opacity / 100.0)
            .ok_or_else(|| unsupported("Mask Opacity must be between 0 and 100"))?;
        masks.push(path_mask);
    }
    let (video_masks, group_masks) = match group_id {
        Some(_) => (Vec::new(), masks),
        None => (masks, Vec::new()),
    };
    // A flat clip's matte stays its sibling; a stage group takes the matte
    // as its direct child, so that its Motion moves the matte with the video,
    // as Premiere keys the clip before its Motion.
    let (video_matte, group_matte) = match group_id {
        Some(_) => (None, matte),
        None => (matte, None),
    };
    // A stage group blends the clip's whole picture, masks and effects
    // applied, onto the tracks below; its picture composites normally inside.
    let (video_blend, group_blend) = match group_id {
        Some(_) => (BlendMode::Normal, clip.blend_mode.fx_mode()),
        None => (clip.blend_mode.fx_mode(), BlendMode::Normal),
    };
    let (effects, mut effect_tracks) = effects::import_effects(
        clip,
        layer_id,
        boundary,
        [source.width, source.height],
        [project.width, project.height],
        scope.effect_ids,
        omissions,
    );
    // Guide tracks precede the clip's own, which keeps the document's track order.
    tracks.extend(layer_tracks);
    let picture = match picture {
        ClipPicture::Asset(asset_id) => {
            let layer = VideoLayer {
                id: layer_id,
                name: format!("Premiere video {}", index + 1),
                description: String::new(),
                metadata: None,
                is_hidden: video_hidden,
                parent: video_parent,
                start_time: None,
                blend_mode: video_blend,
                track_matte: video_matte,
                masks: video_masks,
                corner_radius: None,
                source_range,
                playback: match playback {
                    Some(property) => fx_schema::LayerPlayback::remapped(video_range, property, 0),
                    None => {
                        fx_schema::LayerPlayback::linear(video_range, video_range, source_range, 0)
                    }
                }
                .map_err(unsupported)?,
                preserve_audio_pitch: false,
                source_intrinsic_duration,
                volume: Some(LinearGain::ZERO),
                effects,
                placement: None,
                captions_enabled: None,
                caption_presentation: None,
                frame_blending: clip.frame_blending.map(|mode| match mode {
                    fx_schema::FrameBlendingMode::Simple => {
                        fx_schema::layer::FrameBlendingData::Boolean(true)
                    }
                    mode => fx_schema::layer::FrameBlendingData::Mode(mode),
                }),
                motion_blur: motion_blur_angle.is_some(),
                transform: video_transform,
                source: VideoSource::from_asset(asset_id.clone(), Some(frame), MediaFit::Contain),
            };
            validate_time_range("playback.inputRange", layer.playback.input_range())?;
            validate_time_range("source_range", layer.source_range)?;
            if layer.source_intrinsic_duration.is_zero() {
                return Err(EditableBuildError::invalid_input(
                    "source_intrinsic_duration",
                    "must be greater than zero",
                )
                .into());
            }
            LayerData::Video(layer)
        }
        ClipPicture::Linked => {
            // The clip group keeps the clip's Motion, Opacity and effect keys
            // on the clip clock, as a video layer does; a group's playback
            // also remaps its own keys, so it plays at most the identity-rate
            // seed below. Only the composition plays from the source In, at
            // the clip's speed or time-remapped: a source group under the
            // clip group maps the clip clock onto the composition clock, as a
            // video's source range and playback do.
            let canvas = [source.width, source.height];
            let clock = match playback {
                Some(playback) => Some(playback),
                None if source_range.start != Time::ZERO => {
                    Some(constant_time_remap(false, video_range, source_range)?)
                }
                None => None,
            };
            let clock = match clock
                .map(|clock| on_clip_clock(&clock, video_range.start))
                .transpose()
            {
                Ok(clock) => clock,
                Err(error) => {
                    *scope.next_index = first_index;
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        record,
                        format!("linked composition clock was not imported: {error}"),
                    );
                    return Ok(None);
                }
            };
            let source = clock.map(|clock| {
                let id = LayerId::new(*scope.next_index as u64 + 1);
                *scope.next_index += 1;
                (id, clock)
            });
            let (root, composition_blur) = match super::after_effects::linked_root(
                &clip.media,
                source.as_ref().map_or(layer_id, |(id, _)| *id),
                source_range.end(),
                super::linked_shadow_scale::placement(
                    clip,
                    &transform,
                    scope.parent.is_some(),
                    transform_stage.is_some(),
                ),
                scope,
                dynamics,
                omissions,
            )? {
                Ok(picture) => picture,
                Err(reason) => {
                    *scope.next_index = first_index;
                    omit(omissions, OmissionScope::Occurrence, record, reason);
                    return Ok(None);
                }
            };
            // Only a clip whose picture forms requests the composition's
            // shutter: its Transform's blur, then its composition's own.
            for settings in linked_transform_blur.into_iter().chain(composition_blur) {
                CompositionShutter::request(scope.composition_shutter, settings, record, omissions);
            }
            // The canvas guide and its mask take the ids after the picture's.
            let canvas_guide = LayerId::new(*scope.next_index as u64 + 1);
            let canvas_mask = FxItemId::new(*scope.next_index as u64 + 2);
            *scope.next_index += 2;
            let picture =
                super::after_effects::clip_to_canvas(root, canvas, canvas_guide, canvas_mask)?;
            let content = match source {
                Some((id, clock)) => vec![super::after_effects::source_group(
                    id,
                    format!("Premiere linked source {}", index + 1),
                    layer_id,
                    video_range.duration,
                    clock,
                    picture.into(),
                )?],
                None => picture.into(),
            };
            // On the document clock the seed starts the runtime's remap chain
            // here ([`document_clock_seed`]); under a stage group or nest that
            // starts later, no seed keeps remapped animation on time.
            let seed = if picture_on_document_clock {
                Some(document_clock_seed(video_range)?)
            } else {
                if content.iter().any(super::after_effects::contains_playback) {
                    approximate(
                        omissions,
                        record,
                        super::after_effects::offset_clock_note("linked composition"),
                    );
                }
                None
            };
            if clip.frame_blending.is_some() {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    "frame blending of a linked After Effects composition is not converted",
                );
            }
            // Native keys are on the source clock, which the clip clock of
            // the group's effects matches only under unit forward playback.
            if let Some(reason) = retimed_keys.filter(|_| !effect_tracks.is_empty()) {
                effect_tracks.clear();
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!("effect animation was not imported: {reason}; static values were kept"),
                );
            }
            let group = GroupLayer {
                is_hidden: video_hidden,
                parent: video_parent,
                blend_mode: video_blend,
                track_matte: video_matte,
                masks: video_masks,
                playback: match seed {
                    Some(property) => fx_schema::LayerPlayback::remapped(video_range, property, 0),
                    None => fx_schema::LayerPlayback::linear(
                        video_range,
                        video_range,
                        TimeRangeProperty::new(Time::ZERO, video_range.duration),
                        0,
                    ),
                }
                .map_err(unsupported)?,
                effects,
                motion_blur: motion_blur_angle.is_some(),
                ..plain_group(
                    layer_id,
                    format!("Premiere linked composition {}", index + 1),
                    video_range,
                    video_transform,
                    content,
                )?
            };
            validate_time_range("playback.inputRange", group.playback.input_range())?;
            LayerData::Group(group)
        }
    };
    let (picture, stroke_owner) = match (&picture, clip.stroke) {
        (LayerData::Video(video), Some(profile)) => match stroke::validate(clip, source, scope) {
            Ok(()) => {
                approximate(
                    omissions,
                    record,
                    "Film Impact Stroke retained as editable centered prescale with the measured neutral-profile border approximation; general Size and alpha semantics remain unsupported",
                );
                (stroke::wrap(video.clone(), profile, source, scope)?, true)
            }
            Err(reason) => {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    reason.to_string(),
                );
                (Layer::from_data(&picture)?, false)
            }
        },
        (_, Some(_)) => {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                "Film Impact Stroke requires an opaque physical video",
            );
            (Layer::from_data(&picture)?, false)
        }
        _ => (Layer::from_data(&picture)?, false),
    };
    let mut clip_layers = vec![picture];
    clip_layers.extend(guides);
    let root = group_id.unwrap_or(layer_id);
    let layers = match group_id {
        None => clip_layers,
        Some(group_id) => {
            if let Some(matte) = &group_matte {
                let position = siblings
                    .iter()
                    .position(|layer| layer.id() == matte.layer)
                    .ok_or_else(|| unsupported("the matte layer is not beside its clip"))?;
                let source = siblings.remove(position);
                if !stage_on_document_clock && super::after_effects::remaps_linked_content(&source)
                {
                    approximate(
                        omissions,
                        record,
                        super::after_effects::offset_clock_note(
                            "Track Matte Key's linked composition matte",
                        ),
                    );
                }
                clip_layers.push(into_stage(
                    &source,
                    group_id,
                    active_range.duration,
                    stage_on_document_clock,
                )?);
            }
            let group_transform = match transform_stage {
                Some((_, effect_transform, TransformOwner::KeyedPicture)) => {
                    staged_video_transform(effect_transform, [source.width, source.height])?
                }
                _ => transform,
            };
            let group = GroupLayer {
                is_hidden: !clip.enabled,
                parent: scope.parent,
                blend_mode: group_blend,
                track_matte: group_matte,
                masks: group_masks,
                ..plain_group(
                    group_id,
                    format!(
                        "{}{}",
                        super::tesseract_to_premiere::STAGE_GROUP_NAME,
                        index + 1
                    ),
                    active_range,
                    group_transform,
                    clip_layers,
                )?
            };
            validate_time_range("playback.inputRange", group.playback.input_range())?;
            vec![Layer::from_data(&fx_schema::LayerData::Group(group))?]
        }
    };
    set_tracks(dynamics, tracks)?;
    for (target, track) in effect_tracks {
        dynamics
            .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
            .map_err(map_animation_graph_error)?;
    }
    if let Some(warning) = clip.blend_mode.approximation() {
        approximate(omissions, record, warning);
    }
    siblings.extend(layers);
    Ok(Some(if stroke_owner {
        ItemLayer::Stroke(root)
    } else {
        ItemLayer::Plain(root)
    }))
}

/// The FX transform of a placement's static Motion `transform` and `opacity`,
/// whose picture is `source` pixels in a `canvas`: Premiere's Anchor Point is a fraction of
/// the picture and its Position a fraction of the canvas, so a default Motion
/// centres the picture at its native size (Adobe-measured for video and for
/// stills of three sizes).
pub(super) fn clip_transform(
    motion: &PrStaticTransform,
    opacity: f64,
    source: [u32; 2],
    canvas: [u32; 2],
) -> Result<Transform> {
    let mut transform = identity_transform();
    transform.anchor_point = [
        motion.anchor_point[0] * f64::from(source[0]),
        motion.anchor_point[1] * f64::from(source[1]),
    ];
    transform.position = fx_schema::Position::xy(
        motion.position[0] * f64::from(canvas[0]),
        motion.position[1] * f64::from(canvas[1]),
    );
    transform.scale = motion.scale;
    transform.rotation = motion.rotation;
    transform.opacity = PercentageProperty::new(opacity)
        .ok_or_else(|| unsupported("Premiere opacity must be between 0 and 100"))?;
    Ok(transform)
}

/// Where an imported clip plays on the FX clocks.
struct ClipTiming {
    /// The clip's range on the parent clock.
    active_range: TimeRangeProperty,
    /// The video's range: the clip's own, or under a stage group the same
    /// span on the group clock.
    video_range: TimeRangeProperty,
    source_range: TimeRangeProperty,
    source_intrinsic_duration: Duration,
    playback: Option<TimeRemapProperty>,
}

/// The timing of `clip`, which plays `source` and whose mask is at
/// `boundary`. `None` omits the clip, with its reason in `omissions`: a time
/// remap that cannot import.
fn clip_timing(
    clip: &PrVideoOccurrence,
    source: &PrVideoStream,
    boundary: MaskBoundary,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<Option<ClipTiming>> {
    let active_range = tick_range(clip.start_ticks, clip.end_ticks)?;
    // Native validation already bounds the final-frame hold. Unit-speed clips
    // keep equal durations when rounding changes endpoints; retimed clips must
    // retain their distinct source span for editable playback keyframes.
    // Premiere stores reverse bounds measured backward from the media end.
    // Convert them to forward source-clock endpoints before constructing keys.
    let (source_in, source_out) = if clip.playback_rate < 0.0 {
        (
            source
                .intrinsic_ticks
                .checked_sub(clip.out_ticks)
                .ok_or_else(|| unsupported("reverse source out exceeds intrinsic duration"))?,
            source
                .intrinsic_ticks
                .checked_sub(clip.in_ticks)
                .ok_or_else(|| unsupported("reverse source in exceeds intrinsic duration"))?,
        )
    } else {
        (clip.in_ticks, clip.out_ticks)
    };
    let source_intrinsic_duration = duration_from_ticks(source.intrinsic_ticks)?;
    let source_range = if clip.time_remap.is_some() {
        // A native ramp's source keys address the full media clock.
        TimeRangeProperty::new(Time::ZERO, source_intrinsic_duration)
    } else {
        let mut range = tick_range(source_in, source_out)?;
        if clip.playback_rate == 1.0 {
            range.duration = active_range.duration;
        }
        range
    };
    // Under a stage group, the video and its mask guide are on the
    // group clock, which starts at the clip start.
    let (video_range, clock_origin) = match boundary {
        MaskBoundary::Staged => (TimeRangeProperty::new(Time::ZERO, active_range.duration), 0),
        MaskBoundary::Flat => (active_range, clip.start_ticks),
    };
    let playback = if let Some(remap) = &clip.time_remap {
        // Remap key times are on the parent clock.
        match time_remap_property(remap, clock_origin) {
            Ok(playback) => Some(playback),
            // No unit-speed window would show the remapped source frames.
            Err(error) => {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    record,
                    format!("clip was not imported: {error}"),
                );
                return Ok(None);
            }
        }
    } else if clip.playback_rate != 1.0 {
        Some(constant_time_remap(
            clip.playback_rate.is_sign_negative(),
            video_range,
            source_range,
        )?)
    } else {
        None
    };
    Ok(Some(ClipTiming {
        active_range,
        video_range,
        source_range,
        source_intrinsic_duration,
        playback,
    }))
}

/// The key tracks of a placement's Motion and Opacity `animations`, whose
/// source clock starts at `in_ticks` and whose picture is `source` pixels in
/// a `canvas`, on `motion_owner` and, except Opacity, on the flat Crop or
/// Opacity mask guide `frame_guide`. Position keys scale by the canvas and
/// Anchor Point keys by the source, as [`clip_transform`] scales their static
/// values; Scale Width keys move the X axis alone. A property whose keys
/// cannot import is reported and keeps its static value, and so is every key
/// when `retimed_keys` gives the reason.
#[expect(
    clippy::too_many_arguments,
    reason = "nest placements share this with clips; Anchor Point keys need the source frame"
)]
pub(super) fn motion_tracks(
    animations: &[PrPropertyAnimation],
    in_ticks: i64,
    retimed_keys: Option<&'static str>,
    motion_owner: LayerId,
    frame_guide: Option<LayerId>,
    source: [u32; 2],
    canvas: [u32; 2],
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Vec<(Property, PropertyKeyframeTrack)> {
    let mut layer_tracks = Vec::new();
    for animation in animations {
        if let Some(reason) = retimed_keys {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "{:?} animation was not imported: {reason}; static values were kept",
                    animation.property()
                ),
            );
            continue;
        }
        let guide = frame_guide.filter(|_| animation.property() != PrAnimatedProperty::Opacity);
        let owners = [Some(motion_owner), guide];
        let point = match animation {
            PrPropertyAnimation::Position(keys) => Some((keys, &POSITION_TRACKS, canvas)),
            PrPropertyAnimation::AnchorPoint(keys) => Some((keys, &ANCHOR_POINT_TRACKS, source)),
            PrPropertyAnimation::Opacity(_)
            | PrPropertyAnimation::Rotation(_)
            | PrPropertyAnimation::UniformScale(_)
            | PrPropertyAnimation::ScaleWidth(_) => None,
        };
        if let Some((keys, point, dimensions)) = point {
            for owner in owners.into_iter().flatten() {
                match point_property_tracks(keys, in_ticks, owner, point, dimensions) {
                    Ok(axes) => layer_tracks.extend(
                        axes.map(|(property, track)| (Property::new(owner, property), track)),
                    ),
                    Err(error) => {
                        omit(
                            omissions,
                            OmissionScope::Feature,
                            record,
                            format!("{} animation was not imported: {error}", point.name),
                        );
                        break;
                    }
                }
            }
            continue;
        }
        let properties: &[PropType] = match animation.property() {
            PrAnimatedProperty::Opacity => &[PropType::Opacity],
            PrAnimatedProperty::Position | PrAnimatedProperty::AnchorPoint => &[],
            PrAnimatedProperty::Rotation => &[PropType::Rotation],
            PrAnimatedProperty::UniformScale => &[PropType::ScaleX, PropType::ScaleY],
            PrAnimatedProperty::ScaleWidth => &[PropType::ScaleX],
        };
        for &property_type in properties {
            for owner in owners.into_iter().flatten() {
                match scalar_keys(animation, in_ticks, owner, property_type) {
                    Ok(track) => {
                        layer_tracks.push((Property::new(owner, property_type), track));
                    }
                    Err(error) => {
                        omit(
                            omissions,
                            OmissionScope::Feature,
                            record,
                            format!("{property_type:?} animation was not imported: {error}"),
                        );
                        break;
                    }
                }
            }
        }
    }
    layer_tracks
}

/// The guide layer of a clip's Crop or Linear Wipe mask, or of a linked
/// picture's canvas: a plain rectangle beside the layer that it masks.
pub(super) fn guide_layer(
    id: LayerId,
    name: String,
    parent: Option<LayerId>,
    active_range: TimeRangeProperty,
    transform: Transform,
    rect: RectShape,
) -> RectLayer {
    RectLayer {
        id,
        name,
        description: String::new(),
        is_hidden: false,
        parent,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range,
        effects: Vec::new(),
        motion_blur: false,
        transform,
        rect,
    }
}

/// The guide layer of a clip's Opacity mask: its outline, in source pixels,
/// beside the video. A guide never paints, so it has no fill or stroke.
fn shape_guide(
    id: LayerId,
    name: String,
    parent: Option<LayerId>,
    active_range: TimeRangeProperty,
    transform: Transform,
    path: ShapePath,
) -> ShapeLayer {
    ShapeLayer {
        id,
        name,
        description: String::new(),
        is_hidden: false,
        parent,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range,
        effects: Vec::new(),
        motion_blur: false,
        transform,
        shape: ShapeContent {
            path,
            fills: Vec::new(),
            strokes: Vec::new(),
            round_corners: None,
            offset_paths: None,
            trim: None,
            poly_star: None,
            ellipse: None,
        },
    }
}

/// The Add mask whose shape is the guide `guide_id`.
pub(super) fn guide_mask(id: FxItemId, guide_id: LayerId, feather: f64) -> PathMask {
    PathMask {
        id,
        mode: MaskMode::Add,
        inverted: false,
        layer: Some(guide_id),
        legacy_path: None,
        feather: [feather; 2],
        expansion: 0.0,
        opacity: NonNegativeProperty::new(1.0).expect("one is a valid mask opacity"),
    }
}

/// Reports each font of the converted text once. The document packages media
/// only, so `tsrct` needs every font imported before it renders the text.
fn report_unpackaged_fonts(project: &PrSequence, omissions: &mut Vec<Omission>) {
    // `video_items` order is layer order: bottom track first, then timeline order.
    let mut uses: BTreeMap<&str, (String, usize)> = BTreeMap::new();
    for (index, item) in project.video_items().enumerate() {
        let Some(graphic) = item.graphic() else {
            continue;
        };
        for (name, document) in graphic.text_documents() {
            let (_, count) = uses.entry(document.font.as_str()).or_insert_with(|| {
                let name = if name.is_empty() {
                    format!("Premiere text {}", index + 1)
                } else {
                    name.to_owned()
                };
                let record = graphic.id().unwrap_or("text graphic");
                (format!("{record} ({name:?})"), 0)
            });
            *count += 1;
        }
    }
    for (font, (first, count)) in uses {
        let record = match count - 1 {
            0 => first,
            1 => format!("{first} and 1 more text layer"),
            more => format!("{first} and {more} more text layers"),
        };
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            fonts::not_packaged(font, "preview or export"),
        );
    }
}

pub(super) fn validate_time_range(field: &'static str, range: TimeRangeProperty) -> Result<()> {
    if range.duration.is_zero() {
        return Err(
            EditableBuildError::invalid_input(field, "duration must be greater than zero").into(),
        );
    }
    if range.start.checked_add_duration(range.duration).is_none() {
        return Err(EditableBuildError::invalid_input(field, "must not overflow start").into());
    }
    Ok(())
}

pub(super) fn map_animation_graph_error(source: AnimationGraphError) -> BuildError {
    let (field, reason) = match &source {
        AnimationGraphError::UnknownProperty(_) => {
            ("dependencies", "must reference existing graph entries")
        }
        AnimationGraphError::Cycle { .. } => ("dependencies", "must not create a cycle"),
        AnimationGraphError::TooManyLayerRefs { .. } => {
            ("layer_refs", "must contain at most 16 entries")
        }
        AnimationGraphError::LayerRefsRequireScript { .. } => (
            "layer_refs",
            "can only be attached to a JavaScript animator",
        ),
        AnimationGraphError::AnimatedLayerRefSource { .. } => (
            "layer_refs",
            "referenced layers must have a stable backing asset",
        ),
        AnimationGraphError::UnboundedAnimator { .. } => (
            "animator",
            "an asset-id property must use a constant animator with a finite, enumerable range so required resources stay deterministic",
        ),
        AnimationGraphError::NonEnumerableValue { .. } => (
            "animator",
            "an asset-id property must use a constant string asset id so required resources stay enumerable",
        ),
        AnimationGraphError::ReadOnlyProperty(_) => {
            ("property", "is a read-only property and cannot be animated")
        }
        AnimationGraphError::InvalidKeyframes { .. }
        | AnimationGraphError::DuplicateKeyframeId { .. }
        | AnimationGraphError::DuplicateProperty(_)
        | AnimationGraphError::Wire(_) => ("animator", "contains invalid property keyframe data"),
    };
    EditableBuildError::AnimationGraph {
        field,
        reason,
        source: Box::new(source),
    }
    .into()
}

pub(super) fn set_tracks(
    dynamics: &mut AnimationGraph,
    tracks: Vec<(Property, PropertyKeyframeTrack)>,
) -> Result<()> {
    for (property, track) in tracks {
        dynamics
            .set_property(property, PropertyAnimator::keyframes(track), Vec::new())
            .map_err(map_animation_graph_error)?;
    }
    Ok(())
}

/// The layer name of a converted graphic; `index` is its layer position.
fn text_layer_name(text: &PrText, index: usize) -> String {
    if text.name.is_empty() {
        format!("Premiere text {}", index + 1)
    } else {
        text.name.clone()
    }
}

/// Map one text object of `graphic` to an editable FX text layer, with the
/// graphic's range and visibility.
///
/// Converted text keeps Premiere's PostScript font name verbatim as its family
/// with an empty style; no font catalog is consulted.
pub(super) fn text_layer(
    graphic: &PrGraphic,
    text: &PrText,
    layer_id: LayerId,
    index: usize,
) -> Result<TextLayer> {
    let doc = &text.document;
    let size = f64::from(doc.size);
    let (box_size, vertical_align) = match doc.frame {
        PrTextFrame::Point { .. } => (None, None),
        PrTextFrame::Box {
            width,
            height,
            vertical,
        } => (
            Some([f64::from(width), f64::from(height)]),
            Some(match vertical {
                PrVerticalAlign::Top => VerticalAlign::Top,
                PrVerticalAlign::Center => VerticalAlign::Center,
                PrVerticalAlign::Bottom => VerticalAlign::Bottom,
            }),
        ),
    };
    let leading = Some(
        PositiveProperty::new(super::text::line_spacing(doc))
            .ok_or_else(|| unsupported("text line spacing must be positive"))?,
    );
    let source_text = TextDocument {
        text: doc.text.clone(),
        font_family: Arc::from(doc.font.as_str()),
        font_style: Arc::from(""),
        font_size: PositiveProperty::new(size)
            .ok_or_else(|| unsupported("text size must be positive"))?,
        font_variations: None,
        apply_fill: doc.fill.is_some(),
        fill_color: doc.fill.map_or([1.0; 4], rgba),
        apply_stroke: doc.stroke.is_some(),
        stroke_color: doc.stroke.map(|stroke| rgba(stroke.color)),
        stroke_width: NonNegativeProperty::new(
            doc.stroke
                .map_or(0.0, |stroke| STROKE_WIDTH_RATIO * f64::from(stroke.width)),
        )
        .ok_or_else(|| unsupported("text stroke width must be nonnegative"))?,
        stroke_over_fill: false,
        justification: match doc.justification {
            PrJustification::Left => Justification::Left,
            PrJustification::Right => Justification::Right,
            PrJustification::Center => Justification::Center,
            PrJustification::Justify => Justification::Justify,
        },
        tracking: f64::from(doc.tracking),
        leading,
        baseline_shift: 0.0,
        box_text: box_size.is_some(),
        scale_box_text_with_transform: false,
        box_size,
        // Premiere's box starts at the layer origin.
        box_position: box_size.map(|_| [0.0, 0.0]),
        box_first_baseline: None,
        all_caps: doc.all_caps,
        underline: false,
        strikethrough: false,
        vertical_align,
    };
    let mut transform = object_transform(&text.transform, "text")?;
    transform.anchor_point[1] += super::text::point_anchor_offset(doc);
    Ok(TextLayer {
        id: layer_id,
        name: text_layer_name(text, index),
        description: String::new(),
        is_hidden: !graphic.enabled,
        parent: None,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range: tick_range(graphic.start_ticks, graphic.end_ticks)?,
        effects: Vec::new(),
        motion_blur: false,
        transform,
        source_text,
        animators: Vec::new(),
        path_options: None,
        anchor_options: None,
    })
}

/// The FX transform of a graphic `owner` object's transform.
pub(super) fn object_transform(placement: &PrTextTransform, owner: &str) -> Result<Transform> {
    Ok(Transform {
        anchor_point: placement.anchor,
        position: Position::xy(placement.position[0], placement.position[1]),
        scale: [placement.scale; 2],
        rotation: placement.rotation,
        opacity: PercentageProperty::new(placement.opacity)
            .ok_or_else(|| unsupported(format!("{owner} opacity must be a percentage")))?,
        ..identity_transform()
    })
}

/// An opaque FX color of a Premiere 8-bit paint.
pub(super) fn rgba(PrRgb(rgb): PrRgb) -> [f64; 4] {
    let [r, g, b] = rgb.map(|channel| f64::from(channel) / 255.0);
    [r, g, b, 1.0]
}

#[cfg(test)]
#[path = "tests/premiere_to_tesseract.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/graphic_pop.rs"]
mod graphic_pop_tests;

pub(super) fn tick_range(start: i64, end: i64) -> Result<TimeRangeProperty> {
    let start = time_from_ticks(start)?;
    let end = time_from_ticks(end)?;
    let duration = end
        .checked_sub(start)
        .ok_or_else(|| unsupported("reversed timeline range"))?;
    Ok(TimeRangeProperty::new(start, duration))
}
