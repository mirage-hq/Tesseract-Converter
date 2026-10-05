//! Synthetic metadata regressions, not independent Adobe feature/render oracles.
//! The version-one extension and timecode description match the ignored Logo MOV
//! (SHA-256 025557fd52af5dae558f53a1c2294ec7625323d35750e61e4d75dfb3f54c9de6).

use super::*;

#[test]
fn pcm_timecode_reads_version_zero_pcm_in_both_byte_orders() {
    for codec in [*b"sowt", *b"twos"] {
        let movie = with_tracks(&[track(*b"soun", codec, &pcm_entry(0), 48_000, 144_000, 1)]);
        let metadata = quicktime(&movie).expect("valid version-zero PCM metadata");
        assert_eq!(metadata.audio_sample_rate, 48_000.0);
        assert_eq!(metadata.duration_millis, 3_000);
    }
}

#[test]
fn pcm_timecode_reads_version_one_pcm_in_both_byte_orders() {
    for codec in [*b"sowt", *b"twos"] {
        let movie = with_tracks(&[track(*b"soun", codec, &pcm_entry(1), 48_000, 144_000, 1)]);
        let metadata = quicktime(&movie).expect("valid version-one PCM metadata");
        assert_eq!(metadata.audio_sample_rate, 48_000.0);
    }
}

#[test]
fn pcm_timecode_preserves_logo_profile_with_ancillary_timecode() {
    let timecode = track(*b"tmcd", *b"tmcd", &timecode_entry(), 30, 1, 90);
    let video_only = quicktime(&with_tracks(&[])).unwrap();
    assert_eq!(
        quicktime(&with_tracks(std::slice::from_ref(&timecode)))
            .expect("recognized ancillary timecode"),
        video_only,
    );
    let audio = track(*b"soun", *b"sowt", &pcm_entry(1), 48_000, 144_000, 1);
    let metadata = quicktime(&with_tracks(&[audio, timecode]))
        .expect("Logo ProRes/PCM/timecode metadata must not require recompression");
    assert_eq!(metadata.audio_sample_rate, 48_000.0);
    assert_eq!(metadata.video_codec, *b"ap4h");
    assert_eq!(metadata.dimensions, [1080, 1080]);
    assert_eq!(metadata.duration_millis, 3_000);
    assert_eq!(metadata.frame_rate, video_only.frame_rate);
}

#[test]
fn pcm_timecode_rejects_truncated_or_inconsistent_pcm_extensions() {
    let entry = pcm_entry(1);
    let mut bad_frame = entry.clone();
    bad_frame[36..40].copy_from_slice(&2_u32.to_be_bytes());
    let mut bad_sample = entry.clone();
    bad_sample[40..44].copy_from_slice(&1_u32.to_be_bytes());
    let mut unknown_precision = entry.clone();
    unknown_precision[18..20].copy_from_slice(&24_u16.to_be_bytes());
    let mut unknown_layout = entry.clone();
    unknown_layout[56..60].copy_from_slice(&0x0079_0002_u32.to_be_bytes());
    for malformed in [
        unknown_precision,
        unknown_layout,
        entry[..27].to_vec(),
        entry[..43].to_vec(),
        bad_frame,
        bad_sample,
    ] {
        let movie = with_tracks(&[track(*b"soun", *b"sowt", &malformed, 48_000, 144_000, 1)]);
        assert!(
            quicktime(&movie).is_err(),
            "invalid PCM description was accepted"
        );
    }
}

#[test]
fn pcm_timecode_rejects_truncated_inconsistent_or_shifted_timecode() {
    let entry = timecode_entry();
    let mut zero_duration = entry.clone();
    zero_duration[20..24].fill(0);
    let mut inconsistent_fps = entry.clone();
    inconsistent_fps[24] = 24;
    let mut unknown_flags = entry.clone();
    unknown_flags[15] = 1;
    for malformed in [
        entry[..25].to_vec(),
        zero_duration,
        inconsistent_fps,
        unknown_flags,
    ] {
        let movie = with_tracks(&[track(*b"tmcd", *b"tmcd", &malformed, 30, 1, 90)]);
        assert!(
            quicktime(&movie).is_err(),
            "invalid timecode description was accepted"
        );
    }
    let mut shifted = track(*b"tmcd", *b"tmcd", &entry, 30, 1, 90);
    let mut edit = vec![0; 8];
    edit[4..8].copy_from_slice(&1_u32.to_be_bytes());
    edit.extend_from_slice(&3_000_u32.to_be_bytes());
    edit.extend_from_slice(&1_i32.to_be_bytes());
    edit.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    shifted = atom(
        *b"trak",
        &[
            shifted[8..].to_vec(),
            atom(*b"edts", &atom(*b"elst", &edit)),
        ]
        .concat(),
    );
    assert!(quicktime(&with_tracks(&[shifted])).is_err());
    let timecode = track(*b"tmcd", *b"tmcd", &entry, 30, 1, 90);
    assert!(quicktime(&with_tracks(&[timecode.clone(), timecode])).is_err());
}

#[test]
fn pcm_timecode_unknown_handlers_remain_unsupported() {
    let movie = with_tracks(&[track(*b"meta", *b"tmcd", &timecode_entry(), 30, 1, 90)]);
    assert!(matches!(
        quicktime(&movie),
        Err(MetadataError::Unsupported(_))
    ));
}

