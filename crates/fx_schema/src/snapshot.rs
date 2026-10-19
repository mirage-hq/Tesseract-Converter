//! Baked composition data. Readers never migrate layer or effect records.
use crate::{stored::Stored, Color, ColorManagement, CompositionId, Layer, MotionBlurSettings};
use serde::{Deserialize, Serialize};

#[path = "snapshot_declaration.rs"]
mod declaration;
macro_rules! stored_snapshot {
    ($(#[$attrs:meta])* pub struct $name:ident<C> { $($fields:tt)* }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct SnapshotData<C> { $($fields)* }
    };
}
crate::define_snapshot_schema!(stored_snapshot, allow(dead_code), serde(default));

#[derive(Debug, Clone, PartialEq)]
pub struct FxCompositionDocument<C>(Stored<SnapshotData<C>>);

pub type FxComposition = FxCompositionDocument<Composition>;

impl<C> FxCompositionDocument<C> {
    /// Reject unsupported ACES rendering, retaining the policy for read/save.
    pub fn render_preflight(&self) -> Result<(), crate::ColorManagementError> {
        if let Some(composition) = self.0.wire_value().get("composition") {
            crate::validate_render_domains(&self.0.data().color_management, composition)?;
        }
        self.0.data().color_management.render_preflight()
    }
}

impl<C> Serialize for FxCompositionDocument<C> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = self.0.wire_value();
        if let Some(composition) = value.get("composition") {
            crate::validate_render_domains(&self.0.data().color_management, composition)
                .map_err(serde::ser::Error::custom)?;
        }
        self.0.serialize(serializer)
    }
}

impl<'de, C: serde::de::DeserializeOwned> Deserialize<'de> for FxCompositionDocument<C> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        if let Some(composition) = value.get("composition") {
            crate::reject_nested_color_management(composition).map_err(serde::de::Error::custom)?;
        }
        let object = value
            .as_object_mut()
            .ok_or_else(|| serde::de::Error::custom("expected FX document object"))?;
        let policy = object
            .get("colorManagement")
            .map(|value| ColorManagement::from_value(value.clone()))
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        if let Some(composition) = object.get("composition") {
            crate::validate_render_domains(&policy, composition)
                .map_err(serde::de::Error::custom)?;
        }
        object.insert(
            "colorManagement".into(),
            serde_json::to_value(policy).map_err(serde::de::Error::custom)?,
        );
        Stored::from_value(value)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

impl<C: Serialize + serde::de::DeserializeOwned> FxCompositionDocument<C> {
    pub fn from_data(data: &SnapshotData<C>) -> Result<Self, serde_json::Error> {
        Stored::from_data(data).map(Self)
    }
    pub fn data(&self) -> &SnapshotData<C> {
        self.0.data()
    }
}

macro_rules! stored_snapshot_composition {
    ($(#[$attrs:meta])* pub struct $name:ident { $($fields:tt)* }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
        #[serde(rename_all = "camelCase")]
        pub struct CompositionData { $($fields)* }
    };
}
crate::define_snapshot_composition_schema!(
    stored_snapshot_composition,
    serde(default),
    allow(dead_code),
    serde(with = "crate::time::serde_secs::duration")
);

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct Composition(Stored<CompositionData>);

impl<'de> Deserialize<'de> for Composition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let composition = Self(Stored::deserialize(deserializer)?);
        if composition.0.contains_key("colorManagement") {
            return Err(serde::de::Error::custom(
                crate::ColorManagementError::Misplaced("composition".into()),
            ));
        }
        Ok(composition)
    }
}

impl Composition {
    pub fn from_data(data: &CompositionData) -> Result<Self, serde_json::Error> {
        Stored::from_data(data).map(Self)
    }
    pub fn data(&self) -> &CompositionData {
        self.0.data()
    }
}

impl AsRef<Composition> for Composition {
    fn as_ref(&self) -> &Self {
        self
    }
}
