//! Lower existing FX LayerEffect payloads to fresh native AE Layer Styles.

use fx_schema::{
    BevelDirection, BevelStyle, BevelTechnique, BlendMode, EffectId, GlowSource, LayerEffect,
    LayerStrokePosition, PropertyTarget, PropertyValue, ShapeGradientStop, ShapeGradientType,
    animator::AnimationGraphEntry,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::layer_styles::{
    NativeBevelEmboss, NativeColorOverlay, NativeDropShadow, NativeGradientOverlay,
    NativeInnerGlow, NativeInnerShadow, NativeLayerStyle, NativeLayerStyleAnimation,
    NativeOuterGlow, NativeSatin, NativeStroke, encode_blend_mode,
};

use super::effective_constant;

pub(super) struct LoweredLayerStyle {
    pub style: Option<NativeLayerStyle>,
    pub warnings: Vec<String>,
}

pub(super) fn lower(
    effect: &LayerEffect,
    record_enabled: bool,
    id: Option<EffectId>,
    dynamics: &[AnimationGraphEntry],
    _size: [f64; 2],
) -> LoweredLayerStyle {
    let label = style_label(effect);
    let mut warnings = Vec::new();
    warn_static_animation(
        label,
        id,
        dynamics,
        supported_animation_params(effect),
        &mut warnings,
    );
    let value = match serde_json::to_value(effect) {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!(
                "Layer Style {label}: invalid FX payload ({error}); style omitted, owner and siblings retained"
            ));
            return LoweredLayerStyle {
                style: None,
                warnings,
            };
        }
    };
    let enabled = record_enabled && boolean(&value, "enabled", true);
    let style = match effect {
        LayerEffect::DropShadow(_) => {
            warnings.push("Layer Style Drop Shadow: the shared FX DropShadow payload is canonicalized to AE's native Layer Style rather than Effect Parade; plugin-only Shadow Only behavior is unavailable, while FX spread is retained through native Spread".into());
            let color = normalized_color(
                label,
                color(&value, "color", [0.0, 0.0, 0.0, 1.0]),
                &mut warnings,
            );
            let mut radius = |param| {
                let effective = id.and_then(|id| {
                    match effective_parameter_constant(dynamics, id, param) {
                        Ok(Some(PropertyValue::Float(value))) => Some(*value),
                        Ok(Some(_)) => {
                            warnings.push(format!("Layer Style Drop Shadow / {param}: runtime-visible constant is not numeric; authored base retained"));
                            None
                        }
                        Ok(None) => None,
                        Err(reason) => {
                            warnings.push(format!(
                                "Layer Style Drop Shadow / {param}: {reason}; authored base retained"
                            ));
                            None
                        }
                    }
                });
                nonnegative(
                    label,
                    param,
                    effective.unwrap_or_else(|| number(&value, param, 0.0)),
                    &mut warnings,
                )
            };
            let blur_radius = radius("blurRadius");
            let spread_radius = radius("spreadRadius");
            let total_radius = spread_radius + 2.0 * blur_radius;
            let adjusted_spread = if total_radius > 0.0 {
                spread_radius / total_radius
            } else {
                0.0
            };
            if adjusted_spread > 0.8 && adjusted_spread < 1.0 {
                warnings.push("Layer Style Drop Shadow: FX blur/spread ratio lies in libpag's native Spread discontinuity (80%..100%); native 100% Spread approximation drops the remaining blur".into());
            }
            let blur_animated =
                id.is_some_and(|id| has_parameter_animation(dynamics, id, "blurRadius"));
            let spread_animated =
                id.is_some_and(|id| has_parameter_animation(dynamics, id, "spreadRadius"));
            let animations = if spread_animated {
                warnings.push("Layer Style Drop Shadow / blurRadius + spreadRadius: either animation requires coupled nonlinear native Size and Spread tracks; both animations omitted, authored bases retained".into());
                Vec::new()
            } else if spread_radius == 0.0 {
                style_animations(
                    label,
                    id,
                    dynamics,
                    &[("blurRadius", "dropShadow/blur", 0.5, 0.0)],
                    &mut warnings,
                )
            } else {
                if blur_animated {
                    warnings.push("Layer Style Drop Shadow / blurRadius + spreadRadius: blur animation with nonzero spread requires coupled nonlinear native Size and Spread tracks; animation omitted, authored bases retained".into());
                }
                Vec::new()
            };
            Some(NativeLayerStyle::DropShadow(NativeDropShadow {
                enabled,
                color,
                offset: point(&value, "offset", [0.0; 2]),
                size: blur_radius,
                spread: spread_radius,
                blend_mode: native_blend(label, blend(&value), &mut warnings),
                animations,
            }))
        }
        LayerEffect::InnerShadow(_) => Some(NativeLayerStyle::InnerShadow(NativeInnerShadow {
            enabled,
            color: normalized_color(
                label,
                color(&value, "color", [0.0, 0.0, 0.0, 0.75]),
                &mut warnings,
            ),
            offset: point(&value, "offset", [0.0; 2]),
            size: nonnegative(label, "size", number(&value, "size", 0.0), &mut warnings),
            choke: unit(label, "choke", number(&value, "choke", 0.0), &mut warnings),
            blend_mode: native_blend(label, blend(&value), &mut warnings),
            animations: style_animations(
                label,
                id,
                dynamics,
                &[
                    ("size", "innerShadow/blur", 1.0, 0.0),
                    ("choke", "innerShadow/chokeMatte", 0.01, 0.0),
                ],
                &mut warnings,
            ),
        })),
        LayerEffect::OuterGlow(_) => {
            warnings.push("Layer Style Outer Glow uses a solid-color, zero-noise, default-technique native approximation; gradients, contour, jitter and noise are not represented by the current FX payload.".into());
            Some(NativeLayerStyle::OuterGlow(NativeOuterGlow {
                enabled,
                color: normalized_color(
                    label,
                    color(&value, "color", [1.0, 1.0, 0.75, 0.75]),
                    &mut warnings,
                ),
                size: nonnegative(label, "size", number(&value, "size", 0.0), &mut warnings),
                spread: unit(
                    label,
                    "spread",
                    number(&value, "spread", 0.0),
                    &mut warnings,
                ),
                range: unit(label, "range", number(&value, "range", 0.5), &mut warnings),
                blend_mode: native_blend(label, blend(&value), &mut warnings),
                animations: style_animations(
                    label,
                    id,
                    dynamics,
                    &[
                        ("size", "outerGlow/blur", 1.0, 0.0),
                        ("spread", "outerGlow/chokeMatte", 0.01, 0.0),
                        ("range", "outerGlow/inputRange", 0.01, 0.0),
                    ],
                    &mut warnings,
                ),
            }))
        }
        LayerEffect::InnerGlow(_) => {
            warnings.push("Layer Style Inner Glow uses a solid-color, zero-noise, default-technique native approximation; gradients, contour, jitter and noise are not represented by the current FX payload.".into());
            Some(NativeLayerStyle::InnerGlow(NativeInnerGlow {
                enabled,
                color: normalized_color(
                    label,
                    color(&value, "color", [1.0, 1.0, 0.75, 0.75]),
                    &mut warnings,
                ),
                size: nonnegative(label, "size", number(&value, "size", 0.0), &mut warnings),
                choke: unit(label, "choke", number(&value, "choke", 0.0), &mut warnings),
                range: unit(label, "range", number(&value, "range", 0.5), &mut warnings),
                source: enumeration(&value, "source", GlowSource::Edge),
                blend_mode: native_blend(label, blend(&value), &mut warnings),
                animations: style_animations(
                    label,
                    id,
                    dynamics,
                    &[
                        ("size", "innerGlow/blur", 1.0, 0.0),
                        ("choke", "innerGlow/chokeMatte", 0.01, 0.0),
                        ("range", "innerGlow/inputRange", 0.01, 0.0),
                    ],
                    &mut warnings,
                ),
            }))
        }
        LayerEffect::BevelEmboss(_) => {
            warnings.push("Layer Style Bevel and Emboss exports canonical Screen highlights and Multiply shadows; the FX payload has no separate highlight/shadow blend controls or contour/texture controls.".into());
            Some(NativeLayerStyle::BevelEmboss(NativeBevelEmboss {
                enabled,
                style: enumeration(&value, "style", BevelStyle::InnerBevel),
                technique: enumeration(&value, "technique", BevelTechnique::Smooth),
                depth: nonnegative(label, "depth", number(&value, "depth", 1.0), &mut warnings),
                direction: enumeration(&value, "direction", BevelDirection::Up),
                size: nonnegative(label, "size", number(&value, "size", 5.0), &mut warnings),
                soften: nonnegative(
                    label,
                    "soften",
                    number(&value, "soften", 0.0),
                    &mut warnings,
                ),
                angle: finite(
                    label,
                    "angle",
                    number(&value, "angle", 120.0),
                    120.0,
                    &mut warnings,
                ),
                altitude: finite(
                    label,
                    "altitude",
                    number(&value, "altitude", 30.0),
                    30.0,
                    &mut warnings,
                ),
                highlight_color: normalized_color(
                    label,
                    color(&value, "highlightColor", [1.0, 1.0, 1.0, 0.75]),
                    &mut warnings,
                ),
                shadow_color: normalized_color(
                    label,
                    color(&value, "shadowColor", [0.0, 0.0, 0.0, 0.75]),
                    &mut warnings,
                ),
                animations: style_animations(
                    label,
                    id,
                    dynamics,
                    &[
                        ("depth", "bevelEmboss/strengthRatio", 0.01, 0.0),
                        ("size", "bevelEmboss/blur", 1.0, 0.0),
                        ("soften", "bevelEmboss/softness", 1.0, 0.0),
                        ("angle", "bevelEmboss/localLightingAngle", 1.0, 0.0),
                        ("altitude", "bevelEmboss/localLightingAltitude", 1.0, 0.0),
                    ],
                    &mut warnings,
                ),
            }))
        }
        LayerEffect::Satin(_) => Some(NativeLayerStyle::Satin(NativeSatin {
            enabled,
            color: normalized_color(
                label,
                color(&value, "color", [0.0, 0.0, 0.0, 1.0]),
                &mut warnings,
            ),
            offset: point(&value, "offset", [0.0; 2]),
            size: nonnegative(label, "size", number(&value, "size", 0.0), &mut warnings),
            invert: boolean(&value, "invert", true),
            blend_mode: native_blend(label, blend(&value), &mut warnings),
            animations: style_animations(
                label,
                id,
                dynamics,
                &[("size", "chromeFX/blur", 1.0, 0.0)],
                &mut warnings,
            ),
        })),
        LayerEffect::GradientOverlay(_) => {
            let opacity = unit(
                label,
                "opacity",
                number(&value, "opacity", 1.0),
                &mut warnings,
            );
            let stops: Vec<ShapeGradientStop> = enumeration(&value, "stops", Vec::new());
            if stops.is_empty() {
                warnings.push("Layer Style Gradient Overlay: an empty FX stop list renders nothing and has no equivalent enabled native gradient; style omitted, owner and siblings retained".into());
                None
            } else if constant_color(&stops) {
                let mut overlay_color =
                    normalized_color("Color Overlay", stops[0].color, &mut warnings);
                let stop_alpha = overlay_color[3];
                overlay_color[3] *= opacity;
                let mut animations = style_animations(
                    label,
                    id,
                    dynamics,
                    &[("opacity", "solidFill/opacity", 0.01, 0.0)],
                    &mut warnings,
                );
                for animation in &mut animations {
                    for key in &mut animation.track.keys {
                        key.values.iter_mut().for_each(|value| *value *= stop_alpha);
                    }
                }
                warnings.push("Layer Style Color Overlay: a constant FX GradientOverlay is exported with AE's canonical solidFill native identity".into());
                Some(NativeLayerStyle::ColorOverlay(NativeColorOverlay {
                    enabled,
                    color: overlay_color,
                    blend_mode: native_blend(label, blend(&value), &mut warnings),
                    animations,
                }))
            } else {
                let gradient_type = enumeration(&value, "gradientType", ShapeGradientType::Linear);
                let (start, end) = native_gradient_axis(
                    point(&value, "start", [0.0; 2]),
                    point(&value, "end", [0.0; 2]),
                    &mut warnings,
                );
                Some(NativeLayerStyle::GradientOverlay(NativeGradientOverlay {
                    enabled,
                    opacity,
                    gradient_type,
                    start,
                    end,
                    stops,
                    blend_mode: native_blend(label, blend(&value), &mut warnings),
                    animations: style_animations(
                        label,
                        id,
                        dynamics,
                        &[("opacity", "gradientFill/opacity", 0.01, 0.0)],
                        &mut warnings,
                    ),
                }))
            }
        }
        LayerEffect::Stroke(_) => Some(NativeLayerStyle::Stroke(NativeStroke {
            enabled,
            color: normalized_color(
                label,
                color(&value, "color", [1.0, 0.0, 0.0, 1.0]),
                &mut warnings,
            ),
            size: nonnegative(label, "width", number(&value, "width", 0.0), &mut warnings),
            position: enumeration(&value, "position", LayerStrokePosition::Outside),
            blend_mode: native_blend(label, blend(&value), &mut warnings),
            animations: style_animations(
                label,
                id,
                dynamics,
                &[("width", "frameFX/size", 1.0, 0.0)],
                &mut warnings,
            ),
        })),
        _ => None,
    };
    LoweredLayerStyle { style, warnings }
}

