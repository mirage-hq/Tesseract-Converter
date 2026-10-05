use super::{Content, ObjectId, Reference};
use serde::{Deserialize, Serialize};

/// One native stream record; metadata absent from older input is filled by the writer.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoStream {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<VideoStream>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    /// Native spelling; seen only on still streams.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_overriden_image_orientation_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_rate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_frame_rate_overridden: Option<String>,
    /// Native spelling; the original FrameRate may also be present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) overidden_frame_rate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ignore_alpha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_rect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pixel_aspect_ratio: Option<String>,
    #[serde(rename = "OriginalPAR", skip_serializing_if = "Option::is_none")]
    pub(crate) original_par: Option<String>,
    #[serde(rename = "IsPAROverridden", skip_serializing_if = "Option::is_none")]
    pub(crate) is_par_overridden: Option<String>,
    #[serde(rename = "OverriddenPAR", skip_serializing_if = "Option::is_none")]
    pub(crate) overridden_par: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) codec_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_still: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_numbered_stills: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_continuous_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) original_color_space: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) alpha_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) alpha_info_is_uncertain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) field_type_is_uncertain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) original_field_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) original_image_orientation_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ModificationState {
    #[serde(rename = "@Encoding")]
    pub(crate) encoding: String,
    #[serde(rename = "@BinaryHash")]
    pub(crate) binary_hash: String,
    /// Empty when Premiere stored the same value earlier under this hash.
    #[serde(rename = "$text", default)]
    pub(crate) value: String,
}

/// Importer-private media settings, such as a Color Matte colour.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ImporterPrefs {
    #[serde(rename = "@Encoding")]
    pub(crate) encoding: String,
    #[serde(rename = "@BinaryHash")]
    pub(crate) binary_hash: String,
    /// Empty on ordinary file media (`<ImporterPrefs Encoding=".." BinaryHash=".."/>`).
    #[serde(rename = "$text", default)]
    pub(crate) value: String,
}

/// A native Media record. The XML ID is opaque; UUID creation is writer policy.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Media {
    #[serde(rename = "@ObjectID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_id: Option<String>,
    #[serde(rename = "@ObjectUID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_uid: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) video_stream: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) importer_prefs: Option<ImporterPrefs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) modification_state: Option<ModificationState>,
    #[serde(rename = "RelativePath", default)]
    pub(crate) relative_paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) file_path: Option<String>,
    #[serde(rename = "ImplementationID", skip_serializing_if = "Option::is_none")]
    pub(crate) implementation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// Present on stills and synthetic generator media, such as graphics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) infinite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) file_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content_and_metadata_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) actual_media_file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) conformed_audio_rate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) audio_stream: Option<Reference>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioStream {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioStream>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) audio_channel_layout: String,
    pub(crate) duration: String,
    /// Premiere ticks per sample.
    pub(crate) frame_rate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sample_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioMediaSource {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioMediaSource>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_source: Option<MediaSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) original_duration: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct MediaSource {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media: Option<Reference>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoMediaSource {
    #[serde(rename = "@ObjectID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_id: Option<String>,
    #[serde(rename = "@ObjectUID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_uid: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_source: Option<MediaSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) original_duration: Option<String>,
}
