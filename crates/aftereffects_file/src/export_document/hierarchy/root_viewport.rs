//! Output clipping for an untransformed Group at the final root.
//!
//! This does not restrict any child's input/support bounds. Child effects run
//! before this output clip, and the existing root lens is retained. Nested or
//! transformed occurrences must continue through the full-support planner.

use fx_schema::effect::{EffectData, EffectPayload, EffectRecord, LayerEffect};

use super::*;

pub(super) fn canvas(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    siblings: &[Layer],
    dimensions: fx_schema::Dimensions,
) -> Option<CertifiedCanvas> {
    canvas_inner(group, dynamics, siblings, dimensions, false, false)
}

/// Opacity changes alpha after source rendering without changing its spatial
/// sampling domain. Keep every other owner animation outside this certificate.
pub(super) fn nested_canvas(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    siblings: &[Layer],
    dimensions: fx_schema::Dimensions,
) -> Option<CertifiedCanvas> {
    canvas_inner(group, dynamics, siblings, dimensions, true, false)
}

/// Retain only source pixels which can influence this identity consumer. Effects
/// run on the expanded input, and the native matte remains in its authored stage.
pub(super) fn input_canvas(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    siblings: &[Layer],
    dimensions: fx_schema::Dimensions,
) -> Option<CertifiedCanvas> {
    if group.effects.is_empty() && group.track_matte.is_none() {
        return None;
    }
    let mut canvas = canvas_inner(group, dynamics, siblings, dimensions, true, true)?;
    canvas.root_output = false;
    canvas.intersect_content = true;
    Some(canvas)
}

fn canvas_inner(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    siblings: &[Layer],
    dimensions: fx_schema::Dimensions,
    allow_opacity: bool,
    allow_input_support: bool,
) -> Option<CertifiedCanvas> {
    if group.transform != super::super::identity_fx_transform()
        || group.motion_blur
        || (!allow_input_support && !group.effects.is_empty())
        || (allow_input_support && effect_support::input_reach(&group.effects, dynamics).is_err())
        || !group.masks.is_empty()
        || (!allow_input_support && group.track_matte.is_some())
        || !group.fills.is_empty()
        || [
            group.padding_top.value(),
            group.padding_right.value(),
            group.padding_bottom.value(),
            group.padding_left.value(),
            group.corner_radius_top_left.value(),
            group.corner_radius_top_right.value(),
            group.corner_radius_bottom_right.value(),
            group.corner_radius_bottom_left.value(),
        ]
        .into_iter()
        .any(|value| value != 0.0)
        || dynamics.for_layer(group.id).any(|entry| {
            entry.target.as_property().is_some_and(|target| {
                target.layer_id() == group.id
                    && !(allow_opacity && target.property_type() == fx_schema::PropType::Opacity)
            })
        })
    {
        return None;
    }
    for sibling in siblings {
        if sibling.id() == group.id {
            continue;
        }
        if references(sibling, group.id)? {
            return None;
        }
        if let LayerData::Adjustment(adjustment) = sibling.data()
            && !adjustment.effects.iter().all(pointwise_adjustment)
        {
            return None;
        }
    }
    Some(CertifiedCanvas {
        bounds: Bounds {
            min: [0.0; 2],
            max: [f64::from(dimensions.width), f64::from(dimensions.height)],
        },
        root_output: true,
        consumer_3d: false,
        source_mask: false,
        intersect_content: false,
    })
}

pub(super) fn references(layer: &Layer, target: LayerId) -> Option<bool> {
    let options = super::super::native_layer_options(layer).ok()?;
    if options.matte.is_some_and(|matte| matte.layer == target)
        || super::super::mask_and_transform(layer)
            .is_some_and(|(_, masks, _)| masks.iter().any(|mask| mask.layer == Some(target)))
    {
        return Some(true);
    }
    for record in layer.data().effects() {
        let payload = match record.data() {
            EffectData::Identified { enabled: false, .. } => continue,
            EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
        };
        match payload {
            // CustomShader and its texture inputs are dropped in native output;
            // they must not create source demands absent from unshaded content.
            EffectPayload::Known(_) => {}
            _ => return None,
        }
    }
    if let LayerData::Group(group) = layer.data() {
        for child in &group.layers {
            if references(child, target)? {
                return Some(true);
            }
        }
    }
    Some(false)
}

pub(super) fn pointwise_adjustment(record: &EffectRecord) -> bool {
    let payload = match record.data() {
        EffectData::Identified { enabled: false, .. } => return true,
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
    };
    // These change the current input pixel, not its sampling location. Grain
    // interpolates procedural noise, not neighboring image samples. Root
    // Adjustment dimensions stay unchanged, including Vignette's radial frame.
    // Existing color/kernel approximations are not made fidelity claims here.
    // A diagnosed, unexported shader has no native pixels to sample outside
    // the source canvas; it is not a pointwise implementation of that shader.
    matches!(
        payload,
        EffectPayload::Known(
            LayerEffect::Exposure { .. }
                | LayerEffect::HueSaturation { .. }
                | LayerEffect::TintTritone { .. }
                | LayerEffect::Grain { .. }
                | LayerEffect::Vignette { .. }
        )
    ) || matches!(
        payload,
        EffectPayload::Known(LayerEffect::CustomShader { .. })
    ) && super::super::effects::unmapped_warning(record).is_some()
}
