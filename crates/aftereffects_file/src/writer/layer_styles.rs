//! Fresh native AE Layer Styles property groups.

use fx_schema::{
    BevelDirection, BevelStyle, BevelTechnique, GlowSource, LayerStrokePosition, ShapeGradientType,
};

use crate::{
    layer_styles::{
        NativeBevelEmboss, NativeColorOverlay, NativeDropShadow, NativeGradientOverlay,
        NativeInnerGlow, NativeInnerShadow, NativeLayerStyle, NativeLayerStyleAnimation,
        NativeOuterGlow, NativeSatin, NativeStroke, encode_blend_mode,
    },
    rifx::Chunk,
};

use super::{
    AepWriteError,
    views::{self, ValueKind},
};

// These closures capture the composition clock explicitly for each style.
// Never derive it from a layer/source clock: style keys belong to the owner comp.
macro_rules! clocked_style {
    ($clock:ident, $scalar:ident, $toggle:ident, $color:ident, $blend:ident, $animated:ident) => {
        #[allow(unused_variables)]
        let $scalar = |value: f64, range: Option<(f64, f64)>| -> Result<Chunk, AepWriteError> {
            Ok(views::property_with_clock(
                ValueKind::Scalar,
                &[value],
                range,
                None,
                $clock,
            )?)
        };
        #[allow(unused_variables)]
        let $toggle = |value: f64| -> Result<Chunk, AepWriteError> {
            Ok(views::property_with_clock(
                ValueKind::Toggle,
                &[value],
                None,
                None,
                $clock,
            )?)
        };
        #[allow(unused_variables)]
        let $color = |value: [f64; 4]| -> Result<Chunk, AepWriteError> {
            Ok(views::property_with_clock(
                ValueKind::Color,
                &super::rects::native_color(value),
                None,
                None,
                $clock,
            )?)
        };
        #[allow(unused_variables)]
        let $blend = |value: fx_schema::BlendMode| -> Result<Chunk, AepWriteError> {
            let value = encode_blend_mode(value)
                .ok_or(AepWriteError::Invalid("unsupported Layer Style blend mode"))?;
            $toggle(f64::from(value))
        };
        #[allow(unused_variables)]
        let $animated = |value: f64,
                         range: Option<(f64, f64)>,
                         animations: &[NativeLayerStyleAnimation],
                         name: &str|
         -> Result<Chunk, AepWriteError> {
            let track = animations
                .iter()
                .find(|animation| animation.property == name)
                .map(|animation| &animation.track);
            Ok(views::property_with_clock(
                ValueKind::Scalar,
                &[value],
                range,
                track,
                $clock,
            )?)
        };
    };
}

#[cfg(test)]
fn scalar(value: f64, range: Option<(f64, f64)>) -> Result<Chunk, AepWriteError> {
    Ok(views::property(ValueKind::Scalar, &[value], range)?)
}

fn style_group(enabled: bool, entries: Vec<(&str, Chunk)>) -> Result<Chunk, AepWriteError> {
    // Native enabled style groups use 1; bit 1 marks a disabled group.
    // Combining both bits makes Adobe ignore the otherwise populated style.
    Ok(views::group(
        if enabled { 1 } else { 2 },
        "-_0_/-",
        entries,
    )?)
}

fn split_color(value: [f64; 4]) -> ([f64; 4], f64) {
    ([value[0], value[1], value[2], 1.0], value[3] * 100.0)
}

fn polar(offset: [f64; 2]) -> (f64, f64) {
    let distance = offset[0].hypot(offset[1]);
    let angle = offset[1].atan2(-offset[0]).to_degrees().rem_euclid(360.0);
    (angle, distance)
}

