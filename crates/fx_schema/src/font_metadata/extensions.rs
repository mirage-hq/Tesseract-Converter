use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS};

const TS_OPEN_STRING_INDEX_OBJECT: &str = "{ [key in string]: unknown }";

/// Forward-compatible fields preserved when an older reader rewrites metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetDataExtensions(BTreeMap<String, serde_json::Value>);

impl AssetDataExtensions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert one forward-compatible field.
    pub fn insert(&mut self, key: String, value: serde_json::Value) -> Option<serde_json::Value> {
        self.0.insert(key, value)
    }

    /// Read one forward-compatible field.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.get(key)
    }
}

impl Extend<(String, serde_json::Value)> for AssetDataExtensions {
    fn extend<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = (String, serde_json::Value)>,
    {
        self.0.extend(iter);
    }
}

impl IntoIterator for AssetDataExtensions {
    type Item = (String, serde_json::Value);
    type IntoIter = std::collections::btree_map::IntoIter<String, serde_json::Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl TS for AssetDataExtensions {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_cfg: &Config) -> String {
        TS_OPEN_STRING_INDEX_OBJECT.to_string()
    }

    fn inline(_cfg: &Config) -> String {
        TS_OPEN_STRING_INDEX_OBJECT.to_string()
    }

    fn inline_flattened(_cfg: &Config) -> String {
        format!("({TS_OPEN_STRING_INDEX_OBJECT})")
    }
}