pub(super) fn is_layer_style(effect: &LayerEffect) -> bool {
    matches!(
        effect,
        LayerEffect::DropShadow(_)
            | LayerEffect::OuterGlow(_)
            | LayerEffect::Stroke(_)
            | LayerEffect::GradientOverlay(_)
            | LayerEffect::InnerShadow(_)
            | LayerEffect::InnerGlow(_)
            | LayerEffect::Satin(_)
            | LayerEffect::BevelEmboss(_)
    )
}

pub(super) fn style_label(effect: &LayerEffect) -> &'static str {
    match effect {
        LayerEffect::DropShadow(_) => "Drop Shadow",
        LayerEffect::OuterGlow(_) => "Outer Glow",
        LayerEffect::Stroke(_) => "Stroke",
        LayerEffect::GradientOverlay(style) if constant_color(style.stops()) => "Color Overlay",
        LayerEffect::GradientOverlay(_) => "Gradient Overlay",
        LayerEffect::InnerShadow(_) => "Inner Shadow",
        LayerEffect::InnerGlow(_) => "Inner Glow",
        LayerEffect::Satin(_) => "Satin",
        LayerEffect::BevelEmboss(_) => "Bevel and Emboss",
        _ => "Unknown",
    }
}

pub(super) fn style_key(effect: &LayerEffect) -> &'static str {
    match effect {
        LayerEffect::DropShadow(_) => "dropShadow",
        LayerEffect::OuterGlow(_) => "outerGlow",
        LayerEffect::Stroke(_) => "stroke",
        LayerEffect::GradientOverlay(style) if constant_color(style.stops()) => "colorOverlay",
        LayerEffect::GradientOverlay(_) => "gradientOverlay",
        LayerEffect::InnerShadow(_) => "innerShadow",
        LayerEffect::InnerGlow(_) => "innerGlow",
        LayerEffect::Satin(_) => "satin",
        LayerEffect::BevelEmboss(_) => "bevelEmboss",
        _ => "unknown",
    }
}

