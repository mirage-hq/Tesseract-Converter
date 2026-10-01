//! Video codecs that conversion accepts in both directions.

use super::records;

/// One accepted video codec, identified by its ISO BMFF sample-entry type.
///
/// The native export renderer and the web player (WebCodecs) decode each
/// variant, and each is a format Adobe documents Premiere importing. AME has
/// rendered the derived HEVC input; generated HEVC Premiere projects remain
/// unverified. Entry types that allow in-band parameter-set changes (`avc3`,
/// `hev1`) are excluded because inspection could not see those changes. Dolby
/// Vision entries (`dvh1`, `dvhe`) are excluded because neither renderer maps
/// them to its HEVC decoder (`video_format::validate_codec`); a Dolby Vision
/// record on an `hvc1` entry, the iPhone form, is checked by
/// `video_format::hevc`. Apple ProRes is excluded because the web editor and
/// player decode through WebCodecs, which has no ProRes decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoCodec {
    /// H.264 with its parameter sets in the sample entry (`avc1`).
    H264,
    /// HEVC Main or Main 10 with every parameter set in the sample entry
    /// (`hvc1`), with or without a Dolby Vision record.
    HevcMain,
}

impl VideoCodec {
    /// Maps a sample-entry type to an accepted codec.
    pub(crate) fn from_sample_entry(sample_entry: [u8; 4]) -> Option<Self> {
        Some(match &sample_entry {
            b"avc1" => Self::H264,
            b"hvc1" => Self::HevcMain,
            _ => return None,
        })
    }

    /// Returns the native `VideoStream.CodecType` that export writes, the code
    /// Premiere itself saves for the codec family rather than the file's
    /// sample entry: [`records::CODEC_TYPE`] (`avc1`) for H.264, as Premiere
    /// 24.3-26.5.1 corpus projects store it (the Premiere 12.1.1
    /// `copy_and_paste_effects` project stores `AVC1` for a lowercase `avc1`
    /// file), and [`records::HEVC_CODEC_TYPE`] (`HEVC`) for HEVC, as Premiere
    /// 26.5.1 saved it for three Main 10 `hvc1` masters, one with a Dolby
    /// Vision box (`oracle/M2/hdr/facts.md`).
    pub(crate) fn codec_type(self) -> &'static str {
        match self {
            Self::H264 => records::CODEC_TYPE,
            Self::HevcMain => records::HEVC_CODEC_TYPE,
        }
    }
}
