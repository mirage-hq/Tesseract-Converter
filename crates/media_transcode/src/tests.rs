use super::*;
use std::sync::atomic::AtomicBool;

#[cfg(not(feature = "ffmpeg-library"))]
#[test]
fn library_backend_requires_feature_for_actual_work() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("media.mov");
    fs::write(&input, b"not-media").unwrap();
    let error = backend::probe(&Backend::Library, &input, &AtomicBool::new(false)).unwrap_err();
    assert!(error
        .to_string()
        .contains("requires the `ffmpeg-library` feature"));
}

#[test]
fn pcm_profile_matches_library_backend_without_reducing_precision() {
    let mut audio = model::AudioInfo {
        codec: "pcm_s16be".into(),
        sample_format: "s16".into(),
        sample_rate: 48_000,
        channels: 2,
        channel_layout: "stereo".into(),
        start_seconds: 0.0,
        duration_seconds: 1.0,
    };
    for (source, encoder, output) in [
        ("u8", "pcm_s16le", "s16"),
        ("s16p", "pcm_s16le", "s16"),
        ("s32", "pcm_s32le", "s32"),
        ("fltp", "pcm_f32le", "flt"),
    ] {
        audio.sample_format = source.into();
        assert_eq!(backend::pcm_encoder(&audio).unwrap(), encoder);
        assert!(pcm_sample_format_matches(encoder, output));
    }
    audio.sample_format = "s64".into();
    assert!(backend::pcm_encoder(&audio).is_err());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn ae_primed_mp3_to_wav_rechecks_decoded_audio_without_relaxing_normal_runs() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("primed.mp3");
    let output = directory.path().join("decoded.wav");
    fs::write(
        &input,
        include_bytes!("../tests/fixtures/ae_primed_mono.mp3"),
    )
    .unwrap();
    let cancelled = AtomicBool::new(false);
    let request = || TranscodeRequest {
        input: &input,
        output: &output,
        backend: Backend::Library,
        cancelled: &cancelled,
    };
    let rejected = run(request(), &mut |_| {}).unwrap_err();
    assert!(rejected.to_string().contains("zero-start media"));
    assert!(!output.exists());
    let result = run_for_after_effects_audio(request(), &mut |_| {}).unwrap();
    let source = result.source.audio.unwrap();
    let media = result.media.audio.unwrap();
    assert!(source.start_seconds > 0.0);
    assert_eq!(media.start_seconds, 0.0);
    assert_eq!(media.sample_rate, source.sample_rate);
    assert!(
        (media.duration_seconds - source.duration_seconds).abs()
            < 1.0 / f64::from(source.sample_rate)
    );
    assert!(output.exists());
}

#[test]
fn sony_audio_endpoint_clock_preserves_samples_and_rejects_real_drift() {
    #[derive(serde::Deserialize)]
    struct Endpoint {
        video_clock: model::Ratio,
        frames: u64,
        audio: model::AudioInfo,
        source_audio_samples: u64,
    }
    let fixture: Endpoint =
        serde_json::from_str(include_str!("../tests/fixtures/sony_audio_endpoint.json")).unwrap();
    let scale =
        backend::movie_timescale(&fixture.video_clock, Some(fixture.audio.sample_rate)).unwrap();
    // Both endpoints must be integral movie ticks, including a one-sample edit.
    assert_eq!(scale % fixture.audio.sample_rate as i32, 0);
    assert_eq!(
        i64::from(scale) * i64::from(fixture.video_clock.num) % i64::from(fixture.video_clock.den),
        0
    );
    assert_eq!(scale, 240_000);
    let mut source = video_info("mov", "h264");
    source.audio = Some(fixture.audio.clone());
    let video = source.video.as_mut().unwrap();
    video.frames = fixture.frames;
    video.frame_rate = model::Ratio {
        num: fixture.video_clock.den,
        den: fixture.video_clock.num,
    };
    video.duration_seconds = fixture.audio.duration_seconds;
    let mut output = source.clone();
    output.audio.as_mut().unwrap().codec = "aac".into();
    validate_output(Profile::H264, &source, &output).unwrap();
    for lost_samples in [2, 24] {
        output.audio.as_mut().unwrap().duration_seconds = (fixture.source_audio_samples
            - lost_samples) as f64
            / f64::from(fixture.audio.sample_rate);
        assert!(validate_output(Profile::H264, &source, &output)
            .unwrap_err()
            .to_string()
            .contains("audio timing"));
    }
}

#[test]
fn movie_timescale_reduces_clocks_and_rejects_unrepresentable_lcm() {
    use model::Ratio;
    assert_eq!(
        backend::movie_timescale(&Ratio { num: 2, den: 60 }, Some(48_000)),
        Some(48_000)
    );
    assert_eq!(
        backend::movie_timescale(
            &Ratio {
                num: 1,
                den: 90_000
            },
            Some(44_100)
        ),
        Some(4_410_000)
    );
    assert_eq!(
        backend::movie_timescale(
            &Ratio {
                num: 1,
                den: i32::MAX
            },
            Some(48_000)
        ),
        None
    );
    assert_eq!(
        backend::movie_timescale(
            &Ratio {
                num: 1,
                den: i32::MAX
            },
            None
        ),
        Some(i32::MAX)
    );
    for clock in [Ratio { num: 0, den: 30 }, Ratio { num: 1, den: -30 }] {
        assert_eq!(backend::movie_timescale(&clock, Some(48_000)), None);
    }
    assert_eq!(
        backend::movie_timescale(&Ratio { num: 1, den: 30 }, Some(0)),
        None
    );
}

#[test]
fn output_validation_rejects_one_missing_frame_with_matching_duration() {
    let source = video_info("mov", "h264");
    let mut output = source.clone();
    output.video.as_mut().unwrap().frames = 29;
    let error = validate_output(Profile::H264, &source, &output).unwrap_err();
    assert!(error.to_string().contains("frame count"));
}

#[derive(serde::Deserialize)]
pub(super) struct NativeCoarseClock {
    pub native_time_base: model::Ratio,
    pub native_frame_rate: model::Ratio,
    pub window_pts: Vec<i64>,
    pub window_final_duration: i64,
}

pub(super) fn native_coarse_clock() -> NativeCoarseClock {
    serde_json::from_str(include_str!("../tests/fixtures/native_coarse_clock.json")).unwrap()
}

