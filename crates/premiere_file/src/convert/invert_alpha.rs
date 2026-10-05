//! Premiere Alpha15 is replacement of coverage, not RGB Levels or A*(1-A).
use super::{
    background::{black_shape, identity_transform, plain_group},
    premiere_to_tesseract::guide_layer,
};
use crate::{
    approximate,
    error::{unsupported, Result},
    omit,
    schema::{PrBlendMode, PrEffectParams, PrInvert, PrVideoOccurrence},
    Omission, OmissionScope,
};
use fx_schema::{Layer, LayerData, LayerId, LayerPlayback, LinearGain, TrackMatte, TrackMatteType};

pub(super) fn lower(
    clip: &PrVideoOccurrence,
    canvas: [u32; 2],
    frame: [u32; 2],
    picture: Layer,
    next: &mut usize,
    omissions: &mut Vec<Omission>,
) -> Result<Layer> {
    lower_with_sample(clip, canvas, frame, picture, None, next, omissions)
}

/// The linked importer supplies a second independently allocated source graph.
/// Never clone only the root ID of an imported composition subtree.
pub(super) fn lower_with_sample(
    clip: &PrVideoOccurrence,
    canvas: [u32; 2],
    frame: [u32; 2],
    picture: Layer,
    prepared_sample: Option<Layer>,
    next: &mut usize,
    omissions: &mut Vec<Omission>,
) -> Result<Layer> {
    let Some(alpha) = clip.effects.iter().find(|e| {
        e.enabled
            && matches!(
                e.params,
                PrEffectParams::Invert(PrInvert { channel: 15, .. })
            )
    }) else {
        return Ok(picture);
    };
    if !alpha.enabled || !clip.enabled {
        return Ok(picture);
    }
    let bounds = match picture.data() {
        LayerData::Video(v)
            if !v.is_hidden
                && v.masks.is_empty()
                && v.track_matte.is_none()
                && v.transform.scale == [100.0, 100.0] =>
        {
            Some(v.playback.input_range())
        }
        LayerData::Image(v)
            if !v.is_hidden
                && v.masks.is_empty()
                && v.track_matte.is_none()
                && v.transform.scale == [100.0, 100.0] =>
        {
            Some(v.active_range)
        }
        LayerData::Group(v)
            if prepared_sample.is_some()
                && !v.is_hidden
                && v.masks.is_empty()
                && v.track_matte.is_none()
                && v.effects.is_empty()
                && v.transform.scale == [100.0, 100.0] =>
        {
            Some(v.playback.input_range())
        }
        _ => None,
    };
    let supported = matches!(
        alpha.params,
        PrEffectParams::Invert(PrInvert { blend: 0.0, .. })
    ) && frame == canvas
        && clip.effects.len() == 1
        && alpha.mask.is_none()
        && alpha.animations.is_empty()
        && clip.animations.is_empty()
        && clip.source_effects.is_none()
        && clip.opacity == 100.0
        && clip.transform.is_identity_on_canvas()
        && clip.blend_mode == PrBlendMode::Normal
        && clip.crop.is_default()
        && clip.opacity_mask.is_none()
        && clip.track_matte.is_none()
        && bounds.is_some();
    if !supported {
        omit(omissions,OmissionScope::Feature,clip.record(),"Invert Alpha requires an isolated static full-strength Normal picture (ordinary media or independently imported linked source) matching the canvas at identity Motion/100% opacity, without masks, matte, source effects or owner animation; linked inputs also require unit-forward playback without authored remap; original picture, sound and transfer retained");
        return Ok(picture);
    }
    let range = bounds.expect("checked picture range");
    let mut allocate = || {
        *next += 1;
        LayerId::new(*next as u64)
    };
    let wrapper_id = allocate();
    let carrier_id = allocate();
    let sample_id = prepared_sample
        .as_ref()
        .map_or_else(&mut allocate, Layer::id);
    let backing_id = allocate();
    let mut original = picture.data().clone();
    let mut sample = prepared_sample.map_or_else(|| original.clone(), |layer| layer.data().clone());
    let parent = match &mut original {
        LayerData::Video(v) => {
            let parent = v.parent;
            v.parent = Some(carrier_id);
            parent
        }
        LayerData::Image(v) => {
            let parent = v.parent;
            v.parent = Some(carrier_id);
            parent
        }
        LayerData::Group(v) => {
            let parent = v.parent;
            v.parent = Some(carrier_id);
            parent
        }
        _ => unreachable!("admitted picture"),
    };
    match &mut sample {
        LayerData::Video(v) => {
            v.id = sample_id;
            v.parent = Some(wrapper_id);
            v.volume = Some(LinearGain::ZERO);
        }
        LayerData::Image(v) => {
            v.id = sample_id;
            v.parent = Some(wrapper_id);
        }
        LayerData::Group(v) => {
            // Keep every allocated identity/reference and authored source blend.
            v.parent = Some(wrapper_id);
        }
        _ => unreachable!("admitted picture"),
    }
    let backing = guide_layer(
        backing_id,
        "Opaque Alpha replacement backing".into(),
        Some(carrier_id),
        range,
        identity_transform(),
        black_shape(canvas[0], canvas[1]),
    );
    let mut carrier = plain_group(
        carrier_id,
        "Opaque Alpha carrier".into(),
        range,
        identity_transform(),
        vec![
            Layer::from_data(&original)?,
            Layer::from_data(&LayerData::Rect(backing))?,
        ],
    )?;
    // Identity propagation preserves the picture/sample's source clocks exactly once.
    carrier.playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
    carrier.parent = Some(wrapper_id);
    carrier.track_matte = Some(TrackMatte {
        layer: sample_id,
        mode: TrackMatteType::AlphaInverted,
    });
    let mut wrapper = plain_group(
        wrapper_id,
        "Premiere Invert Alpha".into(),
        range,
        identity_transform(),
        vec![
            Layer::from_data(&LayerData::Group(carrier))?,
            Layer::from_data(&sample)?,
        ],
    )?;
    wrapper.playback = LayerPlayback::linear(range, range, range, 0).map_err(unsupported)?;
    wrapper.parent = parent;
    approximate(omissions,clip.record(),"Invert Alpha uses an opaque black-backed picture and consumed independent AlphaInverted sample: layer-local coverage is 1-A, not A*(1-A). Partial-alpha RGB darkens, unavailable hidden RGB is black, copies are independently editable; source-domain/canvas and edited-export pixel fidelity are unmeasured");
    Layer::from_data(&LayerData::Group(wrapper)).map_err(Into::into)
}

/// Only the existing static owner profile, with unit forward linked playback.
pub(super) fn admits_linked(clip: &PrVideoOccurrence, canvas: [u32; 2], frame: [u32; 2]) -> bool {
    clip.enabled
        && canvas == frame
        && clip.effects.len() == 1
        && clip.effects.iter().all(|e| {
            e.enabled
                && matches!(
                    e.params,
                    PrEffectParams::Invert(PrInvert {
                        channel: 15,
                        blend: 0.0
                    })
                )
                && e.mask.is_none()
                && e.animations.is_empty()
        })
        && clip.animations.is_empty()
        && clip.source_effects.is_none()
        && clip.opacity == 100.0
        && clip.transform.is_identity_on_canvas()
        && clip.blend_mode == PrBlendMode::Normal
        && clip.crop.is_default()
        && clip.opacity_mask.is_none()
        && clip.track_matte.is_none()
        && clip.time_remap.is_none()
        && clip.playback_rate == 1.0
}