fn drop_shadow(
    shadow: &NativeDropShadow,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let (rgb, opacity) = split_color(shadow.color);
    let (angle, distance) = polar(shadow.offset);
    // Invert the existing PAG/libpag-compatible conversion:
    // blur sigma = size * (1 - adjusted spread) / 2;
    // hard spread radius = size * adjusted spread.
    let native_size = shadow.spread + 2.0 * shadow.size;
    let adjusted_spread = if native_size > 0.0 {
        shadow.spread / native_size
    } else {
        0.0
    };
    let spread = if adjusted_spread >= 1.0 {
        100.0
    } else {
        adjusted_spread / 0.8 * 100.0
    };
    style_group(
        shadow.enabled,
        vec![
            ("dropShadow/mode2", blend(shadow.blend_mode)?),
            ("dropShadow/color", color(rgb)?),
            ("dropShadow/opacity", scalar(opacity, Some((0.0, 100.0)))?),
            ("dropShadow/useGlobalAngle", toggle(0.0)?),
            ("dropShadow/localLightingAngle", scalar(angle, None)?),
            (
                "dropShadow/distance",
                scalar(distance, Some((0.0, 30_000.0)))?,
            ),
            (
                "dropShadow/chokeMatte",
                scalar(spread.clamp(0.0, 100.0), Some((0.0, 100.0)))?,
            ),
            (
                "dropShadow/blur",
                animated_scalar(
                    native_size,
                    Some((0.0, 250.0)),
                    &shadow.animations,
                    "dropShadow/blur",
                )?,
            ),
            ("dropShadow/noise", scalar(0.0, Some((0.0, 100.0)))?),
            ("dropShadow/layerConceals", toggle(1.0)?),
        ],
    )
}

fn inner_shadow(
    shadow: &NativeInnerShadow,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let (rgb, opacity) = split_color(shadow.color);
    let (angle, distance) = polar(shadow.offset);
    style_group(
        shadow.enabled,
        vec![
            ("innerShadow/mode2", blend(shadow.blend_mode)?),
            ("innerShadow/color", color(rgb)?),
            ("innerShadow/opacity", scalar(opacity, Some((0.0, 100.0)))?),
            ("innerShadow/useGlobalAngle", toggle(0.0)?),
            ("innerShadow/localLightingAngle", scalar(angle, None)?),
            (
                "innerShadow/distance",
                scalar(distance, Some((0.0, 30_000.0)))?,
            ),
            (
                "innerShadow/chokeMatte",
                animated_scalar(
                    shadow.choke * 100.0,
                    Some((0.0, 100.0)),
                    &shadow.animations,
                    "innerShadow/chokeMatte",
                )?,
            ),
            (
                "innerShadow/blur",
                animated_scalar(
                    shadow.size,
                    Some((0.0, 250.0)),
                    &shadow.animations,
                    "innerShadow/blur",
                )?,
            ),
            ("innerShadow/noise", scalar(0.0, Some((0.0, 100.0)))?),
        ],
    )
}

fn outer_glow(
    glow: &NativeOuterGlow,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let (rgb, opacity) = split_color(glow.color);
    style_group(
        glow.enabled,
        vec![
            ("outerGlow/mode2", blend(glow.blend_mode)?),
            ("outerGlow/opacity", scalar(opacity, Some((0.0, 100.0)))?),
            ("outerGlow/noise", scalar(0.0, Some((0.0, 100.0)))?),
            ("outerGlow/AEColorChoice", toggle(1.0)?),
            ("outerGlow/color", color(rgb)?),
            (
                "outerGlow/gradientSmoothness",
                scalar(100.0, Some((0.0, 100.0)))?,
            ),
            ("outerGlow/glowTechnique", toggle(1.0)?),
            (
                "outerGlow/chokeMatte",
                animated_scalar(
                    glow.spread * 100.0,
                    Some((0.0, 100.0)),
                    &glow.animations,
                    "outerGlow/chokeMatte",
                )?,
            ),
            (
                "outerGlow/blur",
                animated_scalar(
                    glow.size,
                    Some((0.0, 250.0)),
                    &glow.animations,
                    "outerGlow/blur",
                )?,
            ),
            (
                "outerGlow/inputRange",
                animated_scalar(
                    glow.range * 100.0,
                    Some((0.0, 100.0)),
                    &glow.animations,
                    "outerGlow/inputRange",
                )?,
            ),
            ("outerGlow/shadingNoise", scalar(0.0, Some((0.0, 100.0)))?),
        ],
    )
}

