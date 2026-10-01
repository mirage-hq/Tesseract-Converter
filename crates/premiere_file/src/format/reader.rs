//! Gzip/XML decoding into the shared Premiere schema.
//!
//! `load` selects timelines and assembles a `PrProjectFile`; `sequence`,
//! `video`, and `audio` decode one selected sequence from the record graph.

use super::{FormatError, Graph, Located, Result as FormatResult};
use crate::error::{ensure, unsupported, BuildError, Result};
use crate::schema::{
    native::{ClipTrackItem, VideoStream},
    records, FrameRate, MediaId, PrMedia, PrSequence,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

mod adjustment;
mod after_effects;
mod animation;
mod audio;
mod caption;
mod color_matte;
mod dissolve;
mod effects;
mod film_impact;
mod graphic;
mod load;
mod mask;
mod native_media;
mod nested;
mod pop;
mod sequence;
mod still;
mod stroke;
pub(super) mod time_remap;
mod timeline_end;
mod video;
mod visibility;

pub(crate) fn read_xml(path: &Path) -> FormatResult<String> {
    let input = File::open(path)?;
    let mut decoder = flate2::read::MultiGzDecoder::new(input);
    let mut data = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = decoder.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        // Geometric growth avoids repeatedly copying the entire expanded XML.
        // Reserve only after decoding real bytes; never trust gzip size hints.
        data.try_reserve(count)
            .map_err(|source| FormatError::Allocation {
                context: "decompressed XML",
                source,
            })?;
        data.extend_from_slice(&buffer[..count]);
    }
    Ok(String::from_utf8(data)?)
}

/// Reads one selected sequence. `cyclic` holds the timelines on a nesting
/// cycle (`graph::cyclic_sequences`); every placement of one is omitted.
pub(super) fn read_sequence(
    graph: &Graph<'_>,
    sequence_id: Option<&str>,
    cyclic: &BTreeSet<String>,
    media: &mut BTreeMap<MediaId, PrMedia>,
    omissions: &mut Vec<crate::Omission>,
) -> Result<PrSequence> {
    sequence::read_sequence(graph, sequence_id, cyclic, media, omissions)
}

fn required<T>(value: Option<T>, context: &str, field: &str) -> Result<T> {
    value.ok_or_else(|| unsupported(format!("{context}: missing {field}")))
}

fn non_empty(value: Option<String>, context: &str, field: &str) -> Result<String> {
    let value = required(value, context, field)?;
    if value.is_empty() {
        return Err(unsupported(format!("{context}: empty {field}")));
    }
    Ok(value)
}

fn integer(value: &str, context: &str) -> Result<i64> {
    value
        .parse()
        .map_err(BuildError::from)
        .map_err(|source| BuildError::Context {
            context: context.to_owned(),
            source: Box::new(source),
        })
}

/// The pixel size of a sequence, occurrence or stream `FrameRect`
/// (`0,0,width,height`). Only a rectangle at the origin with a positive width
/// and height is read; any other rectangle is unsupported rather than
/// reinterpreted.
fn frame_dimensions(value: &str, identity: &str) -> Result<[u32; 2]> {
    let context = format!("{identity}: invalid FrameRect");
    let mut fields = value.split(',');
    let parse =
        |field: Option<&str>| integer(field.ok_or_else(|| unsupported(context.clone()))?, &context);
    let (left, top, width, height) = (
        parse(fields.next())?,
        parse(fields.next())?,
        parse(fields.next())?,
        parse(fields.next())?,
    );
    ensure!(
        left == 0 && top == 0 && fields.next().is_none(),
        "{context}"
    );
    let width = u32::try_from(width).map_err(|_| unsupported(context.clone()))?;
    let height = u32::try_from(height).map_err(|_| unsupported(context.clone()))?;
    ensure!(width > 0 && height > 0, "{context}");
    Ok([width, height])
}

/// The pixel size of a media or generator `VideoStream`, whose declared
/// pixels must be square: a placement draws the stream at this size.
fn stream_dimensions(stream: &Located<VideoStream>) -> Result<[u32; 2]> {
    let frame = required(
        stream.value.frame_rect.as_deref(),
        &stream.identity,
        "FrameRect",
    )?;
    let dimensions = frame_dimensions(frame, &stream.identity)?;
    if let Some(pixel_aspect_ratio) = &stream.value.pixel_aspect_ratio {
        let ratio = records::PixelAspectRatio::parse(pixel_aspect_ratio, &stream.identity)?;
        ensure!(
            ratio.is_square(),
            "{}: non-square source pixels ({ratio}) are unsupported",
            stream.identity
        );
    }
    Ok(dimensions)
}

/// The integer text of the required native `field` of the record `context`.
fn required_integer(value: Option<&str>, context: &str, field: &str) -> Result<i64> {
    integer(
        required(value, context, field)?,
        &format!("{context}: invalid {field}"),
    )
}

/// The sequence or stream frame rate that a native `FrameRate` (ticks per
/// frame) names, on the record `context`.
fn frame_rate(ticks_per_frame: i64, context: &str) -> Result<FrameRate> {
    FrameRate::from_ticks_per_frame(ticks_per_frame).ok_or_else(|| {
        unsupported(format!(
            "{context}: unsupported video frame rate ({ticks_per_frame} ticks per frame)"
        ))
    })
}

/// Rejects a placement whose native `OriginalSubClipTimeOffset` is not `0`.
///
/// Only Premiere 9.x/10.x saves write it: 31 corpus placements in
/// `free_quotes` and `5_ink_transitions`, all `0`. What another value encodes
/// is unknown, so reading the placement's own In/Out could select other frames.
fn require_zero_subclip_time_offset(item: &ClipTrackItem, identity: &str) -> Result<()> {
    let Some(value) = item.original_sub_clip_time_offset.as_deref() else {
        return Ok(());
    };
    let ticks = integer(
        value,
        &format!("{identity}: invalid OriginalSubClipTimeOffset"),
    )?;
    ensure!(
        ticks == 0,
        "{identity}: nonzero OriginalSubClipTimeOffset ({ticks} ticks) unsupported; only 0 is verified"
    );
    Ok(())
}
