//! Adjustment layers: flagged placements of Black Video generator media
//! (`schema/adjustment.rs`).

use super::{integer, required, required_integer, stream_dimensions, video::native_bool};
use crate::error::{ensure, Result};
use crate::format::{Graph, Located};
use crate::schema::{
    adjustment::{ADJUSTMENT_LAYER_NAME, BLACK_VIDEO_FILE_PATH},
    color_matte::GENERATOR_IMPLEMENTATION_ID,
    native::{Media, SubClip, VideoClip, VideoClipTrackItem, VideoStream},
    records, PrMedia, PrMediaKind, PrVideoStream,
};

/// Whether `item` places a clip whose `AdjustmentLayer` flag is `true`, so
/// that its Black Video media is read as an adjustment layer rather than as a
/// graphic generator. Any traversal failure returns `false`, so the media
/// reader keeps its own errors; `read_placement` reads the flag strictly.
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

/// Read the `Media` record of a flagged placement, which must be the Black
/// Video generator: no file, paths or audio, `Infinite`, and an `IsStill`
/// stream, read at its size; each placement checks that size against its own
/// sequence (`video::read_occurrence`). Any other media under the flag is a
/// shape this reader has not seen and fails closed.
pub(super) fn read_adjustment_media(graph: &Graph<'_>, media: Located<Media>) -> Result<PrMedia> {
    let identity = &media.identity;
    let value = &media.value;
    ensure!(
        value.implementation_id.as_deref() == Some(GENERATOR_IMPLEMENTATION_ID)
            && value.file_path.as_deref() == Some(BLACK_VIDEO_FILE_PATH)
            && value
                .actual_media_file_path
                .as_deref()
                .is_none_or(|path| path == BLACK_VIDEO_FILE_PATH)
            && value.relative_paths.is_empty()
            && value.audio_stream.is_none(),
        "{identity}: an AdjustmentLayer clip must play Black Video generator media"
    );
    ensure!(
        value.infinite.as_deref() == Some("true"),
        "{identity}: adjustment layer media must be Infinite"
    );
    let stream_reference = required(
        value.video_stream.as_ref(),
        identity,
        records::VIDEO_STREAM.tag,
    )?;
    let stream = graph.follow::<VideoStream>(stream_reference, identity)?;
    ensure!(
        stream.value.is_still.as_deref() == Some("true"),
        "{}: adjustment layer media must be an IsStill stream",
        stream.identity
    );
    // Older generators carry FrameRate; 26.5 may carry only this measured
    // override pair. Explicit FrameRate keeps precedence: Premiere also saves
    // a 29.97 original alongside a 30 fps override in the older fixture.
    let frame_rate_ticks = if let Some(rate) = stream.value.frame_rate.as_deref() {
        integer(rate, &format!("{}: invalid FrameRate", stream.identity))?
    } else {
        ensure!(
            flag(
                stream.value.is_frame_rate_overridden.as_deref(),
                &stream.identity,
                "IsFrameRateOverridden",
            )?,
            "{}: missing FrameRate requires IsFrameRateOverridden=true",
            stream.identity
        );
        let rate = required_integer(
            stream.value.overidden_frame_rate.as_deref(),
            &stream.identity,
            "OveriddenFrameRate",
        )?;
        ensure!(
            rate == 8_467_200_000,
            "{}: adjustment layer override must be the measured 30 fps rate (8467200000 ticks per frame)",
            stream.identity
        );
        rate
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
