//! Bounded, source-bound values evaluated by After Effects for enabled expressions.
//!
//! The sidecar contains measurements only. It never carries source expression code or
//! asks the converter to evaluate an expression runtime.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Sidecar format version for ordinary Transform/Effect captures.
pub const EXPRESSION_SAMPLES_VERSION: u32 = 2;
/// Path-qualified numeric Shape identities require this explicit version.
pub const SHAPE_EXPRESSION_SAMPLES_VERSION: u32 = 3;
const LEGACY_EXPRESSION_SAMPLES_VERSION: u32 = 1;
/// Required spacing between consecutive samples.
pub const EXPRESSION_SAMPLE_INTERVAL_MS: u32 = 1;
const MAX_NATIVE_CLOCK_DEVIATION_SECONDS: f64 = 0.0005;

/// Native capture coverage bound into a version-2 sidecar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureScope {
    /// Every composition in the source was captured.
    AllCompositions,
    /// Only one selected root and its reachable graph were captured.
    SelectedComposition {
        /// Native root composition item ID.
        root_composition_id: u32,
    },
}

/// Stable identity of an expression-enabled native property.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PropertyIdentity {
    /// A native Transform property identified by its AE match name.
    Transform {
        /// Native AE property match name.
        match_name: String,
    },
    /// A numeric Shape leaf qualified from the layer's native Contents root.
    Shape {
        /// Ordered native property indices and match names, including root and leaf.
        path: Vec<ShapePathSegment>,
    },
    /// A parameter on an ordinary Effect occurrence.
    Effect {
        /// One-based native AE occurrence index in the layer's ordinary-effect list.
        index: u32,
        /// Native AE parameter match name.
        match_name: String,
    },
    /// A numeric property of one Mask Atom. Converter-evaluated only: Adobe
    /// expression sidecars never carry this identity and reject it.
    Mask {
        /// One-based native Mask Atom index in the layer's Mask Parade.
        index: u32,
        /// Native AE mask property match name.
        match_name: String,
    },
    /// A layer's Source Text. Converter-evaluated only; Adobe expression
    /// sidecars never carry this identity and reject it.
    SourceText {},
    /// A numeric property of one Text Animator (not a selector). Converter-evaluated
    /// only: Adobe expression sidecars never carry this identity and reject it.
    TextAnimator {
        /// One-based native Text Animator index.
        animator: u32,
        /// Native AE animator property match name.
        match_name: String,
    },
}

/// One segment of a root-anchored native Shape property path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapePathSegment {
    /// One-based native property index within this segment's parent.
    pub index: u32,
    /// Native AE match name, not the editable display name.
    pub match_name: String,
}

/// Values for one successfully evaluated expression-enabled property.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluatedProperty {
    /// Native composition item ID containing the layer.
    pub(crate) composition_id: u32,
    /// Native layer ID within the composition.
    pub(crate) layer_id: u32,
    /// Native property identity.
    pub(crate) property: PropertyIdentity,
    /// Parent-composition timestamp of the first value.
    pub(crate) start_ms: i64,
    /// Actual native expression-clock timestamp aligned with each value in v2.
    #[serde(default)]
    pub(crate) sample_times_seconds: Vec<f64>,
    /// One value for each requested integral millisecond, including both endpoints.
    pub(crate) values: Vec<Vec<f64>>,
    /// Internal converter observations are not Adobe sidecars and use a frame grid.
    #[serde(skip)]
    pub(crate) frame_sampled: bool,
}

impl EvaluatedProperty {
    /// Native composition item ID containing the layer.
    pub fn composition_id(&self) -> u32 {
        self.composition_id
    }

    /// Native layer ID within the composition.
    pub fn layer_id(&self) -> u32 {
        self.layer_id
    }

    /// Native property identity.
    pub fn property(&self) -> &PropertyIdentity {
        &self.property
    }

    /// Parent-composition timestamp of the first value.
    pub fn start_ms(&self) -> i64 {
        self.start_ms
    }

    /// Actual native timestamps, or an empty slice for a legacy v1 record.
    pub fn sample_times_seconds(&self) -> &[f64] {
        &self.sample_times_seconds
    }

    /// Values aligned with the requested integral-millisecond grid.
    pub fn values(&self) -> &[Vec<f64>] {
        &self.values
    }

