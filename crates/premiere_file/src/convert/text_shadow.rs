//! Premiere text shadow ↔ one FX `DropShadow` effect on the text layer.
//!
//! Import adds the shadow to the text layer's effect stack, where the FX renderer draws
//! it under the layer's fill and stroke. Export maps one static, enabled,
//! normal-blend `DropShadow` and reports every other effect: Premiere text has
//! no inner shadow, satin, bevel, glow or gradient overlay, and none of a
//! `Stroke` effect's fields (`enabled`, `position`, `blend_mode`, `color`,
//! `width`) can be read outside `fx_schema`. The text's own stroke
//! converts with its document (`convert::text`). A shadow whose values are out
//! of range, or whose text cannot keep it, is omitted as a feature in both
//! directions and the text still converts.
//!
//! The unit mapping is Adobe-verified by AME renders of the
//! `premiere_isolated_text_shadow_stroke` fixture and of seven copies that
//! each change one value of its 45° shadow (blur 0 and 24, size 0 and 8,
//! opacity 100 and 50, distance 48), each fitted on a native 1920×1080 frame:
//! - Angle is degrees clockwise from up, toward where the shadow falls, and
//!   distance is in pixels: offset = distance·(sin, −cos). The 45° shadow
//!   falls up and to the right and the 135° one down and to the right;
//!   distances 24 and 48 fit 24.09 and 48.09 px. `fx_composition` lowers the
//!   same After Effects convention for Directional Blur; the light angle that
//!   `pag::render::styles` reads would mirror these renders across the
//!   135°/315° diagonal.
//! - Blur is a Gaussian of σ = 0.0966 px per unit of blur (`blurRadius`).
//!   Blur 0, 8 and 24 fit σ 0.50, 0.86 and 2.39 px; beyond the 0.50 px edge
//!   of the unblurred shadow that is 0.70 and 2.34 px, within 0.07 px of the
//!   constant. A box kernel fits every blurred render worse.
//! - Size dilates the silhouette before blurring by 0.489 px of radius per
//!   unit (`spreadRadius`): sizes 0, 2 and 8 fit 0.12, 1.03 and 4.01 px,
//!   within 0.04 px of a line with that slope.
//! - Opacity blends a black shadow in linear light with a 2.4 transfer
//!   exponent. The FX renderer blends the color's alpha in encoded values, so the
//!   alpha is 1 − (1 − opacity)^(1/2.4): opacities 50, 80 and 100 fit
//!   encoded alphas 0.246, 0.492 and 1.0, within 0.005 of that curve.
//!
//! Calibration originally used black shadows on 150 px Arial Bold. Existing
//! Premiere 26.5.2 renders at 50 and 300 px, with angle 45°, distance 24, Size 2
//! and Blur 8, have similar frame-pixel horizontal offset and edge-transition
//! width. This supports those observables for that fixed case, not universal
//! font-size invariance. Physical blur sigma and dilation remain unisolated;
//! spread-size, other controls/fonts and transformed text are unverified.
//! Opaque RGB measurements do not establish color/alpha or shader fidelity. The
//! linear-light blend of another color depends on the paint under it, which
//! one alpha cannot express, so its error is not bounded (INFERRED). The
//! effect carries what these values mean. The FX renderer draws a text
//! shadow's σ at 0.6 to 0.75 times a `blurRadius` of 1 to 4 px, and halves
//! and binarizes its spread, so any spread dilates about 2 px; those renderer
//! differences are not compensated here.
//!
//! A graphic Shape's Appearance shadow maps the same way to one `DropShadow`
//! on its shape layer, in the only form that calibration run 1 measured in
//! these units ([`unmeasured_shape_shadow`]); any other form is omitted as a
//! feature and the shape still converts.

use super::effects::{effect_type, EffectIdAllocator};
use crate::{
    error::{ensure, unsupported, Result},
    export_loss::OmissionSink,
    schema::{
        text::{PrRgb, PrShape, PrText, PrVectorMotion, SHAPE_SHADOW_ANGLE},
        text_shadow::PrTextShadow,
        PrAnimatedProperty, PrPropertyAnimation,
    },
    {approximate, omit, Omission, OmissionScope},
};
use fx_schema::{
    AnimationGraph, BlendMode, DropShadow, EffectData, EffectId, EffectPayload, EffectRecord,
    FxItemId, LayerEffect, LayerId, NonNegativeProperty, PropType, PropertyTarget,
};

