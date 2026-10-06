//! Adjustment layers are flagged generator placements, not source pictures.

use super::{integer, required, required_integer, stream_dimensions, video::native_bool};
use crate::error::{ensure, Result};
use crate::format::{Graph, Located};
use crate::schema::{
    adjustment::ADJUSTMENT_LAYER_NAME,
    color_matte::GENERATOR_IMPLEMENTATION_ID,
    native::{Media, SubClip, VideoClip, VideoClipTrackItem, VideoStream},
    records, PrMedia, PrMediaKind, PrVideoStream,
};

/// Whether the placement carries the native adjustment flag.
/// Traversal failures remain the media reader's errors; `read_placement`
/// reads the flag strictly.
pub(super) fn is_flagged(graph: &Graph<'_>, item: &Located<VideoClipTrackItem>) -> bool {
    let Some(sub_reference) = item
        .value
        .clip_track_item
        .as_ref()
        .and_then(|clip_item| clip_item.sub_clip.as_ref())
    else {
        return false;
    };
    let Ok(sub) = graph.follow::<SubClip>(sub_reference, &item.identity) else {
        return false;
    };
    graph
        .follow::<VideoClip>(&sub.value.clip, &sub.identity)
        .is_ok_and(|clip| clip.value.adjustment_layer.as_deref() == Some("true"))
}

/// An `AdjustmentLayer` or `IsAdjustmentLayer` flag, `false` when absent.
pub(super) fn flag(value: Option<&str>, identity: &str, field: &str) -> Result<bool> {
    value
        .map(|value| native_bool(value, identity, field))
        .transpose()
        .map(|flag| flag.unwrap_or(false))
}

/// Read a flagged generator without treating its optional host metadata as picture data.
/// The flag owns adjustment semantics; generator subtype tags, Infinite and
/// IsStill do not change its effect coverage. Generator identity and absence of
/// file-backed/audio content remain required. Each placement checks the stream
/// dimensions against its sequence (`video::read_occurrence`).
pub(super) fn read_adjustment_media(graph: &Graph<'_>, media: Located<Media>) -> Result<PrMedia> {
    let identity = &media.identity;
    let value = &media.value;
    ensure!(
        value.implementation_id.as_deref() == Some(GENERATOR_IMPLEMENTATION_ID)
            && value.relative_paths.is_empty()
            && value.audio_stream.is_none(),
        "{identity}: an AdjustmentLayer clip requires generator media without file-backed or audio content"
    );
    let stream_reference = required(
        value.video_stream.as_ref(),
        identity,
        records::VIDEO_STREAM.tag,
    )?;
    let stream = graph.follow::<VideoStream>(stream_reference, identity)?;
    // Explicit FrameRate keeps precedence: Premiere also saves a 29.97
    // original alongside a 30 fps override in the older fixture.
    let frame_rate_ticks = if let Some(rate) = stream.value.frame_rate.as_deref() {
        integer(rate, &format!("{}: invalid FrameRate", stream.identity))?
    } else {
        required_integer(
            stream.value.overidden_frame_rate.as_deref(),
            &stream.identity,
            "OveriddenFrameRate",
        )?
    };
    let frame_rate = super::frame_rate(frame_rate_ticks, &stream.identity)?;
    let [width, height] = stream_dimensions(&stream)?;
    let intrinsic_ticks = required_integer(
        stream.value.duration.as_deref(),
        &stream.identity,
        "Duration",
    )?;
    Ok(PrMedia {
        name: ADJUSTMENT_LAYER_NAME.to_owned(),
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: Some(PrVideoStream {
            pixel_aspect: Default::default(),
            interpretation: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks,
            frame_rate: frame_rate.into(),
            width,
            height,
            kind: PrMediaKind::Adjustment,
        }),
        audio: None,
    })
}
