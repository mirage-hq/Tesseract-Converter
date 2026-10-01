//! Editable geometry for three measured opaque neutral Stroke profiles.
use super::{identity_transform, plain_group, LayerScope};
use crate::{
    convert::timing::relocate_playback,
    error::{ensure, Result},
    schema::{
        PrAnimatedProperty, PrBlendMode, PrFilmImpactStroke, PrMediaKind, PrVideoOccurrence,
        PrVideoStream,
    },
};
use fx_schema::{
    BlendMode, EffectData, EffectPayload, EffectRecord, GroupLayer, Layer, LayerData, LayerEffect,
    LayerId, LayerStrokePosition, NonNegativeProperty, Position, RectLayer, StrokeOutlineStyle,
    Time, TimeRangeProperty, VideoLayer,
};

pub(super) fn validate(
    clip: &PrVideoOccurrence,
    source: &PrVideoStream,
    scope: &LayerScope<'_, '_, '_>,
) -> Result<()> {
    // File admission accepts only opaque H.264 or HEVC Main/Main10 video.
    // Still/alpha sources, linked compositions and generators cannot enter here.
    ensure!(matches!(source.kind, PrMediaKind::Video { .. })
        && source.width > 0 && source.height > 0
        && scope.parent.is_none() && scope.on_document_clock
        && clip.blend_mode == PrBlendMode::Normal
        && clip.playback_rate == 1.0 && clip.time_remap.is_none()
        && clip.frame_blending.is_none()
        && clip.crop.is_default() && clip.linear_wipe.is_none()
        && clip.opacity_mask.is_none() && clip.track_matte.is_none()
        && clip.active_transforms == 0 && clip.effects.is_empty()
        && clip.transform.scale[0] == clip.transform.scale[1]
        && clip.transform.scale[0].is_finite()
            && clip.transform.scale[0] > 0.0
        && clip.animations.iter().all(|a| a.property() == PrAnimatedProperty::Opacity),
        "Film Impact Stroke requires an opaque physical picture with static uniform Motion on the root clock, unit playback and no crop, mask, other effects or frame blending");
    Ok(())
}

pub(super) fn wrap(
    mut video: VideoLayer,
    profile: PrFilmImpactStroke,
    source: &PrVideoStream,
    scope: &mut LayerScope<'_, '_, '_>,
) -> Result<Layer> {
    let owner = video.id;
    let transform = video.transform;
    let active_range = video.playback.input_range();
    let is_hidden = video.is_hidden;
    let parent = video.parent;
    let local_range = TimeRangeProperty::new(Time::ZERO, active_range.duration);
    let video_id = LayerId::new(*scope.next_index as u64 + 1);
    *scope.next_index += 1;
    let prescale = match profile {
        PrFilmImpactStroke::Outline100 => 100.0,
        _ => 99.0,
    };
    video.id = video_id;
    video.parent = Some(owner);
    video.playback = relocate_playback(&video.playback, local_range)?;
    video.is_hidden = false;
    video.transform = identity_transform();
    video.transform.anchor_point = [
        f64::from(source.width) * 0.5,
        f64::from(source.height) * 0.5,
    ];
    video.transform.position = Position::xy(
        video.transform.anchor_point[0],
        video.transform.anchor_point[1],
    );
    video.transform.scale = [prescale; 2];
    // The source range/intrinsic duration, muted audio, and source asset stay exact.
    let mut children = Vec::with_capacity(2);
    if profile == PrFilmImpactStroke::Frame99 {
        let rect_id = LayerId::new(*scope.next_index as u64 + 1);
        *scope.next_index += 1;
        let mut rect = super::black_shape(source.width, source.height);
        rect.fill_color = [1.0; 4];
        children.push(Layer::from_data(&LayerData::Rect(RectLayer {
            id: rect_id,
            name: "Premiere Stroke original-bounds border".into(),
            description: "Measured neutral 66/99 geometry approximation".into(),
            is_hidden: false,
            parent: Some(owner),
            blend_mode: BlendMode::Normal,
            track_matte: None,
            masks: Vec::new(),
            effects: Vec::new(),
            motion_blur: false,
            active_range: local_range,
            transform: identity_transform(),
            rect,
        }))?);
    } else {
        let width = NonNegativeProperty::new(0.06 * transform.scale[0])
            .expect("positive finite static Motion was validated");
        video
            .effects
            .push(EffectRecord::from_data(&EffectData::Identified {
                id: scope.effect_ids.take(),
                enabled: true,
                effect: EffectPayload::Known(LayerEffect::Stroke(StrokeOutlineStyle::new(
                    [1.0; 4],
                    width,
                    LayerStrokePosition::Outside,
                ))),
            })?);
    }
    // FX child lists are top-to-bottom: picture first, backplate beneath it.
    children.insert(0, Layer::from_data(&LayerData::Video(video))?);
    Layer::from_data(&LayerData::Group(GroupLayer {
        is_hidden,
        parent,
        ..plain_group(
            owner,
            "Premiere Stroke picture".into(),
            active_range,
            transform,
            children,
        )?
    }))
    .map_err(Into::into)
}
