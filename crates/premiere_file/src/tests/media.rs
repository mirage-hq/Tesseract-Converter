//! Media timing rules, tested on real container bytes without conversion.
//!
//! Seven six-frame H.264 samples under `tests/fixtures` provide the
//! non-30-fps container timing that the 30 fps samples lack. Invalid inputs are
//! small in-memory edits of stored samples; tests do not run the FFmpeg CLI.
//!
//! The samples were made offline with FFmpeg 7.1.5 and libx264. For another rate,
//! change the input rate, both time scales to its numerator, and the file name:
//!
//! ```sh
//! ffmpeg -hide_banner -loglevel error -y \
//!   -f lavfi -i color=c=black:s=1920x1080:r=24000/1001 \
//!   -frames:v 6 -an -c:v libx264 -preset veryfast -crf 28 \
//!   -pix_fmt yuv420p -g 30 -bf 0 -threads 1 \
//!   -video_track_timescale 24000 -movie_timescale 24000 -map_metadata -1 \
//!   crates/premiere_file/tests/fixtures/video-23.976fps.mp4
//! ```

use crate::format::FrameRate;
#[cfg(feature = "ffmpeg-library")]
use crate::media::{inspect_video_media, VideoMedia};
use std::path::Path;
#[cfg(feature = "ffmpeg-library")]
use std::{fs, io::Cursor};

#[cfg(feature = "ffmpeg-library")]
const MEDIA: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");
#[cfg(feature = "ffmpeg-library")]
const MEDIA_23_976: &[u8] = include_bytes!("../../tests/fixtures/video-23.976fps.mp4");
#[cfg(feature = "ffmpeg-library")]
const MEDIA_24: &[u8] = include_bytes!("../../tests/fixtures/video-24fps.mp4");
#[cfg(feature = "ffmpeg-library")]
const MVHD: &[&[u8; 4]] = &[b"moov", b"mvhd"];
#[cfg(feature = "ffmpeg-library")]
const TKHD: &[&[u8; 4]] = &[b"moov", b"trak", b"tkhd"];
#[cfg(feature = "ffmpeg-library")]
const ELST: &[&[u8; 4]] = &[b"moov", b"trak", b"edts", b"elst"];
#[cfg(feature = "ffmpeg-library")]
const MDHD: &[&[u8; 4]] = &[b"moov", b"trak", b"mdia", b"mdhd"];
#[cfg(feature = "ffmpeg-library")]
const STBL: [&[u8; 4]; 5] = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];

#[cfg(feature = "ffmpeg-library")]
fn inspect(bytes: &[u8]) -> crate::error::Result<VideoMedia> {
    inspect_video_media(Cursor::new(bytes), Cursor::new(bytes), bytes.len() as u64)
}

#[cfg(feature = "ffmpeg-library")]
fn stbl(child: &'static [u8; 4]) -> Vec<&'static [u8; 4]> {
    [STBL.as_slice(), &[child]].concat()
}

#[cfg(feature = "ffmpeg-library")]
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(feature = "ffmpeg-library")]
fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

/// Returns the offset and size of each box on `path` through plain containers.
#[cfg(feature = "ffmpeg-library")]
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

/// Edits one box payload and fixes parent sizes and shifted chunk offsets.
#[cfg(feature = "ffmpeg-library")]
fn patch_box(bytes: &[u8], path: &[&[u8; 4]], edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let boxes = box_path(bytes, path);
    let (leaf, size) = *boxes.last().unwrap();
    let mut payload = bytes[leaf + 8..leaf + size].to_vec();
    edit(&mut payload);
    let delta = i64::try_from(payload.len()).unwrap() - i64::try_from(size - 8).unwrap();
    let mut patched = [&bytes[..leaf + 8], &payload, &bytes[leaf + size..]].concat();
    for (offset, size) in boxes {
        let size = i64::try_from(size).unwrap() + delta;
        write_u32(&mut patched, offset, u32::try_from(size).unwrap());
    }
    if delta != 0
        && path[0] == b"moov"
        && box_path(bytes, &[b"moov"])[0].0 < box_path(bytes, &[b"mdat"])[0].0
    {
        let (stco, _) = *box_path(&patched, &stbl(b"stco")).last().unwrap();
        for index in 0..read_u32(&patched, stco + 12) as usize {
            let entry = stco + 16 + 4 * index;
            let moved = i64::from(read_u32(&patched, entry)) + delta;
            write_u32(&mut patched, entry, u32::try_from(moved).unwrap());
        }
    }
    patched
}

