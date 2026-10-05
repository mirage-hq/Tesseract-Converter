//! Saved Object Mask raster results, not the AI selection/propagation controller.
//! Native v3 `.prmf` indexes and Tracker records use raw FlatBuffers. Pixel
//! payloads are GDeflate followed by a vertical wrapping-byte predictor.
//! This module only recovers source-bound matte pixels; it does not author FX.

use super::text_payload::{Buffer, Table};
use crate::{
    error::{ensure, unsupported, Result},
    numbered_images::MAX_FRAMES,
    schema::SourceFrameRate,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

mod gdeflate;

/// Observed saved Object Mask sequence clock. Only a reachable typed saved
/// mask permits sampling it on the default 30fps export grid; this is not a
/// nearest-rate policy or a general unsupported-sequence-rate exception.
pub(crate) const SAVED_SEQUENCE_FRAME_TICKS: i64 = 8_511_237_907;
const HEADER_BYTES: usize = 32;
const VERSION: u32 = 3;
// Observed discriminants in the v3 native fixtures, not inferred enums. Other
// tags remain unsupported until their wire semantics are independently pinned.
const RASTER_AUXILIARY_TAG: u8 = 8;
const SAVED_REFERENCE_TAG: u8 = 1;
const FRAME_ENCODING_TAG: u32 = 1;
const INDEX_AUXILIARY_TAG: u32 = 1;
const INDEX_AUXILIARY_FLAG: u8 = 1;
/// Bound a decoded source canvas to 64 MiB of one-byte coverage. Only one
/// frame is decoded at a time; no frame-count × canvas allocation is made.
const MAX_CANVAS_PIXELS: usize = 64 * 1024 * 1024;

/// Strong identities from the actual reference tables, never a GUID scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tracker {
    pub(crate) initial: Uuid,
    pub(crate) propagation: Uuid,
    pub(crate) frame_count: usize,
    pub(crate) frame_ticks: u64,
}

fn required<T>(value: Option<T>, field: &str) -> Result<T> {
    value.ok_or_else(|| unsupported(format!("Object Mask is missing {field}")))
}

fn reference(table: Table<'_>) -> Result<Uuid> {
    table.allow_only(&[0], "sidecar reference")?;
    let text = required(table.string(0)?, "sidecar UUID")?;
    let id = Uuid::parse_str(text).map_err(|_| unsupported("invalid Object Mask sidecar UUID"))?;
    ensure!(
        id.hyphenated().to_string() == text,
        "Object Mask sidecar UUID is not canonical"
    );
    Ok(id)
}

impl Tracker {
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        let root = Buffer::raw_root(bytes, "Object Mask Tracker")?;
        root.allow_only(&[0, 1, 2], "root")?;
        let auxiliary = required(root.table(0)?, "Tracker auxiliary table")?;
        auxiliary.allow_only(&[1, 5], "auxiliary")?;
        ensure!(
            auxiliary.u8(1)? == Some(RASTER_AUXILIARY_TAG),
            "unsupported Object Mask Tracker auxiliary type"
        );
        let controller = required(auxiliary.string(5)?, "controller UUID")?;
        ensure!(
            Uuid::parse_str(controller).is_ok_and(|id| id.hyphenated().to_string() == controller),
            "invalid Object Mask controller UUID"
        );
        let initial = required(root.table(2)?, "initial selection")?;
        initial.allow_only(&[0, 1, 2], "initial selection")?;
        ensure!(
            initial.u8(0)? == Some(SAVED_REFERENCE_TAG),
            "unsupported Object Mask initial selection type"
        );
        let initial_id = reference(required(initial.table(1)?, "initial sidecar reference")?)?;
        let records = root.tables_bounded(1, 1)?;
        let [propagation] = records.as_slice() else {
            return Err(unsupported(
                "Object Mask requires exactly one saved propagation",
            ));
        };
        // Slot 3 is absent in the native fixture. Its clock semantics are not
        // inferred from the neighbouring cadence; a present value is rejected.
        propagation.allow_only(&[0, 1, 2, 4], "propagation")?;
        ensure!(
            propagation.u8(0)? == Some(SAVED_REFERENCE_TAG),
            "unsupported Object Mask propagation type"
        );
        let frame_ticks = required(propagation.u64(2)?, "propagation cadence")?;
        let frame_count = required(propagation.u32(4)?, "propagation frame count")? as usize;
        ensure!(
            frame_ticks > 0 && initial.u64(2)? == Some(frame_ticks),
            "Object Mask Tracker cadences disagree"
        );
        ensure!(
            frame_count > 0 && frame_count <= MAX_FRAMES as usize,
            "Object Mask propagation frame count exceeds bounds"
        );
        Ok(Self {
            initial: initial_id,
            propagation: reference(required(
                propagation.table(1)?,
                "propagation sidecar reference",
            )?)?,
            frame_count,
            frame_ticks,
        })
    }

    /// The caller supplies the source project's resolved sidecar directory.
    /// No directory listing, duplicate-file fallback, or modification time selects it.
    pub(crate) fn propagation_path(&self, directory: &Path) -> PathBuf {
        directory.join(format!("{}.prmf", self.propagation.hyphenated()))
    }

    pub(crate) fn validate_source(&self, rate: SourceFrameRate, duration_ticks: i64) -> Result<()> {
        ensure!(
            u64::try_from(rate.ticks_per_frame()).ok() == Some(self.frame_ticks),
            "Object Mask Tracker cadence differs from the source frame duration"
        );
        ensure!(
            self.frame_ticks.checked_mul(self.frame_count as u64)
                == u64::try_from(duration_ticks).ok(),
            "Object Mask Tracker coverage differs from the source duration"
        );
        Ok(())
    }

    pub(crate) fn propagation<'a>(&self, bytes: &'a [u8], canvas: [u32; 2]) -> Result<Raster<'a>> {
        Raster::decode_index(bytes, canvas, self.frame_ticks, self.frame_count)
    }
}

