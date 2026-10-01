//! Color Matte generator media: no file, one colour, infinite duration.

use super::{required, required_integer, stream_dimensions};
use crate::error::{ensure, unsupported, Result};
use crate::format::{graph::Element, Graph, Located, Record};
use crate::schema::{
    color_matte::{COLOR_MATTE_FILE_PATH, COLOR_MATTE_NAME, GENERATOR_IMPLEMENTATION_ID},
    native::{Media, VideoStream},
    records, PrColorMatte, PrMedia, PrMediaKind,
};
use base64::{engine::general_purpose::STANDARD, Engine};

/// Whether an undecoded `Media` record is Color Matte generator media.
///
/// Graphic and Black Video media share the generator `ImplementationID`, so
/// only the `COLR` `FilePath` (and `ActualMediaFilePath`, when present) marks a
/// matte; other generators keep the graphic reader's validation.
pub(super) fn is_color_matte_media(record: Record<'_>) -> bool {
    let text = |tag| record.element().child(tag).and_then(Element::text);
    text("ImplementationID") == Some(GENERATOR_IMPLEMENTATION_ID)
        && text("FilePath") == Some(COLOR_MATTE_FILE_PATH)
        && text("ActualMediaFilePath").is_none_or(|path| path == COLOR_MATTE_FILE_PATH)
}

/// Read a `Media` record that matched [`is_color_matte_media`], at the size of
/// its stream; each placement checks that size against its own sequence
/// (`video::read_occurrence`).
pub(super) fn read_color_matte_media(graph: &Graph<'_>, media: Located<Media>) -> Result<PrMedia> {
    let identity = &media.identity;
    ensure!(
        media.value.audio_stream.is_none() && media.value.relative_paths.is_empty(),
        "{identity}: Color Matte media must not reference files or audio"
    );
    ensure!(
        media.value.infinite.as_deref() == Some("true"),
        "{identity}: Color Matte media must be Infinite"
    );
    let prefs = required(
        media.value.importer_prefs.as_ref(),
        identity,
        "ImporterPrefs",
    )?;
    ensure!(
        prefs.encoding == records::ENCODING,
        "{identity}: Color Matte ImporterPrefs must be base64"
    );
    let bytes = STANDARD
        .decode(prefs.value.trim())
        .map_err(|error| unsupported(format!("{identity}: invalid ImporterPrefs: {error}")))?;
    let matte = PrColorMatte::from_importer_prefs(&bytes)
        .map_err(|error| unsupported(format!("{identity}: {error}")))?;

    let stream_reference = required(
        media.value.video_stream.as_ref(),
        identity,
        records::VIDEO_STREAM.tag,
    )?;
    let stream = graph.follow::<VideoStream>(stream_reference, identity)?;
    let frame_rate_ticks = required_integer(
        stream.value.frame_rate.as_deref(),
        &stream.identity,
        "FrameRate",
    )?;
    let frame_rate = super::frame_rate(frame_rate_ticks, &stream.identity)?;
    let [width, height] = stream_dimensions(&stream)?;
    let intrinsic_ticks = required_integer(
        stream.value.duration.as_deref(),
        &stream.identity,
        "Duration",
    )?;
    let name = media
        .value
        .title
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| COLOR_MATTE_NAME.to_owned());
    Ok(PrMedia {
        name,
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: Some(crate::schema::PrVideoStream {
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks,
            frame_rate: frame_rate.into(),
            width,
            height,
            kind: PrMediaKind::ColorMatte(matte),
        }),
        audio: None,
    })
}
