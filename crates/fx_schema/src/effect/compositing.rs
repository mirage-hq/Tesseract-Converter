//! AE Compositing Options and the versioned secondary-colour qualifier.
use super::LayerEffect;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Instance-level unknown fields use the established open-object preservation contract.
pub type EffectInstanceExtensions = crate::font_metadata::AssetDataExtensions;

macro_rules! bounded_scalar {
    ($name:ident, $min:expr, $max:expr, $default:expr) => {
        /// A finite, bounded qualifier/compositing scalar.
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
        #[serde(transparent)]
        #[ts(type = "number")]
        pub struct $name(f64);
        impl $name {
            /// Reject non-finite values and values outside the inclusive domain.
            pub fn new(value: f64) -> Option<Self> {
                (value.is_finite() && ($min..=$max).contains(&value)).then_some(Self(value))
            }
            /// Numeric value in the declared units.
            pub fn value(self) -> f64 {
                self.0
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self($default)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = f64::deserialize(deserializer)?;
                Self::new(value).ok_or_else(|| {
                    serde::de::Error::custom(concat!(
                        stringify!($name),
                        " must be finite and within ",
                        stringify!($min),
                        "..=",
                        stringify!($max)
                    ))
                })
            }
        }
    };
}
bounded_scalar!(UnitInterval, 0.0, 1.0, 1.0);
bounded_scalar!(HueDegrees, 0.0, 360.0, 0.0);
bounded_scalar!(DenoiseRadius, 0.0, 10.0, 0.0);
bounded_scalar!(QualifierBlurRadius, 0.0, 100.0, 0.0);
bounded_scalar!(QualifierChoke, -50.0, 50.0, 0.0);

/// Typed inputs accept only v1; persisted readers retain future versions as no-ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(transparent)]
#[ts(type = "1")]
pub struct QualifierVersion(u64);
impl QualifierVersion {
    /// Only v1 may be authored or evaluated by this release.
    pub fn is_supported(self) -> bool {
        self.0 == 1
    }
}
impl Default for QualifierVersion {
    fn default() -> Self {
        Self(1)
    }
}
impl<'de> Deserialize<'de> for QualifierVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u64::deserialize(deserializer)? {
            1 => Ok(Self(1)),
            _ => Err(serde::de::Error::custom(
                "qualifier semanticVersion must be 1",
            )),
        }
    }
}

/// Hue selection in degrees; 0 and 360 represent the same hue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HueSelection {
    pub center: HueDegrees,
    pub width: HueDegrees,
    pub softness: HueDegrees,
}

/// Ordered inclusive interval in saturation or Rec.709 luminance.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QualifierRange {
    pub low: UnitInterval,
    pub high: UnitInterval,
    pub softness: UnitInterval,
}
impl<'de> Deserialize<'de> for QualifierRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            low: UnitInterval,
            high: UnitInterval,
            softness: UnitInterval,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.low.value() > wire.high.value() {
            return Err(serde::de::Error::custom(
                "qualifier range low must be <= high",
            ));
        }
        Ok(Self {
            low: wire.low,
            high: wire.high,
            softness: wire.softness,
        })
    }
}

/// Refinement in layer effect-space pixels: select → denoise → choke → blur → invert.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualifierRefine {
    pub denoise: DenoiseRadius,
    pub blur_radius: QualifierBlurRadius,
    pub choke: QualifierChoke,
}

/// Selection reads unpremultiplied stack-input RGB; alpha-zero pixels have matte zero.
#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QualifierParams {
    pub semantic_version: QualifierVersion,
    pub hue: HueSelection,
    pub saturation: QualifierRange,
    pub luminance: QualifierRange,
    pub refine: QualifierRefine,
    pub invert: bool,
    /// Opaque future persisted payload, never accepted by typed authoring inputs.
    #[serde(skip)]
    #[ts(skip)]
    pub(crate) preserved_future: Option<serde_json::Value>,
}

impl Serialize for QualifierParams {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(raw) = &self.preserved_future {
            return raw.serialize(serializer);
        }
        serde_json::json!({
            "semanticVersion": self.semantic_version, "hue": self.hue,
            "saturation": self.saturation, "luminance": self.luminance,
            "refine": self.refine, "invert": self.invert
        })
        .serialize(serializer)
    }
}