pub(super) fn style_rank(effect: &LayerEffect) -> u8 {
    match style_key(effect) {
        "dropShadow" => 0,
        "innerShadow" => 1,
        "outerGlow" => 2,
        "innerGlow" => 3,
        "bevelEmboss" => 4,
        "satin" => 5,
        "colorOverlay" => 6,
        "gradientOverlay" => 7,
        "stroke" => 8,
        _ => u8::MAX,
    }
}

fn supported_animation_params(effect: &LayerEffect) -> &'static [&'static str] {
    match effect {
        LayerEffect::DropShadow(_) => &["blurRadius", "spreadRadius"],
        LayerEffect::InnerShadow(_) => &["size", "choke"],
        LayerEffect::OuterGlow(_) => &["size", "spread", "range"],
        LayerEffect::InnerGlow(_) => &["size", "choke", "range"],
        LayerEffect::BevelEmboss(_) => &["depth", "size", "soften", "angle", "altitude"],
        LayerEffect::Satin(_) => &["size"],
        LayerEffect::GradientOverlay(_) => &["opacity"],
        LayerEffect::Stroke(_) => &["width"],
        _ => &[],
    }
}

fn has_parameter_animation(dynamics: &[AnimationGraphEntry], id: EffectId, param: &str) -> bool {
    dynamics.iter().any(|entry| {
        matches!(
            &entry.target,
            PropertyTarget::EffectProperty(target)
                if target.effect_id() == id && target.param_name() == param
        ) && effective_constant(&entry.animator).is_none()
    })
}

