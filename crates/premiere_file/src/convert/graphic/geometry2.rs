//! Leading graphic affine effects retain independent editable content stages.
//! Native chain order is outermost first; Vector Motion stays outside them.
use super::{next_layer_id, plain_group, LayerScope};
use crate::{
    approximate,
    error::Result,
    schema::{PrEffectParams, PrGraphic},
    Omission,
};
use fx_schema::{
    AnimationGraph, Layer, LayerData, LayerId, PropertyAnimator, Time, TimeRangeProperty,
};

pub(super) fn stage_ids(graphic: &PrGraphic, scope: &mut LayerScope<'_, '_>) -> Vec<LayerId> {
    let Some(loss) = &graphic.effect_loss else {
        return Vec::new();
    };
    if !loss
        .mapped_effects
        .iter()
        .any(|effect| matches!(effect.params, PrEffectParams::Transform(_)))
    {
        return Vec::new();
    }
    loss.mapped_effects
        .iter()
        .map(|_| next_layer_id(scope))
        .collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "graphic stage lowering shares the occurrence clock and allocation scope"
)]
pub(super) fn import_stages(
    graphic: &PrGraphic,
    dimensions: [u32; 2],
    outer: LayerId,
    ids: &[LayerId],
    mut layers: Vec<Layer>,
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Vec<Layer>> {
    if ids.is_empty() {
        return Ok(layers);
    }
    let Some(loss) = &graphic.effect_loss else {
        return Ok(layers);
    };
    let record = graphic.id().unwrap_or("graphic");
    let window = super::tick_range(graphic.start_ticks, graphic.end_ticks)?;
    let local = TimeRangeProperty::new(Time::ZERO, window.duration);
    for (index, (effect, id)) in loss.mapped_effects.iter().zip(ids).enumerate().rev() {
        let mut stage = plain_group(
            *id,
            "Premiere graphic effect stage".into(),
            local,
            super::identity_transform(),
            layers,
        )?;
        stage.parent = Some(if index == 0 { outer } else { ids[index - 1] });
        if let PrEffectParams::Transform(transform) = &effect.params {
            // read_object omits bypassed Geometry2 before adding mapped_effects;
            // only active affine controls reach this lowering.
            stage.transform =
                super::super::premiere_to_tesseract::staged_video_transform(transform, dimensions)?;
            let tracks = super::super::premiere_to_tesseract::transform_stage_tracks(
                effect,
                transform,
                dimensions,
                graphic.in_ticks,
                *id,
            )?;
            super::set_tracks(dynamics, tracks)?;
            // This host omits blur instead of setting a global FX shutter.
            let without_blur = crate::schema::PrTransform {
                shutter_angle: 0.0,
                composition_shutter_angle: true,
                ..*transform
            };
            for warning in without_blur.approximations(&effect.animations) {
                approximate(omissions, record, warning);
            }
            if let Some(warning) = transform.unretained_skew_axis() {
                approximate(omissions, record, warning);
            }
            if let Some(warning) = transform.unimported_scale_width_keys(&effect.animations) {
                approximate(omissions, record, warning);
            }
            if let Some(warning) =
                super::super::premiere_to_tesseract::curved_transform_position_approximation(effect)
            {
                approximate(omissions, record, warning);
            }
            // No new composition-wide shutter policy is inferred from a graphic.
            if transform.shutter_angle > 0.0
                || effect.animations.iter().any(|animation| {
                    animation.param.id == crate::schema::TRANSFORM_SHUTTER_ANGLE.id
                })
            {
                approximate(omissions, record, "graphic Geometry2 motion blur omitted; editable affine controls retained without native shutter sampling");
            }
            approximate(omissions, record, "graphic Geometry2 retained as an editable source-frame affine Group before Vector Motion and clip Motion/Opacity; native generator clipping, raster sampling and alpha coverage remain unverified; a completely mapped affine chain retains external Track Matte Key coverage as an approximation, while object/clip masks and incomplete or pixel-effect provider chains remain unsupported");
        } else {
            let (effects, tracks) = super::super::effects::import_graphic_effects(
                graphic,
                std::slice::from_ref(effect),
                *id,
                scope.effect_ids,
                omissions,
            );
            stage.effects = effects;
            for (target, track) in tracks {
                dynamics
                    .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
                    .map_err(super::super::premiere_to_tesseract::map_animation_graph_error)?;
            }
        }
        layers = vec![Layer::from_data(&LayerData::Group(stage))?];
    }
    Ok(layers)
}
