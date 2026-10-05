//! Writes the observed progressive, square-pixel, straight-alpha AE link profile.

use super::graph::MediaIds;
use crate::schema::{
    after_effects::CODEC, native::VideoStream, records, ColorSpace, PrVideoStream,
};

pub(super) fn video_stream(video: &PrVideoStream, ids: &MediaIds) -> VideoStream {
    VideoStream {
        object_id: ids.stream,
        class_id: Some(records::VIDEO_STREAM.class_id.to_owned()),
        version: Some(records::VIDEO_STREAM.version.to_owned()),
        frame_rate: Some(video.frame_rate.ticks_per_frame().to_string()),
        is_frame_rate_overridden: None,
        overidden_frame_rate: None,
        duration: Some(video.intrinsic_ticks.to_string()),
        frame_rect: Some(format!("0,0,{},{}", video.width, video.height)),
        codec_type: Some(CODEC.to_owned()),
        original_color_space: Some(
            serde_json::to_string(&ColorSpace::sequence_sdr())
                .expect("native RGB color fields serialize"),
        ),
        alpha_type: Some("1".to_owned()),
        original_field_type: Some("4".to_owned()),
        ignore_alpha: None,
        pixel_aspect_ratio: None,
        original_par: None,
        is_par_overridden: None,
        overridden_par: None,
        is_numbered_stills: None,
        is_still: None,
        is_continuous_time: None,
        is_overriden_image_orientation_type: None,
        alpha_info_is_uncertain: None,
        field_type_is_uncertain: None,
        original_image_orientation_type: None,
    }
}