fn effective_parameter_constant<'a>(
    dynamics: &'a [AnimationGraphEntry],
    id: EffectId,
    param: &str,
) -> Result<Option<&'a PropertyValue>, &'static str> {
    let mut matches = dynamics.iter().filter(|entry| {
        matches!(
            &entry.target,
            PropertyTarget::EffectProperty(target)
                if target.effect_id() == id && target.param_name() == param
        )
    });
    let Some(entry) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err("duplicate animator target");
    }
    if !entry.dependencies.is_empty()
        || !entry.layer_refs.is_empty()
        || entry.random_seed_target.is_some()
    {
        return Err("dependent animator cannot be exported without executing a runtime");
    }
    Ok(effective_constant(&entry.animator))
}

fn style_animations(
    label: &str,
    id: Option<EffectId>,
    dynamics: &[AnimationGraphEntry],
    mappings: &[(&'static str, &'static str, f64, f64)],
    warnings: &mut Vec<String>,
) -> Vec<NativeLayerStyleAnimation> {
    let Some(id) = id else {
        return Vec::new();
    };
    mappings
        .iter()
        .filter_map(|(param, property, scale, offset)| {
            match super::effects::parameter_track(dynamics, id, param, *scale, *offset) {
                Ok(Some(track)) => {
                    warnings.push(format!(
                        "Layer Style {label} / {param}: native keys require an affine owner source clock; nonlinear Time Remap omits the keys and retains the authored base"
                    ));
                    Some(NativeLayerStyleAnimation { property, track })
                }
                Ok(None) => None,
                Err(reason) => {
                    warnings.push(format!(
                        "Layer Style {label} / {param}: {reason}; authored base retained"
                    ));
                    None
                }
            }
        })
        .collect()
}