    /// Value at a requested integral-ms grid point.
    ///
    /// In v2 the aligned actual native timestamp is available through
    /// [`Self::sample_times_seconds`].
    pub fn value_at_ms(&self, time_ms: i64) -> Option<&[f64]> {
        if self.frame_sampled {
            let time = time_ms as f64 / 1_000.0;
            let index = self
                .sample_times_seconds
                .binary_search_by(|sample| sample.total_cmp(&time))
                .ok()?;
            return self.values.get(index).map(Vec::as_slice);
        }
        let offset = time_ms.checked_sub(self.start_ms)?;
        let index = usize::try_from(offset).ok()?;
        self.values.get(index).map(Vec::as_slice)
    }
}

/// Native evaluation failure for one expression-enabled property.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionEvaluationError {
    /// Native composition item ID containing the layer.
    pub(crate) composition_id: u32,
    /// Native layer ID within the composition.
    pub(crate) layer_id: u32,
    /// Native property identity.
    pub(crate) property: PropertyIdentity,
    /// Error reported while evaluating the native property.
    pub(crate) message: String,
}

impl ExpressionEvaluationError {
    /// Native composition item ID containing the layer.
    pub fn composition_id(&self) -> u32 {
        self.composition_id
    }

    /// Native layer ID within the composition.
    pub fn layer_id(&self) -> u32 {
        self.layer_id
    }

    /// Native property identity.
    pub fn property(&self) -> &PropertyIdentity {
        &self.property
    }

    /// Error reported while evaluating the native property.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Versioned expression values tied to the exact source AEP bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionSamples {
    version: u32,
    source_sha256: String,
    sample_interval_ms: u32,
    #[serde(default)]
    capture_scope: Option<CaptureScope>,
    pub(crate) properties: Vec<EvaluatedProperty>,
    pub(crate) errors: Vec<ExpressionEvaluationError>,
    /// Converter-evaluated Source Text strings; never part of Adobe sidecars.
    #[serde(skip)]
    pub(crate) texts: Vec<EvaluatedText>,
}

/// Source Text strings evaluated by the converter at composition-clock times.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EvaluatedText {
    pub(crate) composition_id: u32,
    pub(crate) layer_id: u32,
    pub(crate) sample_times_seconds: Vec<f64>,
    pub(crate) texts: Vec<String>,
}

impl Default for ExpressionSamples {
    fn default() -> Self {
        Self {
            version: EXPRESSION_SAMPLES_VERSION,
            source_sha256: String::new(),
            sample_interval_ms: EXPRESSION_SAMPLE_INTERVAL_MS,
            capture_scope: None,
            properties: Vec::new(),
            errors: Vec::new(),
            texts: Vec::new(),
        }
    }
}

impl ExpressionSamples {
    /// Parses and validates a sidecar for the exact source AEP bytes.
    pub fn from_json_for_source(
        bytes: &[u8],
        source: &[u8],
    ) -> Result<Self, ExpressionSamplesError> {
        if bytes.len() > 128 * 1024 * 1024 {
            // Inspect only the version without allocating property/sample vectors.
            // Legacy sidecars deliberately retain their existing quota behavior.
            #[derive(Deserialize)]
            struct Version {
                version: u32,
            }
            let header: Version = serde_json::from_slice(bytes)?;
            if header.version == SHAPE_EXPRESSION_SAMPLES_VERSION {
                return Err(ExpressionSamplesError::CaptureResourceLimit);
            }
        }
        let samples: Self = serde_json::from_slice(bytes)?;
        if samples.version == SHAPE_EXPRESSION_SAMPLES_VERSION
            && (samples.properties.len() + samples.errors.len() > 4096
                || samples
                    .properties
                    .iter()
                    .any(|property| property.values.len() > 60001)
                || samples
                    .properties
                    .iter()
                    .map(|property| property.values.len())
                    .sum::<usize>()
                    > 250000)
        {
            return Err(ExpressionSamplesError::CaptureResourceLimit);
        }
        samples.validate_for_source(source)?;
        Ok(samples)
    }

