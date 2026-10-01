//! Video container/codec admission before any output archive is published.
//!
//! Keep the filename and sample-entry families aligned with codec::find_video_asset
//! and codec::ffmpeg::VideoCodec::from_stsd_fourcc without coupling the standalone
//! converter to the renderer. Header admission is not full frame decoding or a
//! guarantee that every downstream FFmpeg build includes a particular decoder.

use std::{fs::File, io::BufReader, path::Path};

use fx_conv::{SwfClassification, classify_swf};
use fx_schema::AssetId;
use media_transcode::inspect::{InspectError, StreamKind, inspect};

use super::AepConversionError;

pub(super) fn validate(
    file: &mut File,
    path: &Path,
    asset_id: &AssetId,
) -> Result<String, AepConversionError> {
    let reject = |reason: String| AepConversionError::VideoMedia {
        asset_id: asset_id.clone(),
        path: path.to_owned(),
        reason,
    };
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("swf") {
        let reason = match classify_swf(path) {
            Ok(SwfClassification::EmbeddedVideoCandidate) => {
                "SWF embedded-video candidate requires explicit extraction and admission checks"
                    .into()
            }
            Ok(SwfClassification::ExternalRenderRequired { reason }) => {
                format!("SWF external render required: {reason}")
            }
            Ok(SwfClassification::Unassessed { reason }) => {
                format!("SWF unassessed: {reason}")
            }
            Err(error) => format!("cannot classify SWF: {error}"),
        };
        return Err(reject(reason));
    }
    if !["mp4", "mov", "m4v"]
        .iter()
        .any(|supported| extension.eq_ignore_ascii_case(supported))
    {
        return Err(reject(format!(
            "unsupported video extension {extension:?}; expected mp4, mov or m4v"
        )));
    }
    let length = file
        .metadata()
        .map_err(|error| AepConversionError::io("inspect video", path, error))?
        .len();
    let container = inspect(BufReader::new(file), length, false).map_err(|error| match error {
        InspectError::Io(error) => AepConversionError::io("read video container", path, error),
        error => reject(format!("cannot read video container: {error}")),
    })?;
    let mut codec = None;
    for track in &container.streams {
        if track.kind != StreamKind::Video {
            continue;
        }
        let fourcc = track.codec_tag;
        codec.get_or_insert_with(|| String::from_utf8_lossy(&fourcc).into_owned());
        if !supported_codec(&fourcc) {
            let name = String::from_utf8_lossy(&fourcc);
            let detail = if fourcc == *b"rle " {
                " (QTRLE / QuickTime Animation)"
            } else {
                ""
            };
            return Err(reject(format!("unsupported video codec {name:?}{detail}")));
        }
    }
    codec.ok_or_else(|| reject("container has no video track".into()))
}

fn supported_codec(fourcc: &[u8; 4]) -> bool {
    matches!(
        fourcc,
        b"avc1"
            | b"hev1"
            | b"hvc1"
            | b"vp09"
            | b"apcn"
            | b"apch"
            | b"apcs"
            | b"apco"
            | b"ap4h"
            | b"ap4x"
            | b"mp4v"
            | b"av01"
            | b"apv1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ffmpeg-library")]
    #[test]
    fn header_admission_does_not_decode_prores_tagged_payloads() {
        use std::io::Write;

        // Only the sample-entry tag changes: these remain H.264 payloads. This
        // supplementary mutation tests header-only admission, not ProRes fidelity.
        let source = include_bytes!("../../../tests/fixtures/audio_e2e/movie.mov");
        for tag in [b"avc1", b"ap4h", b"ap4x"] {
            let mut bytes = source.to_vec();
            let stsd = bytes.windows(4).position(|value| value == b"stsd").unwrap();
            assert_eq!(&bytes[stsd + 16..stsd + 20], b"avc1");
            bytes[stsd + 16..stsd + 20].copy_from_slice(tag);
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(&bytes).unwrap();
            let codec = validate(
                file.as_file_mut(),
                Path::new("source.mov"),
                &AssetId::new("video").unwrap(),
            )
            .unwrap();
            assert_eq!(codec.as_bytes(), tag);
        }
    }

    #[test]
    fn codec_families_match_native_dispatch_without_rejecting_prores_alpha() {
        for fourcc in [
            b"avc1", b"hev1", b"hvc1", b"vp09", b"apcn", b"apch", b"apcs", b"apco", b"ap4h",
            b"ap4x", b"mp4v", b"av01", b"apv1",
        ] {
            assert!(supported_codec(fourcc));
        }
        for fourcc in [b"rle ", b"jpeg", b"raw ", b"SVQ3"] {
            assert!(!supported_codec(fourcc));
        }
    }
}
