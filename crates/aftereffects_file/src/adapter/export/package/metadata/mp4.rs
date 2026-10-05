//! Export-only AVC presentation profile. libavformat applies container edits and
//! composition offsets; the original container remains the native decoder input.

use super::{MetadataError, MetadataReadError, QuickTimeMetadata, native_frame_rate};
use crate::media::MediaDuration;
use media_transcode::inspect::{self, InspectError, MediaInspection, StreamKind};
use std::io::{Read, Seek};

mod terminal_sample;

pub(in crate::adapter::export::package) fn from_reader(
    mut reader: impl Read + Seek,
    size: u64,
) -> Result<QuickTimeMetadata, MetadataReadError> {
    let original = super::container_metadata_from_reader(&mut reader, size)?;
    original_profile(&original)?;
    let rounding = final_frame_rounding(&original)?;
    let terminal_sample = terminal_sample::from_original(&original)?;
    let inspection = inspect::inspect_presentation(reader, size).map_err(|error| match error {
        InspectError::Io(error) => MetadataReadError::Io(error),
        InspectError::Unavailable => {
            MetadataError::Unsupported("MP4 inspection requires FFmpeg").into()
        }
        InspectError::Invalid(_) => MetadataError::Malformed("invalid MP4 container").into(),
    })?;
    presentation_profiles(&inspection, rounding, terminal_sample).map_err(Into::into)
}

// FFmpeg's displayed packet grid alone does not establish authored elst rates
// or clap geometry. Reuse the existing container reader/sample-entry validation
// for these original facts before consuming its presentation timestamps.
fn original_profile(bytes: &[u8]) -> Result<(), MetadataError> {
    use super::{
        AtomBudget, atoms, exactly_one, optional_one, parse_handler, parse_video_description,
    };
    let mut budget = AtomBudget {
        remaining: usize::MAX,
    };
    let roots = atoms(bytes, &mut budget)?;
    let movie = exactly_one(&roots, *b"moov", "MP4 requires one movie metadata atom")?;
    for track in atoms(movie.payload, &mut budget)?
        .iter()
        .filter(|atom| atom.kind == *b"trak")
    {
        let children = atoms(track.payload, &mut budget)?;
        if let Some(edits) = optional_one(&children, *b"edts", "MP4 has duplicate edit containers")?
        {
            require_unit_rate_edits(edits.payload, &mut budget)?;
        }
        let media = exactly_one(&children, *b"mdia", "MP4 track requires one media atom")?;
        let children = atoms(media.payload, &mut budget)?;
        let handler = exactly_one(&children, *b"hdlr", "MP4 track requires one handler")?;
        if parse_handler(handler.payload)? != *b"vide" {
            continue;
        }
        let info = exactly_one(&children, *b"minf", "MP4 video requires media information")?;
        let children = atoms(info.payload, &mut budget)?;
        let table = exactly_one(&children, *b"stbl", "MP4 video requires sample table")?;
        let children = atoms(table.payload, &mut budget)?;
        let description =
            exactly_one(&children, *b"stsd", "MP4 video requires sample description")?;
        parse_video_description(description.payload, &mut budget)?;
    }
    Ok(())
}