/// The FX renderer's Gaussian σ, in pixels, per unit of Premiere shadow blur.
const SIGMA_PER_BLUR: f64 = 0.0966;

/// The FX renderer's silhouette dilation radius, in pixels, per unit of Premiere
/// shadow size.
const SPREAD_PER_SIZE: f64 = 0.489;

/// The transfer exponent of the linear light in which Premiere blends a
/// text shadow's opacity.
const LINEAR_LIGHT_GAMMA: f64 = 2.4;

/// The direction an exported zero-distance shadow keeps; any angle draws it
/// identically, and 135° is the inferred Premiere default.
const ANGLE_WITHOUT_DISTANCE: f32 = 135.0;

/// The effect that gives an imported text layer its Premiere shadow; the
/// text belongs to a graphic whose Vector Motion, if kept, is `motion`.
///
/// A shadow with out-of-range values, or one the text cannot keep, is reported
/// with the graphic's record; the text still imports.
pub(super) fn import_text_shadow(
    text: &PrText,
    motion: Option<&PrVectorMotion>,
    record: &str,
    effect_ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> Result<Option<EffectRecord>> {
    let Some(shadow) = text.document.shadow else {
        return Ok(None);
    };
    let reason = unsupported_shadow_reason(text, motion);
    if let Some(reason) = nonuniform_shadow_approximation(text, motion) {
        approximate(omissions, record, format!("editable text DropShadow approximation: {reason}; saved shadow parameters are kept in frame-pixel units rather than a proven native per-axis shadow transform"));
    }
    import_shadow(shadow, "text", reason, record, effect_ids, omissions)
}

/// The effect that gives an imported shape layer its Premiere shadow, as for
/// text, in the form that calibration run 1 measured in the text shadow's
/// units ([`unmeasured_shape_shadow`]); any other form is reported and the
/// shape still imports.
pub(super) fn import_shape_shadow(
    shape: &PrShape,
    motion: Option<&PrVectorMotion>,
    record: &str,
    effect_ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> Result<Option<EffectRecord>> {
    let Some(shadow) = shape.appearance.shadow else {
        return Ok(None);
    };
    let reason =
        unsupported_shape_shadow_reason(shape, motion).or_else(|| unmeasured_shape_shadow(&shadow));
    import_shadow(shadow, "shape", reason, record, effect_ids, omissions)
}

/// The `DropShadow` of `shadow`, cast by an `owner` object that cannot keep
/// it for `reason`, if any; an out-of-range shadow or that reason is
/// reported instead.
fn import_shadow(
    shadow: PrTextShadow,
    owner: &str,
    reason: Option<&'static str>,
    record: &str,
    effect_ids: &mut EffectIdAllocator,
    omissions: &mut Vec<Omission>,
) -> Result<Option<EffectRecord>> {
    let reason = match shadow.validate() {
        Err(error) => Some(error.to_string()),
        Ok(()) => reason.map(str::to_owned),
    };
    if let Some(reason) = reason {
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("{owner} shadow not converted: {reason}"),
        );
        return Ok(None);
    }
    let effect = drop_shadow(shadow)?;
    if let Some(reason) = colored_shadow_blend_approximation(&shadow) {
        approximate(omissions, record, format!("{owner} DropShadow {reason}"));
    }
    Ok(Some(EffectRecord::from_data(&EffectData::Identified {
        id: effect_ids.take(),
        compositing_options: None,
        extensions: Default::default(),
        enabled: true,
        effect: EffectPayload::Known(LayerEffect::DropShadow(effect)),
    })?))
}

/// The opacity mapping is calibrated for a black shadow, whose linear-light
/// blend one encoded alpha reproduces. A translucent shadow of another color
/// blends in linear light against whatever paint is beneath it, which no single
/// alpha expresses, so that conversion is reported as an approximation.
fn colored_shadow_blend_approximation(shadow: &PrTextShadow) -> Option<&'static str> {
    (shadow.color != PrRgb([0, 0, 0]) && shadow.opacity > 0.0 && shadow.opacity < 100.0).then_some(
        "approximation: a translucent non-black shadow blends in linear light in Premiere but with one encoded alpha in FX; the black-shadow opacity calibration is applied and the color error depends on the paint beneath (unmeasured)",
    )
}

