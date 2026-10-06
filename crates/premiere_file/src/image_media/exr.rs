//! OpenEXR source inspection for Premiere's native `oEXR` importer.
//!
//! Keep the original compressed chunks. Inspection reads every header and
//! compressed chunk to reject truncated structure. It deliberately follows
//! OpenEXR's recoverable sequential-chunk path instead of rejecting a damaged
//! offset table that Premiere can reconstruct. It does not decode pixels or
//! narrow Premiere's broader EXR support to a converter-owned channel,
//! compression, part, or sample profile.

use super::{ImageFormat, ValidatedImage};
use crate::error::{ensure, unsupported, BuildError, Result};
use exr::{
    block::reader::Reader,
    error::Error as ExrError,
    meta::{attribute::AttributeValue, header::Header, BlockDescription},
};
use std::io::{BufReader, ErrorKind, Read, Seek, SeekFrom};

pub(super) enum Inspection {
    Native(ValidatedImage),
    PremiereUnsupported {
        image: ValidatedImage,
        reason: &'static str,
    },
}

pub(super) fn inspect(
    mut source: impl Read + Seek,
    expected_size: Option<u64>,
) -> Result<Inspection> {
    let actual_size = source.seek(SeekFrom::End(0))?;
    if let Some(expected_size) = expected_size {
        ensure!(
            actual_size == expected_size,
            "OpenEXR source size changed while it was inspected"
        );
    }
    source.rewind()?;

    let reader = Reader::read_from_buffered(BufReader::new(source), false).map_err(invalid_exr)?;
    let first_header = reader
        .headers()
        .first()
        .ok_or_else(|| unsupported("OpenEXR source has no image headers"))?;
    let premiere_headers = reader
        .headers()
        .iter()
        .enumerate()
        .filter(|(_, header)| premiere_compatible(header))
        .collect::<Vec<_>>();
    let supported = !premiere_headers.is_empty();
    let all_headers;
    let facts_headers = if supported {
        &premiere_headers
    } else {
        all_headers = reader.headers().iter().enumerate().collect::<Vec<_>>();
        &all_headers
    };
    let image = image_facts(facts_headers, first_header, reader.headers().len())?;
    let mut chunks = reader.all_chunks(false).map_err(invalid_exr)?;
    for chunk in &mut chunks {
        chunk.map_err(invalid_exr)?;
    }
    Ok(if supported {
        Inspection::Native(image)
    } else {
        Inspection::PremiereUnsupported {
            image,
            reason: "OpenEXR contains only deep-tiled parts, which Premiere's bundled importer does not expose; the original source is offered to the editable After Effects fallback",
        }
    })
}

fn premiere_compatible(header: &Header) -> bool {
    // The bundled HybridInputFile skips deep-tiled parts but keeps flat tiles,
    // scanline parts, and deep scanline parts. A mixed file remains native by
    // forming its canvas and channel defaults from the retained parts.
    !(header.deep && matches!(header.blocks, BlockDescription::Tiles(_)))
}

