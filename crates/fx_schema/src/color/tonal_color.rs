//! Versioned display-referred SDR tonal color (ENG1809).

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Stable semantic identifier for the v1 tonal color.
pub const TONAL_COLOR_SEMANTIC_ID: &str = "color.tonalColor.v1";

/// Version fixing the tonal-color operation order and equations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TonalColorSemanticVersion(u32);

impl TonalColorSemanticVersion {
    /// The only tonal-color semantic version understood by this reader.
    pub const V1: Self = Self(1);

    /// Numeric wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for TonalColorSemanticVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value == Self::V1.0 {
            Ok(Self::V1)
        } else {
            Err(serde::de::Error::custom(format!(
                "unsupported tonal-color semanticVersion {value}; expected 1"
            )))
        }
    }
}

#[path = "tonal_color_declaration.rs"]
mod declaration;
crate::define_tonal_color_schema!();

/// Whether a persisted payload is a well-discriminated future semantic version
/// that must remain opaque at the tolerant effect-stack boundary.
pub(crate) fn has_unsupported_tonal_color_semantics(payload: &serde_json::Value) -> bool {
    payload
        .get("semanticVersion")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|version| version > u64::from(TonalColorSemanticVersion::V1.get()))
}
