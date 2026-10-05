//! FX-side effect capabilities, not a mapping to Adobe-native effect IDs.
use fx_schema::effect::LayerEffect;

/// Persisted lower-camel-case variant discriminator.
pub(crate) fn effect_type(effect: &LayerEffect) -> &'static str {
    match effect {
        LayerEffect::PersonMatte { .. } => "personMatte",
        LayerEffect::DepthMatte { .. } => "depthMatte",
        LayerEffect::GaussianBlur { .. } => "gaussianBlur",
        LayerEffect::Glow { .. } => "glow",
        LayerEffect::DirectionalBlur { .. } => "directionalBlur",
        LayerEffect::PixelMotionBlur { .. } => "pixelMotionBlur",
        LayerEffect::Mosaic { .. } => "mosaic",
        LayerEffect::LookTransform { .. } => "lookTransform",
        LayerEffect::PrimaryGrade(_) => "primaryGrade",
        LayerEffect::TonalColor(_) => "tonalColor",
        LayerEffect::ColorCurves { .. } => "colorCurves",
        LayerEffect::BrightnessContrast { .. } => "brightnessContrast",
        LayerEffect::ShiftChannels { .. } => "shiftChannels",
        LayerEffect::HueSaturation { .. } => "hueSaturation",
        LayerEffect::RadialBlur { .. } => "radialBlur",
        LayerEffect::Levels { .. } => "levels",
        LayerEffect::Bulge { .. } => "bulge",
        LayerEffect::CornerPin { .. } => "cornerPin",
        LayerEffect::MotionTile { .. } => "motionTile",
        LayerEffect::DropShadow(_) => "dropShadow",
        LayerEffect::OuterGlow(_) => "outerGlow",
        LayerEffect::Stroke(_) => "stroke",
        LayerEffect::GradientOverlay(_) => "gradientOverlay",
        LayerEffect::InnerShadow(_) => "innerShadow",
        LayerEffect::InnerGlow(_) => "innerGlow",
        LayerEffect::Satin(_) => "satin",
        LayerEffect::BevelEmboss(_) => "bevelEmboss",
        LayerEffect::Posterize { .. } => "posterize",
        LayerEffect::PosterizeTime { .. } => "posterizeTime",
        LayerEffect::Vignette { .. } => "vignette",
        LayerEffect::FindEdges { .. } => "findEdges",
        LayerEffect::Exposure { .. } => "exposure",
        LayerEffect::Vibrance { .. } => "vibrance",
        LayerEffect::TemperatureTint { .. } => "temperatureTint",
        LayerEffect::Twirl { .. } => "twirl",
        LayerEffect::Ripple { .. } => "ripple",
        LayerEffect::Sharpen { .. } => "sharpen",
        LayerEffect::LumaKey { .. } => "lumaKey",
        LayerEffect::SimpleChoker { .. } => "simpleChoker",
        LayerEffect::Grain { .. } => "grain",
        LayerEffect::WaveWarp { .. } => "waveWarp",
        LayerEffect::LensDistortion { .. } => "lensDistortion",
        LayerEffect::TintTritone { .. } => "tintTritone",
        LayerEffect::GradientRamp { .. } => "gradientRamp",
        LayerEffect::ChromaticAberration { .. } => "chromaticAberration",
        LayerEffect::TurbulentNoise { .. } => "turbulentNoise",
        LayerEffect::Fisheye { .. } => "fisheye",
        LayerEffect::CustomShader { .. } => "customShader",
        LayerEffect::Unsupported(_) => "unsupported",
    }
}

