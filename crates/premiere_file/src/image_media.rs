//! Inspect one still-image source with the `image` decoders the renderer uses.
//!
//! PNG/JPEG inspection decodes headers: format, pixel dimensions, alpha, and
//! whether an ICC profile is embedded. OpenEXR inspection retains the original
//! compressed source for Premiere's native importer after validating its
//! headers, offset tables and compressed chunks without decoding pixels.
//! Decoder errors reject malformed, truncated,
//! IDAT-less, 12-bit, lossless, and arithmetic-coded files; an Exif orientation
//! other than 1 (the renderer rotates the pixels) and APNG reject too. CMYK/YCCK
//! JPEG, unreadable Exif (drawn as orientation 1), and PNG `cICP` (not exposed
//! by the decoder) convert as the renderer draws them; Premiere parity is inferred.

mod exr;

use crate::{
    error::{unsupported, BuildError, Result},
    media::{MediaFacts, UnsupportedStillMedia},
};
use image::{
    codecs::{jpeg::JpegDecoder, png::PngDecoder},
    metadata::Orientation,
    ImageDecoder, ImageError, ImageReader,
};
use std::io::{BufReader, ErrorKind, Read, Seek};

/// Still-image container accepted by conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFormat {
    Jpeg,
    Png,
    OpenExr,
}

impl ImageFormat {
    /// The format a package file extension names, case-insensitively: the one
    /// still-image extension rule, which `media::MediaContainer` applies per
    /// media kind.
    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "exr" | "sxr" | "mxr" => Some(Self::OpenExr),
            _ => None,
        }
    }

    pub(crate) fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::OpenExr => "image/x-exr",
        }
    }

    pub(crate) fn matches_content_type(self, content_type: &str) -> bool {
        match self {
            Self::OpenExr => matches!(
                content_type,
                "image/x-exr" | "image/exr" | "application/x-exr" | "image/unknown"
            ),
            _ => content_type == self.content_type(),
        }
    }

    /// Whether this file format matches the native still importer that the
    /// media record declares.
    pub(crate) fn matches_media_kind(self, kind: crate::schema::PrMediaKind) -> bool {
        match kind {
            crate::schema::PrMediaKind::Still { .. }
            | crate::schema::PrMediaKind::NumberedStills { .. } => {
                matches!(self, Self::Jpeg | Self::Png)
            }
            crate::schema::PrMediaKind::OpenExr { .. } => self == Self::OpenExr,
            _ => false,
        }
    }
}

/// Source facts shared by every placement of one validated still image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ValidatedImage {
    pub(crate) format: ImageFormat,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixel_aspect: crate::schema::records::PixelAspectRatio,
    pub(crate) open_exr_channels: Option<crate::schema::OpenExrChannels>,
    /// A PNG alpha channel, `tRNS` chunk, or selected OpenEXR A channel.
    pub(crate) alpha: bool,
    /// An embedded ICC profile; the renderer converts only a Display-P3 one.
    pub(crate) icc_profile: bool,
}

impl ValidatedImage {
    /// Why this file cannot stand for a native still that does or does not
    /// declare straight alpha under a package name ending in `extension`.
    ///
    /// The package content type follows the extension, so PNG bytes under a
    /// `.jpg` name reject. Alpha must agree both ways: Premiere would draw an
    /// undeclared alpha channel opaque, and declared alpha needs an alpha
    /// channel. `AlphaType` `1` with RGBA PNG (`cinemagraph`, `phone_title`),
    /// `AlphaType` `2` with RGBA OpenEXR, and the absent declarations for JPEG
    /// and RGB OpenEXR are observed. The grey+alpha, `tRNS` and opaque-PNG
    /// declarations are inferred, so this check can omit such a still on import
    /// but never import one whose file contradicts it.
    pub(crate) fn declaration_mismatch(
        &self,
        extension: Option<&str>,
        declared_alpha: bool,
    ) -> Option<String> {
        if extension.and_then(ImageFormat::from_extension) != Some(self.format) {
            return Some(format!(
                "still file extension does not match its {:?} image data",
                self.format
            ));
        }
        match (self.alpha, declared_alpha) {
            (true, false) => Some(
                "still image carries alpha that its native AlphaType does not declare; Premiere would render it opaque".to_owned(),
            ),
            (false, true) => Some(
                "still image has no alpha channel although its native AlphaType declares straight alpha".to_owned(),
            ),
            _ => None,
        }
    }

    /// The Feature note reported for every still that embeds an ICC profile.
    pub(crate) fn colour_note(&self) -> Option<&'static str> {
        self.icc_profile.then_some(
            "still image embeds an ICC colour profile; colours are kept as stored and parity with Premiere colour management is inferred",
        )
    }
}