#[cfg(feature = "ffmpeg-library")]
fn payload<'a>(bytes: &'a [u8], path: &[&[u8; 4]]) -> &'a [u8] {
    let (offset, size) = *box_path(bytes, path).last().unwrap();
    &bytes[offset + 8..offset + size]
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn supported_media_frame_rates_have_exact_sample_durations() {
    use FrameRate::*;
    // Independent expected ticks; the new samples each contain six frames.
    for (name, frame_rate, duration_ticks) in [
        ("video-23.976fps.mp4", Fps24000Over1001, 63_567_504_000),
        ("video-24fps.mp4", Fps24, 63_504_000_000),
        ("video-25fps.mp4", Fps25, 60_963_840_000),
        ("video-29.97fps.mp4", Fps30000Over1001, 50_854_003_200),
        ("video-30fps.mp4", Fps30, 254_016_000_000),
        ("video-50fps.mp4", Fps50, 30_481_920_000),
        ("video-59.94fps.mp4", Fps60000Over1001, 25_427_001_600),
        ("video-60fps.mp4", Fps60, 25_401_600_000),
    ] {
        let bytes = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap();
        let media = inspect(&bytes).unwrap();
        assert_eq!(media.timing.supported().unwrap().0, frame_rate, "{name}");
        assert_eq!(
            media.timing.supported().unwrap().1,
            duration_ticks,
            "{name}"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn fractional_movie_duration_rounding_preserves_exact_sample_timing() {
    // Six frames take 250.25 ms at 23.976 fps and 250 ms at 24 fps. Change
    // only movie/track/edit durations; keep the encoded payload, sample
    // timestamps, and media duration intact. Every accepted edit keeps all
    // six frames, the Duration that Premiere saves.
    let fps_23_976 = (MEDIA_23_976, FrameRate::Fps24000Over1001, 63_567_504_000);
    let fps_24 = (MEDIA_24, FrameRate::Fps24, 63_504_000_000);
    for ((media, frame_rate, duration_ticks), timescale, duration, accepted) in [
        (fps_23_976, 24000, 6006, true),
        (fps_23_976, 1000, 250, true),
        (fps_23_976, 1000, 251, true),
        (fps_23_976, 600, 150, true),
        (fps_23_976, 600, 151, true),
        (fps_23_976, 1000, 249, true), // A shorter edit may end inside the last frame.
        (fps_23_976, 1000, 252, false), // A longer edit may only round up.
        (fps_23_976, 1000, 208, false), // A whole source frame trimmed away.
        (fps_23_976, 1, 1, false),     // One movie tick is too coarse to be harmless here.
        (fps_23_976, 1, 0, false),
        // IMG_2439's tail at its 600 Hz movie clock: 17 of the last frame's
        // 25 units trimmed; exactly one frame is too many.
        (fps_24, 600, 133, true),
        (fps_24, 600, 125, false),
    ] {
        for path in [MVHD, TKHD, ELST] {
            assert_eq!(payload(media, path)[0], 0, "version-0 layout");
        }
        let bytes = patch_box(media, MVHD, |mvhd| {
            write_u32(mvhd, 12, timescale);
            write_u32(mvhd, 16, duration);
        });
        let bytes = patch_box(&bytes, TKHD, |tkhd| write_u32(tkhd, 20, duration));
        let bytes = patch_box(&bytes, ELST, |elst| write_u32(elst, 8, duration));
        let result = inspect(&bytes);
        if accepted {
            let media = result.unwrap();
            assert_eq!(
                media.timing.supported().unwrap(),
                (frame_rate, duration_ticks),
                "{timescale}/{duration}"
            );
        } else {
            let message = result.unwrap_err().to_string();
            assert!(
                message.contains("edit list"),
                "{timescale}/{duration}: {message}"
            );
        }
    }
}

/// Retimes only the sample tables of the six-frame 24 fps fixture; picture
/// payloads are unchanged. Native-source proof lives in local evidence.
#[cfg(feature = "ffmpeg-library")]
fn unlisted_cfr_bytes(sample_duration: u32, timescale: u32) -> Vec<u8> {
    let duration = 6 * sample_duration;
    let bytes = patch_box(MEDIA_24, MDHD, |mdhd| {
        write_u32(mdhd, 12, timescale);
        write_u32(mdhd, 16, duration);
    });
    let bytes = patch_box(&bytes, &stbl(b"stts"), |stts| {
        write_u32(stts, 12, sample_duration)
    });
    let bytes = patch_box(&bytes, MVHD, |mvhd| {
        write_u32(mvhd, 12, timescale);
        write_u32(mvhd, 16, duration);
    });
    let bytes = patch_box(&bytes, TKHD, |tkhd| write_u32(tkhd, 20, duration));
    patch_box(&bytes, ELST, |elst| write_u32(elst, 8, duration))
}

/// Replace the six-frame fixture's sample clock without touching picture bytes.
#[cfg(feature = "ffmpeg-library")]
fn sample_grid_bytes(starts: &[u32], timescale: u32) -> Vec<u8> {
    assert_eq!(starts.len(), 7);
    let duration = starts[6];
    let bytes = unlisted_cfr_bytes(1, timescale);
    let bytes = patch_box(&bytes, &stbl(b"stts"), |stts| {
        stts.truncate(8);
        write_u32(stts, 4, 6);
        for pair in starts.windows(2) {
            stts.extend(1_u32.to_be_bytes());
            stts.extend((pair[1] - pair[0]).to_be_bytes());
        }
    });
    let bytes = patch_box(&bytes, MDHD, |mdhd| write_u32(mdhd, 16, duration));
    let bytes = patch_box(&bytes, MVHD, |mvhd| write_u32(mvhd, 16, duration));
    let bytes = patch_box(&bytes, TKHD, |tkhd| write_u32(tkhd, 20, duration));
    patch_box(&bytes, ELST, |elst| write_u32(elst, 8, duration))
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn quantized_source_grid_can_be_inspected_but_not_exported() {
    let bytes = sample_grid_bytes(
        &[0, 33333, 66667, 100000, 133333, 166667, 200000],
        1_000_000,
    );
    let media = inspect(&bytes).unwrap();
    assert!(
        media.timing.supported().is_err(),
        "quantized export remains unsupported"
    );
}

#[cfg(feature = "ffmpeg-library")]
fn with_composition_offsets(bytes: &[u8], offsets: &[i32]) -> Vec<u8> {
    patch_box(bytes, &STBL, |table| {
        let mut ctts = Vec::new();
        ctts.extend((16 + 8 * offsets.len() as u32).to_be_bytes());
        ctts.extend(b"ctts");
        ctts.extend([1, 0, 0, 0]); // Signed offsets.
        ctts.extend((offsets.len() as u32).to_be_bytes());
        for offset in offsets {
            ctts.extend(1_u32.to_be_bytes());
            ctts.extend(offset.to_be_bytes());
        }
        table.extend(ctts);
    })
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn nonuniform_presentation_is_not_mislabeled_as_quantized() {
    for grid in [
        [0, 33334, 66667, 100000, 133333, 166667, 200000],
        [0, 33333, 66667, 100000, 133333, 166667, 233333],
    ] {
        let file = inspect(&sample_grid_bytes(&grid, 1_000_000)).unwrap();
        assert!(matches!(
            file.timing.clock,
            crate::media::SampleClock::Irregular { .. }
        ));
        let source = crate::tests::support::video_media()
            .remove(&crate::format::MediaId("source".into()))
            .unwrap()
            .video
            .unwrap();
        assert!(
            file.validate_source(&source).is_err(),
            "physical inspection cannot reinterpret listed native rate"
        );
    }
    let bytes = sample_grid_bytes(
        &[0, 33333, 66667, 100000, 133333, 166667, 200000],
        1_000_000,
    );
    let shifted = with_composition_offsets(&bytes, &[1, 0, 0, 0, 0, 0]);
    assert!(inspect(&shifted)
        .unwrap_err()
        .to_string()
        .contains("edit list"));
    // Constant decode clocks retain their terminal exact presentation-grid gate.
    let constant = with_composition_offsets(MEDIA_24, &[1, -1, 0, 0, 0, 0]);
    assert_eq!(
        inspect(&constant).unwrap().timing.supported().unwrap().0,
        FrameRate::Fps24
    );
    let invalid = with_composition_offsets(MEDIA_24, &[1, 0, 0, 0, 0, 0]);
    assert!(inspect(&invalid)
        .unwrap_err()
        .to_string()
        .contains("exact constant frame grid"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn quantized_edit_cannot_hide_the_actual_last_sample() {
    let bytes = sample_grid_bytes(
        &[0, 33333, 66667, 100000, 133333, 166667, 200000],
        1_000_000,
    );
    for (duration, accepted) in [(166668, true), (166667, false)] {
        let shorter = patch_box(&bytes, ELST, |elst| write_u32(elst, 8, duration));
        assert_eq!(inspect(&shorter).is_ok(), accepted, "{duration}");
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn quantized_native_rate_count_and_endpoint_are_checked() {
    use crate::schema::SourceFrameRate;
    let mut file = inspect(&sample_grid_bytes(
        &[0, 33333, 66667, 100000, 133333, 166667, 200000],
        1_000_000,
    ))
    .unwrap();
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    for (count, native_step) in [
        (6, FrameRate::Fps30.ticks_per_frame()),
        (1131, FrameRate::Fps30.ticks_per_frame()),
        (1131, 8_467_191_533),
    ] {
        file.timing.sample_count = count;
        source.frame_rate = SourceFrameRate::from_ticks_per_frame(native_step).unwrap();
        source.intrinsic_ticks = i64::from(count) * native_step;
        file.validate_source(&source).unwrap();
        source.intrinsic_ticks += 1;
        assert!(file
            .validate_source(&source)
            .unwrap_err()
            .to_string()
            .contains("sample count"));
    }
    // Both two-frame endpoints round to 67ms, so endpoint equality alone
    // cannot admit a known 29.97 rate for a proven nominal 30 frame grid.
    file.timing.sample_count = 2;
    source.frame_rate = FrameRate::Fps30000Over1001.into();
    source.intrinsic_ticks = 2 * FrameRate::Fps30000Over1001.ticks_per_frame();
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("nominal rate"));
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(9_000_000_000).unwrap();
    source.intrinsic_ticks = 18_000_000_000;
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("different milliseconds"));
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(i64::MAX).unwrap();
    source.intrinsic_ticks = i64::MAX;
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("tick range"));
    assert!(SourceFrameRate::from_ticks_per_frame(0).is_err());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn quantized_approximation_is_once_per_retained_media_including_nests() {
    use crate::{
        format::MediaId,
        schema::PrVideoTrack,
        tests::support::{nest_of, video_media, video_sequence},
        OmissionScope,
    };
    use std::sync::Arc;
    let bytes = sample_grid_bytes(
        &[0, 33333, 66667, 100000, 133333, 166667, 200000],
        1_000_000,
    );
    for native_step in [FrameRate::Fps30.ticks_per_frame(), 8_467_191_533] {
        for in_nest in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join("source.mp4"), &bytes).unwrap();
            let mut media = video_media();
            let source = media.get_mut(&MediaId("source".into())).unwrap();
            source.relative_path = Some("source.mp4".into());
            source.relative_paths = vec!["source.mp4".into()];
            let native = source.video.as_mut().unwrap();
            native.frame_rate =
                crate::schema::SourceFrameRate::from_ticks_per_frame(native_step).unwrap();
            native.intrinsic_ticks = 6 * native_step;
            let mut sequence = video_sequence();
            let duration = 3 * FrameRate::Fps30.ticks_per_frame();
            let clip = sequence.video_tracks[0].clip_mut(0);
            clip.start_ticks = 0;
            clip.end_ticks = duration;
            clip.in_ticks = 0;
            clip.out_ticks = duration;
            let mut second = clip.clone();
            second.start_ticks = duration;
            second.end_ticks = 2 * duration;
            sequence.video_tracks[0]
                .items
                .push(crate::schema::PrVideoItem::Media(second));
            sequence.timeline_end_ticks = 2 * duration;
            if in_nest {
                let mut outer = video_sequence();
                outer.video_tracks = vec![PrVideoTrack {
                    items: vec![],
                    nests: vec![nest_of(sequence, 0..2 * duration, 0)],
                    transitions: vec![],
                }];
                outer.timeline_end_ticks = 2 * duration;
                sequence = outer;
            }
            let mut omissions = Vec::new();
            let pending = crate::tesseract_output::convert_premiere_sequence(
                &root.path().canonicalize().unwrap().join("project.prproj"),
                sequence,
                Arc::new(media),
                &mut omissions,
            )
            .unwrap()
            .unwrap_or_else(|| panic!("{omissions:?}"));
            assert_eq!(
                omissions
                    .iter()
                    .filter(|o| o.kind == crate::OmissionKind::Approximated)
                    .count(),
                1,
                "{omissions:?}"
            );
            assert!(
                omissions
                    .iter()
                    .all(|o| o.scope != OmissionScope::Occurrence),
                "{omissions:?}"
            );
            pending
                .write_to_staging(&root.path().join("converted.tsrct"))
                .unwrap();
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unlisted_constant_source_rates_can_be_inspected() {
    for units in [125, 100] {
        inspect(&unlisted_cfr_bytes(units, 2997)).unwrap();
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unlisted_native_source_timing_checks_count_and_rounded_endpoint() {
    use crate::schema::SourceFrameRate;
    for (units, native_step, count) in [(125, 10_594_575_533_i64, 413), (100, 8_475_650_266, 911)] {
        let mut file = inspect(&unlisted_cfr_bytes(units, 2997)).unwrap();
        assert!(
            file.timing.supported().is_err(),
            "export must remain restricted"
        );
        // Exact native/source counts from the pinned native project supplement
        // the smaller six-frame container used by these offline checks.
        file.timing.sample_count = count;
        let mut source = crate::tests::support::video_media()
            .remove(&crate::format::MediaId("source".into()))
            .unwrap()
            .video
            .unwrap();
        source.frame_rate = SourceFrameRate::from_ticks_per_frame(native_step).unwrap();
        source.intrinsic_ticks = i64::from(count) * native_step;
        file.validate_source(&source).unwrap();
        source.intrinsic_ticks += 1;
        assert!(file
            .validate_source(&source)
            .unwrap_err()
            .to_string()
            .contains("sample count"));
        source.frame_rate = SourceFrameRate::from_ticks_per_frame(native_step + 1_000_000).unwrap();
        source.intrinsic_ticks = i64::from(count) * source.frame_rate.ticks_per_frame();
        assert!(file
            .validate_source(&source)
            .unwrap_err()
            .to_string()
            .contains("different milliseconds"));
        source.frame_rate = SourceFrameRate::from_ticks_per_frame(i64::MAX).unwrap();
        source.intrinsic_ticks = i64::MAX;
        assert!(file
            .validate_source(&source)
            .unwrap_err()
            .to_string()
            .contains("tick range"));
        for invalid in [0, -1] {
            assert!(SourceFrameRate::from_ticks_per_frame(invalid).is_err());
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_irregular_duration_uses_full_edit_presentation_end() {
    // Generated with the module's FFmpeg recipe, 16x16 at 30fps, 430 frames,
    // both timescales 600, MOV output. Only the last duration and edit tail
    // differ below: native VideoStream897 records 8602/600, not 8603/600.
    let bytes = include_bytes!("../../tests/fixtures/video-430-frames.mov");
    let bytes = patch_box(bytes, &stbl(b"stts"), |table| {
        table.truncate(8);
        write_u32(table, 4, 2);
        for (count, duration) in [(429_u32, 20_u32), (1, 23)] {
            table.extend(count.to_be_bytes());
            table.extend(duration.to_be_bytes());
        }
    });
    let bytes = patch_box(&bytes, MDHD, |table| write_u32(table, 16, 8603));
    let bytes = patch_box(&bytes, MVHD, |table| write_u32(table, 16, 8602));
    let bytes = patch_box(&bytes, TKHD, |table| write_u32(table, 20, 8602));
    let bytes = patch_box(&bytes, ELST, |table| write_u32(table, 8, 8602));
    let native: crate::schema::native::VideoStream = quick_xml::de::from_str(include_str!(
        "../../tests/fixtures/iphone-edit-duration.xml"
    ))
    .unwrap();
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate = crate::schema::SourceFrameRate::from_ticks_per_frame(
        native.frame_rate.unwrap().parse().unwrap(),
    )
    .unwrap();
    source.intrinsic_ticks = native.duration.unwrap().parse().unwrap();
    let file = inspect(&bytes).unwrap();
    file.validate_source(&source).unwrap();
    source.intrinsic_ticks += source.frame_rate.ticks_per_frame();
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("sample count"));
    let clipped = patch_box(&bytes, ELST, |table| write_u32(table, 8, 8580));
    assert!(inspect(&clipped)
        .unwrap_err()
        .to_string()
        .contains("edit list"));
}

#[cfg(feature = "ffmpeg-library")]
fn irregular_average_duration_bytes() -> Vec<u8> {
    // The same generated MOV recipe as the edit-end fixture, with 1075 frames.
    let bytes = include_bytes!("../../tests/fixtures/video-1075-frames.mov");
    let bytes = patch_box(bytes, &stbl(b"stts"), |table| {
        table.truncate(8);
        write_u32(table, 4, 2);
        for (count, duration) in [(84_u32, 21_u32), (991, 20)] {
            table.extend(count.to_be_bytes());
            table.extend(duration.to_be_bytes());
        }
    });
    let bytes = patch_box(&bytes, MDHD, |table| write_u32(table, 16, 21584));
    let bytes = patch_box(&bytes, MVHD, |table| write_u32(table, 16, 21584));
    let bytes = patch_box(&bytes, TKHD, |table| write_u32(table, 20, 21584));
    patch_box(&bytes, ELST, |table| write_u32(table, 8, 21584))
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_irregular_average_duration_has_one_frame_tolerance() {
    // VideoStream1036's rounded average gives 35974 ms, while the measured
    // presentation endpoint 21584/600 is 35973 ms: only 0.252 ms apart.
    let bytes = irregular_average_duration_bytes();
    let native: crate::schema::native::VideoStream = quick_xml::de::from_str(include_str!(
        "../../tests/fixtures/iphone-average-duration.xml"
    ))
    .unwrap();
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate = crate::schema::SourceFrameRate::from_ticks_per_frame(
        native.frame_rate.unwrap().parse().unwrap(),
    )
    .unwrap();
    source.intrinsic_ticks = native.duration.unwrap().parse().unwrap();
    let file = inspect(&bytes).unwrap();
    file.validate_source(&source).unwrap();
    source.intrinsic_ticks += 1;
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("sample count"));

    // Independent bounds on native period p: (N-1)p <= end <= (N+1)p.
    // Check the immediately adjacent integer periods on each side, retaining
    // the exact count in every descriptor rather than weakening that guard.
    let endpoint = 21584_i64 * (crate::schema::TICKS / 600);
    let lower = (endpoint + 1075) / 1076;
    let upper = endpoint / 1074;
    for (period, accepted) in [
        (lower - 1, false),
        (lower, true),
        (upper, true),
        (upper + 1, false),
    ] {
        source.frame_rate = crate::schema::SourceFrameRate::from_ticks_per_frame(period).unwrap();
        source.intrinsic_ticks = 1075 * period;
        assert_eq!(file.validate_source(&source).is_ok(), accepted, "{period}");
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unlisted_endpoint_rounding_checks_the_half_millisecond_boundary_directly() {
    use crate::schema::SourceFrameRate;
    let mut file = inspect(&unlisted_cfr_bytes(125, 2997)).unwrap();
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    // The 12th source endpoint is native 500 ms versus exact file 501 ms.
    file.timing.sample_count = 12;
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(10_594_575_533).unwrap();
    source.intrinsic_ticks = 12 * source.frame_rate.ticks_per_frame();
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("different milliseconds"));
    // Exact 0.5 ms rounds to 1; one native tick before it rounds to 0.
    file.timing = crate::media::VideoTiming {
        clock: crate::media::SampleClock::Constant { sample_duration: 1 },
        timescale: 2000,
        sample_count: 1,
        ..file.timing
    };
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(crate::schema::TICKS / 2000).unwrap();
    source.intrinsic_ticks = source.frame_rate.ticks_per_frame();
    file.validate_source(&source).unwrap();
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(source.intrinsic_ticks - 1).unwrap();
    source.intrinsic_ticks = source.frame_rate.ticks_per_frame();
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("different milliseconds"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn listed_source_timing_keeps_exact_rate_and_duration_checks() {
    let file = inspect(MEDIA_24).unwrap();
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate = FrameRate::Fps24.into();
    source.intrinsic_ticks = 6 * FrameRate::Fps24.ticks_per_frame();
    file.validate_source(&source).unwrap();
    source.intrinsic_ticks += 1;
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("Duration"));
    source.frame_rate = FrameRate::Fps25.into();
    source.intrinsic_ticks = 6 * FrameRate::Fps25.ticks_per_frame();
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("FrameRate"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn fractional_tick_and_quantized_clocks_are_rejected_at_packaged_export_inspection() {
    use tesseract_file::{AssetKind, TesseractFileBuilder};
    for (bytes, reason) in [
        (
            unlisted_cfr_bytes(125, 2997),
            "physical source period is not an integral Premiere tick",
        ),
        (
            sample_grid_bytes(
                &[0, 33333, 66667, 100000, 133333, 166667, 200000],
                1_000_000,
            ),
            "quantized video sample clocks",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let asset = root.path().join("source.mp4");
        fs::write(&asset, bytes).unwrap();
        let document = crate::test_support::editable_document();
        let input = root.path().join("input.tsrct");
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset("premiere-video-1", &asset, AssetKind::Video)
            .unwrap()
            .write(&input)
            .unwrap();
        let output = root.path().join("exported");
        let error = crate::tests::support::tesseract_to_premiere(&input, &output).unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
        assert!(!output.exists());
    }
}

#[test]
fn only_physical_source_rate_reading_is_widened() {
    let source = include_str!("../../tests/fixtures/one-clip.xml");
    let xml = source.replace(
        "<VideoStream ObjectID=\"8\"><Duration>2540160000000</Duration><FrameRate>8467200000</FrameRate>",
        "<VideoStream ObjectID=\"8\"><Duration>4375559695129</Duration><FrameRate>10594575533</FrameRate>",
    );
    let xml = xml.replace(
        "<OriginalDuration>2540160000000</OriginalDuration>",
        "<OriginalDuration>4375559695129</OriginalDuration>",
    );
    let project = crate::format::inspect_project_with_media(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.frame_rate, FrameRate::Fps30);
    let media = project
        .media(sequence.video_occurrences().next().unwrap())
        .unwrap();
    assert_eq!(
        media.video.as_ref().unwrap().frame_rate.ticks_per_frame(),
        10_594_575_533
    );
    let unknown_sequence = xml.replace(
        "<FrameRate>8467200000</FrameRate>",
        "<FrameRate>10594575533</FrameRate>",
    );
    assert!(crate::format::inspect_project_with_media(&unknown_sequence, None).is_err());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn variable_and_inconsistent_media_clocks_are_rejected() {
    assert_eq!(payload(MEDIA, MDHD)[0], 0, "version-0 layout");
    let step = read_u32(payload(MEDIA, &stbl(b"stts")), 12);
    for (name, bytes, message) in [
        (
            "variable",
            patch_box(MEDIA, &stbl(b"stts"), |stts| {
                assert_eq!(read_u32(stts, 4), 1, "one timing run");
                let count = read_u32(stts, 8);
                stts.truncate(8);
                write_u32(stts, 4, 2);
                for value in [count - 1, step, 1, step + 1] {
                    stts.extend(value.to_be_bytes());
                }
            }),
            // This edit adds a tick without updating the declared duration.
            "declared duration differs",
        ),
        (
            "inconsistent movie clock",
            patch_box(MEDIA, MDHD, |mdhd| write_u32(mdhd, 12, 27 * step)),
            "edit list",
        ),
    ] {
        let error = inspect(&bytes).unwrap_err().to_string();
        assert!(error.contains(message), "{name}: {error}");
    }
}

#[test]
fn only_unreadable_media_stops_conversion_while_unsupported_content_omits() {
    use crate::{
        error::{unsupported, BuildError},
        media::unsupported_media_reason,
    };
    use std::io::{Error, ErrorKind};
    use symphonia::core::errors::Error as AudioError;
    for error in [
        unsupported("MP4 audio must be AAC"),
        BuildError::Mp4(media_transcode::inspect::InspectError::Invalid(
            "bad container".into(),
        )),
        BuildError::Audio(AudioError::DecodeError("bad frame")),
        BuildError::Audio(AudioError::IoError(Error::from(ErrorKind::UnexpectedEof))),
    ] {
        let message = error.to_string();
        assert_eq!(unsupported_media_reason(error).unwrap(), message);
    }
    for error in [
        BuildError::Mp4(media_transcode::inspect::InspectError::Io(Error::other(
            "EIO",
        ))),
        BuildError::Audio(AudioError::IoError(Error::other("EIO"))),
        BuildError::Io(Error::other("EIO")),
    ] {
        let message = error.to_string();
        assert!(
            matches!(unsupported_media_reason(error), Err(BuildError::Io(ref error)) if error.kind() == ErrorKind::Other),
            "{message}"
        );
    }
    assert!(matches!(
        unsupported_media_reason(BuildError::MissingMedia("gone".into())),
        Err(BuildError::MissingMedia(_))
    ));
}

#[test]
fn one_admission_rule_maps_each_media_kind_to_its_containers() {
    use crate::{
        media::admitted_container,
        schema::{PrColorMatte, PrMediaKind},
        tests::support::video_media,
    };
    use tesseract_file::AssetKind::{Audio, Image, Video};
    let source = video_media().into_values().next().unwrap();
    let with_kind = |kind: Option<PrMediaKind>| {
        let mut media = source.clone();
        match kind {
            Some(kind) => media.video.as_mut().unwrap().kind = kind,
            None => media.video = None,
        }
        media
    };
    let video = with_kind(Some(PrMediaKind::Video {
        codec: None,
        hdr_profile: None,
    }));
    let still = with_kind(Some(PrMediaKind::Still { alpha: false }));
    let sound = with_kind(None);
    let matte = with_kind(Some(PrMediaKind::ColorMatte(PrColorMatte {
        rgb: [0, 0, 0],
    })));
    // Each row: file name, then the (asset kind, content type) that video,
    // still, and sound-only records admit it as.
    let mov = Some((Video, "video/quicktime"));
    let mp4 = Some((Video, "video/mp4"));
    for (name, as_video, as_still, as_sound) in [
        ("clip.mp4", mp4, None, mp4),
        ("clip.MOV", mov, None, mov),
        ("sound.M4a", None, None, Some((Audio, "audio/mp4"))),
        ("sound.wav", None, None, Some((Audio, "audio/wav"))),
        ("sound.MP3", None, None, Some((Audio, "audio/mpeg"))),
        ("still.PNG", None, Some((Image, "image/png")), None),
        ("still.jpg", None, Some((Image, "image/jpeg")), None),
        ("still.JPEG", None, Some((Image, "image/jpeg")), None),
        ("clip.m4v", None, None, None),
        ("clip.mkv", None, None, None),
        ("still.gif", None, None, None),
        ("sound.flac", None, None, None),
        ("no-extension", None, None, None),
        (".mp4", None, None, None),
    ] {
        let admitted = |media| {
            admitted_container(media, Path::new(name))
                .map(|container| (container.asset_kind(), container.content_type()))
        };
        assert_eq!(
            (admitted(&video), admitted(&still), admitted(&sound)),
            (as_video, as_still, as_sound),
            "{name}"
        );
        assert_eq!(admitted(&matte), None, "{name}");
    }
}

/// A six-sample source with nonconstant DTS durations, reordered composition
/// times, an origin edit and one missing presentation slot. Picture bytes stay
/// from the independent native-codec fixture; only timing tables are edited.
#[cfg(feature = "ffmpeg-library")]
fn irregular_b_frame_bytes() -> Vec<u8> {
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 700], 2800);
    let bytes = with_composition_offsets(&bytes, &[400, 100, 100, 400, 200, 300]);
    patch_box(&bytes, ELST, |elst| write_u32(elst, 12, 200))
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_presentation_source_admission_checks_native_count_and_endpoint() {
    use crate::schema::SourceFrameRate;
    let file = inspect(&irregular_b_frame_bytes()).unwrap();
    assert!(
        file.timing.supported().is_err(),
        "export remains Constant-only"
    );
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    let step = FrameRate::Fps24.ticks_per_frame() + 1;
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(step).unwrap();
    source.intrinsic_ticks = 6 * step;
    file.validate_source(&source).unwrap();
    source.intrinsic_ticks += 1;
    assert!(file.validate_source(&source).is_err());
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(step + 3_000_000_000).unwrap();
    source.intrinsic_ticks = 6 * source.frame_rate.ticks_per_frame();
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("more than one native frame period"));
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(i64::MAX).unwrap();
    source.intrinsic_ticks = i64::MAX;
    assert!(file
        .validate_source(&source)
        .unwrap_err()
        .to_string()
        .contains("tick range"));
    source.intrinsic_ticks = 6 * FrameRate::Fps24.ticks_per_frame();
    source.frame_rate = FrameRate::Fps24.into();
    assert!(
        file.validate_source(&source).is_err(),
        "listed native interpretation cannot imply an irregular physical clock"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_presentation_keeps_full_edit_uniqueness_and_grid_ambiguity_guards() {
    let valid = irregular_b_frame_bytes();
    assert!(inspect(&valid).is_ok());
    // Final decode duration is 200 units; final presentation interval is 100.
    for (end, accepted) in [(601, true), (600, false)] {
        let bytes = patch_box(&valid, ELST, |elst| write_u32(elst, 8, end));
        assert_eq!(inspect(&bytes).is_ok(), accepted, "{end}");
    }
    let bare = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 700], 2800);
    for (offsets, message) in [
        ([200, 100, 100, 400, 200, 300], "unique"),
        ([400, 100, 100, 400, 200, 400], "physical endpoint"),
    ] {
        let bytes = with_composition_offsets(&bare, &offsets);
        let bytes = patch_box(&bytes, ELST, |elst| write_u32(elst, 12, 200));
        assert!(inspect(&bytes).unwrap_err().to_string().contains(message));
    }
    for (offset, value) in [(12, 0), (16, 0)] {
        let bytes = patch_box(&valid, ELST, |elst| write_u32(elst, offset, value));
        assert!(inspect(&bytes)
            .unwrap_err()
            .to_string()
            .contains("edit list"));
    }
    let ambiguous = sample_grid_bytes(&[0, 4, 8, 13, 17, 21, 25], 100);
    assert!(inspect(&ambiguous)
        .unwrap_err()
        .to_string()
        .contains("ambiguous"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_approximation_preserves_original_bytes_and_once_per_media_warning() {
    use crate::{
        format::MediaId,
        schema::PrVideoTrack,
        tests::support::{nest_of, video_media, video_sequence},
        OmissionScope,
    };
    use std::sync::Arc;
    let bytes = irregular_b_frame_bytes();
    let native_step = FrameRate::Fps24.ticks_per_frame() + 1;
    for in_nest in [false, true] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("source.mp4"), &bytes).unwrap();
        let mut media = video_media();
        let source = media.get_mut(&MediaId("source".into())).unwrap();
        source.relative_path = Some("source.mp4".into());
        source.relative_paths = vec!["source.mp4".into()];
        let native = source.video.as_mut().unwrap();
        native.frame_rate =
            crate::schema::SourceFrameRate::from_ticks_per_frame(native_step).unwrap();
        native.intrinsic_ticks = 6 * native_step;
        let mut sequence = video_sequence();
        let duration = 3 * FrameRate::Fps30.ticks_per_frame();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.start_ticks = 0;
        clip.end_ticks = duration;
        clip.in_ticks = 0;
        clip.out_ticks = duration;
        let mut second = clip.clone();
        second.start_ticks = duration;
        second.end_ticks = 2 * duration;
        sequence.video_tracks[0]
            .items
            .push(crate::schema::PrVideoItem::Media(second));
        sequence.timeline_end_ticks = 2 * duration;
        if in_nest {
            let mut outer = video_sequence();
            outer.video_tracks = vec![PrVideoTrack {
                items: vec![],
                nests: vec![nest_of(sequence, 0..2 * duration, 0)],
                transitions: vec![],
            }];
            outer.timeline_end_ticks = 2 * duration;
            sequence = outer;
        }
        let mut omissions = Vec::new();
        let pending = crate::tesseract_output::convert_premiere_sequence(
            &root.path().canonicalize().unwrap().join("project.prproj"),
            sequence,
            Arc::new(media),
            &mut omissions,
        )
        .unwrap()
        .unwrap_or_else(|| panic!("{omissions:?}"));
        assert_eq!(
            omissions
                .iter()
                .filter(|o| o.kind == crate::OmissionKind::Approximated)
                .count(),
            1,
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .all(|o| o.scope != OmissionScope::Occurrence),
            "{omissions:?}"
        );
        assert!(omissions.iter().any(|note| note
            .reason
            .contains("irregular source presentation timestamps")
            && note
                .reason
                .contains("native intermediate frame selection remains unverified")));
        let output = root.path().join("converted.tsrct");
        pending.write_to_staging(&output).unwrap();
        let archive = tesseract_file::TesseractFile::open(&output).unwrap();
        assert_eq!(
            archive
                .asset("premiere-video-1")
                .unwrap()
                .read_verified_bytes(bytes.len() as u64)
                .unwrap(),
            bytes
        );
        let document = archive.project_json().unwrap();
        fn videos(value: &serde_json::Value, output: &mut Vec<serde_json::Value>) {
            match value {
                serde_json::Value::Object(object) => {
                    if object.get("type").is_some_and(|kind| kind == "Video") {
                        output.push(value.clone());
                    }
                    for child in object.values() {
                        videos(child, output);
                    }
                }
                serde_json::Value::Array(array) => {
                    for child in array {
                        videos(child, output);
                    }
                }
                _ => {}
            }
        }
        let mut placements = Vec::new();
        videos(&document, &mut placements);
        assert_eq!(placements.len(), 2);
        for placement in placements {
            // An ordinary 1x clock, not an editable remap.
            let mapping = &placement["playback"]["mapping"];
            assert_eq!(mapping["type"], "linear");
            assert_eq!(mapping["output"], placement["sourceRange"]);
            assert_eq!(mapping["input"]["duration"], mapping["output"]["duration"]);
            assert_eq!(
                placement["sourceRange"],
                serde_json::json!({"start":0,"duration":100})
            );
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_composition_offset_tables_require_complete_valid_runs() {
    let base = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 700], 2800);
    let short = with_composition_offsets(&base, &[400, 100, 100, 400, 200]);
    let short = patch_box(&short, ELST, |elst| write_u32(elst, 12, 200));
    assert!(inspect(&short)
        .unwrap_err()
        .to_string()
        .contains("composition offset table"));
    let valid = irregular_b_frame_bytes();
    for (offset, value) in [(8, 0), (8, 2), (8, u32::MAX), (0, 2_u32 << 24)] {
        let bytes = patch_box(&valid, &stbl(b"ctts"), |ctts| {
            write_u32(ctts, offset, value)
        });
        assert!(inspect(&bytes)
            .unwrap_err()
            .to_string()
            .contains("composition offset table"));
    }
    let unsigned = patch_box(&valid, &stbl(b"ctts"), |ctts| {
        ctts[0] = 0;
        write_u32(ctts, 12, u32::MAX);
    });
    assert!(inspect(&unsigned)
        .unwrap_err()
        .to_string()
        .contains("physical endpoint"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_whole_source_retimed_root_and_unit_nested_uses_survive() {
    use crate::{
        format::MediaId,
        schema::{PrKeyframeEasing, PrTimeRemap, PrTimeRemapKeyframe, PrVideoTrack},
        tests::support::{nest_of, video_media, video_sequence},
    };
    use std::sync::Arc;
    let quantized = sample_grid_bytes(
        &[0, 33333, 66667, 100000, 133333, 166667, 200000],
        1_000_000,
    );
    for bytes in [
        irregular_b_frame_bytes(),
        MEDIA_24.to_vec(),
        quantized.clone(),
    ] {
        for (rate, remap) in [(2.0, false), (-1.0, false), (1.0, true)] {
            for nested in [false, true] {
                let root = tempfile::tempdir().unwrap();
                fs::write(root.path().join("source.mp4"), &bytes).unwrap();
                let mut media = video_media();
                let source = media.get_mut(&MediaId("source".into())).unwrap();
                source.relative_path = Some("source.mp4".into());
                source.relative_paths = vec!["source.mp4".into()];
                let step = if bytes == quantized {
                    FrameRate::Fps30.ticks_per_frame() + 1
                } else {
                    FrameRate::Fps24.ticks_per_frame() + 1
                };
                let native = source.video.as_mut().unwrap();
                native.frame_rate =
                    crate::schema::SourceFrameRate::from_ticks_per_frame(step).unwrap();
                native.intrinsic_ticks = 6 * step;
                let mut inner = video_sequence();
                let duration = 3 * FrameRate::Fps30.ticks_per_frame();
                let clip = inner.video_tracks[0].clip_mut(0);
                clip.start_ticks = 0;
                clip.end_ticks = duration;
                clip.in_ticks = 0;
                clip.out_ticks = duration;
                clip.playback_rate = rate;
                if rate.abs() != 1.0 {
                    clip.out_ticks = 2 * duration;
                }
                if remap {
                    clip.time_remap = Some(PrTimeRemap {
                        keys: vec![
                            PrTimeRemapKeyframe {
                                timeline_ticks: 0,
                                source_ticks: 0,
                                easing: PrKeyframeEasing::Linear,
                            },
                            PrTimeRemapKeyframe {
                                timeline_ticks: duration,
                                source_ticks: duration,
                                easing: PrKeyframeEasing::Linear,
                            },
                        ],
                    });
                }
                inner.timeline_end_ticks = duration;
                let mut sequence = video_sequence();
                let unit = sequence.video_tracks[0].clip_mut(0);
                unit.start_ticks = 0;
                unit.end_ticks = duration;
                unit.in_ticks = 0;
                unit.out_ticks = duration;
                if nested {
                    sequence.video_tracks.push(PrVideoTrack {
                        items: vec![],
                        nests: vec![nest_of(inner, 0..duration, 0)],
                        transitions: vec![],
                    });
                } else {
                    let mut retimed = inner.video_tracks[0].clip_mut(0).clone();
                    retimed.start_ticks = duration;
                    retimed.end_ticks = 2 * duration;
                    sequence.video_tracks[0]
                        .items
                        .push(crate::schema::PrVideoItem::Media(retimed));
                }
                sequence.timeline_end_ticks = 2 * duration;
                let mut omissions = Vec::new();
                let result = crate::tesseract_output::convert_premiere_sequence(
                    &root.path().canonicalize().unwrap().join("project.prproj"),
                    sequence,
                    Arc::new(media),
                    &mut omissions,
                );
                assert!(
                    result.unwrap().is_some(),
                    "whole-source retiming lost: {rate}/{remap}/{nested}: {omissions:?}"
                );
            }
        }
    }
}

/// Six 24fps samples. The first edit shows 0..125 ms; the later edit skips
/// one physical frame.
#[cfg(feature = "ffmpeg-library")]
fn media_with_later_edit() -> Vec<u8> {
    let bytes = patch_box(MEDIA_24, MVHD, |header| {
        write_u32(header, 12, 600);
        write_u32(header, 16, 150);
    });
    patch_box(&bytes, ELST, |edit| {
        write_u32(edit, 4, 2);
        write_u32(edit, 8, 75);
        edit.extend(75_u32.to_be_bytes());
        edit.extend(4_i32.to_be_bytes());
        edit.extend([0, 1, 0, 0]);
    })
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_picture_can_keep_an_original_file_with_a_later_edit() {
    use crate::{
        format::MediaId,
        tests::support::{video_media, video_sequence},
    };
    use std::sync::Arc;
    // The selected 100 ms placement consumes only the first edit.
    let bytes = media_with_later_edit();
    assert!(inspect(&bytes)
        .unwrap_err()
        .to_string()
        .contains("edit list"));
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("source.mp4"), &bytes).unwrap();
    let mut media = video_media();
    let source = media.get_mut(&MediaId("source".into())).unwrap();
    source.relative_path = Some("source.mp4".into());
    source.relative_paths = vec!["source.mp4".into()];
    let native = source.video.as_mut().unwrap();
    native.frame_rate = FrameRate::Fps24.into();
    native.intrinsic_ticks = 6 * FrameRate::Fps24.ticks_per_frame();
    let mut sequence = video_sequence();
    let duration = crate::schema::TICKS / 10;
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks = 0;
    clip.end_ticks = duration;
    clip.in_ticks = 0;
    clip.out_ticks = duration;
    sequence.timeline_end_ticks = duration;
    let mut omissions = Vec::new();
    let pending = crate::tesseract_output::convert_premiere_sequence(
        &root.path().canonicalize().unwrap().join("project.prproj"),
        sequence,
        Arc::new(media),
        &mut omissions,
    )
    .unwrap()
    .unwrap_or_else(|| panic!("{omissions:?}"));
    let output = root.path().join("converted.tsrct");
    pending.write_to_staging(&output).unwrap();
    let archive = tesseract_file::TesseractFile::open(&output).unwrap();
    assert_eq!(
        archive
            .asset("premiere-video-1")
            .unwrap()
            .read_verified_bytes(bytes.len() as u64)
            .unwrap(),
        bytes
    );
    let document = archive.project_json().unwrap();
    let layer = &document["composition"]["layers"][0];
    assert_eq!(layer["type"], "Video");
    assert_eq!(
        layer["sourceRange"],
        serde_json::json!({"start":0,"duration":100})
    );
    assert_eq!(
        layer["playback"]["mapping"]["output"],
        serde_json::json!({"start":0,"duration":100})
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_inspection_requires_complete_selected_picture_context() {
    use crate::{
        format::{MediaId, PrProjectFile, PremiereProjectXml},
        tests::support::{video_media, video_sequence},
        Premiere, PremiereImportOptions,
    };
    use fx_conv::MediaStatus;
    let bytes = media_with_later_edit();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    fs::write(root.path().join("media/source.mp4"), bytes).unwrap();
    let mut media = video_media();
    let source = media.get_mut(&MediaId("source".into())).unwrap();
    source.relative_path = Some("./media/source.mp4".into());
    source.relative_paths = vec!["./media/source.mp4".into()];
    source.absolute_paths = vec![(
        crate::schema::records::MediaPathField::FilePath,
        root.path().join("media/source.mp4"),
    )];
    let native = source.video.as_mut().unwrap();
    native.kind = crate::schema::PrMediaKind::Video {
        codec: Some(crate::schema::VideoCodec::H264),
        hdr_profile: None,
    };
    native.frame_rate = FrameRate::Fps24.into();
    native.intrinsic_ticks = 6 * FrameRate::Fps24.ticks_per_frame();
    let mut sequence = video_sequence();
    sequence.id = Some("sequence-1".into());
    let duration = FrameRate::Fps30.ticks_per_frame();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks = 0;
    clip.end_ticks = duration;
    clip.in_ticks = 0;
    clip.out_ticks = duration;
    let mut second = clip.clone();
    second.start_ticks = duration;
    second.end_ticks = 2 * duration;
    sequence.video_tracks[0]
        .items
        .push(crate::schema::PrVideoItem::Media(second));
    sequence.timeline_end_ticks = 2 * duration;
    let project = PrProjectFile::from_sequences(vec![sequence], media);
    let path = root.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let target = PrProjectFile::import_targets(&path).unwrap().remove(0).id;
    let options = PremiereImportOptions {
        sequence: Some(target.clone()),
    };
    let report = Premiere.inspect_media(&path, &options, None).unwrap();
    assert_eq!(report.media.len(), 1);
    assert_eq!(report.media[0].status, MediaStatus::Supported, "{report:?}");

    // One retained use cannot authorize the unknown trim of an omitted use
    // of the same file. The native inventory still reaches both placements.
    let xml = crate::format::read_xml(&path).unwrap();
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let omitted = parsed
        .descendants()
        .filter(|node| node.has_tag_name("VideoClipTrackItem"))
        .nth(1)
        .unwrap()
        .descendants()
        .find(|node| node.has_tag_name("End"))
        .unwrap()
        .first_child()
        .unwrap()
        .range();
    let mut broken = xml.clone();
    broken.replace_range(omitted, "invalid tick");
    let incomplete = root.path().join("incomplete.prproj");
    crate::test_support::write_prproj(&incomplete, &broken);
    // Conversion resolves package media from the canonical project path.
    let incomplete = incomplete.canonicalize().unwrap();
    let (loaded, omissions) = PrProjectFile::load_import(&incomplete, Some(&target)).unwrap();
    assert_eq!(
        loaded
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1,
        "{omissions:?}"
    );
    let public = Premiere.inspect_media(&incomplete, &options, None).unwrap();
    assert_eq!(
        public.media[0].status,
        MediaStatus::RequiresTranscode,
        "{public:?}"
    );
    let report = crate::tesseract_output::inspect_native_premiere_media_for_sequence(
        &incomplete,
        loaded.single_sequence().unwrap(),
        None,
    )
    .unwrap();
    // The same whole-source reason, not an unrelated path failure.
    assert_eq!(report.media[0].reason, public.media[0].reason, "{report:?}");
    let output = root.path().join("incomplete-output");
    let Err(error) =
        crate::tesseract_import::TesseractImport::convert(&incomplete, &output, Some(&target))
    else {
        panic!("an omitted use of the same media must block admission");
    };
    assert!(error.to_string().contains("failed admission"), "{error}");
    assert!(!output.exists());
}

/// An omitted placement or an unreadable record of other media does not
/// withhold the proved selected use of a media; an unresolved or ambiguous
/// link, which could hide any use, still does. Inspection and conversion
/// share the proof.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn unrelated_omitted_media_keeps_another_media_selected_context() {
    use crate::{
        format::{MediaId, PrProjectFile, PremiereProjectXml},
        tests::support::{video_media, video_sequence},
        Premiere, PremiereImportOptions,
    };
    use fx_conv::MediaStatus;
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    // Only selected use admits the later edit; the other file is valid whole.
    let selected_bytes = media_with_later_edit();
    let mut media = std::collections::BTreeMap::new();
    for (name, bytes) in [("source", selected_bytes.as_slice()), ("other", MEDIA_24)] {
        let file = root.path().join(format!("media/{name}.mp4"));
        fs::write(&file, bytes).unwrap();
        let mut record = video_media().remove(&MediaId("source".into())).unwrap();
        record.name = format!("{name}.mp4");
        record.relative_path = Some(format!("./media/{name}.mp4"));
        record.relative_paths = vec![format!("./media/{name}.mp4")];
        record.absolute_paths = vec![(crate::schema::records::MediaPathField::FilePath, file)];
        let native = record.video.as_mut().unwrap();
        native.kind = crate::schema::PrMediaKind::Video {
            codec: Some(crate::schema::VideoCodec::H264),
            hdr_profile: None,
        };
        native.frame_rate = FrameRate::Fps24.into();
        native.intrinsic_ticks = 6 * FrameRate::Fps24.ticks_per_frame();
        media.insert(MediaId(name.into()), record);
    }
    let mut sequence = video_sequence();
    sequence.id = Some("sequence-1".into());
    let duration = FrameRate::Fps30.ticks_per_frame();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.start_ticks, clip.end_ticks) = (0, duration);
    (clip.in_ticks, clip.out_ticks) = (0, duration);
    let mut other = clip.clone();
    other.media = MediaId("other".into());
    (other.start_ticks, other.end_ticks) = (duration, 2 * duration);
    sequence.video_tracks[0]
        .items
        .push(crate::schema::PrVideoItem::Media(other));
    sequence.timeline_end_ticks = 2 * duration;
    let project = PrProjectFile::from_sequences(vec![sequence], media);
    let path = root.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let target = PrProjectFile::import_targets(&path).unwrap().remove(0).id;
    let options = PremiereImportOptions {
        sequence: Some(target.clone()),
    };
    let xml = crate::format::read_xml(&path).unwrap();
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    let start = duration.to_string();
    let item = parsed
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoClipTrackItem")
                && node
                    .descendants()
                    .any(|child| child.has_tag_name("Start") && child.text() == Some(&start))
        })
        .unwrap();
    let end = item
        .descendants()
        .find(|node| node.has_tag_name("End"))
        .unwrap()
        .first_child()
        .unwrap()
        .range();
    let owner = |item: roxmltree::Node| {
        item.descendants()
            .find(|node| node.has_tag_name("SubClip"))
            .unwrap()
            .range()
    };
    let sub_clip = owner(item);
    // The other placement plays the media whose selected use is proved.
    let selected = parsed
        .descendants()
        .find(|node| node.has_tag_name("VideoClipTrackItem") && *node != item)
        .map(owner)
        .unwrap();
    let (own, selected) = (&xml[sub_clip.clone()], &xml[selected]);
    let records = || parsed.root_element().children();
    let stream = records()
        .find(|node| {
            node.has_tag_name("Media")
                && node
                    .descendants()
                    .any(|child| child.text().is_some_and(|text| text.contains("other.mp4")))
        })
        .and_then(|media| {
            media
                .children()
                .find(|child| child.has_tag_name("VideoStream"))
        })
        .and_then(|reference| reference.attribute("ObjectRef"))
        .unwrap();
    let frame_rate = records()
        .find(|node| node.has_tag_name("VideoStream") && node.attribute("ObjectID") == Some(stream))
        .and_then(|stream| {
            stream
                .children()
                .find(|child| child.has_tag_name("FrameRate"))
        })
        .unwrap()
        .first_child()
        .unwrap()
        .range();
    let edited = |range: std::ops::Range<usize>, text: &str| {
        let mut edited = xml.clone();
        edited.replace_range(range, text);
        edited
    };
    for (label, xml, expected) in [
        (
            "omitted",
            edited(end, "invalid tick"),
            MediaStatus::Supported,
        ),
        (
            "unreadable",
            edited(frame_rate, "invalid rate"),
            MediaStatus::Supported,
        ),
        (
            "unresolved",
            edited(sub_clip.clone(), r#"<SubClip ObjectRef="missing"/>"#),
            MediaStatus::RequiresTranscode,
        ),
        // A second owner could play the selected media, in either order.
        (
            "ambiguous",
            edited(sub_clip.clone(), &format!("{own}{selected}")),
            MediaStatus::RequiresTranscode,
        ),
        (
            "ambiguous reversed",
            edited(sub_clip, &format!("{selected}{own}")),
            MediaStatus::RequiresTranscode,
        ),
    ] {
        let project = root.path().join(format!("{label}.prproj"));
        crate::test_support::write_prproj(&project, &xml);
        let report = Premiere.inspect_media(&project, &options, None).unwrap();
        let status = |report: &fx_conv::MediaPreflight, name: &str| {
            report
                .media
                .iter()
                .find(|media| media.name == name)
                .map(|media| media.status)
        };
        assert_eq!(
            status(&report, "source.mp4"),
            Some(expected),
            "{label}: {report:?}"
        );
        // Conversion resolves package media from the canonical project path.
        let project = project.canonicalize().unwrap();
        let (loaded, _) = PrProjectFile::load_import(&project, Some(&target)).unwrap();
        let sequence = loaded.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1, "{label}");
        let native = crate::tesseract_output::inspect_native_premiere_media_for_sequence(
            &project, sequence, None,
        )
        .unwrap();
        assert_eq!(
            status(&native, "source.mp4"),
            Some(expected),
            "{label}: {native:?}"
        );
        if label == "unreadable" {
            // The record that cannot be read is reported, not inspected.
            assert_eq!(status(&report, "other.mp4"), None, "{report:?}");
            assert!(!report.unassessed.is_empty());
        }
        if label.starts_with("ambiguous") {
            assert!(
                report
                    .unassessed
                    .iter()
                    .any(|reason| reason.ends_with("native ClipTrackItem/SubClip is duplicated")),
                "{report:?}"
            );
        }
        let output = root.path().join(format!("{label}-output"));
        let converted =
            crate::tesseract_import::TesseractImport::convert(&project, &output, Some(&target));
        assert!(!output.exists());
        if expected == MediaStatus::Supported {
            converted.unwrap().write().unwrap();
            let archive =
                tesseract_file::TesseractFile::open(output.join("project.tsrct")).unwrap();
            assert_eq!(
                archive
                    .asset("premiere-video-1")
                    .unwrap()
                    .read_verified_bytes(selected_bytes.len() as u64)
                    .unwrap(),
                selected_bytes
            );
        } else {
            let Err(error) = converted else {
                panic!("{label}: an unresolved or ambiguous link must block admission");
            };
            assert!(error.to_string().contains("failed admission"), "{error}");
            assert!(!output.exists());
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
fn inspect_selected(
    bytes: &[u8],
    ranges: Vec<std::ops::Range<i64>>,
    uses_audio: bool,
) -> crate::error::Result<VideoMedia> {
    crate::media::inspect_video_for_use(
        Cursor::new(bytes),
        Cursor::new(bytes),
        bytes.len() as u64,
        Some(&crate::media::VideoUse { ranges, uses_audio }),
    )
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_regular_picture_keeps_shifted_origin_and_accepted_final_frame() {
    let duration = crate::schema::TICKS / 4;
    // Positive CTTS origin is not part of the source's elapsed duration.
    let shifted = with_composition_offsets(MEDIA_24, &[2; 6]);
    let shifted = patch_box(&shifted, ELST, |edit| write_u32(edit, 12, 2));
    let file = inspect_selected(&shifted, std::iter::once(0..duration).collect(), false).unwrap();
    assert_eq!(
        file.timing.supported().unwrap(),
        (FrameRate::Fps24, duration)
    );
    assert!(!file.timing.partial_timeline);
    // Existing full-source policy admits a tail inside the last frame. Keep
    // that policy when a selected placement includes the entire native frame.
    let tail = patch_box(MEDIA_24, MVHD, |header| {
        write_u32(header, 12, 600);
        write_u32(header, 16, 133);
    });
    let tail = patch_box(&tail, ELST, |edit| write_u32(edit, 8, 133));
    assert!(inspect_selected(&tail, std::iter::once(0..duration).collect(), false).is_ok());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_partial_edit_rejects_crossing_later_segments_and_ambiguous_origin() {
    let short = media_with_later_edit();
    let ticks = crate::schema::TICKS;
    assert!(inspect_selected(&short, std::iter::once(0..ticks / 10).collect(), false).is_ok());
    assert!(
        inspect_selected(&short, std::iter::once(0..ticks / 6).collect(), false)
            .unwrap_err()
            .to_string()
            .contains("crosses the first")
    );
    assert!(inspect_selected(
        &short,
        std::iter::once(ticks / 10..ticks / 5).collect(),
        false
    )
    .is_err());
    for (offset, value) in [(8, 0), (12, u32::MAX), (12, 1), (16, 0)] {
        let bad = patch_box(&short, ELST, |edit| write_u32(edit, offset, value));
        assert!(
            inspect_selected(&bad, std::iter::once(0..ticks / 10).collect(), false).is_err(),
            "{offset}/{value}"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_irregular_endpoint_requires_exact_native_count_and_safe_interior() {
    use crate::schema::SourceFrameRate;
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 710], 2400);
    let usage = crate::media::VideoUse {
        ranges: std::iter::once(0..crate::schema::TICKS / 10).collect(),
        uses_audio: false,
    };
    let file = inspect_selected(&bytes, usage.ranges.clone(), false).unwrap();
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate =
        SourceFrameRate::from_ticks_per_frame(FrameRate::Fps24.ticks_per_frame() + 1).unwrap();
    source.intrinsic_ticks = 6 * source.frame_rate.ticks_per_frame();
    assert!(
        file.validate_source(&source).is_err(),
        "whole-source endpoint stays strict"
    );
    assert!(file.validate_source_for_use(&source, Some(&usage)).unwrap());
    source.intrinsic_ticks += 1;
    assert!(file
        .validate_source_for_use(&source, Some(&usage))
        .unwrap_err()
        .to_string()
        .contains("sample count"));
    source.intrinsic_ticks -= 1;
    let terminal = crate::media::VideoUse {
        ranges: std::iter::once(crate::schema::TICKS / 5..crate::schema::TICKS / 4).collect(),
        uses_audio: false,
    };
    assert!(file
        .validate_source_for_use(&source, Some(&terminal))
        .unwrap_err()
        .to_string()
        .contains("uncertain final sample"));
    let duplicate = with_composition_offsets(&bytes, &[100, 0, 0, 0, 0, 0]);
    assert!(inspect_selected(&duplicate, usage.ranges.clone(), false)
        .unwrap_err()
        .to_string()
        .contains("unique"));
    for (offset, value) in [(8, 7), (12, 0)] {
        let bad = patch_box(&bytes, &stbl(b"stts"), |table| {
            write_u32(table, offset, value)
        });
        assert!(inspect_selected(&bad, usage.ranges.clone(), false).is_err());
    }
    let ctts = with_composition_offsets(&bytes, &[0; 6]);
    for (offset, value) in [(0, 2 << 24), (8, 0), (12, u32::MAX)] {
        let bad = patch_box(&ctts, &stbl(b"ctts"), |table| {
            write_u32(table, offset, value)
        });
        if offset == 12 {
            let bad = patch_box(&bad, &stbl(b"ctts"), |table| table[0] = 0);
            assert!(inspect_selected(&bad, usage.ranges.clone(), false).is_err());
        } else {
            assert!(inspect_selected(&bad, usage.ranges.clone(), false).is_err());
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_picture_retains_unused_audio_streams_but_consumed_audio_stays_strict() {
    let original = include_bytes!("../../tests/fixtures/video-with-audio.mp4");
    let moov = payload(original, &[b"moov"]);
    let mut offset = 0;
    let mut tracks = Vec::new();
    while offset < moov.len() {
        let size = read_u32(moov, offset) as usize;
        if &moov[offset + 4..offset + 8] == b"trak" {
            tracks.push(&moov[offset..offset + size]);
        }
        offset += size;
    }
    assert_eq!(tracks.len(), 2);
    let audio = patch_box(tracks[1], &[b"trak", b"tkhd"], |header| {
        write_u32(header, 12, 3)
    });
    let bytes = patch_box(original, &[b"moov"], |movie| movie.extend(&audio));
    let ranges: Vec<_> = std::iter::once(0..crate::schema::TICKS / 10).collect();
    assert!(inspect(&bytes)
        .unwrap_err()
        .to_string()
        .contains("multiple audio"));
    assert!(inspect_selected(&bytes, ranges.clone(), false).is_ok());
    assert!(inspect_selected(&bytes, ranges, true)
        .unwrap_err()
        .to_string()
        .contains("multiple audio"));
    assert!(crate::audio_media::inspect_audio_media(
        Cursor::new(bytes.clone()),
        bytes.len() as u64,
        "mp4"
    )
    .is_err());
}

/// Descriptive tags affect neither picture nor sound. Their payload layouts
/// and missing optional fields must not gate otherwise validated media.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn optional_movie_metadata_payloads_do_not_gate_picture_or_sound() {
    use super::audio_media::FailingReader;
    use crate::{error::BuildError, media::unsupported_media_reason};
    fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = u32::try_from(payload.len() + 8).unwrap().to_be_bytes();
        [size.as_slice(), kind, payload].concat()
    }
    let original = include_bytes!("../../tests/fixtures/video-with-audio.mp4");
    // An `mdta` handler, one key, and the item list that holds its value: a
    // `data` box whose type and locale header precedes UTF-8 text.
    let hdlr = mp4_box(b"hdlr", &[&[0; 8][..], b"mdta", &[0; 14]].concat());
    // The same handler box with a 64-bit size.
    let wide_size = u64::try_from(hdlr.len() + 8).unwrap().to_be_bytes();
    let wide_hdlr = [&1_u32.to_be_bytes()[..], b"hdlr", &wide_size, &hdlr[8..]].concat();
    let key = mp4_box(b"mdta", b"com.example.title");
    let keys = mp4_box(b"keys", &[&[0, 0, 0, 0, 0, 0, 0, 1][..], &key].concat());
    let item_list = |data: &[u8]| {
        let item = mp4_box(&1_u32.to_be_bytes(), &mp4_box(b"data", data));
        mp4_box(b"ilst", &item)
    };
    let tags = item_list(b"\0\0\0\x01\0\0\0\0clip");
    // A `data` box without its locale, and an item list that declares one
    // byte more than its `meta` holds.
    let truncated = item_list(b"\0\0\0\x01");
    let mut oversized = tags.clone();
    write_u32(&mut oversized, 0, read_u32(&tags, 0) + 1);
    // Video and sound admission: whether each found its stream, or why not.
    let admission = |bytes: &[u8]| {
        let video = inspect(bytes).map(|_| true);
        let sound = crate::audio_media::inspect_audio_media(
            Cursor::new(bytes.to_vec()),
            bytes.len() as u64,
            "mp4",
        )
        .map(|stream| stream.is_some());
        [video, sound].map(|result| result.map_err(|error| error.to_string()))
    };
    let admitted = [Ok(true), Ok(true)];
    for (parent, path) in [("moov", &[b"moov"][..]), ("udta", &[b"moov", b"udta"][..])] {
        // Both parents end the file, so a read past the `meta` would fail.
        let with_meta = |payload: &[u8]| {
            let meta = mp4_box(b"meta", payload);
            let bytes = patch_box(original, path, |children| children.extend(&meta));
            assert!(bytes.ends_with(&meta), "{parent}");
            bytes
        };
        for (layout, version_and_flags, handler) in [
            ("QuickTime", &[][..], &hdlr),
            ("QuickTime with a 64-bit hdlr size", &[][..], &wide_hdlr),
            ("ISO", &[0; 4][..], &hdlr),
        ] {
            let payload = |list: &[u8]| [version_and_flags, handler, &keys, list].concat();
            let bytes = with_meta(&payload(&tags));
            assert_eq!(admission(&bytes), admitted, "{layout} {parent}");
            for list in [&truncated, &oversized] {
                let bytes = with_meta(&payload(list));
                assert_eq!(admission(&bytes), admitted, "{layout} {parent}");
            }
        }
        for payload in [&[0; 2][..], &[0; 4][..], &[0; 6][..]] {
            assert_eq!(
                admission(&with_meta(payload)),
                admitted,
                "{parent} {payload:?}"
            );
        }
    }
    // Required outer framing still propagates failed reads, rather than
    // treating an unavailable movie as an optional descriptive tag.
    let failing = || FailingReader {
        bytes: Cursor::new(original.to_vec()),
        fail_at: 0,
    };
    for error in [
        inspect_video_media(Cursor::new(original), failing(), original.len() as u64).unwrap_err(),
        crate::audio_media::inspect_audio_media(failing(), original.len() as u64, "mp4")
            .unwrap_err(),
    ] {
        assert!(matches!(
            unsupported_media_reason(error),
            Err(BuildError::Io(ref error)) if error.kind() == std::io::ErrorKind::Other
        ));
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn quarter_turn_requires_native_agreement_for_any_use_and_rejects_skew_or_mirror() {
    use crate::schema::VideoOrientation;
    // Same9 matrix words as a native90degree container; dimensions unchanged.
    let rotate = |matrix: [i32; 9]| {
        patch_box(MEDIA_24, TKHD, |header| {
            for (index, value) in matrix.into_iter().enumerate() {
                header[40 + 4 * index..44 + 4 * index].copy_from_slice(&value.to_be_bytes());
            }
        })
    };
    let unit = 1 << 16;
    let matrix = [0, unit, 0, -unit, 0, 0, 1080 * unit, 0, 1 << 30];
    let bytes = rotate(matrix);
    // The orientation does not depend on whether the selected intervals are
    // known: whole-source inspection and a selected use read the same turn.
    let usage = crate::media::VideoUse {
        ranges: std::iter::once(0..crate::schema::TICKS / 10).collect(),
        uses_audio: false,
    };
    let whole = inspect(&bytes).unwrap();
    let file = inspect_selected(&bytes, usage.ranges.clone(), false).unwrap();
    assert_eq!(whole.orientation, VideoOrientation::Clockwise);
    assert_eq!(file.orientation, VideoOrientation::Clockwise);
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate = FrameRate::Fps24.into();
    source.intrinsic_ticks = crate::schema::TICKS / 4;
    // Both paths require the native orientation to agree.
    for error in [
        whole.validate_source(&source).unwrap_err(),
        file.validate_source_for_use(&source, Some(&usage))
            .unwrap_err(),
    ] {
        assert!(error.to_string().contains("orientation"), "{error}");
    }
    source.orientation = VideoOrientation::Clockwise;
    assert_eq!(source.display_dimensions(), [1080, 1920]);
    assert!(whole.validate_source(&source).is_ok());
    assert!(file.validate_source_for_use(&source, Some(&usage)).is_ok());
    for (index, value) in [(0, 1), (3, unit), (6, 1), (8, 0)] {
        let mut bad = matrix;
        bad[index] = value;
        assert!(inspect(&rotate(bad)).is_err(), "{index}/{value}");
        assert!(
            inspect_selected(&rotate(bad), usage.ranges.clone(), false).is_err(),
            "{index}/{value}"
        );
    }
    for native in ["2", "4", "5", "7", "9"] {
        assert!(VideoOrientation::from_native(native).is_err());
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn partial_legacy_v0_offsets_preserve_the_signed_player_clock_and_exclude_tail() {
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 710], 2400);
    let bytes = with_composition_offsets(&bytes, &[100, -100, 0, 0, 0, 0]);
    let bytes = patch_box(&bytes, &stbl(b"ctts"), |table| table[0] = 0);
    // The source edit hides the tail: full-source admission must still reject.
    let bytes = patch_box(&bytes, ELST, |edit| write_u32(edit, 8, 450));
    assert!(inspect(&bytes)
        .unwrap_err()
        .to_string()
        .contains("edit list changes the full source"));
    let ranges: Vec<_> = std::iter::once(0..crate::schema::TICKS / 10).collect();
    let file = inspect_selected(&bytes, ranges.clone(), false).unwrap();
    assert!(file.timing.legacy_signed_ctts);
    assert!(matches!(
        file.timing.clock,
        crate::media::SampleClock::Irregular { .. }
    ));
    assert!(inspect_selected(
        &bytes,
        std::iter::once(0..crate::schema::TICKS / 4).collect(),
        false
    )
    .is_err());
    let mismatched = patch_box(&bytes, ELST, |edit| write_u32(edit, 12, 1));
    assert!(inspect_selected(&mismatched, ranges.clone(), false)
        .unwrap_err()
        .to_string()
        .contains("origin"));
    let duplicate = patch_box(&bytes, &stbl(b"ctts"), |table| write_u32(table, 12, 0));
    assert!(inspect_selected(&duplicate, ranges, false)
        .unwrap_err()
        .to_string()
        .contains("unique"));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn exact_cfr_legacy_v0_keeps_existing_whole_source_and_selected_final_frames() {
    let bytes = with_composition_offsets(MEDIA_24, &[1, -1, 0, 0, 0, 0]);
    let bytes = patch_box(&bytes, &stbl(b"ctts"), |table| table[0] = 0);
    assert_eq!(
        inspect(&bytes).unwrap().timing.supported().unwrap().0,
        FrameRate::Fps24
    );
    assert!(inspect_selected(
        &bytes,
        std::iter::once(0..crate::schema::TICKS / 4).collect(),
        false
    )
    .is_ok());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn established_signed_v1_irregular_full_edit_keeps_its_valid_final_frame() {
    let bytes = irregular_b_frame_bytes();
    let usage = crate::media::VideoUse {
        ranges: std::iter::once(0..7 * FrameRate::Fps30.ticks_per_frame()).collect(),
        uses_audio: false,
    };
    let file = inspect_selected(&bytes, usage.ranges.clone(), false).unwrap();
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate = crate::schema::SourceFrameRate::from_ticks_per_frame(
        FrameRate::Fps24.ticks_per_frame() + 1,
    )
    .unwrap();
    source.intrinsic_ticks = 6 * source.frame_rate.ticks_per_frame();
    file.validate_source(&source).unwrap();
    assert!(!file.validate_source_for_use(&source, Some(&usage)).unwrap());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn interpreted_whole_cfr_binds_measured_not_saved_cadence() {
    use crate::schema::{SourceFrameRate, SourceInterpretation};
    let mut file = inspect(&unlisted_cfr_bytes(1001, 120000)).unwrap();
    let mut source = crate::tests::support::video_media()
        .into_values()
        .next()
        .unwrap()
        .video
        .unwrap();
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(2_118_936_594).unwrap();
    source.intrinsic_ticks = 6 * 2_118_936_594;
    source.interpretation = SourceInterpretation::Rate(FrameRate::Fps30.into());
    let clock = crate::media::InterpretedPictureClock::bind(&source, &file).unwrap();
    assert_eq!(clock.ratio(), (1001, 4000));
    assert_eq!(clock.duration_millis(), 50);
    let (rate, duration) = file.timing.source_clock().unwrap();
    assert_eq!(rate.ticks_per_frame(), 2_118_916_800);
    assert_eq!(duration, 12_713_500_800);
    assert!(
        file.timing.supported().is_err(),
        "sequence/audio allowlist unchanged"
    );
    source.intrinsic_ticks += source.frame_rate.ticks_per_frame();
    assert!(crate::media::InterpretedPictureClock::bind(&source, &file).is_err());
    source.intrinsic_ticks -= source.frame_rate.ticks_per_frame();
    file.timing.partial_timeline = true;
    assert!(file.timing.source_clock().is_err());
    assert!(crate::media::InterpretedPictureClock::bind(&source, &file).is_err());
    file.timing.partial_timeline = false;
    file.timing.clock = crate::media::SampleClock::Quantized {
        nominal: FrameRate::Fps30,
    };
    assert!(crate::media::InterpretedPictureClock::bind(&source, &file).is_err());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn codec_only_pixel_aspect_checks_the_final_display_dimensions() {
    let mut bytes =
        include_bytes!("../../tests/fixtures/interpreted-pixel-aspect/inputs/movie.mp4").to_vec();
    // Leave the coded picture and VUI SAR2:1 unchanged, but remove the
    // container's duplicate declaration without moving any sample offsets.
    let pasp = bytes.windows(4).position(|value| value == b"pasp").unwrap();
    bytes[pasp..pasp + 4].copy_from_slice(b"free");
    let display = |width: u32| {
        patch_box(&bytes, TKHD, |header| {
            assert_eq!(header[0], 0);
            write_u32(header, 76, width << 16);
        })
    };
    let facts = inspect(&display(640)).unwrap();
    assert_eq!((facts.width, facts.height), (320, 180));
    assert_eq!(facts.pixel_aspect.scale(), 2.0);
    // Both coded and aspect-adjusted tkhd dimensions are valid; an unrelated
    // display width is not, even when the ratio came only from the codec.
    assert!(inspect(&display(320)).is_ok());
    let error = inspect(&display(480)).unwrap_err();
    assert!(error.to_string().contains("display dimensions"), "{error}");
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn missing_par_override_uses_file_ratio_and_publishes_editable_windows() {
    use tesseract_file::TesseractFile;

    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("native")).unwrap();
    fs::create_dir(root.path().join("inputs")).unwrap();
    let input = root.path().join("native/source.prproj");
    let asset = root.path().join("inputs/movie.mp4");
    // Supplemental edit of a pinned Adobe-native source; its real file stores
    // 2:1 pixels. Neither source dimensions nor our writer establish that ratio.
    let mut xml = crate::tests::support::prproj_xml(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/interpreted-pixel-aspect/native/source.prproj"),
    );
    assert!(xml.contains("<OverriddenPAR>3,2</OverriddenPAR>"));
    assert!(xml.contains("<OriginalPAR>2,1</OriginalPAR>"));
    xml = xml
        .replace("<OverriddenPAR>3,2</OverriddenPAR>", "")
        .replace("<OriginalPAR>2,1</OriginalPAR>", "");
    for tag in ["ActualMediaFilePath", "FilePath"] {
        assert!(crate::tests::support::relink(&mut xml, tag, &asset) > 0);
    }
    crate::test_support::write_prproj(&input, &xml);
    fs::write(
        &asset,
        include_bytes!("../../tests/fixtures/interpreted-pixel-aspect/inputs/movie.mp4"),
    )
    .unwrap();
    let output = root.path().join("imported");
    let report = crate::premiere_to_tesseract(
        &input,
        &output,
        Some("b7c7fefb-d8a5-4123-92e5-59f2796f2cce"),
        false,
    )
    .unwrap();
    assert!(report
        .iter()
        .any(|note| note.reason.contains("missing OverriddenPAR") && note.reason.contains("2:1")));
    let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document: serde_json::Value = serde_json::from_slice(file.project_json_bytes()).unwrap();
    let videos: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(videos.len(), 2);
    for layer in videos {
        assert_eq!(
            layer["transform"]["scale"],
            serde_json::json!([100.0, 50.0])
        );
        assert_eq!(layer["source"]["sourceRect"]["width"], 320.0);
        assert_eq!(layer["sourceIntrinsicDuration"], 1000.0);
    }
    // Identity agreement still precedes reading fallback metadata.
    let collision = root.path().join("inputs/collision.mp4");
    fs::write(&collision, MEDIA).unwrap();
    let conflicted = xml.replace(
        &format!("<FilePath>{}</FilePath>", asset.display()),
        &format!("<FilePath>{}</FilePath>", collision.display()),
    );
    assert_ne!(conflicted, xml);
    crate::test_support::write_prproj(&input, &conflicted);
    assert!(crate::premiere_to_tesseract(
        &input,
        root.path().join("collision-output"),
        Some("b7c7fefb-d8a5-4123-92e5-59f2796f2cce"),
        false
    )
    .is_err());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn interpreted_pixel_aspect_import_and_edited_export_keep_coded_pixels() {
    use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

    // Native source: 2:1 file pixels overridden to 3:2, 60 fps interpreted as
    // 30 fps, and two trimmed placements with native uniform Scale 50.
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("native")).unwrap();
    fs::create_dir(root.path().join("inputs")).unwrap();
    let input = root.path().join("native/source.prproj");
    let asset = root.path().join("inputs/movie.mp4");
    // The relative hint leaves the project directory, so admission needs a live
    // absolute alias. Relink the author-host aliases in this temporary copy only.
    let mut xml = crate::tests::support::prproj_xml(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/interpreted-pixel-aspect/native/source.prproj"),
    );
    for tag in ["ActualMediaFilePath", "FilePath"] {
        assert!(
            crate::tests::support::relink(&mut xml, tag, &asset) > 0,
            "native fixture changed its {tag} hints"
        );
    }
    crate::test_support::write_prproj(&input, &xml);
    fs::write(
        &asset,
        include_bytes!("../../tests/fixtures/interpreted-pixel-aspect/inputs/movie.mp4"),
    )
    .unwrap();
    let output = root.path().join("imported");
    crate::premiere_to_tesseract(
        &input,
        &output,
        Some("b7c7fefb-d8a5-4123-92e5-59f2796f2cce"),
        false,
    )
    .unwrap();
    let imported = fs::read_dir(&output)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let file = TesseractFile::open(imported).unwrap();
    let mut document: serde_json::Value =
        serde_json::from_slice(file.project_json_bytes()).unwrap();
    let videos = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|layer| layer["type"] == "Video")
        .collect::<Vec<_>>();
    assert_eq!(videos.len(), 2);
    for layer in videos {
        assert_eq!(layer["transform"]["scale"], serde_json::json!([75.0, 50.0]));
        assert_eq!(
            layer["transform"]["anchorPoint"],
            serde_json::json!([160.0, 90.0])
        );
        assert_eq!(layer["source"]["fit"], "stretch");
        assert_eq!(layer["source"]["sourceRect"]["width"].as_f64(), Some(320.0));
        assert_eq!(
            layer["source"]["sourceRect"]["height"].as_f64(),
            Some(180.0)
        );
        assert_eq!(layer["sourceIntrinsicDuration"].as_f64(), Some(1000.0));
        layer["transform"]["scale"][0] = serde_json::json!(90.0);
    }
    let edited = root.path().join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &asset, AssetKind::Video)
        .unwrap()
        .write(&edited)
        .unwrap();
    let exported = root.path().join("exported");
    crate::tests::support::tesseract_to_premiere(&edited, &exported).unwrap();
    let path = fs::read_dir(&exported)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "prproj"))
        .unwrap();
    let xml = crate::format::read_xml(&path).unwrap();
    assert!(xml.contains("<IsPAROverridden>true</IsPAROverridden>"));
    assert!(xml.contains("<OverriddenPAR>1,1</OverriddenPAR>"));
    let (project, _) = crate::format::PrProjectFile::load_import(&path, None).unwrap();
    let clips = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect::<Vec<_>>();
    assert_eq!(clips.len(), 2);
    for clip in clips {
        assert_eq!(clip.transform.scale, [90.0, 50.0]);
        assert_eq!(clip.playback_rate, 0.5);
    }

    // These two editable inputs are not coded Stretch: Contain centers a
    // 320x90 picture in the fixed frame; Natural draws the full 640x180.
    let mut videos = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|layer| layer["type"] == "Video");
    let contained = videos.next().unwrap();
    contained["source"]["fit"] = serde_json::json!("contain");
    contained["transform"]["anchorPoint"] = serde_json::json!([0.0, 0.0]);
    videos.next().unwrap()["source"]
        .as_object_mut()
        .unwrap()
        .remove("sourceRect");
    let fitted = root.path().join("fitted.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &asset, AssetKind::Video)
        .unwrap()
        .write(&fitted)
        .unwrap();
    let output = root.path().join("fitted");
    crate::tests::support::tesseract_to_premiere(&fitted, &output).unwrap();
    let path = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "prproj"))
        .unwrap();
    let (project, _) = crate::format::PrProjectFile::load_import(&path, None).unwrap();
    let clips = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect::<Vec<_>>();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].transform.scale, [90.0, 25.0]);
    assert_eq!(clips[0].transform.anchor_point, [0.0, -0.5]);
    assert_eq!(clips[1].transform.scale, [180.0, 50.0]);
    assert_eq!(clips[1].transform.anchor_point, [0.25, 0.5]);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn interpreted_picture_import_and_edited_export_use_fractional_source_ticks() {
    use std::io::Write;
    use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    let asset = root.path().join("media/source.mp4");
    fs::write(&asset, unlisted_cfr_bytes(1001, 120000)).unwrap();
    let xml = include_str!("../../tests/fixtures/interpreted-cfr.xml");
    let input = root.path().join("input.prproj");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(xml.as_bytes()).unwrap();
    fs::write(&input, encoder.finish().unwrap()).unwrap();
    let output = root.path().join("imported");
    crate::premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    let imported = fs::read_dir(&output)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let file = TesseractFile::open(imported).unwrap();
    let mut document: serde_json::Value =
        serde_json::from_slice(file.project_json_bytes()).unwrap();
    let layer = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    assert_eq!(layer["sourceIntrinsicDuration"], 50);
    assert_eq!(
        layer["sourceRange"],
        serde_json::json!({"start":0,"duration":51})
    );
    assert_eq!(
        layer["playback"]["mapping"]["input"],
        serde_json::json!({"start":0,"duration":4000})
    );
    assert_eq!(
        layer["playback"]["mapping"]["output"],
        serde_json::json!({"start":0,"duration":1001})
    );
    // Trim to 100..200, then move to 200..300; only the visible window and
    // input offset change. The source starts at 25.025 ms, not 25 ms.
    layer["playback"]["inputRange"] = serde_json::json!({"start":200,"duration":100});
    layer["playback"]["inputOffsetMs"] = serde_json::json!(-100);
    document["composition"]["duration"] = serde_json::json!(400);
    let edited = root.path().join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &asset, AssetKind::Video)
        .unwrap()
        .write(&edited)
        .unwrap();
    let exported = root.path().join("exported");
    crate::tests::support::tesseract_to_premiere(&edited, &exported).unwrap();
    let path = fs::read_dir(&exported)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "prproj"))
        .unwrap();
    let (project, _) = crate::format::PrProjectFile::load_import(&path, None).unwrap();
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.playback_rate, 1001.0 / 4000.0);
    assert_eq!(
        (clip.in_ticks, clip.out_ticks),
        (6_356_750_400, 12_713_500_800)
    );
    let stream = project.media(clip).unwrap().video.as_ref().unwrap();
    assert_eq!(stream.frame_rate.ticks_per_frame(), 2_118_916_800);
    assert_eq!(stream.intrinsic_ticks, 12_713_500_800);
    assert_eq!(
        stream.interpretation,
        crate::schema::SourceInterpretation::Original
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn full_source_legacy_v0_offsets_match_validated_signed_v1_timing() {
    let signed = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 710], 2400);
    let signed = with_composition_offsets(&signed, &[100, -100, 0, 0, 0, 0]);
    let legacy = patch_box(&signed, &stbl(b"ctts"), |table| table[0] = 0);
    let expected = inspect(&signed).unwrap();
    let actual = inspect(&legacy).unwrap();
    assert!(actual.timing.legacy_signed_ctts);
    assert_eq!(actual.timing.sample_count, expected.timing.sample_count);
    assert_eq!(actual.timing.timescale, expected.timing.timescale);
    let crate::media::SampleClock::Irregular {
        media_end: actual_end,
    } = actual.timing.clock
    else {
        panic!("expected irregular clock");
    };
    let crate::media::SampleClock::Irregular {
        media_end: expected_end,
    } = expected.timing.clock
    else {
        panic!("expected irregular clock");
    };
    assert_eq!(actual_end, expected_end);
    assert!(!actual.timing.partial_timeline);
    let usage = std::iter::once(0..crate::schema::TICKS * 710 / 2400).collect();
    assert!(inspect_selected(&legacy, usage, false).is_ok());
}

/// Public-path regression: a native appearance omission does not invalidate
/// independently readable unit source clocks. Synthetic container timing, not
/// an independent Adobe render oracle.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_reverse_import_keeps_sample_times_despite_appearance_omission() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    let bytes = irregular_average_duration_bytes();
    fs::write(root.path().join("media/source.mov"), &bytes).unwrap();
    let input = root.path().join("source.prproj");
    crate::test_support::write_prproj(
        &input,
        include_str!("../../tests/fixtures/iphone-reverse-admission.xml"),
    );
    let options = crate::PremiereImportOptions {
        sequence: Some("sequence-1".into()),
    };
    let report = crate::Premiere
        .inspect_media(&input, &options, None)
        .unwrap();
    assert_eq!(report.media.len(), 1);
    assert_eq!(
        report.media[0].status,
        fx_conv::MediaStatus::Supported,
        "{report:?}"
    );
    let output = root.path().join("import");
    let omissions =
        crate::premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    assert!(omissions
        .iter()
        .any(|item| item.record == "9" && item.reason.contains("FrameRect")));
    let archive = fs::read_dir(output)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let file = tesseract_file::TesseractFile::open(archive).unwrap();
    let document = file.project_json().unwrap();
    let videos: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(videos.len(), 1);
    let video = videos[0];
    let keys = &video["playback"]["mapping"]["property"]["keyframes"];
    assert_eq!(keys[0]["time"], 0);
    assert_eq!(keys[0]["value"], 27398);
    assert_eq!(keys[1]["time"], 467);
    assert_eq!(keys[1]["value"], 26931);
    let asset = video["source"]["assetId"].as_str().unwrap();
    let retained = file
        .asset(asset)
        .unwrap()
        .read_verified_bytes(bytes.len() as u64)
        .unwrap();
    assert_eq!(
        retained, bytes,
        "retiming must not replace the VFR sample clock"
    );
    let timing =
        media_transcode::inspect::inspect(Cursor::new(&retained), retained.len() as u64, true)
            .unwrap();
    let stream = &timing.streams[0];
    assert_eq!((stream.time_base_num, stream.time_base_den), (1, 600));
    let mut starts: Vec<_> = timing
        .packets
        .iter()
        .map(|packet| packet.pts.unwrap())
        .collect();
    starts.sort_unstable();
    // The decoder's nearest-earlier rule uses these retained PTS after the
    // editable reverse clock, not the native average period as a CFR grid.
    for (request_ms, index) in [(27398, 817), (27164, 810), (26931, 803)] {
        let selected = starts.partition_point(|pts| pts * 1000 <= request_ms * 600) - 1;
        assert_eq!(selected, index);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_public_import_proves_clocks_despite_appearance_omission() {
    use crate::test_support::{one_clip_xml, write_prproj, OneClip};
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("media")).unwrap();
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 710], 2400);
    let bytes = with_composition_offsets(&bytes, &[100, -100, 0, 0, 0, 0]);
    let bytes = patch_box(&bytes, &stbl(b"ctts"), |table| table[0] = 0);
    fs::write(root.path().join("media/source.mp4"), &bytes).unwrap();
    let frame = FrameRate::Fps30.ticks_per_frame();
    let period = crate::schema::TICKS * 710 / 2400 / 6;
    let xml = one_clip_xml(OneClip {
        media_frame: period,
        media_duration: period * 6,
        end: frame,
        out_point: frame,
        ..OneClip::default()
    });
    let xml = xml.replace(
        "<TrackItem ObjectRef=\"3\"/>",
        "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"9\"/>",
    );
    let xml = xml.replace("</PremiereData>", &format!(
        "<VideoClipTrackItem ObjectID=\"9\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"4\"/></ComponentOwner><TrackItem><Start>{frame}</Start><End>{}</End></TrackItem><SubClip ObjectRef=\"5\"/></ClipTrackItem><FrameRect>0,0,960,540</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem></PremiereData>", 2 * frame));
    let input = root.path().join("source.prproj");
    write_prproj(&input, &xml);
    let output = root.path().join("import");
    let omissions =
        crate::premiere_to_tesseract(&input, &output, Some("sequence-1"), false).unwrap();
    assert!(omissions
        .iter()
        .any(|item| item.record == "9" && item.reason.contains("FrameRect")));
    let archive = fs::read_dir(&output)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let file = tesseract_file::TesseractFile::open(archive).unwrap();
    let document = file.project_json().unwrap();
    let videos: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(videos.len(), 1);
    let asset = videos[0]["source"]["assetId"].as_str().unwrap();
    assert_eq!(
        file.asset(asset)
            .unwrap()
            .read_verified_bytes(bytes.len() as u64)
            .unwrap(),
        bytes
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_active_replacement_exports_original_pts_bytes_and_shared_retimes() {
    use crate::test_support::{editable_document, linear_playback};
    use serde_json::json;
    use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};
    let root = tempfile::tempdir().unwrap();
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 710], 2400);
    let bytes = with_composition_offsets(&bytes, &[100, -100, 0, 0, 0, 0]);
    let bytes = patch_box(&bytes, &stbl(b"ctts"), |table| table[0] = 0);
    let replacement = root.path().join("replacement.mp4");
    let inactive = root.path().join("inactive.mp4");
    fs::write(&replacement, &bytes).unwrap();
    fs::write(&inactive, MEDIA).unwrap();
    let mut document = editable_document();
    let mut video = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["type"] == "Video")
        .unwrap()
        .clone();
    video["volume"] = json!(0);
    video["sourceIntrinsicDuration"] = json!(296);
    video["sourceRange"] = json!({"start":50,"duration":100});
    video["source"]["eyeContact"] = json!({"enabled":true,"eyeContactAssetId":"irregular"});
    let layers: Vec<_> = (0..2)
        .map(|index| {
            let mut layer = video.clone();
            layer["id"] = json!(index + 1);
            layer["playback"] = linear_playback(
                json!({"start":index*100,"duration":100}),
                json!({"start":50,"duration":100}),
            );
            layer
        })
        .collect();
    document["composition"]["layers"] = json!(layers);
    document["duration"] = json!(0.2);
    let archive = root.path().join("selected.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &inactive, AssetKind::Video)
        .unwrap()
        .add_asset("irregular", &replacement, AssetKind::Video)
        .unwrap()
        .write(&archive)
        .unwrap();
    let output = root.path().join("native");
    let omissions = crate::tesseract_to_premiere(&archive, &output, false).unwrap();
    assert!(
        omissions
            .iter()
            .any(|o| o.reason.contains("nominal descriptor") && o.reason.contains("timestamps")),
        "{omissions:?}"
    );
    let media: Vec<_> = fs::read_dir(output.join("media"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(media.len(), 1);
    assert_eq!(fs::read(&media[0]).unwrap(), bytes);
    let reimport = root.path().join("reimport");
    crate::premiere_to_tesseract(output.join("project.prproj"), &reimport, None, false).unwrap();
    let file = TesseractFile::open(
        fs::read_dir(reimport)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    let doc = file.project_json().unwrap();
    let videos: Vec<_> = doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["type"] == "Video")
        .collect();
    assert_eq!(videos.len(), 2);
    assert_eq!(file.metadata().assets.len(), 1);
    for video in videos {
        assert_eq!(video["sourceRange"], json!({"start":50,"duration":100}));
        assert_eq!(video["sourceIntrinsicDuration"], 296);
        assert_eq!(crate::test_support::layer_range(video)["duration"], 100);
    }
    // Keep the independently required opaque canvas under an omitted use.
    let mut canvas = editable_document()["composition"]["layers"][1].clone();
    canvas["id"] = json!(3);
    canvas["activeRange"] = json!({"start":0,"duration":200});
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(canvas);
    // A validated whole source retains editable retimes alongside a unit sibling.
    for (name, values) in [
        ("nonunit", vec![(100, 50), (200, 200)]),
        ("hold", vec![(100, 100), (200, 100)]),
        ("reverse", vec![(100, 150), (200, 50)]),
        ("ramp", vec![(100, 50), (150, 75), (200, 150)]),
    ] {
        let keys: Vec<_> = values.iter().enumerate().map(|(index, (time, value))| json!({
            "id":format!("key-{index}"), "time":time, "value":value, "easing":{"type":"linear"}
        })).collect();
        document["composition"]["layers"][1]["playback"] = crate::test_support::remapped_playback(
            json!({"start":100,"duration":100}),
            json!({"before":"inactive", "after":"inactive", "keyframes":keys}),
        );
        document["composition"]["layers"][1]["sourceRange"] = json!({"start":50,"duration":150});
        let archive = root.path().join(format!("{name}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset("premiere-video-1", &inactive, AssetKind::Video)
            .unwrap()
            .add_asset("irregular", &replacement, AssetKind::Video)
            .unwrap()
            .write(&archive)
            .unwrap();
        let output = root.path().join(name);
        let omissions = crate::tesseract_to_premiere(&archive, &output, false).unwrap();
        assert!(
            omissions
                .iter()
                .any(|o| o.reason.contains("nominal descriptor")),
            "{name}: {omissions:?}"
        );
        let (project, _) = crate::PrProjectFile::load(output.join("project.prproj")).unwrap();
        let mut clips: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .filter(|clip| !project.media(clip).unwrap().is_generator())
            .collect();
        clips.sort_by_key(|clip| clip.start_ticks);
        let ms = crate::schema::TICKS_PER_MILLISECOND;
        let frame = FrameRate::Fps30.ticks_per_frame();
        let stream = project.media(clips[0]).unwrap().video.as_ref().unwrap();
        let source_frame = stream.frame_rate.ticks_per_frame();
        let reverse_in = stream.intrinsic_ticks - 150 * ms - (source_frame - 1);
        // The reverse tail selects one source frame; the ramp's half-frame
        // cut snaps upward. Its final one-frame leg becomes a source-frame hold.
        let expected = match name {
            "nonunit" => vec![(3, 6, 50 * ms, 200 * ms, 1.5, false)],
            "hold" => vec![(3, 6, 100 * ms, 200 * ms, 1.0, true)],
            "reverse" => vec![
                (3, 5, reverse_in, reverse_in + 2 * frame, -1.0, false),
                (5, 6, source_frame, source_frame + frame, 1.0, true),
            ],
            "ramp" => vec![
                (3, 5, 50 * ms, 50 * ms + frame, 0.5, false),
                (5, 6, 2 * source_frame, 2 * source_frame + frame, 1.0, true),
            ],
            _ => unreachable!(),
        };
        assert_eq!(clips.len(), expected.len() + 1, "{name}");
        assert_eq!(clips[0].start_ticks, 0);
        assert_eq!(clips[0].end_ticks, 3 * frame);
        assert_eq!(clips[0].in_ticks, 50 * ms);
        assert_eq!(clips[0].out_ticks, 150 * ms);
        assert_eq!(clips[0].playback_rate, 1.0);
        assert!(clips[0].time_remap.is_none());
        for (clip, &(start, end, source_in, source_out, rate, held)) in
            clips[1..].iter().zip(&expected)
        {
            assert_eq!(clip.start_ticks, start * frame, "{name}");
            assert_eq!(clip.end_ticks, end * frame, "{name}");
            assert_eq!(clip.in_ticks, source_in, "{name}");
            assert_eq!(clip.out_ticks, source_out, "{name}");
            assert_eq!(clip.playback_rate, rate, "{name}");
            assert_eq!(clip.media, clips[0].media);
            assert_eq!(clip.time_remap.is_some(), held, "{name}");
            if let Some(remap) = &clip.time_remap {
                assert!(remap.keys.iter().all(|key| key.source_ticks == source_in));
            }
        }
        assert!(clips
            .windows(2)
            .all(|pair| pair[0].end_ticks == pair[1].start_ticks));
        assert_eq!(clips.last().unwrap().end_ticks, 200 * ms);
        if matches!(name, "reverse" | "ramp") {
            let diagnostic = if name == "reverse" {
                "source-frame selection can differ"
            } else {
                "editable speed and Frame Hold segments"
            };
            assert!(
                omissions.iter().any(|o| o.reason.contains(diagnostic)),
                "{omissions:?}"
            );
            assert!(!omissions.iter().any(|o| o
                .reason
                .contains("selected source endpoints at constant speed")));
        }
        // Relocation must not make the exported package depend on its old path.
        let relocated = root.path().join(format!("relocated-{name}"));
        fs::rename(&output, &relocated).unwrap();
        let reimport = root.path().join(format!("reimport-{name}"));
        crate::premiere_to_tesseract(relocated.join("project.prproj"), &reimport, None, false)
            .unwrap();
        let file = TesseractFile::open(
            fs::read_dir(reimport)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        )
        .unwrap();
        let doc = file.project_json().unwrap();
        let mut videos: Vec<_> = doc["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .collect();
        videos.sort_by_key(|layer| layer["playback"]["inputRange"]["start"].as_i64());
        assert_eq!(videos.len(), clips.len(), "{name}");
        let round_ms = |ticks: i64| (ticks + ms / 2) / ms;
        for (video, clip) in videos.iter().zip(&clips) {
            let start = round_ms(clip.start_ticks);
            let end = round_ms(clip.end_ticks);
            assert_eq!(
                video["playback"]["inputRange"],
                json!({"start":start,"duration":end-start})
            );
            assert_eq!(video["playback"]["inputOffsetMs"], 0);
            assert_eq!(video["sourceIntrinsicDuration"], 296);
            assert_eq!(video["source"]["assetId"], videos[0]["source"]["assetId"]);
            assert_eq!(video["volume"].as_f64(), Some(0.0));
            let mapping = &video["playback"]["mapping"];
            if clip.playback_rate == 1.0 && clip.time_remap.is_none() {
                assert_eq!(mapping["type"], "linear");
                assert_eq!(mapping["output"], json!({"start":50,"duration":100}));
                assert_eq!(video["sourceRange"], mapping["output"]);
                continue;
            }
            // Public import retains each native part: reverse bounds are
            // reflected about the physical source end, holds keep a constant
            // key pair, and all exposed times use nearest milliseconds.
            let held = clip.time_remap.is_some();
            let (first, last) = if held {
                (clip.in_ticks, clip.in_ticks)
            } else if clip.playback_rate < 0.0 {
                (
                    stream.intrinsic_ticks - clip.in_ticks,
                    stream.intrinsic_ticks - clip.out_ticks,
                )
            } else {
                (clip.in_ticks, clip.out_ticks)
            };
            let (first, last) = (round_ms(first), round_ms(last));
            assert_eq!(mapping["type"], "timeRemap");
            let keys = mapping["property"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2);
            assert_eq!(
                (keys[0]["time"].as_i64(), keys[1]["time"].as_i64()),
                (Some(start), Some(end))
            );
            assert_eq!(
                (keys[0]["value"].as_i64(), keys[1]["value"].as_i64()),
                (Some(first), Some(last))
            );
            assert!(keys.iter().all(|key| key["easing"]["type"] == "linear"));
            let extrapolation = if held { "continue" } else { "inactive" };
            assert_eq!(mapping["property"]["before"], extrapolation);
            assert_eq!(mapping["property"]["after"], extrapolation);
            let range = if held {
                json!({"start":0,"duration":296})
            } else {
                json!({"start":first.min(last),"duration":(last-first).abs()})
            };
            assert_eq!(video["sourceRange"], range);
        }
        let asset = videos[0]["source"]["assetId"].as_str().unwrap();
        assert_eq!(
            file.asset(asset)
                .unwrap()
                .read_verified_bytes(bytes.len() as u64)
                .unwrap(),
            bytes
        );
        assert_eq!(file.metadata().assets.len(), 1);
        let packaged: Vec<_> = fs::read_dir(relocated.join("media"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(packaged.len(), 1);
        assert_eq!(fs::read(&packaged[0]).unwrap(), bytes);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn irregular_nominal_descriptor_rejects_ambiguous_listed_rate_and_partial_edit() {
    let bytes = sample_grid_bytes(
        &[0, 33334, 66667, 100000, 133333, 166667, 200000],
        1_000_000,
    );
    let file = inspect(&bytes).unwrap();
    assert!(file
        .timing
        .source_clock()
        .unwrap_err()
        .to_string()
        .contains("ambiguous"));
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 710], 2400);
    let mut file = inspect(&bytes).unwrap();
    let (rate, duration) = file.timing.source_clock().unwrap();
    assert_eq!(duration, rate.ticks_per_frame() * 6);
    assert!(
        (i128::from(duration) * 2400 - i128::from(crate::schema::TICKS) * 710).abs() <= 3 * 2400
    );
    file.timing.partial_timeline = true;
    assert!(file
        .timing
        .source_clock()
        .unwrap_err()
        .to_string()
        .contains("partial"));
}

#[cfg(feature = "ffmpeg-library")]
mod declared_tail;

#[cfg(feature = "ffmpeg-library")]
mod presentation_origin;