#[derive(Debug)]
pub(crate) struct Raster<'a> {
    /// Source-time order, independent of the saved index's vector order.
    pub(crate) frames: Vec<Frame<'a>>,
}

#[derive(Debug)]
pub(crate) struct Frame<'a> {
    pub(crate) timestamp_ticks: u64,
    pub(crate) crop: [u32; 4],
    pub(crate) canvas: [u32; 2],
    payload: &'a [u8],
}

impl<'a> Raster<'a> {
    pub(crate) fn decode_index(
        bytes: &'a [u8],
        canvas: [u32; 2],
        frame_ticks: u64,
        frame_count: usize,
    ) -> Result<Self> {
        ensure!(
            frame_ticks > 0 && frame_count > 0 && frame_count <= MAX_FRAMES as usize,
            "Object Mask requested frame count/cadence exceeds bounds"
        );
        canvas_pixels(canvas)?;
        let header = bytes
            .get(..HEADER_BYTES)
            .ok_or_else(|| unsupported("truncated Object Mask prmf header"))?;
        ensure!(
            &header[..4] == b"prmf"
                && u32::from_le_bytes(header[4..8].try_into().expect("four version bytes"))
                    == VERSION,
            "unsupported Object Mask prmf magic/version"
        );
        let word = |start: usize| {
            u64::from_le_bytes(
                header[start..start + 8]
                    .try_into()
                    .expect("bounded header word"),
            )
        };
        let index_start = usize::try_from(word(8))
            .map_err(|_| unsupported("Object Mask index offset overflows"))?;
        let index_length = usize::try_from(word(16))
            .map_err(|_| unsupported("Object Mask index length overflows"))?;
        ensure!(
            word(24) == HEADER_BYTES as u64
                && index_start >= HEADER_BYTES
                && index_start.checked_add(index_length) == Some(bytes.len()),
            "Object Mask index does not match file boundaries"
        );
        let root = Buffer::raw_root(&bytes[index_start..], "Object Mask prmf index")?;
        root.allow_only(&[0, 1, 2], "root")?;
        ensure!(
            root.u32(0)? == Some(VERSION),
            "unsupported Object Mask index version"
        );
        let auxiliary = required(root.table(1)?, "index auxiliary table")?;
        auxiliary.allow_only(&[0, 1, 2], "auxiliary")?;
        ensure!(
            auxiliary.u32(0)? == Some(INDEX_AUXILIARY_TAG)
                && auxiliary.u8(1)? == Some(RASTER_AUXILIARY_TAG)
                && auxiliary.u8(2)? == Some(INDEX_AUXILIARY_FLAG),
            "unsupported Object Mask raster encoding"
        );
        let tables = root.tables_bounded(2, frame_count)?;
        ensure!(
            tables.len() == frame_count,
            "Object Mask index count differs from Tracker"
        );
        let mut frames = Vec::with_capacity(frame_count);
        let mut ranges = Vec::with_capacity(frame_count);
        for table in tables {
            table.allow_only(&[0, 1, 2, 3, 4, 6], "frame")?;
            ensure!(
                table.u32(0)? == Some(FRAME_ENCODING_TAG),
                "unsupported Object Mask frame encoding"
            );
            let crop: [u32; 4] = required(table.u32_array(3)?, "frame crop")?;
            ensure!(
                table.u32_array(4)? == Some(canvas),
                "Object Mask canvas differs from source FrameRect"
            );
            validate_crop(crop, canvas)?;
            let offset = usize::try_from(required(table.u64(2)?, "frame payload offset")?)
                .map_err(|_| unsupported("Object Mask payload offset overflows"))?;
            let length = required(table.u32(6)?, "frame payload length")? as usize;
            let end = offset
                .checked_add(length)
                .filter(|end| *end <= index_start)
                .ok_or_else(|| {
                    unsupported("Object Mask frame payload overlaps index or exceeds file")
                })?;
            ensure!(
                offset >= HEADER_BYTES && length > 0,
                "Object Mask payload overlaps header or is empty"
            );
            ranges.push(offset..end);
            frames.push(Frame {
                timestamp_ticks: table.u64(1)?.unwrap_or(0),
                crop,
                canvas,
                payload: &bytes[offset..end],
            });
        }
        ranges.sort_by_key(|range| range.start);
        ensure!(
            ranges.windows(2).all(|pair| pair[0].end <= pair[1].start),
            "Object Mask frame payloads overlap"
        );
        frames.sort_by_key(|frame| frame.timestamp_ticks);
        ensure!(
            frames
                .iter()
                .enumerate()
                .all(|(index, frame)| frame_ticks.checked_mul(index as u64)
                    == Some(frame.timestamp_ticks)),
            "Object Mask index timestamps differ from Tracker cadence"
        );
        Ok(Self { frames })
    }
}