fn drop_shadow(shadow: PrTextShadow) -> Result<DropShadow> {
    let PrRgb([red, green, blue]) = shadow.color;
    let channel = |value: u8| f64::from(value) / 255.0;
    let (sin, cos) = f64::from(shadow.angle).to_radians().sin_cos();
    let distance = f64::from(shadow.distance);
    let non_negative = |value: f64, field: &str| {
        NonNegativeProperty::new(value)
            .ok_or_else(|| unsupported(format!("text shadow {field} must be nonnegative")))
    };
    Ok(DropShadow {
        enabled: true,
        color: [
            channel(red),
            channel(green),
            channel(blue),
            alpha_from_opacity(f64::from(shadow.opacity) / 100.0),
        ],
        offset: [distance * sin, -distance * cos],
        blur_radius: non_negative(f64::from(shadow.blur) * SIGMA_PER_BLUR, "blur")?,
        spread_radius: non_negative(f64::from(shadow.size) * SPREAD_PER_SIZE, "size")?,
        blend_mode: BlendMode::Normal,
    })
}

/// The alpha, blended in encoded values, that darkens like a black shadow of
/// Premiere `opacity` (0 to 1) blended in linear light.
fn alpha_from_opacity(opacity: f64) -> f64 {
    1.0 - (1.0 - opacity).powf(LINEAR_LIGHT_GAMMA.recip())
}

/// The Premiere opacity (0 to 1) that inverts [`alpha_from_opacity`].
fn opacity_from_alpha(alpha: f64) -> f64 {
    1.0 - (1.0 - alpha).powf(LINEAR_LIGHT_GAMMA)
}

/// Why `text` cannot keep its shadow in either direction, if it cannot.
///
/// The FX renderer offsets a drop shadow in frame pixels and casts it from the rendered
/// paint. Whether Premiere scales and rotates the shadow with its text layer,
/// and what it casts without a fill, is unverified, so those texts keep their
/// other styling and report the shadow instead of approximating it. Scale or
/// Rotation keys scale or rotate the text for part of its time, and so does a
/// keyed Vector Motion that scales or rotates the whole graphic.
fn unsupported_shadow_reason(
    text: &PrText,
    motion: Option<&PrVectorMotion>,
) -> Option<&'static str> {
    if text.document.fill.is_none() {
        return Some("its text has no fill");
    }
    if text
        .source_text_keys
        .iter()
        .any(|key| key.document.fill.is_none())
    {
        return Some("its text has a Source Text key without a fill");
    }
    if text.horizontal_scale.is_some() {
        None
    } else {
        scaled_or_rotated_text(text).or_else(|| motion_shadow_reason(motion))
    }
}

fn nonuniform_shadow_approximation(
    text: &PrText,
    motion: Option<&PrVectorMotion>,
) -> Option<&'static str> {
    if text.horizontal_scale.is_none()
        || text.document.fill.is_none()
        || text
            .source_text_keys
            .iter()
            .any(|key| key.document.fill.is_none())
    {
        return None;
    }
    scaled_or_rotated_text(text).or_else(|| motion_shadow_reason(motion))
}

/// Why `text`'s own transform is not the unscaled, unrotated one that its
/// shadow and its background box are calibrated on, if it is not: it is
/// scaled or rotated, statically or with keys.
pub(super) fn scaled_or_rotated_text(text: &PrText) -> Option<&'static str> {
    let transform = &text.transform;
    if transform.scale != 100.0
        || text.horizontal_scale.is_some_and(|scale| scale != 100.0)
        || transform.rotation != 0.0
    {
        return Some("its text is scaled or rotated");
    }
    if scales_or_rotates(&text.animations) {
        return Some("its text has Scale or Rotation keys");
    }
    None
}

