//! Still-image variants of the shared native media records.
//!
//! The field set follows Premiere's own still streams: `IsStill`, the
//! sequence frame rate (as on the generator stills of the 30 fps
//! `corporate_slideshow`), the twelve-hour synthetic duration, and a `RAW `
//! codec code, with no source color space or orientation (`stills_and_panorama`
//! JPEG, `phone_title` and `cinemagraph` RGBA PNG). `AlphaType` `1` is written
//! only when the file carries straight alpha: observed for RGBA PNG, inferred
//! for grey+alpha and `tRNS` PNG. `IsOverridenImageOrientationType`, whose
//! meaning is unknown, is not written.

use super::graph::MediaIds;
use crate::schema::{
    native::VideoStream, records, PrVideoStream, STILL_CODEC_TYPE, STILL_STRAIGHT_ALPHA_TYPE,
};

pub(super) fn video_stream(spec: &PrVideoStream, alpha: bool, ids: &MediaIds) -> VideoStream {
    VideoStream {
        object_id: ids.stream,
        class_id: Some(records::VIDEO_STREAM.class_id.to_owned()),
        version: Some(records::VIDEO_STREAM.version.to_owned()),
        is_still: Some("true".to_owned()),
        is_continuous_time: None,
        alpha_info_is_uncertain: None,
        is_overriden_image_orientation_type: None,
        frame_rate: Some(spec.frame_rate.ticks_per_frame().to_string()),
        is_frame_rate_overridden: None,
        overidden_frame_rate: None,
        duration: Some(spec.intrinsic_ticks.to_string()),
        ignore_alpha: None,
        frame_rect: Some(format!("0,0,{},{}", spec.width, spec.height)),
        pixel_aspect_ratio: None,
        codec_type: Some(STILL_CODEC_TYPE.to_owned()),
        original_color_space: None,
        alpha_type: alpha.then(|| STILL_STRAIGHT_ALPHA_TYPE.to_owned()),
        field_type_is_uncertain: Some("true".to_owned()),
        original_field_type: None,
        original_image_orientation_type: None,
    }
}