fn image_facts(
    headers: &[(usize, &Header)],
    pixel_aspect_header: &Header,
    part_count: usize,
) -> Result<ValidatedImage> {
    ensure!(!headers.is_empty(), "OpenEXR source has no image headers");

    // Premiere's bundled OpenEXR importer forms one canvas from the parts'
    // display windows. Preserve that behavior without imposing a part count or
    // requiring dataWindow and displayWindow to match.
    let mut left = i64::MAX;
    let mut top = i64::MAX;
    let mut right = i64::MIN;
    let mut bottom = i64::MIN;
    for (_, header) in headers {
        let window = header.shared_attributes.display_window;
        let x = i64::from(window.position.x());
        let y = i64::from(window.position.y());
        let width = i64::try_from(window.size.width())
            .map_err(|_| unsupported("OpenEXR display width exceeds Premiere's range"))?;
        let height = i64::try_from(window.size.height())
            .map_err(|_| unsupported("OpenEXR display height exceeds Premiere's range"))?;
        ensure!(width > 0 && height > 0, "OpenEXR display window is empty");
        left = left.min(x);
        top = top.min(y);
        right = right.max(
            x.checked_add(width)
                .ok_or_else(|| unsupported("OpenEXR display window exceeds Premiere's range"))?,
        );
        bottom = bottom.max(
            y.checked_add(height)
                .ok_or_else(|| unsupported("OpenEXR display window exceeds Premiere's range"))?,
        );
    }
    let width = u32::try_from(right - left)
        .map_err(|_| unsupported("OpenEXR display width exceeds Premiere's range"))?;
    let height = u32::try_from(bottom - top)
        .map_err(|_| unsupported("OpenEXR display height exceeds Premiere's range"))?;

    let has_channel = |name: &[u8]| {
        headers.iter().any(|(index, header)| {
            // HybridInputFile prefixes channels in named parts after part zero.
            // Its default preferences search only exact R/G/B/A/Y/RY/BY names.
            let prefixed =
                part_count > 1 && *index > 0 && header.own_attributes.layer_name.is_some();
            !prefixed
                && header
                    .channels
                    .list
                    .iter()
                    .any(|channel| channel.name.as_slice() == name)
        })
    };
    let alpha = has_channel(b"A");
    let open_exr_channels = if has_channel(b"Y") && !has_channel(b"R") {
        if has_channel(b"RY") && has_channel(b"BY") {
            crate::schema::OpenExrChannels::LumaChroma
        } else {
            crate::schema::OpenExrChannels::Luma
        }
    } else {
        crate::schema::OpenExrChannels::Rgb {
            red: has_channel(b"R"),
            green: has_channel(b"G"),
            blue: has_channel(b"B"),
        }
    };
    Ok(ValidatedImage {
        format: ImageFormat::OpenExr,
        width,
        height,
        // SDKGetInfo8 reads the first file header even when HybridInputFile
        // skips that part's deep-tiled channels and display window.
        pixel_aspect: pixel_aspect(pixel_aspect_header)?,
        open_exr_channels: Some(open_exr_channels),
        alpha,
        // OpenEXR carries named colour/chromaticity attributes rather than an
        // embedded ICC profile in the PNG/JPEG sense.
        icc_profile: false,
    })
}

fn pixel_aspect(header: &Header) -> Result<crate::schema::records::PixelAspectRatio> {
    // Premiere's bundled importer gives this fnord extension precedence over
    // the standard floating-point attribute when it has usable positive terms.
    // An unusable extension falls back to the standard value instead of
    // discarding an otherwise valid source.
    if let Some(AttributeValue::Rational((numerator, denominator))) = header
        .own_attributes
        .other
        .iter()
        .find_map(|(name, value)| (name.as_slice() == b"pixelAspectRatioRational").then_some(value))
    {
        if *numerator > 0 && *denominator > 0 {
            return crate::schema::records::PixelAspectRatio::new(
                *numerator as u64,
                u64::from(*denominator),
            );
        }
    }
    pixel_aspect_float(header.shared_attributes.pixel_aspect)
}

fn pixel_aspect_float(value: f32) -> Result<crate::schema::records::PixelAspectRatio> {
    ensure!(
        value.is_finite() && value > 0.0,
        "OpenEXR pixelAspectRatio must be positive and finite"
    );
    if value == 1.0 {
        return Ok(crate::schema::records::PixelAspectRatio::SQUARE);
    }

    // Premiere's importer exposes 32-bit numerator/denominator fields. Use a
    // continued fraction to keep the closest representable ratio instead of
    // rejecting an otherwise usable source.
    let limit = u128::from(u32::MAX);
    let mut rest = f64::from(value);
    let (mut previous_numerator, mut numerator) = (0_u128, 1_u128);
    let (mut previous_denominator, mut denominator) = (1_u128, 0_u128);
    loop {
        let whole = rest.floor() as u128;
        let Some(next_numerator) = whole
            .checked_mul(numerator)
            .and_then(|value| value.checked_add(previous_numerator))
        else {
            break;
        };
        let Some(next_denominator) = whole
            .checked_mul(denominator)
            .and_then(|value| value.checked_add(previous_denominator))
        else {
            break;
        };
        if next_numerator > limit || next_denominator > limit {
            break;
        }
        (previous_numerator, numerator) = (numerator, next_numerator);
        (previous_denominator, denominator) = (denominator, next_denominator);
        let fraction = rest - rest.floor();
        if fraction <= f64::EPSILON {
            break;
        }
        rest = fraction.recip();
    }
    if numerator == 0 {
        numerator = 1;
        denominator = limit;
    } else if denominator == 0 {
        numerator = limit;
        denominator = 1;
    }
    crate::schema::records::PixelAspectRatio::new(numerator as u64, denominator as u64)
}

fn invalid_exr(error: ExrError) -> BuildError {
    match error {
        ExrError::Io(error) if error.kind() != ErrorKind::UnexpectedEof => error.into(),
        error => unsupported(format!("OpenEXR structure cannot be read: {error}")),
    }
}

#[cfg(test)]
mod tests;
