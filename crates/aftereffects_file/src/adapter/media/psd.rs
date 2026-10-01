//! PSD v1 RGB8 decoder. This extracts stored pixels; it does not render
//! Photoshop's masks, adjustment layers, effects, or layer compositing.

use image::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Selection {
    Merged,
    Layer { id: u32, index: u32 },
}

#[derive(Debug, thiserror::Error)]
pub(super) enum DecodeError {
    #[error("malformed PSD: {0}")]
    Malformed(&'static str),
    #[error("unsupported PSD: {0}")]
    Unsupported(&'static str),
    #[error("PSD exceeds safe bounds: {0}")]
    Bounds(&'static str),
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(DecodeError::Bounds("section length overflow"))?;
        let value = self
            .bytes
            .get(self.pos..end)
            .ok_or(DecodeError::Malformed("truncated section"))?;
        self.pos = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| DecodeError::Malformed("u16"))?,
        ))
    }

    fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(i16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| DecodeError::Malformed("i16"))?,
        ))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| DecodeError::Malformed("u32"))?,
        ))
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| DecodeError::Malformed("i32"))?,
        ))
    }

    fn section(&mut self) -> Result<Cursor<'a>, DecodeError> {
        let len =
            usize::try_from(self.u32()?).map_err(|_| DecodeError::Bounds("section length"))?;
        Ok(Cursor::new(self.take(len)?))
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }
}

#[derive(Clone, Copy)]
struct Rect {
    top: u32,
    left: u32,
    width: u32,
    height: u32,
}

struct Channel<'a> {
    id: i16,
    length: usize,
    bytes: &'a [u8],
}

struct Layer<'a> {
    id: Option<u32>,
    bounds: &'a [u8],
    channels: Vec<Channel<'a>>,
    supported: bool,
}

fn dimensions(width: u32, height: u32) -> Result<usize, DecodeError> {
    if width == 0 || height == 0 {
        return Err(DecodeError::Malformed("empty image bounds"));
    }
    if width > 30_000 || height > 30_000 {
        return Err(DecodeError::Bounds("PSD v1 dimension limit (30,000)"));
    }
    let pixels = u64::from(width) * u64::from(height);
    usize::try_from(pixels).map_err(|_| DecodeError::Bounds("pixel count"))
}

fn rect(cursor: &mut Cursor<'_>, canvas: [u32; 2]) -> Result<Rect, DecodeError> {
    let top = cursor.i32()?;
    let left = cursor.i32()?;
    let bottom = cursor.i32()?;
    let right = cursor.i32()?;
    if top < 0
        || left < 0
        || bottom <= top
        || right <= left
        || i64::from(bottom) > i64::from(canvas[1])
        || i64::from(right) > i64::from(canvas[0])
    {
        return Err(DecodeError::Unsupported(
            "layer rectangle outside canvas or empty",
        ));
    }
    let width = u32::try_from(right - left).map_err(|_| DecodeError::Bounds("layer width"))?;
    let height = u32::try_from(bottom - top).map_err(|_| DecodeError::Bounds("layer height"))?;
    dimensions(width, height)?;
    Ok(Rect {
        top: u32::try_from(top).map_err(|_| DecodeError::Bounds("layer top"))?,
        left: u32::try_from(left).map_err(|_| DecodeError::Bounds("layer left"))?,
        width,
        height,
    })
}

