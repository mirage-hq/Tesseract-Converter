//! Generic data derived from one project asset.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{LayerId, Time};

/// Maximum number of named metadata entries stored on one physical asset.
pub const MAX_ASSET_METADATA_ENTRIES: usize = 32;

/// Maximum UTF-8 byte length of one semantic metadata name.
pub const MAX_ASSET_METADATA_NAME_BYTES: usize = 64;

/// Maximum number of named layer references on one animator.
pub const MAX_ANIMATOR_LAYER_REFS: usize = 16;

/// Named metadata attached to one project asset.
pub type AssetMetadataMap = BTreeMap<String, AssetMetadata>;

/// One layer exposed to a JavaScript animator through `input.refs`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct LayerRef {
    /// Referenced layer. Copy and remint operations rewrite this field.
    pub layer_id: LayerId,
}

/// Stable JavaScript alias to a remappable layer reference.
pub type LayerRefMap = BTreeMap<String, LayerRef>;

/// Host-resolved metadata map for one referenced layer's backing asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetMetadataSource<'a> {
    pub metadata: Option<&'a AssetMetadataMap>,
    /// Referenced media occurrence mapped onto its backing asset clock.
    pub source_time: Time,
}

/// Raw named data derived from an asset.
///
/// The schema describes the stored value. It does not prescribe time,
/// interpolation, storage, or domain semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub struct AssetMetadata {
    /// JSON Schema for the raw value exposed to consumers.
    #[serde(rename = "valueSchema")]
    #[ts(rename = "valueSchema")]
    #[ts(type = "unknown")]
    pub value_schema: serde_json::Value,
    /// Raw JSON stored with the project. It must match `value_schema`.
    #[ts(type = "unknown")]
    pub value: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::AssetMetadata;

    #[test]
    fn generic_inline_metadata_round_trips_without_domain_variants() {
        let metadata: AssetMetadata = serde_json::from_value(json!({
            "valueSchema": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["timeMs", "value"]
                }
            },
            "value": [
                {"timeMs": 0, "value": {"x": 10, "y": 20}},
                {"timeMs": 100, "value": {"x": 20, "y": 30}}
            ]
        }))
        .expect("generic metadata should deserialize");

        let value = serde_json::to_value(metadata).expect("metadata should serialize");
        assert_eq!(value["value"][1]["timeMs"], 100);
        assert!(value.get("interpolation").is_none());
    }

    #[test]
    fn asset_id_is_an_ordinary_schema_property() {
        let metadata: AssetMetadata = serde_json::from_value(json!({
            "valueSchema": {
                "type": "object",
                "properties": {"assetId": {"type": "string"}},
                "required": ["assetId"]
            },
            "value": {"assetId": "mask-asset"}
        }))
        .expect("generic metadata should deserialize");

        assert_eq!(metadata.value["assetId"], "mask-asset");
    }
}