/// Inspect a still image by its content, independently of its file name.
pub(crate) fn inspect_image_media(reader: impl Read + Seek) -> Result<ValidatedImage> {
    let reader = ImageReader::new(BufReader::new(reader)).with_guessed_format()?;
    match reader.format() {
        Some(image::ImageFormat::Jpeg) => {
            let decoder = JpegDecoder::new(reader.into_inner()).map_err(undecodable)?;
            decoded_facts(ImageFormat::Jpeg, decoder)
        }
        Some(image::ImageFormat::Png) => {
            let decoder = PngDecoder::new(reader.into_inner()).map_err(undecodable)?;
            if decoder.is_apng().map_err(undecodable)? {
                return Err(unsupported(
                    "animated PNG (APNG) is unsupported; image sequences are out of scope for stills",
                ));
            }
            decoded_facts(ImageFormat::Png, decoder)
        }
        Some(image::ImageFormat::OpenExr) => match exr::inspect(reader.into_inner(), None)? {
            exr::Inspection::Native(image) => Ok(image),
            exr::Inspection::PremiereUnsupported { reason, .. } => Err(unsupported(reason)),
        },
        other => Err(unsupported(format!(
            "still image must be a PNG, JPEG, or OpenEXR file; {} data is unsupported",
            other.map_or("unrecognized image", |format| format.to_mime_type())
        ))),
    }
}

/// Inspect an export source and verify that its archived byte count is stable.
pub(crate) fn inspect_export_image_media(
    reader: impl Read + Seek,
    size: u64,
) -> Result<MediaFacts> {
    let reader = ImageReader::new(BufReader::new(reader)).with_guessed_format()?;
    if reader.format() == Some(image::ImageFormat::OpenExr) {
        return Ok(match exr::inspect(reader.into_inner(), Some(size))? {
            exr::Inspection::Native(image) => MediaFacts::Still(image),
            exr::Inspection::PremiereUnsupported { image, reason } => {
                MediaFacts::UnsupportedStill(UnsupportedStillMedia {
                    width: image.width,
                    height: image.height,
                    reason,
                })
            }
        });
    }
    inspect_image_media(reader.into_inner()).map(MediaFacts::Still)
}

/// PNG pHYs stores pixels per unit, so pixel width/height is Y/X. The PNG
/// specification defines square pixels when pHYs is absent, independently of
/// the coded dimensions: https://www.w3.org/TR/png-3/#11pHYs .
pub(crate) fn inspect_png_pixel_aspect(
    reader: impl Read + Seek,
) -> Result<(crate::schema::records::PixelAspectRatio, &'static str)> {
    let decoder = png::Decoder::new(BufReader::new(reader));
    let reader = decoder.read_info().map_err(|error| match error {
        png::DecodingError::IoError(error) if error.kind() != ErrorKind::UnexpectedEof => {
            BuildError::Io(error)
        }
        error => unsupported(format!(
            "PNG pixel-aspect metadata cannot be decoded: {error}"
        )),
    })?;
    match reader.info().pixel_dims {
        Some(density) => Ok((
            crate::schema::records::PixelAspectRatio::new(
                u64::from(density.yppu),
                u64::from(density.xppu),
            )?,
            "PNG pHYs metadata",
        )),
        None => Ok((
            crate::schema::records::PixelAspectRatio::SQUARE,
            "PNG-defined square-pixel default (no pHYs)",
        )),
    }
}

fn decoded_facts(format: ImageFormat, mut decoder: impl ImageDecoder) -> Result<ValidatedImage> {
    let orientation = decoder.orientation().map_err(undecodable)?;
    if orientation != Orientation::NoTransforms {
        return Err(unsupported(format!(
            "still image Exif orientation {} is unsupported; only unrotated (orientation 1) stills convert",
            orientation.to_exif()
        )));
    }
    let (width, height) = decoder.dimensions();
    Ok(ValidatedImage {
        format,
        width,
        height,
        pixel_aspect: crate::schema::records::PixelAspectRatio::SQUARE,
        open_exr_channels: None,
        alpha: decoder.color_type().has_alpha(),
        icc_profile: decoder.icc_profile().map_err(undecodable)?.is_some(),
    })
}