    /// Rejects a sidecar that was not captured for the resolved import root.
    pub fn validate_conversion_scope(
        &self,
        selected_composition_id: u32,
        composition_count: usize,
    ) -> Result<(), ExpressionSamplesError> {
        if self.source_sha256.is_empty() && self.properties.is_empty() && self.errors.is_empty() {
            return Ok(());
        }
        match (&self.capture_scope, self.version) {
            (
                Some(CaptureScope::AllCompositions),
                EXPRESSION_SAMPLES_VERSION | SHAPE_EXPRESSION_SAMPLES_VERSION,
            ) => Ok(()),
            (
                Some(CaptureScope::SelectedComposition {
                    root_composition_id,
                }),
                EXPRESSION_SAMPLES_VERSION | SHAPE_EXPRESSION_SAMPLES_VERSION,
            ) if *root_composition_id == selected_composition_id => Ok(()),
            (
                Some(CaptureScope::SelectedComposition {
                    root_composition_id,
                }),
                EXPRESSION_SAMPLES_VERSION | SHAPE_EXPRESSION_SAMPLES_VERSION,
            ) => Err(ExpressionSamplesError::CaptureRootMismatch {
                captured: *root_composition_id,
                selected: selected_composition_id,
            }),
            (None, LEGACY_EXPRESSION_SAMPLES_VERSION) if composition_count == 1 => Ok(()),
            (None, LEGACY_EXPRESSION_SAMPLES_VERSION) => {
                Err(ExpressionSamplesError::AmbiguousLegacyCapture { composition_count })
            }
            _ => Err(ExpressionSamplesError::InvalidCaptureScope),
        }
    }

    /// Successful evaluated-property records.
    pub fn properties(&self) -> &[EvaluatedProperty] {
        &self.properties
    }

    /// Native per-property evaluation failures.
    pub fn errors(&self) -> &[ExpressionEvaluationError] {
        &self.errors
    }

    /// Returns whether a layer has any successfully evaluated Transform property.
    pub(crate) fn has_transform_layer(&self, composition_id: u32, layer_id: u32) -> bool {
        self.properties.iter().any(|sample| {
            sample.composition_id == composition_id
                && sample.layer_id == layer_id
                && matches!(sample.property, PropertyIdentity::Transform { .. })
        })
    }

    /// Finds the unique successful record for a native property identity.
    pub fn lookup(
        &self,
        composition_id: u32,
        layer_id: u32,
        property: &PropertyIdentity,
    ) -> Option<&EvaluatedProperty> {
        self.properties.iter().find(|sample| {
            sample.composition_id == composition_id
                && sample.layer_id == layer_id
                && &sample.property == property
        })
    }

    fn validate_for_source(&self, source: &[u8]) -> Result<(), ExpressionSamplesError> {
        if !matches!(
            self.version,
            LEGACY_EXPRESSION_SAMPLES_VERSION
                | EXPRESSION_SAMPLES_VERSION
                | SHAPE_EXPRESSION_SAMPLES_VERSION
        ) {
            return Err(ExpressionSamplesError::UnsupportedVersion(self.version));
        }
        match (&self.capture_scope, self.version) {
            (None, LEGACY_EXPRESSION_SAMPLES_VERSION)
            | (Some(_), EXPRESSION_SAMPLES_VERSION | SHAPE_EXPRESSION_SAMPLES_VERSION) => {}
            _ => return Err(ExpressionSamplesError::InvalidCaptureScope),
        }
        if self.sample_interval_ms != EXPRESSION_SAMPLE_INTERVAL_MS {
            return Err(ExpressionSamplesError::InvalidSampleInterval(
                self.sample_interval_ms,
            ));
        }
        let actual_hash = format!("{:x}", Sha256::digest(source));
        if self.source_sha256 != actual_hash {
            return Err(ExpressionSamplesError::SourceHashMismatch {
                expected: actual_hash,
                actual: self.source_sha256.clone(),
            });
        }
        let mut identities = HashSet::new();
        for (record, sample) in self.properties.iter().enumerate() {
            validate_identity(&sample.property, record, self.version)?;
            insert_identity(
                &mut identities,
                sample.composition_id,
                sample.layer_id,
                &sample.property,
            )?;
            validate_values(record, sample, self.version)?;
        }
        for (record, error) in self.errors.iter().enumerate() {
            validate_identity(
                &error.property,
                self.properties.len() + record,
                self.version,
            )?;
            insert_identity(
                &mut identities,
                error.composition_id,
                error.layer_id,
                &error.property,
            )?;
        }
        Ok(())
    }
}

