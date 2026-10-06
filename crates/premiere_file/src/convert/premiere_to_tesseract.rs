//! Build canonical editable documents without runtime mutation dependencies.

#[path = "pop.rs"]
mod pop;
pub(crate) use pop::omit_emulation as omit_pop_emulation;
#[path = "stroke.rs"]
mod stroke;

use super::{
    audio::VolumeKey,
    background::{black_shape, identity_transform, plain_group},
    effects, fonts,
    graphic::{fx_path, scaled_path},
    keyframes,
    nested::{self, ItemLayer, ItemLayers, LayerScope},
    tesseract_to_premiere::LINEAR_WIPE_GUIDE_PREFIX,
    text::STROKE_WIDTH_RATIO,
    timing::{self, duration_from_ticks, time_from_ticks},
};
use crate::{
    error::{ensure, unsupported, BuildError, EditableBuildError, Result},
    format::{MediaId, PrGraphic, PrMedia, PrSequence, PrVideoItem, PrVideoOccurrence},
    linked_compositions::LinkedCompositions,
    schema::{
        text::{
            PrJustification, PrRgb, PrShapePath, PrTextFrame, PrTextTransform, PrVerticalAlign,
            EMPTY_TEXT_FONT,
        },
        MaskBoundary, PrAnimatedProperty, PrAudioOccurrence, PrEffect, PrEffectParamKeys,
        PrFadeCurve, PrKeyframeEasing, PrLinearWipe, PrMask, PrMatteChannel, PrMediaKind,
        PrNestOccurrence, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe, PrStaticCrop,
        PrStaticTransform, PrText, PrTimeRemap, PrTrackMatte, PrTransform, PrVideoStream,
        PrVideoTrack, PrVolumeKeys, TransformOwner, TICKS_PER_MILLISECOND, TRANSFORM_OPACITY,
        TRANSFORM_POSITION, TRANSFORM_ROTATION, TRANSFORM_SCALE_HEIGHT, TRANSFORM_SCALE_WIDTH,
        TRANSFORM_SHUTTER_ANGLE,
    },
    {approximate, omit, Omission, OmissionScope},
};
use fx_schema::{
    animator::{
        AnimationGraphEntry, AnimationGraphError, AnimatorData, PropertyKeyframe,
        PropertyKeyframeEasing, PropertyKeyframeTrack,
    },
    AnimationGraph, AssetId, AudioLayer, AudioSource, BlendMode, CompositionId, Dimensions,
    Duration, EditableFxCompositionDocument, FXComposition, FxItemId, GroupLayer, Justification,
    KeyframeId, Layer, LayerData, LayerId, LayerRefMap, LinearGain, MaskMode, MediaFit,
    MotionBlurSettings, NonNegativeProperty, PathMask, PercentageProperty, Position,
    PositiveProperty, PositiveRect, PropType, Property, PropertyAnimator, PropertyValue,
    RectBounds, RectLayer, RectShape, ShapeContent, ShapeLayer, ShapePath, TextDocument, TextLayer,
    Time, TimeOffset, TimeRangeProperty, TimeRemapExtrapolation, TimeRemapKeyframe,
    TimeRemapProperty, TrackMatte, TrackMatteType, Transform, VerticalAlign, VideoLayer,
    VideoSource,
};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};

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
/// Time Remapping is unverified except for the bounded physical-video Rotation
/// path in `import_video_clip`, so the other keys fail closed.
pub(super) fn retimed_keys_reason(clip: &PrVideoOccurrence) -> Option<&'static str> {
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
    let mut keys = Vec::with_capacity(wipe.completion.len().max(1));
    // A constant still needs one editable guide key: the exporter distinguishes
    // a wipe from an ordinary Crop by the guide's animated axis.
    if wipe.completion.is_empty() {
        keys.push(PropertyKeyframe::new(
            keyframe_id(guide_id, "linear-wipe", 0),
            TimeOffset::ZERO,
            PropertyValue::Float(100.0 - wipe.initial_completion),
            PropertyKeyframeEasing::Linear,
        ));
    }
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
pub(super) fn linear_wipe_guide(
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

/// Clip Volume keys on the layer clock. The other stages scale each value;
/// the easing of a Linear segment is fitted to its own Level values, because
/// Premiere's curve depends on them.
fn level_keys(keys: &PrVolumeKeys, source_in: i64, reference_level: f64) -> Result<Vec<VolumeKey>> {
    keys.keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            Ok(VolumeKey {
                millis: keyframes::layer_millis(key.source_ticks, source_in)?,
                gain: key.value * keys.gain,
                easing: match (index.checked_sub(1), key.easing) {
                    (_, PrKeyframeEasing::Hold) => PropertyKeyframeEasing::Hold,
                    (Some(previous), _) => {
                        if reference_level == 1.0 {
                            super::audio::fitted_level_easing(keys.keys[previous].value, key.value)
                        } else {
                            super::audio::fitted_level_easing_at_reference(
                                keys.keys[previous].value,
                                key.value,
                                reference_level,
                            )
                        }
                    }
                    (None, _) => PropertyKeyframeEasing::Linear,
                },
            })
        })
        .collect()
}

/// A layer's `AudioVolume` key track.
fn volume_keys(keys: &[VolumeKey], layer_id: LayerId) -> Result<PropertyKeyframeTrack> {
    let output = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            PropertyKeyframe::new(
                keyframe_id(layer_id, "volume", index),
                TimeOffset::from_millis(key.millis),
                PropertyValue::Float(key.gain),
                key.easing,
            )
        })
        .collect();
    PropertyKeyframeTrack::new(output).map_err(|error| {
        unsupported(format!(
            "Premiere volume keyframe times/values cannot be imported: {error}"
        ))
    })
}

/// The keys of a placement's fade-in and fade-out, at the level that its clip
/// Level keys `level_keys` hold over each. The reader admits a fade only where
/// that Level holds one value, with no Level key within 2 ms of it but one
/// exactly at its full-level edge. Short spans may coarsen inner keys. That
/// key moves onto the fade's edge millisecond: it rounds relative to the In
/// point and the fade like a clip boundary, which can part the two by 1 ms.
fn placement_fade_keys(
    clip: &PrAudioOccurrence,
    level_keys: &mut [VolumeKey],
    omissions: &mut Vec<Omission>,
) -> Result<[Vec<VolumeKey>; 2]> {
    let layer_millis = |ticks: i64| -> Result<i64> {
        i64::try_from(tick_range(clip.start_ticks, ticks)?.duration.as_millis())
            .map_err(|_| unsupported("audio fade exceeds the millisecond range"))
    };
    // Level keys that import in full, one for each native key.
    let native = clip
        .volume_keys
        .as_ref()
        .map(|keys| keys.keys.as_slice())
        .filter(|keys| keys.len() == level_keys.len())
        .unwrap_or_default();
    let mut output = [Vec::new(), Vec::new()];
    for (fade, fade_in, keys) in [(&clip.fade_in, true, 0), (&clip.fade_out, false, 1)] {
        let Some(fade) = fade else {
            continue;
        };
        let (start, end) = if fade_in {
            (0, layer_millis(clip.start_ticks + fade.duration_ticks)?)
        } else {
            (
                layer_millis(clip.end_ticks - fade.duration_ticks)?,
                layer_millis(clip.end_ticks)?,
            )
        };
        let edge_time = if fade_in {
            clip.start_ticks + fade.duration_ticks
        } else {
            clip.end_ticks - fade.duration_ticks
        };
        let edge_ticks = clip.source_at(edge_time)?;
        let (start, end, edge) = if clip.uses_layer_clock() {
            (start, end, if fade_in { end } else { start })
        } else {
            let timeline = if fade_in {
                clip.start_ticks..edge_time
            } else {
                edge_time..clip.end_ticks
            };
            let source = clip.source_part(&timeline)?;
            (
                keyframes::layer_millis(source.start, 0)?,
                keyframes::layer_millis(source.end, 0)?,
                keyframes::layer_millis(edge_ticks, 0)?,
            )
        };
        if let Some(index) = native.iter().position(|key| key.source_ticks == edge_ticks) {
            level_keys[index].millis = edge;
        }
        let level = level_keys
            .iter()
            .rev()
            .find(|key| key.millis <= edge)
            .or(level_keys.first())
            .map_or(clip.volume.as_f64(), |key| key.gain);
        match super::audio::fade_keys(fade.curve, fade_in, start, end, level) {
            Some(fade_keys) => {
                if let PrFadeCurve::Custom(shape) = fade.curve {
                    approximate(
                        omissions,
                        fade.id.as_deref().unwrap_or("audio transition"),
                        format!("Custom Fade {} shape {} approximated by editable Volume keys of the squared-sine power model; four incoming and six half-gain outgoing controls measured, other parameters interpolated; no native Custom Fade replay on export", if fade_in { "incoming" } else { "outgoing" }, shape.value()),
                    );
                }
                output[keys] = fade_keys;
            }
            None => omit(
                omissions,
                OmissionScope::Feature,
                fade.id.as_deref().unwrap_or("audio transition"),
                "audio fade has no positive millisecond span; fade not converted",
            ),
        }
    }
    Ok(output)
}

/// The `AudioVolume` key track of `clip`, if it has keys: its clip Level keys
/// and the keys of its fades. A fade shares its full-level key with a Level
/// key there, or with the other fade where they touch; the key keeps the
/// easing that arrives at it, the fade-in's, else the Level's (stable order).
pub(super) fn placement_volume_track(
    clip: &PrAudioOccurrence,
    layer_id: LayerId,
    intrinsic_ticks: i64,
    omissions: &mut Vec<Omission>,
) -> Result<Option<PropertyKeyframeTrack>> {
    if clip.playback_rate != 1.0 && clip.has_volume_animation() {
        approximate(
            omissions,
            clip.record(),
            super::audio::RETIMED_GAIN_CLOCK_WARNING,
        );
    }
    // Custom automation exports as ordinary Level records. An explicit fade,
    // or silence at the source edge, identifies its precision band on reimport
    // without persisting origin metadata. Other Volume tracks retain unity.
    let reference_level = clip
        .volume_keys
        .as_ref()
        .filter(|native| {
            clip.fade_in.is_some()
                || clip.fade_out.is_some()
                || native
                    .keys
                    .first()
                    .is_some_and(|key| key.source_ticks == clip.in_ticks && key.value == 0.0)
                || native
                    .keys
                    .last()
                    .is_some_and(|key| key.source_ticks == clip.out_ticks && key.value == 0.0)
        })
        .map(|native| {
            (native
                .keys
                .iter()
                .map(|key| key.value)
                .fold(0.0_f64, f64::max)
                * native.gain)
                .min(1.0)
        });
    let mut level = match &clip.volume_keys {
        // Checked alone, so that sharing fade keys cannot hide Level keys
        // that the track rejects.
        Some(native) => level_keys(
            native,
            if clip.uses_layer_clock() {
                clip.in_ticks
            } else {
                0
            },
            reference_level
                .filter(|_| native.gain > 0.0)
                .map_or(1.0, |level| level / native.gain),
        )
        .and_then(|level| volume_keys(&level, layer_id).map(|_| level))?,
        None => Vec::new(),
    };
    let [fade_in, fade_out] = placement_fade_keys(clip, &mut level, omissions)?;
    if let Some(native) = &clip.volume_keys {
        let (refined, limited) =
            super::audio::refine_level_keys(level, native.gain, reference_level)?;
        level = refined;
        if limited {
            approximate(
                omissions,
                format!("AudioVolume layer {layer_id}"),
                "Volume curve fit exceeds 0.01 dB at the 1 ms key timing limit",
            );
        }
    }
    let mut keys = [fade_in, level, fade_out].concat();
    keys.sort_by_key(|key| key.millis);
    keys.dedup_by(|later, earlier| later.millis == earlier.millis && later.gain == earlier.gain);
    if clip.playback_rate < 0.0 {
        keys = super::audio::reverse_volume_keys(
            keys,
            keyframes::layer_millis(intrinsic_ticks, 0)?,
            clip.record(),
            omissions,
        )?;
    }
    (!keys.is_empty())
        .then(|| volume_keys(&keys, layer_id))
        .transpose()
}