#[test]
fn remux_timescale_preserves_native_coarse_clock_without_widening_cadence() {
    let clock = native_coarse_clock();
    let scale = backend::remux_video_timescale(&clock.native_time_base).unwrap();
    assert_eq!(scale, 600);
    assert_eq!(clock.window_final_duration, 20);
    let expected = f64::from(clock.native_frame_rate.den) * f64::from(scale)
        / f64::from(clock.native_frame_rate.num);
    for pair in clock.window_pts.windows(2) {
        let delta = (pair[1] - pair[0]) as f64;
        assert!(model::nominal_frame_delta(delta, expected));
    }
    assert!(!model::nominal_frame_delta(21.0 * 32.0, expected * 32.0));
    for delta in [0.0, -20.0, 22.0, 40.0] {
        assert!(!model::nominal_frame_delta(delta, expected));
    }
    for (num, den, expected) in [
        (1, i32::MAX, Some(i32::MAX)),
        (3, 1000, Some(1000)),
        (i32::MAX, i32::MAX, Some(i32::MAX)),
        (0, 600, None),
        (1, 0, None),
        (-1, 600, None),
        (1, i32::MIN, None),
    ] {
        assert_eq!(
            backend::remux_video_timescale(&model::Ratio { num, den }),
            expected
        );
    }
}

#[test]
fn review_nominal_clock_rejects_duplicates_gaps_and_extreme_deltas() {
    use crate::model::nominal_frame_delta;
    assert!(!nominal_frame_delta(0.0, 1.0));
    assert!(!nominal_frame_delta(2.0, 1.0));
    assert!(!nominal_frame_delta(-1.0, 1.0));
    let extreme = (i128::from(i64::MAX) - i128::from(i64::MIN)) as f64;
    assert!(!nominal_frame_delta(extreme, 1.0));
    assert!(nominal_frame_delta(1.0, 1.0));
    // 30000/1001 fps on a millisecond clock alternates 33/34 ticks.
    assert!(nominal_frame_delta(33.0, 1001.0 / 30.0));
    assert!(nominal_frame_delta(34.0, 1001.0 / 30.0));
    assert!(!nominal_frame_delta(35.0, 1001.0 / 30.0));
    // ffprobe's decimal timestamp rounding must not reject an exact clock.
    assert!(nominal_frame_delta(1.00000002, 1.0));
}

#[test]
fn ae_output_contract_failures_are_fatal_instead_of_source_policy_fallback() {
    let source = video_info("mov", "h264");
    for alpha_changed in [false, true] {
        let mut output = source.clone();
        if alpha_changed {
            output.video.as_mut().unwrap().alpha = true;
        } else {
            output.video.as_mut().unwrap().frames -= 1;
        }
        let error = validate_output(Profile::H264, &source, &output).unwrap_err();
        assert!(matches!(
            output_validation_error(Destination::AfterEffects, error),
            TranscodeError::Protocol(_)
        ));
    }
    assert!(matches!(
        output_validation_error(Destination::AfterEffects, TranscodeError::Cancelled),
        TranscodeError::Cancelled
    ));
    assert!(matches!(
        output_validation_error(
            Destination::General,
            TranscodeError::Policy("unsupported".into())
        ),
        TranscodeError::Policy(_)
    ));
}

#[test]
fn edit_list_free_clock_rejects_reordering_offsets_missing_packets_and_subtick_drift() {
    let video = video_info("mp4", "h264").video.unwrap();
    for mutation in 0..5 {
        let mut clock = DirectVideoClock::new(&video);
        for index in 0..30 {
            let pts = index + i64::from(mutation == 1);
            let dts = index - i64::from(mutation == 2);
            if mutation != 3 || index != 29 {
                clock.observe(Some(pts), Some(dts), if mutation == 4 { 2 } else { 1 });
            }
        }
        assert_eq!(clock.complete(), mutation == 0);
    }
    let rate = model::Ratio { num: 24, den: 1 };
    let time_base = model::Ratio {
        num: 1,
        den: 12_288,
    };
    assert!(exact_frame_time(Some(512), 1, &time_base, &rate));
    assert!(!exact_frame_time(Some(513), 1, &time_base, &rate));
    assert!(!exact_frame_time(Some(1), 0, &time_base, &rate));
    assert!(!exact_frame_time(None, 0, &time_base, &rate));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn ae_destination_remux_preserves_the_checked_fractional_source_clock() {
    let parent = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../premiere_file/tests/fixtures/video-24fps.mp4");
    let output = parent.path().join("prepared.mov");
    let result = run_for_after_effects(
        TranscodeRequest {
            input: &source,
            output: &output,
            backend: Backend::Library,
            cancelled: &AtomicBool::new(false),
        },
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(result.operation, Operation::Remux);
    let before = result.source.video.unwrap();
    let after = result.media.video.unwrap();
    assert_eq!(before.frames, 6);
    assert_eq!(after.frames, before.frames);
    assert_eq!(after.duration_seconds, 0.25);
    assert_eq!(after.frame_rate, before.frame_rate);
    assert_eq!(after.alpha, before.alpha);
    assert_eq!(result.output_sha256, sha256_file(&output).unwrap());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn h264_ae_planning_does_not_remux_high422_with_a_direct_clock() {
    let directory = tempfile::tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../premiere_file/tests/fixtures/video-24fps.mp4");
    let output = directory.path().join("prepared.mov");
    let cancelled = AtomicBool::new(false);
    let request = TranscodeRequest {
        input: &input,
        output: &output,
        backend: Backend::Library,
        cancelled: &cancelled,
    };
    let mut source = backend::probe(&Backend::Library, &input, &cancelled).unwrap();
    let capabilities = backend::capabilities(&Backend::Library, &cancelled).unwrap();
    let can_encode_h264 = capabilities
        .encoders
        .iter()
        .any(|encoder| matches!(encoder.as_str(), "h264_videotoolbox" | "h264_mf"));
    // Keep real packet timing so the direct-clock check cannot hide the pixel gate.
    for (pixel_format, expected) in [
        ("yuv422p10le", Operation::Transcode),
        ("yuv420p", Operation::Remux),
        ("yuvj420p", Operation::Remux),
    ] {
        source.video.as_mut().unwrap().pixel_format = pixel_format.into();
        let selected = plan_after_effects(&request, &input, &source);
        if expected == Operation::Transcode && !can_encode_h264 {
            assert!(matches!(selected, Err(TranscodeError::Backend { stderr })
                if stderr == "selected native backend has no approved H.264 encoder"));
        } else {
            assert_eq!(selected.unwrap().operation, expected);
        }
    }
}

/// AE callers recover from `Policy` as unsupported source media. A source
/// change after the backend returns must abort instead of becoming that signal.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn ae_source_change_is_fatal_while_unsupported_source_remains_policy() {
    use std::io::Write;

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../premiere_file/tests/fixtures");
    let directory = tempfile::tempdir().unwrap();
    let cancelled = AtomicBool::new(false);
    let unsupported = run_for_after_effects(
        TranscodeRequest {
            input: &fixtures.join("video-with-audio.mp4"),
            output: &directory.path().join("audio.mov"),
            backend: Backend::Library,
            cancelled: &cancelled,
        },
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(unsupported, TranscodeError::Policy(_)),
        "{unsupported:?}"
    );

    let source = directory.path().join("source.mp4");
    fs::copy(fixtures.join("video-24fps.mp4"), &source).unwrap();
    let output = directory.path().join("prepared.mov");
    let mut changed = false;
    let error = run_for_after_effects(
        TranscodeRequest {
            input: &source,
            output: &output,
            backend: Backend::Library,
            cancelled: &cancelled,
        },
        &mut |progress| {
            // Only the post-backend status has no measured media time.
            if progress.status == ProgressStatus::Finishing
                && progress.processed_seconds.is_none()
                && !changed
            {
                let mut file = fs::OpenOptions::new().append(true).open(&source).unwrap();
                file.write_all(b"x").unwrap();
                changed = true;
            }
        },
    )
    .unwrap_err();
    assert!(changed);
    assert!(
        matches!(error, TranscodeError::Protocol(ref reason) if reason.contains("source media changed")),
        "{error:?}"
    );
    assert!(!output.exists());
    assert!(fs::read_dir(directory.path()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".media-transcode-")));
}

#[test]
fn ae_existing_destination_is_an_io_conflict_instead_of_source_policy() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("prepared.mov");
    fs::write(&output, b"foreign").unwrap();
    let error = run_for_after_effects(
        TranscodeRequest {
            input: &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../premiere_file/tests/fixtures/video-24fps.mp4"),
            output: &output,
            backend: Backend::Library,
            cancelled: &AtomicBool::new(false),
        },
        &mut |_| panic!("an existing destination must be rejected before preparation"),
    )
    .unwrap_err();
    assert!(
        matches!(error, TranscodeError::Io { ref source, .. } if source.kind() == io::ErrorKind::AlreadyExists),
        "{error:?}"
    );
    assert_eq!(fs::read(&output).unwrap(), b"foreign");
}

