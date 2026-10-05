//! Solid generator media: Color Matte and ordinary Black Video, without files.
//!
//! Black Video uses the existing black Color Matte binding; adjustment-layer
//! placements of the same `BLAK` generator retain their separate reader.

use super::{required, required_integer, stream_dimensions};
use crate::error::{ensure, unsupported, Result};
use crate::format::{graph::Element, Graph, Located, Record};
use crate::schema::{
    adjustment::{BLACK_VIDEO_FILE_PATH, BLACK_VIDEO_TITLE},
    color_matte::{COLOR_MATTE_FILE_PATH, COLOR_MATTE_NAME, GENERATOR_IMPLEMENTATION_ID},
    native::{Media, VideoStream},
    records, PrColorMatte, PrMedia, PrMediaKind,
};
use base64::{engine::general_purpose::STANDARD, Engine};

/// Whether an undecoded `Media` record is Color Matte generator media.
///
/// Only the `COLR` path marks a Color Matte; generators share an importer ID.
pub(super) fn is_color_matte_media(record: Record<'_>) -> bool {
    is_generator_media(record, COLOR_MATTE_FILE_PATH)
}

/// `BLAK` identifies Black Video media, including adjustment-layer sources.
/// The caller checks the adjustment flag first; neither name nor importer ID
/// alone distinguishes an ordinary black solid.
pub(super) fn is_black_video_media(record: Record<'_>) -> bool {
    is_generator_media(record, BLACK_VIDEO_FILE_PATH)
}

fn is_generator_media(record: Record<'_>, file_path: &str) -> bool {
    let text = |tag| record.element().child(tag).and_then(Element::text);
    text("ImplementationID") == Some(GENERATOR_IMPLEMENTATION_ID)
        && text("FilePath") == Some(file_path)
        && text("ActualMediaFilePath").is_none_or(|path| path == file_path)
}

/// Read unflagged Black Video as the existing editable black solid binding.
/// Its native stream carries the source size/rate/duration; the occurrence
/// checks canvas compatibility and keeps its own source and timeline ranges.
pub(super) fn read_black_video_media(graph: &Graph<'_>, media: Located<Media>) -> Result<PrMedia> {
    let identity = &media.identity;
    ensure!(
        media.value.audio_stream.is_none() && media.value.relative_paths.is_empty(),
        "{identity}: Black Video media must not reference files or audio"
    );
    ensure!(
        media.value.infinite.as_deref() == Some("true"),
        "{identity}: Black Video media must be Infinite"
    );
    ensure!(
        media.value.importer_prefs.is_none(),
        "{identity}: Black Video media must not carry ImporterPrefs"
    );
    let reference = required(
        media.value.video_stream.as_ref(),
        identity,
        records::VIDEO_STREAM.tag,
    )?;
    let stream = graph.follow::<VideoStream>(reference, identity)?;
    ensure!(
        stream.value.is_still.as_deref() == Some("true"),
        "{}: Black Video media must be an IsStill stream",
        stream.identity
    );
    solid_media(
        media,
        stream,
        PrColorMatte { rgb: [0; 3] },
        BLACK_VIDEO_TITLE,
    )
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
    // Resolve inline definitions too: one hash must not name conflicting values.
    let stored = graph.binary_value(&prefs.binary_hash, identity)?;
    let value = if prefs.value.trim().is_empty() {
        required(stored, identity, "ImporterPrefs binary value")?
    } else {
        &prefs.value
    };
    let encoded: String = value
        .chars()
        .filter(|value| !value.is_ascii_whitespace())
        .collect();
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| unsupported(format!("{identity}: invalid ImporterPrefs: {error}")))?;
    let matte = PrColorMatte::from_importer_prefs(&bytes)
        .map_err(|error| unsupported(format!("{identity}: {error}")))?;

    let stream_reference = required(
        media.value.video_stream.as_ref(),
        identity,
        records::VIDEO_STREAM.tag,
    )?;
    let stream = graph.follow::<VideoStream>(stream_reference, identity)?;
    solid_media(media, stream, matte, COLOR_MATTE_NAME)
}

fn solid_media(
    media: Located<Media>,
    stream: Located<VideoStream>,
    matte: PrColorMatte,
    default_name: &str,
) -> Result<PrMedia> {
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
        .unwrap_or_else(|| default_name.to_owned());
    Ok(PrMedia {
        name,
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: Some(crate::schema::PrVideoStream {
            pixel_aspect: Default::default(),
            interpretation: Default::default(),
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