/// FX effects excluded from native conversion; `None` is a candidate, not a
/// claim of an implemented AEP mapping or Adobe render equivalence.
pub(crate) fn unsupported_reason(effect: &LayerEffect) -> Option<&'static str> {
    match effect {
        LayerEffect::PersonMatte { .. } | LayerEffect::DepthMatte { .. } => {
            Some("ML-generated matte sources have no native effect equivalent")
        }
        LayerEffect::LookTransform { .. }
        | LayerEffect::PrimaryGrade(_)
        | LayerEffect::TonalColor(_)
        | LayerEffect::ColorCurves { .. } => {
            Some("reader-specific color operation has no native effect equivalent")
        }
        LayerEffect::CustomShader { .. } => {
            Some("custom WGSL shaders cannot be exported as native effects")
        }
        LayerEffect::ChromaticAberration { .. } => Some(
            "Chromatic channel/alpha assembly failed native RGBA checks; no verified exact editable construction",
        ),
        LayerEffect::Unsupported(_) => {
            Some("unknown future-version effect payload is not editable")
        }
        LayerEffect::GaussianBlur { .. }
        | LayerEffect::Glow { .. }
        | LayerEffect::OuterGlow(_)
        | LayerEffect::Stroke(_)
        | LayerEffect::GradientOverlay(_)
        | LayerEffect::InnerShadow(_)
        | LayerEffect::InnerGlow(_)
        | LayerEffect::Satin(_)
        | LayerEffect::BevelEmboss(_)
        | LayerEffect::DirectionalBlur { .. }
        | LayerEffect::PixelMotionBlur { .. }
        | LayerEffect::Mosaic { .. }
        | LayerEffect::BrightnessContrast { .. }
        | LayerEffect::ShiftChannels { .. }
        | LayerEffect::HueSaturation { .. }
        | LayerEffect::RadialBlur { .. }
        | LayerEffect::Levels { .. }
        | LayerEffect::Bulge { .. }
        | LayerEffect::CornerPin { .. }
        | LayerEffect::MotionTile { .. }
        | LayerEffect::DropShadow(_)
        | LayerEffect::Posterize { .. }
        | LayerEffect::PosterizeTime { .. }
        | LayerEffect::Vignette { .. }
        | LayerEffect::FindEdges { .. }
        | LayerEffect::Exposure { .. }
        | LayerEffect::Vibrance { .. }
        | LayerEffect::TemperatureTint { .. }
        | LayerEffect::Twirl { .. }
        | LayerEffect::Ripple { .. }
        | LayerEffect::Sharpen { .. }
        | LayerEffect::LumaKey { .. }
        | LayerEffect::SimpleChoker { .. }
        | LayerEffect::Grain { .. }
        | LayerEffect::WaveWarp { .. }
        | LayerEffect::LensDistortion { .. }
        | LayerEffect::TintTritone { .. }
        | LayerEffect::GradientRamp { .. }
        | LayerEffect::TurbulentNoise { .. }
        | LayerEffect::Fisheye { .. } => None,
    }
}