#[test]
fn movie_tick_ceiling_preserves_exact_media_clock_and_rejects_edits() {
    let movie = movie_with_video_timing(1_434, 15_360, 43, 512);
    let metadata = quicktime(&movie).expect("whole-track duration rounds upward to one movie tick");
    assert_eq!(metadata.duration_millis, 1_434);
    assert_eq!(metadata.duration_millis_floor, 1_433);
    assert_eq!(metadata.frame_rate, native_frame_rate(15_360, 512).unwrap());
    for duration in [1_433, 1_435] {
        assert!(quicktime(&movie_with_video_timing(duration, 15_360, 43, 512)).is_err());
    }
    let ftyp_size = u32::from_be_bytes(movie[..4].try_into().unwrap()) as usize;
    let moov = &movie[ftyp_size + 8..];
    let mvhd_size = u32::from_be_bytes(moov[..4].try_into().unwrap()) as usize;
    let trak = &moov[mvhd_size + 8..];
    for (duration, start) in [(1_434_u32, 0_i32), (1_433, 0), (1_434, 1)] {
        let edit = [
            [0_u8; 4],
            1_u32.to_be_bytes(),
            duration.to_be_bytes(),
            start.to_be_bytes(),
            0x0001_0000_u32.to_be_bytes(),
        ]
        .concat();
        let track = atom(
            *b"trak",
            &[trak, &atom(*b"edts", &atom(*b"elst", &edit))].concat(),
        );
        let edited = [
            movie[..ftyp_size].to_vec(),
            atom(*b"moov", &[&moov[..mvhd_size], &track].concat()),
        ]
        .concat();
        assert_eq!(quicktime(&edited).is_ok(), duration == 1_434 && start == 0);
    }
}

fn pcm_entry(version: u16) -> Vec<u8> {
    let mut entry = vec![0; 28];
    entry[6..8].copy_from_slice(&1_u16.to_be_bytes());
    entry[8..10].copy_from_slice(&version.to_be_bytes());
    entry[16..18].copy_from_slice(&2_u16.to_be_bytes());
    entry[18..20].copy_from_slice(&16_u16.to_be_bytes());
    entry[20..22].copy_from_slice(&(-1_i16).to_be_bytes());
    entry[24..28].copy_from_slice(&(48_000_u32 << 16).to_be_bytes());
    if version == 1 {
        for value in [1_u32, 2, 4, 2] {
            entry.extend_from_slice(&value.to_be_bytes());
        }
        // Valid stereo channel-layout child, as in the real source's 108-byte entry.
        let mut channels = vec![0; 56];
        channels[4..8].copy_from_slice(&0x0065_0002_u32.to_be_bytes());
        entry.extend_from_slice(&atom(*b"chan", &channels));
    }
    entry
}

fn timecode_entry() -> Vec<u8> {
    let mut entry = vec![0; 26];
    entry[6..8].copy_from_slice(&1_u16.to_be_bytes());
    entry[16..20].copy_from_slice(&30_u32.to_be_bytes());
    entry[20..24].copy_from_slice(&1_u32.to_be_bytes());
    entry[24] = 30;
    entry
}

fn track(
    handler_kind: [u8; 4],
    codec: [u8; 4],
    entry: &[u8],
    timescale: u32,
    count: u32,
    delta: u32,
) -> Vec<u8> {
    let mut header = track_header(3_000);
    header[76..84].fill(0);
    let description = [
        &[0_u8; 4],
        &1_u32.to_be_bytes(),
        atom(codec, entry).as_slice(),
    ]
    .concat();
    let table = atom(
        *b"stbl",
        &[
            atom(*b"stsd", &description),
            atom(*b"stts", &timing(count, delta)),
        ]
        .concat(),
    );
    let media = atom(
        *b"mdia",
        &[
            atom(*b"mdhd", &media_header(timescale, count * delta)),
            atom(*b"hdlr", &handler(handler_kind)),
            atom(*b"minf", &table),
        ]
        .concat(),
    );
    atom(*b"trak", &[atom(*b"tkhd", &header), media].concat())
}

fn with_tracks(tracks: &[Vec<u8>]) -> Vec<u8> {
    let mut movie = movie_with_video_timing(3_000, 30, 90, 1);
    // Replace only the synthetic video's codec and width, retaining valid timing.
    for offset in 0..movie.len() - 4 {
        if movie[offset..offset + 4] == *b"avc1" {
            movie[offset..offset + 4].copy_from_slice(b"ap4h");
        }
    }
    let width = (1920_u32 << 16).to_be_bytes();
    let offset = movie.windows(4).position(|value| value == width).unwrap();
    movie[offset..offset + 4].copy_from_slice(&(1080_u32 << 16).to_be_bytes());
    let codec = movie.windows(4).position(|value| value == b"ap4h").unwrap();
    movie[codec + 28..codec + 30].copy_from_slice(&1080_u16.to_be_bytes());
    let ftyp_size = u32::from_be_bytes(movie[..4].try_into().unwrap()) as usize;
    let mut payload = movie[ftyp_size + 8..].to_vec();
    for track in tracks {
        payload.extend_from_slice(track);
    }
    [movie[..ftyp_size].to_vec(), atom(*b"moov", &payload)].concat()
}
