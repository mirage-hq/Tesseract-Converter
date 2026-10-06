//! Container and codec admission, tested on real encoder output.
//!
//! The HEVC and VP9 samples under `tests/fixtures` were made with FFmpeg 9.0.1;
//! the video-formats manifest case pins the HEVC sample:
//!
//! ```sh
//! SOURCE="color=c=0x00a000:s=1920x1080:r=30,format=rgb24"
//! SQUARE="color=c=white:s=120x120:r=30,format=rgb24"
//! TAGS="setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv"
//! ffmpeg -f lavfi -i "$SOURCE" -f lavfi -i "$SQUARE" -filter_complex \
//!   "[0][1]overlay=x='mod(n*24,1800)':y=480,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,$TAGS" \
//!   -frames:v 60 -an -c:v libx265 -preset medium -crf 30 -tag:v hvc1 \
//!   -x265-params log-level=error:info=0:keyint=30:min-keyint=30 \
//!   -video_track_timescale 30 -movie_timescale 30 -map_metadata -1 \
//!   -fflags +bitexact -flags:v +bitexact feature_video_formats_hevc.mp4
//! ffmpeg -f lavfi -i color=c=black:s=64x64:r=30 -frames:v 1 -an -c:v libvpx-vp9 -b:v 20k \
//!   -video_track_timescale 30 -movie_timescale 30 -map_metadata -1 \
//!   -fflags +bitexact -flags:v +bitexact video-vp9-64x64.mp4
//! ```
//!
//! Each `hvcC` constant below is the configuration record of a two-frame
//! 1920x1080 libx265 4.3 encode of the HEVC source with `-x265-params info=0`,
//! changed only as its comment says. `REFERENCE_SETS` instead comes from the
//! macOS 26.5.1 hardware encoder, `-c:v hevc_videotoolbox -profile:v main
//! -b:v 2M`, and the `MAIN10_*` records from the same encoder with
//! `-profile:v main10 -pix_fmt p010le` on a `format=p010le,setparams=...`
//! source tagged BT.2020 with the HLG (`arib-std-b67`) or PQ (`smpte2084`)
//! transfer, the encode of the HDR pass-through fixture
//! `feature_hdr_hlg_hvc1.mp4` at 1920x1080. A `_FULL_RANGE` twin repeats its
//! base encode with `out_range=pc` and `range=pc`; FFmpeg's `trace_headers`
//! shows that each pair differs only in `video_full_range_flag`, so the twin
//! proves that the VUI read after the captured branch stays aligned. Tests
//! splice one record into the HEVC sample, whose `moov` follows `mdat`, so no
//! chunk offsets move. No local encoder writes Dolby Vision, so its
//! configuration records are written by the spec layout in `dolby_vision`.
//!
//! No local encoder (x265 4.3, VideoToolbox) writes sub-layer profile or level
//! fields, PCM, predicted reference picture sets, or long-term reference
//! pictures, so the reader rejects them; their tests set the flag in a
//! captured record at the bit position that `trace_headers` reports.

#[cfg(feature = "ffmpeg-library")]
use crate::{
    format::FrameRate,
    schema::{HdrProfile, VideoCodec},
};
use crate::{
    media::{inspect_video_media, VideoMedia},
    video_format::validate_video_file_name,
};
use std::{io::Cursor, path::Path};

#[cfg(feature = "ffmpeg-library")]
const H264_MP4: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");
#[cfg(feature = "ffmpeg-library")]
const H264_MOV: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mov");
/// 8-bit H.264 whose VUI declares BT.2020 primaries, PQ and the BT.2020 matrix.
#[cfg(feature = "ffmpeg-library")]
const H264_HDR_TAGS: &[u8] = include_bytes!("../../tests/fixtures/video-hdr-tags.mp4");
const HEVC: &[u8] = include_bytes!("../../tests/fixtures/feature_video_formats_hevc.mp4");
/// The HDR pass-through fixture: VideoToolbox Main 10 HLG at 320x180 with a
/// QuickTime timecode track (`-timecode 00:00:00:00 -f mov`).
#[cfg(feature = "ffmpeg-library")]
const HLG_MOV: &[u8] = include_bytes!("../../tests/fixtures/feature_hdr_hlg_hvc1.mov");
#[cfg(feature = "ffmpeg-library")]
const VP9: &[u8] = include_bytes!("../../tests/fixtures/video-vp9-64x64.mp4");
const STSD: &[&[u8; 4]] = &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"];
#[cfg(feature = "ffmpeg-library")]
const SECOND: i64 = crate::schema::TICKS;

