//! Export-only physical admission. Unsupported encoding never becomes a native
//! picture: retain a typed media result so lowering can record its source boundary.

use super::{validate_export_media_timing, MediaFacts, UnsupportedVideoMedia, VideoMedia};
use crate::{
    error::{ensure, Result},
    media_metadata::{read_export_movie_metadata, ColourDescription, SampleDescription},
    schema::records::PixelAspectRatio,
};
use media_transcode::inspect::{inspect, PacketInfo, StreamKind};
use std::io::{Read, Seek, SeekFrom};

pub(crate) fn inspect_export_video_media(
    reader: impl Read + Seek,
    mut metadata_reader: impl Read + Seek,
    size: u64,
) -> Result<MediaFacts> {
    let metadata = read_export_movie_metadata(&mut metadata_reader, size)?;
    let inspection = inspect(reader, size, true)?;
    let videos: Vec<_> = inspection
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Video)
        .collect();
    let [stream] = videos.as_slice() else {
        return Err(crate::error::unsupported(
            "source requires exactly one video stream",
        ));
    };
    ensure!(
        inspection
            .streams
            .iter()
            .filter(|stream| stream.kind == StreamKind::Audio)
            .count()
            <= 1
            && metadata
                .tracks
                .iter()
                .filter(|track| track.handler == *b"soun")
                .count()
                <= 1,
        "multiple audio streams are unsupported when sound is consumed or usage is unknown"
    );
    ensure!(
        !metadata
            .tracks
            .iter()
            .any(|track| matches!(&track.handler, b"sbtl" | b"text" | b"subt" | b"clcp")),
        "subtitle or caption tracks are unsupported"
    );
    let track = metadata
        .tracks
        .iter()
        .find(|track| track.handler == *b"vide")
        .ok_or_else(|| crate::error::unsupported("missing video track"))?;
    let description = track
        .sample_description
        .as_ref()
        .ok_or_else(|| crate::error::unsupported("missing video sample description"))?;
    ensure!(
        stream.codec_tag == description.entry,
        "MP4 must contain one unambiguous visual sample description"
    );
    ensure!(
        (stream.width, stream.height)
            == (u32::from(description.width), u32::from(description.height)),
        "MP4 sample-entry dimensions must match the decoded stream dimensions"
    );
    // Required packet and edit facts remain fatal. Export keeps coherent
    // presentation edits in the original bytes and uses their physical clock
    // only for facts that the native source descriptor requires.
    let timing =
        validate_export_media_timing(stream, &inspection.packets, track, metadata.timescale, size)?;
    let unsupported = match &description.entry {
        b"avc1" => {
            let (depth, colour, aspect) =
                crate::media_metadata::inspect_export_h264(description, &stream.extradata)?;
            description.validate_display_dimensions(aspect)?;
            if depth != 8 {
                Some("H.264 must be progressive 8-bit 4:2:0 with matching dimensions".to_owned())
            } else if colour.is_some_and(|colour| colour.codes().1 == 13) {
                Some(
                    "sRGB video transfer has no established native Premiere source profile"
                        .to_owned(),
                )
            } else {
                None
            }
        }
        b"ap4h" | b"ap4x" | b"apch" | b"apcn" | b"apcs" | b"apco" => {
            ensure!(
                stream.codec_name == "prores",
                "ProRes sample entry does not identify ProRes media"
            );
            let aspect = description.pixel_aspect.unwrap_or_default();
            reconcile_aspect(description, stream.sample_aspect_ratio)?;
            description.validate_display_dimensions(aspect)?;
            validate_prores_frames(
                &mut metadata_reader,
                description,
                inspection
                    .packets
                    .iter()
                    .filter(|packet| packet.stream_index == stream.index),
            )?;
            Some(format!(
                "video codec {:?} has no native Premiere picture/alpha export mapping",
                String::from_utf8_lossy(&description.entry)
            ))
        }
        _ => None,
    };
    // Validate an unsupported codec's physical payload before returning a
    // local loss. A presentation edit must not hide malformed media.
    let format = if unsupported.is_none() {
        Some(crate::video_format::validate_codec(
            stream.codec_tag,
            &stream.extradata,
            description,
            &mut metadata_reader,
        )?)
    } else {
        None
    };
    if let Some(reason) = unsupported {
        return Ok(MediaFacts::UnsupportedVideo(UnsupportedVideoMedia {
            width: stream.width,
            height: stream.height,
            timing: Some(timing),
            reason,
        }));
    }
    let format = format.expect("validated native codec");
    Ok(MediaFacts::Video(VideoMedia {
        pixel_aspect: format.pixel_aspect,
        codec: format.codec,
        bit_depth: format.bit_depth,
        colour: format.colour,
        width: stream.width,
        height: stream.height,
        orientation: format.orientation,
        timing,
    }))
}

