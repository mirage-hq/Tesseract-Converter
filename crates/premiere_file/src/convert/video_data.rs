//! A conversion-only view of both persisted video spellings.
//!
//! This maps declared fields, not runtime migrations: no clocks, shader presets,
//! identifiers, or missing timing are inferred, and the archive is never edited.

use std::borrow::Cow;

use fx_schema::{
    layer::MediaFitData, Layer, LayerData, MediaFit, MediaSourceKind, VideoLayer, VideoSource,
};

use crate::error::{unsupported, Result};

pub(crate) fn video_data(layer: &Layer) -> Result<Option<Cow<'_, VideoLayer>>> {
    let legacy = match layer.data() {
        LayerData::Video(video) => return Ok(Some(Cow::Borrowed(video))),
        LayerData::Media(media) if media.source.kind == MediaSourceKind::Video => media,
        _ => return Ok(None),
    };
    let source_range = legacy.source_range.ok_or_else(|| {
        unsupported(format!(
            "layer {}: sourceRange is required for video media",
            legacy.id
        ))
    })?;
    let source_intrinsic_duration = legacy.source_intrinsic_duration.ok_or_else(|| {
        unsupported(format!(
            "layer {}: sourceIntrinsicDuration is required for video media",
            legacy.id
        ))
    })?;
    let fit = match &legacy.source.fit {
        None | Some(MediaFitData::Contain) => MediaFit::Contain,
        Some(MediaFitData::None) => MediaFit::None,
        Some(MediaFitData::Cover) => MediaFit::Cover,
        Some(MediaFitData::Stretch) => MediaFit::Stretch,
        Some(MediaFitData::Custom {
            scale,
            content_center,
        }) => MediaFit::Custom {
            scale: *scale,
            content_center: *content_center,
        },
    };
    Ok(Some(Cow::Owned(VideoLayer {
        id: legacy.id,
        name: legacy.name.clone(),
        description: legacy.description.clone(),
        metadata: None,
        is_hidden: legacy.is_hidden,
        parent: legacy.parent,
        start_time: legacy.start_time,
        blend_mode: legacy.blend_mode,
        track_matte: legacy.track_matte.clone(),
        masks: legacy.masks.clone(),
        corner_radius: legacy.corner_radius,
        source_range,
        playback: match &legacy.playback {
            Some(property) => {
                fx_schema::LayerPlayback::remapped(legacy.active_range, property.clone(), 0)
            }
            None => fx_schema::LayerPlayback::linear(
                legacy.active_range,
                legacy.active_range,
                source_range,
                0,
            ),
        }
        .map_err(unsupported)?,
        preserve_audio_pitch: false,
        source_intrinsic_duration,
        volume: legacy.volume,
        effects: legacy.effects.clone(),
        placement: legacy.placement,
        captions_enabled: legacy.captions_enabled,
        caption_presentation: legacy.caption_presentation.clone(),
        frame_blending: legacy.frame_blending.clone(),
        motion_blur: legacy.motion_blur,
        transform: legacy.transform,
        source: VideoSource {
            asset_id: legacy.source.asset_id.clone(),
            eye_contact: None,
            audio_enhancement: None,
            input_transform: legacy.source.input_transform.clone(),
            frame_rect: legacy.source.source_rect,
            fit,
            time_remap: legacy.source.time_remap,
        },
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn wire(kind: &str) -> Value {
        let document: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/editable-video.json")).unwrap();
        let mut layer = document["composition"]["layers"][0].clone();
        layer["activeRange"] = layer["playback"]["inputRange"].clone();
        layer.as_object_mut().unwrap().remove("playback");
        layer["type"] = json!("Media");
        layer["source"]["kind"] = json!(kind);
        layer
    }

    #[test]
    fn legacy_view_retains_fields_without_rewriting_the_record() {
        let mut wire = wire("video");
        wire["startTime"] = json!(2.5);
        wire["source"]["fit"] = json!("none");
        wire["frameBlending"] = json!(true);
        wire["future"] = json!({"retained": true});
        let layer: Layer = serde_json::from_value(wire.clone()).unwrap();
        let view = video_data(&layer).unwrap().unwrap();
        assert_eq!(view.start_time, Some(2.5));
        assert_eq!(view.source_range.start, fx_schema::Time::ZERO);
        assert_eq!(view.source.fit, MediaFit::None);
        assert_eq!(
            view.frame_blending,
            Some(fx_schema::layer::FrameBlendingData::Boolean(true))
        );
        assert_eq!(serde_json::to_value(&layer).unwrap(), wire);
    }

    #[test]
    fn legacy_video_timing_is_required_not_inferred() {
        for key in ["sourceRange", "sourceIntrinsicDuration"] {
            let mut wire = wire("video");
            wire.as_object_mut().unwrap().remove(key);
            let layer: Layer = serde_json::from_value(wire).unwrap();
            assert!(video_data(&layer).unwrap_err().to_string().contains(key));
        }
    }

    #[test]
    fn legacy_image_is_not_inspected_as_video() {
        let layer: Layer = serde_json::from_value(wire("image")).unwrap();
        assert!(video_data(&layer).unwrap().is_none());
    }
}
