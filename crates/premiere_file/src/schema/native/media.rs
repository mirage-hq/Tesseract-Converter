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
    /// Interpret Footage colour state; Version 19 saves all four (`{}` and a
    /// nil GUID when unset), Version 21 only `InputLUTSpecified`. Premiere
    /// 26.5.2 (Version 23) saves an active Color Space Override as
    /// `OverriddenColorSpace` plus `IsColorSpacOverridden` `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) selected_color_space: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) overridden_color_space: Option<String>,
    /// Native spelling.
    #[serde(
        rename = "IsColorSpacOverridden",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) is_color_space_overridden: Option<String>,
    #[serde(rename = "InputLUTSpecified", skip_serializing_if = "Option::is_none")]
    pub(crate) input_lut_specified: Option<String>,
    #[serde(rename = "InputLUTID", skip_serializing_if = "Option::is_none")]
    pub(crate) input_lut_id: Option<String>,
    /// Premiere 26.5.2 (Version 23) saves a browsed Input LUT as this edge to
    /// a [`LutAsset`] plus its two names; see `schema::PrClipInputLut`.
    #[serde(rename = "InputLUTAsset", skip_serializing_if = "Option::is_none")]
    pub(crate) input_lut_asset: Option<Reference>,
    #[serde(
        rename = "InputLUTDisplayName",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) input_lut_display_name: Option<String>,
    #[serde(rename = "InputLUTFileName", skip_serializing_if = "Option::is_none")]
    pub(crate) input_lut_file_name: Option<String>,
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

/// A LUT registered with Premiere (`LUTAsset`, Version 1): its registry GUID,
/// names and embedded samples (`cube_lut::Lut3d::from_itx3`).
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LutAsset {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<LutAsset>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "Id", skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(rename = "Displayname", skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<String>,
    #[serde(rename = "Filename", skip_serializing_if = "Option::is_none")]
    pub(crate) file_name: Option<String>,
    #[serde(rename = "Blob", skip_serializing_if = "Option::is_none")]
    pub(crate) blob: Option<LutBlob>,
}

/// The base64 samples of a [`LutAsset`]; empty when an earlier element
/// stores the same `BinaryHash`.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LutBlob {
    #[serde(rename = "@Encoding")]
    pub(crate) encoding: String,
    #[serde(rename = "@BinaryHash")]
    pub(crate) binary_hash: String,
    #[serde(rename = "$text", default)]
    pub(crate) value: String,
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