fn reconcile_aspect(description: &SampleDescription, stream: Option<[i32; 2]>) -> Result<()> {
    if let Some([x, y]) = stream {
        ensure!(x > 0 && y > 0, "invalid video pixel aspect ratio");
        let aspect = PixelAspectRatio::new(x as u64, y as u64)?;
        ensure!(
            description.pixel_aspect.unwrap_or_default().agrees(aspect),
            "conflicting video pixel aspect ratio declarations"
        );
    }
    Ok(())
}

// ProRes packets are intra frames with an explicit size, signature, dimensions,
// scan mode and colour declaration. Check those headers, not just a fourcc that
// could have been applied to unrelated payloads. This is not full frame decoding.
fn validate_prores_frames<'a>(
    reader: &mut (impl Read + Seek),
    description: &SampleDescription,
    packets: impl Iterator<Item = &'a PacketInfo>,
) -> Result<()> {
    let mut colour = description.colour;
    for packet in packets {
        ensure!(packet.size >= 28, "truncated ProRes frame header");
        let position = u64::try_from(packet.position)
            .map_err(|_| crate::error::unsupported("invalid ProRes frame byte range"))?;
        reader.seek(SeekFrom::Start(position))?;
        let mut header = [0; 28];
        reader.read_exact(&mut header)?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("four-byte frame size"));
        let header_size = u16::from_be_bytes([header[8], header[9]]);
        ensure!(
            usize::try_from(length) == Ok(packet.size)
                && &header[4..8] == b"icpf"
                && header_size >= 20
                && usize::from(header_size) + 8 <= packet.size,
            "invalid ProRes frame header"
        );
        // Each low flag bit selects a 64-byte matrix immediately after the
        // fixed 20-byte header. The declared header, not just the packet, must
        // contain both selected tables before this becomes a physical fact.
        let matrix_bytes =
            usize::from(header[27] & 1 != 0) * 64 + usize::from(header[27] & 2 != 0) * 64;
        ensure!(
            usize::from(header_size) >= 20 + matrix_bytes,
            "ProRes quantization matrices exceed the declared frame header"
        );
        let mut matrices = [0; 128];
        reader.read_exact(&mut matrices[..matrix_bytes])?;
        ensure!(
            u16::from_be_bytes([header[10], header[11]]) <= 1
                && (
                    u16::from_be_bytes([header[16], header[17]]),
                    u16::from_be_bytes([header[18], header[19]])
                ) == (description.width, description.height)
                && (header[20] >> 2) & 3 == 0
                && [2, 3].contains(&(header[20] >> 6))
                && header[25] & 0x0f <= 2,
            "ProRes must have coherent dimensions, progressive frames and a known alpha layout"
        );
        ColourDescription::merge(
            &mut colour,
            crate::media_metadata::validate_export_color(
                u16::from(header[22]),
                u16::from(header[23]),
                u16::from(header[24]),
            )?,
        )?;
    }
    Ok(())
}