/// `-pix_fmt yuv420p10le`: Main 10 profile, 10-bit luma and chroma, BT.709.
const MAIN10: &str = "01022000000090000000000078f000fcfdfafa00000f03a00001001840010c01ffff022000000300900000030000030078959809a10001002f420101022000000300900000030000030078a003c08010e4d96566924caf016a02020208000003000800000300f040a2000100074401c172b46240";
/// VideoToolbox Main 10 with BT.2020 primaries, HLG transfer and BT.2020nc matrix.
#[cfg(feature = "ffmpeg-library")]
const MAIN10_HLG: &str = "010220000000b0000000000078f000fcfdfafa00000f03a00001001840010c01ffff022000000300b000000300000300781b0240a10001002b420101022000000300b00000030000030078a003c0801107cad881bb916452ffcb9fc4feb016a122412010a2000100084401c072e1905324";
/// The same encode with the PQ transfer.
#[cfg(feature = "ffmpeg-library")]
const MAIN10_PQ: &str = "010220000000b0000000000078f000fcfdfafa00000f03a00001001840010c01ffff022000000300b000000300000300781b0240a10001002b420101022000000300b00000030000030078a003c0801107cad881bb916452ffcb9fc4feb016a122012010a2000100084401c072e3414c90";
/// `-pix_fmt yuv422p`: Range Extensions profile, 8-bit 4:2:2.
const REXT_422: &str = "0104080000009d080000000078f000fcfef8f800000f03a00001001740010c01ffff0408000003009d08000003000078959809a10001002c4201010408000003009d08000003000078b003c08010e596566924caf016a020202080000003008000000f04a2000100074401c172b46240";
/// `out_range=pc` and `range=pc`: VUI `video_full_range_flag` set.
const FULL_RANGE: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a10001002d420101016000000300900000030000030078a003c08010e596566924caf016e020202080000003008000000f04a2000100074401c172b46240";
/// BT.2020 primaries and matrix with SMPTE ST 2084 (PQ) transfer in the VUI.
const PQ: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a10001002d420101016000000300900000030000030078a003c08010e596566924caf016a122012080000003008000000f04a2000100074401c172b46240";
/// Without the `setparams` tags and the `scale` colour matrix: VUI signal
/// type present (limited range) with no colour description, which declares
/// nothing.
#[cfg(feature = "ffmpeg-library")]
const NO_COLOUR: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a10001002a420101016000000300900000030000030078a003c08010e596566924caf0168080000003008000000f04a2000100074401c172b46240";
/// `setsar=2/1`: VUI aspect_ratio_idc 16.
#[cfg(feature = "ffmpeg-library")]
const SAR_2_1: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a10001002d420101016000000300900000030000030078a003c08010e596566924caf106a020202080000003008000000f04a2000100074401c172b46240";
/// `min-cu-size=16`: coded 1920x1088 with a bottom conformance window.
#[cfg(feature = "ffmpeg-library")]
const CROPPED: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a10001002e420101016000000300900000030000030078a003c0801107cb965664e4caf016a020202080000003008000000f04a2000100074401c172b46240";
/// `interlace=tff` with `field_mode=tff`: interlaced source, field-timed VUI.
#[cfg(feature = "ffmpeg-library")]
const INTERLACED: &str = "01016000000040000000000078f000fcfdf8f800000f04a00001001840010c01ffff016000000300400000030000030078959809a10001002d420101016000000300400000030000030078a003c08010e596566924caf016a020202680000003008000000f04a2000100074401c172b4624027000100064e0181010f80";
/// `temporal-layers=3`: `sps_max_sub_layers_minus1` 2 with no sub-layer
/// profile or level fields, and ordering information for each sub-layer.
const SUB_LAYERS: &str = "01016000000090000000000078f000fcfdf8f800001b03a00001001e40010c04ffff01600000030090000003000003007800009594aca5650240a1000100334201040160000003009000000300000300780000a003c08010e5965652b295964932bc05a808080820000003002000000303c1a2000100074401c172b46240";
#[cfg(feature = "ffmpeg-library")]
const SUB_LAYERS_FULL_RANGE: &str = "01016000000090000000000078f000fcfdf8f800001b03a00001001e40010c04ffff01600000030090000003000003007800009594aca5650240a1000100334201040160000003009000000300000300780000a003c08010e5965652b295964932bc05b808080820000003002000000303c1a2000100074401c172b46240";
/// `scaling-list=<file>`, an HM-style file whose INTRA matrices are the ramp
/// 16, 17, ..., INTER matrices the ramp 17, 18, ..., chroma V matrices flat 16,
/// and every DC 12: `scaling_list_data` codes default, copied, and DPCM
/// matrices, with DC coefficients for 16x16 and 32x32.
#[cfg(feature = "ffmpeg-library")]
const SCALING_LISTS: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a1000101e0420101016000000300900000030000030078a003c08010e596566924f84041c71ce1439ce1439c71c413090838e39c28739c28738e3882610080f0f0f1e0b078f1e0e878f1e3c0903c78f1e3c0ac3c78f1e3c7819078f1e3c78f1e0641e3c78f1e3c0ac3c78f1e3c0903c78f1e0e878f1e0b078f0f0f081421fffffffffffffffe12080f0f0f1e0b078f1e0e878f1e3c0903c78f1e3c0ac3c78f1e3c7819078f1e3c78f1e0641e3c78f1e3c0ac3c78f1e3c0903c78f1e0e878f1e0b078f0f0f081091020203c3c3c782c1e3c783a1e3c78f0240f1e3c78f02b0f1e3c78f1e0641e3c78f1e3c7819078f1e3c78f02b0f1e3c78f0240f1e3c783a1e3c782c1e3c3c3c2051023fffffffffffffffc40a080f0f0f1e0b078f1e0e878f1e3c0903c78f1e3c0ac3c78f1e3c7819078f1e3c78f1e0641e3c78f1e3c0ac3c78f1e3c0903c78f1e0e878f1e0b078f0f0f081091020203c3c3c782c1e3c783a1e3c78f0240f1e3c78f02b0f1e3c78f1e0641e3c78f1e3c7819078f1e3c78f02b0f1e3c78f0240f1e3c783a1e3c782c1e3c3c3c211028203c3c3c782c1e3c783a1e3c78f0240f1e3c78f02b0f1e3c78f1e0641e3c78f1e3c7819078f1e3c78f02b0f1e3c78f0240f1e3c783a1e3c782c1e3c3c3c20af016a020202080000003008000000f04a2000100074401c172b46240";
#[cfg(feature = "ffmpeg-library")]
const SCALING_LISTS_FULL_RANGE: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a1000101e0420101016000000300900000030000030078a003c08010e596566924f84041c71ce1439ce1439c71c413090838e39c28739c28738e3882610080f0f0f1e0b078f1e0e878f1e3c0903c78f1e3c0ac3c78f1e3c7819078f1e3c78f1e0641e3c78f1e3c0ac3c78f1e3c0903c78f1e0e878f1e0b078f0f0f081421fffffffffffffffe12080f0f0f1e0b078f1e0e878f1e3c0903c78f1e3c0ac3c78f1e3c7819078f1e3c78f1e0641e3c78f1e3c0ac3c78f1e3c0903c78f1e0e878f1e0b078f0f0f081091020203c3c3c782c1e3c783a1e3c78f0240f1e3c78f02b0f1e3c78f1e0641e3c78f1e3c7819078f1e3c78f02b0f1e3c78f0240f1e3c783a1e3c782c1e3c3c3c2051023fffffffffffffffc40a080f0f0f1e0b078f1e0e878f1e3c0903c78f1e3c0ac3c78f1e3c7819078f1e3c78f1e0641e3c78f1e3c0ac3c78f1e3c0903c78f1e0e878f1e0b078f0f0f081091020203c3c3c782c1e3c783a1e3c78f0240f1e3c78f02b0f1e3c78f1e0641e3c78f1e3c7819078f1e3c78f02b0f1e3c78f0240f1e3c783a1e3c782c1e3c3c3c211028203c3c3c782c1e3c783a1e3c78f0240f1e3c78f02b0f1e3c78f1e0641e3c78f1e3c7819078f1e3c78f02b0f1e3c78f0240f1e3c783a1e3c782c1e3c3c3c20af016e020202080000003008000000f04a2000100074401c172b46240";
/// VideoToolbox: four explicitly coded short-term reference picture sets,
/// scaling lists enabled without data, ordering information for the highest
/// sub-layer only, and a bottom conformance window on a coded 1920x1088 picture.
const REFERENCE_SETS: &str = "010160000000b0000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300b000000300000300781b0240a10001002a420101016000000300b00000030000030078a003c0801107cb881bb916452ffcb9fc4feb016a02020201a2000100074401c072f05324";
#[cfg(feature = "ffmpeg-library")]
const REFERENCE_SETS_FULL_RANGE: &str = "010160000000b0000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300b000000300000300781b0240a10001002a420101016000000300b00000030000030078a003c0801107cb881bb916452ffcb9fc4feb016e02020201a2000100074401c072f05324";
/// `display-window=0,0,0,8`: a VUI default display window.
#[cfg(feature = "ffmpeg-library")]
const DISPLAY_WINDOW: &str = "01016000000090000000000078f000fcfdf8f800000f03a00001001840010c01ffff016000000300900000030000030078959809a10001002f420101016000000300900000030000030078a003c08010e596566924caf016a0202021e260000003002000000303c1a2000100074401c172b46240";
#[cfg(feature = "ffmpeg-library")]
const ACCEPTED_CODECS: &str = "conversion accepts H.264 (avc1), HEVC (hvc1) or Apple ProRes";
#[cfg(feature = "ffmpeg-library")]
const PRORES_4444_ALPHA: &[u8] = include_bytes!("../../tests/fixtures/video-prores4444-alpha.mov");
const ACCEPTED_CONTAINERS: &str = "conversion accepts MP4 or QuickTime MOV video";
#[cfg(feature = "ffmpeg-library")]
const PQ_COLOUR: &str = "BT.2020/PQ/BT.2020nc";
#[cfg(feature = "ffmpeg-library")]
const HLG_COLOUR: &str = "BT.2020/HLG/BT.2020nc";

fn inspect(bytes: &[u8]) -> crate::error::Result<VideoMedia> {
    inspect_video_media(Cursor::new(bytes), Cursor::new(bytes), bytes.len() as u64)
}

#[cfg(feature = "ffmpeg-library")]
fn codec(sample_entry: &[u8; 4]) -> VideoCodec {
    VideoCodec::from_sample_entry(*sample_entry, 24).unwrap()
}
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

/// Returns the offset and size of each box on `path` through plain containers.
fn box_path(bytes: &[u8], path: &[&[u8; 4]]) -> Vec<(usize, usize)> {
    let (mut offset, mut end) = (0, bytes.len());
    path.iter()
        .map(|kind| loop {
            assert!(
                offset + 8 <= end,
                "missing {:?} box",
                std::str::from_utf8(*kind)
            );
            let size = read_u32(bytes, offset) as usize;
            assert!(size >= 8 && offset + size <= end, "unsupported box size");
            if &bytes[offset + 4..offset + 8] != *kind {
                offset += size;
                continue;
            }
            let found = (offset, size);
            (offset, end) = (offset + 8, offset + size);
            break found;
        })
        .collect()
}

fn rejection(bytes: &[u8]) -> String {
    inspect(bytes).unwrap_err().to_string()
}

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

/// Offset of the one sample entry inside `stsd` (after its version and count).
fn sample_entry(bytes: &[u8]) -> usize {
    let (stsd, _) = *box_path(bytes, STSD).last().unwrap();
    assert_eq!(read_u32(bytes, stsd + 12), 1, "one sample entry");
    stsd + 16
}

#[cfg(feature = "ffmpeg-library")]
fn with_sample_entry_type(bytes: &[u8], kind: &[u8; 4]) -> Vec<u8> {
    let mut patched = bytes.to_vec();
    let entry = sample_entry(bytes);
    patched[entry + 4..entry + 8].copy_from_slice(kind);
    patched
}