fn inner_glow(
    glow: &NativeInnerGlow,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let (rgb, opacity) = split_color(glow.color);
    let source = match glow.source {
        GlowSource::Edge => 1.0,
        GlowSource::Center => 2.0,
    };
    style_group(
        glow.enabled,
        vec![
            ("innerGlow/mode2", blend(glow.blend_mode)?),
            ("innerGlow/opacity", scalar(opacity, Some((0.0, 100.0)))?),
            ("innerGlow/noise", scalar(0.0, Some((0.0, 100.0)))?),
            ("innerGlow/AEColorChoice", toggle(1.0)?),
            ("innerGlow/color", color(rgb)?),
            (
                "innerGlow/gradientSmoothness",
                scalar(100.0, Some((0.0, 100.0)))?,
            ),
            ("innerGlow/glowTechnique", toggle(1.0)?),
            ("innerGlow/innerGlowSource", toggle(source)?),
            (
                "innerGlow/chokeMatte",
                animated_scalar(
                    glow.choke * 100.0,
                    Some((0.0, 100.0)),
                    &glow.animations,
                    "innerGlow/chokeMatte",
                )?,
            ),
            (
                "innerGlow/blur",
                animated_scalar(
                    glow.size,
                    Some((0.0, 250.0)),
                    &glow.animations,
                    "innerGlow/blur",
                )?,
            ),
            (
                "innerGlow/inputRange",
                animated_scalar(
                    glow.range * 100.0,
                    Some((0.0, 100.0)),
                    &glow.animations,
                    "innerGlow/inputRange",
                )?,
            ),
            ("innerGlow/shadingNoise", scalar(0.0, Some((0.0, 100.0)))?),
        ],
    )
}

fn bevel(
    bevel: &NativeBevelEmboss,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let style = match bevel.style {
        BevelStyle::OuterBevel => 1.0,
        BevelStyle::InnerBevel => 2.0,
        BevelStyle::Emboss => 3.0,
        BevelStyle::PillowEmboss => 4.0,
        BevelStyle::StrokeEmboss => 5.0,
    };
    let technique = match bevel.technique {
        BevelTechnique::Smooth => 1.0,
        BevelTechnique::ChiselHard => 2.0,
        BevelTechnique::ChiselSoft => 3.0,
    };
    let direction = match bevel.direction {
        BevelDirection::Up => 1.0,
        BevelDirection::Down => 2.0,
    };
    let (highlight_rgb, highlight_opacity) = split_color(bevel.highlight_color);
    let (shadow_rgb, shadow_opacity) = split_color(bevel.shadow_color);
    style_group(
        bevel.enabled,
        vec![
            ("bevelEmboss/bevelStyle", toggle(style)?),
            ("bevelEmboss/bevelTechnique", toggle(technique)?),
            (
                "bevelEmboss/strengthRatio",
                animated_scalar(
                    bevel.depth * 100.0,
                    Some((0.0, 1000.0)),
                    &bevel.animations,
                    "bevelEmboss/strengthRatio",
                )?,
            ),
            ("bevelEmboss/bevelDirection", toggle(direction)?),
            (
                "bevelEmboss/blur",
                animated_scalar(
                    bevel.size,
                    Some((0.0, 250.0)),
                    &bevel.animations,
                    "bevelEmboss/blur",
                )?,
            ),
            (
                "bevelEmboss/softness",
                animated_scalar(
                    bevel.soften,
                    Some((0.0, 16.0)),
                    &bevel.animations,
                    "bevelEmboss/softness",
                )?,
            ),
            ("bevelEmboss/useGlobalAngle", toggle(0.0)?),
            (
                "bevelEmboss/localLightingAngle",
                animated_scalar(
                    bevel.angle,
                    None,
                    &bevel.animations,
                    "bevelEmboss/localLightingAngle",
                )?,
            ),
            (
                "bevelEmboss/localLightingAltitude",
                animated_scalar(
                    bevel.altitude,
                    Some((0.0, 90.0)),
                    &bevel.animations,
                    "bevelEmboss/localLightingAltitude",
                )?,
            ),
            ("bevelEmboss/highlightMode", toggle(11.0)?),
            ("bevelEmboss/highlightColor", color(highlight_rgb)?),
            (
                "bevelEmboss/highlightOpacity",
                scalar(highlight_opacity, Some((0.0, 100.0)))?,
            ),
            ("bevelEmboss/shadowMode", toggle(5.0)?),
            ("bevelEmboss/shadowColor", color(shadow_rgb)?),
            (
                "bevelEmboss/shadowOpacity",
                scalar(shadow_opacity, Some((0.0, 100.0)))?,
            ),
        ],
    )
}

fn satin(
    satin: &NativeSatin,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let (rgb, opacity) = split_color(satin.color);
    let (angle, distance) = polar(satin.offset);
    style_group(
        satin.enabled,
        vec![
            ("chromeFX/mode2", blend(satin.blend_mode)?),
            ("chromeFX/color", color(rgb)?),
            ("chromeFX/opacity", scalar(opacity, Some((0.0, 100.0)))?),
            ("chromeFX/localLightingAngle", scalar(angle, None)?),
            ("chromeFX/distance", scalar(distance, Some((0.0, 250.0)))?),
            (
                "chromeFX/blur",
                animated_scalar(
                    satin.size,
                    Some((0.0, 250.0)),
                    &satin.animations,
                    "chromeFX/blur",
                )?,
            ),
            (
                "chromeFX/invert",
                toggle(f64::from(u8::from(satin.invert)))?,
            ),
        ],
    )
}