fn read_layers<'a>(section: &mut Cursor<'a>) -> Result<Vec<Layer<'a>>, DecodeError> {
    if section.remaining() == 0 {
        return Ok(Vec::new());
    }
    let mut info = section.section()?;
    if info.remaining() == 0 {
        return Ok(Vec::new());
    }
    let count = info.i16()?;
    let count = usize::from(count.unsigned_abs());
    let mut layers = Vec::with_capacity(count);
    for _ in 0..count {
        // Empty groups and off-canvas siblings must not veto an independently
        // selected raster layer. Validate geometry only after selecting it.
        let bounds = info.take(16)?;
        let channel_count = usize::from(info.u16()?);
        let mut channels = Vec::with_capacity(channel_count);
        for _ in 0..channel_count {
            let id = info.i16()?;
            let length =
                usize::try_from(info.u32()?).map_err(|_| DecodeError::Bounds("channel length"))?;
            if length < 2 {
                return Err(DecodeError::Malformed("channel has no compression header"));
            }
            channels.push(Channel {
                id,
                length,
                bytes: &[],
            });
        }
        let signature = info.take(4)?;
        let blend = info.take(4)?;
        let opacity = info.u8()?;
        let clipping = info.u8()?;
        let flags = info.u8()?;
        info.u8()?; // filler
        let mut extra = info.section()?;
        let mask = extra.section()?;
        let blend_ranges = extra.section()?;
        let name_len = usize::from(extra.u8()?);
        let name_padded = (name_len + 1)
            .checked_add(3)
            .ok_or(DecodeError::Bounds("name length"))?
            & !3;
        extra.take(name_padded - 1)?;
        let mut id = None;
        let mut known_tags = true;
        while extra.remaining() != 0 {
            if extra.take(4)? != b"8BIM" {
                return Err(DecodeError::Unsupported("layer additional-info signature"));
            }
            let key = extra.take(4)?;
            let size =
                usize::try_from(extra.u32()?).map_err(|_| DecodeError::Bounds("tag length"))?;
            let value = extra.take(size)?;
            if size % 2 != 0 {
                extra.take(1)?;
            }
            match key {
                b"lyid" => {
                    if size != 4 || id.is_some() {
                        return Err(DecodeError::Malformed("duplicate or invalid layer ID"));
                    }
                    id = Some(u32::from_be_bytes(
                        value
                            .try_into()
                            .map_err(|_| DecodeError::Malformed("layer ID"))?,
                    ));
                }
                // Display/protection metadata does not change stored raster channels.
                b"luni" | b"lnsr" | b"lclr" | b"lspf" => {}
                _ => known_tags = false,
            }
        }
        layers.push(Layer {
            id,
            bounds,
            channels,
            supported: signature == b"8BIM"
                && blend == b"norm"
                && opacity == 255
                && clipping == 0
                && flags & !0x08 == 0
                && mask.remaining() == 0
                && blend_ranges
                    .bytes
                    .chunks(8)
                    .all(|range| range == [0, 0, 255, 255, 0, 0, 255, 255])
                && known_tags,
        });
    }
    for layer in &mut layers {
        for channel in &mut layer.channels {
            channel.bytes = info.take(channel.length)?;
        }
    }
    // PSD layer-info may have one alignment byte; any other unexplained data is invalid.
    if info.remaining() > 1 {
        return Err(DecodeError::Malformed("trailing layer channel data"));
    }
    Ok(layers)
}

fn read_resources(mut resources: Cursor<'_>) -> Result<bool, DecodeError> {
    let mut real_merged = true;
    let mut seen_version = false;
    while resources.remaining() != 0 {
        if resources.take(4)? != b"8BIM" {
            return Err(DecodeError::Malformed("image resource signature"));
        }
        let id = resources.u16()?;
        let name_len = usize::from(resources.u8()?);
        resources.take(name_len)?;
        if (name_len + 1) % 2 != 0 {
            resources.take(1)?;
        }
        let value = resources.section()?;
        if value.remaining() % 2 != 0 {
            resources.take(1)?;
        }
        if id == 1057 {
            if seen_version {
                return Err(DecodeError::Malformed("duplicate version-info resource"));
            }
            seen_version = true;
            let mut value = value;
            if value.u32()? != 1 {
                return Err(DecodeError::Unsupported("version-info resource version"));
            }
            real_merged = match value.u8()? {
                0 => false,
                1 => true,
                _ => return Err(DecodeError::Malformed("merged-preview flag")),
            };
        }
    }
    Ok(real_merged)
}

fn unpack_rows(
    source: &mut Cursor<'_>,
    lengths: &[usize],
    row: usize,
) -> Result<Vec<u8>, DecodeError> {
    let capacity = lengths
        .len()
        .checked_mul(row)
        .ok_or(DecodeError::Bounds("RLE plane"))?;
    let mut output = Vec::with_capacity(capacity);
    for &packed_len in lengths {
        let mut packed = Cursor::new(source.take(packed_len)?);
        let row_end = output
            .len()
            .checked_add(row)
            .ok_or(DecodeError::Bounds("RLE row"))?;
        while packed.remaining() > 0 {
            let control = packed.u8()?;
            match control {
                0..=127 => {
                    let run = usize::from(control) + 1;
                    if output.len().checked_add(run).is_none_or(|n| n > row_end) {
                        return Err(DecodeError::Malformed("RLE literal exceeds row"));
                    }
                    output.extend_from_slice(packed.take(run)?);
                }
                128 => {} // PackBits no-op.
                129..=255 => {
                    let run = 257 - usize::from(control);
                    if output.len().checked_add(run).is_none_or(|n| n > row_end) {
                        return Err(DecodeError::Malformed("RLE repeat exceeds row"));
                    }
                    output.resize(output.len() + run, packed.u8()?);
                }
            }
        }
        if output.len() != row_end {
            return Err(DecodeError::Malformed("RLE row decodes short"));
        }
    }
    Ok(output)
}