/// Offsets and sizes of the sample entry's child boxes.
fn entry_children(bytes: &[u8]) -> Vec<(usize, usize, [u8; 4])> {
    let entry = sample_entry(bytes);
    let end = entry + read_u32(bytes, entry) as usize;
    let mut offset = entry + 8 + 78;
    let mut children = Vec::new();
    while offset < end {
        let size = read_u32(bytes, offset) as usize;
        children.push((
            offset,
            size,
            bytes[offset + 4..offset + 8].try_into().unwrap(),
        ));
        offset += size;
    }
    children
}

/// Replaces bytes `start..end` inside the sample entry and grows its parents.
fn splice_entry(bytes: &[u8], start: usize, end: usize, replacement: &[u8]) -> Vec<u8> {
    assert!(
        box_path(bytes, &[b"moov"])[0].0 > box_path(bytes, &[b"mdat"])[0].0,
        "moov follows mdat, so chunk offsets stay valid"
    );
    let delta = i64::try_from(replacement.len()).unwrap() - i64::try_from(end - start).unwrap();
    let parents: Vec<usize> = box_path(bytes, STSD)
        .into_iter()
        .map(|(offset, _)| offset)
        .chain([sample_entry(bytes)])
        .collect();
    let mut patched = [&bytes[..start], replacement, &bytes[end..]].concat();
    for offset in parents {
        let size = i64::from(read_u32(&patched, offset)) + delta;
        write_u32(&mut patched, offset, u32::try_from(size).unwrap());
    }
    patched
}

/// Offset and size of the HEVC sample's `hvcC` box.
fn hevc_configuration_box() -> (usize, usize) {
    let (offset, size, _) = entry_children(HEVC)
        .into_iter()
        .find(|(_, _, kind)| kind == b"hvcC")
        .unwrap();
    (offset, size)
}

/// The unedited `hvcC` payload of the HEVC sample.
fn hevc_configuration() -> Vec<u8> {
    let (offset, size) = hevc_configuration_box();
    HEVC[offset + 8..offset + size].to_vec()
}

/// The HLG fixture with its timecode track's `hdlr` handler type renamed.
#[cfg(feature = "ffmpeg-library")]
fn with_data_track_handler(bytes: &[u8], handler: &[u8; 4]) -> Vec<u8> {
    let hdlr = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, kind)| *kind == b"hdlr")
        // Box type, version and flags, pre-defined, then the handler type.
        .map(|(at, _)| at + 12)
        .find(|&at| &bytes[at..at + 4] == b"tmcd")
        .unwrap();
    let mut patched = bytes.to_vec();
    patched[hdlr..hdlr + 4].copy_from_slice(handler);
    patched
}

