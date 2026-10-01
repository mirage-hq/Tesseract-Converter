//! Shared native reference shapes.

use super::{ObjectId, Uid};
use serde::{Deserialize, Serialize};

/// A native graph edge. Premiere uses either an object ID or an object UID.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Reference {
    #[serde(rename = "@ObjectRef", skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(rename = "@ObjectURef", skip_serializing_if = "Option::is_none")]
    pub(crate) uid: Option<String>,
    #[serde(rename = "@Index", skip_serializing_if = "Option::is_none")]
    pub(crate) index: Option<String>,
}

impl Reference {
    pub(crate) fn object<T>(id: ObjectId<T>) -> Self {
        Self {
            id: Some(id.value.to_string()),
            uid: None,
            index: None,
        }
    }

    pub(crate) fn indexed_object<T>(index: usize, id: ObjectId<T>) -> Self {
        Self {
            index: Some(index.to_string()),
            ..Self::object(id)
        }
    }

    pub(crate) fn uid<T>(id: Uid<T>) -> Self {
        Self {
            id: None,
            uid: Some(id.as_native_string()),
            index: None,
        }
    }

    pub(crate) fn indexed_uid<T>(index: usize, id: Uid<T>) -> Self {
        Self {
            index: Some(index.to_string()),
            ..Self::uid(id)
        }
    }
}

/// References stored as element children whose tag is not semantically relevant.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ReferenceList {
    #[serde(rename = "$value", default)]
    pub(crate) items: Vec<Reference>,
}