fn warn_static_animation(
    label: &str,
    id: Option<EffectId>,
    dynamics: &[AnimationGraphEntry],
    supported: &[&str],
    warnings: &mut Vec<String>,
) {
    let Some(id) = id else {
        return;
    };
    for entry in dynamics {
        if let PropertyTarget::EffectProperty(target) = &entry.target
            && target.effect_id() == id
            && !supported.contains(&target.param_name())
        {
            warnings.push(format!(
                "Layer Style {label} / {}: no compatible native Layer Style key mapping; authored base retained",
                target.param_name()
            ));
        }
    }
}

fn number(value: &Value, name: &str, default: f64) -> f64 {
    value.get(name).and_then(Value::as_f64).unwrap_or(default)
}

fn boolean(value: &Value, name: &str, default: bool) -> bool {
    value.get(name).and_then(Value::as_bool).unwrap_or(default)
}

fn point(value: &Value, name: &str, default: [f64; 2]) -> [f64; 2] {
    value
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() >= 2)
        .and_then(|values| Some([values[0].as_f64()?, values[1].as_f64()?]))
        .filter(|values| values.iter().all(|value| value.is_finite()))
        .unwrap_or(default)
}

fn color(value: &Value, name: &str, default: [f64; 4]) -> [f64; 4] {
    value
        .get(name)
        .and_then(Value::as_array)
        .filter(|values| values.len() >= 4)
        .and_then(|values| {
            Some([
                values[0].as_f64()?,
                values[1].as_f64()?,
                values[2].as_f64()?,
                values[3].as_f64()?,
            ])
        })
        .unwrap_or(default)
}