/// Malformed or unsupported data rejects the still; failing to read the
/// source stays an I/O error.
fn undecodable(error: ImageError) -> BuildError {
    match error {
        ImageError::IoError(error) if error.kind() != ErrorKind::UnexpectedEof => error.into(),
        error => unsupported(format!("still image data cannot be decoded: {error}")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_pixel_aspect_uses_density_or_the_format_default_not_dimensions() {
        let png = |density: Option<png::PixelDimensions>| {
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.set_pixel_dims(density);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&[0; 6])
                    .unwrap();
            }
            bytes
        };
        for unit in [png::Unit::Unspecified, png::Unit::Meter] {
            let bytes = png(Some(png::PixelDimensions {
                xppu: 1000,
                yppu: 2000,
                unit,
            }));
            let (aspect, origin) =
                super::inspect_png_pixel_aspect(std::io::Cursor::new(bytes)).unwrap();
            assert_eq!(aspect.terms(), (2000, 1000));
            assert_eq!(origin, "PNG pHYs metadata");
        }
        let bytes = png(None);
        let (aspect, origin) =
            super::inspect_png_pixel_aspect(std::io::Cursor::new(&bytes)).unwrap();
        assert!(aspect.is_square());
        assert!(origin.contains("no pHYs"));
        assert!(super::inspect_png_pixel_aspect(std::io::Cursor::new(&bytes[..16])).is_err());
        for (xppu, yppu) in [(0, 1), (1, 0)] {
            let bytes = png(Some(png::PixelDimensions {
                xppu,
                yppu,
                unit: png::Unit::Unspecified,
            }));
            assert!(super::inspect_png_pixel_aspect(std::io::Cursor::new(bytes)).is_err());
        }
    }

    use super::{ImageFormat::*, *};
    use std::io::Cursor;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/feature_still_");
    const IDAT: (&[u8; 4], &[u8]) = (b"IDAT", &[0; 4]);

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{FIXTURES}{name}")).unwrap()
    }

    /// A 1920×1080 PNG of `colour` type with `chunks` after IHDR; the image
    /// data can be a stub because only headers are decoded.
    fn png(colour: u8, chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let ihdr = [0, 0, 7, 128, 0, 0, 4, 56, 8, colour, 0, 0, 0];
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in [(b"IHDR", &ihdr[..])].iter().chain(chunks) {
            let mut crc = flate2::Crc::new();
            crc.update(&[&kind[..], data].concat());
            let length = (data.len() as u32).to_be_bytes();
            bytes.extend([&length[..], &kind[..], data, &crc.sum().to_be_bytes()].concat());
        }
        bytes
    }

    /// The fixture JPEG with one more segment after SOI.
    fn jpeg_with(marker: u8, payload: &[u8]) -> Vec<u8> {
        let jpeg = fixture("opaque.jpg");
        let length = (payload.len() as u16 + 2).to_be_bytes();
        [&jpeg[..2], &[0xff, marker], &length, payload, &jpeg[2..]].concat()
    }

    /// The three-component fixture JPEG with a fourth (CMYK) component
    /// declared in its frame and scan headers.
    fn cmyk_jpeg() -> Vec<u8> {
        let mut jpeg = fixture("opaque.jpg");
        let at = |jpeg: &[u8], marker| jpeg.windows(2).position(|w| w == [0xff, marker]);
        let scan = at(&jpeg, 0xda).unwrap();
        jpeg.splice(scan + 3..scan + 5, [14, 4]); // length and component count
        jpeg.splice(scan + 11..scan + 11, [4, 0x11]); // component 4, tables 1/1
        let frame = at(&jpeg, 0xc0).unwrap();
        jpeg.splice(frame + 3..frame + 4, [20]);
        jpeg[frame + 9] = 4;
        jpeg.splice(frame + 19..frame + 19, [4, 0x11, 0]); // component 4, 1×1, table 0
        jpeg
    }

    #[test]
    fn decoder_facts_accept_or_reject_each_still() {
        let exif_3 = b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x03\0\0\0\0";
        let (jpeg, rgba) = (fixture("opaque.jpg"), fixture("transparent.png"));
        let opaque = png(2, &[IDAT]);
        let trns = png(3, &[(b"PLTE", &[0; 3]), (b"tRNS", &[0]), IDAT]);
        let bad_exif = jpeg_with(0xe1, b"Exif\0\0XX\0*");
        let icc = jpeg_with(0xe2, b"ICC_PROFILE\0\x01\x01data");
        let rotated = jpeg_with(0xe1, exif_3);
        let apng = png(6, &[(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]), IDAT]);
        let idat_less = png(6, &[(b"IEND", &[])]);
        let truncated = rgba[..40].to_vec();
        let cases = [
            (jpeg, Ok((Jpeg, false, false))),
            (rgba, Ok((Png, true, false))),
            (opaque, Ok((Png, false, false))),
            (trns, Ok((Png, true, false))),
            (cmyk_jpeg(), Ok((Jpeg, false, false))),
            (bad_exif, Ok((Jpeg, false, false))),
            (icc, Ok((Jpeg, false, true))),
            (rotated, Err("Exif orientation 3 is unsupported")),
            (apng, Err("animated PNG")),
            (idat_less, Err("cannot be decoded")),
            (truncated, Err("cannot be decoded")),
            (b"GIF89a".to_vec(), Err("image/gif data is unsupported")),
        ];
        for (row, (bytes, expected)) in cases.into_iter().enumerate() {
            let facts = inspect_image_media(Cursor::new(bytes))
                .map(|image| {
                    (
                        image.format,
                        image.alpha,
                        image.icc_profile,
                        image.width,
                        image.height,
                    )
                })
                .map_err(|error| error.to_string());
            match expected {
                Ok((format, alpha, icc)) => {
                    assert_eq!(facts, Ok((format, alpha, icc, 1920, 1080)), "row {row}")
                }
                Err(reason) => assert!(facts.is_err_and(|e| e.contains(reason)), "row {row}"),
            }
        }
    }
}