fn require_unit_rate_edits(
    bytes: &[u8],
    budget: &mut super::AtomBudget,
) -> Result<(), MetadataError> {
    let edits = super::atoms(bytes, budget)?;
    let list = super::exactly_one(
        &edits,
        *b"elst",
        "MP4 edit container requires one edit list",
    )?
    .payload;
    if edits.len() != 1 {
        return Err(MetadataError::Unsupported(
            "MP4 edit-container extensions are unsupported",
        ));
    }
    if list.len() < 8 {
        return Err(MetadataError::Malformed(
            "MP4 edit list structure is invalid",
        ));
    }
    let entry_size = match list[0] {
        0 => 12_usize,
        1 => 20,
        _ => {
            return Err(MetadataError::Unsupported(
                "MP4 edit-list version is unsupported",
            ));
        }
    };
    if list[1..4] != [0; 3] {
        return Err(MetadataError::Unsupported(
            "MP4 edit-list flags are unsupported",
        ));
    }
    let count = usize::try_from(super::read_u32(list, 4, "MP4 edit count is truncated")?)
        .map_err(|_| MetadataError::Malformed("MP4 edit count exceeds address range"))?;
    let expected_length = count
        .checked_mul(entry_size)
        .and_then(|length| length.checked_add(8))
        .ok_or(MetadataError::Malformed("MP4 edit-list size overflows"))?;
    if count == 0 || list.len() != expected_length {
        return Err(MetadataError::Malformed(
            "MP4 edit-list entry length is invalid",
        ));
    }
    for index in 0..count {
        let rate = super::read_u32(
            list,
            8 + index * entry_size + entry_size - 4,
            "MP4 edit rate is truncated",
        )?;
        if rate != 0x0001_0000 {
            return Err(MetadataError::Unsupported(
                "MP4 dwell or nonunit edit rates are unsupported",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct FinalFrameRounding {
    edit_duration: u64,
    movie_duration: u64,
}

// Adobe interprets this verified millisecond movie-clock rounding profile as
// complete frames. Preserve the original file and its zero-origin packet grid.
fn final_frame_rounding(bytes: &[u8]) -> Result<Option<FinalFrameRounding>, MetadataError> {
    let mut budget = super::AtomBudget {
        remaining: usize::MAX,
    };
    let roots = super::atoms(bytes, &mut budget)?;
    let movie = super::exactly_one(&roots, *b"moov", "MP4 requires one movie")?;
    let children = super::atoms(movie.payload, &mut budget)?;
    let header = super::parse_movie_header(
        super::exactly_one(&children, *b"mvhd", "MP4 requires movie header")?.payload,
    )?;
    if header.timescale != 1000 {
        return Ok(None);
    }
    let mut result = None;
    for track in children.iter().filter(|atom| atom.kind == *b"trak") {
        let track = super::atoms(track.payload, &mut budget)?;
        let media = super::exactly_one(&track, *b"mdia", "MP4 requires media")?;
        let media = super::atoms(media.payload, &mut budget)?;
        if super::parse_handler(
            super::exactly_one(&media, *b"hdlr", "MP4 requires handler")?.payload,
        )? != *b"vide"
        {
            continue;
        }
        let Some(edits) = super::optional_one(&track, *b"edts", "MP4 duplicate edits")? else {
            return Ok(None);
        };
        let edits = super::atoms(edits.payload, &mut budget)?;
        let list = super::exactly_one(&edits, *b"elst", "MP4 requires edit list")?.payload;
        // The independently verified profile has one nonnegative version-zero
        // unit-rate edit; multi-edit, delayed and other clock profiles stay closed.
        if list.len() != 20
            || list[0..4] != [0; 4]
            || super::read_u32(list, 4, "MP4 edit count")? != 1
            || super::read_u32(list, 12, "MP4 edit origin")? > i32::MAX as u32
            || super::read_u32(list, 16, "MP4 edit rate")? != 0x0001_0000
        {
            return Ok(None);
        }
        let edit_duration = u64::from(super::read_u32(list, 8, "MP4 edit duration")?);
        let track_header = super::parse_track_header(
            super::exactly_one(&track, *b"tkhd", "MP4 requires track header")?.payload,
        )?;
        if track_header.duration != header.duration || result.is_some() {
            return Ok(None);
        }
        result = Some(FinalFrameRounding {
            edit_duration,
            movie_duration: header.duration,
        });
    }
    Ok(result)
}

#[cfg(test)]
fn presentation(media: &MediaInspection) -> Result<QuickTimeMetadata, MetadataError> {
    presentation_profiles(media, None, None)
}

#[cfg(test)]
fn presentation_with_rounding(
    media: &MediaInspection,
    rounding: Option<FinalFrameRounding>,
) -> Result<QuickTimeMetadata, MetadataError> {
    presentation_profiles(media, rounding, None)
}

fn presentation_profiles(
    media: &MediaInspection,
    rounding: Option<FinalFrameRounding>,
    terminal_sample: Option<terminal_sample::TerminalSample>,
) -> Result<QuickTimeMetadata, MetadataError> {
    let unsupported = MetadataError::Unsupported;
    let mut videos = media
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Video);
    let video = videos
        .next()
        .ok_or(unsupported("MP4 has no video stream"))?;
    if videos.next().is_some() || video.codec_tag != *b"avc1" || video.codec_name != "h264" {
        return Err(unsupported(
            "MP4 native profile requires a single AVC video stream",
        ));
    }
    if video
        .sample_aspect_ratio
        .is_some_and(|[num, den]| num <= 0 || den <= 0 || num != den)
    {
        return Err(unsupported("MP4 native profile requires square pixels"));
    }
    if video
        .display_matrix
        .is_some_and(|matrix| matrix != [65536, 0, 0, 0, 65536, 0, 0, 0, 1073741824])
    {
        return Err(unsupported(
            "MP4 native profile requires an identity display matrix",
        ));
    }
    let dimensions = [
        u16::try_from(video.width).map_err(|_| unsupported("MP4 width exceeds native range"))?,
        u16::try_from(video.height).map_err(|_| unsupported("MP4 height exceeds native range"))?,
    ];
    if dimensions.contains(&0)
        || video.time_base_num <= 0
        || video.time_base_den <= 0
        || video.duration <= 0
    {
        return Err(MetadataError::Malformed(
            "MP4 video geometry or clock is invalid",
        ));
    }
    let mut packets: Vec<_> = media
        .packets
        .iter()
        .filter(|packet| packet.stream_index == video.index)
        .collect();
    packets.sort_unstable_by_key(|packet| packet.pts);
    let first = packets
        .first()
        .ok_or(unsupported("MP4 video has no presentation packets"))?;
    let delta = first.duration;
    if delta <= 0 {
        return Err(unsupported("MP4 video has no positive frame duration"));
    }
    // Sorting PTS, rather than DTS, keeps reordered AVC frames in the displayed
    // clock. Reject delay/trim/irregular grids not represented by this profile.
    for (index, packet) in packets.iter().enumerate() {
        let expected = i64::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(delta))
            .ok_or(MetadataError::Malformed("MP4 presentation clock overflows"))?;
        if packet.pts != Some(expected) || packet.duration != delta {
            return Err(unsupported(
                "MP4 video presentation is not a zero-origin constant frame grid",
            ));
        }
    }
    let duration = i64::try_from(packets.len())
        .ok()
        .and_then(|count| count.checked_mul(delta))
        .ok_or(MetadataError::Malformed(
            "MP4 presentation duration overflows",
        ))?;
    let rounded_final_frame = rounding.is_some_and(|rounding| {
        let grid = u128::try_from(duration).unwrap_or(0) * video.time_base_num as u128 * 1000;
        let denominator = video.time_base_den as u128;
        let edited = u128::from(rounding.edit_duration) * denominator;
        let tick_denominator = 1000 * video.time_base_num as u128;
        grid / denominator == u128::from(rounding.edit_duration)
            && grid.div_ceil(denominator) == u128::from(rounding.movie_duration)
            && (edited + tick_denominator / 2) / tick_denominator == video.duration as u128
            && video.duration < duration
    });
    // Native AE displays the single zero-duration terminal sample for a full
    // frame. Only admit the independently verified original-container profile.
    let complete_terminal_sample = terminal_sample.is_some_and(|sample| {
        sample.matches(
            packets.len(),
            delta,
            video.time_base_num,
            video.time_base_den,
            video.duration,
        )
    });
    if duration != video.duration && !rounded_final_frame && !complete_terminal_sample {
        return Err(unsupported(
            "MP4 video duration disagrees with presentation packets",
        ));
    }
    let sample_delta = u32::try_from(delta)
        .ok()
        .and_then(|delta| delta.checked_mul(video.time_base_num as u32))
        .ok_or(unsupported("MP4 frame duration exceeds native range"))?;
    let frame_rate = native_frame_rate(video.time_base_den as u32, sample_delta)?;
    let numerator = u64::try_from(duration)
        .ok()
        .and_then(|duration| duration.checked_mul(video.time_base_num as u64))
        .and_then(|duration| duration.checked_mul(1000))
        .ok_or(MetadataError::Malformed("MP4 duration overflows"))?;
    let denominator = video.time_base_den as u64;
    let mut audio = media
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Audio);
    let audio_sample_rate = if let Some(stream) = audio.next() {
        if audio.next().is_some()
            || stream.codec_tag != *b"mp4a"
            || stream.codec_name != "aac"
            || stream.sample_rate == 0
            || !(1..=2).contains(&stream.channels)
        {
            return Err(unsupported(
                "MP4 native profile requires mono/stereo AAC audio",
            ));
        }
        f64::from(stream.sample_rate)
    } else {
        0.0
    };
    if media
        .streams
        .iter()
        .any(|stream| !matches!(stream.kind, StreamKind::Video | StreamKind::Audio))
    {
        return Err(unsupported(
            "MP4 native profile does not support ancillary streams",
        ));
    }
    // The native 50/60 fps sources store sample count / rate in sspc. A
    // 24576 Hz duration would shorten fractional-tick endpoints such as 3.2 s.
    let native_duration = if matches!(frame_rate.integer, 50 | 60) && frame_rate.fractional == 0 {
        Some(MediaDuration {
            numerator: u32::try_from(packets.len())
                .map_err(|_| unsupported("MP4 sample count exceeds native duration range"))?,
            denominator: frame_rate.integer,
        })
    } else {
        None
    };
    Ok(QuickTimeMetadata {
        dimensions,
        duration_millis: numerator.div_ceil(denominator),
        duration_millis_floor: numerator / denominator,
        duration_native_ticks: None,
        frame_rate,
        native_duration,
        video_codec: *b"avc1",
        audio_sample_rate,
    })
}

#[cfg(all(test, feature = "ffmpeg-library"))]
mod tests {
    use super::*;
    use std::io::Cursor;

    const NATIVE_MEDIA: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../premiere_file/tests/fixtures/feature_rate_24_blue.mp4"
    ));

    #[test]
    fn mp4_public_p030_profile_retains_complete_final_frame() {
        let bytes =
            include_bytes!("../../../../../tests/fixtures/p030-final-frame/public-241-frames.mp4");
        let result = from_reader(Cursor::new(bytes), bytes.len() as u64).unwrap();
        assert_eq!(result.duration_millis, 10_042);
        assert_eq!(result.duration_millis_floor, 10_041);
        let trimmed = mutate_atom_path(bytes, &[*b"moov", *b"trak", *b"edts", *b"elst"], &|list| {
            let mut list = list.to_vec();
            list[8..12].copy_from_slice(&10_040_u32.to_be_bytes());
            list
        });
        assert!(from_reader(Cursor::new(&trimmed), trimmed.len() as u64).is_err());
    }

    #[test]
    fn mp4_final_frame_rounding_requires_exact_movie_clock_bracketing() {
        let mut media = native_media();
        let video = media
            .streams
            .iter_mut()
            .find(|stream| stream.kind == StreamKind::Video)
            .unwrap();
        let index = video.index;
        video.time_base_num = 1;
        video.time_base_den = 12_288;
        video.duration = 123_384;
        let template = media
            .packets
            .iter()
            .find(|packet| packet.stream_index == index)
            .unwrap()
            .clone();
        media.packets = (0..241)
            .map(|i| {
                let mut packet = template.clone();
                packet.pts = Some(i * 512);
                packet.dts = packet.pts;
                packet.duration = 512;
                packet
            })
            .collect();
        let rounding = FinalFrameRounding {
            edit_duration: 10_041,
            movie_duration: 10_042,
        };
        assert!(presentation(&media).is_err());
        let result = presentation_with_rounding(&media, Some(rounding)).unwrap();
        assert_eq!(result.duration_millis, 10_042);
        assert_eq!(
            result.frame_rate,
            crate::writer::footage::NativeFrameRate::integer(24)
        );
        assert!(
            presentation_with_rounding(
                &media,
                Some(FinalFrameRounding {
                    edit_duration: 10_040,
                    ..rounding
                })
            )
            .is_err()
        );
        assert!(
            presentation_with_rounding(
                &media,
                Some(FinalFrameRounding {
                    movie_duration: 10_041,
                    ..rounding
                })
            )
            .is_err()
        );
        media
            .streams
            .iter_mut()
            .find(|stream| stream.kind == StreamKind::Video)
            .unwrap()
            .duration -= 512;
        assert!(presentation_with_rounding(&media, Some(rounding)).is_err());
    }

    #[test]
    fn mp4_p004_terminal_zero_sample_keeps_the_native_complete_frame() {
        let bytes = include_bytes!(
            "../../../../../tests/fixtures/p004-terminal-sample/public-45-frames.mp4"
        );
        let inspection =
            inspect::inspect_presentation(Cursor::new(bytes), bytes.len() as u64).unwrap();
        assert!(presentation(&inspection).is_err());
        let native = from_reader(Cursor::new(bytes), bytes.len() as u64).unwrap();
        assert_eq!(native.duration_millis, 375);
        assert_eq!(native.duration_millis_floor, 375);
        assert_eq!(
            native.frame_rate,
            crate::writer::footage::NativeFrameRate::integer(120)
        );
        for (offset, value) in [(16, 2_u32), (20, 1), (8, 43)] {
            let changed = mutate_atom_path(
                bytes,
                &[*b"moov", *b"trak", *b"mdia", *b"minf", *b"stbl", *b"stts"],
                &|payload| {
                    let mut payload = payload.to_vec();
                    payload[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
                    payload
                },
            );
            assert!(from_reader(Cursor::new(&changed), changed.len() as u64).is_err());
        }
        let shifted = mutate_atom_path(
            bytes,
            &[*b"moov", *b"trak", *b"edts", *b"elst"],
            &|payload| {
                let mut payload = payload.to_vec();
                payload[12..16].copy_from_slice(&128_u32.to_be_bytes());
                payload
            },
        );
        assert!(from_reader(Cursor::new(&shifted), shifted.len() as u64).is_err());
        for codec in [*b"hvc1", *b"apch", *b"avc3"] {
            let changed = mutate_atom_path(
                bytes,
                &[*b"moov", *b"trak", *b"mdia", *b"minf", *b"stbl", *b"stsd"],
                &|payload| {
                    let mut payload = payload.to_vec();
                    assert_eq!(&payload[12..16], b"avc1");
                    payload[12..16].copy_from_slice(&codec);
                    payload
                },
            );
            assert!(terminal_sample::from_original(&changed).unwrap().is_none());
        }
        let sample = terminal_sample::from_original(bytes).unwrap().unwrap();
        assert!(sample.matches(45, 128, 1, 15360, 5632));
        assert!(!sample.matches(44, 128, 1, 15360, 5632));
        assert!(!sample.matches(45, 128, 1, 15360, 5631));
        assert!(!sample.matches(45, 128, 2, 15360, 5632));
    }

    fn native_media() -> MediaInspection {
        inspect::inspect_presentation(Cursor::new(NATIVE_MEDIA), NATIVE_MEDIA.len() as u64).unwrap()
    }

    #[test]
    fn mp4_native_source_uses_presentation_not_decode_order() {
        let mut media = native_media();
        media.packets.reverse();
        let result = presentation(&media).unwrap();
        assert_eq!(result.dimensions, [1920, 1080]);
        assert_eq!(result.duration_millis, 5000);
        assert_eq!(result.duration_millis_floor, 5000);
        assert_eq!(
            result.frame_rate,
            crate::writer::footage::NativeFrameRate::integer(24)
        );
        assert_eq!(result.audio_sample_rate, 0.0);
    }

    #[test]
    fn mp4_b_frames_and_nonzero_native_edit_origin_use_the_displayed_clock() {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../premiere_file/tests/fixtures/video-30fps-10s.mp4"
        ));
        let raw = inspect::inspect(Cursor::new(bytes), bytes.len() as u64, true).unwrap();
        assert_ne!(raw.packets[0].pts, Some(0));
        assert!(presentation(&raw).is_err());
        let displayed =
            inspect::inspect_presentation(Cursor::new(bytes), bytes.len() as u64).unwrap();
        assert!(
            displayed
                .packets
                .iter()
                .any(|packet| packet.pts != packet.dts)
        );
        let result = presentation(&displayed).unwrap();
        assert_eq!(result.duration_millis, 10_000);
        assert_eq!(
            result.frame_rate,
            crate::writer::footage::NativeFrameRate::integer(30)
        );
    }

    #[test]
    fn mp4_geometry_preserves_unspecified_square_and_identity_states() {
        let mut media = native_media();
        for aspect in [None, Some([1, 1]), Some([2, 2])] {
            media.streams[0].sample_aspect_ratio = aspect;
            for matrix in [None, Some([65536, 0, 0, 0, 65536, 0, 0, 0, 1073741824])] {
                media.streams[0].display_matrix = matrix;
                assert!(presentation(&media).is_ok());
            }
        }
        media.streams[0].sample_aspect_ratio = Some([4, 3]);
        assert!(matches!(
            presentation(&media),
            Err(MetadataError::Unsupported(_))
        ));
        media.streams[0].sample_aspect_ratio = None;
        media.streams[0].display_matrix = Some([0, 65536, 0, -65536, 0, 0, 0, 0, 1073741824]);
        assert!(matches!(
            presentation(&media),
            Err(MetadataError::Unsupported(_))
        ));
    }

    #[test]
    fn mp4_irregular_duplicate_delayed_or_inconsistent_presentation_is_not_silently_retimed() {
        let original = native_media();
        for alteration in 0..4 {
            let mut media = original.clone();
            match alteration {
                0 => media.packets[1].pts = media.packets[0].pts,
                1 => media.packets[1].duration += 1,
                2 => media
                    .packets
                    .iter_mut()
                    .for_each(|packet| packet.pts = packet.pts.map(|pts| pts + 1)),
                _ => media.streams[0].duration += 1,
            }
            assert!(matches!(
                presentation(&media),
                Err(MetadataError::Unsupported(_))
            ));
        }
    }

    // Mutate parsed atoms, not byte-pattern matches. This fixture's moov follows
    // mdat, so adding metadata cannot invalidate the existing sample offsets.
    fn mutate_atom_path(
        bytes: &[u8],
        path: &[[u8; 4]],
        change: &impl Fn(&[u8]) -> Vec<u8>,
    ) -> Vec<u8> {
        if path.is_empty() {
            return change(bytes);
        }
        let mut budget = super::super::AtomBudget {
            remaining: usize::MAX,
        };
        let atoms = super::super::atoms(bytes, &mut budget).unwrap();
        assert_eq!(atoms.iter().filter(|atom| atom.kind == path[0]).count(), 1);
        if path[0] == *b"moov" {
            let media = atoms.iter().position(|atom| atom.kind == *b"mdat").unwrap();
            let movie = atoms.iter().position(|atom| atom.kind == *b"moov").unwrap();
            assert!(
                media < movie,
                "metadata mutation must preserve sample offsets"
            );
        }
        let mut output = Vec::new();
        for atom in atoms {
            let payload = if atom.kind == path[0] {
                mutate_atom_path(atom.payload, &path[1..], change)
            } else {
                atom.payload.to_vec()
            };
            output.extend_from_slice(&u32::try_from(payload.len() + 8).unwrap().to_be_bytes());
            output.extend_from_slice(&atom.kind);
            output.extend_from_slice(&payload);
        }
        output
    }

    #[test]
    fn mp4_original_dwell_reverse_and_nonunit_edit_rates_are_not_ordinary_video() {
        for version in [0, 1] {
            for rate in [0_u32, 0x0000_8000, 0x0001_0000, 0x0002_0000, 0xffff_0000] {
                let bytes = mutate_atom_path(
                    NATIVE_MEDIA,
                    &[*b"moov", *b"trak", *b"edts", *b"elst"],
                    &|list| {
                        assert_eq!(list.len(), 20);
                        if version == 0 {
                            let mut list = list.to_vec();
                            list[16..20].copy_from_slice(&rate.to_be_bytes());
                            list
                        } else {
                            let mut output = vec![1, 0, 0, 0];
                            output.extend_from_slice(&1_u32.to_be_bytes());
                            output.extend_from_slice(
                                &u64::from(u32::from_be_bytes(list[8..12].try_into().unwrap()))
                                    .to_be_bytes(),
                            );
                            output.extend_from_slice(
                                &i64::from(i32::from_be_bytes(list[12..16].try_into().unwrap()))
                                    .to_be_bytes(),
                            );
                            output.extend_from_slice(&rate.to_be_bytes());
                            output
                        }
                    },
                );
                let result = from_reader(Cursor::new(&bytes), bytes.len() as u64);
                if rate == 0x0001_0000 {
                    assert!(result.is_ok(), "version {version}: {result:?}");
                } else {
                    assert!(
                        matches!(
                            result,
                            Err(MetadataReadError::Profile(MetadataError::Unsupported(_)))
                        ),
                        "version {version}, rate {rate:#x}"
                    );
                }
            }
        }
    }

    #[test]
    fn mp4_original_clean_aperture_must_cover_the_full_uncropped_source() {
        for width in [1920_u32, 1800] {
            let bytes = mutate_atom_path(
                NATIVE_MEDIA,
                &[*b"moov", *b"trak", *b"mdia", *b"minf", *b"stbl", *b"stsd"],
                &|description| {
                    let mut description = description.to_vec();
                    let old_size = u32::from_be_bytes(description[8..12].try_into().unwrap());
                    description[8..12].copy_from_slice(&(old_size + 40).to_be_bytes());
                    description.extend_from_slice(&40_u32.to_be_bytes());
                    description.extend_from_slice(b"clap");
                    for value in [width, 1, 1080, 1, 0, 1, 0, 1] {
                        description.extend_from_slice(&value.to_be_bytes());
                    }
                    description
                },
            );
            let result = from_reader(Cursor::new(&bytes), bytes.len() as u64);
            if width == 1920 {
                assert_eq!(result.unwrap().dimensions, [1920, 1080]);
            } else {
                assert!(matches!(
                    result,
                    Err(MetadataReadError::Profile(MetadataError::Unsupported(_)))
                ));
            }
        }
    }

    #[test]
    fn mp4_unsupported_codec_and_truncated_container_are_rejected() {
        let mut media = native_media();
        media.streams[0].codec_tag = *b"hvc1";
        assert!(matches!(
            presentation(&media),
            Err(MetadataError::Unsupported(_))
        ));
        assert!(from_reader(Cursor::new(&NATIVE_MEDIA[..20]), 20).is_err());
    }
}