/// Fallible, initialized storage follows the reader's allocation policy, not
/// a content quota. Peak decoding storage is bounded per admitted source frame.
pub(crate) fn zeroed(length: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|source| super::FormatError::Allocation {
            context: "Object Mask raster",
            source,
        })?;
    bytes.resize(length, 0);
    Ok(bytes)
}

fn validate_crop([x, y, width, height]: [u32; 4], canvas: [u32; 2]) -> Result<()> {
    ensure!(
        width > 0
            && height > 0
            && x.checked_add(width).is_some_and(|end| end <= canvas[0])
            && y.checked_add(height).is_some_and(|end| end <= canvas[1]),
        "Object Mask crop is outside its canvas"
    );
    Ok(())
}

fn canvas_pixels([width, height]: [u32; 2]) -> Result<usize> {
    let size = u64::from(width) * u64::from(height);
    ensure!(
        width > 0 && height > 0 && size <= MAX_CANVAS_PIXELS as u64,
        "Object Mask canvas exceeds {MAX_CANVAS_PIXELS} coverage pixels"
    );
    Ok(size as usize)
}

impl Frame<'_> {
    /// Full-canvas, one-byte coverage: 255 selects, 0 excludes. Reconstruct in
    /// crop coordinates, continuously across compression tiles, before placement.
    pub(crate) fn coverage(&self) -> Result<Vec<u8>> {
        let canvas_size = canvas_pixels(self.canvas)?;
        validate_crop(self.crop, self.canvas)?;
        let [x, y, width, height] = self.crop.map(|value| value as usize);
        let mut crop = zeroed(width * height)?;
        gdeflate::decode(self.payload, &mut crop)?;
        for index in width..crop.len() {
            crop[index] = crop[index].wrapping_add(crop[index - width]);
        }
        let mut canvas = zeroed(canvas_size)?;
        let stride = self.canvas[0] as usize;
        for (row, pixels) in crop.chunks_exact(width).enumerate() {
            let start = (y + row) * stride + x;
            canvas[start..start + width].copy_from_slice(pixels);
        }
        Ok(canvas)
    }
}

#[cfg(test)]
mod tests;
