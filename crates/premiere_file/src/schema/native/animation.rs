//! Typed clip-local intrinsic Motion records.

use super::{Node, ObjectId, Reference, ReferenceList, RetainedOrSkipped};
use serde::{Deserialize, Deserializer, Serialize};

/// Shared ObjectID marker for scalar and point Motion parameters.
#[derive(Debug)]
pub(crate) enum MotionParamId {}

/// Shared ObjectID marker for Premiere's intrinsic source-time mapping.
#[derive(Debug)]
pub(crate) enum TimeParamId {}

#[derive(Debug, Serialize)]
pub(crate) struct MotionComponents {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "Component")]
    pub(crate) items: Vec<Reference>,
}

impl<'de> Deserialize<'de> for MotionComponents {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = ReferenceList::deserialize(deserializer)?;
        Ok(Self {
            version: None,
            items: references.items,
        })
    }
}

impl MotionComponents {
    pub(crate) fn from_ids(ids: impl IntoIterator<Item = ObjectId<VideoFilterComponent>>) -> Self {
        Self {
            version: Some("1".into()),
            items: ids
                .into_iter()
                .enumerate()
                .map(|(index, id)| Reference::indexed_object(index, id))
                .collect(),
        }
    }
}

/// One parameter collection: native children may use any tag; writing uses Param.
#[derive(Debug, Serialize)]
pub(crate) struct MotionParams {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "Param")]
    pub(crate) items: Vec<Reference>,
}

impl<'de> Deserialize<'de> for MotionParams {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = ReferenceList::deserialize(deserializer)?;
        Ok(Self {
            version: None,
            items: references.items,
        })
    }
}

impl MotionParams {
    pub(crate) fn from_ids(ids: impl IntoIterator<Item = ObjectId<MotionParamId>>) -> Self {
        Self {
            version: Some("1".into()),
            items: ids
                .into_iter()
                .enumerate()
                .map(|(index, id)| Reference::indexed_object(index, id))
                .collect(),
        }
    }
}

/// The mask components (`AE.ADBE AEMask`) that a component owns. Every
/// corpus save writes it between `Component` (or the private data) and
/// `MatchName`, with one `SubComponent` per mask.
#[derive(Debug, Serialize)]
pub(crate) struct SubComponents {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "SubComponent")]
    pub(crate) items: Vec<Reference>,
}

impl<'de> Deserialize<'de> for SubComponents {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = ReferenceList::deserialize(deserializer)?;
        Ok(Self {
            version: None,
            items: references.items,
        })
    }
}

impl SubComponents {
    pub(crate) fn from_ids(ids: impl IntoIterator<Item = ObjectId<VideoFilterComponent>>) -> Self {
        Self {
            version: Some("1".into()),
            items: ids
                .into_iter()
                .enumerate()
                .map(|(index, id)| Reference::indexed_object(index, id))
                .collect(),
        }
    }
}

/// One video component. Every reader that decodes one must accept or reject
/// its `sub_components`: only the intrinsic Opacity converts a mask
/// (`reader/mask.rs`).
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct VideoFilterComponent {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<VideoFilterComponent>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) component: Option<MotionBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) premiere_filter_private_data: Option<RetainedOrSkipped<MotionPrivateData>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sub_components: Option<SubComponents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) match_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) video_filter_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct MotionBody {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) params: Option<MotionParams>,
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<String>,
    /// The layer name that Essential Graphics shows for a graphic Text component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) instance_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bypass: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) intrinsic: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MotionPrivateData {
    #[serde(rename = "@Encoding")]
    pub(crate) encoding: &'static str,
    #[serde(rename = "@BinaryHash")]
    pub(crate) binary_hash: String,
    #[serde(rename = "$text")]
    pub(crate) value: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoComponentParam {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<MotionParamId>,
    #[serde(rename = "@ClassID")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    /// Premiere omits `Name` on some graphic parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_time_varying: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) discontinuous_interpolate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parameter_control_type: Option<String>,
    pub(crate) start_keyframe: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keyframes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lower_bound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) upper_bound: Option<String>,
    #[serde(rename = "ParameterID")]
    pub(crate) parameter_id: String,
    #[serde(rename = "LowerUIBound", skip_serializing_if = "Option::is_none")]
    pub(crate) lower_ui_bound: Option<String>,
    #[serde(rename = "UpperUIBound", skip_serializing_if = "Option::is_none")]
    pub(crate) upper_ui_bound: Option<String>,
    /// No Premiere save writes it on an intrinsic Motion or Opacity parameter,
    /// so their readers reject any value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bypass: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct TimeRemapping {
    #[serde(rename = "@ObjectID")]
    pub(crate) _object_id: ObjectId<TimeRemapping>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: String,
    #[serde(rename = "@Version")]
    pub(crate) version: String,
    pub(crate) keyframes: Reference,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct TimeComponentParam {
    #[serde(rename = "@ObjectID")]
    pub(crate) _object_id: ObjectId<TimeParamId>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: String,
    #[serde(rename = "@Version")]
    pub(crate) version: String,
    pub(crate) name: String,
    pub(crate) is_time_varying: String,
    pub(crate) is_locked: String,
    pub(crate) discontinuous_interpolate: String,
    pub(crate) parameter_control_type: String,
    pub(crate) start_keyframe: String,
    pub(crate) keyframes: String,
    #[serde(rename = "CurrentValue")]
    pub(crate) _current_value: String,
    #[serde(rename = "ParameterID")]
    pub(crate) parameter_id: String,
    pub(crate) range_locked: String,
    pub(crate) lower_bound: String,
    #[serde(rename = "UpperBound")]
    pub(crate) _upper_bound: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PointComponentParam {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<MotionParamId>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) name: &'static str,
    /// Graphic point parameters omit this flag and the control type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_time_varying: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parameter_control_type: Option<&'static str>,
    pub(crate) start_keyframe: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keyframes: Option<String>,
    #[serde(rename = "ParameterID")]
    pub(crate) parameter_id: usize,
}

/// Panel expansion state that Premiere stores on the Source Text parameter.
#[derive(Debug, Serialize)]
pub(crate) struct GraphicsProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "ECP.Graphics.Expanded")]
    pub(crate) expanded: &'static str,
}

/// A binary value stored as base64 text.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct EncodedValue {
    #[serde(rename = "@Encoding")]
    pub(crate) encoding: String,
    #[serde(rename = "@BinaryHash", skip_serializing_if = "Option::is_none")]
    pub(crate) binary_hash: Option<String>,
    /// Empty when Premiere stored the same value earlier under this hash.
    #[serde(rename = "$text", default)]
    pub(crate) value: String,
}

/// An arbitrary-data parameter, such as a graphic's Source Text.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct ArbVideoComponentParam {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<MotionParamId>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(default, skip_serializing_if = "RetainedOrSkipped::is_skipped")]
    pub(crate) node: RetainedOrSkipped<Node<GraphicsProperties>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parameter_control_type: Option<String>,
    // The fields below follow the order in which Premiere 26.5.1 saves a
    // keyed Source Text: `IsTimeVarying`, `ParameterID`,
    // `StartKeyframePosition`, `Keyframes`, `StartKeyframeValue`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_time_varying: Option<String>,
    #[serde(rename = "ParameterID")]
    pub(crate) parameter_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_keyframe_position: Option<String>,
    /// Source Text keys, `ticks,base64;` per key, each a complete document
    /// (Premiere 14 corpus projects and the Premiere 26.5.1 save).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keyframes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_keyframe_value: Option<EncodedValue>,
}