/// Persisted-only reader: retain an unknown qualifier verbatim while lowering
/// to an empty, non-inverted, unrefined selection. Typed options remain strict.
pub fn deserialize_persisted_compositing_options<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<EffectCompositingOptions>, D::Error> {
    use serde::de::Error as _;
    let mut raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    let future = raw.as_ref().and_then(|v| v.get("qualifier")).and_then(|q| {
        q.get("semanticVersion")
            .and_then(serde_json::Value::as_u64)
            .filter(|version| *version != 1)
            .map(|version| (version, q.clone()))
    });
    if let Some((version, original)) = future {
        raw.as_mut()
            .expect("future qualifier has an options object")["qualifier"] = serde_json::json!({
            "semanticVersion":1, "hue":{"center":0,"width":0,"softness":0},
            "saturation":{"low":0,"high":1,"softness":0},
            "luminance":{"low":0,"high":1,"softness":0},
            "refine":{"denoise":0,"blurRadius":0,"choke":0}, "invert":false
        });
        let mut options: EffectCompositingOptions =
            serde_json::from_value(raw.expect("options object")).map_err(D::Error::custom)?;
        let qualifier = options.qualifier.as_mut().expect("placeholder qualifier");
        qualifier.semantic_version = QualifierVersion(version);
        qualifier.preserved_future = Some(original);
        return Ok(Some(options));
    }
    raw.map(serde_json::from_value)
        .transpose()
        .map_err(D::Error::custom)
}

/// Optional instance-level AE compositing controls. Missing is the historical path.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectCompositingOptions {
    #[serde(default)]
    pub mask_references: Vec<crate::FxItemId>,
    #[serde(default)]
    pub effect_opacity: UnitInterval,
    /// Boxed: the qualifier is large and keeps the stored effect record compact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<Box<QualifierParams>>,
}
impl EffectCompositingOptions {
    /// Neutral controls must not introduce a new render operation or pass.
    pub fn is_neutral(&self) -> bool {
        self.mask_references.is_empty()
            && self.effect_opacity.value() == 1.0
            && self.qualifier.is_none()
    }
    /// Revalidate public compound fields before authoring; scalar types are already bounded.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(qualifier) = &self.qualifier {
            if !qualifier.semantic_version.is_supported() || qualifier.preserved_future.is_some() {
                return Err("qualifier semanticVersion must be 1");
            }
            if qualifier.saturation.low.value() > qualifier.saturation.high.value()
                || qualifier.luminance.low.value() > qualifier.luminance.high.value()
            {
                return Err("qualifier low must be <= high");
            }
        }
        Ok(())
    }
}

/// Exhaustive property-class policy, shared by every writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectCompositingClass {
    PixelColour,
    Neighbourhood,
    Geometry,
    Matte,
    Temporal,
    Generator,
    Opaque,
}
#[doc(hidden)]
#[macro_export]
macro_rules! define_effect_compositing_class {
    () => {
        /// Adding an effect variant requires an explicit compositing policy.
        pub fn compositing_class(&self) -> $crate::effect::EffectCompositingClass {
            use $crate::effect::EffectCompositingClass as Class;
            use LayerEffect::*;
            match self {
                LookTransform { .. }
                | PrimaryGrade(_)
                | TonalColor(_)
                | ColorCurves { .. }
                | BrightnessContrast { .. }
                | ShiftChannels { .. }
                | HueSaturation { .. }
                | Levels { .. }
                | Posterize { .. }
                | Vignette { .. }
                | Exposure { .. }
                | Vibrance { .. }
                | TemperatureTint { .. }
                | Grain { .. }
                | TintTritone { .. } => Class::PixelColour,
                GaussianBlur { .. }
                | Glow { .. }
                | DirectionalBlur { .. }
                | Mosaic { .. }
                | RadialBlur { .. }
                | FindEdges { .. }
                | Sharpen { .. }
                | ChromaticAberration { .. } => Class::Neighbourhood,
                Bulge { .. }
                | CornerPin { .. }
                | MotionTile { .. }
                | DropShadow(_)
                | OuterGlow(_)
                | Stroke(_)
                | GradientOverlay(_)
                | InnerShadow(_)
                | InnerGlow(_)
                | Satin(_)
                | BevelEmboss(_)
                | Twirl { .. }
                | Ripple { .. }
                | WaveWarp { .. }
                | LensDistortion { .. }
                | Fisheye { .. } => Class::Geometry,
                PersonMatte { .. } | DepthMatte { .. } | LumaKey { .. } | SimpleChoker { .. } => {
                    Class::Matte
                }
                PixelMotionBlur { .. } | PosterizeTime { .. } => Class::Temporal,
                GradientRamp { .. } | TurbulentNoise { .. } => Class::Generator,
                CustomShader { .. } | Unsupported(_) => Class::Opaque,
            }
        }
    };
}
impl super::LayerEffect {
    crate::define_effect_compositing_class!();
}
impl EffectCompositingClass {
    /// AE mask references and effect opacity are allowed on generators, but not keys or temporal/opaque ops.
    pub fn allows_compositing(self) -> bool {
        matches!(
            self,
            Self::PixelColour | Self::Neighbourhood | Self::Geometry | Self::Generator
        )
    }
    /// HSL qualifies only operations that colour or sample existing image pixels.
    pub fn allows_qualifier(self) -> bool {
        matches!(self, Self::PixelColour | Self::Neighbourhood)
    }
}