fn validate_identity(
    property: &PropertyIdentity,
    record: usize,
    version: u32,
) -> Result<(), ExpressionSamplesError> {
    if let PropertyIdentity::Shape { path } = property
        && (version != SHAPE_EXPRESSION_SAMPLES_VERSION
            || !(2..=64).contains(&path.len())
            || path
                .first()
                .is_none_or(|segment| segment.match_name != "ADBE Root Vectors Group")
            || path.iter().any(|segment| {
                segment.index == 0
                    || segment.match_name.is_empty()
                    || segment.match_name.len() > 1024
            }))
    {
        return Err(ExpressionSamplesError::InvalidShapeIdentity { record });
    }
    if matches!(property, PropertyIdentity::Effect { index: 0, .. }) {
        return Err(ExpressionSamplesError::InvalidEffectIndex { record });
    }
    if matches!(
        property,
        PropertyIdentity::Mask { .. }
            | PropertyIdentity::TextAnimator { .. }
            | PropertyIdentity::SourceText {}
    ) {
        return Err(ExpressionSamplesError::ConverterOnlyIdentity { record });
    }
    Ok(())
}

fn insert_identity(
    identities: &mut HashSet<(u32, u32, PropertyIdentity)>,
    composition_id: u32,
    layer_id: u32,
    property: &PropertyIdentity,
) -> Result<(), ExpressionSamplesError> {
    if !identities.insert((composition_id, layer_id, property.clone())) {
        return Err(ExpressionSamplesError::DuplicateIdentity {
            composition_id,
            layer_id,
            property: property.clone(),
        });
    }
    Ok(())
}

fn validate_values(
    record: usize,
    sample: &EvaluatedProperty,
    version: u32,
) -> Result<(), ExpressionSamplesError> {
    if sample.values.is_empty() {
        return Err(ExpressionSamplesError::EmptyValues { record });
    }
    let dimension = sample.values[0].len();
    if !(1..=4).contains(&dimension) {
        return Err(ExpressionSamplesError::InvalidDimension { record, dimension });
    }
    for (sample_index, value) in sample.values.iter().enumerate() {
        if value.len() != dimension {
            return Err(ExpressionSamplesError::InconsistentDimension {
                record,
                sample: sample_index,
                expected: dimension,
                actual: value.len(),
            });
        }
        for (component, scalar) in value.iter().enumerate() {
            if !scalar.is_finite() {
                return Err(ExpressionSamplesError::NonFiniteValue {
                    record,
                    sample: sample_index,
                    component,
                });
            }
        }
    }
    let duration_ms = i64::try_from(sample.values.len() - 1)
        .map_err(|_| ExpressionSamplesError::TimestampOverflow { record })?;
    sample
        .start_ms
        .checked_add(duration_ms)
        .ok_or(ExpressionSamplesError::TimestampOverflow { record })?;
    if version == LEGACY_EXPRESSION_SAMPLES_VERSION {
        if !sample.sample_times_seconds.is_empty() {
            return Err(ExpressionSamplesError::UnexpectedActualTimes { record });
        }
        return Ok(());
    }
    if sample.sample_times_seconds.len() != sample.values.len() {
        return Err(ExpressionSamplesError::ActualTimeCount {
            record,
            actual: sample.sample_times_seconds.len(),
            expected: sample.values.len(),
        });
    }
    let mut previous = None;
    for (sample_index, time) in sample.sample_times_seconds.iter().copied().enumerate() {
        if !time.is_finite() {
            return Err(ExpressionSamplesError::NonFiniteTimestamp {
                record,
                sample: sample_index,
            });
        }
        let requested = (sample.start_ms as f64 + sample_index as f64) / 1_000.0;
        if (time - requested).abs() > MAX_NATIVE_CLOCK_DEVIATION_SECONDS {
            return Err(ExpressionSamplesError::TimestampTooFarFromRequest {
                record,
                sample: sample_index,
            });
        }
        if previous.is_some_and(|previous| time <= previous) {
            return Err(ExpressionSamplesError::NonIncreasingTimestamp {
                record,
                sample: sample_index,
            });
        }
        previous = Some(time);
    }
    Ok(())
}