fn plane(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, DecodeError> {
    let len = dimensions(width, height)?;
    let row = usize::try_from(width).map_err(|_| DecodeError::Bounds("row width"))?;
    let rows = usize::try_from(height).map_err(|_| DecodeError::Bounds("row count"))?;
    let mut source = Cursor::new(bytes);
    let compression = source.u16()?;
    let mut output = Vec::with_capacity(len);
    match compression {
        0 => {
            if source.remaining() != len {
                return Err(DecodeError::Malformed(
                    "raw channel length differs from dimensions",
                ));
            }
            output.extend_from_slice(source.take(len)?);
        }
        1 => {
            let mut lengths = Vec::with_capacity(rows);
            for _ in 0..rows {
                lengths.push(usize::from(source.u16()?));
            }
            output = unpack_rows(&mut source, &lengths, row)?;
            if source.remaining() != 0 {
                return Err(DecodeError::Malformed("extra RLE channel data"));
            }
        }
        _ => {
            return Err(DecodeError::Unsupported(
                "channel compression (only raw/PackBits)",
            ));
        }
    }
    Ok(output)
}

fn pixels(
    channels: &[Channel<'_>],
    rect: Rect,
    canvas: [u32; 2],
    expected: [u32; 2],
) -> Result<RgbaImage, DecodeError> {
    let cropped = expected == [rect.width, rect.height];
    if !cropped && expected != canvas {
        return Err(DecodeError::Unsupported(
            "requested dimensions are neither layer crop nor canvas",
        ));
    }
    let output_len = dimensions(expected[0], expected[1])?
        .checked_mul(4)
        .ok_or(DecodeError::Bounds("RGBA allocation"))?;
    let source_len = dimensions(rect.width, rect.height)?;
    let mut result = vec![0u8; output_len];
    let mut found = [false; 4];
    for channel in channels {
        let component = match channel.id {
            0..=2 => usize::try_from(channel.id).map_err(|_| DecodeError::Bounds("channel ID"))?,
            -1 => 3,
            _ => return Err(DecodeError::Unsupported("extra/mask layer channel")),
        };
        if found[component] {
            return Err(DecodeError::Malformed("duplicate channel"));
        }
        found[component] = true;
        let values = plane(channel.bytes, rect.width, rect.height)?;
        let width = usize::try_from(rect.width).map_err(|_| DecodeError::Bounds("layer width"))?;
        let stride =
            usize::try_from(expected[0]).map_err(|_| DecodeError::Bounds("canvas width"))?;
        let x = if cropped {
            0
        } else {
            usize::try_from(rect.left).map_err(|_| DecodeError::Bounds("left"))?
        };
        let y = if cropped {
            0
        } else {
            usize::try_from(rect.top).map_err(|_| DecodeError::Bounds("top"))?
        };
        for (i, value) in values.iter().enumerate() {
            let dst = ((y + i / width) * stride + x + i % width) * 4 + component;
            result[dst] = *value;
        }
    }
    if !found[0..3].iter().all(|present| *present) {
        return Err(DecodeError::Unsupported("missing RGB channels"));
    }
    if !found[3] {
        if cropped {
            for pixel in result.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        } else {
            let width = usize::try_from(rect.width).map_err(|_| DecodeError::Bounds("width"))?;
            let stride = usize::try_from(expected[0]).map_err(|_| DecodeError::Bounds("stride"))?;
            let left = usize::try_from(rect.left).map_err(|_| DecodeError::Bounds("left"))?;
            let top = usize::try_from(rect.top).map_err(|_| DecodeError::Bounds("top"))?;
            for i in 0..source_len {
                result[((top + i / width) * stride + left + i % width) * 4 + 3] = 255;
            }
        }
    }
    RgbaImage::from_raw(expected[0], expected[1], result)
        .ok_or(DecodeError::Bounds("RGBA dimensions"))
}

/// Extracts stored composite pixels or one exactly identified editable layer.
/// An expected layer size equal to its rectangle returns the crop; otherwise a
/// canvas-sized request places the original rectangle on transparent canvas.
pub(super) fn decode(
    bytes: &[u8],
    selection: Selection,
    expected: [u32; 2],
) -> Result<RgbaImage, DecodeError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != b"8BPS" || cursor.u16()? != 1 {
        return Err(DecodeError::Unsupported("PSD v1 signature/version"));
    }
    if cursor.take(6)? != [0; 6] {
        return Err(DecodeError::Malformed("reserved header bytes"));
    }
    let channel_count = cursor.u16()?;
    let height = cursor.u32()?;
    let width = cursor.u32()?;
    dimensions(width, height)?;
    let canvas = [width, height];
    if cursor.u16()? != 8 || cursor.u16()? != 3 {
        return Err(DecodeError::Unsupported("only 8-bit RGB PSD is supported"));
    }
    if !(3..=4).contains(&channel_count) {
        return Err(DecodeError::Unsupported("composite channel count"));
    }
    if cursor.section()?.remaining() != 0 {
        return Err(DecodeError::Unsupported("indexed/color-mode data"));
    }
    // The adapter always diagnoses omitted color-profile conversion. Resource
    // lengths are still validated, and a known absent merged preview is never
    // treated as authoritative pixels.
    let real_merged = read_resources(cursor.section()?)?;
    if selection == Selection::Merged && !real_merged {
        return Err(DecodeError::Unsupported("PSD has no real merged preview"));
    }
    let mut layer_mask = cursor.section()?;
    match selection {
        Selection::Merged => {
            // Stored composite pixels are authoritative, even when the layer
            // records require features the editable-layer path cannot decode.
            let merged_alpha = if layer_mask.remaining() == 0 {
                false
            } else {
                let mut info = layer_mask.section()?;
                if info.remaining() == 0 {
                    false
                } else {
                    info.i16()? < 0
                }
            };
            if expected != canvas {
                return Err(DecodeError::Unsupported(
                    "merged dimensions differ from canvas",
                ));
            }
            if merged_alpha && channel_count != 4 {
                return Err(DecodeError::Malformed(
                    "merged transparency channel is absent",
                ));
            }
            if channel_count == 4 && !merged_alpha {
                return Err(DecodeError::Unsupported(
                    "fourth composite channel without merged transparency marker",
                ));
            }
            let compression = cursor.u16()?;
            let rows = usize::try_from(height).map_err(|_| DecodeError::Bounds("rows"))?;
            let mut image = RgbaImage::new(width, height);
            match compression {
                0 => {
                    let n = dimensions(width, height)?;
                    for id in 0..usize::from(channel_count) {
                        for (pixel, value) in image.pixels_mut().zip(cursor.take(n)?.iter()) {
                            pixel[id] = *value;
                        }
                    }
                }
                1 => {
                    // PSD composite RLE has one row-length table across *all* channels.
                    let table_len = rows
                        .checked_mul(usize::from(channel_count))
                        .and_then(|n| n.checked_mul(2))
                        .ok_or(DecodeError::Bounds("RLE table"))?;
                    let table = cursor.take(table_len)?;
                    let mut lengths = Cursor::new(table);
                    let row =
                        usize::try_from(width).map_err(|_| DecodeError::Bounds("row width"))?;
                    for id in 0..usize::from(channel_count) {
                        let mut row_sizes = Vec::with_capacity(rows);
                        for _ in 0..rows {
                            row_sizes.push(usize::from(lengths.u16()?));
                        }
                        let values = unpack_rows(&mut cursor, &row_sizes, row)?;
                        for (pixel, value) in image.pixels_mut().zip(values) {
                            pixel[id] = value;
                        }
                    }
                }
                _ => {
                    return Err(DecodeError::Unsupported(
                        "composite compression (only raw/PackBits)",
                    ));
                }
            }
            if cursor.remaining() != 0 {
                return Err(DecodeError::Malformed("extra composite data"));
            }
            // Per-plane compression headers are absent in the composite payload.
            if channel_count == 3 {
                for pixel in image.pixels_mut() {
                    pixel[3] = 255;
                }
            }
            Ok(image)
        }
        Selection::Layer { id, index } => {
            let layers = read_layers(&mut layer_mask)?;
            if layer_mask.remaining() != 0
                && (layer_mask.section()?.remaining() != 0 || layer_mask.remaining() != 0)
            {
                return Err(DecodeError::Unsupported(
                    "global layer mask or additional info",
                ));
            }
            let mut ids = std::collections::HashSet::new();
            for layer in &layers {
                if let Some(id) = layer.id
                    && !ids.insert(id)
                {
                    return Err(DecodeError::Malformed("duplicate layer IDs"));
                }
            }
            let index = usize::try_from(index).map_err(|_| DecodeError::Bounds("layer index"))?;
            let layer = layers
                .get(index)
                .ok_or(DecodeError::Unsupported("layer index absent"))?;
            if layer.id != Some(id) {
                return Err(DecodeError::Unsupported(
                    "layer ID and record index disagree",
                ));
            }
            if !layer.supported {
                return Err(DecodeError::Unsupported(
                    "layer mask, effect, blend, opacity, visibility, group, or unknown tag",
                ));
            }
            let rect = rect(&mut Cursor::new(layer.bounds), canvas)?;
            pixels(&layer.channels, rect, canvas, expected)
        }
    }
}

#[cfg(test)]
mod tests;
