//! Stage flat pictures without changing the caller's export or packing state.
use super::super::{
    color_matte,
    nested::LayerExport,
    packing::PicturePacker,
    tesseract_to_premiere::{self as export, ClipLayers},
};
use crate::{
    error::{unsupported, Result},
    format::{MediaId, PrMedia},
    schema::{PrVideoItem, PrVideoOccurrence},
};
use fx_schema::{GroupLayer, LayerData};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn prepare(
    fades: [&GroupLayer; 2],
    context: &LayerExport<'_, '_>,
    media: &mut BTreeMap<MediaId, PrMedia>,
) -> Result<Vec<PrVideoOccurrence>> {
    let mut properties = BTreeMap::new();
    let mut written = export::WrittenAnimation::default();
    let mut audio = Vec::new();
    let mut motion_blur_written = false;
    let mut unmapped_group_motion_blur = Vec::new();
    let mut packer = PicturePacker::new(
        "Cross Dissolve",
        [context.width, context.height],
        context.frame_rate,
        0,
    );
    let container = packer.root();
    let mut local = LayerExport {
        dynamics: context.dynamics,
        property_tracks: &mut properties,
        written: &mut written,
        audio_facts: context.audio_facts,
        fonts: context.fonts,
        audio: &mut audio,
        media_facts: context.media_facts,
        authored_video_durations: context.authored_video_durations,
        natural_frames: context.natural_frames,
        media,
        packer: &mut packer,
        container,
        boundary: None,
        width: context.width,
        height: context.height,
        frame_rate: context.frame_rate,
        origin: context.origin,
        sampled_picture_end: context.sampled_picture_end,
        depth: context.depth,
        canvas: None,
        group_guide: None,
        in_moved_nest: context.in_moved_nest,
        nest_scale: context.nest_scale,
        motion_blur: context.motion_blur,
        motion_blur_written: &mut motion_blur_written,
        unmapped_group_motion_blur: &mut unmapped_group_motion_blur,
    };
    let mut omissions = Vec::new();
    let mut pictures = Vec::new();
    for fade in fades {
        let layer = &fade.layers[0];
        let record = format!("Cross Dissolve picture {}", layer.id());
        let clip = match layer.data() {
            LayerData::Video(video) => {
                let frame = export::source_frame(video.source.frame_rect)?;
                let (clip, source) = export::export_video_clip(
                    ClipLayers::Video(video),
                    video,
                    Some(fade.id),
                    false,
                    (frame, None),
                    &mut local,
                    &record,
                    &mut omissions,
                )?
                .ok_or_else(|| unsupported("a Cross Dissolve picture could not be exported"))?;
                export::source_media(local.media, &clip.media).video = Some(source);
                clip
            }
            LayerData::Image(image) => {
                let (clip, source) = super::super::still::export_image_layer(
                    image,
                    Some(fade.id),
                    None,
                    &mut local,
                    &mut omissions,
                    &record,
                )?
                .ok_or_else(|| unsupported("a Cross Dissolve still could not be exported"))?;
                local.media.entry(clip.media.clone()).or_insert(source);
                clip
            }
            LayerData::Rect(rect) => {
                let Some(PrVideoItem::Media(clip)) = color_matte::export_rect_layer(
                    rect,
                    Some(fade.id),
                    &BTreeSet::new(),
                    &fade.layers,
                    &mut local,
                    &mut omissions,
                    &record,
                )?
                else {
                    return Err(unsupported("a Cross Dissolve solid could not be exported"));
                };
                clip
            }
            _ => {
                return Err(unsupported(
                    "Cross Dissolve requires flat physical pictures",
                ))
            }
        };
        pictures.push(clip);
    }
    // The ordinary path must keep typed loss ownership and scoped diagnostics
    // when a child cannot export without loss. No speculative state escapes.
    if !omissions.is_empty() || !audio.is_empty() || motion_blur_written {
        return Err(unsupported(
            "Cross Dissolve pictures require ordinary scoped export",
        ));
    }
    Ok(pictures)
}