/// Validation failure for an expression-sample sidecar.
#[derive(Debug, Error)]
pub enum ExpressionSamplesError {
    /// JSON syntax or shape is invalid.
    #[error("invalid expression samples JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Sidecar format version is not supported.
    #[error("unsupported expression samples version {0}")]
    UnsupportedVersion(u32),
    /// Capture scope is absent or present for the wrong sidecar version.
    #[error("expression samples capture_scope does not match the sidecar version")]
    InvalidCaptureScope,
    /// A selected-root sidecar belongs to another import root.
    #[error(
        "expression samples were captured for root composition {captured}, but import selected {selected}"
    )]
    CaptureRootMismatch { captured: u32, selected: u32 },
    /// A legacy sidecar cannot be bound within a multi-composition source.
    #[error(
        "legacy expression samples are ambiguous for an AEP with {composition_count} compositions; recapture as version 2"
    )]
    AmbiguousLegacyCapture { composition_count: usize },
    /// Samples are not spaced at the required one-millisecond requested interval.
    #[error("expression sample interval must be 1 ms, got {0}")]
    InvalidSampleInterval(u32),
    /// Sidecar belongs to different source bytes.
    #[error("expression samples source SHA-256 mismatch: expected {expected}, got {actual}")]
    SourceHashMismatch { expected: String, actual: String },
    /// A version-3 capture exceeds the fixed native capture resource limits.
    #[error("version 3 expression samples exceed the fixed capture resource limits")]
    CaptureResourceLimit,
    /// Shape identity is malformed or appears before its explicit format version.
    #[error(
        "expression sample record {record} requires version 3 and a bounded root-anchored Shape path"
    )]
    InvalidShapeIdentity { record: usize },
    /// An Effect identity uses zero instead of AE's one-based occurrence index.
    #[error("expression sample record {record} effect index must be at least 1")]
    InvalidEffectIndex { record: usize },
    /// A converter-evaluated identity appeared in an Adobe sidecar.
    #[error("expression sample record {record} uses a converter-only Mask/Text Animator identity")]
    ConverterOnlyIdentity { record: usize },
    /// More than one record names the same native property.
    #[error(
        "duplicate expression sample identity for composition {composition_id}, layer {layer_id}, property {property:?}"
    )]
    DuplicateIdentity {
        composition_id: u32,
        layer_id: u32,
        property: PropertyIdentity,
    },
    /// A successful property has no samples.
    #[error("expression sample record {record} has no values")]
    EmptyValues { record: usize },
    /// A sample vector is empty or wider than the supported scalar/vector/color range.
    #[error(
        "expression sample record {record} has unsupported dimension {dimension}; expected 1 through 4"
    )]
    InvalidDimension { record: usize, dimension: usize },
    /// Sample vectors for one property do not share a dimension.
    #[error(
        "expression sample record {record}, sample {sample} has dimension {actual}; expected {expected}"
    )]
    InconsistentDimension {
        record: usize,
        sample: usize,
        expected: usize,
        actual: usize,
    },
    /// A component is NaN or infinite.
    #[error(
        "expression sample record {record}, sample {sample}, component {component} is not finite"
    )]
    NonFiniteValue {
        record: usize,
        sample: usize,
        component: usize,
    },
    /// Inclusive one-millisecond timestamps do not fit in an `i64`.
    #[error("expression sample record {record} timestamp range overflows i64")]
    TimestampOverflow { record: usize },
    /// Legacy records may not carry v2 actual timestamps.
    #[error("legacy expression sample record {record} unexpectedly carries actual timestamps")]
    UnexpectedActualTimes { record: usize },
    /// Native timestamps must align one-to-one with values.
    #[error(
        "expression sample record {record} has {actual} actual timestamps; expected {expected}"
    )]
    ActualTimeCount {
        record: usize,
        actual: usize,
        expected: usize,
    },
    /// A native timestamp is not finite.
    #[error("expression sample record {record}, timestamp {sample} is nonfinite")]
    NonFiniteTimestamp { record: usize, sample: usize },
    /// A native timestamp is implausibly far from its requested millisecond.
    #[error(
        "expression sample record {record}, timestamp {sample} is too far from its requested time"
    )]
    TimestampTooFarFromRequest { record: usize, sample: usize },
    /// Native timestamps must be strictly increasing.
    #[error("expression sample record {record}, timestamp {sample} is not strictly increasing")]
    NonIncreasingTimestamp { record: usize, sample: usize },
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const SOURCE: &[u8] = b"source-aep-bytes";

    fn source_hash() -> String {
        format!("{:x}", Sha256::digest(SOURCE))
    }

    fn property(match_name: &str, start_ms: i64, values: Value) -> Value {
        json!({
            "composition_id": 7,
            "layer_id": 11,
            "property": { "kind": "transform", "match_name": match_name },
            "start_ms": start_ms,
            "values": values,
        })
    }

    fn sidecar(properties: Vec<Value>, errors: Vec<Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "version": 1,
            "source_sha256": source_hash(),
            "sample_interval_ms": 1,
            "properties": properties,
            "errors": errors,
        }))
        .unwrap()
    }

    #[test]
    fn shape_paths_require_version_three_and_preserve_repeated_groups() {
        let identity = json!({"kind": "shape", "path": [
            {"index": 2, "match_name": "ADBE Root Vectors Group"},
            {"index": 1, "match_name": "ADBE Vector Group"},
            {"index": 2, "match_name": "ADBE Vector Transform Group"},
            {"index": 3, "match_name": "ADBE Vector Scale"},
        ]});
        let mut first = property("unused", 0, json!([[100.0, 100.0], [101.0, 100.0]]));
        first["property"] = identity;
        first["sample_times_seconds"] = json!([0.0, 0.001]);
        let mut second = first.clone();
        second["property"]["path"][1]["index"] = json!(2);
        let mut document: Value =
            serde_json::from_slice(&sidecar(vec![first, second], vec![])).unwrap();
        document["version"] = json!(3);
        document["capture_scope"] =
            json!({"mode": "selected_composition", "root_composition_id": 1});
        let parse = |value: &Value| {
            ExpressionSamples::from_json_for_source(&serde_json::to_vec(value).unwrap(), SOURCE)
        };
        let parsed = parse(&document).unwrap();
        assert_eq!(parsed.properties().len(), 2);
        assert_ne!(
            parsed.properties()[0].property(),
            parsed.properties()[1].property()
        );
        parsed.validate_conversion_scope(1, 2).unwrap();
        assert!(parsed.validate_conversion_scope(2, 2).is_err());
        document["version"] = json!(2);
        assert!(matches!(
            parse(&document),
            Err(ExpressionSamplesError::InvalidShapeIdentity { .. })
        ));
        document["version"] = json!(3);
        for path in [
            json!([]),
            json!([{"index": 2, "match_name": "ADBE Mask Parade"}, {"index": 1, "match_name": "ADBE Vector Scale"}]),
            json!([{"index": 0, "match_name": "ADBE Root Vectors Group"}, {"index": 1, "match_name": "ADBE Vector Scale"}]),
            json!([{"index": 2, "match_name": "ADBE Root Vectors Group"}, {"index": 1, "match_name": ""}]),
            json!([{"index": 2, "match_name": "ADBE Root Vectors Group", "name": "Contents"}, {"index": 1, "match_name": "ADBE Vector Scale"}]),
        ] {
            let mut malformed = document.clone();
            malformed["properties"][0]["property"]["path"] = path;
            assert!(parse(&malformed).is_err());
        }
        document["properties"][1] = document["properties"][0].clone();
        assert!(matches!(
            parse(&document),
            Err(ExpressionSamplesError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn oversized_version_three_is_rejected_before_property_deserialization() {
        // This is valid JSON, but a property cannot deserialize from a number.
        // The byte limit must win before the full property schema is visited.
        let mut bytes = br#"{"version":3,"properties":[0]}"#.to_vec();
        bytes.resize(128 * 1024 * 1024 + 1, b' ');
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&bytes, SOURCE),
            Err(ExpressionSamplesError::CaptureResourceLimit)
        ));
    }

    #[test]
    fn oversized_legacy_sidecar_keeps_existing_quota_behavior() {
        let mut bytes = sidecar(Vec::new(), Vec::new());
        bytes.resize(128 * 1024 * 1024 + 1, b' ');
        assert!(ExpressionSamples::from_json_for_source(&bytes, SOURCE).is_ok());
    }

    #[test]
    fn version_three_retains_fixed_capture_resource_bounds() {
        let mut document: Value = serde_json::from_slice(&sidecar(vec![], vec![])).unwrap();
        document["version"] = json!(3);
        document["capture_scope"] = json!({"mode": "all_compositions"});
        let parse = |value: &Value| {
            ExpressionSamples::from_json_for_source(&serde_json::to_vec(value).unwrap(), SOURCE)
        };
        document["errors"] = json!((0..4097).map(|index| json!({
            "composition_id": index, "layer_id": 1,
            "property": {"kind": "transform", "match_name": "ADBE Opacity"}, "message": "failed"
        })).collect::<Vec<_>>());
        assert!(matches!(
            parse(&document),
            Err(ExpressionSamplesError::CaptureResourceLimit)
        ));
        document["errors"] = json!([]);
        document["properties"] =
            json!([property("ADBE Opacity", 0, json!(vec![vec![0.0]; 60002]))]);
        assert!(matches!(
            parse(&document),
            Err(ExpressionSamplesError::CaptureResourceLimit)
        ));
        document["properties"] = json!(
            (0..5)
                .map(|index| property(
                    &format!("ADBE Control-{index}"),
                    0,
                    json!(vec![vec![0.0]; 50001])
                ))
                .collect::<Vec<_>>()
        );
        assert!(matches!(
            parse(&document),
            Err(ExpressionSamplesError::CaptureResourceLimit)
        ));
    }

    #[test]
    fn parses_inclusive_one_millisecond_vectors_and_looks_them_up() {
        let bytes = sidecar(
            vec![property(
                "ADBE Position",
                -1,
                json!([[1.0, 2.0], [3.0, 4.0]]),
            )],
            vec![],
        );
        let samples = ExpressionSamples::from_json_for_source(&bytes, SOURCE).unwrap();
        let identity = PropertyIdentity::Transform {
            match_name: "ADBE Position".into(),
        };
        assert!(samples.has_transform_layer(7, 11));
        assert!(!samples.has_transform_layer(7, 12));
        let evaluated = samples.lookup(7, 11, &identity).unwrap();
        assert_eq!(evaluated.value_at_ms(-1), Some([1.0, 2.0].as_slice()));
        assert_eq!(evaluated.value_at_ms(0), Some([3.0, 4.0].as_slice()));
        assert_eq!(evaluated.value_at_ms(1), None);
    }

    #[test]
    fn rejects_malformed_shape_and_wrong_source_hash() {
        let malformed = br#"{"version":1}"#;
        assert!(matches!(
            ExpressionSamples::from_json_for_source(malformed, SOURCE),
            Err(ExpressionSamplesError::Json(_))
        ));

        let mut value: Value = serde_json::from_slice(&sidecar(vec![], vec![])).unwrap();
        value["source_sha256"] = "0".repeat(64).into();
        let wrong_hash = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&wrong_hash, SOURCE),
            Err(ExpressionSamplesError::SourceHashMismatch { .. })
        ));
    }

    #[test]
    fn rejects_zero_based_effect_index_and_accepts_native_index_one() {
        let mut sidecar = json!({
            "version": 1,
            "source_sha256": source_hash(),
            "sample_interval_ms": 1,
            "properties": [{
                "composition_id": 7,
                "layer_id": 11,
                "property": { "kind": "effect", "index": 0, "match_name": "ADBE Slider Control-0001" },
                "start_ms": 0,
                "values": [[1.0]],
            }],
            "errors": [],
        });
        let invalid = serde_json::to_vec(&sidecar).unwrap();
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&invalid, SOURCE),
            Err(ExpressionSamplesError::InvalidEffectIndex { record: 0 })
        ));

        sidecar["properties"][0]["property"]["index"] = 1.into();
        let valid = serde_json::to_vec(&sidecar).unwrap();
        assert!(ExpressionSamples::from_json_for_source(&valid, SOURCE).is_ok());
    }

    #[test]
    fn rejects_duplicate_success_or_error_identity() {
        let successful = property("ADBE Opacity", 0, json!([[100.0]]));
        let error = json!({
            "composition_id": 7,
            "layer_id": 11,
            "property": { "kind": "transform", "match_name": "ADBE Opacity" },
            "message": "native evaluation failed",
        });
        let bytes = sidecar(vec![successful], vec![error]);
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&bytes, SOURCE),
            Err(ExpressionSamplesError::DuplicateIdentity { .. })
        ));
    }

    #[test]
    fn rejects_inconsistent_sample_dimensions() {
        let inconsistent = sidecar(
            vec![property("ADBE Position", 0, json!([[1.0], [2.0, 3.0]]))],
            vec![],
        );
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&inconsistent, SOURCE),
            Err(ExpressionSamplesError::InconsistentDimension { .. })
        ));
    }

    #[test]
    fn accepts_record_and_sample_counts_above_the_former_limits() {
        let errors = (0..=4_096)
            .map(|index| {
                json!({
                    "composition_id": u32::try_from(index).unwrap(),
                    "layer_id": 1,
                    "property": { "kind": "transform", "match_name": "ADBE Opacity" },
                    "message": "failed",
                })
            })
            .collect();
        let records =
            ExpressionSamples::from_json_for_source(&sidecar(vec![], errors), SOURCE).unwrap();
        assert_eq!(records.errors().len(), 4_097);

        let values = vec![vec![0.0]; 250_001];
        let samples = ExpressionSamples::from_json_for_source(
            &sidecar(vec![property("ADBE Position", 0, json!(values))], vec![]),
            SOURCE,
        )
        .unwrap();
        assert_eq!(samples.properties()[0].values().len(), 250_001);
    }

    #[test]
    fn accepts_long_property_identity_and_error_messages() {
        let long_name = "x".repeat(1_025);
        let long_message = "reason".repeat(300);
        let sidecar = sidecar(
            vec![property(&long_name, 0, json!([[1.0]]))],
            vec![json!({
                "composition_id": 1,
                "layer_id": 2,
                "property": { "kind": "transform", "match_name": "ADBE Scale" },
                "message": long_message,
            })],
        );
        let parsed = ExpressionSamples::from_json_for_source(&sidecar, SOURCE).unwrap();
        assert!(matches!(
            parsed.properties()[0].property(),
            PropertyIdentity::Transform { match_name } if match_name == &long_name
        ));
        assert_eq!(parsed.errors()[0].message(), "reason".repeat(300));
    }

    #[test]
    fn version_two_binds_capture_scope_and_actual_native_times() {
        let mut value: Value = serde_json::from_slice(&sidecar(
            vec![property(
                "ADBE Position",
                -1,
                json!([[1.0, 2.0], [3.0, 4.0]]),
            )],
            vec![],
        ))
        .unwrap();
        value["version"] = 2.into();
        value["capture_scope"] = json!({
            "mode": "selected_composition",
            "root_composition_id": 7,
        });
        value["properties"][0]["sample_times_seconds"] = json!([-0.001, 0.0]);
        let bytes = serde_json::to_vec(&value).unwrap();
        let samples = ExpressionSamples::from_json_for_source(&bytes, SOURCE).unwrap();
        assert_eq!(
            samples.properties()[0].sample_times_seconds(),
            [-0.001, 0.0]
        );
        samples.validate_conversion_scope(7, 2).unwrap();
        assert!(matches!(
            samples.validate_conversion_scope(8, 2),
            Err(ExpressionSamplesError::CaptureRootMismatch { .. })
        ));

        value["capture_scope"] = json!({ "mode": "all_compositions" });
        let all =
            ExpressionSamples::from_json_for_source(&serde_json::to_vec(&value).unwrap(), SOURCE)
                .unwrap();
        all.validate_conversion_scope(8, 2).unwrap();

        value["properties"][0]["sample_times_seconds"][1] = 5.0.into();
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&serde_json::to_vec(&value).unwrap(), SOURCE,),
            Err(ExpressionSamplesError::TimestampTooFarFromRequest { .. })
        ));

        let legacy = ExpressionSamples::from_json_for_source(
            &sidecar(vec![property("ADBE Position", 0, json!([[1.0]]))], vec![]),
            SOURCE,
        )
        .unwrap();
        legacy.validate_conversion_scope(7, 1).unwrap();
        assert!(matches!(
            legacy.validate_conversion_scope(7, 2),
            Err(ExpressionSamplesError::AmbiguousLegacyCapture { .. })
        ));
    }

    #[test]
    fn rejects_timestamp_overflow_and_unknown_fields() {
        let overflow = sidecar(
            vec![property("ADBE Position", i64::MAX, json!([[1.0], [2.0]]))],
            vec![],
        );
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&overflow, SOURCE),
            Err(ExpressionSamplesError::TimestampOverflow { .. })
        ));

        let mut value: Value = serde_json::from_slice(&sidecar(vec![], vec![])).unwrap();
        value["unexpected"] = true.into();
        let unknown = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            ExpressionSamples::from_json_for_source(&unknown, SOURCE),
            Err(ExpressionSamplesError::Json(_))
        ));
    }
}