/// FX `effectProperty` names; static-only fields and typed enum popups are omitted.
/// Custom shaders have dynamic params and are excluded from native conversion.
#[cfg(test)]
pub(crate) fn animatable_params(effect: &LayerEffect) -> &'static [&'static str] {
    match effect {
        LayerEffect::PersonMatte { .. }
        | LayerEffect::DepthMatte { .. }
        | LayerEffect::LookTransform { .. }
        | LayerEffect::PrimaryGrade(_)
        | LayerEffect::TonalColor(_)
        | LayerEffect::ColorCurves { .. }
        | LayerEffect::ShiftChannels { .. }
        | LayerEffect::PosterizeTime { .. }
        | LayerEffect::CustomShader { .. }
        | LayerEffect::Unsupported(_) => &[],
        LayerEffect::GaussianBlur { .. } => &["blurriness"],
        LayerEffect::Glow { .. } => &["glowThreshold", "glowRadius", "glowIntensity"],
        LayerEffect::DirectionalBlur { .. } => &["direction", "blurLength"],
        LayerEffect::PixelMotionBlur { .. } => &["shutterAngle", "shutterSamples", "vectorDetail"],
        LayerEffect::Mosaic { .. } => &["horizontalBlocks", "verticalBlocks"],
        LayerEffect::BrightnessContrast { .. } => &["brightness", "contrast"],
        LayerEffect::HueSaturation { .. } => &[
            "hue",
            "saturation",
            "lightness",
            "colorize",
            "colorizeHue",
            "colorizeSaturation",
            "colorizeLightness",
        ],
        LayerEffect::RadialBlur { .. } => &["centerX", "centerY", "amount"],
        LayerEffect::Levels { .. } => &[
            "inputBlack",
            "inputWhite",
            "gamma",
            "outputBlack",
            "outputWhite",
        ],
        LayerEffect::Bulge { .. } => &[
            "centerX",
            "centerY",
            "horizontalRadius",
            "verticalRadius",
            "bulgeHeight",
            "pinning",
        ],
        LayerEffect::CornerPin { .. } => &[
            "upperLeftX",
            "upperLeftY",
            "upperRightX",
            "upperRightY",
            "lowerLeftX",
            "lowerLeftY",
            "lowerRightX",
            "lowerRightY",
        ],
        LayerEffect::MotionTile { .. } => &[
            "tileCenterX",
            "tileCenterY",
            "tileWidth",
            "tileHeight",
            "outputWidth",
            "outputHeight",
            "mirrorEdges",
            "phase",
        ],
        LayerEffect::DropShadow(_) => &["enabled", "color", "offset", "blurRadius", "spreadRadius"],
        LayerEffect::OuterGlow(_) => &["enabled", "color", "size", "spread", "range"],
        LayerEffect::Stroke(_) => &["enabled", "color", "width"],
        LayerEffect::GradientOverlay(_) => &["enabled", "opacity", "start", "end"],
        LayerEffect::InnerShadow(_) => &["enabled", "color", "offset", "size", "choke"],
        LayerEffect::InnerGlow(_) => &["enabled", "color", "size", "choke", "range"],
        LayerEffect::Satin(_) => &["enabled", "color", "offset", "size", "invert"],
        LayerEffect::BevelEmboss(_) => &[
            "enabled",
            "depth",
            "size",
            "soften",
            "angle",
            "altitude",
            "highlightColor",
            "shadowColor",
        ],
        LayerEffect::Posterize { .. } => &["levels"],
        LayerEffect::Vignette { .. } => &["amount", "radius", "feather"],
        LayerEffect::FindEdges { .. } => &["invert"],
        LayerEffect::Exposure { .. } => &["exposure", "offset", "gammaCorrection"],
        LayerEffect::Vibrance { .. } => &["vibrance", "saturation"],
        LayerEffect::TemperatureTint { .. } => &["temperature", "tint"],
        LayerEffect::Twirl { .. } => &["angle", "radius", "centerX", "centerY"],
        LayerEffect::Ripple { .. } => &["amplitude", "frequency", "phase", "centerX", "centerY"],
        LayerEffect::Sharpen { .. } => &["amount"],
        LayerEffect::LumaKey { .. } => &["threshold", "softness", "invert"],
        LayerEffect::SimpleChoker { .. } => &["choke"],
        LayerEffect::Grain { .. } => &["intensity", "size", "softness", "aspectRatio", "seed"],
        LayerEffect::WaveWarp { .. } => &["waveHeight", "waveWidth", "direction", "phase"],
        LayerEffect::LensDistortion { .. } => &["amount", "centerX", "centerY"],
        LayerEffect::TintTritone { .. } => &[
            "blackR", "blackG", "blackB", "whiteR", "whiteG", "whiteB", "amount",
        ],
        LayerEffect::GradientRamp { .. } => &[
            "startX", "startY", "endX", "endY", "startR", "startG", "startB", "endR", "endG",
            "endB", "blend", "shape",
        ],
        LayerEffect::ChromaticAberration { .. } => &["amount", "direction"],
        LayerEffect::TurbulentNoise { .. } => &[
            "brightness",
            "contrast",
            "scale",
            "complexity",
            "subInfluence",
            "evolution",
            "rotation",
            "offsetX",
            "offsetY",
            "invert",
            "blend",
        ],
        LayerEffect::Fisheye { .. } => &["amount", "centerX", "centerY"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_warp_rate_is_static_but_is_a_candidate() {
        let effect = LayerEffect::PosterizeTime {
            frame_rate: Some(12.0),
        };
        assert_eq!(effect_type(&effect), "posterizeTime");
        assert_eq!(unsupported_reason(&effect), None);
        assert!(animatable_params(&effect).is_empty());
    }

    #[test]
    fn all_layer_styles_are_native_conversion_candidates() {
        let styles = [
            LayerEffect::DropShadow(Default::default()),
            LayerEffect::OuterGlow(Default::default()),
            LayerEffect::Stroke(Default::default()),
            LayerEffect::GradientOverlay(Default::default()),
            LayerEffect::InnerShadow(Default::default()),
            LayerEffect::InnerGlow(Default::default()),
            LayerEffect::Satin(Default::default()),
            LayerEffect::BevelEmboss(Default::default()),
        ];
        assert!(
            styles
                .iter()
                .all(|style| unsupported_reason(style).is_none())
        );
        assert_eq!(effect_type(&styles[0]), "dropShadow");
        assert_eq!(
            animatable_params(&styles[0]),
            &["enabled", "color", "offset", "blurRadius", "spreadRadius"]
        );
    }

    #[test]
    fn custom_shader_has_no_finite_native_param_catalog() {
        let effect = LayerEffect::CustomShader {
            name: "example".into(),
            description: String::new(),
            wgsl: String::new(),
            params: vec![],
            texture_inputs: vec![],
        };
        assert_eq!(effect_type(&effect), "customShader");
        assert!(unsupported_reason(&effect).is_some());
        assert!(animatable_params(&effect).is_empty());
    }

    #[test]
    fn grain_uses_uniform_name_not_persisted_field_name() {
        let effect = LayerEffect::Grain {
            amount: None,
            size: None,
            softness: None,
            aspect_ratio: None,
            seed: None,
        };
        assert_eq!(
            animatable_params(&effect),
            &["intensity", "size", "softness", "aspectRatio", "seed"]
        );
    }
}