/// Keep the bracketing keys of the clipped active window, including outside
/// support keys needed to preserve an in-flight eased fade without refitting it.
fn rebase_audio_track(
    track: PropertyKeyframeTrack,
    original: TimeRangeProperty,
    active: TimeRangeProperty,
) -> Result<PropertyKeyframeTrack> {
    let shift = i64::try_from(active.start.as_millis() - original.start.as_millis())
        .map_err(|_| unsupported("audio key shift exceeds signed milliseconds"))?;
    let end = shift
        .checked_add(
            i64::try_from(active.duration.as_millis())
                .map_err(|_| unsupported("audio key duration exceeds signed milliseconds"))?,
        )
        .ok_or_else(|| unsupported("audio key window overflows"))?;
    let keys = track.keyframes();
    let first = keys
        .iter()
        .rposition(|key| key.layer_time().as_millis() <= shift)
        .unwrap_or(0);
    let last = keys
        .iter()
        .position(|key| key.layer_time().as_millis() >= end)
        .unwrap_or(keys.len() - 1);
    let keys = keys[first..=last]
        .iter()
        .map(|key| {
            let time = key
                .layer_time()
                .as_millis()
                .checked_sub(shift)
                .ok_or_else(|| unsupported("audio key rebasing overflows"))?;
            Ok(PropertyKeyframe::new(
                key.id().clone(),
                TimeOffset::from_millis(time),
                key.value().clone(),
                key.easing(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    PropertyKeyframeTrack::new(keys)
        .map_err(|error| unsupported(format!("rebased audio keys: {error}")))
}

/// `clip`'s `remap` as an editable curve on the parent clock on which the clip
/// starts at `start_ticks`, with the playback input offset that it needs. The
/// clip reaches a key `timeline_ticks / playback_rate` ticks after its start;
/// that parent time stays an exact fraction until it rounds once to the
/// millisecond, ties away from zero, and a key that rounds before zero fails.
/// When keys of a curve from another In or speed
/// ([`PrVideoOccurrence::remaps_from_in_or_speed`]) precede the parent clock's
/// zero, as keys before a trimmed In can, the curve and its input first move
/// later together by the fewest whole milliseconds that keep every key at or
/// after zero; another remap keeps a zero offset.
fn time_remap_property(
    remap: &PrTimeRemap,
    start_ticks: i64,
    playback_rate: f64,
    from_in_or_speed: bool,
) -> Result<(TimeRemapProperty, i64)> {
    let out_of_range = || unsupported("TimeRemapping parent time exceeds Premiere's tick range");
    // `ticks / playback_rate` is `ticks * per_tick / denominator`, exactly.
    let reciprocal = timing::reciprocal(playback_rate);
    let (per_tick, denominator) = reciprocal.ok_or_else(out_of_range)?;
    let millisecond = denominator
        .checked_mul(i128::from(TICKS_PER_MILLISECOND))
        .ok_or_else(out_of_range)?;
    // Each key's parent time in ticks, times `denominator`.
    let parent_times = remap
        .keys
        .iter()
        .map(|key| {
            i128::from(start_ticks)
                .checked_mul(denominator)?
                .checked_add(i128::from(key.timeline_ticks).checked_mul(per_tick)?)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(out_of_range)?;
    let deficit = match parent_times.iter().min() {
        Some(&earliest) if earliest < 0 && from_in_or_speed => {
            earliest.checked_neg().ok_or_else(out_of_range)?
        }
        _ => 0,
    };
    let input_offset_ms = deficit / millisecond + i128::from(deficit % millisecond != 0);
    let keys = remap
        .keys
        .iter()
        .zip(parent_times)
        .enumerate()
        .map(|(index, (key, parent_time))| {
            let rounded = input_offset_ms
                .checked_mul(millisecond)
                .and_then(|shift| parent_time.checked_add(shift))
                .and_then(|shifted| timing::nearest(shifted, millisecond))
                .ok_or_else(out_of_range)?;
            let parent_millis = u64::try_from(rounded)
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
    let property = TimeRemapProperty::new(
        keys,
        TimeRemapExtrapolation::Continue,
        TimeRemapExtrapolation::Continue,
    )
    .map_err(|error| unsupported(format!("TimeRemapping cannot be imported: {error}")))?;
    let input_offset_ms = i64::try_from(input_offset_ms)
        .map_err(|_| unsupported("TimeRemapping input offset exceeds the clock range"))?;
    Ok((property, input_offset_ms))
}

/// [`time_remap_property`] for `clip`, which plays `remap` over `window` on a
/// parent clock on which it starts at `start_ticks`. A curve from another In
/// or speed ([`PrVideoOccurrence::remaps_from_in_or_speed`]) must also cover
/// the millisecond window that the clip plays, moved by its input offset:
/// from the first key to the last, or to the key before a last key at the end
/// of `source`, whose segment is unmeasured. Within the constant-speed
/// tolerance the saved In and Out can leave input uncovered, which a slow
/// speed stretches into whole milliseconds of playback.
fn played_time_remap(
    clip: &PrVideoOccurrence,
    remap: &PrTimeRemap,
    source: &PrVideoStream,
    window: TimeRangeProperty,
    start_ticks: i64,
) -> Result<(TimeRemapProperty, i64)> {
    mapped_time_remap(
        remap,
        clip.playback_rate,
        clip.remaps_from_in_or_speed(),
        source.intrinsic_ticks,
        window,
        start_ticks,
    )
}

/// Shared input/source clock mapping for physical video and nested composites.
/// Round signed input/rate + placement once, retain every authored key and ease,
/// and recheck the played window after integer-millisecond quantization.
pub(super) fn mapped_time_remap(
    remap: &PrTimeRemap,
    playback_rate: f64,
    from_in_or_speed: bool,
    intrinsic_ticks: i64,
    window: TimeRangeProperty,
    start_ticks: i64,
) -> Result<(TimeRemapProperty, i64)> {
    let (property, input_offset_ms) =
        time_remap_property(remap, start_ticks, playback_rate, from_in_or_speed)?;
    let extends_past_media = remap
        .keys
        .last()
        .is_some_and(|key| key.source_ticks > intrinsic_ticks);
    if extends_past_media {
        // Native admission bounds the saved Out. Recheck the actual emitted
        // window and rounded keys against the exact media end: independent
        // millisecond rounding must not turn an unused tail into played media.
        let keys = property.keyframes();
        let millis = |time: Time| i128::from(time.as_millis());
        let ticks = |time: Time| millis(time) * i128::from(TICKS_PER_MILLISECOND);
        let offset = i128::from(input_offset_ms);
        let start = millis(window.start) + offset;
        let end = millis(window.end()) + offset;
        let bounded = keys.len() >= 2 && {
            let first = &keys[0];
            let previous = &keys[keys.len() - 2];
            let last = &keys[keys.len() - 1];
            millis(first.time) <= start
                && last.easing == PropertyKeyframeEasing::Linear
                && keys[..keys.len() - 1]
                    .iter()
                    .all(|key| ticks(key.value) <= i128::from(intrinsic_ticks))
                && crate::schema::linear_tail_within_media(
                    millis(previous.time)..millis(last.time),
                    ticks(previous.value)..ticks(last.value),
                    end,
                    i128::from(intrinsic_ticks),
                )
        };
        ensure!(
            bounded,
            "TimeRemapping rounded playback window reaches outside the media bounds"
        );
    }
    if !from_in_or_speed {
        return Ok((property, input_offset_ms));
    }
    let ends_at_media_end = remap
        .keys
        .last()
        .is_some_and(|key| key.source_ticks == intrinsic_ticks);
    let keys = property.keyframes();
    let tail = 1 + usize::from(ends_at_media_end);
    let covering = keys.len().saturating_sub(tail);
    let (Some(first), Some(last)) = (keys.first(), keys.get(covering)) else {
        return Err(unsupported("TimeRemapping has no keys"));
    };
    let millis = |time: Time| i128::from(time.as_millis());
    let offset = i128::from(input_offset_ms);
    let start = millis(window.start) + offset;
    let end = millis(window.end()) + offset;
    let (first, last) = (millis(first.time), millis(last.time));
    if first > start || end > last {
        return Err(unsupported(format!(
            "TimeRemapping from a source In or at another speed plays input {start} to {end} ms, outside its covering keys at {first} to {last} ms"
        )));
    }
    Ok((property, input_offset_ms))
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
/// `start` and which reads them `input_offset_ms` later, with its key times on
/// the clip's own clock, which starts at zero and has no offset. A key before
/// the clip fails, also one that only the offset kept at or after zero.
fn on_clip_clock(
    playback: &TimeRemapProperty,
    start: Time,
    input_offset_ms: i64,
) -> Result<TimeRemapProperty> {
    let origin = i128::from(start.as_millis()) + i128::from(input_offset_ms);
    let keyframes = playback
        .keyframes()
        .iter()
        .map(|key| {
            let time = u64::try_from(i128::from(key.time.as_millis()) - origin)
                .map_err(|_| unsupported("a playback key precedes its clip"))?;
            Ok(TimeRemapKeyframe {
                time: Time::from_millis(time),
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

pub(super) fn constant_time_remap(
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
/// `dimensions`, as Motion Position keys convert. Spatial handles are retained
/// only on curved adjacent segments, with one 2D decision shared by both axes.
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
        // Native straight paths follow temporal easing alone; FX would traverse
        // retained handles parametrically. Decide each side in 2D so both axes
        // agree, without dropping handles of a neighboring curved segment.
        let curved_in = index
            .checked_sub(1)
            .is_some_and(|previous| crate::schema::spatial::is_curved(&native_keys[previous], key));
        let curved_out = native_keys
            .get(index + 1)
            .is_some_and(|next| crate::schema::spatial::is_curved(key, next));
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
                    curved_in
                        .then_some(key.spatial_in_tangent)
                        .flatten()
                        .map(|tangent| tangent[axis] * scale),
                    curved_out
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
/// points are the effect's, in source pixels; the scale is
/// Scale Height on both axes under Uniform Scale (T3); the rotation is the
/// effect's, clockwise (T4); the skew is the effect's and the skew axis
/// Skew Axis − 90° under a skew, else 0 ([`PrTransform::fx_skew_axis`],
/// the convention that the export gate measured); the opacity is the
/// effect's, which FX blends in sRGB. The
/// group's Motion and clip Opacity apply after it (T10), so the two
/// opacities multiply.
pub(super) fn staged_video_transform(
    transform: &PrTransform,
    source: [u32; 2],
) -> Result<Transform> {
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
    pub(super) fn request(
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

const CURVED_TRANSFORM_POSITION_APPROXIMATION: &str = "Transform Position curved spatial path retains editable tangents but FX traverses parametrically rather than native constant-speed distance";

/// Report the timing difference only for a curved Position that survived the
/// staged Transform mapping. Callers commit this warning with the stage.
pub(super) fn curved_transform_position_approximation(effect: &PrEffect) -> Option<&'static str> {
    effect
        .animations
        .iter()
        .any(|animation| {
            animation.param.id == TRANSFORM_POSITION.id
                && animation
                    .keys
                    .point()
                    .is_some_and(|keys| crate::schema::spatial::curved_segment(keys).is_some())
        })
        .then_some(CURVED_TRANSFORM_POSITION_APPROXIMATION)
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
/// rejects every other keyed parameter except adjustment Geometry2's Anchor,
/// which maps to paired `AnchorPointX`/`AnchorPointY` tracks.
pub(super) fn transform_stage_tracks(
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
            (PrEffectParamKeys::Point(keys), id)
                if id == crate::schema::TRANSFORM_ANCHOR_POINT.id
                    && matches!(
                        effect.params,
                        crate::schema::PrEffectParams::AdjustmentGeometry2(_)
                    ) =>
            {
                let axes =
                    point_property_tracks(keys, source_in, layer_id, &ANCHOR_POINT_TRACKS, source)?;
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
    linked: &mut LinkedCompositions<'_>,
    omissions: &mut Vec<Omission>,
) -> Result<EditableFxCompositionDocument> {
    sequence_document_with_progress(
        project,
        media,
        asset_ids,
        &crate::media::PictureClocks::new(),
        linked,
        omissions,
        fx_conv::Progress::default(),
    )
}

pub(crate) fn sequence_document_with_progress(
    project: &PrSequence,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    picture_clocks: &crate::media::PictureClocks,
    linked: &mut LinkedCompositions<'_>,
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
        return Err(EditableBuildError::invalid_input(
            "dimensions",
            "width and height must be non-zero",
        )
        .into());
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
            project.dimensions(),
            &mut next_index,
            &mut effect_ids,
            &mut composition_shutter,
            picture_clocks,
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
        &mut effect_ids,
        linked,
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
/// ids of `next_index` and their Volume and fade keys in `dynamics`. A key
/// track that cannot import is reported, and its sound keeps its placement at
/// zero gain without its fades: the value before the keys would play through
/// their silent intervals.
#[allow(clippy::too_many_arguments)]
pub(super) fn audio_layers(
    sounds: &[PrAudioOccurrence],
    parent: Option<LayerId>,
    next_index: &mut usize,
    effect_ids: &mut effects::EffectIdAllocator,
    linked: &mut LinkedCompositions<'_>,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
    progress: Option<(fx_conv::ProgressPhase<'_>, &Cell<usize>)>,
) -> Result<Vec<Layer>> {
    let mut layers = Vec::with_capacity(sounds.len());
    for (index, clip) in sounds.iter().enumerate() {
        ensure!(
            clip.source_channel.is_none(),
            "selected audio channels must be extracted before editable conversion"
        );
        let _processed = Processed { progress };
        if media
            .get(&clip.media)
            .is_some_and(|media| media.after_effects_composition().is_some())
        {
            if let Some(layer) = super::linked_audio::layer(
                clip,
                parent,
                next_index,
                effect_ids,
                linked,
                dynamics,
                media
                    .get(&clip.media)
                    .and_then(|media| media.audio.as_ref())
                    .ok_or_else(|| unsupported("linked audio has no source clock"))?
                    .intrinsic_ticks,
                omissions,
            )? {
                layers.push(layer);
            }
            continue;
        }
        let source = media
            .get(&clip.media)
            .and_then(|source| source.audio.as_ref())
            .ok_or_else(|| unsupported("audio occurrence references unknown media"))?;
        let asset_id = asset_ids
            .get(&clip.media)
            .ok_or_else(|| unsupported("audio occurrence has no asset ID"))?;
        // Sound follows the same boundary rounding as picture.
        let original_range = tick_range(clip.start_ticks, clip.end_ticks)?;
        let (active_range, source_range) = if let Some(clock) = &source.prepared_clock {
            let window = match clock.window(clip) {
                Ok(window) => window,
                Err(error) => {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        clip.record(),
                        format!("prepared audio clock was not imported: {error}"),
                    );
                    continue;
                }
            };
            let Some((timeline, raw)) = window else {
                approximate(omissions, clip.record(), "delayed audio interval is entirely outside the playable edit; retained as silence");
                continue;
            };
            let timeline = tick_range(timeline.start, timeline.end)?;
            let raw = tick_range(raw.start, raw.end)?;
            // Unit sound retains its established shorter-span rounding. Retimed
            // sound keeps both independent windows for its explicit source map.
            let duration = timeline.duration.min(raw.duration);
            if duration.as_millis() == 0 {
                approximate(
                    omissions,
                    clip.record(),
                    "delayed playable sound collapses on the editable millisecond grid",
                );
                continue;
            }
            approximate(omissions, clip.record(), "delayed sound intersected with its playable presentation edit and mapped to raw source; absolute boundaries rounded to milliseconds, unit span clipped to the shorter rounded window; volume/fade keys rebased with boundary-support keys retained");
            (
                if clip.playback_rate == 1.0 {
                    TimeRangeProperty::new(timeline.start, duration)
                } else {
                    timeline
                },
                if clip.playback_rate == 1.0 {
                    TimeRangeProperty::new(raw.start, duration)
                } else {
                    raw
                },
            )
        } else {
            (
                original_range,
                if clip.playback_rate == 1.0 {
                    TimeRangeProperty::new(time_from_ticks(clip.in_ticks)?, original_range.duration)
                } else {
                    let (source_in, source_out) = if clip.playback_rate < 0.0 {
                        (
                            source.intrinsic_ticks - clip.out_ticks,
                            source.intrinsic_ticks - clip.in_ticks,
                        )
                    } else {
                        (clip.in_ticks, clip.out_ticks)
                    };
                    if source_in < 0 {
                        approximate(
                            omissions,
                            clip.record(),
                            super::audio::REVERSE_SOURCE_START_CLIPPING_WARNING,
                        );
                    }
                    tick_range(source_in.max(0), source_out)?
                },
            )
        };
        if active_range.duration.as_millis() == 0 || source_range.duration.as_millis() == 0 {
            omit(
                omissions,
                OmissionScope::Occurrence,
                clip.record(),
                "audio placement/source window collapses on the editable millisecond grid",
            );
            continue;
        }
        let layer_id = LayerId::new(*next_index as u64 + 1);
        *next_index += 1;
        let mut volume = clip.volume;
        let native_intrinsic_ticks = source
            .prepared_clock
            .as_ref()
            .map_or(source.intrinsic_ticks, |clock| {
                clock.presentation_duration_ticks()
            });
        let volume_track =
            placement_volume_track(clip, layer_id, native_intrinsic_ticks, omissions).and_then(
                |track| {
                    if let Some(clock) = &source.prepared_clock {
                        track
                            .map(|track| {
                                if clip.uses_layer_clock() {
                                    rebase_audio_track(track, original_range, active_range)
                                } else {
                                    let shift =
                                        keyframes::layer_millis(clock.source_offset_ticks(), 0)?;
                                    let keys = track
                                        .keyframes()
                                        .iter()
                                        .map(|key| {
                                            Ok(PropertyKeyframe::new(
                                                key.id().clone(),
                                                TimeOffset::from_millis(
                                                    key.layer_time()
                                                        .as_millis()
                                                        .checked_sub(shift)
                                                        .ok_or_else(|| {
                                                            unsupported(
                                                        "prepared audio key clock overflows",
                                                    )
                                                        })?,
                                                ),
                                                key.value().clone(),
                                                key.easing(),
                                            ))
                                        })
                                        .collect::<Result<Vec<_>>>()?;
                                    PropertyKeyframeTrack::new(keys)
                                        .map_err(|error| unsupported(error.to_string()))
                                }
                            })
                            .transpose()
                    } else {
                        Ok(track)
                    }
                },
            );
        match volume_track {
            Ok(Some(track)) => dynamics
                .set_property(
                    Property::new(layer_id, PropType::AudioVolume),
                    PropertyAnimator::keyframes(track),
                    Vec::new(),
                )
                .map_err(map_animation_graph_error)?,
            Ok(None) => {}
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
        if clip.playback_rate != 1.0 {
            approximate(omissions, clip.record(), format!(
                "constant audio clock uses independently rounded millisecond endpoints; saved signed rate {} becomes {} over the editable window",
                clip.playback_rate,
                source_range.duration.as_millis() as f64 / active_range.duration.as_millis() as f64 * clip.playback_rate.signum(),
            ));
        }
        let effective_rate = source_range.duration.as_millis() as f64
            / active_range.duration.as_millis() as f64
            * clip.playback_rate.signum();
        if clip.preserve_audio_pitch
            && !crate::schema::RUNTIME_PITCH_RATES.contains(&effective_rate)
        {
            approximate(omissions, clip.record(), "saved pitch-ON remains editable, but the existing audio runtime preserves pitch only on forward 0.25x–4x source maps; this clock uses ordinary mapped resampling");
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
            playback: super::audio::playback(clip, active_range, source_range, layer_id)?,
            preserve_audio_pitch: clip.preserve_audio_pitch,
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
    mut scope: LayerScope<'_, '_>,
    media: &BTreeMap<MediaId, PrMedia>,
    asset_ids: &BTreeMap<MediaId, fx_schema::AssetId>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
    progress: Option<(fx_conv::ProgressPhase<'_>, &Cell<usize>)>,
) -> Result<Vec<Layer>> {
    let mut layers = Vec::with_capacity(project.video_items().count() * 2);
    let mut adjustment_wipes = Vec::new();
    let mut offset = scope.first_index + project.video_items().count();
    // How many placements key each matte, by the matte's track and start: a
    // stage group takes only a matte of its own.
    let mut matte_consumers: BTreeMap<(usize, i64), usize> = BTreeMap::new();
    for (index, track) in project.video_tracks.iter().enumerate() {
        let keyed = track
            .items
            .iter()
            .filter_map(PrVideoItem::media)
            .filter_map(|clip| Some((clip.track_matte?, clip.timeline_ticks())))
            .chain(
                track
                    .nests
                    .iter()
                    .filter_map(|nest| Some((nest.track_matte?, nest.timeline_ticks()))),
            );
        for (matte, range) in keyed {
            if let Ok(provider) =
                crate::schema::track_matte_provider(&project.video_tracks, index, range, matte)
            {
                *matte_consumers
                    .entry((matte.track_index, provider.range.start))
                    .or_default() += 1;
            }
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
                PrVideoItem::Capsule(capsule) => {
                    let Some((root, guide)) = super::graphic::import_capsule(
                        capsule,
                        project.dimensions(),
                        layer_id,
                        index,
                        &mut scope,
                        dynamics,
                        omissions,
                    )?
                    else {
                        continue;
                    };
                    scope
                        .item_layers
                        .insert(item_key, ItemLayer::Plain(root.id()));
                    layers.push(root);
                    layers.extend(guide);
                    continue;
                }
                PrVideoItem::Graphic(graphic) => {
                    let imported = if graphic.clip_motion == PrStaticTransform::default() {
                        super::graphic::import_graphic(
                            graphic,
                            project.dimensions(),
                            layer_id,
                            index,
                            &mut scope,
                            dynamics,
                            omissions,
                        )?
                    } else {
                        // A Source Graphic placement that its own clip Motion
                        // moves: a group of its content is its root.
                        super::graphic::import_moved_graphic(
                            graphic,
                            project.dimensions(),
                            layer_id,
                            index,
                            &mut scope,
                            dynamics,
                            omissions,
                        )?
                        .map(|root| (root, None))
                    };
                    let Some((root, guide)) = imported else {
                        continue;
                    };
                    // A matte names the whole root: the group of several
                    // objects, not the first object, which takes `layer_id`.
                    scope
                        .item_layers
                        .insert(item_key, ItemLayer::Plain(root.id()));
                    layers.push(root);
                    layers.extend(guide);
                    continue;
                }
            };
            let record = clip.record();
            let matte = match clip.track_matte {
                None => None,
                Some(matte) => match matte_layer(
                    &scope.item_layers,
                    &project.video_tracks,
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
                },
            };
            // Generator media has no asset: a matte becomes an editable
            // rectangle, an adjustment an FX adjustment layer.
            match media
                .get(&clip.media)
                .and_then(|source| source.video.as_ref())
                .map(|video| video.kind)
            {
                Some(PrMediaKind::Adjustment) => {
                    if clip.linear_wipe.is_some()
                        || clip.effects.iter().any(|effect| {
                            matches!(
                                effect.params,
                                crate::schema::PrEffectParams::AdjustmentGeometry2(_)
                            )
                        })
                    {
                        adjustment_wipes.push((layer_id, clip));
                    }
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
                Some(PrMediaKind::ColorMatte(color)) => {
                    let canvas = [project.width, project.height];
                    let carries_key = clip
                        .effects
                        .iter()
                        .chain(clip.source_effects.iter().flat_map(|s| &s.effects))
                        .any(crate::schema::PrEffect::requires_coverage);
                    let (mut mapped, mut effect_tracks) = if carries_key {
                        approximate(omissions, clip.record(), "Color Matte retains Legacy Luma and its editable sibling stack on the generated rectangle; native generator/effect order is approximate");
                        effects::import_source_effects(
                            clip,
                            layer_id,
                            false,
                            MaskBoundary::Flat,
                            canvas,
                            canvas,
                            canvas,
                            scope.effect_ids,
                            omissions,
                        )
                    } else {
                        effects::omit_stroke(clip, "Color Matte", omissions);
                        (Vec::new(), Vec::new())
                    };
                    let (own, tracks) = effects::import_effects(
                        clip,
                        layer_id,
                        false,
                        MaskBoundary::Flat,
                        scope.parent.is_some(),
                        false,
                        PrMediaKind::ColorMatte(color),
                        canvas,
                        canvas,
                        canvas,
                        scope.effect_ids,
                        omissions,
                    );
                    mapped.extend(own);
                    effect_tracks.extend(tracks);
                    if !effects::retains_coverage(clip, &mapped, omissions) {
                        continue;
                    }
                    effects::report_source_effects(clip, carries_key, omissions);
                    let mut rect = super::color_matte::rect_layer(
                        project,
                        color,
                        tick_range(clip.start_ticks, clip.end_ticks)?,
                        LayerId::new(index as u64 + 1),
                        format!("Premiere color matte {}", index + 1),
                        !clip.enabled,
                    );
                    rect.parent = scope.parent;
                    rect.blend_mode = clip.blend_mode.fx_mode();
                    rect.track_matte = matte;
                    rect.effects = mapped;
                    if clip.transform != PrStaticTransform::default()
                        || clip
                            .animations
                            .iter()
                            .any(|animation| animation.property() != PrAnimatedProperty::Opacity)
                    {
                        rect.transform =
                            clip_transform(&clip.transform, clip.opacity, canvas, canvas)?;
                    }
                    rect.transform.opacity = PercentageProperty::new(clip.opacity)
                        .ok_or_else(|| unsupported("Premiere opacity must be between 0 and 100"))?;
                    let mut outline_keys = None;
                    let mut mask_tracks = Vec::new();
                    let opacity_guide = if let Some(mask) = &clip.opacity_mask {
                        let guide_id = LayerId::new(*scope.next_index as u64 + 1);
                        let mask_id = FxItemId::new(*scope.next_index as u64 + 2);
                        let prepare = || -> Result<_> {
                            ensure!(
                                clip.crop.is_default()
                                    && clip.track_matte.is_none()
                                    && clip.linear_wipe.is_none()
                                    && clip.effects.is_empty()
                                    && clip
                                        .source_effects
                                        .as_ref()
                                        .is_none_or(|source| source.effects.is_empty()
                                            && source.active_transforms == 0),
                                "Color Matte Opacity mask has no mixed coverage/effect stage"
                            );
                            if !mask.path_keys.is_empty() || mask.has_numeric_keys() {
                                ensure!(mask_path_keys_reason(clip).is_none(),
                                    "Color Matte Opacity mask keys require the existing unit-forward source clock");
                            }
                            let mut reports = Vec::new();
                            let (path_mask, path) = opacity_mask(
                                mask,
                                mask_id,
                                guide_id,
                                canvas,
                                clip.record(),
                                &mut reports,
                            )?;
                            let keys = if mask.path_keys.is_empty() {
                                None
                            } else {
                                Some((
                                    Property::new(guide_id, PropType::ShapePath),
                                    mask_path_track(mask, guide_id, clip.in_ticks, canvas)
                                        .map_err(unsupported)?,
                                ))
                            };
                            let tracks =
                                super::mask_animation::import_tracks(mask, mask_id, clip.in_ticks)
                                    .map_err(unsupported)?;
                            let mut transform = rect.transform;
                            transform.opacity = identity_transform().opacity;
                            let guide = shape_guide(
                                guide_id,
                                format!("Premiere Opacity mask {}", index + 1),
                                rect.parent,
                                rect.active_range,
                                transform,
                                path,
                            );
                            Ok((path_mask, guide, keys, tracks, reports))
                        };
                        match prepare() {
                            Ok((path_mask, guide, keys, tracks, reports)) => {
                                rect.masks.push(path_mask);
                                outline_keys = keys;
                                mask_tracks = tracks;
                                omissions.extend(reports);
                                *scope.next_index += 2;
                                Some(guide)
                            }
                            Err(error) => {
                                omit(omissions, OmissionScope::Occurrence, clip.record(),
                                    format!("Color Matte Opacity mask not converted; masked occurrence omitted: {error}"));
                                continue;
                            }
                        }
                    } else {
                        None
                    };
                    let mut guide = if clip.crop.is_default() {
                        None
                    } else {
                        let guide_id = LayerId::new(*scope.next_index as u64 + 1);
                        let mask_id = FxItemId::new(*scope.next_index as u64 + 2);
                        *scope.next_index += 2;
                        Some(super::color_matte::bind_sharp_crop(
                            &mut rect, &clip.crop, canvas, guide_id, mask_id,
                        )?)
                    };
                    if let Some(guide) = &mut guide {
                        guide.transform = rect.transform;
                        guide.transform.opacity = identity_transform().opacity;
                    }
                    let mut key_omissions = Vec::new();
                    let tracks = motion_tracks(
                        &clip.animations,
                        clip.in_ticks,
                        retimed_keys_reason(clip),
                        layer_id,
                        guide
                            .as_ref()
                            .map(|guide| guide.id)
                            .or_else(|| opacity_guide.as_ref().map(|guide| guide.id)),
                        canvas,
                        canvas,
                        clip.record(),
                        &mut key_omissions,
                    );
                    if !key_omissions.is_empty()
                        && (matte_consumers.contains_key(&item_key) || clip.opacity_mask.is_some())
                    {
                        for omission in key_omissions {
                            omit(omissions, OmissionScope::Occurrence, clip.record(),
                                format!("Color Matte coverage keys cannot be preserved; coverage and dependent consumers omitted: {omission}"));
                        }
                        continue;
                    }
                    omissions.extend(key_omissions);
                    set_tracks(dynamics, tracks)?;
                    set_tracks(dynamics, outline_keys.into_iter().collect())?;
                    for (target, track) in mask_tracks.into_iter().chain(effect_tracks) {
                        dynamics
                            .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
                            .map_err(map_animation_graph_error)?;
                    }
                    validate_time_range("active_range", rect.active_range)?;
                    layers.push(Layer::from_data(&fx_schema::LayerData::Rect(rect))?);
                    if let Some(guide) = guide {
                        layers.push(Layer::from_data(&fx_schema::LayerData::Rect(guide))?);
                    }
                    if let Some(guide) = opacity_guide {
                        layers.push(Layer::from_data(&fx_schema::LayerData::Shape(guide))?);
                    }
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
                    | PrMediaKind::Still { .. }
                    | PrMediaKind::NumberedStills { .. }
                    | PrMediaKind::OpenExr { .. },
                )
                | None => {}
            }
            let source = media
                .get(&clip.media)
                .and_then(|source| source.video.as_ref())
                .ok_or_else(|| unsupported("video occurrence references unknown media"))?;
            if !source.pixel_aspect.is_square() {
                approximate(omissions, clip.record(),
                    "source pixel aspect is normalized into editable scale on decoded pixels; source-space spatial effects may differ, and export uses square-pixel interpretation rather than restoring the original override");
            }
            if source.kind.is_numbered_stills() {
                if let Some(reason) = crate::numbered_images::unsupported_occurrence(clip) {
                    omit(omissions, OmissionScope::Occurrence, clip.record(), reason);
                    continue;
                }
                let asset_id = asset_ids
                    .get(&clip.media)
                    .ok_or_else(|| unsupported("numbered-image occurrence has no asset ID"))?;
                if scope.parent.is_some() {
                    omit(omissions, OmissionScope::Occurrence, clip.record(),
                        "numbered-image sequence sampling under a containing clock is unproved; occurrence omitted");
                    continue;
                }
                let sampled = match crate::numbered_images::sampling::occurrence(
                    clip,
                    source,
                    project.frame_rate,
                ) {
                    Ok(sampled) => sampled,
                    Err(error) => {
                        omit(
                            omissions,
                            OmissionScope::Occurrence,
                            clip.record(),
                            error.to_string(),
                        );
                        continue;
                    }
                };
                let active_range = sampled.window;
                let frames = super::timed_images::import_frames(
                    source,
                    asset_id,
                    layer_id,
                    sampled.frames,
                    scope.next_index,
                )?;
                let canvas = [project.width, project.height];
                let mut group = plain_group(
                    layer_id,
                    format!("Premiere numbered images {}", index + 1),
                    active_range,
                    source_aspect_transform(
                        clip_transform(
                            &clip.transform,
                            clip.opacity,
                            [source.width, source.height],
                            canvas,
                        )?,
                        source,
                    ),
                    frames,
                )?;
                group.parent = scope.parent;
                group.is_hidden = !clip.enabled;
                group.blend_mode = clip.blend_mode.fx_mode();
                let mut key_omissions = Vec::new();
                let mut tracks = motion_tracks(
                    &clip.animations,
                    clip.in_ticks,
                    None,
                    layer_id,
                    None,
                    [source.width, source.height],
                    canvas,
                    clip.record(),
                    &mut key_omissions,
                );
                if !key_omissions.is_empty() {
                    for omission in key_omissions {
                        omit(omissions, OmissionScope::Occurrence, clip.record(),
                            format!("numbered-image key clock cannot be preserved; occurrence omitted: {omission}"));
                    }
                    continue;
                }
                source_aspect_tracks(&mut tracks, source, &[Some(layer_id)])?;
                set_tracks(dynamics, tracks)?;
                layers.push(Layer::from_data(&LayerData::Group(group))?);
                scope
                    .item_layers
                    .insert(item_key, ItemLayer::Plain(layer_id));
                if let Some(warning) = clip.blend_mode.approximation() {
                    approximate(omissions, clip.record(), warning);
                }
                continue;
            }
            if source.kind.is_still() {
                if clip
                    .opacity_mask
                    .as_ref()
                    .is_some_and(|mask| !mask.path_keys.is_empty())
                {
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        clip.record(),
                        "Mask Path keys on a still are not converted; only a video clip's Opacity mask converts keyed",
                    );
                    continue;
                }
                let unsupported_mask = clip.opacity_mask.as_ref().and_then(|mask| {
                    if mask.has_numeric_keys() {
                        Some("numeric Opacity mask keys on a still are not converted; this host has no admitted numeric mask clock")
                    } else if mask.expansion != 0.0 {
                        Some("Mask Expansion on a still is not converted")
                    } else {
                        None
                    }
                });
                if let Some(reason) = unsupported_mask {
                    omit(omissions, OmissionScope::Occurrence, clip.record(), reason);
                    continue;
                }
                let asset_id = asset_ids
                    .get(&clip.media)
                    .ok_or_else(|| unsupported("still occurrence has no asset ID"))?;
                let record = clip.record();
                let retains_source_key = clip.source_effects.as_ref().is_some_and(|s| {
                    s.effects
                        .iter()
                        .any(crate::schema::PrEffect::requires_coverage)
                });
                effects::report_source_effects(clip, retains_source_key, omissions);
                let canvas = [project.width, project.height];
                let frame = [source.width, source.height];
                let (mut effects, mut effect_tracks) = effects::import_still_effects(
                    clip,
                    layer_id,
                    matte_consumers.contains_key(&(track_index, clip.start_ticks)),
                    source.kind,
                    frame,
                    canvas,
                    scope.effect_ids,
                    omissions,
                );
                if retains_source_key {
                    approximate(omissions, record, "Legacy Luma source stack retained on the still before its own effects but after its mask; native source/mask order is approximate");
                    let (mut source_effects, source_tracks) = effects::import_source_effects(
                        clip,
                        layer_id,
                        false,
                        MaskBoundary::Flat,
                        frame,
                        frame,
                        canvas,
                        scope.effect_ids,
                        omissions,
                    );
                    source_effects.extend(effects);
                    effects = source_effects;
                    effect_tracks.extend(source_tracks);
                }
                if !effects::retains_coverage(clip, &effects, omissions) {
                    continue;
                }
                // Their keys are on the source clock, as the still's Motion
                // keys are, which an image layer's clock matches only under
                // unit forward playback.
                let retimed = retimed_keys_reason(clip).filter(|_| !effect_tracks.is_empty());
                if let Some(reason) = retimed {
                    effect_tracks.clear();
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        record,
                        format!(
                            "effect animation was not imported: {reason}; static values were kept"
                        ),
                    );
                }
                let active_range = tick_range(clip.start_ticks, clip.end_ticks)?;
                let transform = source_aspect_transform(
                    clip_transform(&clip.transform, clip.opacity, frame, canvas)?,
                    source,
                );
                let mut image = super::still::image_layer(
                    source,
                    asset_id,
                    layer_id,
                    format!("Premiere still {}", index + 1),
                    active_range,
                    !clip.enabled,
                    transform,
                )?;
                image.parent = scope.parent;
                image.blend_mode = clip.blend_mode.fx_mode();
                image.effects = effects;
                validate_time_range("active_range", image.active_range)?;
                // The one mask that a still keeps is its Crop (a Crop effect or
                // its Motion Crop) or its Opacity mask (`reader/still.rs`),
                // which mask the image's own frame before Motion: as a flat
                // video's, its guide is beside the image with the image's
                // transform and Motion keys.
                let guide = if clip.crop.is_default() && clip.opacity_mask.is_none() {
                    None
                } else {
                    let guide_id = LayerId::new(*scope.next_index as u64 + 1);
                    let mask_id = FxItemId::new(*scope.next_index as u64 + 2);
                    *scope.next_index += 2;
                    Some(match &clip.opacity_mask {
                        None => {
                            image
                                .masks
                                .push(crop_mask(&clip.crop, mask_id, guide_id, record, omissions));
                            fx_schema::LayerData::Rect(guide_layer(
                                guide_id,
                                format!("Premiere Crop guide {}", index + 1),
                                scope.parent,
                                active_range,
                                transform,
                                crop_rect(&clip.crop, frame),
                            ))
                        }
                        Some(mask) => {
                            let (path_mask, outline) =
                                opacity_mask(mask, mask_id, guide_id, frame, record, omissions)?;
                            image.masks.push(path_mask);
                            fx_schema::LayerData::Shape(shape_guide(
                                guide_id,
                                format!("Premiere Opacity mask {}", index + 1),
                                scope.parent,
                                active_range,
                                transform,
                                outline,
                            ))
                        }
                    })
                };
                let frame_guide = guide.as_ref().map(fx_schema::LayerData::id);
                let picture = super::invert_alpha::lower(
                    clip,
                    canvas,
                    frame,
                    Layer::from_data(&fx_schema::LayerData::Image(image))?,
                    scope.next_index,
                    omissions,
                )?;
                let picture_id = picture.id();
                layers.push(picture);
                if let Some(guide) = guide {
                    layers.push(Layer::from_data(&guide)?);
                }
                // A still's keys import as a video's do.
                let mut tracks = motion_tracks(
                    &clip.animations,
                    clip.in_ticks,
                    retimed_keys_reason(clip),
                    layer_id,
                    frame_guide,
                    frame,
                    canvas,
                    record,
                    omissions,
                );
                source_aspect_tracks(&mut tracks, source, &[Some(layer_id), frame_guide])?;
                set_tracks(dynamics, tracks)?;
                for (target, track) in effect_tracks {
                    dynamics
                        .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
                        .map_err(map_animation_graph_error)?;
                }
                let index = layers
                    .iter()
                    .position(|layer| layer.id() == picture_id)
                    .expect("new still occurrence");
                layers[index] = super::channel_levels::lower(
                    clip,
                    canvas,
                    frame,
                    layers[index].clone(),
                    scope.next_index,
                    scope.effect_ids,
                    dynamics,
                    omissions,
                )?;
                scope
                    .item_layers
                    .insert(item_key, ItemLayer::Plain(layers[index].id()));
                if let Some(warning) = clip.blend_mode.approximation() {
                    approximate(omissions, record, warning);
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
            let matte_shared = clip.track_matte.is_some_and(|matte| {
                crate::schema::track_matte_provider(
                    &project.video_tracks,
                    track_index,
                    clip.timeline_ticks(),
                    matte,
                )
                .is_ok_and(|provider| {
                    provider.range != clip.timeline_ticks()
                        || matte_consumers
                            .get(&(matte.track_index, provider.range.start))
                            .is_some_and(|count| *count > 1)
                })
            });
            let raster_asset = match clip
                .opacity_mask
                .as_ref()
                .and_then(|mask| mask.raster.as_ref())
            {
                Some(crate::schema::RasterMask::Prepared(id)) => Some(
                    asset_ids
                        .get(id)
                        .ok_or_else(|| unsupported("recovered Object Mask has no asset ID"))?,
                ),
                Some(crate::schema::RasterMask::Saved(_)) => {
                    omit(omissions, OmissionScope::Occurrence, record, "Object Mask requires source-bound sidecar recovery before editable import; masked occurrence omitted");
                    continue;
                }
                None => None,
            };
            let root = import_video_clip(
                VideoClip {
                    raster_asset,
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
            if let Some(mut root) = root {
                if let Some(index) = layers.iter().position(|layer| layer.id() == root.id()) {
                    let prepared_sample = if matches!(picture, ClipPicture::Linked)
                        && super::invert_alpha::admits_linked(
                            clip,
                            [project.width, project.height],
                            source.display_dimensions(),
                        ) {
                        // Reuse the linked source importer and its central identity
                        // allocation, mask-reference rewriting and recursive muting.
                        // Import just picture, never a second Premiere audio item.
                        let sample_index = *scope.next_index;
                        *scope.next_index += 1;
                        let mut samples = Vec::new();
                        let sample_root = import_video_clip(
                            VideoClip {
                                raster_asset: None,
                                clip,
                                source,
                                picture,
                                index: sample_index,
                                track_index,
                                matte: None,
                                matte_shared: false,
                                static_matte: false,
                            },
                            project,
                            &mut scope,
                            &mut samples,
                            dynamics,
                            omissions,
                        )?;
                        sample_root.and_then(|root| {
                            samples.into_iter().find(|layer| layer.id() == root.id())
                        })
                    } else {
                        None
                    };
                    layers[index] = super::invert_alpha::lower_with_sample(
                        clip,
                        [project.width, project.height],
                        source.display_dimensions(),
                        layers[index].clone(),
                        prepared_sample,
                        scope.next_index,
                        omissions,
                    )?;
                    layers[index] = super::channel_levels::lower(
                        clip,
                        [project.width, project.height],
                        source.display_dimensions(),
                        layers[index].clone(),
                        scope.next_index,
                        scope.effect_ids,
                        dynamics,
                        omissions,
                    )?;
                    if layers[index].id() != root.id() {
                        root = ItemLayer::Plain(layers[index].id());
                    }
                }
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
        let handled = super::cross_dissolve::import(
            project,
            track_index,
            &mut scope,
            &mut layers,
            media,
            &matte_consumers,
            dynamics,
            omissions,
        )?;
        import_transitions(
            project,
            &scope,
            track_index,
            &handled,
            &layers,
            media,
            &matte_consumers,
            dynamics,
            omissions,
        );
    }
    restrict_track_matte_consumers(
        project,
        &scope.item_layers,
        &mut layers,
        dynamics,
        omissions,
    )?;
    effects::finish_posterize_time_import(
        &mut layers,
        dynamics,
        scope.parent.is_some(),
        omissions,
    )?;
    omit_unconsumed_mattes(
        project,
        &scope.item_layers,
        &mut layers,
        dynamics,
        omissions,
    )?;
    super::adjustment_wipe::wrap(
        project,
        &adjustment_wipes,
        &mut scope,
        &mut layers,
        dynamics,
        omissions,
    )?;
    Ok(layers)
}

/// Keep the authored mappings/keys and provider clock. Only activation is
/// clipped to known coverage; rounding inward avoids an exposed edge sample.
fn restrict_track_matte_consumers(
    project: &PrSequence,
    item_layers: &ItemLayers,
    layers: &mut Vec<Layer>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<()> {
    for (index, track) in project.video_tracks.iter().enumerate() {
        let keyed = track
            .items
            .iter()
            .filter_map(PrVideoItem::media)
            .filter_map(|clip| Some((clip.timeline_ticks(), clip.track_matte?, clip.record())))
            .chain(track.nests.iter().filter_map(|nest| {
                Some((
                    nest.timeline_ticks(),
                    nest.track_matte?,
                    nest.id.as_deref().unwrap_or("nested placement"),
                ))
            }));
        for (range, matte, record) in keyed {
            let Ok(provider) = crate::schema::track_matte_provider(
                &project.video_tracks,
                index,
                range.clone(),
                matte,
            ) else {
                continue;
            };
            let Some(root) = item_layers.get(&(index, range.start)) else {
                continue;
            };
            let Some(position) = layers.iter().position(|layer| layer.id() == root.id()) else {
                continue;
            };
            if provider.covered_range == range {
                continue;
            }
            for uncovered in [
                range.start..provider.covered_range.start,
                provider.covered_range.end..range.end,
            ] {
                if uncovered.start < uncovered.end {
                    omit(omissions, OmissionScope::Feature, record,
                        format!("Track Matte uncovered interval {}..{} ticks omitted; covered content retained without extending or holding its provider, native outside-provider behavior is unproved", uncovered.start, uncovered.end));
                }
            }
            let ticks = i128::from(TICKS_PER_MILLISECOND);
            let first = -(-i128::from(provider.covered_range.start)).div_euclid(ticks);
            let last = i128::from(provider.covered_range.end).div_euclid(ticks);
            let old = layers[position].active_range();
            let first = first.max(i128::from(old.start.as_millis()));
            let last = last.min(i128::from(old.end().as_millis()));
            if last <= first {
                let data = layers[position].data();
                remove_layer_dynamics(dynamics, data)?;
                layers.remove(position);
                omit(omissions, OmissionScope::Occurrence, record, "Track Matte covered interval has no representable millisecond window; consumer omitted without exposing uncovered content");
                continue;
            }
            let window = TimeRangeProperty::new(
                Time::from_millis(
                    u64::try_from(first)
                        .map_err(|_| unsupported("negative matte coverage window"))?,
                ),
                Duration::from_millis(
                    u64::try_from(last - first)
                        .map_err(|_| unsupported("invalid matte coverage duration"))?,
                ),
            );
            let mut data = layers[position].data().clone();
            if matches!(data, LayerData::Rect(_) | LayerData::Image(_)) {
                rebase_layer_dynamics(dynamics, &data, old.start, window.start)?;
            }
            match &mut data {
                LayerData::Video(video) => {
                    video.playback = trimmed_matte_playback(&video.playback, window)?
                }
                LayerData::Group(group) => {
                    group.playback = trimmed_matte_playback(&group.playback, window)?
                }
                LayerData::Rect(rect) => rect.active_range = window,
                LayerData::Image(image) => image.active_range = window,
                _ => {
                    remove_layer_dynamics(dynamics, &data)?;
                    layers.remove(position);
                    omit(omissions, OmissionScope::Occurrence, record, "Track Matte consumer has no supported bounded activation window; occurrence omitted without exposing uncovered content");
                    continue;
                }
            }
            layers[position] = Layer::from_data(&data)?;
        }
    }
    Ok(())
}

/// Narrows only the parent-clock visibility window. `inputOffsetMs` addresses
/// that parent clock, so retaining it (with the authored mapping) preserves
/// every source/content sample at the same document time.
fn trimmed_matte_playback(
    playback: &fx_schema::LayerPlayback,
    window: TimeRangeProperty,
) -> Result<fx_schema::LayerPlayback> {
    use fx_schema::{LayerPlayback, LayerPlaybackMapping};
    match playback.mapping() {
        LayerPlaybackMapping::Linear { input, output } => {
            LayerPlayback::linear(window, *input, *output, playback.input_offset_ms())
        }
        LayerPlaybackMapping::TimeRemap { property } => {
            LayerPlayback::remapped(window, property.clone(), playback.input_offset_ms())
        }
    }
    .map_err(unsupported)
}

fn rebase_layer_dynamics(
    dynamics: &mut AnimationGraph,
    layer: &LayerData,
    old_start: Time,
    new_start: Time,
) -> Result<()> {
    let delta = new_start
        .as_millis()
        .checked_sub(old_start.as_millis())
        .and_then(|delta| i64::try_from(delta).ok())
        .ok_or_else(|| unsupported("Track Matte coverage start exceeds the animation clock"))?;
    if delta == 0 {
        return Ok(());
    }
    let mut entries = dynamics.entries().to_vec();
    for entry in &mut entries {
        if !layer_owns_target(layer, &entry.target) {
            continue;
        }
        let AnimatorData::Keyframes {
            track,
            enabled,
            disabled_value,
        } = entry.animator.data()
        else {
            continue;
        };
        let keys = track
            .keyframes()
            .iter()
            .map(|key| {
                let time = key
                    .layer_time()
                    .as_millis()
                    .checked_sub(delta)
                    .ok_or_else(|| unsupported("Track Matte animation clock exceeds i64"))?;
                Ok(PropertyKeyframe::new(
                    key.id().clone(),
                    TimeOffset::from_millis(time),
                    key.value().clone(),
                    key.easing(),
                )
                .with_spatial_tangents(key.spatial_in_tangent(), key.spatial_out_tangent()))
            })
            .collect::<Result<Vec<_>>>()?;
        let track =
            PropertyKeyframeTrack::new(keys).map_err(|error| unsupported(error.to_string()))?;
        entry.animator = PropertyAnimator::from_data(&AnimatorData::Keyframes {
            track,
            enabled: *enabled,
            disabled_value: disabled_value.clone(),
        })?;
    }
    *dynamics = AnimationGraph::from_entries(entries).map_err(map_animation_graph_error)?;
    Ok(())
}

fn remove_layer_dynamics(dynamics: &mut AnimationGraph, layer: &LayerData) -> Result<()> {
    let mut removed = BTreeSet::new();
    for entry in dynamics.entries() {
        if layer_owns_target(layer, &entry.target) {
            removed.insert(entry.target.clone());
        }
    }
    let mut entries = dynamics.entries().to_vec();
    loop {
        let before = entries.len();
        entries.retain(|entry| {
            let remove = removed.contains(&entry.target)
                || entry
                    .dependencies
                    .iter()
                    .any(|target| layer_owns_target(layer, target) || removed.contains(target))
                || entry.random_seed_target.as_ref().is_some_and(|target| {
                    layer_owns_target(layer, target) || removed.contains(target)
                });
            if remove {
                removed.insert(entry.target.clone());
            }
            !remove
        });
        if entries.len() == before {
            break;
        }
    }
    *dynamics = AnimationGraph::from_entries(entries).map_err(map_animation_graph_error)?;
    Ok(())
}

fn layer_owns_target(layer: &LayerData, target: &fx_schema::PropertyTarget) -> bool {
    if target.layer_id() == Some(layer.id()) {
        return true;
    }
    if target.effect_id().is_some_and(|target_id| {
        layer.effects().iter().any(|effect| {
            matches!(
                effect.data(),
                fx_schema::EffectData::Identified { id, .. } if *id == target_id
            )
        })
    }) {
        return true;
    }
    let masks = match layer {
        LayerData::Rect(rect) => rect.masks.as_slice(),
        LayerData::Image(image) => image.masks.as_slice(),
        _ => &[],
    };
    target
        .fx_item_id()
        .is_some_and(|target_id| masks.iter().any(|mask| mask.id == target_id))
}

/// The layer that `layer`'s track matte consumes.
fn consumed_matte(layer: &Layer) -> Option<LayerId> {
    match layer.data() {
        LayerData::Video(video) => video.track_matte.as_ref(),
        LayerData::Group(group) => group.track_matte.as_ref(),
        LayerData::Rect(rect) => rect.track_matte.as_ref(),
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
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<()> {
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
                PrVideoItem::Capsule(capsule) => capsule.placement.id.clone().unwrap_or_default(),
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
            .filter_map(|clip| Some((clip.timeline_ticks(), clip.track_matte?)))
            .chain(
                track
                    .nests
                    .iter()
                    .filter_map(|nest| Some((nest.timeline_ticks(), nest.track_matte?))),
            );
        for (range, matte) in keyed {
            let start = range.start;
            let Ok(provider) = crate::schema::track_matte_provider(
                &project.video_tracks,
                track_index,
                range,
                matte,
            ) else {
                continue;
            };
            let placed = item_layers
                .get(&(track_index, start))
                .is_some_and(|root| layers.iter().any(|layer| layer.id() == root.id()));
            // A matte clip that was not converted left no layer to drop.
            let Some(matte_layer) = item_layers
                .get(&(matte.track_index, provider.range.start))
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
            let Some(position) = layers.iter().position(|layer| layer.id() == matte_layer) else {
                continue;
            };
            remove_layer_dynamics(dynamics, layers[position].data())?;
            layers.remove(position);
            omit(
                omissions,
                OmissionScope::Occurrence,
                record_at(matte.track_index, provider.range.start),
                format!(
                    "matte source of the omitted clip {} was not converted: Premiere does not draw a track-matte source",
                    record_at(track_index, start)
                ),
            );
        }
    }
    Ok(())
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
    tracks: &[PrVideoTrack],
    matte: PrTrackMatte,
    range: Range<i64>,
    track_index: usize,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> std::result::Result<TrackMatte, String> {
    let provider = crate::schema::track_matte_provider(tracks, track_index, range.clone(), matte)?;
    let layer = item_layers
        .get(&(matte.track_index, provider.range.start))
        .map(|item| item.id())
        .ok_or_else(|| {
            format!(
                "track {track_index}, range {}..{} ticks: native matte provider {} on track {} at {}..{} ticks was not converted; occurrence omitted",
                range.start, range.end, provider.record.unwrap_or("unnamed"), matte.track_index, provider.range.start, provider.range.end
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
/// video, or moves a graphic's content under its clip Motion. Its keys stay
/// layer-local, and a group keeps its own children.
pub(super) fn into_stage(
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
                "{} layer {} cannot be relocated into an editable picture stage; supported layer types are Video, Group, Image, Rect, Shape and Text",
                layer.layer_type_name(), layer.id()
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
    /// Cross Dissolve New: a linear, encoded-RGB opacity ramp.
    CrossDissolve,
}

impl OpacityDissolve {
    /// The name in the ramp's keyframe ids ([`keyframe_id`]).
    fn key_name(self) -> &'static str {
        match self {
            Self::FilmImpact => "film-impact-dissolve",
            Self::CrossDissolve => "cross-dissolve",
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
            Self::CrossDissolve => PropertyKeyframeEasing::Linear,
        }
    }

    /// The report of a converted ramp.
    fn approximation(self) -> &'static str {
        match self {
            Self::FilmImpact => "Film Impact default one-sided dissolve retained as editable smoothstep opacity; temporal curve and SDR encoded-value compositing approximate the measured native linear-light fade",
            Self::CrossDissolve => "Cross Dissolve New retained as editable linear opacity at the picture boundary; opaque SDR controls follow the measured linear ramp, while alpha and general edited native fidelity remain unmeasured",
        }
    }
}

/// The two measured Film Impact profiles own separate editable properties:
/// Dissolve opacity and Pop geometry. Cross Dissolve New uses linear opacity
/// for a head or tail; two-sided weighted pictures are handled separately.
/// `matte_consumers` counts the placements that key each matte item, by the
/// matte's track and start, as [`video_layers`] counts them.
#[expect(clippy::too_many_arguments)]
fn import_transitions(
    project: &PrSequence,
    scope: &LayerScope<'_, '_>,
    track_index: usize,
    handled: &BTreeSet<usize>,
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
    for (index, transition) in track.transitions.iter().enumerate() {
        if handled.contains(&index) {
            continue;
        }
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
            PrVideoTransitionKind::CrossDissolve => OpacityDissolve::CrossDissolve,
            // Converted above.
            PrVideoTransitionKind::FilmImpactPop => continue,
        };
        let converted = (|| -> Result<()> {
            let (id, tail) = match (
                dissolve,
                transition.outgoing_clip.as_deref(),
                transition.incoming_clip.as_deref(),
            ) {
                (_, Some(id), None) => (id, true),
                (_, None, Some(id)) => (id, false),
                _ => return Err(unsupported("requires one linked head or tail picture")),
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
                            | PrMediaKind::OpenExr {
                                numbered: false,
                                ..
                            }
                            | PrMediaKind::AfterEffectsComposition(_)
                    )
                ),
                OpacityDissolve::CrossDissolve => matches!(
                    source_kind,
                    Some(
                        PrMediaKind::Video { .. }
                            | PrMediaKind::Still { .. }
                            | PrMediaKind::OpenExr {
                                numbered: false,
                                ..
                            }
                            | PrMediaKind::ColorMatte(_)
                            | PrMediaKind::AfterEffectsComposition(_)
                    )
                ),
            };
            if dissolve == OpacityDissolve::CrossDissolve {
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
                    (other.kind == transition.kind
                        || (dissolve == OpacityDissolve::CrossDissolve
                            && other.kind == PrVideoTransitionKind::FilmImpactDissolve))
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
                        | (
                            OpacityDissolve::CrossDissolve,
                            LayerData::Rect(_) | LayerData::Image(_) | LayerData::Video(_),
                        ) => true,
                        (_, LayerData::Group(_)) => {
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
                    OpacityDissolve::CrossDissolve => format!(
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
    raster_asset: Option<&'a fx_schema::AssetId>,
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
    /// Whether the provider must stay on its independent clock: another
    /// consumer shares it, or its native placement window differs.
    matte_shared: bool,
    /// A4 measured only an unshared canvas-sized still at default static Motion.
    static_matte: bool,
}

/// Why a keyed picture's Transform is omitted when its matte is outside the
/// form that A4 measured ([`measured_static_matte`]); the Track Matte Key
/// itself converts as without the Transform.
const UNMEASURED_MATTE_TRANSFORM_REASON: &str = "Transform with Track Matte Key requires a canvas-sized still matte, unshared and at default static Motion, opaque, normal blend and unretimed (measured A4); Transform omitted, existing Track Matte Key retained";

/// Whether the matte that `clip` keys is an unshared canvas-sized still at
/// default static Motion, without effects of its own or of its master clip,
/// the only matte A4 measured a Transform against.
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
        && source.source_effects.is_none()
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
    scope: &mut LayerScope<'_, '_>,
    siblings: &mut Vec<Layer>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Option<ItemLayer>> {
    let VideoClip {
        raster_asset,
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
    let effect_scope = match super::effect_mask::native_scope(
        clip,
        source.kind,
        [source.width, source.height],
        [project.width, project.height],
        scope.document_canvas,
    ) {
        Ok(index) if index.is_none() || source.interpretation == Default::default() => index,
        result => {
            let reason = result
                .err()
                .unwrap_or_else(|| "effect masks on interpreted media are not converted".into());
            omit(
                omissions,
                OmissionScope::Occurrence,
                clip.record(),
                format!("effect mask scope: {reason}; occurrence omitted"),
            );
            return Ok(None);
        }
    };
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
    // How the clip's mask converts with `source_effects` source effects on its
    // picture before it.
    let boundary = |source_effects| {
        clip.mask_boundary(
            [source.width, source.height],
            [project.width, project.height],
            source_effects,
        )
        .map(|boundary| if effect_scope.is_some() { MaskBoundary::Staged } else { boundary })
        .and_then(|boundary| {
            let boundary = if raster_asset.is_some() { MaskBoundary::Staged } else { boundary };
            let Some(matte) = matte.as_ref().filter(|_| boundary == MaskBoundary::Staged)
            else {
                return Ok(boundary);
            };
            // A stage group moves its matte under itself, where another clip's
            // key could not evaluate it on that clip's clock.
            if matte_shared {
                let independent_window = clip.track_matte.is_some_and(|key| {
                    crate::schema::track_matte_provider(&project.video_tracks, track_index, clip.timeline_ticks(), key)
                        .is_ok_and(|provider| provider.range != clip.timeline_ticks())
                });
                return Err(if independent_window {
                    "a Track Matte Key whose provider has an independent placement window is not converted on a clip that its Motion or effects stage; provider clock cannot be rebased"
                } else {
                    "a Track Matte Key whose matte keys another clip too is not converted on a clip that its Motion or effects stage"
                });
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
        })
    };
    // Source effects apply before the clip's whole pipeline, so they stage its
    // mask as its own effects applied before the mask do. A clip that cannot
    // take them there usually converts without them. Legacy Luma instead keeps
    // its stack on the available picture with a diagnosed order approximation;
    // `retains_coverage` rejects any later failure to carry that enabled key.
    let source_stack = clip
        .source_effects
        .as_ref()
        .filter(|stack| !stack.effects.is_empty());
    let (boundary, converts_source) = match (
        source_stack,
        boundary(source_stack.map_or(0, |stack| stack.effects.len())),
    ) {
        (Some(_), Ok(boundary)) => (Ok(boundary), true),
        (Some(stack), Err(reason)) => {
            let without = boundary(0);
            let retain_key = without.is_ok()
                && stack
                    .effects
                    .iter()
                    .any(crate::schema::PrEffect::requires_coverage);
            if without.is_ok() && !retain_key {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    clip.record(),
                    format!(
                        "source effects of {} were not imported: {reason}",
                        stack.master
                    ),
                );
            }
            if retain_key {
                approximate(omissions, clip.record(), format!("Legacy Luma source stack of {} retained on the available picture plane to preserve coverage; native source/mask order is approximate: {reason}", stack.master));
            }
            (without, retain_key)
        }
        (None, boundary) => (boundary, false),
    };
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
    }) = clip_timing(clip, source, boundary, scope, project, record, omissions)?
    else {
        return Ok(None);
    };
    let retimed_keys = retimed_keys_reason(clip);
    let transform = source_aspect_transform(
        clip_transform(
            &clip.transform,
            clip.opacity,
            [source.width, source.height],
            [project.width, project.height],
        )?,
        source,
    );
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
    // A keyed Mask Path imports as its guide's outline keys, or the clip is
    // omitted, before it requests anything, rather than shown with its mask
    // frozen at one outline.
    let outline_keys = match (&clip.opacity_mask, mask_ids) {
        (Some(mask), Some((guide_id, _))) if !mask.path_keys.is_empty() => {
            let track = match mask_path_keys_reason(clip) {
                Some(reason) => Err(reason.to_owned()),
                None => {
                    mask_path_track(mask, guide_id, clip.in_ticks, [source.width, source.height])
                }
            };
            match track {
                Ok(track) => Some((Property::new(guide_id, PropType::ShapePath), track)),
                Err(reason) => {
                    *scope.next_index = first_index;
                    omit(
                        omissions,
                        OmissionScope::Occurrence,
                        record,
                        format!(
                            "track {track_index}, range {}..{} ticks: Mask Path keys were not imported: {reason}; occurrence omitted",
                            clip.start_ticks, clip.end_ticks
                        ),
                    );
                    return Ok(None);
                }
            }
        }
        _ => None,
    };
    let numeric_tracks = match (&clip.opacity_mask, mask_ids) {
        (Some(mask), Some((_, mask_id))) if mask.has_numeric_keys() => {
            let tracks = match mask_path_keys_reason(clip) {
                Some(reason) => Err(reason.to_owned()),
                None => super::mask_animation::import_tracks(mask, mask_id, clip.in_ticks),
            };
            match tracks {
                Ok(tracks) => tracks,
                Err(reason) => {
                    *scope.next_index = first_index;
                    omit(omissions, OmissionScope::Occurrence, record,
                        format!("numeric Opacity mask keys were not imported: {reason}; occurrence omitted"));
                    return Ok(None);
                }
            }
        }
        _ => Vec::new(),
    };
    let mut layer_tracks = Vec::new();
    let mut retained_curved_transform_position = false;
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
            Ok(tracks) => {
                layer_tracks.extend(tracks);
                retained_curved_transform_position =
                    curved_transform_position_approximation(effect).is_some();
            }
            Err(reason) => omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("Transform animation was not imported: {reason}; static values were kept"),
            ),
        }
    }
    // Explicit Video playback already warps its own property clock into media
    // time. Keep the native Rotation times; subtracting In would apply the trim
    // twice. A staged Motion owner does not share the video's playback clock.
    let media_clock_rotation = clip.has_media_clock_rotation()
        && matches!(source.kind, PrMediaKind::Video { .. })
        && group_id.is_none()
        && scope.parent.is_none();
    layer_tracks.extend(motion_tracks(
        &clip.animations,
        if media_clock_rotation {
            0
        } else {
            clip.in_ticks
        },
        if media_clock_rotation {
            None
        } else {
            retimed_keys
        },
        motion_owner,
        frame_guide,
        [source.width, source.height],
        [project.width, project.height],
        record,
        omissions,
    ));
    if let (false, Some((guide_id, mask_id))) = (clip.crop.is_default(), mask_ids) {
        masks.push(crop_mask(&clip.crop, mask_id, guide_id, record, omissions));
        guides.push(Layer::from_data(&fx_schema::LayerData::Rect(guide_layer(
            guide_id,
            format!("Premiere Crop guide {}", index + 1),
            video_parent,
            video_range,
            // Premiere applies this standard Crop before its fixed Motion
            // effect, so the editable guide must share the video's
            // transform, and a flat guide also its Motion keys (above).
            video_transform,
            crop_rect(&clip.crop, [source.width, source.height]),
        )))?);
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
    let mut raster_matte = None;
    if let (Some(asset), Some(mask), Some((guide_id, _))) =
        (raster_asset, &clip.opacity_mask, mask_ids)
    {
        let sampled = match crate::numbered_images::sampling::occurrence(
            clip,
            source,
            project.frame_rate,
        ) {
            Ok(sampled) => sampled,
            Err(error) => {
                *scope.next_index = first_index;
                omit(omissions, OmissionScope::Occurrence, record, format!("Object Mask sampled coverage could not be retained: {error}; masked occurrence omitted"));
                return Ok(None);
            }
        };
        let origin = active_range.start.as_millis();
        let duration = video_range.duration.as_millis();
        // The source picture keeps its native trim/clock. Translate sampled
        // boundaries to the existing stage origin; do not shift the video to
        // the first output sample or independently retime it.
        let mut frames: Vec<_> = sampled
            .frames
            .into_iter()
            .filter_map(|(index, range)| {
                let start = sampled.window.start.as_millis() + range.start.as_millis();
                let end = start + range.duration.as_millis();
                let left = start.saturating_sub(origin).min(duration);
                let right = end.saturating_sub(origin).min(duration);
                (right > left).then_some((
                    index,
                    TimeRangeProperty::new(
                        Time::from_millis(left),
                        Duration::from_millis(right - left),
                    ),
                ))
            })
            .collect();
        if let Some((_, first)) = frames.first_mut() {
            first.duration =
                Duration::from_millis(first.start.as_millis() + first.duration.as_millis());
            first.start = Time::ZERO;
        } else {
            *scope.next_index = first_index;
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                "Object Mask has no sampled coverage in its trim; masked occurrence omitted",
            );
            return Ok(None);
        }
        let frames =
            super::timed_images::import_frames(source, asset, guide_id, frames, scope.next_index)?;
        let mut provider = plain_group(
            guide_id,
            "Object Mask supplied matte".to_owned(),
            video_range,
            identity_transform(),
            frames,
        )?;
        provider.parent = video_parent;
        provider.transform.opacity =
            PercentageProperty::new(mask.opacity).expect("validated mask opacity");
        guides.push(Layer::from_data(&LayerData::Group(provider))?);
        raster_matte = Some(TrackMatte {
            layer: guide_id,
            mode: if mask.inverted {
                TrackMatteType::AlphaInverted
            } else {
                TrackMatteType::Alpha
            },
        });
    }
    if let (Some(mask), Some((guide_id, mask_id))) = (&clip.opacity_mask, mask_ids) {
        if mask.raster.is_none() {
            // In the video's frame like the Crop guide.
            let (path_mask, outline) = opacity_mask(
                mask,
                mask_id,
                guide_id,
                [source.width, source.height],
                record,
                omissions,
            )?;
            guides.push(Layer::from_data(&fx_schema::LayerData::Shape(
                shape_guide(
                    guide_id,
                    format!("Premiere Opacity mask {}", index + 1),
                    video_parent,
                    video_range,
                    video_transform,
                    outline,
                ),
            ))?);
            masks.push(path_mask);
            tracks.extend(outline_keys);
        }
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
    // FX lays a video layer's effects over its media frame, and a group's,
    // the linked composition's picture, over the document canvas even in a nest.
    let effects_frame = match picture {
        ClipPicture::Asset(_) => [source.width, source.height],
        ClipPicture::Linked => scope.document_canvas,
    };
    // The source effects come first in the picture's one stack, on the
    // clip's clock, as Premiere applies them first there.
    // Constant-rate video playback is already an explicit FX media-time
    // remap (including reverse). Its own effects sample that media clock,
    // unlike a plain unit-speed layer's elapsed clock. Keep ascending source
    // keys/easing unchanged; playback, not a second key reversal, traverses them.
    let media_clock_effects = matches!(picture, ClipPicture::Asset(_))
        && matches!(source.kind, PrMediaKind::Video { .. })
        && clip.playback_rate != 1.0
        && clip.time_remap.is_none();
    let (mut effects, mut effect_tracks) = if converts_source {
        effects::import_source_effects(
            clip,
            layer_id,
            media_clock_effects,
            boundary,
            [source.width, source.height],
            effects_frame,
            [project.width, project.height],
            scope.effect_ids,
            omissions,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let source_converted = !effects.is_empty();
    let (own_effects, own_tracks, scope_children, scope_suffix) = if let Some(native) = effect_scope
    {
        match super::effect_mask::import(
            clip,
            native,
            layer_id,
            group_id.ok_or_else(|| unsupported("effect scope has no group"))?,
            video_range,
            scope,
            omissions,
        ) {
            Ok(imported) => (
                imported.prefix,
                imported.tracks,
                imported.children,
                imported.suffix,
            ),
            Err(reason) => {
                *scope.next_index = first_index;
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    record,
                    format!("effect mask scope: {reason}; occurrence omitted"),
                );
                return Ok(None);
            }
        }
    } else {
        let (effects, tracks) = effects::import_effects(
            clip,
            layer_id,
            media_clock_effects,
            boundary,
            scope.parent.is_some(),
            false,
            source.kind,
            [source.width, source.height],
            effects_frame,
            [project.width, project.height],
            scope.effect_ids,
            omissions,
        );
        (effects, tracks, Vec::new(), Vec::new())
    };
    effects.extend(own_effects);
    effect_tracks.extend(own_tracks);
    if effect_scope.is_none() && !effects::retains_coverage(clip, &effects, omissions) {
        *scope.next_index = first_index;
        return Ok(None);
    }
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
                playback,
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
                // Fill the coded frame: contain would also apply the file's
                // display aspect and letterbox before our saved-PAR scale.
                source: VideoSource::from_asset(asset_id.clone(), Some(frame), MediaFit::Stretch),
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
            let playback_offset_ms = playback.input_offset_ms();
            let clock = match playback.time_remap().cloned() {
                Some(playback) => Some(playback),
                None if source_range.start != Time::ZERO => {
                    Some(constant_time_remap(false, video_range, source_range)?)
                }
                None => None,
            };
            let clock = match clock
                .map(|clock| on_clip_clock(&clock, video_range.start, playback_offset_ms))
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
        (LayerData::Video(video), Some(profile)) => {
            match stroke::validate(clip, source, scope, source_converted) {
                Ok(()) => {
                    approximate(omissions, record, "Film Impact Stroke retained as editable centered prescale with the measured neutral-profile border approximation; general Size and alpha semantics remain unsupported");
                    (stroke::wrap(video.clone(), profile, source, scope)?, true)
                }
                Err(reason) => {
                    if profile.hides_source() {
                        omit(omissions, OmissionScope::Occurrence, record,
                            format!("Stroke outline cannot be isolated: {reason}; source remains concealed; independent siblings/audio retained"));
                        return Ok(None);
                    }
                    omit(
                        omissions,
                        OmissionScope::Feature,
                        record,
                        reason.to_string(),
                    );
                    (Layer::from_data(&picture)?, false)
                }
            }
        }
        (_, Some(profile)) => {
            if profile.hides_source() {
                omit(omissions, OmissionScope::Occurrence, record,
                    "Stroke source remains concealed on an unsupported host; independent siblings/audio retained");
                return Ok(None);
            }
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
    let mut clip_layers = scope_children;
    clip_layers.push(picture);
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
                    source_aspect_transform(
                        staged_video_transform(effect_transform, [source.width, source.height])?,
                        source,
                    )
                }
                _ => transform,
            };
            ensure!(
                raster_matte.is_none() || group_matte.is_none(),
                "Object Mask raster and ordinary group matte cannot both own the same stage"
            );
            let group = GroupLayer {
                is_hidden: !clip.enabled,
                parent: scope.parent,
                blend_mode: group_blend,
                track_matte: raster_matte.or(group_matte),
                masks: group_masks,
                effects: scope_suffix,
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
    source_aspect_tracks(&mut tracks, source, &[Some(motion_owner), frame_guide])?;
    set_tracks(dynamics, tracks)?;
    for (target, track) in effect_tracks.into_iter().chain(numeric_tracks) {
        dynamics
            .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
            .map_err(map_animation_graph_error)?;
    }
    if let Some(warning) = clip.blend_mode.approximation() {
        approximate(omissions, record, warning);
    }
    if retained_curved_transform_position {
        approximate(omissions, record, CURVED_TRANSFORM_POSITION_APPROXIMATION);
    }
    effects::report_source_effects(clip, source_converted, omissions);
    siblings.extend(layers);
    Ok(Some(if stroke_owner {
        ItemLayer::Stroke(root)
    } else {
        ItemLayer::Plain(root)
    }))
}

fn source_aspect_transform(mut transform: Transform, source: &PrVideoStream) -> Transform {
    let scale = source.pixel_scale();
    transform.scale[0] *= scale[0];
    transform.scale[1] *= scale[1];
    transform
}

fn source_aspect_tracks(
    tracks: &mut [(Property, PropertyKeyframeTrack)],
    source: &PrVideoStream,
    owners: &[Option<LayerId>],
) -> Result<()> {
    if source.pixel_aspect.is_square() {
        return Ok(());
    }
    let scale = source.pixel_scale();
    for (property, track) in tracks {
        if !owners.contains(&Some(property.layer_id())) {
            continue;
        }
        let factor = match property.property_type() {
            PropType::ScaleX => scale[0],
            PropType::ScaleY => scale[1],
            _ => continue,
        };
        if factor == 1.0 {
            continue;
        }
        let keys = track
            .keyframes()
            .iter()
            .map(|key| {
                let PropertyValue::Float(value) = key.value() else {
                    return Err(unsupported("source scale key must be numeric"));
                };
                Ok(PropertyKeyframe::new(
                    key.id().clone(),
                    key.layer_time(),
                    PropertyValue::Float(*value * factor),
                    key.easing(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        *track = PropertyKeyframeTrack::new(keys)
            .map_err(|error| unsupported(format!("source pixel aspect scale keys: {error}")))?;
    }
    Ok(())
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
    Ok(Transform {
        opacity: PercentageProperty::new(opacity)
            .ok_or_else(|| unsupported("Premiere opacity must be between 0 and 100"))?,
        ..motion_transform(motion, source, canvas)
    })
}

/// The FX transform of the static Motion `motion` of a picture of `source`
/// pixels in a `canvas`, at full opacity, as [`clip_transform`] maps it.
pub(super) fn motion_transform(
    motion: &PrStaticTransform,
    source: [u32; 2],
    canvas: [u32; 2],
) -> Transform {
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
    transform
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
    playback: fx_schema::LayerPlayback,
}

/// The timing of `clip`, which plays `source` and whose mask is at
/// `boundary`. An unrepresentable optional curve retains independently valid
/// saved constant playback, with an approximation diagnostic; otherwise `None`
/// omits the clip with its reason in `omissions`.
fn clip_timing(
    clip: &PrVideoOccurrence,
    source: &PrVideoStream,
    boundary: MaskBoundary,
    scope: &LayerScope<'_, '_>,
    project: &PrSequence,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<Option<ClipTiming>> {
    let active_range = tick_range(clip.start_ticks, clip.end_ticks)?;
    if !matches!(
        source.interpretation,
        crate::schema::SourceInterpretation::Original
    ) {
        let mut imported = || -> Result<ClipTiming> {
            let clock = scope
                .picture_clocks
                .get(&clip.media)
                .ok_or_else(|| unsupported("interpreted picture has no bound physical clock"))?
                .as_ref()
                .map_err(|error| unsupported(format!("interpreted picture: {error}")))?;
            let crate::media::PictureClock::Interpreted(clock) = clock else {
                return Err(unsupported(
                    "interpreted picture requires an interpreted physical clock",
                ));
            };
            let (video_range, origin) = match boundary {
                MaskBoundary::Staged => {
                    // The stage starts at rounded parent milliseconds. Retain
                    // the native residual in its child clock, rather than
                    // making staging an implicit source-origin approximation.
                    let rounded = timing::ticks_from_time(active_range.start, "stage origin")?;
                    (
                        TimeRangeProperty::new(Time::ZERO, active_range.duration),
                        clip.start_ticks
                            .checked_sub(rounded)
                            .ok_or_else(|| unsupported("interpreted stage origin overflows"))?,
                    )
                }
                MaskBoundary::Flat => (active_range, clip.start_ticks),
            };
            let exact = if clip.playback_rate == 1.0 && clip.time_remap.is_none() {
                match timing::interpreted_playback(*clock, video_range, clip.in_ticks, origin) {
                    Ok(mapping) => Some(mapping),
                    Err(reason) => {
                        approximate(omissions, record, format!("interpreted affine origin approximated on the existing millisecond source clock: {reason}; physical selection and picture retained"));
                        None
                    }
                }
            } else {
                None
            };
            let (playback, source_range) = if let Some(exact) = exact {
                exact
            } else {
                let (mut start, mut end) = (clip.in_ticks, clip.out_ticks);
                if let Some(remap) = &clip.time_remap {
                    // Approximate easing on the consumed input window, not the
                    // min/max of unused historical source keys.
                    let at = |time: i64| -> Result<i64> {
                        let pair = remap
                            .keys
                            .windows(2)
                            .find(|pair| {
                                pair[0].timeline_ticks <= time && time <= pair[1].timeline_ticks
                            })
                            .ok_or_else(|| {
                                unsupported(
                                    "interpreted remap does not cover the current input window",
                                )
                            })?;
                        let span =
                            i128::from(pair[1].timeline_ticks) - i128::from(pair[0].timeline_ticks);
                        let delta =
                            i128::from(pair[1].source_ticks) - i128::from(pair[0].source_ticks);
                        let value = i128::from(pair[0].source_ticks)
                            + (i128::from(time) - i128::from(pair[0].timeline_ticks)) * delta
                                / span;
                        i64::try_from(value)
                            .map_err(|_| unsupported("interpreted remap endpoint overflows"))
                    };
                    (start, end) = (at(0)?, at(clip.out_ticks - clip.in_ticks)?);
                } else if clip.playback_rate < 0.0 {
                    let intrinsic = source.interpreted_duration()?;
                    (start, end) = (intrinsic - end, intrinsic - start);
                }
                let (n, d) = clock.ratio();
                let denominator = d
                    .checked_mul(i128::from(crate::schema::TICKS_PER_MILLISECOND))
                    .ok_or_else(|| unsupported("interpreted source clock overflows"))?;
                let mapped = |ticks: i64| -> Result<u64> {
                    let value = i128::from(ticks)
                        .checked_mul(n)
                        .ok_or_else(|| unsupported("interpreted source clock overflows"))?
                        / denominator;
                    u64::try_from(value)
                        .map_err(|_| unsupported("interpreted selection is negative"))
                };
                let physical_start = mapped(start)?;
                let physical_end = mapped(end)?.min(clock.duration_millis());
                let held = clip.held_source_ticks().map(mapped).transpose()?;
                ensure!(
                    physical_end > physical_start
                        || held.is_some_and(|time| time < clock.duration_millis()),
                    "interpreted selection has no physical picture span"
                );
                let selection = if held.is_some() {
                    TimeRangeProperty::new(
                        Time::ZERO,
                        Duration::from_millis(clock.duration_millis()),
                    )
                } else {
                    TimeRangeProperty::new(
                        Time::from_millis(physical_start),
                        Duration::from_millis(physical_end - physical_start),
                    )
                };
                approximate(omissions, record,
                    "interpreted TimeRemapping/speed retained as constant playback over the authored physical source selection; non-linear timing and fractional endpoint loss are approximate; owner controls and independent effects retained");
                let mut mapping =
                    constant_time_remap(clip.playback_rate < 0.0, video_range, selection)?;
                if let Some(held) = held {
                    let keys = mapping
                        .keyframes()
                        .iter()
                        .cloned()
                        .map(|mut key| {
                            key.value = Time::from_millis(held);
                            key
                        })
                        .collect();
                    mapping = TimeRemapProperty::new(
                        keys,
                        TimeRemapExtrapolation::Inactive,
                        TimeRemapExtrapolation::Inactive,
                    )
                    .map_err(|error| {
                        unsupported(format!(
                            "interpreted held playback cannot be imported: {error}"
                        ))
                    })?;
                }
                (
                    fx_schema::LayerPlayback::remapped(video_range, mapping, 0)
                        .map_err(unsupported)?,
                    selection,
                )
            };
            Ok(ClipTiming {
                active_range,
                video_range,
                source_range,
                playback,
                source_intrinsic_duration: Duration::from_millis(clock.duration_millis()),
            })
        };
        return match imported() {
            Ok(timing) => Ok(Some(timing)),
            Err(error) => {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    record,
                    format!("interpreted picture was not imported: {error}"),
                );
                Ok(None)
            }
        };
    }
    // Under a stage group the video uses the group clock, starting at zero.
    let (video_range, clock_origin) = match boundary {
        MaskBoundary::Staged => (TimeRangeProperty::new(Time::ZERO, active_range.duration), 0),
        MaskBoundary::Flat => (active_range, clip.start_ticks),
    };
    // Check the bound clock before either curve preparation or constant recovery;
    // a presentation-origin source cannot acquire a new playback clock.
    let presentation_origin = match scope.picture_clocks.get(&clip.media) {
        Some(clock) => {
            let clock = clock
                .as_ref()
                .map_err(|error| unsupported(format!("picture clock: {error}")))?;
            if let crate::media::PictureClock::PresentationOrigin(origin) = clock {
                ensure!(
                    clip.playback_rate == 1.0 && clip.time_remap.is_none(),
                    "presentation-origin picture requires unit-forward playback"
                );
                Some(*origin)
            } else {
                None
            }
        }
        None => None,
    };
    let constant =
        || constant_clip_timing(clip, source, active_range, video_range, presentation_origin);
    let Some(remap) = &clip.time_remap else {
        let timing = constant()?;
        // The reader may already have discarded an optional curve after native
        // base validation. Prepare its rounded source span before any mutation.
        if scope.parent.is_none()
            && matches!(source.kind, crate::schema::PrMediaKind::Video { .. })
            && timing.source_range.duration.as_millis() == 0
        {
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                "clip was not imported: saved source span rounds to an empty editable range",
            );
            return Ok(None);
        }
        return Ok(Some(timing));
    };
    // Curve preparation has no shared layer, effect or animator mutation.
    match played_time_remap(clip, remap, source, video_range, clock_origin) {
        Ok((property, offset)) => {
            let source_intrinsic_duration = duration_from_ticks(source.intrinsic_ticks)?;
            Ok(Some(ClipTiming {
                active_range,
                video_range,
                source_range: TimeRangeProperty::new(Time::ZERO, source_intrinsic_duration),
                source_intrinsic_duration,
                playback: fx_schema::LayerPlayback::remapped(video_range, property, offset)
                    .map_err(unsupported)?,
            }))
        }
        Err(error) => {
            // Only physical video with an ordinary native curve may discard it.
            // FrameHold and nonphysical callers keep their existing omission.
            if scope.parent.is_none()
                && matches!(source.kind, crate::schema::PrMediaKind::Video { .. })
                && remap
                    .held_source_ticks(clip.end_ticks - clip.start_ticks)
                    .is_none()
                && clip.validate_time_remap_curve(remap, source).is_ok()
            {
                let base = clip.validate_base_on_grid(
                    project.frame_rate,
                    project
                        .native_frame_ticks
                        .unwrap_or(project.frame_rate.ticks_per_frame()),
                    source,
                );
                let base = base.map_err(crate::error::BuildError::from).and_then(|()| {
                    let timing = constant()?;
                    ensure!(
                        timing.source_range.duration.as_millis() > 0,
                        "saved source span rounds to an empty editable range"
                    );
                    ensure!(
                        u128::from(timing.source_range.start.as_millis())
                            + u128::from(timing.source_range.duration.as_millis())
                            <= u128::try_from(
                                source.intrinsic_ticks / crate::schema::TICKS_PER_MILLISECOND
                            )
                            .map_err(|_| unsupported("negative physical source duration"))?,
                        "rounded saved source selection reaches beyond the physical clock"
                    );
                    Ok(timing)
                });
                if let Ok(timing) = base {
                    approximate(omissions, record, format!(
                        "TimeRemapping was not imported; saved constant-rate playback retained as an approximation: {error}"
                    ));
                    return Ok(Some(timing));
                }
                // The existing bounded-selection fallback below still
                // revalidates coverage/grid before retaining content.
            }
            let (source_in, source_out) = if clip.playback_rate < 0.0 {
                (
                    source
                        .intrinsic_ticks
                        .checked_sub(clip.out_ticks)
                        .ok_or_else(|| {
                            unsupported("reverse source out exceeds intrinsic duration")
                        })?,
                    source
                        .intrinsic_ticks
                        .checked_sub(clip.in_ticks)
                        .ok_or_else(|| {
                            unsupported("reverse source in exceeds intrinsic duration")
                        })?,
                )
            } else {
                (clip.in_ticks, clip.out_ticks)
            };
            let end = source_out.min(source.intrinsic_ticks);
            if source_in < 0 || end <= source_in {
                omit(
                    omissions,
                    OmissionScope::Occurrence,
                    record,
                    format!(
                        "no physical source interval remains after TimeRemapping recovery: {error}"
                    ),
                );
                return Ok(None);
            }
            let ms = crate::schema::TICKS_PER_MILLISECOND;
            let first =
                u64::try_from((i128::from(source_in) + i128::from(ms) - 1) / i128::from(ms))
                    .map_err(|_| unsupported("recovered source selection is negative"))?;
            let last = u64::try_from(end / ms)
                .map_err(|_| unsupported("recovered source selection is negative"))?;
            if last <= first {
                omit(omissions,OmissionScope::Occurrence,record,"no representable physical millisecond span remains after TimeRemapping recovery");
                return Ok(None);
            }
            let mut bounded = clip.clone();
            bounded.time_remap = None;
            bounded.out_ticks = clip.out_ticks.min(source.intrinsic_ticks);
            bounded.playback_rate = ((bounded.out_ticks - bounded.in_ticks) as f64
                / (clip.end_ticks - clip.start_ticks) as f64)
                .copysign(clip.playback_rate);
            if bounded
                .validate_base_on_grid(
                    project.frame_rate,
                    project
                        .native_frame_ticks
                        .unwrap_or(project.frame_rate.ticks_per_frame()),
                    source,
                )
                .is_ok()
            {
                let selection = TimeRangeProperty::new(
                    Time::from_millis(first),
                    Duration::from_millis(last - first),
                );
                approximate(omissions,record,format!("TimeRemapping approximated using bounded authored source trim at constant speed: {error}; picture and other controls retained"));
                return Ok(Some(ClipTiming {
                    active_range,
                    video_range,
                    source_range: selection,
                    source_intrinsic_duration: duration_from_ticks(source.intrinsic_ticks)?,
                    playback: fx_schema::LayerPlayback::remapped(
                        video_range,
                        constant_time_remap(
                            clip.playback_rate.is_sign_negative(),
                            video_range,
                            selection,
                        )?,
                        0,
                    )
                    .map_err(unsupported)?,
                }));
            }
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                format!("clip was not imported: {error}"),
            );
            Ok(None)
        }
    }
}

/// The existing saved constant/reverse clock, with no endpoints inferred from
/// a rejected optional curve. The caller validates native bounds before using
/// this mapping as a fallback.
fn constant_clip_timing(
    clip: &PrVideoOccurrence,
    source: &PrVideoStream,
    active_range: TimeRangeProperty,
    video_range: TimeRangeProperty,
    presentation_origin: Option<crate::media::PresentationOrigin>,
) -> Result<ClipTiming> {
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
    let mut source_range = {
        let mut range = tick_range(source_in, source_out)?;
        if clip.playback_rate == 1.0 {
            range.duration = active_range.duration;
        }
        range
    };
    if let Some(origin) = presentation_origin {
        source_range.start = Time::from_millis(origin.shifted_start(
            source_range.start.as_millis(),
            source_range.duration.as_millis(),
        )?);
    }
    let playback = if clip.playback_rate != 1.0 {
        fx_schema::LayerPlayback::remapped(
            video_range,
            constant_time_remap(
                clip.playback_rate.is_sign_negative(),
                video_range,
                source_range,
            )?,
            0,
        )
    } else {
        fx_schema::LayerPlayback::linear(video_range, video_range, source_range, 0)
    }
    .map_err(unsupported)?;
    Ok(ClipTiming {
        active_range,
        video_range,
        source_range,
        source_intrinsic_duration,
        playback,
    })
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

/// The rectangle of a Crop guide: what `crop` keeps of a picture's own frame
/// of `source` pixels, which Premiere crops before Motion moves the picture.
pub(super) fn crop_rect(crop: &PrStaticCrop, source: [u32; 2]) -> RectShape {
    let [source_width, source_height] = source.map(f64::from);
    let mut rect = black_shape(source[0], source[1]);
    // A hidden owner does not consume its mask guide for painting. Keep the
    // outline available when the owner is re-enabled, but never paint it alone.
    rect.fill_enabled = false;
    rect.position = [
        source_width * crop.left / 100.0,
        source_height * crop.top / 100.0,
    ];
    rect.size = [
        source_width * (100.0 - crop.left - crop.right) / 100.0,
        source_height * (100.0 - crop.top - crop.bottom) / 100.0,
    ];
    rect
}

/// The Add mask that the Crop guide `guide_id` of `crop` shapes. FX masks
/// cannot represent negative feather widths, so the mask keeps the crop
/// geometry, and a feathered Crop is reported as approximated.
fn crop_mask(
    crop: &PrStaticCrop,
    mask_id: FxItemId,
    guide_id: LayerId,
    record: &str,
    omissions: &mut Vec<Omission>,
) -> PathMask {
    if crop.edge_feather != 0.0 {
        approximate(
            omissions,
            record,
            "Crop Edge Feather is approximated by an FX mask; Premiere feather visuals are not preserved exactly",
        );
    }
    guide_mask(mask_id, guide_id, crop.edge_feather.max(0.0))
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
pub(super) fn shape_guide(
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

/// The FX mask of a clip's Opacity mask `mask`, shaped by the guide
/// `guide_id`, and the guide's outline: the mask's outline, which the corpus
/// stores in unit fractions of the source frame, in the pixels of that
/// `frame`.
pub(super) fn opacity_mask(
    mask: &PrMask,
    mask_id: FxItemId,
    guide_id: LayerId,
    frame: [u32; 2],
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Result<(PathMask, ShapePath)> {
    ensure!(
        mask.raster.is_none(),
        "Object Mask raster requires source-bound physical-video recovery"
    );
    for warning in mask.approximations() {
        approximate(omissions, record, warning);
    }
    let path_mask = opacity_path_mask(mask_id, guide_id, mask)?;
    Ok((
        path_mask,
        scaled_path(&fx_path(&mask.path), frame.map(f64::from)),
    ))
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

/// The Add mask of a clip's Opacity `mask` over its guide `guide_id`, with
/// its Feather, Mask Opacity and Inverted.
pub(super) fn opacity_path_mask(
    id: FxItemId,
    guide_id: LayerId,
    mask: &PrMask,
) -> Result<PathMask> {
    let mut path_mask = guide_mask(id, guide_id, mask.feather);
    path_mask.inverted = mask.inverted;
    path_mask.expansion = mask.expansion;
    path_mask.opacity = NonNegativeProperty::new(mask.opacity / 100.0)
        .ok_or_else(|| unsupported("Mask Opacity must be between 0 and 100"))?;
    Ok(path_mask)
}

/// Why the Mask Path keys of `clip` cannot import as its guide's outline keys
/// ([`mask_path_track`]), if they cannot. Premiere keys a Mask Path on the
/// clip's source clock and the guide counts its keys from the clip start.
/// Subtracting the source In preserves a trim, including keys before the
/// visible window. A changed playback speed needs another clock mapping.
fn mask_path_keys_reason(clip: &PrVideoOccurrence) -> Option<&'static str> {
    if clip.playback_rate != 1.0 || clip.time_remap.is_some() {
        Some("they are on the source clock, which a retimed, reversed, held or time-remapped clip does not play at unit speed from its start")
    } else {
        None
    }
}

/// The `ShapePath` track of the guide `guide_id` of the keyed outline of
/// `mask`: one key per native key, drawn in the source `frame`'s pixels as
/// the static guide is, on the layer clock from `source_in`, as Motion keys
/// are, each eased as Premiere draws the interval before it
/// ([`mask_key_easing`]).
fn mask_path_track(
    mask: &PrMask,
    guide_id: LayerId,
    source_in: i64,
    frame: [u32; 2],
) -> std::result::Result<PropertyKeyframeTrack, String> {
    let paths: Vec<_> = mask
        .path_keys
        .iter()
        .map(|key| scaled_path(&fx_path(&key.path), frame.map(f64::from)))
        .collect();
    let mut easings = vec![PropertyKeyframeEasing::Linear];
    for (keys, paths) in mask.path_keys.windows(2).zip(paths.windows(2)) {
        easings.push(
            mask_key_easing((&keys[0].path, &paths[0]), (&keys[1].path, &paths[1])).map_err(
                |reason| {
                    format!(
                        "the Mask Path keys at source ticks {} and {} {reason}",
                        keys[0].source_ticks, keys[1].source_ticks
                    )
                },
            )?,
        );
    }
    let keys = mask
        .path_keys
        .iter()
        .zip(paths)
        .zip(easings)
        .enumerate()
        .map(|(index, ((key, path), easing))| {
            let millis = keyframes::layer_millis(key.source_ticks, source_in)
                .map_err(|error| error.to_string())?;
            Ok(PropertyKeyframe::new(
                keyframe_id(guide_id, "mask-path", index),
                TimeOffset::from_millis(millis),
                PropertyValue::Path(path),
                easing,
            ))
        })
        .collect::<std::result::Result<Vec<_>, String>>()?;
    PropertyKeyframeTrack::new(keys)
        .map_err(|error| format!("its keyframe times/values cannot be imported: {error}"))
}

/// The FX easing of the mask outline key `to`, for the interval after the
/// key `from`, each given as its Premiere outline and FX path, that draws
/// what Premiere draws there ([`crate::schema::PrMaskPathKey`]), or why FX
/// cannot: Hold between outlines whose vertex counts differ, which Premiere
/// holds (FX then shows the later outline from its key, where Premiere's
/// switch is inferred), and Linear between outlines of one count, which
/// Premiere moves vertex by vertex and FX command by command, only when their
/// command kinds match; otherwise FX resamples both by arclength
/// (`interpolate_path` in `fx_composition`). Both directions use this one
/// rule.
pub(super) fn mask_key_easing(
    (from_outline, from_path): (&PrShapePath, &ShapePath),
    (to_outline, to_path): (&PrShapePath, &ShapePath),
) -> std::result::Result<PropertyKeyframeEasing, &'static str> {
    if from_outline.vertices.len() != to_outline.vertices.len() {
        return Ok(PropertyKeyframeEasing::Hold);
    }
    let kinds = |path: &ShapePath| {
        path.commands
            .iter()
            .map(std::mem::discriminant)
            .collect::<Vec<_>>()
    };
    if kinds(from_path) == kinds(to_path) {
        Ok(PropertyKeyframeEasing::Linear)
    } else {
        Err("draw a segment straight at one key and curved at the other; FX would resample the outline between them")
    }
}

/// Reports each font of the converted text once. The document packages media
/// only, so `tsrct` needs every font imported before it renders the text.
fn report_unpackaged_fonts(project: &PrSequence, omissions: &mut Vec<Omission>) {
    // `video_items` order is layer order: bottom track first, then timeline order.
    let mut uses: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for (index, item) in project.video_items().enumerate() {
        let Some(graphic) = item.graphic() else {
            continue;
        };
        for (name, document) in graphic.text_documents() {
            // An empty Text saved without a font takes [`EMPTY_TEXT_FONT`].
            let font = match document.font.as_str() {
                "" => EMPTY_TEXT_FONT.join(" "),
                font => font.to_owned(),
            };
            let (_, count) = uses.entry(font).or_insert_with(|| {
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
            fonts::not_packaged(&font, "preview or export"),
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
    if tracks.is_empty() {
        return Ok(());
    }
    if tracks.len() == 1 {
        let (property, track) = tracks
            .into_iter()
            .next()
            .expect("one animation track was checked above");
        return dynamics
            .set_property(property, PropertyAnimator::keyframes(track), Vec::new())
            .map_err(map_animation_graph_error);
    }
    let mut raw = dynamics.wire_value().clone();
    if raw.get("entries").is_none() {
        raw["entries"] = serde_json::Value::Array(Vec::new());
    }
    let mut entries = dynamics.entries().to_vec();
    let mut inserted = false;

    for (property, track) in tracks {
        let target = fx_schema::PropertyTarget::from(property);
        let entry = AnimationGraphEntry {
            target: target.clone(),
            animator: PropertyAnimator::keyframes(track),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: LayerRefMap::default(),
        };
        let value = serde_json::to_value(&entry)
            .map_err(|error| AnimationGraphError::Wire(error.to_string()))
            .map_err(map_animation_graph_error)?;
        let mut next_entries = entries.clone();
        let index = next_entries.iter().position(|entry| entry.target == target);
        if let Some(index) = index {
            next_entries[index] = entry;
        } else {
            next_entries.push(entry);
        }

        // Validate every intermediate state, as repeated `set_property` did. On
        // failure, commit only the preceding valid insertions below, preserving
        // its first-error and partial-success behavior.
        let validated = match AnimationGraph::from_entries(next_entries) {
            Ok(validated) => validated,
            Err(error) => {
                if inserted {
                    let committed: AnimationGraph = serde_json::from_value(raw)
                        .map_err(|source| AnimationGraphError::Wire(source.to_string()))
                        .map_err(map_animation_graph_error)?;
                    *dynamics = committed;
                }
                return Err(map_animation_graph_error(error));
            }
        };
        entries = validated.entries().to_vec();
        let raw_entries = raw["entries"]
            .as_array_mut()
            .expect("checked graph entries are an array");
        if let Some(index) = index {
            raw_entries[index] = value;
        } else {
            raw_entries.push(value);
        }
        inserted = true;
    }

    *dynamics = serde_json::from_value(raw)
        .map_err(|error| AnimationGraphError::Wire(error.to_string()))
        .map_err(map_animation_graph_error)?;
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
/// with an empty style; no font catalog is consulted. An empty Text saved
/// without a font takes [`EMPTY_TEXT_FONT`].
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
    let [font_family, font_style] = if doc.font.is_empty() {
        EMPTY_TEXT_FONT
    } else {
        [doc.font.as_str(), ""]
    };
    let source_text = TextDocument {
        text: doc.text.clone(),
        font_family: Arc::from(font_family),
        font_style: Arc::from(font_style),
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
    if let Some(horizontal) = text.horizontal_scale {
        transform.scale[0] = horizontal;
    }
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
#[path = "tests/set_tracks.rs"]
mod set_tracks_tests;
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
