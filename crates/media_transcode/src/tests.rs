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

#[test]
fn output_validation_rejects_one_missing_frame_with_matching_duration() {
    let source = video_info("mov", "h264");
    let mut output = source.clone();
    output.video.as_mut().unwrap().frames = 29;
    let error = validate_output(Profile::H264, &source, &output).unwrap_err();
    assert!(error.to_string().contains("frame count"));
}

fn video_info(container: &str, codec: &str) -> MediaInfo {
    MediaInfo {
        version: "test".into(),
        container: container.into(),
        timecode: None,
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
            color: unknown_color(),
            first_keyframe: true,
            max_keyframe_interval: 30,
        }),
        audio: None,
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
  *" -show_frames "*) printf '%s\n' 'key_frame=1|best_effort_timestamp_time=0.0' 'key_frame=0|best_effort_timestamp_time=0.033333333' 'key_frame=0|best_effort_timestamp_time=0.066666667' ;;
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
        assert!(error.to_string().contains("already exists"));
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
        assert!(error.to_string().contains("source media changed"));
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
