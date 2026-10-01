//! Canonical persisted video and image layer schema.
use super::{
    model::{deserialize_persisted_image_source, deserialize_persisted_video_source},
    BlendMode, ImageSource as FxImageSource, LinearGain, MediaPlacement, PathMask, TrackMatte,
    Transform, VideoSource,
};
use crate::{
    effect::EffectRecord as EffectInstance, DonorCaptionPresentation, Duration, LayerId,
    LayerPlayback, TimeRangeProperty,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[path = "media_declaration.rs"]
mod media_declaration;

crate::define_media_layer_schema! {
    metadata_serde: [],
    video_serde: [],
    image_serde: [],
    start_time: Option<crate::ScalarProperty>,
    start_attrs: [serde(default, skip_serializing_if = "Option::is_none")],
    frame_blending: Option<super::media_record::FrameBlendingData>,
    frame_attrs: [serde(default, skip_serializing_if = "Option::is_none"), ts(type = "boolean | FrameBlendingMode")],
    video_source_attrs: [serde(deserialize_with = "deserialize_persisted_video_source")],
    image_source_attrs: [serde(deserialize_with = "deserialize_persisted_image_source")],
    image_effect_attrs: [serde(default, skip_serializing_if = "Vec::is_empty", deserialize_with = "crate::effect::deserialize_effects")]
}
