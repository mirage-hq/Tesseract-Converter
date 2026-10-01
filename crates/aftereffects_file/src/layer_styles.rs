//! Typed native AE Layer Styles shared by import, lowering, and the writer.
//!
//! Layer Styles are a separate property group from `ADBE Effect Parade`.

use fx_schema::{
    BevelDirection, BevelStyle, BevelTechnique, BlendMode, GlowSource, LayerStrokePosition,
    ShapeGradientStop, ShapeGradientType, layer::ShapePaint,
};

use crate::{
    properties::{self, NumericProperty},
    rifx::Chunk,
    structure_document::shapes::gradient,
};

const DROP_SHADOW: &str = "dropShadow/enabled";
const INNER_SHADOW: &str = "innerShadow/enabled";
const OUTER_GLOW: &str = "outerGlow/enabled";
const INNER_GLOW: &str = "innerGlow/enabled";
const BEVEL_EMBOSS: &str = "bevelEmboss/enabled";
const SATIN: &str = "chromeFX/enabled";
const COLOR_OVERLAY: &str = "solidFill/enabled";
const GRADIENT_OVERLAY: &str = "gradientFill/enabled";
const PATTERN_OVERLAY: &str = "patternFill/enabled";
const STROKE: &str = "frameFX/enabled";
const DEFAULT_GLOW_COLOR: [f64; 4] = [1.0, 1.0, 190.0 / 255.0, 1.0];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeLayerStyleAnimation {
    pub property: &'static str,
    pub track: crate::writer::NumericTrack,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativeLayerStyle {
    DropShadow(NativeDropShadow),
    InnerShadow(NativeInnerShadow),
    OuterGlow(NativeOuterGlow),
    InnerGlow(NativeInnerGlow),
    BevelEmboss(NativeBevelEmboss),
    Satin(NativeSatin),
    ColorOverlay(NativeColorOverlay),
    GradientOverlay(NativeGradientOverlay),
    Stroke(NativeStroke),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeDropShadow {
    pub enabled: bool,
    pub color: [f64; 4],
    pub offset: [f64; 2],
    pub size: f64,
    pub spread: f64,
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeInnerShadow {
    pub enabled: bool,
    pub color: [f64; 4],
    pub offset: [f64; 2],
    pub size: f64,
    pub choke: f64,
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeOuterGlow {
    pub enabled: bool,
    pub color: [f64; 4],
    pub size: f64,
    pub spread: f64,
    pub range: f64,
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeInnerGlow {
    pub enabled: bool,
    pub color: [f64; 4],
    pub size: f64,
    pub choke: f64,
    pub range: f64,
    pub source: GlowSource,
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeBevelEmboss {
    pub enabled: bool,
    pub style: BevelStyle,
    pub technique: BevelTechnique,
    pub depth: f64,
    pub direction: BevelDirection,
    pub size: f64,
    pub soften: f64,
    pub angle: f64,
    pub altitude: f64,
    pub highlight_color: [f64; 4],
    pub shadow_color: [f64; 4],
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeSatin {
    pub enabled: bool,
    pub color: [f64; 4],
    pub offset: [f64; 2],
    pub size: f64,
    pub invert: bool,
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeColorOverlay {
    pub enabled: bool,
    pub color: [f64; 4],
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeGradientOverlay {
    pub enabled: bool,
    pub opacity: f64,
    pub gradient_type: ShapeGradientType,
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub stops: Vec<ShapeGradientStop>,
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeStroke {
    pub enabled: bool,
    pub color: [f64; 4],
    pub size: f64,
    pub position: LayerStrokePosition,
    pub blend_mode: BlendMode,
    pub animations: Vec<NativeLayerStyleAnimation>,
}

pub(crate) struct DecodedLayerStyles {
    pub styles: Vec<NativeLayerStyle>,
    pub source_properties: Vec<Vec<(String, NumericProperty)>>,
    pub warnings: Vec<String>,
}

struct DecodedProperties {
    values: Vec<(String, NumericProperty)>,
    malformed: usize,
}

impl DecodedProperties {
    fn scalar(&self, name: &str, default: f64) -> f64 {
        self.values
            .iter()
            .find(|(candidate, _)| candidate == name)
            .and_then(|(_, numeric)| initial_values(numeric).first().copied())
            .unwrap_or(default)
    }

    fn color(&self, name: &str, default: [f64; 4]) -> [f64; 4] {
        self.values
            .iter()
            .find(|(candidate, _)| candidate == name)
            .and_then(|(_, numeric)| {
                let values = initial_values(numeric);
                (values.len() >= 3).then(|| {
                    [
                        values[0],
                        values[1],
                        values[2],
                        values.get(3).copied().unwrap_or(1.0),
                    ]
                })
            })
            .unwrap_or(default)
    }

    fn malformed_only(&self) -> bool {
        self.values.is_empty() && self.malformed > 0
    }
}

pub(crate) fn read(content: &[Chunk], size: [f64; 2]) -> DecodedLayerStyles {
    let mut result = DecodedLayerStyles {
        styles: Vec::new(),
        source_properties: Vec::new(),
        warnings: Vec::new(),
    };
    let roots = match properties::root_runs(content) {
        Ok(roots) => roots,
        Err(error) => {
            result
                .warnings
                .push(format!("Layer Styles property root malformed: {error}"));
            return result;
        }
    };
    let style_roots: Vec<_> = roots
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Layer Styles")
        .collect();
    let [(.., style_root)] = style_roots.as_slice() else {
        if style_roots.len() > 1 {
            result
                .warnings
                .push("duplicate native Layer Styles groups; styles omitted".into());
        }
        return result;
    };
    let groups = match properties::unique_list(style_root, *b"tdgp").and_then(properties::runs) {
        Ok(groups) => groups,
        Err(error) => {
            result
                .warnings
                .push(format!("Layer Styles group malformed: {error}"));
            return result;
        }
    };
    let mut seen = Vec::new();
    for (name, run) in groups {
        if name == "ADBE Blend Options Group" {
            continue;
        }
        let enabled = properties::group_enabled_or_warn(run, name, &mut result.warnings);
        let leaves = match properties::unique_list(run, *b"tdgp").and_then(properties::runs) {
            Ok(leaves) => leaves,
            Err(error) => {
                result.warnings.push(format!(
                    "Layer Style {name}: malformed property group; style omitted: {error}"
                ));
                continue;
            }
        };
        if leaves.is_empty() && !enabled {
            // Every fresh AE layer contains disabled empty style scaffolds.
            continue;
        }
        if name == PATTERN_OVERLAY {
            result.warnings.push("Layer Style Pattern Overlay: current FX has no bitmap/pattern style payload; pattern semantics omitted, owner and supported siblings retained".into());
            continue;
        }
        let label = style_label(name);
        let Some(label) = label else {
            result.warnings.push(format!(
                "Layer Style {name}: no current editable FX mapping; style omitted, owner and supported siblings retained"
            ));
            continue;
        };
        for (control, _) in &leaves {
            if !known_controls(name).contains(control) {
                result.warnings.push(format!(
                    "Layer Style {label} / {control}: unknown native control omitted"
                ));
            }
        }
        if seen.contains(&name) {
            result.warnings.push(format!(
                "Layer Style {label}: duplicate native occurrence omitted; first occurrence retained"
            ));
            continue;
        }
        let style = match name {
            DROP_SHADOW => decode_drop_shadow(enabled, &leaves, &mut result.warnings),
            INNER_SHADOW => decode_inner_shadow(enabled, &leaves, &mut result.warnings),
            OUTER_GLOW => decode_outer_glow(enabled, &leaves, &mut result.warnings),
            INNER_GLOW => decode_inner_glow(enabled, &leaves, &mut result.warnings),
            BEVEL_EMBOSS => decode_bevel(enabled, &leaves, &mut result.warnings),
            SATIN => decode_satin(enabled, &leaves, &mut result.warnings),
            COLOR_OVERLAY => decode_color_overlay(enabled, &leaves, &mut result.warnings),
            GRADIENT_OVERLAY => {
                decode_gradient_overlay(enabled, &leaves, size, &mut result.warnings)
            }
            STROKE => decode_stroke(enabled, &leaves, &mut result.warnings),
            _ => None,
        };
        if let Some((style, source_properties)) = style {
            seen.push(name);
            result.styles.push(style);
            result.source_properties.push(source_properties);
        }
    }
    result
}

fn style_label(name: &str) -> Option<&'static str> {
    match name {
        DROP_SHADOW => Some("Drop Shadow"),
        INNER_SHADOW => Some("Inner Shadow"),
        OUTER_GLOW => Some("Outer Glow"),
        INNER_GLOW => Some("Inner Glow"),
        BEVEL_EMBOSS => Some("Bevel and Emboss"),
        SATIN => Some("Satin"),
        COLOR_OVERLAY => Some("Color Overlay"),
        GRADIENT_OVERLAY => Some("Gradient Overlay"),
        STROKE => Some("Stroke"),
        _ => None,
    }
}

fn known_controls(name: &str) -> &'static [&'static str] {
    match name {
        DROP_SHADOW => &[
            "dropShadow/mode2",
            "dropShadow/color",
            "dropShadow/opacity",
            "dropShadow/useGlobalAngle",
            "dropShadow/localLightingAngle",
            "dropShadow/distance",
            "dropShadow/chokeMatte",
            "dropShadow/blur",
            "dropShadow/noise",
            "dropShadow/layerConceals",
        ],
        INNER_SHADOW => &[
            "innerShadow/mode2",
            "innerShadow/color",
            "innerShadow/opacity",
            "innerShadow/useGlobalAngle",
            "innerShadow/localLightingAngle",
            "innerShadow/distance",
            "innerShadow/chokeMatte",
            "innerShadow/blur",
            "innerShadow/noise",
        ],
        OUTER_GLOW => &[
            "outerGlow/mode2",
            "outerGlow/opacity",
            "outerGlow/noise",
            "outerGlow/AEColorChoice",
            "outerGlow/color",
            "outerGlow/gradient",
            "outerGlow/gradientSmoothness",
            "outerGlow/glowTechnique",
            "outerGlow/chokeMatte",
            "outerGlow/blur",
            "outerGlow/inputRange",
            "outerGlow/shadingNoise",
        ],
        INNER_GLOW => &[
            "innerGlow/mode2",
            "innerGlow/opacity",
            "innerGlow/noise",
            "innerGlow/AEColorChoice",
            "innerGlow/color",
            "innerGlow/gradient",
            "innerGlow/gradientSmoothness",
            "innerGlow/glowTechnique",
            "innerGlow/innerGlowSource",
            "innerGlow/chokeMatte",
            "innerGlow/blur",
            "innerGlow/inputRange",
            "innerGlow/shadingNoise",
        ],
        BEVEL_EMBOSS => &[
            "bevelEmboss/bevelStyle",
            "bevelEmboss/bevelTechnique",
            "bevelEmboss/strengthRatio",
            "bevelEmboss/bevelDirection",
            "bevelEmboss/blur",
            "bevelEmboss/softness",
            "bevelEmboss/useGlobalAngle",
            "bevelEmboss/localLightingAngle",
            "bevelEmboss/localLightingAltitude",
            "bevelEmboss/highlightMode",
            "bevelEmboss/highlightColor",
            "bevelEmboss/highlightOpacity",
            "bevelEmboss/shadowMode",
            "bevelEmboss/shadowColor",
            "bevelEmboss/shadowOpacity",
        ],
        SATIN => &[
            "chromeFX/mode2",
            "chromeFX/color",
            "chromeFX/opacity",
            "chromeFX/localLightingAngle",
            "chromeFX/distance",
            "chromeFX/blur",
            "chromeFX/invert",
        ],
        COLOR_OVERLAY => &["solidFill/mode2", "solidFill/color", "solidFill/opacity"],
        GRADIENT_OVERLAY => &[
            "gradientFill/mode2",
            "gradientFill/opacity",
            "gradientFill/gradient",
            "gradientFill/gradientSmoothness",
            "gradientFill/angle",
            "gradientFill/type",
            "gradientFill/reverse",
            "gradientFill/align",
            "gradientFill/scale",
            "gradientFill/offset",
        ],
        STROKE => &[
            "frameFX/mode2",
            "frameFX/color",
            "frameFX/size",
            "frameFX/opacity",
            "frameFX/style",
        ],
        _ => &[],
    }
}

fn decode_properties(
    label: &str,
    leaves: &[(&str, &[Chunk])],
    custom: &[&str],
    warnings: &mut Vec<String>,
) -> DecodedProperties {
    let mut decoded = DecodedProperties {
        values: Vec::new(),
        malformed: 0,
    };
    for (name, run) in leaves {
        if custom.contains(name) {
            continue;
        }
        if decoded.values.iter().any(|(existing, _)| existing == name) {
            warnings.push(format!(
                "Layer Style {label} / {name}: duplicate property; first value retained"
            ));
            continue;
        }
        // Gradient Offset uses AE's two-component Point descriptor, including
        // its integer marker, but stores continuous percentage coordinates.
        let decode = if *name == "gradientFill/offset" {
            properties::read_effect_point
        } else {
            properties::read_numeric
        };
        let numeric = match properties::unique_list(run, *b"tdbs").and_then(decode) {
            Ok(numeric) => numeric,
            Err(error) => {
                decoded.malformed += 1;
                warnings.push(format!("Layer Style {label} / {name}: {error}"));
                continue;
            }
        };
        if numeric.animated || numeric.expression_enabled {
            warnings.push(format!(
                "Layer Style {label} / {name}: static native summary retains the initial value; structural import handles compatible tracks separately"
            ));
        }
        decoded.values.push(((*name).to_owned(), numeric));
    }
    decoded
}

fn reject_malformed_only(
    label: &str,
    decoded: &DecodedProperties,
    warnings: &mut Vec<String>,
) -> bool {
    if decoded.malformed_only() {
        warnings.push(format!(
            "Layer Style {label}: all supplied controls were malformed; style omitted rather than synthesized from defaults"
        ));
        true
    } else {
        false
    }
}

fn shadow_color(
    decoded: &DecodedProperties,
    color_name: &str,
    opacity_name: &str,
    default_color: [f64; 4],
    default_opacity: f64,
) -> [f64; 4] {
    let mut color = decoded.color(color_name, default_color);
    color[3] *= decoded.scalar(opacity_name, default_opacity) / 100.0;
    color
}

fn polar_offset(angle: f64, distance: f64) -> [f64; 2] {
    let radians = angle.to_radians();
    [-distance * radians.cos(), distance * radians.sin()]
}

fn decode_drop_shadow(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Drop Shadow";
    let decoded = decode_properties(label, leaves, &[], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    warn_nondefault(&decoded, label, "dropShadow/useGlobalAngle", 0.0, warnings);
    warn_nondefault(&decoded, label, "dropShadow/noise", 0.0, warnings);
    warn_nondefault(&decoded, label, "dropShadow/layerConceals", 1.0, warnings);
    let authored_size = decoded.scalar("dropShadow/blur", 5.0);
    let authored_spread = decoded.scalar("dropShadow/chokeMatte", 0.0) / 100.0;
    let adjusted_spread = if authored_spread >= 1.0 {
        authored_spread
    } else {
        authored_spread * 0.8
    };
    let mut style = NativeDropShadow {
        enabled,
        color: shadow_color(
            &decoded,
            "dropShadow/color",
            "dropShadow/opacity",
            [0.0, 0.0, 0.0, 1.0],
            75.0,
        ),
        offset: polar_offset(
            decoded.scalar("dropShadow/localLightingAngle", 120.0),
            decoded.scalar("dropShadow/distance", 5.0),
        ),
        size: authored_size * (1.0 - adjusted_spread) * 0.5,
        spread: authored_size * adjusted_spread,
        blend_mode: blend_mode(&decoded, label, "dropShadow/mode2", 5.0, warnings),
        animations: Vec::new(),
    };
    normalize_shadow(
        label,
        &mut style.color,
        &mut style.offset,
        &mut style.size,
        &mut style.spread,
        warnings,
    )?;
    Some((NativeLayerStyle::DropShadow(style), decoded.values))
}

fn decode_inner_shadow(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Inner Shadow";
    let decoded = decode_properties(label, leaves, &[], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    warnings.push("Layer Style Inner Shadow: the current FX renderer uses a bounded single-pass approximation and composites as Normal; the editable native blend mode is retained but not rendered".into());
    warn_nondefault(&decoded, label, "innerShadow/useGlobalAngle", 0.0, warnings);
    warn_nondefault(&decoded, label, "innerShadow/noise", 0.0, warnings);
    let mut style = NativeInnerShadow {
        enabled,
        color: shadow_color(
            &decoded,
            "innerShadow/color",
            "innerShadow/opacity",
            [0.0, 0.0, 0.0, 1.0],
            75.0,
        ),
        offset: polar_offset(
            decoded.scalar("innerShadow/localLightingAngle", 120.0),
            decoded.scalar("innerShadow/distance", 5.0),
        ),
        size: decoded.scalar("innerShadow/blur", 5.0),
        choke: decoded.scalar("innerShadow/chokeMatte", 0.0) / 100.0,
        blend_mode: blend_mode(&decoded, label, "innerShadow/mode2", 5.0, warnings),
        animations: Vec::new(),
    };
    normalize_shadow(
        label,
        &mut style.color,
        &mut style.offset,
        &mut style.size,
        &mut style.choke,
        warnings,
    )?;
    style.choke = style.choke.clamp(0.0, 1.0);
    Some((NativeLayerStyle::InnerShadow(style), decoded.values))
}

fn decode_outer_glow(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Outer Glow";
    let decoded = decode_properties(label, leaves, &["outerGlow/gradient"], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    for (name, default) in [
        ("outerGlow/noise", 0.0),
        ("outerGlow/AEColorChoice", 1.0),
        ("outerGlow/gradientSmoothness", 100.0),
        ("outerGlow/glowTechnique", 1.0),
        ("outerGlow/shadingNoise", 0.0),
    ] {
        warn_nondefault(&decoded, label, name, default, warnings);
    }
    let mut style = NativeOuterGlow {
        enabled,
        color: shadow_color(
            &decoded,
            "outerGlow/color",
            "outerGlow/opacity",
            DEFAULT_GLOW_COLOR,
            75.0,
        ),
        size: decoded.scalar("outerGlow/blur", 5.0),
        spread: decoded.scalar("outerGlow/chokeMatte", 0.0) / 100.0,
        range: decoded.scalar("outerGlow/inputRange", 50.0) / 100.0,
        blend_mode: blend_mode(&decoded, label, "outerGlow/mode2", 11.0, warnings),
        animations: Vec::new(),
    };
    normalize_glow(
        label,
        &mut style.color,
        &mut style.size,
        &mut style.spread,
        &mut style.range,
        warnings,
    )?;
    Some((NativeLayerStyle::OuterGlow(style), decoded.values))
}

fn decode_inner_glow(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Inner Glow";
    let decoded = decode_properties(label, leaves, &["innerGlow/gradient"], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    warnings.push("Layer Style Inner Glow: the current FX renderer uses range as intensity and Center as inverse edge coverage, and composites as Normal; these differ from AE's spatial contour/blend semantics".into());
    for (name, default) in [
        ("innerGlow/noise", 0.0),
        ("innerGlow/AEColorChoice", 1.0),
        ("innerGlow/gradientSmoothness", 100.0),
        ("innerGlow/glowTechnique", 1.0),
        ("innerGlow/shadingNoise", 0.0),
    ] {
        warn_nondefault(&decoded, label, name, default, warnings);
    }
    let source = match decoded.scalar("innerGlow/innerGlowSource", 1.0).round() as i64 {
        1 => GlowSource::Edge,
        2 => GlowSource::Center,
        value => {
            warnings.push(format!(
                "Layer Style {label} / Source {value}: unsupported ordinal; Edge retained"
            ));
            GlowSource::Edge
        }
    };
    let mut style = NativeInnerGlow {
        enabled,
        color: shadow_color(
            &decoded,
            "innerGlow/color",
            "innerGlow/opacity",
            DEFAULT_GLOW_COLOR,
            75.0,
        ),
        size: decoded.scalar("innerGlow/blur", 5.0),
        choke: decoded.scalar("innerGlow/chokeMatte", 0.0) / 100.0,
        range: decoded.scalar("innerGlow/inputRange", 50.0) / 100.0,
        source,
        blend_mode: blend_mode(&decoded, label, "innerGlow/mode2", 11.0, warnings),
        animations: Vec::new(),
    };
    normalize_glow(
        label,
        &mut style.color,
        &mut style.size,
        &mut style.choke,
        &mut style.range,
        warnings,
    )?;
    Some((NativeLayerStyle::InnerGlow(style), decoded.values))
}

fn decode_bevel(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Bevel and Emboss";
    let decoded = decode_properties(label, leaves, &[], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    warnings.push("Layer Style Bevel and Emboss: current FX uses one bounded alpha-edge lighting pass; outer halo, contour, texture and exact AE highlight/shadow compositing are approximated".into());
    warn_nondefault(&decoded, label, "bevelEmboss/useGlobalAngle", 0.0, warnings);
    warn_nondefault(&decoded, label, "bevelEmboss/highlightMode", 11.0, warnings);
    warn_nondefault(&decoded, label, "bevelEmboss/shadowMode", 5.0, warnings);
    let style = match decoded.scalar("bevelEmboss/bevelStyle", 2.0).round() as i64 {
        1 => BevelStyle::OuterBevel,
        2 => BevelStyle::InnerBevel,
        3 => BevelStyle::Emboss,
        4 => BevelStyle::PillowEmboss,
        5 => BevelStyle::StrokeEmboss,
        value => {
            warnings.push(format!(
                "Layer Style {label} / Style {value}: unsupported ordinal; Inner Bevel retained"
            ));
            BevelStyle::InnerBevel
        }
    };
    let technique = match decoded.scalar("bevelEmboss/bevelTechnique", 1.0).round() as i64 {
        1 => BevelTechnique::Smooth,
        2 => BevelTechnique::ChiselHard,
        3 => BevelTechnique::ChiselSoft,
        value => {
            warnings.push(format!(
                "Layer Style {label} / Technique {value}: unsupported ordinal; Smooth retained"
            ));
            BevelTechnique::Smooth
        }
    };
    let direction = match decoded.scalar("bevelEmboss/bevelDirection", 1.0).round() as i64 {
        1 => BevelDirection::Up,
        2 => BevelDirection::Down,
        value => {
            warnings.push(format!(
                "Layer Style {label} / Direction {value}: unsupported ordinal; Up retained"
            ));
            BevelDirection::Up
        }
    };
    let mut native = NativeBevelEmboss {
        enabled,
        style,
        technique,
        depth: decoded.scalar("bevelEmboss/strengthRatio", 100.0) / 100.0,
        direction,
        size: decoded.scalar("bevelEmboss/blur", 5.0),
        soften: decoded.scalar("bevelEmboss/softness", 0.0),
        angle: decoded.scalar("bevelEmboss/localLightingAngle", 120.0),
        altitude: decoded.scalar("bevelEmboss/localLightingAltitude", 30.0),
        highlight_color: shadow_color(
            &decoded,
            "bevelEmboss/highlightColor",
            "bevelEmboss/highlightOpacity",
            [1.0; 4],
            75.0,
        ),
        shadow_color: shadow_color(
            &decoded,
            "bevelEmboss/shadowColor",
            "bevelEmboss/shadowOpacity",
            [0.0, 0.0, 0.0, 1.0],
            75.0,
        ),
        animations: Vec::new(),
    };
    if !all_finite(&[
        native.depth,
        native.size,
        native.soften,
        native.angle,
        native.altitude,
    ]) || !all_finite(&native.highlight_color)
        || !all_finite(&native.shadow_color)
    {
        warnings.push(format!(
            "Layer Style {label}: non-finite mapped value; style omitted"
        ));
        return None;
    }
    native.depth = native.depth.max(0.0);
    native.size = native.size.max(0.0);
    native.soften = native.soften.max(0.0);
    clamp_color(&mut native.highlight_color);
    clamp_color(&mut native.shadow_color);
    Some((NativeLayerStyle::BevelEmboss(native), decoded.values))
}

fn decode_satin(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Satin";
    let decoded = decode_properties(label, leaves, &[], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    warnings.push("Layer Style Satin: current FX uses a bounded contour-interference approximation and composites as Normal; the editable native blend mode is retained but not rendered".into());
    let mut style = NativeSatin {
        enabled,
        color: shadow_color(
            &decoded,
            "chromeFX/color",
            "chromeFX/opacity",
            [0.0, 0.0, 0.0, 1.0],
            50.0,
        ),
        offset: polar_offset(
            decoded.scalar("chromeFX/localLightingAngle", 19.0),
            decoded.scalar("chromeFX/distance", 11.0),
        ),
        size: decoded.scalar("chromeFX/blur", 14.0),
        invert: decoded.scalar("chromeFX/invert", 1.0) != 0.0,
        blend_mode: blend_mode(&decoded, label, "chromeFX/mode2", 5.0, warnings),
        animations: Vec::new(),
    };
    let mut unused = 0.0;
    normalize_shadow(
        label,
        &mut style.color,
        &mut style.offset,
        &mut style.size,
        &mut unused,
        warnings,
    )?;
    Some((NativeLayerStyle::Satin(style), decoded.values))
}

fn decode_color_overlay(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Color Overlay";
    let decoded = decode_properties(label, leaves, &[], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    let mut color = shadow_color(
        &decoded,
        "solidFill/color",
        "solidFill/opacity",
        [1.0, 0.0, 0.0, 1.0],
        100.0,
    );
    if !all_finite(&color) {
        warnings.push(format!(
            "Layer Style {label}: non-finite mapped value; style omitted"
        ));
        return None;
    }
    clamp_color(&mut color);
    Some((
        NativeLayerStyle::ColorOverlay(NativeColorOverlay {
            enabled,
            color,
            blend_mode: blend_mode(&decoded, label, "solidFill/mode2", 1.0, warnings),
            animations: Vec::new(),
        }),
        decoded.values,
    ))
}

fn decode_gradient_overlay(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    size: [f64; 2],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Gradient Overlay";
    let decoded = decode_properties(label, leaves, &["gradientFill/gradient"], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    warn_nondefault(
        &decoded,
        label,
        "gradientFill/gradientSmoothness",
        100.0,
        warnings,
    );
    if decoded.scalar("gradientFill/align", 1.0) == 0.0 {
        warnings.push("Layer Style Gradient Overlay / Align with Layer: document-space alignment has no FX equivalent; layer-local geometry retained".into());
    }
    let gradient_type = match decoded.scalar("gradientFill/type", 1.0).round() as i64 {
        1 => ShapeGradientType::Linear,
        2 => ShapeGradientType::Radial,
        3 => ShapeGradientType::Conic,
        4 => ShapeGradientType::Reflected,
        value => {
            warnings.push(format!(
                "Layer Style {label} / Style {value}: unsupported ordinal; Linear retained"
            ));
            ShapeGradientType::Linear
        }
    };
    let custom = leaves
        .iter()
        .find_map(|(name, run)| (*name == "gradientFill/gradient").then_some(*run));
    let decoded_gradient = match custom {
        Some(run) => match gradient::decode(run, 1, [0.0; 2], [1.0, 0.0]) {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!(
                    "Layer Style {label} / Colors: {error}; style omitted"
                ));
                return None;
            }
        },
        None => gradient::ae_default(1, [0.0; 2], [1.0, 0.0]),
    };
    warnings.extend(
        decoded_gradient
            .warnings
            .into_iter()
            .map(|warning| format!("Layer Style {label} / Colors: {warning}")),
    );
    if decoded_gradient.animated {
        warnings.push("Layer Style Gradient Overlay / Colors: animated stops have no FX stop-animation target; first stop set retained".into());
    }
    let ShapePaint::Gradient { mut stops, .. } = decoded_gradient.paint else {
        return None;
    };
    if decoded.scalar("gradientFill/reverse", 0.0) != 0.0 {
        stops.reverse();
        for stop in &mut stops {
            stop.offset = 1.0 - stop.offset;
        }
    }
    let offset = decoded
        .values
        .iter()
        .find(|(name, _)| name == "gradientFill/offset")
        .and_then(|(_, property)| {
            let values = initial_values(property);
            (values.len() >= 2).then(|| [values[0], values[1]])
        })
        .unwrap_or([0.0; 2]);
    let (start, end) = gradient_points(
        size,
        decoded.scalar("gradientFill/angle", 90.0),
        decoded.scalar("gradientFill/scale", 100.0),
        offset,
    )?;
    let opacity = decoded.scalar("gradientFill/opacity", 100.0) / 100.0;
    if !opacity.is_finite() || !all_finite(&start) || !all_finite(&end) {
        warnings.push(format!(
            "Layer Style {label}: non-finite mapped value; style omitted"
        ));
        return None;
    }
    Some((
        NativeLayerStyle::GradientOverlay(NativeGradientOverlay {
            enabled,
            opacity: opacity.clamp(0.0, 1.0),
            gradient_type,
            start,
            end,
            stops,
            blend_mode: blend_mode(&decoded, label, "gradientFill/mode2", 1.0, warnings),
            animations: Vec::new(),
        }),
        decoded.values,
    ))
}

fn decode_stroke(
    enabled: bool,
    leaves: &[(&str, &[Chunk])],
    warnings: &mut Vec<String>,
) -> Option<(NativeLayerStyle, Vec<(String, NumericProperty)>)> {
    let label = "Stroke";
    let decoded = decode_properties(label, leaves, &[], warnings);
    if reject_malformed_only(label, &decoded, warnings) {
        return None;
    }
    let position = match decoded.scalar("frameFX/style", 1.0).round() as i64 {
        1 => LayerStrokePosition::Outside,
        2 => LayerStrokePosition::Inside,
        3 => LayerStrokePosition::Center,
        value => {
            warnings.push(format!(
                "Layer Style {label} / Position {value}: unsupported ordinal; Outside retained"
            ));
            LayerStrokePosition::Outside
        }
    };
    let mut color = shadow_color(
        &decoded,
        "frameFX/color",
        "frameFX/opacity",
        [1.0, 0.0, 0.0, 1.0],
        100.0,
    );
    let size = decoded.scalar("frameFX/size", 3.0);
    if !all_finite(&color) || !size.is_finite() {
        warnings.push(format!(
            "Layer Style {label}: non-finite mapped value; style omitted"
        ));
        return None;
    }
    clamp_color(&mut color);
    Some((
        NativeLayerStyle::Stroke(NativeStroke {
            enabled,
            color,
            size: size.max(0.0),
            position,
            blend_mode: blend_mode(&decoded, label, "frameFX/mode2", 1.0, warnings),
            animations: Vec::new(),
        }),
        decoded.values,
    ))
}

fn warn_nondefault(
    decoded: &DecodedProperties,
    label: &str,
    name: &str,
    default: f64,
    warnings: &mut Vec<String>,
) {
    let value = decoded.scalar(name, default);
    if (value - default).abs() > f64::EPSILON {
        warnings.push(format!(
            "Layer Style {label} / {name}: native value {value} has no current FX equivalent; canonical {default} semantics substituted"
        ));
    }
}

fn blend_mode(
    decoded: &DecodedProperties,
    label: &str,
    name: &str,
    default: f64,
    warnings: &mut Vec<String>,
) -> BlendMode {
    let native = decoded.scalar(name, default).round();
    let mapped = if (0.0..=f64::from(u8::MAX)).contains(&native) {
        decode_blend_mode(native as u8)
    } else {
        None
    };
    mapped.unwrap_or_else(|| {
        warnings.push(format!(
            "Layer Style {label} / Blend Mode {native}: unsupported native mode; Normal retained"
        ));
        BlendMode::Normal
    })
}

fn normalize_shadow(
    label: &str,
    color: &mut [f64; 4],
    offset: &mut [f64; 2],
    size: &mut f64,
    spread: &mut f64,
    warnings: &mut Vec<String>,
) -> Option<()> {
    if !all_finite(color) || !all_finite(offset) || !size.is_finite() || !spread.is_finite() {
        warnings.push(format!(
            "Layer Style {label}: non-finite mapped value; style omitted"
        ));
        return None;
    }
    clamp_color(color);
    *size = size.max(0.0);
    *spread = spread.max(0.0);
    Some(())
}

fn normalize_glow(
    label: &str,
    color: &mut [f64; 4],
    size: &mut f64,
    spread: &mut f64,
    range: &mut f64,
    warnings: &mut Vec<String>,
) -> Option<()> {
    if !all_finite(color) || !size.is_finite() || !spread.is_finite() || !range.is_finite() {
        warnings.push(format!(
            "Layer Style {label}: non-finite mapped value; style omitted"
        ));
        return None;
    }
    clamp_color(color);
    *size = size.max(0.0);
    *spread = spread.clamp(0.0, 1.0);
    *range = range.clamp(0.0, 1.0);
    Some(())
}

fn gradient_points(
    _size: [f64; 2],
    angle: f64,
    scale: f64,
    offset: [f64; 2],
) -> Option<([f64; 2], [f64; 2])> {
    if !angle.is_finite() || !scale.is_finite() || !all_finite(&offset) {
        return None;
    }
    // Match the existing PAG Layer Style conversion: native 100% spans 100
    // layer-local pixels around Offset, independently of owner dimensions.
    let center = offset;
    let length = scale.max(0.0);
    let radians = angle.to_radians();
    let axis = [radians.cos() * length * 0.5, radians.sin() * length * 0.5];
    Some((
        [center[0] - axis[0], center[1] - axis[1]],
        [center[0] + axis[0], center[1] + axis[1]],
    ))
}

fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite())
}

fn clamp_color(color: &mut [f64; 4]) {
    color
        .iter_mut()
        .for_each(|value| *value = value.clamp(0.0, 1.0));
}

fn initial_values(property: &NumericProperty) -> &[f64] {
    property
        .keyframes
        .first()
        .map_or(property.values.as_slice(), |key| key.values.as_slice())
}

/// Bounded native Layer Style blend ordinals; Adobe export acceptance is unverified.
pub(crate) const fn decode_blend_mode(value: u8) -> Option<BlendMode> {
    match value {
        1 => Some(BlendMode::Normal),
        5 => Some(BlendMode::Multiply),
        11 => Some(BlendMode::Screen),
        _ => None,
    }
}

/// Bounded native Layer Style blend ordinals; Adobe export acceptance is unverified.
pub(crate) const fn encode_blend_mode(value: BlendMode) -> Option<u8> {
    match value {
        BlendMode::Normal => Some(1),
        BlendMode::Multiply => Some(5),
        BlendMode::Screen => Some(11),
        _ => None,
    }
}