/// Why `shape` cannot keep its shadow in either direction, if it cannot: as
/// for text, whether Premiere scales and rotates the shadow with its object
/// is unverified.
fn unsupported_shape_shadow_reason(
    shape: &PrShape,
    motion: Option<&PrVectorMotion>,
) -> Option<&'static str> {
    let transform = &shape.transform;
    if transform.scale != 100.0
        || shape.horizontal_scale.is_some_and(|scale| scale != 100.0)
        || transform.rotation != 0.0
    {
        return Some("its shape is scaled or rotated");
    }
    motion_shadow_reason(motion)
}

/// Why a graphic's kept Vector Motion stops its objects' shadows, if it
/// does: it scales or rotates them, statically or with keys.
fn motion_shadow_reason(motion: Option<&PrVectorMotion>) -> Option<&'static str> {
    motion
        .is_some_and(|motion| {
            motion.scale != 100.0 || motion.rotation != 0.0 || scales_or_rotates(&motion.animations)
        })
        .then_some("its graphic's Vector Motion scales or rotates it")
}

/// Why calibration run 1 does not establish a shape shadow in the text
/// shadow's units, if it does not. Its color, its distance (pixels, down and
/// to the right: 40 → (+28, +28) px) and its size (15 and 30 → a 7-8 and a
/// 14-16 px band, where the text rule gives 7.46 and 14.79 px) match the text
/// calibration within the renders' 2 px chroma resolution. Its opacity blend
/// (60 % over red: green and blue as the text's linear-light blend, red 189
/// against 174) and blur (40 → an edge σ of about 2.4 px against the text's
/// 3.86 px) are not established, and Appearance stores no angle.
fn unmeasured_shape_shadow(shadow: &PrTextShadow) -> Option<&'static str> {
    if shadow.opacity != 100.0 {
        Some("a shape shadow's opacity blend is not measured")
    } else if shadow.blur != 0.0 {
        Some("a shape shadow's blur is not measured")
    } else if shadow.distance != 0.0 && shadow.angle != SHAPE_SHADOW_ANGLE {
        Some("a shape shadow falls down and to the right, at 135°")
    } else {
        None
    }
}

fn scales_or_rotates(animations: &[PrPropertyAnimation]) -> bool {
    animations.iter().any(|animation| {
        matches!(
            animation.property(),
            PrAnimatedProperty::UniformScale | PrAnimatedProperty::Rotation
        )
    })
}