fn enumeration<T: DeserializeOwned>(value: &Value, name: &str, default: T) -> T {
    value
        .get(name)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or(default)
}

fn blend(value: &Value) -> BlendMode {
    enumeration(value, "blendMode", BlendMode::Normal)
}

fn native_blend(label: &str, value: BlendMode, warnings: &mut Vec<String>) -> BlendMode {
    if encode_blend_mode(value).is_some() {
        value
    } else {
        warnings.push(format!(
            "Layer Style {label}: FX blend mode {value:?} has no verified native Layer Style ordinal; Normal substituted"
        ));
        BlendMode::Normal
    }
}

fn finite(label: &str, name: &str, value: f64, default: f64, warnings: &mut Vec<String>) -> f64 {
    if value.is_finite() {
        value
    } else {
        warnings.push(format!(
            "Layer Style {label} / {name}: non-finite value replaced with {default}"
        ));
        default
    }
}

fn nonnegative(label: &str, name: &str, value: f64, warnings: &mut Vec<String>) -> f64 {
    let value = finite(label, name, value, 0.0, warnings);
    if value < 0.0 {
        warnings.push(format!(
            "Layer Style {label} / {name}: negative value clamped to zero"
        ));
    }
    value.max(0.0)
}

fn unit(label: &str, name: &str, value: f64, warnings: &mut Vec<String>) -> f64 {
    let value = finite(label, name, value, 0.0, warnings);
    let clamped = value.clamp(0.0, 1.0);
    if clamped != value {
        warnings.push(format!(
            "Layer Style {label} / {name}: value outside 0..=1 was clamped"
        ));
    }
    clamped
}

fn normalized_color(label: &str, original: [f64; 4], warnings: &mut Vec<String>) -> [f64; 4] {
    let color = original.map(|value| {
        if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        }
    });
    if color != original {
        warnings.push(format!(
            "Layer Style {label}: color/opacity outside native 0..=1 range was clamped"
        ));
    }
    color
}

fn native_gradient_axis(
    start: [f64; 2],
    end: [f64; 2],
    warnings: &mut Vec<String>,
) -> ([f64; 2], [f64; 2]) {
    let vector = [end[0] - start[0], end[1] - start[1]];
    let length = vector[0].hypot(vector[1]);
    let clamped = length.clamp(10.0, 150.0);
    if clamped == length {
        return (start, end);
    }
    warnings.push(format!(
        "Layer Style Gradient Overlay: FX axis length {length}px is outside native Scale 10..=150; clamped to {clamped}px around the authored center"
    ));
    let center = [(start[0] + end[0]) * 0.5, (start[1] + end[1]) * 0.5];
    let direction = if length > f64::EPSILON {
        [vector[0] / length, vector[1] / length]
    } else {
        [1.0, 0.0]
    };
    let half = clamped * 0.5;
    (
        [
            center[0] - direction[0] * half,
            center[1] - direction[1] * half,
        ],
        [
            center[0] + direction[0] * half,
            center[1] + direction[1] * half,
        ],
    )
}

fn constant_color(stops: &[ShapeGradientStop]) -> bool {
    stops
        .first()
        .is_some_and(|first| stops.iter().all(|stop| stop.color == first.color))
}
