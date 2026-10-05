//! Native/FX joint support policy for consumer-demand cropping.
//! A finite FX shader footprint alone does not certify native AE support.

use fx_schema::effect::{EffectData, EffectPayload, EffectRecord, LayerEffect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Support {
    Pointwise,
    /// Native support not yet independently established. Never choose a
    /// finite canvas from the FX effect's nominal UI radius alone.
    Full(&'static str),
}

pub(super) fn effect(record: &EffectRecord) -> Support {
    let payload = match record.data() {
        EffectData::Identified { enabled: false, .. } => return Support::Pointwise,
        EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
    };
    match payload {
        EffectPayload::Known(
            LayerEffect::Exposure { .. }
            | LayerEffect::HueSaturation { .. }
            | LayerEffect::TintTritone { .. },
        ) => Support::Pointwise,
        EffectPayload::Known(LayerEffect::CustomShader { .. })
            if super::super::effects::unmapped_warning(record).is_some() =>
        {
            // The shader is explicitly omitted from native output, not treated
            // as a pointwise implementation of the original WGSL program.
            Support::Pointwise
        }
        EffectPayload::Known(LayerEffect::Glow { .. }) => {
            Support::Full("Glow native finite reach at the crop boundary is not proved")
        }
        EffectPayload::Known(LayerEffect::GaussianBlur { .. }) => {
            Support::Full("Gaussian native finite reach at the crop boundary is not proved")
        }
        EffectPayload::Known(LayerEffect::ChromaticAberration { .. })
            if super::super::effects::unmapped_warning(record).is_some() =>
        {
            // This source control is not present in the generated native
            // consumer; it cannot expand that consumer's sampling demand.
            // Its separate omission diagnostic is still emitted by lowering.
            Support::Pointwise
        }
        _ => Support::Full("Effect has no proven native and FX support preimage"),
    }
}

/// Source input needed by the approved transparent-edge Gaussian/default Outer
/// Glow profile. Kernel reaches add across stages; the result is never a canvas
/// cap. The caller intersects content with this expanded consumer preimage.
pub(super) fn input_reach(
    records: &[EffectRecord],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<f64, &'static str> {
    let mut reach = 0.0;
    for record in records.iter().rev() {
        let (id, payload) = match record.data() {
            EffectData::Identified { enabled: false, .. } => continue,
            EffectData::Identified { id, effect, .. } => (Some(*id), effect),
            EffectData::Legacy(effect) => (None, effect),
        };
        let radius = match payload {
            EffectPayload::Known(LayerEffect::GaussianBlur {
                blurriness,
                repeat_edge_pixels,
                layer_size: None,
            }) if !repeat_edge_pixels.unwrap_or(false) => {
                super::animated_bounds::effect_scalar_max(
                    dynamics,
                    id,
                    "blurriness",
                    blurriness.value(),
                )?
            }
            EffectPayload::Known(LayerEffect::OuterGlow(glow)) => {
                if !glow.enabled {
                    continue;
                }
                // Spread is the hard-dilation share of Size, with the remaining
                // share supplying softness, not an independent extra radius.
                super::animated_bounds::effect_scalar_max(dynamics, id, "size", glow.size.value())?
            }
            _ if effect(record) == Support::Pointwise => 0.0,
            _ => return Err("Effect has no finite source-input support profile"),
        };
        reach += radius;
        if !reach.is_finite() {
            return Err("Effect input support overflows");
        }
    }
    Ok(reach)
}

/// Retain known additive reach, omitting only the explicitly opted-in blur
/// profiles whose native boundary support is unproved. Never guess a radius.
pub(super) fn viewport_approximation_reach(
    records: &[EffectRecord],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<f64, &'static str> {
    let mut reach = 0.0;
    for record in records {
        let (id, payload) = match record.data() {
            EffectData::Identified { enabled: false, .. } => continue,
            EffectData::Identified { id, effect, .. } => (Some(*id), effect),
            EffectData::Legacy(effect) => (None, effect),
        };
        let radius = match payload {
            EffectPayload::Known(LayerEffect::GaussianBlur {
                blurriness,
                layer_size,
                ..
            }) => {
                // Supported Gaussian reach wins; validation failures do not
                // become permission to ignore malformed controls.
                match input_reach(std::slice::from_ref(record), dynamics) {
                    Ok(radius) => radius,
                    Err("Effect has no finite source-input support profile") => {
                        super::animated_bounds::effect_scalar_max(
                            dynamics,
                            id,
                            "blurriness",
                            blurriness.value(),
                        )?;
                        if layer_size.is_some_and(|(width, height)| {
                            !width.is_finite()
                                || !height.is_finite()
                                || width <= 0.0
                                || height <= 0.0
                        }) {
                            return Err("Gaussian source domain is non-finite or empty");
                        }
                        0.0
                    }
                    Err(reason) => return Err(reason),
                }
            }
            EffectPayload::Known(LayerEffect::DirectionalBlur {
                direction,
                blur_length,
            }) if direction.is_finite() => {
                super::animated_bounds::effect_scalar_max(
                    dynamics,
                    id,
                    "blurLength",
                    blur_length.value(),
                )?;
                0.0
            }
            EffectPayload::Known(LayerEffect::RadialBlur {
                center_x,
                center_y,
                amount,
            }) if center_x.is_finite() && center_y.is_finite() => {
                super::animated_bounds::effect_scalar_max(dynamics, id, "amount", *amount)?;
                0.0
            }
            _ => input_reach(std::slice::from_ref(record), dynamics)?,
        };
        reach += radius;
        if !reach.is_finite() {
            return Err("Effect input support overflows");
        }
    }
    Ok(reach)
}

/// Enclose a static zero-spread native Drop Shadow stack, without granting a
/// consumer crop certificate. AE Size is twice the FX blur radius; it is the
/// approved finite-support upper bound, not the quantized visible alpha extent.
/// Keep other effects and animated controls outside this bounded profile.
pub(super) fn shadow_bounds(
    mut bounds: super::Bounds,
    records: &[EffectRecord],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<super::Bounds, &'static str> {
    for record in records.iter().rev() {
        let (id, payload) = match record.data() {
            EffectData::Identified { enabled: false, .. } => continue,
            EffectData::Identified { id, effect, .. } => (Some(*id), effect),
            EffectData::Legacy(effect) => (None, effect),
        };
        if id.is_some_and(|id| {
            dynamics.iter().any(|entry| {
                matches!(&entry.target, fx_schema::PropertyTarget::EffectProperty(target)
                if target.effect_id() == id)
            })
        }) {
            return Err("Animated Drop Shadow has no static support enclosure");
        }
        let EffectPayload::Known(LayerEffect::DropShadow(shadow)) = payload else {
            return Err("Only static Drop Shadow has this native support enclosure");
        };
        if !shadow.enabled {
            continue;
        }
        let radius = 2.0 * shadow.blur_radius.value();
        if shadow.spread_radius.value() != 0.0
            || !radius.is_finite()
            || radius < 0.0
            || !shadow.offset.iter().all(|value| value.is_finite())
        {
            return Err("Drop Shadow support requires finite controls and zero spread");
        }
        let shifted = super::Bounds {
            min: std::array::from_fn(|axis| bounds.min[axis] + shadow.offset[axis]),
            max: std::array::from_fn(|axis| bounds.max[axis] + shadow.offset[axis]),
        }
        .expand(radius)?;
        bounds.include(shifted);
        if !bounds
            .min
            .iter()
            .chain(&bounds.max)
            .all(|value| value.is_finite())
        {
            return Err("Drop Shadow support overflow");
        }
    }
    Ok(bounds)
}

pub(super) fn stack(records: &[EffectRecord]) -> Result<(), &'static str> {
    for record in records.iter().rev() {
        if let Support::Full(reason) = effect(record) {
            return Err(reason);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shadow(offset: [f64; 2], radius: f64, spread: f64) -> EffectRecord {
        serde_json::from_value(serde_json::json!({
            "id": 810, "enabled": true, "effect": {
                "type": "dropShadow", "enabled": true,
                "offset": offset, "blurRadius": radius, "spreadRadius": spread
            }
        }))
        .unwrap()
    }

    #[test]
    fn spatial_shadow_exception_requires_active_shadow() {
        let dynamics = crate::export_document::AnimationIndex::new(&[]);
        assert!(!super::super::has_active_shadow(&[]));
        let active = shadow([0.0; 2], 0.0, 0.0);
        assert!(super::super::has_active_shadow(std::slice::from_ref(
            &active
        )));
        for (record_enabled, shadow_enabled) in [(false, true), (true, false), (false, false)] {
            let mut value = serde_json::to_value(&active).unwrap();
            value["enabled"] = serde_json::json!(record_enabled);
            value["effect"]["enabled"] = serde_json::json!(shadow_enabled);
            let disabled = serde_json::from_value(value).unwrap();
            assert!(!super::super::has_active_shadow(std::slice::from_ref(
                &disabled
            )));
            assert!(super::super::static_shadow_stack(&[disabled], &dynamics));
        }
        let unsupported: EffectRecord = serde_json::from_value(serde_json::json!({
            "id": 811, "enabled": false, "effect": {"type": "unsupportedSpatialEffect"}
        }))
        .unwrap();
        assert!(!super::super::has_active_shadow(std::slice::from_ref(
            &unsupported
        )));
        assert!(super::super::static_shadow_stack(&[unsupported], &dynamics));
    }

    #[test]
    fn native_shadow_defined_support_contains_measured_extent_and_content() {
        let dynamics = crate::export_document::AnimationIndex::new(&[]);
        let input = super::super::Bounds {
            min: [320.0; 2],
            max: [448.0; 2],
        };
        let bounds = shadow_bounds(input, &[shadow([0.0, 28.0], 65.0, 0.0)], &dynamics).unwrap();
        assert_eq!(bounds.min, [190.0, 218.0]);
        assert_eq!(bounds.max, [578.0, 606.0]);
        // Independent AE26.5 alpha support was [199,227,569,597]. The
        // definition-derived bound intentionally does not shrink to those pixels.
        let bounds =
            shadow_bounds(input, &[shadow([-200.0, -300.0], 0.0, 0.0)], &dynamics).unwrap();
        assert_eq!(bounds.min, [120.0, 20.0]);
        assert_eq!(bounds.max, input.max);
        assert!(shadow_bounds(input, &[shadow([0.0; 2], 65.0, 1.0)], &dynamics).is_err());
        let overflow = super::super::Bounds {
            min: [f64::MAX; 2],
            max: [f64::MAX; 2],
        };
        assert!(shadow_bounds(overflow, &[shadow([f64::MAX; 2], 65.0, 0.0)], &dynamics).is_err());
    }
}