/// Export the effect stack of an FX text layer, `effects` of layer
/// `layer_id`: the Premiere shadow it maps to, if any, with every other
/// effect reported under `record`.
///
/// `text` is the Premiere text already built from the layer, and `motion`
/// the Vector Motion that its graphic keeps.
pub(super) fn export_text_effects(
    (effects, layer_id): (&[EffectRecord], LayerId),
    dynamics: &AnimationGraph,
    text: &PrText,
    motion: Option<&PrVectorMotion>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<PrTextShadow> {
    let (effect, label, shadow) = one_drop_shadow(effects, "text", omissions, record)?;
    let exported = premiere_shadow(effect, shadow, layer_id, dynamics, text, motion);
    if let Ok(native) = &exported {
        if let Some(reason) = nonuniform_shadow_approximation(text, motion) {
            approximate(omissions, record, format!("editable native text shadow approximation: {reason}; frame-pixel DropShadow parameters are exported without a proven native per-axis shadow transform"));
        }
        if let Some(reason) = colored_shadow_blend_approximation(native) {
            approximate(omissions, record, format!("native text shadow {reason}"));
        }
    }
    exported_shadow(exported, &label, omissions, record)
}

/// Export the effect stack of an FX shape layer as [`export_text_effects`]
/// does for text: the shape's Premiere shadow, in the form that
/// [`unmeasured_shape_shadow`] allows, with every other effect reported.
pub(super) fn export_shape_effects(
    (effects, layer_id): (&[EffectRecord], LayerId),
    dynamics: &AnimationGraph,
    shape: &PrShape,
    motion: Option<&PrVectorMotion>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<PrTextShadow> {
    let (effect, label, shadow) = one_drop_shadow(effects, "shape", omissions, record)?;
    let reason = unsupported_shape_shadow_reason(shape, motion);
    let exported = shadow_from_effect(effect, shadow, layer_id, dynamics, ("shape", reason))
        .and_then(|shadow| match unmeasured_shape_shadow(&shadow) {
            Some(reason) => Err(unsupported(reason)),
            None => Ok(PrTextShadow {
                angle: SHAPE_SHADOW_ANGLE,
                ..shadow
            }),
        });
    if let Some(reason) = exported
        .as_ref()
        .ok()
        .and_then(colored_shadow_blend_approximation)
    {
        approximate(omissions, record, format!("native shape shadow {reason}"));
    }
    exported_shadow(exported, &label, omissions, record)
}

/// The shadow that export keeps, or `None` with the reason reported.
fn exported_shadow(
    exported: Result<PrTextShadow>,
    label: &str,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<PrTextShadow> {
    match exported {
        Ok(shadow) => Some(shadow),
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("drop shadow {label} was not exported: {error}"),
            );
            None
        }
    }
}

/// The one drop shadow of an `owner` object's effect stack, with its label;
/// every other effect, and more than one drop shadow, is reported.
fn one_drop_shadow<'e>(
    effects: &'e [EffectRecord],
    owner: &str,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<(&'e EffectRecord, String, &'e DropShadow)> {
    let mut shadows = Vec::new();
    for (position, effect) in (1..).zip(effects) {
        let (id, enabled, payload) = record_parts(effect);
        let label = match id {
            Some(id) => id.to_string(),
            None => format!("at stack position {position}"),
        };
        let reason = match payload {
            EffectPayload::Known(LayerEffect::DropShadow(shadow)) => {
                shadows.push((effect, label, shadow));
                continue;
            }
            EffectPayload::Known(LayerEffect::Stroke(_)) => {
                format!(
                    "stroke effect {label} was not exported; only the {owner}'s own stroke converts"
                )
            }
            payload => {
                match style_without_counterpart(payload) {
                    Some(kind) => {
                        format!("{kind} effect {label} was not exported: Premiere {owner} has no {kind}")
                    }
                    None => format!("{} effect {label} was not exported", effect_type(payload)),
                }
            }
        };
        let coverage = super::effects::omitted_coverage_consequence(enabled, payload);
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("{reason}{coverage}"),
        );
    }
    // A disabled shadow draws nothing, so it must not displace the one active
    // shadow that Premiere can keep; it is reported on its own.
    let (active, disabled): (Vec<_>, Vec<_>) = shadows
        .into_iter()
        .partition(|(effect, _, shadow)| record_parts(effect).1 && shadow.enabled);
    let mut shadows = if active.is_empty() {
        disabled
    } else {
        for (_, label, _) in &disabled {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "drop shadow {label} was not exported: unsupported conversion: it is disabled"
                ),
            );
        }
        active
    };
    if shadows.len() > 1 {
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!(
                "{} drop shadows were not exported: Premiere {owner} has one shadow",
                shadows.len()
            ),
        );
        return None;
    }
    shadows.pop()
}

/// A record's id, enabled state and payload. A pre-v13 record has no id, so
/// no animator targets it, and is enabled.
fn record_parts(effect: &EffectRecord) -> (Option<EffectId>, bool, &EffectPayload) {
    match effect.data() {
        EffectData::Identified {
            id,
            enabled,
            effect,
            ..
        } => (Some(*id), *enabled, effect),
        EffectData::Legacy(effect) => (None, true, effect),
    }
}

/// The layer styles that Premiere text has no counterpart for.
fn style_without_counterpart(payload: &EffectPayload) -> Option<&'static str> {
    let EffectPayload::Known(effect) = payload else {
        return None;
    };
    match effect {
        LayerEffect::InnerShadow(_) => Some("inner shadow"),
        LayerEffect::Satin(_) => Some("satin"),
        LayerEffect::BevelEmboss(_) => Some("bevel and emboss"),
        LayerEffect::OuterGlow(_) | LayerEffect::InnerGlow(_) => Some("glow"),
        LayerEffect::GradientOverlay(_) => Some("gradient overlay"),
        _ => None,
    }
}

