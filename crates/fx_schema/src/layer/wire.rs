//! Immutable, lossless layer records.
#[path = "wire_declaration.rs"]
mod wire_declaration;
use super::{BooleanOperationLayer, LayerData};
use crate::{stored::Stored, LayerId, TimeRangeProperty};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct Layer(Stored<LayerData>);

impl<'de> Deserialize<'de> for Layer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let layer = Self(Stored::deserialize(deserializer)?);
        if matches!(
            layer.data(),
            LayerData::Video(_) | LayerData::Audio(_) | LayerData::Group(_)
        ) && layer.0.contains_key("activeRange")
        {
            return Err(serde::de::Error::custom(
                "windowed playback cannot contain legacy activeRange",
            ));
        }
        if matches!(layer.data(), LayerData::Group(_)) && layer.0.contains_key("sourceRange") {
            return Err(serde::de::Error::custom(
                "Group playback cannot contain sourceRange",
            ));
        }
        layer
            .validate_timing_ranges()
            .map_err(serde::de::Error::custom)?;
        Ok(layer)
    }
}

impl Layer {
    pub fn from_data(data: &LayerData) -> Result<Self, serde_json::Error> {
        serde_json::from_value(crate::stored::checked_value(data)?)
    }
    pub fn data(&self) -> &LayerData {
        self.0.data()
    }
    pub fn wire_value(&self) -> &Value {
        self.0.wire_value()
    }
    pub fn name(&self) -> &str {
        self.data().name()
    }
    pub fn id(&self) -> LayerId {
        self.data().id()
    }
    pub fn parent_id(&self) -> Option<LayerId> {
        self.data().parent_id()
    }
    pub fn child_layers(&self) -> Option<&[Layer]> {
        self.data().child_layers()
    }
    pub fn effects(&self) -> &[crate::effect::EffectRecord] {
        self.data().effects()
    }
    pub fn active_range(&self) -> TimeRangeProperty {
        self.data().active_range()
    }
    pub fn supports_source_range(&self) -> bool {
        self.data().supports_source_range()
    }
    pub fn layer_type_name(&self) -> &'static str {
        self.data().layer_type_name()
    }
    pub fn validate_timing_ranges(&self) -> Result<(), String> {
        self.data().validate_timing_ranges()
    }

    pub fn known_value(&self) -> Value {
        let mut value = serde_json::to_value(self.data()).expect("checked layer data serializes");
        if let Some(children) = self.child_layers() {
            value["layers"] = Value::Array(children.iter().map(Self::known_value).collect());
        }
        if value.get("effects").is_some() {
            value["effects"] = Value::Array(
                self.effects()
                    .iter()
                    .map(crate::effect::EffectRecord::known_value)
                    .collect(),
            );
        }
        value
    }
}

pub(super) fn deserialize_boolean<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BooleanOperationLayer, D::Error> {
    let raw = Value::deserialize(deserializer)?;
    if raw.get("playback").is_some() {
        return Err(serde::de::Error::custom(
            "playback is only valid on Group, Video, or Audio layers",
        ));
    }
    serde_json::from_value(raw).map_err(serde::de::Error::custom)
}
