//! Canonical JSON persistence boundary for an editable FX composition.

use crate::{Color, Dimensions, FXComposition};
use serde::{ser::SerializeMap, Deserialize, Serialize};
use serde_json::{value::RawValue, Value};
use std::collections::BTreeMap;

/// Current major version of [`EditableFxCompositionDocument`].
pub const EDITABLE_FX_DOCUMENT_FORMAT_VERSION: u8 = 1;

/// Stable schema identifier written into editable FX documents.
pub const EDITABLE_FX_DOCUMENT_SCHEMA_URL: &str =
    "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json";

/// Exact JSON Schema for [`EditableFxCompositionDocument`].
pub const EDITABLE_FX_COMPOSITION_SCHEMA: &str =
    include_str!("../editable_fx_composition.schema.json");

/// Errors produced by the editable FX JSON persistence boundary.
#[derive(Debug, thiserror::Error)]
pub enum EditableFxDocumentError {
    /// The bytes are not valid JSON or do not match the typed FX model.
    #[error("invalid editable FX JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A document-level invariant is invalid or unsupported.
    #[error("invalid editable FX document: {0}")]
    Invalid(String),
}

#[path = "editable_document_declaration.rs"]
mod declaration;

macro_rules! stored_editable_document {
    ($(#[$attrs:meta])* pub struct $name:ident<C> {
        $($(#[$field_attrs:meta])* $field:ident: $ty:ty,)*
    }) => {
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name<C> {
            $($field: $ty,)*
            // A storage sidecar, not another definition of the envelope.
            envelope: serde_json::Map<String, Value>,
        }
    };
}
crate::define_editable_document_schema!(stored_editable_document);
crate::define_document_dimensions_schema!();

macro_rules! editable_document_wire {
    ($(#[$attrs:meta])* pub struct $name:ident<C> { $($fields:tt)* }) => {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct EditableFxDocumentWire<C> { $($fields)* }
    };
}
crate::define_editable_document_schema!(editable_document_wire);
type EditableFxCompositionDocumentWire = EditableFxDocumentWire<FXComposition>;

/// Editable document with the canonical, runtime-independent composition payload.
pub type EditableFxCompositionDocument = EditableFxDocument<FXComposition>;

impl<C: AsRef<FXComposition>> Serialize for EditableFxDocument<C> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The stored schema composition already owns its lossless wire value.
        // Serialize it by reference instead of cloning the full FX tree into a
        // second Value. Insert at the same sorted key position as Map::insert.
        let mut output = serializer.serialize_map(Some(self.envelope.len() + 1))?;
        let mut inserted = false;
        for (name, value) in &self.envelope {
            if !inserted && name.as_str() > "composition" {
                output.serialize_entry("composition", self.composition.as_ref())?;
                inserted = true;
            }
            output.serialize_entry(name, value)?;
        }
        if !inserted {
            output.serialize_entry("composition", self.composition.as_ref())?;
        }
        output.end()
    }
}

impl<C> EditableFxDocument<C>
where
    C: AsRef<FXComposition> + From<FXComposition>,
{
    /// Changes the composition carrier at an ownership boundary without
    /// serializing or reconstructing the document envelope. All retained unknown
    /// fields move with it; the carrier's `From` conversion owns runtime setup.
    #[must_use]
    pub fn into_carrier<D>(self) -> EditableFxDocument<D>
    where
        D: AsRef<FXComposition> + From<FXComposition> + From<C>,
    {
        EditableFxDocument {
            schema: self.schema,
            format_version: self.format_version,
            dimensions: self.dimensions,
            duration: self.duration,
            background_color: self.background_color,
            composition: self.composition.into(),
            unknown_fields: self.unknown_fields,
            envelope: self.envelope,
        }
    }

    /// Creates a validated editable document.
    pub fn new(
        dimensions: Dimensions,
        duration: crate::time::Duration,
        background_color: Option<Color>,
        composition: C,
    ) -> Result<Self, EditableFxDocumentError> {
        let mut document = Self {
            schema: EDITABLE_FX_DOCUMENT_SCHEMA_URL.to_owned(),
            format_version: EDITABLE_FX_DOCUMENT_FORMAT_VERSION,
            dimensions: dimensions.into(),
            duration,
            background_color,
            composition,
            unknown_fields: BTreeMap::new(),
            envelope: serde_json::Map::new(),
        };
        document.validate()?;
        document.envelope = serde_json::Map::from_iter([
            ("$schema".to_owned(), Value::from(document.schema.clone())),
            (
                "formatVersion".to_owned(),
                Value::from(document.format_version),
            ),
            (
                "dimensions".to_owned(),
                serde_json::to_value(document.dimensions)?,
            ),
            ("duration".to_owned(), Value::from(duration.as_secs())),
        ]);
        if let Some(color) = background_color {
            document
                .envelope
                .insert("backgroundColor".to_owned(), serde_json::to_value(color)?);
        }
        Ok(document)
    }

    /// Parses and validates a canonical editable FX JSON document.
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, EditableFxDocumentError> {
        let wire: EditableFxCompositionDocumentWire = serde_json::from_slice(bytes)?;
        // Retain only the small document envelope: the composition has already
        // been decoded into the typed wire and must not become another Value.
        let raw_fields: BTreeMap<&str, &RawValue> = serde_json::from_slice(bytes)?;
        let envelope = raw_fields
            .into_iter()
            .filter(|(name, _)| *name != "composition")
            .map(|(name, raw)| {
                serde_json::from_str(raw.get()).map(|value| (name.to_owned(), value))
            })
            .collect::<Result<serde_json::Map<String, Value>, _>>()?;
        let document = Self {
            schema: wire.schema,
            format_version: wire.format_version,
            dimensions: wire.dimensions,
            duration: wire.duration,
            background_color: wire.background_color,
            composition: C::from(wire.composition),
            unknown_fields: wire.unknown_fields,
            envelope,
        };
        document.validate()?;
        Ok(document)
    }

    /// Parses and validates a JSON value produced by an editor or agent.
    pub fn from_json_value(value: Value) -> Result<Self, EditableFxDocumentError> {
        let bytes = serde_json::to_vec(&value)?;
        // Do not retain the full JSON tree while allocating the decoded document.
        drop(value);
        Self::from_json_slice(&bytes)
    }

    /// Serializes deterministic, human-readable JSON suitable for direct edits.
    pub fn to_json_vec(&self) -> Result<Vec<u8>, EditableFxDocumentError> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Returns a JSON value suitable for direct manipulation by an agent.
    pub fn to_json_value(&self) -> Result<Value, EditableFxDocumentError> {
        self.validate()?;
        Ok(serde_json::to_value(self)?)
    }

    /// Returns the canvas dimensions supplied to the renderer.
    #[must_use]
    pub fn dimensions(&self) -> Dimensions {
        self.dimensions.into()
    }

    /// Returns the document duration.
    #[must_use]
    pub fn duration(&self) -> crate::time::Duration {
        self.duration
    }

    /// Returns the optional canvas background color.
    #[must_use]
    pub fn background_color(&self) -> Option<Color> {
        self.background_color
    }

    /// Names of unrecognized document fields retained during loading.
    pub fn unknown_field_names(&self) -> impl Iterator<Item = &str> {
        self.unknown_fields.keys().map(String::as_str)
    }

    /// Returns the editable composition.
    #[must_use]
    pub fn composition(&self) -> &C {
        &self.composition
    }

    /// Returns the editable composition for invariant-preserving mutation APIs.
    pub fn composition_mut(&mut self) -> &mut C {
        &mut self.composition
    }

    /// Replaces the composition and its document-level duration together.
    ///
    /// The standalone project-action adapter uses this seam because timing
    /// actions may fit the mounted track item to the composition's root-layer
    /// envelope. Keeping the two values synchronized prevents the persisted
    /// document from clipping or extending the edited composition.
    pub fn replace_composition_and_duration(
        &mut self,
        composition: C,
        duration: crate::time::Duration,
    ) -> Result<(), EditableFxDocumentError> {
        composition
            .as_ref()
            .validate()
            .map_err(|error| EditableFxDocumentError::Invalid(error.to_string()))?;
        if duration.is_zero() {
            return Err(EditableFxDocumentError::Invalid(
                "duration must be greater than zero".to_owned(),
            ));
        }
        if duration != self.duration {
            self.envelope
                .insert("duration".to_owned(), Value::from(duration.as_secs()));
        }
        self.composition = composition;
        self.duration = duration;
        Ok(())
    }

    fn validate(&self) -> Result<(), EditableFxDocumentError> {
        self.composition
            .as_ref()
            .validate()
            .map_err(|error| EditableFxDocumentError::Invalid(error.to_string()))?;
        if self.schema != EDITABLE_FX_DOCUMENT_SCHEMA_URL {
            return Err(EditableFxDocumentError::Invalid(format!(
                "unsupported $schema {:?}",
                self.schema
            )));
        }
        if self.format_version != EDITABLE_FX_DOCUMENT_FORMAT_VERSION {
            return Err(EditableFxDocumentError::Invalid(format!(
                "unsupported formatVersion {}; expected {EDITABLE_FX_DOCUMENT_FORMAT_VERSION}",
                self.format_version
            )));
        }
        if self.dimensions.width == 0 || self.dimensions.height == 0 {
            return Err(EditableFxDocumentError::Invalid(
                "dimensions must be non-zero".to_owned(),
            ));
        }
        if self.duration.is_zero() {
            return Err(EditableFxDocumentError::Invalid(
                "duration must be greater than zero".to_owned(),
            ));
        }
        if self.background_color.is_some_and(|color| {
            color
                .iter()
                .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
        }) {
            return Err(EditableFxDocumentError::Invalid(
                "backgroundColor channels must be finite values from 0 through 1".to_owned(),
            ));
        }
        Ok(())
    }
}

impl From<Dimensions> for DimensionsWire {
    fn from(value: Dimensions) -> Self {
        Self {
            width: value.width,
            height: value.height,
        }
    }
}

impl From<DimensionsWire> for Dimensions {
    fn from(value: DimensionsWire) -> Self {
        Self::new(value.width, value.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal_document() -> Value {
        json!({
            "$schema": EDITABLE_FX_DOCUMENT_SCHEMA_URL,
            "formatVersion": 1,
            "dimensions": { "width": 1080, "height": 1920 },
            "duration": 6.0,
            "backgroundColor": [0.0, 0.0, 0.0, 1.0],
            "composition": {
                "id": "composition-1",
                "name": "Editable",
                "layers": [],
                "futureCompositionField": { "kept": true }
            },
            "futureEnvelopeField": { "alsoKept": true }
        })
    }

    #[test]
    fn round_trip_preserves_editable_and_unknown_fields() {
        let document = EditableFxCompositionDocument::from_json_value(minimal_document()).unwrap();
        assert_eq!(document.composition().name(), "Editable");
        assert_eq!(
            document.unknown_field_names().collect::<Vec<_>>(),
            ["futureEnvelopeField"]
        );
        let encoded = document.to_json_value().unwrap();
        assert_eq!(
            encoded["composition"]["futureCompositionField"]["kept"],
            true
        );
        assert_eq!(encoded["futureEnvelopeField"]["alsoKept"], true);
        assert_eq!(encoded["duration"], 6.0);
    }

    #[test]
    fn value_and_slice_parsing_have_identical_results_and_errors() {
        for value in [
            minimal_document(),
            {
                let mut value = minimal_document();
                value["futureEnvelopeField"] = json!({"null": null, "large": 9007199254740991_u64});
                value
            },
            {
                let mut value = minimal_document();
                value["formatVersion"] = json!(2);
                value
            },
            {
                let mut value = minimal_document();
                value["dimensions"]["width"] = json!(0);
                value
            },
        ] {
            let bytes = serde_json::to_vec(&value).unwrap();
            let from_value = EditableFxCompositionDocument::from_json_value(value)
                .map(|document| document.to_json_vec().unwrap())
                .map_err(|error| error.to_string());
            let from_slice = EditableFxCompositionDocument::from_json_slice(&bytes)
                .map(|document| document.to_json_vec().unwrap())
                .map_err(|error| error.to_string());
            assert_eq!(from_value, from_slice);
        }
    }

    #[test]
    fn rejects_wrong_version_and_invalid_document_geometry() {
        let mut value = minimal_document();
        value["formatVersion"] = json!(2);
        assert!(EditableFxCompositionDocument::from_json_value(value).is_err());

        let mut value = minimal_document();
        value["dimensions"]["width"] = json!(0);
        assert!(EditableFxCompositionDocument::from_json_value(value).is_err());
    }

    #[test]
    fn typed_runtime_carrier_serializes_only_the_canonical_payload() {
        struct RuntimeCarrier(FXComposition);
        impl AsRef<FXComposition> for RuntimeCarrier {
            fn as_ref(&self) -> &FXComposition {
                &self.0
            }
        }
        impl From<FXComposition> for RuntimeCarrier {
            fn from(document: FXComposition) -> Self {
                Self(document)
            }
        }
        impl From<RuntimeCarrier> for FXComposition {
            fn from(carrier: RuntimeCarrier) -> Self {
                carrier.0
            }
        }
        // The carrier deliberately has no Serialize implementation: persistence
        // must borrow the canonical payload, never serialize runtime state.
        let canonical = EditableFxCompositionDocument::from_json_value(minimal_document())
            .expect("valid canonical fixture");
        let runtime = EditableFxDocument::<RuntimeCarrier>::from_json_value(minimal_document())
            .expect("valid typed runtime fixture");
        assert_eq!(
            runtime
                .to_json_value()
                .expect("canonical payload serialization"),
            canonical
                .to_json_value()
                .expect("canonical document serialization"),
        );
        let expected = canonical.to_json_vec().expect("canonical bytes");
        let runtime: EditableFxDocument<RuntimeCarrier> = canonical.into_carrier();
        assert_eq!(runtime.to_json_vec().unwrap(), expected);
        let restored: EditableFxCompositionDocument = runtime.into_carrier();
        assert_eq!(restored.to_json_vec().unwrap(), expected);
        assert_eq!(
            restored.to_json_value().unwrap()["futureEnvelopeField"]["alsoKept"],
            true
        );
        assert_eq!(
            restored.to_json_value().unwrap()["composition"]["futureCompositionField"]["kept"],
            true
        );
    }

    #[test]
    fn unchanged_envelope_preserves_number_and_null_representations() {
        for duration in [json!(6), json!(6.0), json!(6.0001)] {
            let mut original = minimal_document();
            original["duration"] = duration;
            original["backgroundColor"] = Value::Null;
            let mut document =
                EditableFxCompositionDocument::from_json_value(original.clone()).unwrap();
            document
                .replace_composition_and_duration(
                    document.composition().clone(),
                    document.duration(),
                )
                .unwrap();
            assert_eq!(document.to_json_value().unwrap(), original);
        }
    }

    #[test]
    fn borrowed_serialization_preserves_canonical_key_order_and_unknowns() {
        let source = minimal_document();
        let document = EditableFxCompositionDocument::from_json_value(source.clone()).unwrap();
        let mut expected = serde_json::to_vec_pretty(&source).unwrap();
        expected.push(b'\n');
        assert_eq!(document.to_json_vec().unwrap(), expected);
    }

    #[test]
    fn raw_composition_is_excluded_from_retained_envelope() {
        let mut original = minimal_document();
        original["futureEnvelopeField"] = json!({"nested": [null, 1, {"ok": true}]});
        original["composition"]["futureCompositionField"] = json!({"kept": true});
        let document = EditableFxCompositionDocument::from_json_value(original.clone()).unwrap();
        assert!(!document.envelope.contains_key("composition"));
        assert_eq!(document.to_json_value().unwrap(), original);
    }

    #[test]
    fn intentional_duration_replacement_updates_retained_envelope() {
        let mut document =
            EditableFxCompositionDocument::from_json_value(minimal_document()).unwrap();
        let duration = crate::time::Duration::from_secs(7.5);
        document
            .replace_composition_and_duration(document.composition().clone(), duration)
            .unwrap();
        let value = document.to_json_value().unwrap();
        assert_eq!(value["duration"], json!(7.5));
        assert_eq!(
            EditableFxCompositionDocument::from_json_value(value)
                .unwrap()
                .duration(),
            duration
        );
    }

    #[test]
    fn rejected_duration_replacement_preserves_entire_document() {
        let mut document =
            EditableFxCompositionDocument::from_json_value(minimal_document()).unwrap();
        let original = document.to_json_value().unwrap();
        let replacement =
            serde_json::from_value(json!({"id": "different", "name": "different", "layers": []}))
                .unwrap();
        assert!(document
            .replace_composition_and_duration(replacement, crate::time::Duration::from_secs(0.0))
            .is_err());
        assert_eq!(document.to_json_value().unwrap(), original);
    }

    #[test]
    fn embedded_schema_identifies_this_document_version() {
        let schema: Value = serde_json::from_str(EDITABLE_FX_COMPOSITION_SCHEMA).unwrap();
        assert_eq!(schema["$id"], EDITABLE_FX_DOCUMENT_SCHEMA_URL);
        assert_eq!(schema["properties"]["formatVersion"]["const"], 1);
        assert!(schema["properties"]["composition"]["$ref"].is_string());
    }
}
