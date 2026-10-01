//! Canonical, versioned color-transform semantics.
//!
//! ENG-1810 defines persisted reader contracts only. Evaluation and LUT asset
//! loading intentionally belong to later rollout phases.

use std::{fmt, str::FromStr};

use crate::AssetId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod declaration;
mod primary_grade;

pub(crate) use primary_grade::has_unsupported_primary_grade_semantics;
pub use primary_grade::{
    PrimaryGrade, PrimaryGradeError, PrimaryGradeSemanticVersion, PRIMARY_GRADE_SEMANTIC_ID,
};

/// Phase-0 display-referred working/output encoding.
pub const SDR_REC709_DISPLAY_ENCODING_ID: &str = "jerboa:sdr-rec709-display:v1";
/// Stable semantic identifier for a source-owned Input Transform.
pub const INPUT_TRANSFORM_SEMANTIC_ID: &str = "color.inputTransform.v1";
/// Stable semantic identifier for an ordered Look Transform.
pub const LOOK_TRANSFORM_SEMANTIC_ID: &str = "color.lookTransform.v1";

/// A syntactically valid, versioned color-encoding identifier.
///
/// Readers preserve unregistered identifiers so a newer writer's document can
/// survive a round trip. Writer activation may impose a registry check later.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ColorEncodingId(String);

impl ColorEncodingId {
    /// Parse `{authority}:{name}:v{positive integer}` using lower-kebab tokens.
    pub fn new(value: impl Into<String>) -> Result<Self, ColorEncodingIdError> {
        let value = value.into();
        if valid_encoding_id(&value) {
            Ok(Self(value))
        } else {
            Err(ColorEncodingIdError(value))
        }
    }

    /// Borrow the persisted identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ColorEncodingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ColorEncodingId {
    type Err = ColorEncodingIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ColorEncodingId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Invalid canonical color-encoding identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorEncodingIdError(String);

impl fmt::Display for ColorEncodingIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "color encoding id `{}` must match lower-kebab authority:name:vN with N >= 1",
            self.0
        )
    }
}

impl std::error::Error for ColorEncodingIdError {}

fn valid_encoding_id(value: &str) -> bool {
    let mut parts = value.split(':');
    let Some(authority) = parts.next() else {
        return false;
    };
    let Some(name) = parts.next() else {
        return false;
    };
    let Some(version) = parts.next() else {
        return false;
    };
    if parts.next().is_some() || !valid_lower_kebab(authority) || !valid_lower_kebab(name) {
        return false;
    }
    let Some(digits) = version.strip_prefix('v') else {
        return false;
    };
    !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits.parse::<u32>().is_ok_and(|number| number > 0)
}

fn valid_lower_kebab(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|token| {
            !token.is_empty()
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// Semantic version fixed by the Phase-0 color contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ColorTransformSemanticVersion(u32);

impl ColorTransformSemanticVersion {
    /// The only supported semantic version.
    pub const V1: Self = Self(1);

    /// Numeric wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ColorTransformSemanticVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value == Self::V1.0 {
            Ok(Self::V1)
        } else {
            Err(serde::de::Error::custom(format!(
                "unsupported color semanticVersion {value}; expected 1"
            )))
        }
    }
}

/// Interpolation fixed by the Phase-0 LUT contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum ColorLutInterpolation {
    /// Filterable 3D texture interpolation.
    Trilinear,
}

crate::define_color_transform_schema!();

impl ColorTransform {
    /// Build a strict v1 3D-LUT transform.
    #[must_use]
    pub fn lut3d(
        asset_id: AssetId,
        input_encoding: ColorEncodingId,
        output_encoding: ColorEncodingId,
    ) -> Self {
        Self::Lut3d {
            asset_id,
            input_encoding,
            output_encoding,
            interpolation: ColorLutInterpolation::Trilinear,
        }
    }
}

impl InputTransform {
    /// Build a semantic-version-1 Input Transform.
    #[must_use]
    pub const fn v1(transform: ColorTransform) -> Self {
        Self {
            semantic_version: ColorTransformSemanticVersion::V1,
            transform,
        }
    }
}

/// A finite normalized Look Transform mix.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(transparent)]
pub struct NormalizedColorMix(f64);

impl NormalizedColorMix {
    /// Fully bypassed/neutral mix.
    pub const BYPASSED: Self = Self(0.0);
    /// Fully applied default mix.
    pub const FULL: Self = Self(1.0);

    /// Validate a normalized mix.
    pub fn new(value: f64) -> Result<Self, NormalizedColorMixError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(NormalizedColorMixError(value))
        }
    }

    /// Numeric wire value.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for NormalizedColorMix {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Invalid normalized Look Transform mix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizedColorMixError(f64);

impl fmt::Display for NormalizedColorMixError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "color mix {} must be finite and in 0..=1",
            self.0
        )
    }
}

impl std::error::Error for NormalizedColorMixError {}

pub(crate) const fn default_color_mix() -> NormalizedColorMix {
    NormalizedColorMix::FULL
}

crate::define_persisted_input_transform_schema!();

impl PersistedInputTransform {
    /// Wrap a supported canonical Input Transform.
    #[must_use]
    pub fn supported(transform: InputTransform) -> Self {
        Self(PersistedInputTransformValue::Supported(transform))
    }

    /// Borrow the supported payload, or `None` for an opaque future payload.
    #[must_use]
    pub fn as_supported(&self) -> Option<&InputTransform> {
        match &self.0 {
            PersistedInputTransformValue::Supported(transform) => Some(transform),
            PersistedInputTransformValue::Unsupported(_) => None,
        }
    }

    /// Borrow the exact unsupported JSON payload retained by the reader.
    #[must_use]
    pub fn unsupported_payload(&self) -> Option<&serde_json::Value> {
        match &self.0 {
            PersistedInputTransformValue::Supported(_) => None,
            PersistedInputTransformValue::Unsupported(payload) => Some(payload),
        }
    }

    pub(crate) fn from_persisted_value(
        payload: serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        if has_unsupported_color_semantics(&payload) {
            return Ok(Self(PersistedInputTransformValue::Unsupported(payload)));
        }
        serde_json::from_value(payload).map(Self::supported)
    }
}

impl From<InputTransform> for PersistedInputTransform {
    fn from(transform: InputTransform) -> Self {
        Self::supported(transform)
    }
}

impl Serialize for PersistedInputTransform {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.0 {
            PersistedInputTransformValue::Supported(transform) => transform.serialize(serializer),
            PersistedInputTransformValue::Unsupported(payload) => payload.serialize(serializer),
        }
    }
}

/// Whether a persisted semantic payload belongs to a future, well-discriminated
/// contract rather than being malformed v1 data.
pub fn has_unsupported_color_semantics(payload: &serde_json::Value) -> bool {
    payload
        .get("semanticVersion")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|version| version != 1)
        || payload
            .pointer("/transform/type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|transform_type| transform_type != "lut3d")
        || payload
            .pointer("/transform/interpolation")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|interpolation| interpolation != "trilinear")
}