/// Replaces the HEVC sample's `hvcC` box with one that holds `record`.
fn with_hevc_configuration(record: &[u8]) -> Vec<u8> {
    let (offset, size) = hevc_configuration_box();
    let length = u32::try_from(record.len() + 8).unwrap().to_be_bytes();
    splice_entry(
        HEVC,
        offset,
        offset + size,
        &[&length, b"hvcC".as_slice(), record].concat(),
    )
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn hevc_configuration_with_native_arrays_above_64_kib_is_accepted() {
    let mut record = hevc_configuration();
    let first_unit_length = usize::from(u16::from_be_bytes([record[26], record[27]]));
    let unit = record[26..28 + first_unit_length].to_vec();
    let extra = 3000_u16;
    let original = u16::from_be_bytes([record[24], record[25]]);
    record.splice(
        28 + first_unit_length..28 + first_unit_length,
        unit.repeat(usize::from(extra)),
    );
    record[24..26].copy_from_slice(&(original + extra).to_be_bytes());
    assert!(record.len() > 64 * 1024);
    let inspected = inspect(&with_hevc_configuration(&record)).unwrap();
    assert_eq!(inspected.codec, VideoCodec::HevcMain);
}

/// Returns `record` with bit `position` of its sequence parameter set set.
/// Positions count like FFmpeg's `trace_headers`: from the start of the NAL
/// header, after emulation-prevention bytes are removed.
fn with_sps_bit_set(record: &[u8], position: usize) -> Vec<u8> {
    let mut output = record[..23].to_vec();
    let mut cursor = 23;
    for _ in 0..record[22] {
        let nal_type = record[cursor] & 0x3f;
        let count = u16::from_be_bytes([record[cursor + 1], record[cursor + 2]]);
        output.extend_from_slice(&record[cursor..cursor + 3]);
        cursor += 3;
        for _ in 0..count {
            let length = usize::from(u16::from_be_bytes([record[cursor], record[cursor + 1]]));
            let mut unit = record[cursor + 2..cursor + 2 + length].to_vec();
            cursor += 2 + length;
            if nal_type == 33 {
                let mut payload = without_emulation_prevention(&unit);
                let mask = 0x80 >> (position % 8);
                assert_eq!(payload[position / 8] & mask, 0, "SPS bit {position} is set");
                payload[position / 8] |= mask;
                unit = with_emulation_prevention(&payload);
            }
            output.extend_from_slice(&u16::try_from(unit.len()).unwrap().to_be_bytes());
            output.extend_from_slice(&unit);
        }
    }
    output
}

/// Removes each emulation-prevention byte, the `03` of `00 00 03`.
fn without_emulation_prevention(unit: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(unit.len());
    let mut zeros = 0;
    for &byte in unit {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        payload.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    payload
}

/// Inserts an emulation-prevention byte before `00`-`03` after two zero bytes.
fn with_emulation_prevention(payload: &[u8]) -> Vec<u8> {
    let mut unit = Vec::with_capacity(payload.len() + 4);
    let mut zeros = 0;
    for &byte in payload {
        if zeros >= 2 && byte <= 3 {
            unit.push(3);
            zeros = 0;
        }
        unit.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    unit
}

/// Returns the HEVC sample with its `colr` box declaring the given codes.
#[cfg(feature = "ffmpeg-library")]
fn with_colr(bytes: &[u8], primaries: u16, transfer: u16, matrix: u16) -> Vec<u8> {
    let (offset, size, kind) = entry_children(bytes)
        .into_iter()
        .find(|(_, _, kind)| kind == b"colr")
        .unwrap();
    assert_eq!((size, &kind), (19, b"colr"));
    let mut patched = bytes.to_vec();
    for (field, value) in [primaries, transfer, matrix].into_iter().enumerate() {
        let at = offset + 12 + 2 * field;
        patched[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    patched
}

fn with_full_range_colr(bytes: &[u8]) -> Vec<u8> {
    let (offset, size, _) = entry_children(bytes)
        .into_iter()
        .find(|(_, _, kind)| kind == b"colr")
        .unwrap();
    assert_eq!(size, 19);
    let mut bytes = bytes.to_vec();
    bytes[offset + 18] = 0x80;
    bytes
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn accepted_codecs_keep_their_sample_entry_and_exact_timing() {
    for (name, bytes, sample_entry, size, duration_ticks, colour) in [
        ("H.264 MP4", H264_MP4, b"avc1", (1920, 1080), SECOND, None),
        (
            "H.264 QuickTime",
            H264_MOV,
            b"avc1",
            (1920, 1080),
            SECOND,
            None,
        ),
        (
            "HEVC Main MP4",
            HEVC,
            b"hvc1",
            (1920, 1080),
            2 * SECOND,
            None,
        ),
        // The fixture's timecode track draws nothing and is ignored.
        (
            "HEVC Main 10 HLG QuickTime with a tmcd track",
            HLG_MOV,
            b"hvc1",
            (320, 180),
            2 * SECOND,
            Some(HLG_COLOUR),
        ),
    ] {
        let media = inspect(bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(media.codec, codec(sample_entry), "{name}");
        assert_eq!((media.width, media.height), size, "{name}");
        assert_eq!(
            media.timing.supported().unwrap().0,
            FrameRate::Fps30,
            "{name}"
        );
        assert_eq!(
            media.timing.supported().unwrap().1,
            duration_ticks,
            "{name}"
        );
        assert_eq!(
            media.colour.map(|colour| colour.to_string()).as_deref(),
            colour,
            "{name}"
        );
    }
    // Tracks are classified by their handler: nonvisual data handlers beside
    // the picture are ignored, subtitle and caption handlers reject because
    // conversion would drop visible text, and a second picture or sound
    // track still rejects.
    for handler in [b"tmcd", b"mebx", b"meta", b"data"] {
        let media = inspect(&with_data_track_handler(HLG_MOV, handler))
            .unwrap_or_else(|error| panic!("{}: {error}", String::from_utf8_lossy(handler)));
        assert_eq!(
            media.colour.map(|colour| colour.to_string()).as_deref(),
            Some(HLG_COLOUR)
        );
    }
    for handler in [b"sbtl", b"text", b"subt", b"clcp"] {
        let name = String::from_utf8_lossy(handler);
        assert_eq!(
            rejection(&with_data_track_handler(HLG_MOV, handler)),
            format!("unsupported conversion: subtitle or caption tracks ({name}) are unsupported"),
        );
    }
    assert_eq!(
        rejection(&with_data_track_handler(HLG_MOV, b"vide")),
        "unsupported conversion: source requires exactly one video stream"
    );
    // QuickTime (iPhone captures) ends the sample description with four zero
    // bytes after its children; other short remainders stay malformed.
    let entry = sample_entry(HEVC);
    let end = entry + read_u32(HEVC, entry) as usize;
    inspect(&splice_entry(HEVC, end, end, &[0; 4])).unwrap();
    assert_eq!(
        rejection(&splice_entry(HEVC, end, end, &[0, 0, 0, 1])),
        "unsupported conversion: invalid or excessive MP4 metadata boxes"
    );
}

/// The HEVC sample with `record` as its `hvcC` and an unspecified `colr` box,
/// so the record's VUI is the file's only colour declaration.
#[cfg(feature = "ffmpeg-library")]
fn hevc_vui(record: &str) -> Vec<u8> {
    with_colr(&with_hevc_configuration(&hex(record)), 2, 2, 2)
}

/// The sequence parameter set NAL unit of a captured `hvcC` record.
#[cfg(feature = "ffmpeg-library")]
fn sequence_parameter_set(record: &[u8]) -> &[u8] {
    let vps = 23 + 3 + 2;
    let sps_array = vps + usize::from(u16::from_be_bytes([record[vps - 2], record[vps - 1]]));
    assert_eq!(record[sps_array] & 0x3f, 33);
    let length = usize::from(u16::from_be_bytes([
        record[sps_array + 3],
        record[sps_array + 4],
    ]));
    &record[sps_array + 5..sps_array + 5 + length]
}

/// `record` with `other`'s sequence parameter set appended to its SPS array,
/// a stream whose two parameter sets share a format but may declare
/// different colours.
#[cfg(feature = "ffmpeg-library")]
fn with_second_sps(record: &str, other: &str) -> Vec<u8> {
    let (record, other) = (hex(record), hex(other));
    let vps = 23 + 3 + 2;
    let sps_array = vps + usize::from(u16::from_be_bytes([record[vps - 2], record[vps - 1]]));
    let first = sequence_parameter_set(&record);
    let end = sps_array + 5 + first.len();
    let second = sequence_parameter_set(&other);
    let mut spliced = record.clone();
    spliced.splice(
        end..end,
        [
            u16::try_from(second.len())
                .unwrap()
                .to_be_bytes()
                .as_slice(),
            second,
        ]
        .concat(),
    );
    spliced[sps_array + 1..sps_array + 3].copy_from_slice(&2_u16.to_be_bytes());
    spliced
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn non_bt709_colour_passes_through_once_per_file_or_rejects() {
    use HdrProfile::{Hlg10Bit, Pq10Bit};
    const BT709: &str = "BT.709/BT.709/BT.709";
    for (name, bytes, colour, profile) in [
        // BT.709 declared in the VUI and the colr box: the SDR default.
        ("8-bit BT.709", HEVC.to_vec(), None, None),
        (
            "10-bit BT.709",
            with_hevc_configuration(&hex(MAIN10)),
            None,
            None,
        ),
        // Unspecified in one place and BT.709 in the other declares BT.709.
        (
            "BT.709 VUI, unspecified colr",
            with_colr(HEVC, 2, 2, 2),
            None,
            None,
        ),
        (
            "unspecified VUI, BT.709 colr",
            with_colr(&with_hevc_configuration(&hex(NO_COLOUR)), 1, 1, 1),
            None,
            None,
        ),
        // An explicit declaration in the VUI, the colr box, or both. Only the
        // 10-bit BT.2020 HLG and PQ forms have a measured Premiere profile.
        ("8-bit PQ VUI", hevc_vui(PQ), Some(PQ_COLOUR), None),
        (
            "10-bit HLG VUI",
            hevc_vui(MAIN10_HLG),
            Some(HLG_COLOUR),
            Some(Hlg10Bit),
        ),
        (
            "10-bit PQ VUI",
            hevc_vui(MAIN10_PQ),
            Some(PQ_COLOUR),
            Some(Pq10Bit),
        ),
        (
            "H.264 PQ VUI",
            H264_HDR_TAGS.to_vec(),
            Some(PQ_COLOUR),
            None,
        ),
        (
            "P3 colr",
            with_colr(&with_hevc_configuration(&hex(NO_COLOUR)), 12, 1, 1),
            Some("P3/BT.709/BT.709"),
            None,
        ),
        (
            "PQ colr and VUI",
            with_colr(&with_hevc_configuration(&hex(PQ)), 9, 16, 9),
            Some(PQ_COLOUR),
            None,
        ),
        // Fields reconcile one by one: an unspecified transfer in the colr
        // box takes the VUI's PQ.
        (
            "PQ VUI, colr without a transfer",
            with_colr(&with_hevc_configuration(&hex(MAIN10_PQ)), 9, 2, 9),
            Some(PQ_COLOUR),
            Some(Pq10Bit),
        ),
        (
            "two HLG parameter sets",
            with_colr(
                &with_hevc_configuration(&with_second_sps(MAIN10_HLG, MAIN10_HLG)),
                2,
                2,
                2,
            ),
            Some(HLG_COLOUR),
            Some(Hlg10Bit),
        ),
        (
            "HLG fixture",
            HLG_MOV.to_vec(),
            Some(HLG_COLOUR),
            Some(Hlg10Bit),
        ),
    ] {
        let media = inspect(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            media.colour.map(|colour| colour.to_string()).as_deref(),
            colour,
            "{name}"
        );
        assert_eq!(media.hdr_profile(), profile, "{name}");
    }
    assert_eq!(
        inspect(&hevc_vui(MAIN10_HLG))
            .unwrap()
            .colour
            .unwrap()
            .passthrough_warning(),
        "video colour BT.2020/HLG/BT.2020nc passes through unchanged; display depends on the player"
    );
    let conflict = |first: &str, later: &str| {
        format!("media declares conflicting colour metadata {first} and {later}")
    };
    for (name, bytes, expected) in [
        (
            "BT.2020 colr with unmapped transfer over BT.709 VUI",
            with_colr(HEVC, 9, 14, 9),
            conflict("BT.2020/unspecified/BT.2020nc", BT709),
        ),
        // Explicit BT.709 is a declaration, not a default: it conflicts with
        // HDR or P3 in the other place, in either order.
        (
            "BT.709 colr over a PQ VUI",
            with_hevc_configuration(&hex(PQ)),
            conflict(BT709, PQ_COLOUR),
        ),
        (
            "BT.709 colr over an HLG VUI",
            with_hevc_configuration(&hex(MAIN10_HLG)),
            conflict(BT709, HLG_COLOUR),
        ),
        (
            "PQ colr over a BT.709 VUI",
            with_colr(HEVC, 9, 16, 9),
            conflict(PQ_COLOUR, BT709),
        ),
        (
            "P3 colr over a BT.709 VUI",
            with_colr(HEVC, 12, 1, 1),
            conflict("P3/BT.709/BT.709", BT709),
        ),
        (
            "BT.709 transfer in colr over an HLG VUI",
            with_colr(&with_hevc_configuration(&hex(MAIN10_HLG)), 9, 1, 9),
            conflict("BT.2020/BT.709/BT.2020nc", HLG_COLOUR),
        ),
        (
            "HLG colr over a PQ VUI",
            with_colr(&with_hevc_configuration(&hex(PQ)), 9, 18, 9),
            conflict(HLG_COLOUR, PQ_COLOUR),
        ),
        (
            "HLG and PQ parameter sets",
            with_colr(
                &with_hevc_configuration(&with_second_sps(MAIN10_HLG, MAIN10_PQ)),
                2,
                2,
                2,
            ),
            conflict(HLG_COLOUR, PQ_COLOUR),
        ),
        (
            "BT.709 and HLG parameter sets",
            with_colr(
                &with_hevc_configuration(&with_second_sps(MAIN10, MAIN10_HLG)),
                2,
                2,
                2,
            ),
            conflict(BT709, HLG_COLOUR),
        ),
    ] {
        assert_eq!(
            rejection(&bytes),
            format!("unsupported conversion: {expected}"),
            "{name}"
        );
    }
}

#[test]
fn container_colour_recovery_keeps_known_fields_and_export_admission() {
    use crate::media_metadata::{validate_color, validate_export_color, ColourDescription};

    for declaration in [(0, 0, 1), (u16::MAX, u16::MAX, 1), (5, 6, 6)] {
        let mut colour = validate_color(declaration.0, declaration.1, declaration.2).unwrap();
        assert!(colour.unwrap().passes_through());
        ColourDescription::merge(&mut colour, validate_color(1, 1, 1).unwrap()).unwrap();
        assert_eq!(colour.unwrap().codes(), (1, 1, 1));
        assert!(colour.unwrap().passthrough_warning().contains("unmapped"));
        assert!(validate_export_color(declaration.0, declaration.1, declaration.2).is_err());
    }
    for declaration in [(1, 13, 1), (9, 13, 9)] {
        assert!(validate_color(declaration.0, declaration.1, declaration.2).is_err());
    }
    assert!(validate_export_color(1, 13, 1).is_ok());
    assert!(validate_export_color(9, 13, 9).is_err());
    assert!(validate_color(2, 2, 2).unwrap().is_none());
    let mut colour = validate_color(0, 0, 1).unwrap();
    assert!(ColourDescription::merge(&mut colour, validate_color(9, 16, 9).unwrap()).is_err());
    // Missing declarations alone do not invent a measured HDR profile.
    assert_eq!(validate_color(0, 0, 0).unwrap().unwrap().codes(), (2, 2, 2));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn container_colour_recovery_preserves_native_write_video_bytes_and_clocks() {
    use tesseract_file::TesseractFile;

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let original = std::fs::read(fixtures.join("feature_linked_av_source.mp4")).unwrap();
    let baseline = inspect(&original).unwrap();
    for declaration in [(0, 0, 1), (u16::MAX, u16::MAX, 1)] {
        // Supplementary container-tag edit only; the native source, samples,
        // configuration and clocks remain the public linked A/V fixture's.
        let bytes = with_colr(&original, declaration.0, declaration.1, declaration.2);
        let inspected = inspect(&bytes).unwrap();
        assert_eq!(inspected.codec, baseline.codec);
        assert_eq!(inspected.bit_depth, 8);
        assert_eq!(inspected.timing.sample_count, baseline.timing.sample_count);
        assert_eq!(inspected.timing.timescale, baseline.timing.timescale);
        assert!(inspected.hdr_profile().is_none());
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("feature_linked_av_source.mp4"), &bytes).unwrap();
        let source = temp.path().join("linked.prproj");
        std::fs::copy(fixtures.join("feature_linked_av_strict.prproj"), &source).unwrap();
        let (native, _) = crate::format::PrProjectFile::load(&source).unwrap();
        assert_eq!(native.sequences[0].frame_rate, FrameRate::Fps30);
        let output = temp.path().join("converted");
        let omissions = crate::premiere_to_tesseract(
            &source,
            &output,
            Some("80acdd81-0a96-4677-b17f-b2ffe2dff738"),
            false,
        )
        .unwrap();
        let warnings: Vec<_> = omissions
            .iter()
            .filter(|note| note.reason.contains("unmapped video colour metadata"))
            .collect();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, crate::OmissionKind::Approximated);
        assert!(warnings[0]
            .reason
            .contains("original bytes retained without a colour transform"));
        let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
        let document = file.project_json().unwrap();
        let layers = document["composition"]["layers"].as_array().unwrap();
        assert_eq!(
            layers
                .iter()
                .filter(|layer| matches!(layer["type"].as_str(), Some("Video" | "Audio")))
                .count(),
            2
        );
        for kind in ["Video", "Audio"] {
            let layer = layers.iter().find(|layer| layer["type"] == kind).unwrap();
            assert_eq!(
                layer["sourceRange"],
                serde_json::json!({"start": 0, "duration": 5000})
            );
            assert_eq!(
                crate::test_support::layer_range(layer),
                &serde_json::json!({"start": 0, "duration": 5000})
            );
            let id = layer["source"]["assetId"].as_str().unwrap();
            assert_eq!(
                file.asset(id)
                    .unwrap()
                    .read_verified_bytes(bytes.len() as u64)
                    .unwrap(),
                bytes
            );
        }
        let picture = layers
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(picture["volume"], 0.0);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn container_colour_recovery_keeps_required_format_and_range_bounds() {
    let bytes = with_colr(HEVC, 0, 0, 1);
    assert!(rejection(&with_full_range_colr(&bytes)).contains("full-range container requires"));
    let mut dimensions = bytes.clone();
    let entry = sample_entry(&dimensions);
    dimensions[entry + 32..entry + 34].copy_from_slice(&1800_u16.to_be_bytes());
    assert!(rejection(&dimensions).contains("HEVC picture size"));
    let mut truncated = bytes;
    truncated.pop();
    assert!(inspect(&truncated).is_err());
    let full = with_colr(
        &with_full_range_colr(&with_hevc_configuration(&hex(FULL_RANGE))),
        0,
        0,
        1,
    );
    assert!(inspect(&full).is_ok());
}

/// A `DolbyVisionConfigurationRecord` for the given profile, level 6, an RPU,
/// no enhancement layer, a base layer and the given compatibility ID.
#[cfg(feature = "ffmpeg-library")]
fn dolby_vision(profile: u8, el_present: bool, compatibility_id: u8) -> [u8; 24] {
    let mut record = [0; 24];
    record[0] = 1;
    record[2] = profile << 1;
    record[3] = (6 << 3) | (1 << 2) | (u8::from(el_present) << 1) | 1;
    record[4] = compatibility_id << 4;
    record
}

/// The HEVC sample renamed to `entry` with a Dolby Vision configuration box
/// of type `kind` after its `hvcC`.
#[cfg(feature = "ffmpeg-library")]
fn with_dolby_vision(entry: &[u8; 4], kind: &[u8; 4], record: &[u8]) -> Vec<u8> {
    let (offset, size) = hevc_configuration_box();
    let length = u32::try_from(record.len() + 8).unwrap().to_be_bytes();
    let end = offset + size;
    let spliced = splice_entry(HEVC, end, end, &[&length, kind.as_slice(), record].concat());
    with_sample_entry_type(&spliced, entry)
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn dolby_vision_records_on_hvc1_pass_through_and_dolby_entries_reject() {
    // Profile 8.4, the HLG-compatible form, and 8.1 (HDR10-compatible). An
    // iPhone writes profile 8.4 as `01 00 10 35 40 ...` in a `dvvC` box on a
    // plain `hvc1` entry, the record that
    // `dolby_vision(8, false, 4)` reproduces.
    assert_eq!(
        &dolby_vision(8, false, 4)[..5],
        &[0x01, 0x00, 0x10, 0x35, 0x40]
    );
    for (kind, compatibility_id) in [(b"dvvC", 4), (b"dvvC", 1), (b"dvcC", 4)] {
        let media = inspect(&with_dolby_vision(
            b"hvc1",
            kind,
            &dolby_vision(8, false, compatibility_id),
        ))
        .unwrap_or_else(|error| panic!("{}: {error}", String::from_utf8_lossy(kind)));
        assert_eq!(media.codec, VideoCodec::HevcMain);
        // Premiere saves the HEVC family code for every HEVC master.
        assert_eq!(media.codec.codec_type(), "1212503619");
        assert_eq!((media.width, media.height), (1920, 1080));
    }
    let record = dolby_vision(8, false, 4);
    for (name, bytes, expected) in [
        // The renderers map neither Dolby Vision entry to their HEVC decoder,
        // even with a base layer that would pass on hvc1.
        (
            "dvh1 entry",
            with_dolby_vision(b"dvh1", b"dvvC", &record),
            "Dolby Vision dvh1 sample entries are not decodable by the Tesseract engine; hvc1 with a Dolby Vision record converts",
        ),
        (
            "dvhe entry",
            with_dolby_vision(b"dvhe", b"dvvC", &record),
            "Dolby Vision dvhe sample entries are not decodable by the Tesseract engine; hvc1 with a Dolby Vision record converts",
        ),
        (
            "enhancement layer",
            with_dolby_vision(b"hvc1", b"dvcC", &dolby_vision(7, true, 0)),
            "Dolby Vision profile 7 with an enhancement layer or without a base layer is unsupported",
        ),
        (
            "profile 5",
            with_dolby_vision(b"hvc1", b"dvcC", &dolby_vision(5, false, 0)),
            "Dolby Vision profile 5 has no cross-compatible base layer; it needs a Dolby Vision decoder",
        ),
        (
            "truncated record",
            with_dolby_vision(b"hvc1", b"dvvC", &record[..12]),
            "invalid Dolby Vision configuration size 12",
        ),
    ] {
        let error = inspect(&bytes).expect_err("unsupported Dolby Vision media");
        assert_eq!(error.to_string(), format!("unsupported conversion: {expected}"), "{name}");
        if matches!(name, "dvh1 entry" | "dvhe entry") {
            assert!(matches!(error, crate::error::BuildError::UnsupportedVideoCodec(_)), "{name}: {error}");
        } else {
            // Profile, enhancement-layer and malformed configuration failures
            // are not codec omissions and must retain strict admission.
            assert!(matches!(error, crate::error::BuildError::Unsupported(_)), "{name}: {error}");
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unsupported_sample_entries_reject_with_their_code() {
    for (bytes, code) in [
        // In-band parameter sets, which inspection cannot see.
        (with_sample_entry_type(H264_MP4, b"avc3"), "avc3"),
        (with_sample_entry_type(HEVC, b"hev1"), "hev1"),
        (VP9.to_vec(), "vp09"),
        // ProRes RAW has no Premiere-saved record and no Adobe decode proof.
        (with_sample_entry_type(H264_MOV, b"aprn"), "aprn"),
    ] {
        assert_eq!(
            rejection(&bytes),
            format!(
                "unsupported conversion: video codec \"{code}\" is unsupported; {ACCEPTED_CODECS}"
            )
        );
    }
}

/// Apple ProRes passes through to the Adobe-bound package: inspection
/// classifies the profile and, for a 32-bit 4444 entry, its alpha, from the
/// sample entry alone. Import admits 4444 through the native decoder and
/// retains the sibling-preserving codec rejection for other profiles.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn prores_admits_4444_for_native_import_and_keeps_other_profiles_as_candidates() {
    use crate::schema::{video_codec::ProResProfile, PrMediaKind};
    let video = inspect(PRORES_4444_ALPHA).unwrap();
    assert_eq!(
        video.codec,
        VideoCodec::ProRes {
            profile: ProResProfile::P4444,
            alpha: true
        }
    );
    assert!(video.codec.has_alpha());
    assert_eq!(video.bit_depth, 12);
    assert_eq!((video.width, video.height), (32, 32));
    assert_eq!(video.timing.sample_count, 3);
    assert_eq!(video.codec.codec_type(), "1634743400");
    // A 4:2:2 profile has no alpha whatever its entry says; the relabelled
    // H.264 MOV has a 24-bit entry and no parameter sets to read.
    let standard = inspect(&with_sample_entry_type(H264_MOV, b"apcn")).unwrap();
    assert_eq!(
        standard.codec,
        VideoCodec::ProRes {
            profile: ProResProfile::Standard,
            alpha: false
        }
    );
    assert_eq!(standard.bit_depth, 10);
    crate::media::inspect_media(
        PrMediaKind::Video {
            codec: None,
            hdr_profile: None,
        },
        Cursor::new(PRORES_4444_ALPHA),
        Cursor::new(PRORES_4444_ALPHA),
        PRORES_4444_ALPHA.len() as u64,
        None,
    )
    .unwrap();
    {
        let bytes = with_sample_entry_type(H264_MOV, b"apcn");
        let remedy = ", so prepare the source with tsrct-conv transcode";
        let bytes = bytes.as_slice();
        let error = crate::media::inspect_media(
            PrMediaKind::Video {
                codec: None,
                hdr_profile: None,
            },
            Cursor::new(bytes),
            Cursor::new(bytes),
            bytes.len() as u64,
            None,
        )
        .unwrap_err();
        assert!(
            matches!(error, crate::error::BuildError::UnsupportedVideoCodec(_)),
            "{error}"
        );
        let message = error.to_string();
        assert!(
            message.contains(
                "is not decodable by the web player; import accepts H.264 (avc1) or HEVC (hvc1)"
            ) && message.ends_with(remedy),
            "{error}"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn alpha_media_prores4444_rejects_unproved_full_range() {
    let bytes = include_bytes!("../../tests/fixtures/alpha-media/prores4444.mov");
    let entry = sample_entry(bytes);
    let end = entry + read_u32(bytes, entry) as usize;
    let colr = [
        19_u32.to_be_bytes().as_slice(),
        b"colrnclx",
        &[0, 1, 0, 1, 0, 1, 0x80],
    ]
    .concat();
    let bytes = splice_entry(bytes, end, end, &colr);
    assert!(rejection(&bytes).contains("full-range ProRes"));
}

#[test]
fn video_file_names_must_be_mp4_or_mov() {
    for name in ["clip.mp4", "clip.MOV", "clip.Mp4"] {
        validate_video_file_name(Path::new(name)).unwrap();
    }
    // `.m4v` is ISO BMFF too, but the renderer's audio preflight never resolves it.
    for (name, extension) in [("clip.m4v", "m4v"), ("clip.mkv", "mkv"), ("clip", "")] {
        assert_eq!(
            validate_video_file_name(Path::new(name))
                .unwrap_err()
                .to_string(),
            format!(
                "unsupported conversion: video file extension \"{extension}\" is unsupported; {ACCEPTED_CONTAINERS}"
            ),
            "{name}"
        );
    }
}

/// Makes a captured record claim HEVC Main 8-bit 4:2:0 in its header, so only
/// its sequence parameter set still declares the variant.
fn main_header(mut record: Vec<u8>) -> Vec<u8> {
    record[1] = 0x01;
    record[2..6].copy_from_slice(&0x6000_0000_u32.to_be_bytes());
    record[16] = 0xfd;
    record[17] = 0xf8;
    record[18] = 0xf8;
    record
}

#[test]
fn hevc_header_and_parameter_sets_must_declare_main_or_main_10_4_2_0() {
    let mut chroma_422 = hevc_configuration();
    chroma_422[16] = 0xfe;
    let mut luma_10_bit = hevc_configuration();
    luma_10_bit[17] = 0xfa;
    let mut twelve_bit = hevc_configuration();
    twelve_bit[17] = 0xfc;
    twelve_bit[18] = 0xfc;
    let mut ten_bit_header = hevc_configuration();
    ten_bit_header[17] = 0xfa;
    ten_bit_header[18] = 0xfa;
    for (record, expected) in [
        (
            hex(REXT_422),
            "HEVC profile 4 is unsupported; conversion accepts HEVC Main and Main 10",
        ),
        (
            chroma_422,
            "HEVC must be 8- or 10-bit 4:2:0; found chroma format 2 at 8/8 bits",
        ),
        (
            luma_10_bit,
            "HEVC must be 8- or 10-bit 4:2:0; found chroma format 1 at 10/8 bits",
        ),
        (
            twelve_bit,
            "HEVC must be 8- or 10-bit 4:2:0; found chroma format 1 at 12/12 bits",
        ),
        // Headers edited to claim another depth: the parameter sets must agree.
        (
            ten_bit_header,
            "HEVC sequence parameter set declares chroma format 1 at 8/8 bits, unlike its hvcC header",
        ),
        (
            main_header(hex(MAIN10)),
            "HEVC sequence parameter set declares chroma format 1 at 10/10 bits, unlike its hvcC header",
        ),
        (
            main_header(hex(REXT_422)),
            "HEVC sequence parameter set declares chroma format 2 at 8/8 bits, unlike its hvcC header",
        ),
    ] {
        let error = rejection(&with_hevc_configuration(&record));
        assert!(error.contains(expected), "{error}");
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn hevc_sequence_parameters_decide_range_color_aspect_scan_and_size() {
    for (record, expected) in [
        (SAR_2_1, "conflicting video pixel aspect ratio declarations"),
        (
            INTERLACED,
            "HEVC declares interlaced source pictures; interlaced video is unsupported",
        ),
        (
            DISPLAY_WINDOW,
            "HEVC default display window cropping is unsupported",
        ),
    ] {
        assert_eq!(
            rejection(&with_hevc_configuration(&hex(record))),
            format!("unsupported conversion: {expected}")
        );
    }
    // Matching non-square container and codec declarations are now admitted.
    let mut anamorphic = with_hevc_configuration(&hex(SAR_2_1));
    let spacing = anamorphic
        .windows(4)
        .position(|value| value == b"pasp")
        .unwrap()
        + 4;
    assert_eq!(&anamorphic[spacing..spacing + 8], &[0, 0, 0, 1, 0, 0, 0, 1]);
    anamorphic[spacing..spacing + 4].copy_from_slice(&2_u32.to_be_bytes());
    assert_eq!(inspect(&anamorphic).unwrap().codec, codec(b"hvc1"));

    // Declared progressive, the same pictures are still field-coded in the VUI.
    // The SPS NAL starts at 57; after `00 00 03` its tenth byte holds the
    // progressive and interlaced source flags.
    let mut field_coded = hex(INTERLACED);
    assert_eq!(field_coded[57 + 9], 0x40);
    field_coded[57 + 9] = 0x80;
    let error = rejection(&with_hevc_configuration(&field_coded));
    assert!(
        error.contains("field-coded or field-timed HEVC is unsupported"),
        "{error}"
    );
    // A coded 1920x1088 picture with a conformance window displays 1920x1080.
    let cropped = inspect(&with_hevc_configuration(&hex(CROPPED))).unwrap();
    assert_eq!(cropped.codec, codec(b"hvc1"));
    assert_eq!((cropped.width, cropped.height), (1920, 1080));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn captured_parameter_set_branches_keep_the_vui_read_aligned() {
    for (branch, record, full_range) in [
        ("sub-layers", SUB_LAYERS, SUB_LAYERS_FULL_RANGE),
        ("scaling list data", SCALING_LISTS, SCALING_LISTS_FULL_RANGE),
        (
            "explicit reference picture sets",
            REFERENCE_SETS,
            REFERENCE_SETS_FULL_RANGE,
        ),
    ] {
        let media = inspect(&with_hevc_configuration(&hex(record)))
            .unwrap_or_else(|error| panic!("{branch}: {error}"));
        assert_eq!(media.codec, codec(b"hvc1"), "{branch}");
        assert_eq!((media.width, media.height), (1920, 1080), "{branch}");
        // The twin differs only in video_full_range_flag, read after the branch.
        let twin = inspect(&with_full_range_colr(&with_hevc_configuration(&hex(
            full_range,
        ))))
        .unwrap();
        assert_eq!(twin.codec, media.codec, "{branch}");
        assert_eq!(twin.bit_depth, 8, "{branch}");
    }
}

#[test]
fn uncaptured_parameter_set_branches_reject() {
    let sub_layer_information = "HEVC sub-layer profile or level information (sub_layer_profile_present_flag, sub_layer_level_present_flag) is unsupported";
    for (record, position, flag, expected) in [
        (
            hex(SUB_LAYERS),
            120,
            "sub_layer_profile_present_flag[0]",
            sub_layer_information,
        ),
        (
            hex(SUB_LAYERS),
            121,
            "sub_layer_level_present_flag[0]",
            sub_layer_information,
        ),
        (
            hevc_configuration(),
            205,
            "pcm_enabled_flag",
            "HEVC PCM coding (pcm_enabled_flag) is unsupported",
        ),
        (
            hex(REFERENCE_SETS),
            230,
            "inter_ref_pic_set_prediction_flag of the second set",
            "HEVC predicted short-term reference picture sets (inter_ref_pic_set_prediction_flag) are unsupported",
        ),
        (
            hevc_configuration(),
            207,
            "long_term_ref_pics_present_flag",
            "HEVC long-term reference pictures (long_term_ref_pics_present_flag) are unsupported",
        ),
    ] {
        assert_eq!(
            rejection(&with_hevc_configuration(&with_sps_bit_set(
                &record, position
            ))),
            format!("unsupported conversion: {expected}"),
            "{flag}"
        );
    }
}

#[test]
fn layered_and_in_band_hevc_configurations_reject() {
    let original = hevc_configuration();
    // The first array holds the VPS; its first NAL unit follows the array
    // header and a two-byte length.
    assert_eq!(original[23] & 0x3f, 32);
    let vps = 23 + 3 + 2;
    let mut layered = original.clone();
    // vps_max_layers_minus1 occupies the low two bits of RBSP byte 0 and the
    // high four bits of RBSP byte 1, after the two-byte NAL header; set it to 1.
    assert_eq!(layered[vps + 2] & 0b11, 0);
    layered[vps + 3] = (layered[vps + 3] & 0x0f) | 0x10;
    let mut in_band = original.clone();
    // Clear array_completeness on the SPS array.
    let sps_array = vps + usize::from(u16::from_be_bytes([original[vps - 2], original[vps - 1]]));
    assert_eq!(original[sps_array] & 0x3f, 33);
    in_band[sps_array] &= 0x7f;
    for (bytes, expected) in [
        (
            with_hevc_configuration(&layered),
            "multi-layer HEVC (alpha or multiview) is unsupported",
        ),
        (
            with_hevc_configuration(&in_band),
            "HEVC sample entry must store complete type-33 parameter sets in hvcC",
        ),
    ] {
        let error = rejection(&bytes);
        assert!(error.contains(expected), "{error}");
    }
    // Renaming a child box keeps every size and offset valid.
    let (btrt, _, kind) = *entry_children(HEVC).last().unwrap();
    assert_eq!(&kind, b"btrt");
    for (child, expected) in [
        (b"dvwC", "Dolby Vision in a wrapper is unsupported"),
        (b"lhvC", "layered HEVC is unsupported"),
    ] {
        let mut renamed = HEVC.to_vec();
        renamed[btrt + 4..btrt + 8].copy_from_slice(child);
        assert_eq!(
            rejection(&renamed),
            format!("unsupported conversion: {expected}")
        );
    }
    let (hvcc, _) = hevc_configuration_box();
    let mut renamed = HEVC.to_vec();
    renamed[hvcc + 4..hvcc + 8].copy_from_slice(b"free");
    let error = rejection(&renamed);
    assert!(
        error.contains("HEVC sample entry must contain one hvcC configuration"),
        "{error}"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn an_interlaced_fiel_box_rejects_h264_and_hevc() {
    let fiel = |fields: u8| [&10_u32.to_be_bytes()[..], b"fiel", &[fields, 0]].concat();
    // The HEVC sample declares one progressive field.
    let (hevc_fiel, size, kind) = entry_children(HEVC)[1];
    assert_eq!((&kind, size), (b"fiel", 10));
    let mut hevc = HEVC.to_vec();
    hevc[hevc_fiel + 8] = 2;
    // The H.264 MOV sample has none, so append one after its last child box.
    let (last, last_size, _) = *entry_children(H264_MOV).last().unwrap();
    let end = last + last_size;
    inspect(&splice_entry(H264_MOV, end, end, &fiel(1))).unwrap();
    let h264 = splice_entry(H264_MOV, end, end, &fiel(2));
    for (codec, bytes) in [("HEVC", hevc), ("H.264", h264)] {
        assert_eq!(
            rejection(&bytes),
            "unsupported conversion: interlaced video is unsupported; the fiel box must declare one progressive field",
            "{codec}"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn only_an_identity_clean_aperture_is_admitted() {
    // Aperture width and height, then horizontal and vertical offsets, each a
    // numerator and a denominator.
    let clap = |words: &[u32]| {
        let payload: Vec<u8> = words.iter().flat_map(|word| word.to_be_bytes()).collect();
        let size = u32::try_from(payload.len() + 8).unwrap().to_be_bytes();
        [&size[..], b"clap", &payload].concat()
    };
    let whole = clap(&[1920, 1, 1080, 1, 0, 1, 0, 1]);
    let cropping = "MP4 clean-aperture cropping unsupported";
    let invalid = "invalid MP4 clean aperture";
    // Both 1920x1080 samples gain the box after their last child box.
    for (codec, bytes) in [("HEVC", HEVC), ("H.264 QuickTime", H264_MOV)] {
        let (last, last_size, _) = *entry_children(bytes).last().unwrap();
        let end = last + last_size;
        let with = |children: &[u8]| splice_entry(bytes, end, end, children);
        // iPhone captures declare the whole picture, then end the sample
        // entry with QuickTime's four zero bytes.
        for children in [
            [&whole[..], &[0; 4]].concat(),
            clap(&[3840, 2, 3240, 3, 0, 5, 0, 7]),
        ] {
            let media =
                inspect(&with(&children)).unwrap_or_else(|error| panic!("{codec}: {error}"));
            assert_eq!((media.width, media.height), (1920, 1080), "{codec}");
        }
        for (children, reason) in [
            (clap(&[1904, 1, 1080, 1, 0, 1, 0, 1]), cropping),
            (clap(&[1920, 1, 1072, 1, 0, 1, 0, 1]), cropping),
            // Half a pixel wider, which a truncating division reads as 1920.
            (clap(&[3841, 2, 1080, 1, 0, 1, 0, 1]), cropping),
            (clap(&[1920, 1, 1080, 1, 1, 2, 0, 1]), cropping),
            // A vertical offset of minus one pixel.
            (clap(&[1920, 1, 1080, 1, 0, 1, u32::MAX, 1]), cropping),
            // 1920 times 2236963 wraps around to 1664 in 32 bits.
            (clap(&[1664, 2_236_963, 1080, 1, 0, 1, 0, 1]), cropping),
            (clap(&[0, 0, 1080, 1, 0, 1, 0, 1]), invalid),
            (clap(&[1920, 1, 1080, 1, 0, 0, 0, 1]), invalid),
            (clap(&[1920, 1, 1080, 1, 0, 1, 0]), invalid),
            (clap(&[1920, 1, 1080, 1, 0, 1, 0, 1, 0]), invalid),
            (
                [&whole[..], &whole].concat(),
                "duplicate MP4 display metadata",
            ),
        ] {
            assert_eq!(
                inspect(&with(&children))
                    .map(drop)
                    .map_err(|error| error.to_string()),
                Err(format!("unsupported conversion: {reason}")),
                "{codec}: {children:?}"
            );
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn ambiguous_sample_descriptions_keep_the_h264_message() {
    // The MP4 parser reads the original bytes, while the metadata reader sees
    // the sample entry renamed, so the bounded box path finds no such entry.
    for (bytes, expected) in [
        (
            H264_MP4,
            "MP4 must contain one unambiguous AVC sample description",
        ),
        (
            HEVC,
            "MP4 must contain one unambiguous hvc1 sample description",
        ),
    ] {
        let renamed = with_sample_entry_type(bytes, b"free");
        let error = inspect_video_media(
            Cursor::new(bytes),
            Cursor::new(renamed.as_slice()),
            bytes.len() as u64,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(error, format!("unsupported conversion: {expected}"));
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn full_range_eight_bit_sdr_hevc_keeps_its_original_configuration() {
    let bytes = with_full_range_colr(&with_hevc_configuration(&hex(FULL_RANGE)));
    let video = inspect(&bytes).unwrap();
    assert_eq!(video.codec, VideoCodec::HevcMain);
    assert_eq!(video.bit_depth, 8);
    assert_eq!((video.width, video.height), (1920, 1080));
}

#[test]
fn full_range_rejects_ambiguous_flags_and_unproved_depth_or_colour() {
    assert!(
        rejection(&with_hevc_configuration(&hex(FULL_RANGE))).contains("conflicting full-range")
    );
    for bytes in [
        with_full_range_colr(&with_hevc_configuration(&hex(MAIN10))),
        with_full_range_colr(&with_hevc_configuration(&hex(PQ))),
    ] {
        assert!(
            inspect(&bytes).is_err(),
            "10-bit or HDR full range is unproved"
        );
    }
    for (depth, colour) in [
        (10, None),
        (8, crate::media_metadata::validate_color(9, 16, 9).unwrap()),
    ] {
        assert!(
            crate::media_metadata::validate_video_range(None, Some(true), depth, colour)
                .unwrap_err()
                .to_string()
                .contains("8-bit")
        );
    }
    let mut reserved = with_full_range_colr(&with_hevc_configuration(&hex(FULL_RANGE)));
    let (offset, _, _) = entry_children(&reserved)
        .into_iter()
        .find(|(_, _, kind)| kind == b"colr")
        .unwrap();
    reserved[offset + 18] |= 1;
    assert!(rejection(&reserved).contains("reserved MP4 color flags"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn full_range_h264_requires_the_bitstream_signal_even_when_nclx_declares_full() {
    // Six generated red64x64 frames at30fps, full-range8-bit4:2:0 BT.709.
    // ffmpeg9: color=red:s=64x64:r=30,format=rgb24,scale=out_color_matrix=bt709:out_range=pc,format=yuv420p;
    // libx264 veryfast CRF28, g30 bf0 threads1, color_rangepc/bt709,
    // movie/video timescale30, sixframes, noaudio, map_metadata-1.
    let full = include_bytes!("../../tests/fixtures/video-full-range-sdr.mp4");
    let file = inspect(full).unwrap();
    assert_eq!(
        (file.codec, file.bit_depth, file.width, file.height),
        (VideoCodec::H264, 8, 64, 64)
    );
    // The original fixture has no bitstream range signal. Add only nclxfull;
    // the player's decoder does not receive that container-only declaration.
    let entry = sample_entry(H264_MOV);
    let end = entry + read_u32(H264_MOV, entry) as usize;
    let colr = [
        19_u32.to_be_bytes().as_slice(),
        b"colrnclx",
        &1_u16.to_be_bytes(),
        &1_u16.to_be_bytes(),
        &1_u16.to_be_bytes(),
        &[0x80],
    ]
    .concat();
    let declared = splice_entry(H264_MOV, end, end, &colr);
    assert!(rejection(&declared).contains("explicit coherent full-range bitstream"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn bt709_10bit_source_profile_does_not_bypass_h264_transcode_preflight() {
    use fx_conv::{MediaRemediation, MediaStatus};

    // Supplementary synthetic SPS: the baseline fixture's dimensions/timing,
    // changed to High 4:2:2 with chroma_format_idc=2 and both bit depths=10.
    // The unchanged samples are not decodable with this header: preflight must
    // reject its format before decoding, not treat this as native render proof.
    let sps = with_emulation_prevention(&hex("677a0028b6cb403c0113f2e02200000002000000781e306540"));
    let (offset, size, _) = entry_children(H264_MOV)
        .into_iter()
        .find(|(_, _, kind)| kind == b"avcC")
        .unwrap();
    let mut avcc = hex("017a0028ffe1");
    avcc.extend_from_slice(&u16::try_from(sps.len()).unwrap().to_be_bytes());
    avcc.extend_from_slice(&sps);
    avcc.extend_from_slice(&hex("01000468ce0fc8"));
    let mut boxed = u32::try_from(avcc.len() + 8)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    boxed.extend_from_slice(b"avcC");
    boxed.extend_from_slice(&avcc);
    let media = splice_entry(H264_MOV, offset, offset + size, &boxed);
    let reason = "H.264 must be progressive 8-bit 4:2:0 with matching dimensions";
    assert!(inspect(&media).unwrap_err().to_string().contains(reason));

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("media")).unwrap();
    std::fs::write(root.path().join("media/source.mp4"), &media).unwrap();
    let records = roxmltree::Document::parse(include_str!(
        "../../tests/fixtures/bt709-10bit-source-streams.xml"
    ))
    .unwrap();
    for profile in records
        .descendants()
        .filter(|node| node.has_tag_name("OriginalColorSpace"))
    {
        let xml = include_str!("../../tests/fixtures/one-clip.xml").replace(
            "<VideoStream ObjectID=\"8\">",
            &format!(
                "<VideoStream ObjectID=\"8\"><OriginalColorSpace>{}</OriginalColorSpace>",
                profile.text().unwrap()
            ),
        );
        let path = root.path().join("source.prproj");
        crate::test_support::write_prproj(&path, &xml);
        let report = crate::Premiere
            .inspect_media(&path, &crate::PremiereImportOptions::default(), None)
            .unwrap();
        assert_eq!(report.media.len(), 1, "{report:?}");
        assert_eq!(
            report.media[0].status,
            MediaStatus::RequiresTranscode,
            "{report:?}"
        );
        assert_eq!(
            report.media[0].remediation,
            MediaRemediation::TranscodeCandidate
        );
        assert!(
            report.media[0].reason.as_deref().unwrap().contains(reason),
            "{report:?}"
        );
    }
}
