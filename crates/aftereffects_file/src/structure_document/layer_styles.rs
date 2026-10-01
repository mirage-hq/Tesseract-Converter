//! Best-effort import of native AE Layer Styles into existing FX `LayerEffect`s.

use fx_schema::{
    EffectData, EffectId, EffectPayload, EffectRecord, LayerEffect, PropertyTarget,
    animator::AnimationGraphEntry,
};
use serde_json::json;

use super::{
    animation::{self, NumericAnimationClock, NumericAnimationTarget},
    animation_budget::AnimationBudget,
};
use crate::{
    layer_styles::{self, NativeLayerStyle},
    properties::NumericProperty,
    structure::Layer,
};

pub(super) struct ImportedLayerStyles {
    pub effects: Vec<EffectRecord>,
    pub animations: Vec<AnimationGraphEntry>,
    pub warnings: Vec<String>,
}

pub(super) fn import(
    layer: &Layer,
    size: [f64; 2],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> ImportedLayerStyles {
    let decoded = layer_styles::read(&layer.content, size);
    let mut result = ImportedLayerStyles {
        effects: Vec::new(),
        animations: Vec::new(),
        warnings: decoded.warnings,
    };
    for (style, source_properties) in decoded.styles.into_iter().zip(decoded.source_properties) {
        let label = style_label(&style);
        // Keep the cursor unchanged if this style fails validation or serialization.
        let mut candidate_id = *next_id;
        let Some(raw_id) = super::reserve_ids(&mut candidate_id, 1) else {
            result.warnings.push(format!(
                "Layer Style {label}: generated layer identifier space exhausted; style omitted"
            ));
            continue;
        };
        let effect = match editable_effect(&style) {
            Ok(effect) => effect,
            Err(error) => {
                result.warnings.push(format!(
                    "Layer Style {label}: mapped value is outside the current FX representation ({error}); style omitted"
                ));
                continue;
            }
        };
        let id = EffectId::new(raw_id);
        let checkpoint = budget.checkpoint();
        let animations = import_animations(
            layer,
            &style,
            &source_properties,
            id,
            budget,
            &mut result.warnings,
        );
        match EffectRecord::from_data(&EffectData::Identified {
            id,
            enabled: layer.record.flags().effects_active,
            effect: EffectPayload::Known(effect),
        }) {
            Ok(effect) => {
                *next_id = candidate_id;
                result.effects.push(effect);
                result.animations.extend(animations);
            }
            Err(error) => {
                budget.rollback(checkpoint);
                result.warnings.push(format!(
                    "Layer Style {label}: mapped value is outside the current FX representation ({error}); style omitted"
                ));
            }
        }
    }
    result
}

fn import_animations(
    layer: &Layer,
    style: &NativeLayerStyle,
    source_properties: &[(String, NumericProperty)],
    id: EffectId,
    budget: &mut AnimationBudget,
    warnings: &mut Vec<String>,
) -> Vec<AnimationGraphEntry> {
    let mut animations = Vec::new();
    let clock = NumericAnimationClock::parent_identity(layer);
    let drop_shadow_blur_is_compatible = !matches!(style, NativeLayerStyle::DropShadow(_))
        || drop_shadow_blur_animation_compatible(source_properties);
    for (name, numeric) in source_properties {
        if !numeric.animated && !numeric.expression_enabled {
            continue;
        }
        if matches!(style, NativeLayerStyle::DropShadow(_))
            && name == "dropShadow/blur"
            && !drop_shadow_blur_is_compatible
        {
            warnings.push("Layer Style Drop Shadow / dropShadow/blur: animated native Size cannot become blurRadius while native Spread is nonzero or animated; initial value retained".into());
            continue;
        }
        let Some((param, scale)) = animation_mapping(style, name) else {
            warnings.push(format!(
                "Layer Style {} / {name}: no compatible editable animation target; initial value retained",
                style_label(style)
            ));
            continue;
        };
        if numeric.expression_enabled {
            warnings.push(format!(
                "Layer Style {} / {name}: AE expression is not executed; authored keys/initial value are retained where available",
                style_label(style)
            ));
        }
        let clock = match &clock {
            Ok(clock) => *clock,
            Err(error) => {
                warnings.push(format!(
                    "Layer Style {} / {name}: {error}; initial value retained",
                    style_label(style)
                ));
                continue;
            }
        };
        let target =
            NumericAnimationTarget::float(PropertyTarget::effect_param(id, param), 0, scale);
        let (entries, entry_warnings) =
            animation::numeric_entries(name, numeric, &[target], clock, budget);
        animations.extend(entries);
        warnings.extend(
            entry_warnings
                .into_iter()
                .map(|warning| format!("Layer Style {}: {warning}", style_label(style))),
        );
    }
    animations
}

fn drop_shadow_blur_animation_compatible(source_properties: &[(String, NumericProperty)]) -> bool {
    source_properties
        .iter()
        .find(|(name, _)| name == "dropShadow/chokeMatte")
        .is_some_and(|(_, property)| {
            !property.animated
                && !property.expression_enabled
                && property.values.first().copied() == Some(0.0)
        })
}

fn animation_mapping(style: &NativeLayerStyle, name: &str) -> Option<(&'static str, f64)> {
    match (style, name) {
        (NativeLayerStyle::DropShadow(_), "dropShadow/blur") => Some(("blurRadius", 0.5)),
        (NativeLayerStyle::InnerShadow(_), "innerShadow/blur") => Some(("size", 1.0)),
        (NativeLayerStyle::InnerShadow(_), "innerShadow/chokeMatte") => Some(("choke", 0.01)),
        (NativeLayerStyle::OuterGlow(_), "outerGlow/blur") => Some(("size", 1.0)),
        (NativeLayerStyle::OuterGlow(_), "outerGlow/chokeMatte") => Some(("spread", 0.01)),
        (NativeLayerStyle::OuterGlow(_), "outerGlow/inputRange") => Some(("range", 0.01)),
        (NativeLayerStyle::InnerGlow(_), "innerGlow/blur") => Some(("size", 1.0)),
        (NativeLayerStyle::InnerGlow(_), "innerGlow/chokeMatte") => Some(("choke", 0.01)),
        (NativeLayerStyle::InnerGlow(_), "innerGlow/inputRange") => Some(("range", 0.01)),
        (NativeLayerStyle::BevelEmboss(_), "bevelEmboss/strengthRatio") => Some(("depth", 0.01)),
        (NativeLayerStyle::BevelEmboss(_), "bevelEmboss/blur") => Some(("size", 1.0)),
        (NativeLayerStyle::BevelEmboss(_), "bevelEmboss/softness") => Some(("soften", 1.0)),
        (NativeLayerStyle::BevelEmboss(_), "bevelEmboss/localLightingAngle") => {
            Some(("angle", 1.0))
        }
        (NativeLayerStyle::BevelEmboss(_), "bevelEmboss/localLightingAltitude") => {
            Some(("altitude", 1.0))
        }
        (NativeLayerStyle::Satin(_), "chromeFX/blur") => Some(("size", 1.0)),
        (NativeLayerStyle::GradientOverlay(_), "gradientFill/opacity") => Some(("opacity", 0.01)),
        (NativeLayerStyle::Stroke(_), "frameFX/size") => Some(("width", 1.0)),
        _ => None,
    }
}

fn editable_effect(style: &NativeLayerStyle) -> Result<LayerEffect, serde_json::Error> {
    let value = match style {
        NativeLayerStyle::DropShadow(style) => json!({
            "type": "dropShadow",
            "enabled": style.enabled,
            "color": style.color,
            "offset": style.offset,
            "blurRadius": style.size,
            "spreadRadius": style.spread,
            "blendMode": style.blend_mode,
        }),
        NativeLayerStyle::InnerShadow(style) => json!({
            "type": "innerShadow",
            "enabled": style.enabled,
            "color": style.color,
            "offset": style.offset,
            "size": style.size,
            "choke": style.choke,
            "blendMode": style.blend_mode,
        }),
        NativeLayerStyle::OuterGlow(style) => json!({
            "type": "outerGlow",
            "enabled": style.enabled,
            "color": style.color,
            "size": style.size,
            "spread": style.spread,
            "range": style.range,
            "blendMode": style.blend_mode,
        }),
        NativeLayerStyle::InnerGlow(style) => json!({
            "type": "innerGlow",
            "enabled": style.enabled,
            "color": style.color,
            "size": style.size,
            "choke": style.choke,
            "range": style.range,
            "source": style.source,
            "blendMode": style.blend_mode,
        }),
        NativeLayerStyle::BevelEmboss(style) => json!({
            "type": "bevelEmboss",
            "enabled": style.enabled,
            "style": style.style,
            "technique": style.technique,
            "depth": style.depth,
            "direction": style.direction,
            "size": style.size,
            "soften": style.soften,
            "angle": style.angle,
            "altitude": style.altitude,
            "highlightColor": style.highlight_color,
            "shadowColor": style.shadow_color,
        }),
        NativeLayerStyle::Satin(style) => json!({
            "type": "satin",
            "enabled": style.enabled,
            "color": style.color,
            "offset": style.offset,
            "size": style.size,
            "invert": style.invert,
            "blendMode": style.blend_mode,
        }),
        NativeLayerStyle::ColorOverlay(style) => json!({
            "type": "gradientOverlay",
            "enabled": style.enabled,
            "opacity": 1.0,
            "gradientType": "linear",
            "start": [0.0, 0.0],
            "end": [1.0, 0.0],
            "stops": [
                { "offset": 0.0, "color": style.color },
                { "offset": 1.0, "color": style.color }
            ],
            "blendMode": style.blend_mode,
        }),
        NativeLayerStyle::GradientOverlay(style) => json!({
            "type": "gradientOverlay",
            "enabled": style.enabled,
            "opacity": style.opacity,
            "gradientType": style.gradient_type,
            "start": style.start,
            "end": style.end,
            "stops": style.stops,
            "blendMode": style.blend_mode,
        }),
        NativeLayerStyle::Stroke(style) => json!({
            "type": "stroke",
            "enabled": style.enabled,
            "color": style.color,
            "width": style.size,
            "position": style.position,
            "blendMode": style.blend_mode,
        }),
    };
    serde_json::from_value(value)
}

fn style_label(style: &NativeLayerStyle) -> &'static str {
    match style {
        NativeLayerStyle::DropShadow(_) => "Drop Shadow",
        NativeLayerStyle::InnerShadow(_) => "Inner Shadow",
        NativeLayerStyle::OuterGlow(_) => "Outer Glow",
        NativeLayerStyle::InnerGlow(_) => "Inner Glow",
        NativeLayerStyle::BevelEmboss(_) => "Bevel and Emboss",
        NativeLayerStyle::Satin(_) => "Satin",
        NativeLayerStyle::ColorOverlay(_) => "Color Overlay",
        NativeLayerStyle::GradientOverlay(_) => "Gradient Overlay",
        NativeLayerStyle::Stroke(_) => "Stroke",
    }
}