#[test]
fn missing_encoder_is_a_backend_failure_only_for_the_ae_destination() {
    let capabilities = model::Capabilities {
        version: "test".into(),
        encoders: Vec::new(),
    };
    let source = video_info("mov", "h264");
    for profile in [Profile::H264, Profile::Prores4444] {
        let select = |destination| {
            select_encoder(
                &Backend::Library,
                &source,
                profile,
                &capabilities,
                destination,
            )
            .unwrap_err()
        };
        assert!(matches!(
            select(Destination::General),
            TranscodeError::Policy(_)
        ));
        assert!(matches!(
            select(Destination::AfterEffects),
            TranscodeError::Backend { .. }
        ));
    }
}

#[cfg(all(feature = "ffmpeg-library", target_os = "macos"))]
#[test]
fn ae_destination_normalizes_b_frames_without_changing_presentation_time() {
    let parent = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../premiere_file/tests/fixtures/source_rect_64x36.mp4");
    let output = parent.path().join("prepared.mov");
    let result = run_for_after_effects(
        TranscodeRequest {
            input: &source,
            output: &output,
            backend: Backend::Library,
            cancelled: &AtomicBool::new(false),
        },
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(result.operation, Operation::Transcode);
    assert_eq!(result.encoder, "h264_videotoolbox");
    let before = result.source.video.unwrap();
    let after = result.media.video.unwrap();
    assert_eq!(after.frames, before.frames);
    assert_eq!(after.duration_seconds, before.duration_seconds);
    assert_eq!(after.frame_rate, before.frame_rate);
    assert_eq!(after.start_seconds, 0.0);
}

fn video_info(container: &str, codec: &str) -> MediaInfo {
    MediaInfo {
        version: "test".into(),
        container: container.into(),
        timecode: None,
        timecode_stream_index: None,
        camera_metadata: Vec::new(),
        video: Some(model::VideoInfo {
            codec: codec.into(),
            width: 16,
            height: 16,
            pixel_format: "yuv420p".into(),
            frame_rate: model::Ratio { num: 30, den: 1 },
            time_base: model::Ratio { num: 1, den: 30 },
            start_seconds: 0.0,
            duration_seconds: 1.0,
            frames: 30,
            constant_frame_rate: true,
            alpha: false,
            interlaced: false,
            sample_aspect_ratio: model::Ratio { num: 1, den: 1 },
            rotation_degrees: 0.0,
            display_matrix: model::identity_display_matrix(),
            color: unknown_color(),
            first_keyframe: true,
            max_keyframe_interval: 30,
        }),
        audio: None,
    }
}

// Only codec/pixel/color/numeric facts from a native camera probe are pinned.
// Timing, rotation and ancillary tracks are deliberately outside this regression.
fn high422_source(container: &str) -> MediaInfo {
    let mut source = video_info(container, "h264");
    let mut video = serde_json::to_value(source.video.as_ref().unwrap()).unwrap();
    let descriptor: serde_json::Value = serde_json::from_str(include_str!(
        "../tests/fixtures/h264_high422_descriptor.json"
    ))
    .unwrap();
    for (key, value) in descriptor.as_object().unwrap() {
        video[key] = value.clone();
    }
    source.video = Some(serde_json::from_value(video).unwrap());
    source
}

#[test]
fn h264_output_rejects_unchanged_high_chroma_or_depth() {
    let source = high422_source("mp4");
    for pixel_format in [
        "yuv422p10le",
        "yuv420p10le",
        "yuv422p",
        "yuv444p",
        "unknown",
    ] {
        let mut output = source.clone();
        output.video.as_mut().unwrap().pixel_format = pixel_format.into();
        assert!(
            validate_output(Profile::H264, &source, &output).is_err(),
            "{pixel_format}"
        );
    }
    for pixel_format in ["yuv420p", "yuvj420p"] {
        let mut output = source.clone();
        output.video.as_mut().unwrap().pixel_format = pixel_format.into();
        validate_output(Profile::H264, &source, &output).unwrap();
    }
}

#[test]
fn a_demuxed_swf_track_does_not_bypass_flash_semantic_assessment() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.swf");
    fs::write(&input, b"ZWS\x0d\x08\x00\x00\x00").unwrap();
    let error = validate_source_file(&input, &video_info("swf", "flv1")).unwrap_err();
    assert!(error
        .to_string()
        .contains("external rendering or assessment"));
    // Other files are independent even when unsupported Flash is nearby.
    let other = directory.path().join("input.mov");
    fs::write(&other, b"another media file").unwrap();
    validate_source_file(&other, &video_info("mov", "qtrle")).unwrap();
}

