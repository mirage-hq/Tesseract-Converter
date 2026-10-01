//! Versioned display-referred SDR primary grading (ENG-1825).

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Stable semantic identifier for the v1 primary grade.
pub const PRIMARY_GRADE_SEMANTIC_ID: &str = "color.primaryGrade.v1";

/// Version fixing the primary-grade operation order and equations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct PrimaryGradeSemanticVersion(u32);

impl PrimaryGradeSemanticVersion {
    /// The only primary-grade semantic version understood by this reader.
    pub const V1: Self = Self(1);

    /// Numeric wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for PrimaryGradeSemanticVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        if value == Self::V1.0 {
            Ok(Self::V1)
        } else {
            Err(serde::de::Error::custom(format!(
                "unsupported primary-grade semanticVersion {value}; expected 1"
            )))
        }
    }
}

#[path = "primary_grade_declaration.rs"]
mod declaration;
crate::define_primary_grade_schema!();

/// Whether a persisted payload is a well-discriminated future semantic version
/// that must remain opaque at the tolerant effect-stack boundary.
pub(crate) fn has_unsupported_primary_grade_semantics(payload: &serde_json::Value) -> bool {
    payload
        .get("semanticVersion")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|version| version != u64::from(PrimaryGradeSemanticVersion::V1.get()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_controls_reject_non_finite_out_of_range_and_future_version() {
        let neutral = serde_json::to_value(PrimaryGrade::default()).expect("serialize grade");
        assert_eq!(
            serde_json::from_value::<PrimaryGrade>(neutral.clone()).expect("read neutral grade"),
            PrimaryGrade::default()
        );

        for (field, value) in [
            ("temperature", 1.01),
            ("exposure", 5.01),
            ("contrast", -2.01),
        ] {
            let mut invalid = neutral.clone();
            invalid[field] = serde_json::json!(value);
            assert!(serde_json::from_value::<PrimaryGrade>(invalid).is_err());
        }
        let mut non_finite = neutral.clone();
        non_finite["vibrance"] = serde_json::Value::String("NaN".to_owned());
        assert!(serde_json::from_value::<PrimaryGrade>(non_finite).is_err());

        let mut future = neutral;
        future["semanticVersion"] = serde_json::json!(2);
        assert!(has_unsupported_primary_grade_semantics(&future));
        assert!(serde_json::from_value::<PrimaryGrade>(future).is_err());
    }
}