fn color_overlay(
    overlay: &NativeColorOverlay,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let (rgb, opacity) = split_color(overlay.color);
    style_group(
        overlay.enabled,
        vec![
            ("solidFill/mode2", blend(overlay.blend_mode)?),
            ("solidFill/color", color(rgb)?),
            (
                "solidFill/opacity",
                animated_scalar(
                    opacity,
                    Some((0.0, 100.0)),
                    &overlay.animations,
                    "solidFill/opacity",
                )?,
            ),
        ],
    )
}

fn gradient_overlay(
    overlay: &NativeGradientOverlay,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let vector = [
        overlay.end[0] - overlay.start[0],
        overlay.end[1] - overlay.start[1],
    ];
    let scale = vector[0].hypot(vector[1]);
    let angle = vector[1].atan2(vector[0]).to_degrees().rem_euclid(360.0);
    let offset = [
        (overlay.start[0] + overlay.end[0]) * 0.5,
        (overlay.start[1] + overlay.end[1]) * 0.5,
    ];
    let kind = match overlay.gradient_type {
        ShapeGradientType::Linear => 1.0,
        ShapeGradientType::Radial => 2.0,
        ShapeGradientType::Conic => 3.0,
        ShapeGradientType::Reflected => 4.0,
    };
    style_group(
        overlay.enabled,
        vec![
            ("gradientFill/mode2", blend(overlay.blend_mode)?),
            (
                "gradientFill/opacity",
                animated_scalar(
                    overlay.opacity * 100.0,
                    Some((0.0, 100.0)),
                    &overlay.animations,
                    "gradientFill/opacity",
                )?,
            ),
            (
                "gradientFill/gradient",
                super::shapes::gradient_colors(&overlay.stops)?,
            ),
            (
                "gradientFill/gradientSmoothness",
                scalar(100.0, Some((0.0, 100.0)))?,
            ),
            ("gradientFill/angle", scalar(angle, None)?),
            ("gradientFill/type", toggle(kind)?),
            ("gradientFill/reverse", toggle(0.0)?),
            ("gradientFill/align", toggle(1.0)?),
            ("gradientFill/scale", scalar(scale, Some((10.0, 150.0)))?),
            (
                "gradientFill/offset",
                // Unlike plugin Points, style Offset stores raw percentages;
                // unit dimensions retain those values with the native 2D layout.
                super::effect_points::static_property_with_clock(&offset, [1.0, 1.0], clock)?,
            ),
        ],
    )
}

#[cfg(test)]
fn stroke(stroke: &NativeStroke) -> Result<Chunk, AepWriteError> {
    stroke_with_clock(stroke, super::keyframes::PropertyClock::DEFAULT)
}

fn stroke_with_clock(
    stroke: &NativeStroke,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    clocked_style!(clock, scalar, toggle, color, blend, animated_scalar);
    let (rgb, opacity) = split_color(stroke.color);
    let position = match stroke.position {
        LayerStrokePosition::Outside => 1.0,
        LayerStrokePosition::Inside => 2.0,
        LayerStrokePosition::Center => 3.0,
    };
    style_group(
        stroke.enabled,
        vec![
            ("frameFX/mode2", blend(stroke.blend_mode)?),
            ("frameFX/color", color(rgb)?),
            (
                "frameFX/size",
                animated_scalar(
                    stroke.size,
                    Some((1.0, 250.0)),
                    &stroke.animations,
                    "frameFX/size",
                )?,
            ),
            ("frameFX/opacity", scalar(opacity, Some((0.0, 100.0)))?),
            ("frameFX/style", toggle(position)?),
        ],
    )
}

