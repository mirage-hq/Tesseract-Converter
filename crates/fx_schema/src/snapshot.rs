//! Baked composition data. Readers never migrate layer or effect records.
use crate::{stored::Stored, Color, CompositionId, Layer, MotionBlurSettings};
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
crate::define_snapshot_schema!(stored_snapshot, allow(dead_code));

#[derive(Debug, Clone, PartialEq)]
pub struct FxCompositionDocument<C>(Stored<SnapshotData<C>>);

pub type FxComposition = FxCompositionDocument<Composition>;

impl<C> Serialize for FxCompositionDocument<C> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, C: serde::de::DeserializeOwned> Deserialize<'de> for FxCompositionDocument<C> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Stored::deserialize(deserializer).map(Self)
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(as = "serde_json::Value")]
pub struct Composition(Stored<CompositionData>);

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