#[cfg(test)]
mod tests {
    use crate::properties::{NumericKeyframe, NumericProperty, NumericValueKind};

    use super::drop_shadow_blur_animation_compatible;

    #[test]
    fn native_style_survives_former_occurrence_limit() {
        let project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/layer_styles/styles_static_adobe.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(composition) = &project.item(1).unwrap().kind
        else {
            panic!("native DropShadow target must be a composition");
        };
        let layer = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 15)
            .unwrap();
        let mut next_id = 10_000;
        let imported = super::import(
            layer,
            [f64::from(composition.width), f64::from(composition.height)],
            &mut next_id,
            &mut super::AnimationBudget::default(),
        );
        assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
        assert_eq!(next_id, 10_001);
        next_id = u64::MAX - 1;
        for expected in [1, 0] {
            let imported = super::import(
                layer,
                [f64::from(composition.width), f64::from(composition.height)],
                &mut next_id,
                &mut super::AnimationBudget::default(),
            );
            assert_eq!(imported.effects.len(), expected);
            assert_eq!(next_id, u64::MAX);
        }
    }

    fn scalar(value: f64, animated: bool) -> NumericProperty {
        NumericProperty {
            values: vec![value],
            animated,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        }
    }

    fn keyed_size_from_zero() -> NumericProperty {
        let key = |time_secs, value| NumericKeyframe {
            time_secs,
            values: vec![value],
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: vec![0.0],
            in_influence: vec![0.0],
            out_speed: vec![0.0],
            out_influence: vec![0.0],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        NumericProperty {
            values: Vec::new(),
            animated: true,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: vec![key(0.0, 0.0), key(1.0, 10.0)],
            value_kind: NumericValueKind::Continuous,
        }
    }

    #[test]
    fn zero_first_size_key_does_not_hide_nonzero_or_animated_native_spread() {
        let nonzero = vec![
            ("dropShadow/blur".into(), keyed_size_from_zero()),
            ("dropShadow/chokeMatte".into(), scalar(20.0, false)),
        ];
        assert!(!drop_shadow_blur_animation_compatible(&nonzero));

        let animated = vec![
            ("dropShadow/blur".into(), keyed_size_from_zero()),
            ("dropShadow/chokeMatte".into(), scalar(0.0, true)),
        ];
        assert!(!drop_shadow_blur_animation_compatible(&animated));

        let zero = vec![
            ("dropShadow/blur".into(), keyed_size_from_zero()),
            ("dropShadow/chokeMatte".into(), scalar(0.0, false)),
        ];
        assert!(drop_shadow_blur_animation_compatible(&zero));
    }
}