fn encoded(
    style: &NativeLayerStyle,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    Ok(match style {
        NativeLayerStyle::DropShadow(style) => ("dropShadow/enabled", drop_shadow(style, clock)?),
        NativeLayerStyle::InnerShadow(style) => {
            ("innerShadow/enabled", inner_shadow(style, clock)?)
        }
        NativeLayerStyle::OuterGlow(style) => ("outerGlow/enabled", outer_glow(style, clock)?),
        NativeLayerStyle::InnerGlow(style) => ("innerGlow/enabled", inner_glow(style, clock)?),
        NativeLayerStyle::BevelEmboss(style) => ("bevelEmboss/enabled", bevel(style, clock)?),
        NativeLayerStyle::Satin(style) => ("chromeFX/enabled", satin(style, clock)?),
        NativeLayerStyle::ColorOverlay(style) => {
            ("solidFill/enabled", color_overlay(style, clock)?)
        }
        NativeLayerStyle::GradientOverlay(style) => {
            ("gradientFill/enabled", gradient_overlay(style, clock)?)
        }
        NativeLayerStyle::Stroke(style) => ("frameFX/enabled", stroke_with_clock(style, clock)?),
    })
}

fn layer_styles(
    styles: &[NativeLayerStyle],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut encoded_styles = Vec::with_capacity(styles.len());
    for style in styles {
        let encoded = encoded(style, clock)?;
        if encoded_styles
            .iter()
            .any(|(name, _): &(&str, Chunk)| *name == encoded.0)
        {
            return Err(AepWriteError::Invalid("duplicate native Layer Style"));
        }
        encoded_styles.push(encoded);
    }
    let mut entries = vec![(
        "ADBE Blend Options Group",
        views::group(
            1,
            "-_0_/-",
            vec![("ADBE Adv Blend Group", views::group(1, "-_0_/-", vec![])?)],
        )?,
    )];
    for name in [
        "dropShadow/enabled",
        "innerShadow/enabled",
        "outerGlow/enabled",
        "innerGlow/enabled",
        "bevelEmboss/enabled",
        "chromeFX/enabled",
        "solidFill/enabled",
        "gradientFill/enabled",
        "patternFill/enabled",
        "frameFX/enabled",
    ] {
        let group = if let Some((_, group)) = encoded_styles
            .iter()
            .find(|(candidate, _)| *candidate == name)
        {
            group.clone()
        } else {
            views::group(2, "-_0_/-", vec![])?
        };
        entries.push((name, group));
    }
    Ok(views::group(1, "-_0_/-", entries)?)
}

