//! Container and codec admission for packaged video files.
//!
//! Conversion accepts ISO BMFF files named `.mp4` or `.mov`, holding one
//! sample entry of a [`VideoCodec`]: H.264 and HEVC in both directions, and
//! Apple ProRes for the Adobe-bound direction, where the file passes through
//! to Premiere or a linked AEP ([`VideoCodec::tesseract_player_rejection`]
//! keeps import classifying it as a transcode candidate). The native export renderer also finds
//! `.m4v` video, but its audio extension list has no `m4v`, so its audio preflight classes every `.m4v` video layer, even a
//! video-only one, as missing audio and fails the export ("never resolved").
//! `.m4v` therefore rejects in both directions until that list includes it.
//! Any other file extension or sample entry rejects with that extension or
//! code; conversion never transcodes media. HDR and other non-BT.709 colour
//! passes through in the unchanged bytes with one warning per file
//! (`media_metadata::ColourDescription`). Other container content under an
//! accepted name fails MP4 parsing.

mod hevc;

use crate::{
    error::{ensure, unsupported, Result},
    media::MediaContainer,
    media_metadata::{validate_h264, ColourDescription, SampleDescription},
    schema::VideoCodec,
};
use std::{
    io::{Read, Seek},
    path::Path,
};

/// Checks the container that a media file name declares, before reading it.
pub(crate) fn validate_video_file_name(path: &Path) -> Result<()> {
    if MediaContainer::from_path(path).is_some_and(MediaContainer::holds_video) {
        return Ok(());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    Err(unsupported(format!(
        "video file extension {extension:?} is unsupported; conversion accepts MP4 or QuickTime MOV video"
    )))
}

/// What the accepted sample entry and its parameter sets declare.
pub(crate) struct VideoFormat {
    pub(crate) codec: VideoCodec,
    /// Luma and chroma bit depth: 8, or 10 for HEVC Main 10.
    pub(crate) bit_depth: u8,
    /// The colour description that passes through, if any.
    pub(crate) colour: Option<ColourDescription>,
    pub(crate) orientation: crate::schema::VideoOrientation,
    pub(crate) pixel_aspect: crate::schema::records::PixelAspectRatio,
}

/// Classifies the one sample entry and checks the configuration it carries.
pub(crate) fn validate_codec(
    codec_tag: [u8; 4],
    extradata: &[u8],
    description: &SampleDescription,
    mut reader: impl Read + Seek,
) -> Result<VideoFormat> {
    let entry_name = String::from_utf8_lossy(&codec_tag);
    let codec = VideoCodec::from_sample_entry(codec_tag, description.depth).ok_or_else(|| {
        // The renderers' sample-entry tables map `hvc1` and `hev1` to the
        // HEVC decoder but not the Dolby Vision entries, so a converted
        // project holding one would not play or export.
        if matches!(&codec_tag, b"dvh1" | b"dvhe") {
            return crate::error::BuildError::UnsupportedVideoCodec(format!(
                "Dolby Vision {entry_name} sample entries are not decodable by the Tesseract engine; hvc1 with a Dolby Vision record converts"
            ));
        }
        if codec_tag == *b"rle " {
            return crate::error::BuildError::UnsupportedVideoCodec(
                "video codec \"rle \" is unsupported (QuickTime Animation / QTRLE): the Tesseract player has no decoder; explicitly prepare the whole source with transcode and a source-bound media map (verified 8-bit alpha prepares as ProRes 4444 MOV)".into(),
            );
        }
        crate::error::BuildError::UnsupportedVideoCodec(format!(
            "video codec {entry_name:?} is unsupported; conversion accepts H.264 (avc1), HEVC (hvc1) or Apple ProRes"
        ))
    })?;
    if description.entry != codec_tag {
        let name = if codec == VideoCodec::H264 {
            "AVC"
        } else {
            &entry_name
        };
        return Err(unsupported(format!(
            "MP4 must contain one unambiguous {name} sample description"
        )));
    }
    let (bit_depth, colour, pixel_aspect) = match codec {
        VideoCodec::H264 => {
            let (colour, pixel_aspect) = validate_h264(description, extradata)?;
            (8, colour, pixel_aspect)
        }
        VideoCodec::HevcMain => hevc::validate_configuration(description, &mut reader)?,
        // ProRes carries no parameter sets: the frame is the sample entry's,
        // colour and aspect are the container's declarations, and 4:2:2
        // profiles are 10-bit, 4444 profiles up to 12-bit.
        VideoCodec::ProRes { profile, .. } => {
            ensure!(
                description.full_range != Some(true),
                "full-range ProRes is unsupported; Apple ProRes is a limited-range format"
            );
            (
                if profile.is_4444() { 12 } else { 10 },
                description.colour,
                description
                    .pixel_aspect
                    .unwrap_or(crate::schema::records::PixelAspectRatio::SQUARE),
            )
        }
    };
    description.validate_display_dimensions(pixel_aspect)?;
    Ok(VideoFormat {
        codec,
        bit_depth,
        colour: colour.filter(ColourDescription::passes_through),
        orientation: description.orientation,
        pixel_aspect,
    })
}
