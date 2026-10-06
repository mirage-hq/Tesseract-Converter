//! Admit the native PNG envelope proved by the public RGB8/RGBA8 control.
//! Pixels and color metadata stay in the original file; this is not a transfer
//! conversion or a claim of general ICC/alpha equivalence between renderers.

use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use crate::{adapter::AepConversionError, writer::footage::NativeSourceFormat};

pub(super) fn native_profile(
    source: &Path,
) -> Result<Option<(NativeSourceFormat, [u16; 2])>, AepConversionError> {
    let mut input = File::open(source)
        .map_err(|error| AepConversionError::io("open PNG header", source, error))?;
    let mut header = [0; 33];
    input
        .read_exact(&mut header)
        .map_err(|error| AepConversionError::io("read PNG header", source, error))?;
    // Archive MIME/extension are hints, not a replacement for source bytes.
    // Preserve the existing guessed-format EXR path for a JPEG named .png.
    if header[..8] != *b"\x89PNG\r\n\x1a\n" {
        return Ok(None);
    }
    if header[8..12] != 13_u32.to_be_bytes() || header[12..16] != *b"IHDR" {
        return Err(AepConversionError::Input("invalid PNG IHDR"));
    }
    let format = match (header[24], header[25], header[28]) {
        (8, 2, 0) => NativeSourceFormat::PngRgb,
        (8, 6, 0) => NativeSourceFormat::PngRgba,
        // Keep the existing EXR preparation for unproved PNG envelopes.
        _ => return Ok(None),
    };
    let input = File::open(source)
        .map_err(|error| AepConversionError::io("open PNG for validation", source, error))?;
    // Archive pixels are untrusted: retain the decoder's allocation limit so
    // oversized compressed images fail normally instead of aborting on OOM.
    let reader = image::ImageReader::with_format(BufReader::new(input), image::ImageFormat::Png);
    // Validate the full stream, not just a dimension header. Do not encode this
    // decoded buffer: that would lose source metadata and change pinned bytes.
    let decoded = reader.decode()?;
    let expected = if format == NativeSourceFormat::PngRgb {
        image::ColorType::Rgb8
    } else {
        image::ColorType::Rgba8
    };
    // RGB with tRNS expands to RGBA, outside the proved three-channel envelope.
    if decoded.color() != expected {
        return Ok(None);
    }
    let width = u16::try_from(decoded.width())
        .map_err(|_| AepConversionError::Input("PNG width exceeds native field"))?;
    let height = u16::try_from(decoded.height())
        .map_err(|_| AepConversionError::Input("PNG height exceeds native field"))?;
    if width == 0 || height == 0 {
        return Err(AepConversionError::Input("PNG has zero dimensions"));
    }
    Ok(Some((format, [width, height])))
}

#[cfg(test)]
mod tests {
    use super::native_profile;
    use crate::writer::footage::NativeSourceFormat;

    #[test]
    fn native_png_profile_distinguishes_rgb_and_rgba_without_rewriting() {
        let root = tempfile::tempdir().unwrap();
        let rgb = root.path().join("rgb.png");
        let rgba = root.path().join("rgba.png");
        image::RgbImage::from_pixel(3, 2, image::Rgb([35, 19, 43]))
            .save(&rgb)
            .unwrap();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([35, 19, 43, 128]))
            .save(&rgba)
            .unwrap();
        for (path, format) in [
            (&rgb, NativeSourceFormat::PngRgb),
            (&rgba, NativeSourceFormat::PngRgba),
        ] {
            let bytes = std::fs::read(path).unwrap();
            assert_eq!(native_profile(path).unwrap(), Some((format, [3, 2])));
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn native_png_profile_keeps_mislabeled_jpeg_on_existing_path() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("jpeg.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([35, 19, 43]))
            .save_with_format(&source, image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(native_profile(&source).unwrap(), None);
    }

    #[test]
    fn native_png_profile_keeps_unproved_grayscale_on_existing_path() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("gray.png");
        image::GrayImage::from_pixel(2, 2, image::Luma([128]))
            .save(&source)
            .unwrap();
        assert_eq!(native_profile(&source).unwrap(), None);
    }

    #[test]
    fn native_png_profile_keeps_rgb_transparency_on_existing_path() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("transparent-rgb.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([35, 19, 43]))
            .save(&source)
            .unwrap();
        let mut bytes = std::fs::read(&source).unwrap();
        // Valid tRNS chunk: transparent RGB [35,19,43], including its CRC.
        bytes.splice(
            33..33,
            [
                0, 0, 0, 6, b't', b'R', b'N', b'S', 0, 35, 0, 19, 0, 43, 0x5a, 0x1b, 0xb6, 0x2c,
            ],
        );
        std::fs::write(&source, &bytes).unwrap();
        let decoded = image::open(&source).unwrap();
        assert_eq!(decoded.color(), image::ColorType::Rgba8);
        assert_eq!(decoded.to_rgba8().get_pixel(0, 0).0, [35, 19, 43, 0]);
        assert_eq!(native_profile(&source).unwrap(), None);
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
    }

    #[test]
    fn native_png_profile_keeps_high_bit_depth_on_existing_path() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("rgba16.png");
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            2,
            2,
            image::Rgba([10000, 20000, 30000, 40000]),
        )
        .save(&source)
        .unwrap();
        assert_eq!(native_profile(&source).unwrap(), None);
    }

    #[test]
    fn native_png_profile_rejects_oversized_decode_before_allocating_pixels() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("oversized.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([35, 19, 43, 128]))
            .save(&source)
            .unwrap();
        let mut bytes = std::fs::read(&source).unwrap();
        // Dimensions fit the native u16 fields, but require 1.6 GB of pixels.
        bytes[16..20].copy_from_slice(&20_000_u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&20_000_u32.to_be_bytes());
        let mut crc = !0_u32;
        for byte in &bytes[12..29] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320_u32 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
        std::fs::write(&source, bytes).unwrap();
        // The IDAT is deliberately small: require an allocation-limit error,
        // not a later truncated-stream error after allocating the huge buffer.
        assert!(matches!(
            native_profile(&source),
            Err(crate::adapter::AepConversionError::Image(
                image::ImageError::Limits(_)
            ))
        ));
    }

    #[test]
    fn native_png_profile_rejects_a_truncated_pixel_stream() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("broken.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([35, 19, 43, 128]))
            .save(&source)
            .unwrap();
        let bytes = std::fs::read(&source).unwrap();
        std::fs::write(&source, &bytes[..40]).unwrap();
        assert!(native_profile(&source).is_err());
    }
}