fn premiere_shadow(
    effect: &EffectRecord,
    shadow: &DropShadow,
    layer_id: LayerId,
    dynamics: &AnimationGraph,
    text: &PrText,
    motion: Option<&PrVectorMotion>,
) -> Result<PrTextShadow> {
    let reason = unsupported_shadow_reason(text, motion);
    shadow_from_effect(effect, shadow, layer_id, dynamics, ("text", reason))
}

/// The Premiere shadow of one static, enabled, normal-blend `DropShadow`
/// whose `owner` object can keep it, unless it cannot for `reason`.
fn shadow_from_effect(
    effect: &EffectRecord,
    shadow: &DropShadow,
    layer_id: LayerId,
    dynamics: &AnimationGraph,
    (owner, reason): (&str, Option<&'static str>),
) -> Result<PrTextShadow> {
    let (_, enabled, _) = record_parts(effect);
    ensure!(enabled && shadow.enabled, "it is disabled");
    ensure!(
        !is_animated(dynamics, layer_id, effect),
        "animated {owner} shadows are unsupported"
    );
    ensure!(
        shadow.blend_mode == BlendMode::Normal,
        "Premiere {owner} shadows blend normally"
    );
    if let Some(reason) = reason {
        return Err(unsupported(reason));
    }
    let [red, green, blue, alpha] = shadow.color;
    ensure!(
        shadow
            .color
            .iter()
            .all(|channel| (0.0..=1.0).contains(channel)),
        "its color channels must be 0 to 1"
    );
    let [x, y] = shadow.offset;
    ensure!(x.is_finite() && y.is_finite(), "its offset must be finite");
    let distance = x.hypot(y);
    let angle = if distance == 0.0 {
        ANGLE_WITHOUT_DISTANCE
    } else {
        // Degrees clockwise from up, inverting `drop_shadow`.
        x.atan2(-y).to_degrees().rem_euclid(360.0) as f32
    };
    // Premiere stores shadows as 32-bit floats; `validate` rejects overflow.
    // The channels were checked to be 0 to 1, so each rounds into `u8`.
    let shadow = PrTextShadow {
        color: PrRgb([red, green, blue].map(|channel| (channel * 255.0).round() as u8)),
        opacity: (opacity_from_alpha(alpha) * 100.0) as f32,
        angle,
        distance: distance as f32,
        size: (shadow.spread_radius.value() / SPREAD_PER_SIZE) as f32,
        blur: (shadow.blur_radius.value() / SIGMA_PER_BLUR) as f32,
    };
    shadow.validate()?;
    Ok(shadow)
}

/// The former layer-style id that a record migrated from the old style stack
/// keeps in its stored `legacySource`, if any.
fn legacy_style_id(effect: &EffectRecord) -> Option<FxItemId> {
    let source = effect.wire_value().get("legacySource")?;
    if source.get("kind")? != "layerStyle" {
        return None;
    }
    source.get("itemId")?.as_u64().map(FxItemId::new)
}

/// Whether an animator targets `effect`. The inline `dropShadow*` layer
/// properties are compatibility addresses of the layer's stacked shadow.
fn is_animated(dynamics: &AnimationGraph, layer_id: LayerId, effect: &EffectRecord) -> bool {
    let (id, _, _) = record_parts(effect);
    let style_id = legacy_style_id(effect);
    dynamics.entries().iter().any(|entry| match &entry.target {
        PropertyTarget::EffectProperty(target) => Some(target.effect_id()) == id,
        PropertyTarget::FxItemProperty(target) => style_id == Some(target.item_id()),
        PropertyTarget::LayerProperty(property) => {
            property.layer_id() == layer_id
                && matches!(
                    property.property_type(),
                    PropType::DropShadowEnabled
                        | PropType::DropShadowColor
                        | PropType::DropShadowOffset
                        | PropType::DropShadowBlurRadius
                        | PropType::DropShadowSpreadRadius
                )
        }
    })
}

#[cfg(test)]
#[path = "tests/text_shadow.rs"]
mod tests;
