//! Media timing rules, tested on real container bytes without conversion.
//!
//! Seven six-frame H.264 samples under `tests/fixtures` provide the
//! non-30-fps container timing that the 30 fps samples lack. Invalid inputs are
//! small in-memory edits of stored samples, so ordinary tests do not need FFmpeg.
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

use crate::{
    format::FrameRate,
    media::{inspect_video_media, VideoMedia},
};
use std::{fs, io::Cursor, path::Path};

const MEDIA: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");
const MEDIA_23_976: &[u8] = include_bytes!("../../tests/fixtures/video-23.976fps.mp4");
const MEDIA_24: &[u8] = include_bytes!("../../tests/fixtures/video-24fps.mp4");
const MVHD: &[&[u8; 4]] = &[b"moov", b"mvhd"];
const TKHD: &[&[u8; 4]] = &[b"moov", b"trak", b"tkhd"];
const ELST: &[&[u8; 4]] = &[b"moov", b"trak", b"edts", b"elst"];
const MDHD: &[&[u8; 4]] = &[b"moov", b"trak", b"mdia", b"mdhd"];
const STBL: [&[u8; 4]; 5] = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];

fn inspect(bytes: &[u8]) -> crate::error::Result<VideoMedia> {
    inspect_video_media(Cursor::new(bytes), Cursor::new(bytes), bytes.len() as u64)
}

fn stbl(child: &'static [u8; 4]) -> Vec<&'static [u8; 4]> {
    [STBL.as_slice(), &[child]].concat()
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

/// Edits one box payload and fixes parent sizes and shifted chunk offsets.
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

fn payload<'a>(bytes: &'a [u8], path: &[&[u8; 4]]) -> &'a [u8] {
    let (offset, size) = *box_path(bytes, path).last().unwrap();
    &bytes[offset + 8..offset + size]
}

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

#[test]
fn unlisted_constant_source_rates_can_be_inspected() {
    for units in [125, 100] {
        inspect(&unlisted_cfr_bytes(units, 2997)).unwrap();
    }
}

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

#[test]
fn unlisted_and_quantized_clocks_are_rejected_at_packaged_export_inspection() {
    use tesseract_file::{AssetKind, TesseractFileBuilder};
    for (bytes, reason) in [
        (
            unlisted_cfr_bytes(125, 2997),
            "unsupported video frame rate",
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
fn irregular_b_frame_bytes() -> Vec<u8> {
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 700], 2800);
    let bytes = with_composition_offsets(&bytes, &[400, 100, 100, 400, 200, 300]);
    patch_box(&bytes, ELST, |elst| write_u32(elst, 12, 200))
}

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
    source.frame_rate = SourceFrameRate::from_ticks_per_frame(step + 1_000_000_000).unwrap();
    source.intrinsic_ticks = 6 * source.frame_rate.ticks_per_frame();
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
    source.intrinsic_ticks = 6 * FrameRate::Fps24.ticks_per_frame();
    source.frame_rate = FrameRate::Fps24.into();
    assert!(
        file.validate_source(&source).is_err(),
        "listed native interpretation cannot imply an irregular physical clock"
    );
}

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

#[test]
#[ignore = "requires local licensed GAIL_TIMING_SOURCE; not redistributed or native timing proof"]
fn local_external_irregular_b_frame_source_matches_pinned_aggregate_facts() {
    use sha2::{Digest, Sha256};
    let bytes =
        fs::read(std::env::var_os("GAIL_TIMING_SOURCE").expect("local source path")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "697f2b79686669b44c60c00a2ac5af487c3aa55d1d1013252d9f9f8db38b3527"
    );
    let file = inspect(&bytes).unwrap();
    assert!(matches!(
        file.timing.clock,
        crate::media::SampleClock::Irregular {
            media_end: 1_003_008
        }
    ));
    assert_eq!(file.timing.timescale, 15360);
    assert_eq!(file.timing.sample_count, 1958);
    let mut source = crate::tests::support::video_media()
        .remove(&crate::format::MediaId("source".into()))
        .unwrap()
        .video
        .unwrap();
    source.frame_rate =
        crate::schema::SourceFrameRate::from_ticks_per_frame(8_471_509_805).unwrap();
    source.intrinsic_ticks = 16_587_216_198_190;
    file.validate_source(&source).unwrap();
    assert!(file.timing.supported().is_err());
}

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
                .contains("intermediate native frame-clock interpretation remains unverified")));
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

#[test]
fn irregular_retimed_root_and_nested_uses_fail_video_admission() {
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
                if bytes == MEDIA_24 || bytes == quantized {
                    assert!(result.unwrap().is_some(), "existing Constant/Quantized retiming changed: {rate}/{remap}/{nested}: {omissions:?}");
                } else {
                    let error = match result {
                        Err(error) => error.to_string(),
                        Ok(_) => {
                            panic!("irregular retimed media survived: {rate}/{remap}/{nested}")
                        }
                    };
                    assert!(
                        error.contains("failed admission")
                            && error
                                .contains("requires unit forward playback without Time Remapping"),
                        "{error}"
                    );
                }
            }
        }
    }
}

/// Six 24fps samples. The first edit shows 0..125 ms; the later edit skips
/// one physical frame.
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
    let report = Premiere.inspect_media(&incomplete, &options, None).unwrap();
    assert_eq!(
        report.media[0].status,
        MediaStatus::RequiresTranscode,
        "{report:?}"
    );
    let report = crate::tesseract_output::inspect_native_premiere_media_for_sequence(
        &incomplete,
        loaded.single_sequence().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        report.media[0].status,
        MediaStatus::RequiresTranscode,
        "{report:?}"
    );
}

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

#[test]
fn selected_legacy_v0_offsets_preserve_the_signed_player_clock_and_exclude_tail() {
    let bytes = sample_grid_bytes(&[0, 100, 200, 300, 400, 500, 710], 2400);
    let bytes = with_composition_offsets(&bytes, &[100, -100, 0, 0, 0, 0]);
    let bytes = patch_box(&bytes, &stbl(b"ctts"), |table| table[0] = 0);
    assert!(inspect(&bytes)
        .unwrap_err()
        .to_string()
        .contains("selected unit interior"));
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