#[test]
fn canonical_container_requires_a_recognized_major_brand() {
    let raw = "mov,mp4,m4a,3gp,3g2,mj2";
    assert_eq!(model::detected_container(raw, Some("qt  ")), "mov");
    assert_eq!(model::detected_container(raw, Some("isom")), "mp4");
    assert_eq!(model::detected_container(raw, Some("unknown")), raw);
    assert_eq!(model::detected_container(raw, None), raw);
    assert_eq!(model::detected_container("wav", Some("isom")), "wav");
}

#[test]
fn planning_uses_copy_only_for_matching_proven_container() {
    let directory = tempfile::tempdir().unwrap();
    let cancelled = AtomicBool::new(false);
    let backend = Backend::External {
        ffmpeg: directory.path().join("unused"),
        ffprobe: directory.path().join("unused"),
    };
    let source = video_info("mp4", "h264");
    let copy = plan(
        &backend,
        &directory.path().join("out.mp4"),
        &source,
        &cancelled,
    )
    .unwrap();
    assert_eq!(copy.operation, Operation::Copy);

    let mut prores = video_info("mov", "prores");
    let video = prores.video.as_mut().unwrap();
    video.alpha = true;
    video.pixel_format = "gbrap16le".into();
    let copy = plan(
        &backend,
        &directory.path().join("alpha.mov"),
        &prores,
        &cancelled,
    )
    .unwrap();
    assert_eq!(copy.operation, Operation::Copy);
    assert_eq!(copy.encoder, "copy");

    let wav = MediaInfo {
        version: "test".into(),
        container: "wav".into(),
        video: None,
        audio: Some(model::AudioInfo {
            codec: "pcm_s24le".into(),
            sample_format: "s32".into(),
            sample_rate: 48_000,
            channels: 2,
            channel_layout: "stereo".into(),
            start_seconds: 0.0,
            duration_seconds: 1.0,
        }),
        timecode: None,
        timecode_stream_index: None,
        camera_metadata: Vec::new(),
    };
    let copy = plan(
        &backend,
        &directory.path().join("audio.wav"),
        &wav,
        &cancelled,
    )
    .unwrap();
    assert_eq!(copy.operation, Operation::Copy);
    assert_eq!(copy.encoder, "copy");
}

#[cfg(unix)]
mod process_tests {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        sync::{atomic::Ordering, Arc},
        thread,
        time::Duration,
    };

    fn executable(path: &Path, source: &str) {
        fs::write(path, source).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    fn external_backend(directory: &Path, delay: bool) -> Backend {
        let ffprobe = directory.join("ffprobe.sh");
        executable(
            &ffprobe,
            &format!(
                r#"#!/bin/sh
set -eu
case " $* " in
  *" -show_pixel_formats "*) printf '%s\n' '{{"pixel_formats":[{{"name":"rgb24","flags":{{"alpha":0}}}},{{"name":"yuv420p","flags":{{"alpha":0}}}},{{"name":"vuya","flags":{{"alpha":1}}}},{{"name":"rgba64le","flags":{{"alpha":1}}}},{{"name":"rgba64be","flags":{{"alpha":1}}}},{{"name":"bgra64le","flags":{{"alpha":1}}}},{{"name":"bgra64be","flags":{{"alpha":1}}}}]}}' ;;
  *" -show_frames "*) printf '%s\n' 'key_frame=1|best_effort_timestamp=0|best_effort_timestamp_time=0.0' 'key_frame=0|best_effort_timestamp=1|best_effort_timestamp_time=0.033333333' 'key_frame=0|best_effort_timestamp=2|best_effort_timestamp_time=0.066666667' ;;
  *)
    printf '%s\n' probe >> "{count}"
    case " $* " in
      *".mp4"*) printf '%s\n' '{output_doc}' ;;
      *) printf '%s\n' '{source_doc}' ;;
    esac
    ;;
