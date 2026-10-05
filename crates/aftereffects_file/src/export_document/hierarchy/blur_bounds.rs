//! Source-local support for the static, transparent-edge native Shape blur.
//! This does not certify Gaussian consumer-demand cropping or other effects.

use fx_schema::{
    EffectData, EffectPayload, EffectRecord, LayerEffect, Position, PropertyTarget, ShapeLayer,
};

use crate::export_document::AnimationIndex;

pub(super) fn has_dynamics(records: &[EffectRecord], dynamics: &AnimationIndex<'_>) -> bool {
    records.iter().any(|record| {
        matches!(record.data(), EffectData::Identified { id, enabled: true, effect: EffectPayload::Known(LayerEffect::GaussianBlur { .. }) }
            if dynamics.iter().any(|entry| matches!(&entry.target,
                PropertyTarget::EffectProperty(target) if target.effect_id() == *id)))
    })
}

pub(super) fn shape_reach(shape: &ShapeLayer, dynamics: &AnimationIndex<'_>) -> f64 {
    let transform = &shape.transform;
    // Native Shape continuous rasterization can change the effect/transform
    // order. The independent control proves only a static translation-only
    // plane, not scaled, rotated, projective or animator-owned Shape transforms.
    if !matches!(transform.position, Position::TwoD(_))
        || transform.scale != [100.0; 2]
        || transform.rotation != 0.0
        || transform.skew != 0.0
        || transform.skew_axis != 0.0
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
        || dynamics.for_layer(shape.id).next().is_some()
    {
        return 0.0;
    }
    let mut reach = None;
    for record in &shape.effects {
        let payload = match record.data() {
            EffectData::Identified { enabled: false, .. } => continue,
            EffectData::Identified { id, effect, .. } => {
                // Dynamic/disabled animator semantics retain the existing
                // content-only approximation, never a new finite certificate.
                if dynamics.iter().any(|entry| {
                    matches!(&entry.target,
                    PropertyTarget::EffectProperty(target) if target.effect_id() == *id)
                }) {
                    return 0.0;
                }
                effect
            }
            EffectData::Legacy(effect) => effect,
        };
        match payload {
            EffectPayload::Known(LayerEffect::GaussianBlur {
                blurriness,
                repeat_edge_pixels,
                layer_size: None,
            }) if !repeat_edge_pixels.unwrap_or(false) && reach.is_none() => {
                reach = Some(blurriness.value());
            }
            // Mixed kernels, repeat-edge planes and unknown effects do not
            // acquire an exact enclosure from this isolated native profile.
            _ => return 0.0,
        }
    }
    reach.unwrap_or(0.0)
}