pub(super) fn rebase_animations(
    styles: &mut [NativeLayerStyle],
    clock: &super::source_clock::SourceClockPlan,
) -> Result<(), AepWriteError> {
    for style in styles {
        let animations = match style {
            NativeLayerStyle::DropShadow(style) => &mut style.animations,
            NativeLayerStyle::InnerShadow(style) => &mut style.animations,
            NativeLayerStyle::OuterGlow(style) => &mut style.animations,
            NativeLayerStyle::InnerGlow(style) => &mut style.animations,
            NativeLayerStyle::BevelEmboss(style) => &mut style.animations,
            NativeLayerStyle::Satin(style) => &mut style.animations,
            NativeLayerStyle::ColorOverlay(style) => &mut style.animations,
            NativeLayerStyle::GradientOverlay(style) => &mut style.animations,
            NativeLayerStyle::Stroke(style) => &mut style.animations,
        };
        if clock.has_time_remap() {
            animations.clear();
            continue;
        }
        let active_start = i64::try_from(clock.active_range.start.as_millis()).map_err(|_| {
            AepWriteError::Invalid("Layer Style active-range start exceeds native key range")
        })?;
        for animation in animations {
            for key in &mut animation.track.keys {
                key.time_millis =
                    key.time_millis
                        .checked_sub(active_start)
                        .ok_or(AepWriteError::Invalid(
                            "Layer Style key-time rebase overflowed",
                        ))?;
            }
            clock.rebase_track_times(&mut animation.track)?;
        }
    }
    Ok(())
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn apply(layer: &mut Chunk, styles: &[NativeLayerStyle]) -> Result<(), AepWriteError> {
    apply_with_clock(layer, styles, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn apply_with_clock(
    layer: &mut Chunk,
    styles: &[NativeLayerStyle],
    clock: super::keyframes::PropertyClock,
) -> Result<(), AepWriteError> {
    if styles.is_empty() {
        return Ok(());
    }
    let records = layer.children_mut().ok_or(AepWriteError::Invalid(
        "Layer Style owner is not a native layer",
    ))?;
    let root = records
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .ok_or(AepWriteError::Invalid(
            "Layer Style owner has no property root",
        ))?;
    let Some(index) = root.iter().position(|chunk| {
        chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|data| data.starts_with(b"ADBE Layer Styles\0"))
    }) else {
        let index = root
            .iter()
            .position(|chunk| {
                chunk.id() == *b"tdmn"
                    && chunk
                        .data_payload()
                        .is_some_and(|data| data.starts_with(b"ADBE Group End\0"))
            })
            .unwrap_or(root.len());
        let mut name = [0; 40];
        name[..17].copy_from_slice(b"ADBE Layer Styles");
        root.splice(
            index..index,
            [
                Chunk::data(*b"tdmn", name.to_vec())?,
                layer_styles(styles, clock)?,
            ],
        );
        return Ok(());
    };
    let Some(group) = root.get_mut(index + 1) else {
        return Err(AepWriteError::Invalid(
            "Layer Styles scaffold has no property group",
        ));
    };
    if group.list_kind() != Some(*b"tdgp") {
        return Err(AepWriteError::Invalid("Layer Styles scaffold is malformed"));
    }
    *group = layer_styles(styles, clock)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(entries: Vec<(&str, Chunk)>) -> crate::layer_styles::DecodedLayerStyles {
        let content = vec![
            views::group(
                3,
                "-_0_/-",
                vec![(
                    "ADBE Layer Styles",
                    views::group(3, "-_0_/-", entries).unwrap(),
                )],
            )
            .unwrap(),
        ];
        crate::layer_styles::read(&content, [320.0, 180.0])
    }

    #[test]
    fn malformed_style_does_not_discard_valid_sibling_or_synthesize_defaults() {
        let stroke = NativeStroke {
            enabled: true,
            color: [0.1, 0.2, 0.3, 0.4],
            size: 8.0,
            position: LayerStrokePosition::Center,
            blend_mode: fx_schema::BlendMode::Normal,
            animations: Vec::new(),
        };
        let result = decode(vec![
            ("outerGlow/enabled", Chunk::data(*b"junk", vec![]).unwrap()),
            ("frameFX/enabled", super::stroke(&stroke).unwrap()),
        ]);
        assert_eq!(result.styles.len(), 1);
        assert!(matches!(
            result.styles[0],
            NativeLayerStyle::Stroke(ref actual) if actual == &stroke
        ));
        assert!(result.warnings.iter().any(|warning| {
            warning.contains("malformed property group") && warning.contains("outerGlow")
        }));
    }

    #[test]
    fn thirty_fps_style_animation_and_static_siblings_share_clock() {
        let stroke = NativeStroke {
            enabled: true,
            color: [0.1, 0.2, 0.3, 0.4],
            size: 8.0,
            position: LayerStrokePosition::Center,
            blend_mode: fx_schema::BlendMode::Normal,
            animations: vec![NativeLayerStyleAnimation {
                property: "frameFX/size",
                track: super::super::NumericTrack {
                    keys: vec![super::super::NumericKeyframe {
                        time_millis: 1_100,
                        values: vec![12.0],
                        easing: vec![super::super::KeyframeEasing::Hold],
                        spatial_in: Vec::new(),
                        spatial_out: Vec::new(),
                    }],
                },
            }],
        };
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let native = stroke_with_clock(&stroke, clock).unwrap();
        fn inspect(chunk: &Chunk, clocks: &mut Vec<u32>, keys: &mut Vec<i32>) {
            if chunk.id() == *b"tdb4" {
                let bytes = chunk.data_payload().unwrap();
                clocks.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
            }
            if chunk.id() == *b"ldat" {
                let bytes = chunk.data_payload().unwrap();
                keys.push(i32::from_be_bytes(bytes[..4].try_into().unwrap()));
            }
            if let Some(children) = chunk.children() {
                for child in children {
                    inspect(child, clocks, keys);
                }
            }
        }
        let mut clocks = Vec::new();
        let mut keys = Vec::new();
        inspect(&native, &mut clocks, &mut keys);
        assert_eq!(clocks, vec![30_720; 5]);
        assert_eq!(keys, vec![33_792]);
    }

    #[test]
    fn populated_pattern_overlay_is_diagnosed_without_losing_owner() {
        let pattern = style_group(
            true,
            vec![(
                "patternFill/opacity",
                scalar(35.0, Some((0.0, 100.0))).unwrap(),
            )],
        )
        .unwrap();
        let result = decode(vec![("patternFill/enabled", pattern)]);
        assert!(result.styles.is_empty());
        assert!(result.warnings.iter().any(|warning| {
            warning.contains("Pattern Overlay")
                && warning.contains("no bitmap/pattern style payload")
        }));
    }
}