esac
"#,
                count = directory.join("count").display(),
                source_doc = format_args!("{{\"program_version\":{{\"version\":\"mock-7\"}},\"format\":{{\"format_name\":\"avi\",\"duration\":\"1.0\",\"start_time\":\"0.0\"}},\"streams\":[{{\"codec_type\":\"video\",\"codec_name\":\"qtrle\",\"width\":16,\"height\":16,\"pix_fmt\":\"rgb24\",\"r_frame_rate\":\"30/1\",\"avg_frame_rate\":\"30/1\",\"time_base\":\"1/30\",\"start_time\":\"0.0\",\"duration\":\"1.0\",\"nb_frames\":\"3\",\"sample_aspect_ratio\":\"1:1\",\"field_order\":\"progressive\"}}]}}"),
                output_doc = format_args!("{{\"program_version\":{{\"version\":\"mock-7\"}},\"format\":{{\"format_name\":\"mp4\",\"duration\":\"1.0\",\"start_time\":\"0.0\"}},\"streams\":[{{\"codec_type\":\"video\",\"codec_name\":\"h264\",\"width\":16,\"height\":16,\"pix_fmt\":\"yuv420p\",\"r_frame_rate\":\"30/1\",\"avg_frame_rate\":\"30/1\",\"time_base\":\"1/30\",\"start_time\":\"0.0\",\"duration\":\"1.0\",\"nb_frames\":\"3\",\"sample_aspect_ratio\":\"1:1\",\"field_order\":\"progressive\"}}]}}")
            ),
        );
        let ffmpeg = directory.join("ffmpeg.sh");
        let sleep = if delay { "sleep 10" } else { "" };
        executable(
            &ffmpeg,
            &format!(
                r#"#!/bin/sh
set -eu
if [ "${{2:-}}" = "-encoders" ]; then
  printf '%s\n' ' V..... libx264 mock' ' A..... aac mock' ' V..... prores_ks mock' ' A..... pcm_s16le mock'
  exit 0
fi
{sleep}
for argument in "$@"; do output="$argument"; done
input=""
previous=""
for argument in "$@"; do
  if [ "$previous" = "-i" ]; then input="$argument"; fi
  previous="$argument"
done
cp "$input" "$output"
printf '%s\n' 'out_time_us=500000' 'progress=end'
"#
            ),
        );
        Backend::External { ffmpeg, ffprobe }
    }

    fn fixture(delay: bool) -> (tempfile::TempDir, PathBuf, Backend) {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("clip.avi");
        fs::write(&input, b"mock-media").unwrap();
        let backend = external_backend(directory.path(), delay);
        (directory, input, backend)
    }

    fn request<'a>(
        input: &'a Path,
        output: &'a Path,
        backend: Backend,
        cancelled: &'a AtomicBool,
    ) -> TranscodeRequest<'a> {
        TranscodeRequest {
            input,
            output,
            backend,
            cancelled,
        }
    }

    #[test]
    fn h264_planning_normalizes_high422_without_reencoding_eight_bit_420() {
        let (directory, _, backend) = fixture(false);
        let cancelled = AtomicBool::new(false);
        for container in ["mp4", "mov"] {
            for extension in ["mp4", "mov"] {
                let output = directory.path().join(format!("out.{extension}"));
                let mut source = high422_source(container);
                let selected = plan(&backend, &output, &source, &cancelled).unwrap();
                assert_eq!(selected.operation, Operation::Transcode);
                assert_eq!(selected.profile, Profile::H264);
                assert_eq!(selected.encoder, "libx264");
                for pixel_format in ["yuv420p", "yuvj420p"] {
                    source.video.as_mut().unwrap().pixel_format = pixel_format.into();
                    let selected = plan(&backend, &output, &source, &cancelled).unwrap();
                    assert_eq!(
                        selected.operation,
                        if container == extension {
                            Operation::Copy
                        } else {
                            Operation::Remux
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn h264_incompatible_backend_output_is_not_published() {
        let (directory, input, backend) = fixture(false);
        let Backend::External { ffprobe, .. } = &backend else {
            unreachable!();
        };
        let script = fs::read_to_string(ffprobe)
            .unwrap()
            .replace("yuv420p", "yuv422p10le");
        executable(ffprobe, &script);
        let output = directory.path().join("out.mp4");
        let error = run(
            request(&input, &output, backend, &AtomicBool::new(false)),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("8-bit 4:2:0"));
        assert!(!output.exists());
        assert_eq!(fs::read(input).unwrap(), b"mock-media");
    }

    #[test]
    fn review_external_alpha_cannot_publish_opaque_mp4() {
        for pixel_format in ["vuya", "rgba64le", "rgba64be", "bgra64le", "bgra64be"] {
            let (directory, input, backend) = fixture(false);
            let Backend::External { ffprobe, .. } = &backend else {
                unreachable!();
            };
            let script = fs::read_to_string(ffprobe).unwrap();
            // Change source metadata only, retaining the independent descriptor table.
            executable(
                ffprobe,
                &script.replace(
                    "\"pix_fmt\":\"rgb24\"",
                    &format!("\"pix_fmt\":\"{pixel_format}\""),
                ),
            );
            let output = directory.path().join("prepared.mp4");
            let cancelled = AtomicBool::new(false);
            let source = crate::backend::probe(&backend, &input, &cancelled).unwrap();
            assert!(source.video.as_ref().unwrap().alpha);
            let mov_plan = plan(
                &backend,
                &directory.path().join("prepared.mov"),
                &source,
                &cancelled,
            );
            if pixel_format == "vuya" {
                assert_eq!(
                    mov_plan.unwrap().job_encoder().as_deref(),
                    Some("prores_ks")
                );
            } else {
                assert!(
                    mov_plan.is_err(),
                    "high-precision alpha must not use the eight-bit profile"
                );
            }
            let result = run(request(&input, &output, backend, &cancelled), &mut |_| {});
            assert!(
                result.is_err(),
                "alpha format {pixel_format} was accepted for opaque MP4"
            );
            assert!(!output.exists());
            assert_no_staging(directory.path());
        }
    }

    #[test]
    fn review_external_coarse_clock_gap_is_rejected() {
        let (directory, input, backend) = fixture(false);
        let Backend::External { ffprobe, .. } = &backend else {
            unreachable!();
        };
        let script = fs::read_to_string(ffprobe).unwrap();
        executable(
            ffprobe,
            &script
                .replace("best_effort_timestamp=2|", "best_effort_timestamp=3|")
                .replace("0.066666667", "0.100000000"),
        );
        let output = directory.path().join("prepared.mp4");
        let cancelled = AtomicBool::new(false);
        assert!(run(request(&input, &output, backend, &cancelled), &mut |_| {}).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn review_external_exact_90khz_clock_ignores_decimal_rounding() {
        let (_directory, input, backend) = fixture(false);
        let Backend::External { ffprobe, .. } = &backend else {
            unreachable!();
        };
        let script = fs::read_to_string(ffprobe).unwrap();
        let script = script
            .replace("\"time_base\":\"1/30\"", "\"time_base\":\"1/90000\"")
            .replace("\"30/1\"", "\"30000/1001\"")
            .replace("best_effort_timestamp=1|", "best_effort_timestamp=3003|")
            .replace("best_effort_timestamp=2|", "best_effort_timestamp=6006|")
            .replace("0.033333333", "0.033367")
            .replace("0.066666667", "0.066733");
        executable(ffprobe, &script);
        let cancelled = AtomicBool::new(false);
        let source = crate::backend::probe(&backend, &input, &cancelled).unwrap();
        assert!(source.video.unwrap().constant_frame_rate);
        // The same rounded decimal values must not hide a missing whole frame.
        executable(
            ffprobe,
            &script.replace("best_effort_timestamp=6006|", "best_effort_timestamp=9009|"),
        );
        let source = crate::backend::probe(&backend, &input, &cancelled).unwrap();
        assert!(!source.video.unwrap().constant_frame_rate);
    }

    #[test]
    fn camera_metadata_descriptors_are_accepted_by_external_probe() {
        // Derived from native MOV/MP4 ffprobe descriptors, with a generic timecode
        // and no customer payload or identifiers; not camera-payload decoding proof.
        let cases: Vec<Vec<serde_json::Value>> = serde_json::from_str(include_str!(
            "../tests/fixtures/camera_metadata_descriptors.json"
        ))
        .unwrap();
        for streams in cases {
            let tracks = streams
                .iter()
                .map(|stream| format!(",{stream}"))
                .collect::<String>();
            let (_directory, input, backend) = fixture(false);
            let Backend::External { ffprobe, .. } = &backend else {
                unreachable!()
            };
            let script = fs::read_to_string(ffprobe)
                .unwrap()
                .replace(
                    "\"format_name\":\"avi\"",
                    "\"format_name\":\"mov,mp4,m4a,3gp,3g2,mj2\"",
                )
                .replace(
                    "\"field_order\":\"progressive\"}]}",
                    &format!("\"field_order\":\"progressive\"}}{tracks}]}}"),
                );
            executable(ffprobe, &script);
            let source = backend::probe(&backend, &input, &AtomicBool::new(false)).unwrap();
            assert!(source.video.is_some());
            assert!(
                source.timecode.is_none(),
                "rtmd labels must not fabricate tmcd"
            );
        }
    }

    #[test]
    fn camera_metadata_transcode_reports_loss_and_maps_later_timecode() {
        let (directory, input, backend) = fixture(false);
        let Backend::External { ffprobe, ffmpeg } = &backend else {
            unreachable!()
        };
        let script = fs::read_to_string(ffprobe)
            .unwrap()
            .replace("\"format_name\":\"avi\"", "\"format_name\":\"mov,mp4\"")
            .replace("\"format_name\":\"mp4\"", "\"format_name\":\"mov,mp4\"");
        let source_tracks = r#",{"index":2,"codec_type":"data","codec_tag_string":"rtmd","tags":{"timecode":"01:02:03:04"}},{"index":3,"codec_type":"data","codec_tag_string":"mebx"},{"index":4,"codec_type":"data","codec_tag_string":"tmcd","tags":{"timecode":"01:02:03:04"}}"#;
        // Only the source gains camera tracks; output still has its real tmcd.
        let source_marker = "\"pix_fmt\":\"rgb24\"";
        let script = script.lines().map(|line| {
            let tracks = if line.contains(source_marker) { source_tracks } else if line.contains("\"codec_name\":\"h264\"") {
                r#",{"index":1,"codec_type":"data","codec_tag_string":"tmcd","tags":{"timecode":"01:02:03:04"}}"#
            } else { "" };
            line.replace("\"field_order\":\"progressive\"}]}", &format!("\"field_order\":\"progressive\"}}{tracks}]}}"))
        }).collect::<Vec<_>>().join("\n");
        executable(ffprobe, &script);
        let command_log = directory.path().join("args");
        executable(
            ffmpeg,
            &fs::read_to_string(ffmpeg).unwrap().replace(
                "input=\"\"",
                &format!(
                    "printf '%s\\n' \"$@\" > '{}'\ninput=\"\"",
                    command_log.display()
                ),
            ),
        );
        let output = directory.path().join("prepared.mp4");
        let result = run(
            request(&input, &output, backend, &AtomicBool::new(false)),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(result.source.timecode_stream_index, Some(4));
        assert_eq!(result.media.timecode_stream_index, Some(1));
        assert_eq!(result.warnings.len(), 2);
        assert!(result.warnings[0]
            .contains("rtmd camera metadata stream 2 including its timecode label"));
        assert!(result.warnings[1].contains("mebx camera metadata stream 3"));
        let args = fs::read_to_string(command_log).unwrap();
        assert!(
            args.contains("-map\n0:4\n-c:d\ncopy\n-map_metadata:s:d:0\n0:s:4\n"),
            "{args}"
        );
        assert!(!args.contains("0:d:0"));
        assert!(metadata_omission_warnings(&result.source, Operation::Copy).is_empty());
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["warnings"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn camera_metadata_compatible_copy_retains_bytes_without_loss_warning() {
        let (directory, input, backend) = fixture(false);
        let Backend::External { ffprobe, .. } = &backend else {
            unreachable!()
        };
        let script = fs::read_to_string(ffprobe).unwrap()
            .replace("\"format_name\":\"avi\"", "\"format_name\":\"mov,mp4\",\"tags\":{\"major_brand\":\"qt  \"}")
            .replace("\"codec_name\":\"qtrle\"", "\"codec_name\":\"h264\"")
            .replace("\"pix_fmt\":\"rgb24\"", "\"pix_fmt\":\"yuv420p\"")
            .replace("\"field_order\":\"progressive\"}]}", r#""field_order":"progressive"},{"index":2,"codec_type":"data","codec_tag_string":"rtmd","tags":{"timecode":"01:02:03:04"}}]}"#);
        executable(ffprobe, &script);
        let output = directory.path().join("prepared.mov");
        let result = run(
            request(&input, &output, backend, &AtomicBool::new(false)),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(result.operation, Operation::Copy);
        assert_eq!(result.input_sha256, result.output_sha256);
        assert_eq!(result.source.camera_metadata, result.media.camera_metadata);
        assert_eq!(result.media.camera_metadata.len(), 1);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn camera_metadata_does_not_admit_other_streams() {
        for extra in [
            r#",{"index":3,"codec_type":"data","codec_tag_string":"zzzz"}"#,
            r#",{"index":3,"codec_type":"subtitle"}"#,
            r#",{"index":3,"codec_type":"attachment"}"#,
            r#",{"index":3,"codec_type":"video"}"#,
            r#",{"index":3,"codec_type":"audio"},{"index":4,"codec_type":"audio"}"#,
            r#",{"index":3,"codec_type":"data","codec_tag_string":"tmcd"}"#,
            r#",{"index":3,"codec_type":"data","codec_tag_string":"tmcd","tags":{"timecode":"01:02:03:04"}},{"index":4,"codec_type":"data","codec_tag_string":"tmcd","tags":{"timecode":"01:02:03:04"}}"#,
        ] {
            let (_directory, input, backend) = fixture(false);
            let Backend::External { ffprobe, .. } = &backend else {
                unreachable!()
            };
            let script = fs::read_to_string(ffprobe).unwrap()
                .replace("\"format_name\":\"avi\"", "\"format_name\":\"mov,mp4\"")
                .replace("\"field_order\":\"progressive\"}]}", &format!(r#""field_order":"progressive"}},{{"index":2,"codec_type":"data","codec_tag_string":"mebx"}}{extra}]}}"#));
            executable(ffprobe, &script);
            assert!(
                backend::probe(&backend, &input, &AtomicBool::new(false)).is_err(),
                "{extra}"
            );
        }
    }

    #[test]
    fn ae_destination_rejects_external_backend_before_any_tool_runs() {
        let (directory, input, backend) = fixture(false);
        let output = directory.path().join("prepared.mov");
        let cancelled = AtomicBool::new(false);
        let error =
            run_for_after_effects(request(&input, &output, backend, &cancelled), &mut |_| {
                panic!("rejected backend must not start preparation")
            })
            .unwrap_err();
        assert!(
            matches!(error, TranscodeError::Policy(ref reason) if reason.contains("library backend"))
        );
        assert!(!directory.path().join("count").exists());
        assert!(!output.exists());
        assert!(!fs::read_dir(directory.path()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".media-transcode-")));
    }

    #[test]
    fn external_remux_pins_timescale_but_encoding_keeps_its_own_clock() {
        let (directory, input, backend) = fixture(false);
        let Backend::External { ffmpeg, .. } = &backend else {
            unreachable!()
        };
        let arguments = directory.path().join("arguments");
        executable(
            ffmpeg,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                arguments.display()
            ),
        );
        for profile in [Profile::RemuxVideo, Profile::H264] {
            let mut source = video_info("mov", "h264");
            source.video.as_mut().unwrap().time_base = native_coarse_clock().native_time_base;
            let job = model::Job {
                input: input.clone(),
                output: directory.path().join("prepared.mp4"),
                profile,
                encoder: None,
                source,
                destination: model::Destination::General,
            };
            crate::backend::transcode(&backend, &job, &AtomicBool::new(false), &mut |_| {})
                .unwrap();
            let args = fs::read_to_string(&arguments).unwrap();
            assert_eq!(
                args.contains("-video_track_timescale\n600\n"),
                profile == Profile::RemuxVideo
            );
        }
    }

    #[test]
    fn remux_jobs_select_no_encoder() {
        let directory = tempfile::tempdir().unwrap();
        let backend = external_backend(directory.path(), false);
        let cancelled = AtomicBool::new(false);
        for (container, output) in [("mp4", "out.mov"), ("mov", "out.mp4")] {
            let remux = plan(
                &backend,
                &directory.path().join(output),
                &video_info(container, "h264"),
                &cancelled,
            )
            .unwrap();
            assert_eq!(remux.operation, Operation::Remux);
            // The library backend rejects any encoder for a stream-copy remux.
            assert_eq!(remux.job_encoder(), None);
        }
        let encode = plan(
            &backend,
            &directory.path().join("encoded.mp4"),
            &video_info("mov", "prores"),
            &cancelled,
        )
        .unwrap();
        assert_eq!(encode.operation, Operation::Transcode);
        assert_eq!(encode.job_encoder().as_deref(), Some("libx264"));
    }

    #[test]
    fn single_file_transcode_reports_progress_and_publishes() {
        let (directory, input, backend) = fixture(false);
        let output = directory.path().join("prepared.mp4");
        let cancelled = AtomicBool::new(false);
        let mut statuses = Vec::new();
        let result = run(
            request(&input, &output, backend, &cancelled),
            &mut |value| statuses.push(value.status),
        )
        .unwrap();
        assert_eq!(result.operation, Operation::Transcode);
        assert_eq!(result.input, fs::canonicalize(input).unwrap());
        assert!(output.is_file());
        assert_eq!(statuses.last(), Some(&ProgressStatus::Complete));
    }

    #[test]
    fn failed_second_invocation_preserves_first_result() {
        let (directory, input, backend) = fixture(false);
        let first = directory.path().join("first.mp4");
        let cancelled = AtomicBool::new(false);
        run(request(&input, &first, backend, &cancelled), &mut |_| {}).unwrap();
        let expected = fs::read(&first).unwrap();
        let error = run(
            request(
                &input,
                &first,
                external_backend(directory.path(), false),
                &cancelled,
            ),
            &mut |_| {},
        )
        .unwrap_err();
        // The standalone command keeps its established policy classification.
        assert!(
            matches!(error, TranscodeError::Policy(ref reason) if reason.contains("already exists"))
        );
        assert_eq!(fs::read(first).unwrap(), expected);
    }

    #[test]
    fn publication_race_preserves_foreign_output() {
        let (directory, input, backend) = fixture(false);
        let output = directory.path().join("prepared.mp4");
        let cancelled = AtomicBool::new(false);
        let mut inserted = false;
        let error = run(
            request(&input, &output, backend, &cancelled),
            &mut |progress| {
                if progress.status == ProgressStatus::Finishing && !inserted {
                    fs::write(&output, b"foreign").unwrap();
                    inserted = true;
                }
            },
        )
        .unwrap_err();
        assert!(matches!(error, TranscodeError::Io { .. }));
        assert_eq!(fs::read(output).unwrap(), b"foreign");
    }

    #[test]
    fn source_mutation_is_rejected_and_staging_is_cleaned() {
        let (directory, input, backend) = fixture(false);
        let output = directory.path().join("prepared.mp4");
        let cancelled = AtomicBool::new(false);
        let mut changed = false;
        let error = run(
            request(&input, &output, backend, &cancelled),
            &mut |progress| {
                if progress.status == ProgressStatus::Finishing && !changed {
                    fs::write(&input, b"changed").unwrap();
                    changed = true;
                }
            },
        )
        .unwrap_err();
        assert!(
            matches!(error, TranscodeError::Policy(ref reason) if reason.contains("source media changed"))
        );
        assert!(!output.exists());
        assert_no_staging(directory.path());
    }

    #[test]
    fn cancellation_kills_backend_and_cleans_staging() {
        let (directory, input, backend) = fixture(true);
        let output = directory.path().join("prepared.mp4");
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancelled);
        let interrupter = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::Relaxed);
        });
        let error = run(request(&input, &output, backend, &cancelled), &mut |_| {}).unwrap_err();
        interrupter.join().unwrap();
        assert!(matches!(error, TranscodeError::Cancelled));
        assert!(!output.exists());
        assert_no_staging(directory.path());
    }

    fn assert_no_staging(directory: &Path) {
        assert!(fs::read_dir(directory).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".media-transcode-")));
    }
}

#[test]
fn quarter_turn_display_matrix_policy_and_output_preservation() {
    let mut source = video_info("mov", "h264");
    let video = source.video.as_mut().unwrap();
    video.width = 1920;
    video.height = 1080;
    video.display_matrix = [0, 65_536, 0, -65_536, 0, 0, 70_778_880, 0, 1 << 30];
    video.rotation_degrees = -90.0;
    validate_source(&source, false).unwrap();
    validate_output(Profile::RemuxVideo, &source, &source).unwrap();
    let mut output = source.clone();
    output.video.as_mut().unwrap().display_matrix[6] = 0;
    assert!(validate_output(Profile::RemuxVideo, &source, &output).is_err());
    output.video.as_mut().unwrap().display_matrix = model::identity_display_matrix();
    output.video.as_mut().unwrap().rotation_degrees = 0.0;
    assert!(validate_output(Profile::RemuxVideo, &source, &output).is_err());
    source.video.as_mut().unwrap().rotation_degrees = f64::NAN;
    assert!(validate_source(&source, false).is_err());
}

#[test]
fn display_matrix_accepts_only_unit_quarter_turns_and_canonical_translations() {
    let w = 1920;
    let h = 1080;
    for (linear, translation, angle) in [
        ([65_536, 0, 0, 65_536], [0, 0], 0.0),
        ([0, 65_536, -65_536, 0], [h * 65_536, 0], -90.0),
        ([-65_536, 0, 0, -65_536], [w * 65_536, h * 65_536], -180.0),
        ([0, -65_536, 65_536, 0], [0, w * 65_536], 90.0),
    ] {
        for [x, y] in [[0, 0], translation] {
            let matrix = [
                linear[0],
                linear[1],
                0,
                linear[2],
                linear[3],
                0,
                x,
                y,
                1 << 30,
            ];
            assert_eq!(
                model::display_matrix_rotation(&matrix, w as u32, h as u32),
                Some(angle)
            );
            for index in 0..9 {
                let mut invalid = matrix;
                invalid[index] += 1;
                assert_eq!(
                    model::display_matrix_rotation(&invalid, w as u32, h as u32),
                    None
                );
            }
        }
    }
    for linear in [
        [-65_536, 0, 0, 65_536],
        [131_072, 0, 0, 131_072],
        [65_536, 1, 0, 65_536],
        [46_341, 46_341, -46_341, 46_341],
    ] {
        let matrix = [
            linear[0],
            linear[1],
            0,
            linear[2],
            linear[3],
            0,
            0,
            0,
            1 << 30,
        ];
        assert_eq!(
            model::display_matrix_rotation(&matrix, w as u32, h as u32),
            None
        );
    }
}

#[test]
fn legacy_video_info_defaults_to_identity_display_matrix() {
    let info = video_info("mp4", "h264");
    let mut json = serde_json::to_value(&info).unwrap();
    json["video"]
        .as_object_mut()
        .unwrap()
        .remove("display_matrix");
    let decoded: MediaInfo = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, info);
}

#[test]
fn after_effects_preparation_keeps_unrotated_contract() {
    let mut source = video_info("mp4", "h264");
    let video = source.video.as_mut().unwrap();
    video.display_matrix = [0, 65_536, 0, -65_536, 0, 0, 0, 0, 1 << 30];
    video.rotation_degrees = -90.0;
    validate_source(&source, false).unwrap();
    let input = Path::new("/not-opened.mp4");
    let request = TranscodeRequest {
        input,
        output: Path::new("/not-created.mov"),
        backend: Backend::Library,
        cancelled: &AtomicBool::new(false),
    };
    let error = plan_after_effects(&request, input, &source).unwrap_err();
    assert!(
        matches!(error, TranscodeError::Policy(ref reason) if reason.contains("identity display matrix"))
    );
}

/// Numeric-only descriptors reduced from native media probes; no customer identifiers.
#[derive(serde::Deserialize)]
pub(super) struct NativeQuarterTurnDescriptor {
    pub width: u32,
    pub height: u32,
    pub display_matrix: [i32; 9],
    pub rotation_degrees: f64,
}

pub(super) fn native_quarter_turn_descriptors() -> Vec<NativeQuarterTurnDescriptor> {
    serde_json::from_str(include_str!(
        "../tests/fixtures/native_quarter_turn_descriptors.json"
    ))
    .unwrap()
}

#[test]
fn implicit_mov_pcm_stereo_preserves_audio_validation() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/implicit_pcm_stereo.json")).unwrap();
    let mut source = video_info(fixture["container"].as_str().unwrap(), "h264");
    let mut audio: model::AudioInfo = serde_json::from_value(fixture["audio"].clone()).unwrap();
    audio.channel_layout = model::canonical_audio_layout(
        &source.container,
        &audio.codec,
        &audio.channel_layout,
        audio.channels,
    );
    source.audio = Some(audio);
    let mut output = source.clone();
    output.audio.as_mut().unwrap().channel_layout = "stereo".into();
    validate_output(Profile::H264, &source, &output).unwrap();
    for changed in ["layout", "duration", "start", "rate", "channels"] {
        let mut invalid = output.clone();
        let audio = invalid.audio.as_mut().unwrap();
        match changed {
            "layout" => audio.channel_layout = "downmix".into(),
            "duration" => audio.duration_seconds += 2.0 / 48_000.0,
            "start" => audio.start_seconds = 0.001,
            "rate" => audio.sample_rate = 44_100,
            "channels" => audio.channels = 1,
            _ => unreachable!(),
        }
        assert!(validate_output(Profile::H264, &source, &invalid).is_err());
    }
}

#[test]
fn implicit_pcm_layout_inference_is_bounded() {
    for container in ["mov", "mp4", "mov,mp4,m4a,3gp,3g2,mj2"] {
        for (channels, unspecified, expected) in
            [(1, "1 channels", "mono"), (2, "2 channels", "stereo")]
        {
            for layout in ["", "unknown", unspecified] {
                assert_eq!(
                    model::canonical_audio_layout(container, "pcm_s16be", layout, channels),
                    expected
                );
            }
        }
        for (codec, layout, channels) in [
            ("pcm_s16be", "", 6),
            ("pcm_s16be", "6 channels", 6),
            ("pcm_s16be", "downmix", 2),
            ("pcm_s16be", "2 channels (FR+FL)", 2),
            ("pcm_s16be", "1 channels", 2),
            ("aac", "unknown", 2),
        ] {
            assert_eq!(
                model::canonical_audio_layout(container, codec, layout, channels),
                layout
            );
        }
    }
    assert_eq!(
        model::canonical_audio_layout("matroska", "pcm_s16be", "unknown", 2),
        "unknown"
    );
}
