//! Lossless composition storage and checked, transactional replacements.
use crate::{
    AnimationGraph, CompositionId, Layer, LayerData, LayerId, MotionBlurSettings, PropType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[path = "declaration.rs"]
mod declaration;

macro_rules! stored_composition {
    ($(#[$docs:meta])* pub struct $name:ident { $($fields:tt)* } provenance { $($provenance:tt)* }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase")]
        struct CompositionData {
            $($provenance)*
            $($fields)*
        }
    };
}
crate::define_editable_composition_schema!(
    stored_composition,
    serde(default),
    allow(dead_code),
    allow(dead_code),
    allow(dead_code)
);

fn deserialize_optional_schema_version<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u8>, D::Error> {
    Option::<u8>::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct FXComposition(crate::stored::Stored<CompositionData>);

impl<'de> Deserialize<'de> for FXComposition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let composition = Self(crate::stored::Stored::deserialize(deserializer)?);
        composition.validate().map_err(serde::de::Error::custom)?;
        Ok(composition)
    }
}

impl AsRef<FXComposition> for FXComposition {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl FXComposition {
    pub fn empty(id: CompositionId, name: impl Into<String>) -> Self {
        Self::try_from_parts(id, name, AnimationGraph::new(), Vec::new())
            .expect("empty composition has valid structural data")
    }

    pub fn try_from_parts(
        id: CompositionId,
        name: impl Into<String>,
        dynamics: AnimationGraph,
        layers: Vec<Layer>,
    ) -> Result<Self, ValidationError> {
        let data = CompositionData {
            version: None,
            id,
            name: name.into(),
            dynamics,
            layers,
            motion_blur: MotionBlurSettings::default(),
            segment_layer_ids: Vec::new(),
        };
        let value = crate::stored::checked_value(&data)
            .map_err(|error| ValidationError::Wire(error.to_string()))?;
        Self::from_value(value)
    }

    fn from_value(value: Value) -> Result<Self, ValidationError> {
        let data = crate::stored::Stored::from_value(value)
            .map_err(|error| ValidationError::Wire(error.to_string()))?;
        let composition = Self(data);
        composition.validate()?;
        Ok(composition)
    }

    pub fn try_replace_parts(
        &mut self,
        dynamics: AnimationGraph,
        layers: Vec<Layer>,
    ) -> Result<(), ValidationError> {
        let mut value = self.0.wire_value().clone();
        value["dynamics"] = serde_json::to_value(dynamics)
            .map_err(|error| ValidationError::Wire(error.to_string()))?;
        value["layers"] = serde_json::to_value(layers)
            .map_err(|error| ValidationError::Wire(error.to_string()))?;
        let candidate = Self::from_value(value)?;
        *self = candidate;
        Ok(())
    }

    pub fn clone_with_id(&self, id: CompositionId) -> Self {
        let mut value = self.0.wire_value().clone();
        value["id"] = serde_json::to_value(id).expect("composition identifiers serialize");
        Self::from_value(value)
            .expect("replacing only the composition identifier preserves validity")
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        super::validation::validate(self)
    }
    pub fn composition_id(&self) -> &CompositionId {
        &self.0.data().id
    }
    pub fn id(&self) -> &str {
        self.0.data().id.as_str()
    }
    pub fn name(&self) -> &str {
        &self.0.data().name
    }
    pub fn layers(&self) -> &[Layer] {
        &self.0.data().layers
    }
    pub fn segment_layer_ids(&self) -> &[LayerId] {
        &self.0.data().segment_layer_ids
    }
    pub fn is_segment_layer(&self, id: LayerId) -> bool {
        self.segment_layer_ids().contains(&id)
            || self
                .layers()
                .iter()
                .any(|layer| layer.id() == id && matches!(layer.data(), LayerData::AiEdit(_)))
    }
    pub fn motion_blur(&self) -> MotionBlurSettings {
        self.0.data().motion_blur
    }
    pub fn dynamics(&self) -> &AnimationGraph {
        &self.0.data().dynamics
    }

    pub fn set_motion_blur(&mut self, settings: MotionBlurSettings) -> Result<(), ValidationError> {
        let mut value = self.0.wire_value().clone();
        value["motionBlur"] = crate::stored::checked_value(&settings)
            .map_err(|error| ValidationError::Wire(error.to_string()))?;
        let candidate = Self::from_value(value)?;
        *self = candidate;
        Ok(())
    }

    /// Equivalent to `unknown_fields().next().is_some()`, without materializing
    /// JSON trees or cloning unknown values. Stops at the first unknown record.
    pub fn has_unknown_fields(&self) -> bool {
        self.0.has_unknown_fields(&[(0, "layers"), (0, "dynamics")])
            || self.layers().iter().any(Layer::has_unknown_fields)
            || self.dynamics().has_unknown_fields()
    }

    pub fn unknown_fields(&self) -> impl Iterator<Item = (String, Value)> {
        let mut known =
            serde_json::to_value(self.0.data()).expect("checked composition data serializes");
        known["layers"] = Value::Array(self.layers().iter().map(Layer::known_value).collect());
        known["dynamics"] = self.dynamics().known_value();
        let mut fields = Vec::new();
        collect_unknown(self.0.wire_value(), &known, "", &mut fields);
        fields.into_iter()
    }
}

fn collect_unknown(raw: &Value, known: &Value, path: &str, fields: &mut Vec<(String, Value)>) {
    match (raw, known) {
        (Value::Object(raw), Value::Object(known)) => {
            for (key, value) in raw {
                let path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match known.get(key) {
                    Some(known) => collect_unknown(value, known, &path, fields),
                    None => fields.push((path, value.clone())),
                }
            }
        }
        (Value::Array(raw), Value::Array(known)) => {
            for (index, (raw, known)) in raw.iter().zip(known).enumerate() {
                collect_unknown(raw, known, &format!("{path}.{index}"), fields);
            }
        }
        _ => {}
    }
}

/// Structural validation failures for canonical editable compositions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("{0}")]
    Wire(String),
    /// A layer's active/source timing ranges are malformed.
    #[error("{0}")]
    LayerTiming(String),
    /// A PAG or AI Edit hierarchy invariant is malformed.
    #[error("{0}")]
    LayerHierarchy(String),
    /// A takeover media placement appears below the root stack.
    #[error("FX media takeover placement on layer {0} requires a root layer")]
    NestedMediaPlacement(LayerId),
    /// A read-only graph dependency addresses an unsupported layer kind.
    #[error(
        "FX composition layer {layer_id} ({layer_type}) does not support read-only property {property_type}"
    )]
    UnsupportedReadOnlyProperty {
        layer_id: LayerId,
        layer_type: &'static str,
        property_type: PropType,
    },
    /// A graph dependency or metadata input addresses a missing layer.
    #[error("FX composition layer {0} was not found")]
    MissingLayer(LayerId),
    /// An asset-metadata input addresses a non-media layer.
    #[error("FX composition layer {layer_id} ({layer_type}) does not support read-only property mediaSourceAssetId")]
    UnsupportedAssetMetadataSource {
        layer_id: LayerId,
        layer_type: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::FXComposition;
    use serde_json::json;

    #[test]
    fn unknown_presence_does_not_materialize_the_composition() {
        for wire in [
            json!({"id": "main", "name": "empty"}),
            json!({"id": "main", "name": "empty", "future": {"data": [1,2,3]}}),
        ] {
            let composition: FXComposition = serde_json::from_value(wire).unwrap();
            assert!(!composition.0.is_wire_materialized());
            let present = composition.has_unknown_fields();
            assert!(!composition.0.is_wire_materialized());
            assert_eq!(present, composition.unknown_fields().next().is_some());
        }
    }
}
