//! Bounded RGB isolation using public Levels, ShiftChannels, Screen/Divide and Alpha matte.
use super::{
    background::{black_shape, identity_transform, plain_group},
    effects::EffectIdAllocator,
    premiere_to_tesseract::guide_layer,
};
use crate::{
    approximate,
    error::{unsupported, Result},
    omit,
    schema::{PrBlendMode, PrEffectParams, PrLevels, PrVideoOccurrence},
    Omission, OmissionScope,
};
use fx_schema::effect::ChannelSource;
use fx_schema::{
    AnimationGraph, BlendMode, EffectData, EffectPayload, EffectRecord, Layer, LayerData,
    LayerEffect, LayerId, LayerPlayback, LinearGain, PropertyTarget, TrackMatte, TrackMatteType,
};

fn picture_effects(layer: &mut LayerData) -> &mut Vec<EffectRecord> {
    match layer {
        LayerData::Video(v) => &mut v.effects,
        LayerData::Image(v) => &mut v.effects,
        _ => unreachable!("checked ordinary picture"),
    }
}
fn place_silent_picture(layer: &mut LayerData, id: LayerId, parent: LayerId, blend: BlendMode) {
    match layer {
        LayerData::Video(v) => {
            v.id = id;
            v.parent = Some(parent);
            v.blend_mode = blend;
            v.volume = Some(LinearGain::ZERO);
        }
        LayerData::Image(v) => {
            v.id = id;
            v.parent = Some(parent);
            v.blend_mode = blend;
        }
        _ => unreachable!("checked ordinary picture"),
    }
}
fn record(effect: LayerEffect, ids: &mut EffectIdAllocator) -> Result<EffectRecord> {
    Ok(EffectRecord::from_data(&EffectData::Identified {
        id: ids.take(),
        enabled: true,
        compositing_options: None,
        extensions: Default::default(),
        effect: EffectPayload::Known(effect),
    })?)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one fresh occurrence, shared ids and its native-key graph"
)]
pub(super) fn lower(
    clip: &PrVideoOccurrence,
    canvas: [u32; 2],
    frame: [u32; 2],
    picture: Layer,
    next: &mut usize,
    ids: &mut EffectIdAllocator,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Layer> {
    let Some(correction) = clip
        .effects
        .iter()
        .find(|e| matches!(e.params, PrEffectParams::Levels(PrLevels::Corrections(_))))
    else {
        return Ok(picture);
    };
    if !clip.enabled {
        omit(omissions,OmissionScope::Feature,clip.record(),"RGB Levels channel correction omitted on a disabled picture; original master, source and independent audio retained without helper graphs");
        return Ok(picture);
    }
    let (range, parent) = match picture.data() {
        LayerData::Video(v)
            if !v.is_hidden
                && v.masks.is_empty()
                && v.track_matte.is_none()
                && v.transform.scale == [100.0, 100.0] =>
        {
            (Some(v.playback.input_range()), v.parent)
        }
        LayerData::Image(v)
            if !v.is_hidden
                && v.masks.is_empty()
                && v.track_matte.is_none()
                && v.transform.scale == [100.0, 100.0] =>
        {
            (Some(v.active_range), v.parent)
        }
        _ => (None, None),
    };
    if range.is_none()
        || frame != canvas
        || clip.effects.len() != 2
        || !matches!(
            clip.effects[0].params,
            PrEffectParams::Levels(PrLevels::Master { .. })
        )
        || clip.effects.iter().any(|e| e.mask.is_some())
        || clip.source_effects.is_some()
        || !clip.animations.is_empty()
        || clip.opacity != 100.0
        || !clip.transform.is_identity_on_canvas()
        || clip.blend_mode != PrBlendMode::Normal
        || !clip.crop.is_default()
        || clip.opacity_mask.is_some()
        || clip.track_matte.is_some()
    {
        omit(omissions,OmissionScope::Feature,clip.record(),"RGB Levels branches require one unmasked Levels component on an ordinary canvas-sized picture at static identity Motion/100% opacity and native Normal blend; master, siblings, audio and original picture retained without channel correction");
        return Ok(picture);
    }
    let range = range.expect("checked range");
    let PrEffectParams::Levels(PrLevels::Corrections(rows)) = correction.params else {
        unreachable!()
    };
    let mut allocate = || {
        *next += 1;
        LayerId::new(*next as u64)
    };
    let wrapper_id = allocate();
    let carrier_id = allocate();
    let sample_id = allocate();
    let backing_id = allocate();
    let sum_id = allocate();
    let alpha_rgb_id = allocate();
    let white_id = allocate();
    let alpha_backing_id = allocate();
    let original = picture.data().clone();
    let original_id = picture.id();
    let mut sample = original.clone();
    place_silent_picture(&mut sample, sample_id, wrapper_id, BlendMode::Normal);
    picture_effects(&mut sample).clear();
    let mut children = Vec::new();
    for (channel, row) in rows.into_iter().enumerate() {
        let mut branch = original.clone();
        let id = if channel == 0 {
            original_id
        } else {
            allocate()
        };
        place_silent_picture(&mut branch, id, sum_id, BlendMode::Screen);
        // Imported master keyframes belong to each picture's unchanged clock.
        // Copies receive new effect identities, never shared mutable graph ids.
        if channel != 0 {
            for effect in picture_effects(&mut branch) {
                let EffectData::Identified {
                    id: old,
                    enabled,
                    effect: payload,
                    compositing_options,
                    extensions,
                } = effect.data()
                else {
                    unreachable!("fresh effects have ids")
                };
                let new = ids.take();
                let entries: Vec<_> = dynamics
                    .entries()
                    .iter()
                    .filter(|e| e.target.effect_id() == Some(*old))
                    .cloned()
                    .collect();
                for entry in entries {
                    let PropertyTarget::EffectProperty(target) = entry.target else {
                        unreachable!()
                    };
                    dynamics
                        .set_property(
                            PropertyTarget::effect_param(new, target.param_name()),
                            {
                                let mut animator = entry.animator.data().clone();
                                if let fx_schema::animator::AnimatorData::Keyframes {
                                    track, ..
                                } = &mut animator
                                {
                                    *track = fx_schema::animator::PropertyKeyframeTrack::new(
                                        track
                                            .keyframes()
                                            .iter()
                                            .map(|key| {
                                                fx_schema::animator::PropertyKeyframe::new(
                                                    fx_schema::KeyframeId::new(format!(
                                                        "channel-levels-{}-{}",
                                                        new.value(),
                                                        key.id().as_str()
                                                    )),
                                                    key.layer_time(),
                                                    key.value().clone(),
                                                    key.easing(),
                                                )
                                            })
                                            .collect(),
                                    )
                                    .map_err(|e| unsupported(e.to_string()))?;
                                }
                                fx_schema::PropertyAnimator::from_data(&animator)?
                            },
                            Vec::new(),
                        )
                        .map_err(|e| unsupported(e.to_string()))?;
                }
                *effect = EffectRecord::from_data(&EffectData::Identified {
                    id: new,
                    enabled: *enabled,
                    effect: payload.clone(),
                    compositing_options: compositing_options.clone(),
                    extensions: extensions.clone(),
                })?;
            }
        }
        picture_effects(&mut branch).push(EffectRecord::from_data(&EffectData::Identified {
            id: ids.take(),
            enabled: correction.enabled,
            compositing_options: None,
            extensions: Default::default(),
            effect: EffectPayload::Known(LayerEffect::Levels {
                input_black: row[0],
                input_white: row[1],
                output_black: row[2],
                output_white: row[3],
                gamma: row[4] / 100.0,
            }),
        })?);
        let mut selectors = [ChannelSource::FullOff; 3];
        selectors[channel] = [
            ChannelSource::Red,
            ChannelSource::Green,
            ChannelSource::Blue,
        ][channel];
        picture_effects(&mut branch).push(record(
            LayerEffect::ShiftChannels {
                take_red_from: selectors[0],
                take_green_from: selectors[1],
                take_blue_from: selectors[2],
            },
            ids,
        )?);
        children.push(Layer::from_data(&branch)?);
    }
    let backing = guide_layer(
        backing_id,
        "RGB assembly black backing".into(),
        Some(sum_id),
        range,
        identity_transform(),
        black_shape(canvas[0], canvas[1]),
    );
    children.push(Layer::from_data(&LayerData::Rect(backing))?);
    let mut sum = plain_group(
        sum_id,
        "Isolated RGB sum".into(),
        range,
        identity_transform(),
        children,
    )?;
    sum.parent = Some(carrier_id);
    sum.playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
    // Disjoint own-channel RGB makes Screen cross-products zero: RGB is A*C.
    // Screen source-over alpha keeps the opaque backing at 1 even on float
    // targets; Add instead accumulates 1+3A and corrupts the following Divide.
    // Sum is opaque with RGB A*C. White-over-black is opaque with RGB A.
    // Divide restores C before the original Alpha gate restores A. At A=0,
    // Divide yields white, but the final zero Alpha gate removes it entirely.
    let mut white = original.clone();
    place_silent_picture(&mut white, white_id, alpha_rgb_id, BlendMode::Normal);
    picture_effects(&mut white).clear();
    picture_effects(&mut white).push(record(
        LayerEffect::ShiftChannels {
            take_red_from: ChannelSource::FullOn,
            take_green_from: ChannelSource::FullOn,
            take_blue_from: ChannelSource::FullOn,
        },
        ids,
    )?);
    let alpha_backing = guide_layer(
        alpha_backing_id,
        "Alpha-to-RGB black backing".into(),
        Some(alpha_rgb_id),
        range,
        identity_transform(),
        black_shape(canvas[0], canvas[1]),
    );
    let mut alpha_rgb = plain_group(
        alpha_rgb_id,
        "Alpha divisor".into(),
        range,
        identity_transform(),
        vec![
            Layer::from_data(&white)?,
            Layer::from_data(&LayerData::Rect(alpha_backing))?,
        ],
    )?;
    alpha_rgb.parent = Some(carrier_id);
    alpha_rgb.blend_mode = BlendMode::Divide;
    alpha_rgb.playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
    let mut carrier = plain_group(
        carrier_id,
        "RGB Levels assembly".into(),
        range,
        identity_transform(),
        vec![
            Layer::from_data(&LayerData::Group(alpha_rgb))?,
            Layer::from_data(&LayerData::Group(sum))?,
        ],
    )?;
    carrier.parent = Some(wrapper_id);
    carrier.playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
    carrier.track_matte = Some(TrackMatte {
        layer: sample_id,
        mode: TrackMatteType::Alpha,
    });
    let mut wrapper = plain_group(
        wrapper_id,
        "Premiere channel Levels".into(),
        range,
        identity_transform(),
        vec![
            Layer::from_data(&LayerData::Group(carrier))?,
            Layer::from_data(&sample)?,
        ],
    )?;
    wrapper.parent = parent;
    wrapper.playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
    approximate(omissions,clip.record(),"RGB Levels uses independent editable master-then-channel Levels/own-channel ShiftChannels branches, Screen over opaque black, a white-source-over-black Alpha divisor and an original Alpha gate. Divide recovers straight RGB before restoring source alpha; zero alpha remains transparent. Blend precision, clipping and native processing differences remain approximations. Master keys and source clocks are copied independently, not live-linked. Current native/linked export uses graph contents; native equality is unmeasured");
    Ok(Layer::from_data(&LayerData::Group(wrapper))?)
}
