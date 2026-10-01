//! Safe single-file media transcoding with library and external-command backends.

mod backend;
pub mod inspect;
pub mod model;
#[cfg(feature = "ffmpeg-library")]
mod native;
mod process;

use fx_conv::sha256_file;
use model::{Job, MediaInfo, Profile};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// The selected backend is fixed for the complete run; failures never fall back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// Link and call FFmpeg directly; requires the `ffmpeg-library` feature.
    Library,
    /// Spawn only the explicitly selected FFmpeg and FFprobe executables.
    External { ffmpeg: PathBuf, ffprobe: PathBuf },
}

/// Inputs borrowed for one synchronous single-file operation.
pub struct TranscodeRequest<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub backend: Backend,
    pub cancelled: &'a AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStatus {
    Unknown,
    Encoding,
    Finishing,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub source: PathBuf,
    pub processed_seconds: Option<f64>,
    pub total_seconds: Option<f64>,
    pub speed: Option<f64>,
    pub eta_seconds: Option<f64>,
    pub status: ProgressStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Copy,
    Remux,
    Transcode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodeResult {
    pub input: PathBuf,
    pub output: PathBuf,
    pub input_sha256: String,
    pub output_sha256: String,
    pub backend: String,
    pub operation: Operation,
    pub encoder: String,
    pub source: MediaInfo,
    pub media: MediaInfo,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("media policy rejected the operation: {0}")]
    Policy(String),
    #[error("backend failed: {stderr}")]
    Backend { stderr: String },
    #[error("backend protocol error: {0}")]
    Protocol(String),
    #[error("failed to spawn {command}: {source}")]
    Spawn {
        command: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to wait for backend process: {0}")]
    Wait(#[source] io::Error),
    #[error("failed to read backend process output: {0}")]
    ReadChild(#[source] io::Error),
    #[error("I/O at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid backend JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("media identity error: {0}")]
    MediaMap(#[from] fx_conv::MediaMapError),
}

#[derive(Debug)]
struct Plan {
    profile: Profile,
    operation: Operation,
    encoder: String,
}

impl Plan {
    /// Encoder for the backend job. Stream-copy remux selects no encoder.
    fn job_encoder(&self) -> Option<String> {
        (self.profile != Profile::RemuxVideo).then(|| self.encoder.clone())
    }
}

struct Staging {
    directory: PathBuf,
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Transform one media file, validate it, and atomically publish without replacement.
pub fn run(
    request: TranscodeRequest<'_>,
    progress: &mut dyn FnMut(&Progress),
) -> Result<TranscodeResult, TranscodeError> {
    check_cancel(request.cancelled)?;
    validate_paths(request.input, request.output)?;
    ensure_fresh_destination(request.output)?;

    let input =
        fs::canonicalize(request.input).map_err(|source| io_error(request.input, source))?;
    let input_sha256 = sha256_file(&input)?;
    let source = backend::probe(&request.backend, &input, request.cancelled)?;
    validate_source_file(&input, &source)?;
    validate_source(&source)?;
    let plan = plan(&request.backend, request.output, &source, request.cancelled)?;

    let parent = request.output.parent().unwrap_or_else(|| Path::new("."));
    let directory = parent.join(format!(".media-transcode-{}", uuid::Uuid::new_v4()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&directory)
        .map_err(|source| io_error(&directory, source))?;
    let staging = Staging { directory };
    let suffix = request
        .output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| TranscodeError::Policy("output has no supported suffix".into()))?;
    let staged_output = staging.directory.join(format!("output.{suffix}"));

    emit_status(progress, &input, ProgressStatus::Unknown);
    if plan.operation == Operation::Copy {
        fs::copy(&input, &staged_output).map_err(|source| io_error(&staged_output, source))?;
    } else {
        let job = Job {
            input: input.clone(),
            output: staged_output.clone(),
            profile: plan.profile,
            source: source.clone(),
            encoder: plan.job_encoder(),
        };
        let started = Instant::now();
        let duration = media_duration(&source);
        backend::transcode(
            &request.backend,
            &job,
            request.cancelled,
            &mut |processed| {
                let elapsed = started.elapsed().as_secs_f64();
                let speed = (elapsed > 0.0).then_some(processed / elapsed);
                let finishing = processed >= duration;
                let eta_seconds = (!finishing)
                    .then(|| {
                        speed
                            .filter(|value| *value > 0.0)
                            .map(|value| (duration - processed) / value)
                    })
                    .flatten();
                progress(&Progress {
                    source: input.clone(),
                    processed_seconds: Some(processed),
                    total_seconds: Some(duration),
                    speed,
                    eta_seconds,
                    status: if finishing {
                        ProgressStatus::Finishing
                    } else {
                        ProgressStatus::Encoding
                    },
                });
            },
        )?;
    }

    check_cancel(request.cancelled)?;
    emit_status(progress, &input, ProgressStatus::Finishing);
    let media = backend::probe(&request.backend, &staged_output, request.cancelled)?;
    let output_sha256 = sha256_file(&staged_output)?;
    if plan.operation == Operation::Copy {
        if output_sha256 != input_sha256 {
            return Err(TranscodeError::Policy(
                "byte copy changed source bytes".into(),
            ));
        }
    } else {
        validate_output(plan.profile, &source, &media)?;
    }
    if source.timecode != media.timecode {
        return Err(TranscodeError::Policy(
            "transcode changed recognized timecode metadata".into(),
        ));
    }
    verify_source_freshness(&input, &input_sha256)?;
    check_cancel(request.cancelled)?;
    publish_no_replace(&staged_output, request.output)?;
    emit_status(progress, &input, ProgressStatus::Complete);

    Ok(TranscodeResult {
        input,
        output: request.output.to_owned(),
        input_sha256,
        output_sha256,
        backend: backend_name(&request.backend).into(),
        operation: plan.operation,
        encoder: plan.encoder,
        source,
        media,
    })
}

fn validate_paths(input: &Path, output: &Path) -> Result<(), TranscodeError> {
    if !input.is_absolute() || !output.is_absolute() {
        return Err(TranscodeError::Policy(
            "input and output must be absolute paths".into(),
        ));
    }
    if input == output {
        return Err(TranscodeError::Policy(
            "input and output must be different files".into(),
        ));
    }
    Ok(())
}

fn plan(
    backend: &Backend,
    output: &Path,
    source: &MediaInfo,
    cancelled: &AtomicBool,
) -> Result<Plan, TranscodeError> {
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| TranscodeError::Policy("output has no supported suffix".into()))?;
    let source_container = source.container.to_ascii_lowercase();

    let (profile, operation) = match (&source.video, &source.audio, extension.as_str()) {
        (None, Some(audio), "wav")
            if source_container == "wav" && audio.codec.starts_with("pcm_") =>
        {
            (Profile::AudioPcm, Operation::Copy)
        }
        (None, Some(_), "wav") => (Profile::AudioPcm, Operation::Transcode),
        (Some(video), _, "mp4") if video.alpha => {
            return Err(TranscodeError::Policy(
                "alpha video requires a .mov ProRes 4444 output".into(),
            ))
        }
        (Some(video), _, "mp4") if compatible_video_codec("mp4", &video.codec) => {
            let operation = if source_container == "mp4" {
                Operation::Copy
            } else {
                Operation::Remux
            };
            (Profile::RemuxVideo, operation)
        }
        (Some(_), _, "mp4") => (Profile::H264, Operation::Transcode),
        (Some(video), _, "mov")
            if video.alpha && source_container == "mov" && video.codec == "prores" =>
        {
            (Profile::Prores4444, Operation::Copy)
        }
        (Some(video), _, "mov") if video.alpha && !video.has_eight_bit_alpha() => {
            return Err(TranscodeError::Policy(
                "alpha encoding preserves only verified 8-bit alpha formats".into(),
            ))
        }
        (Some(video), _, "mov") if video.alpha => (Profile::Prores4444, Operation::Transcode),
        (Some(video), _, "mov") if compatible_video_codec("mov", &video.codec) => {
            let operation = if source_container == "mov" {
                Operation::Copy
            } else {
                Operation::Remux
            };
            (Profile::RemuxVideo, operation)
        }
        (Some(_), _, "mov") => (Profile::H264, Operation::Transcode),
        (None, Some(_), _) => {
            return Err(TranscodeError::Policy(
                "standalone audio output must use the .wav suffix".into(),
            ))
        }
        (None, None, _) => {
            return Err(TranscodeError::Policy(
                "source has no audio or video stream".into(),
            ))
        }
        _ => {
            return Err(TranscodeError::Policy(
                "video output must use .mp4 or .mov".into(),
            ))
        }
    };

    if operation == Operation::Copy {
        return Ok(Plan {
            profile,
            operation,
            encoder: "copy".into(),
        });
    }
    let capabilities = backend::capabilities(backend, cancelled)?;
    let encoder = select_encoder(backend, source, profile, &capabilities)?;
    Ok(Plan {
        profile,
        operation,
        encoder,
    })
}

fn compatible_video_codec(container: &str, codec: &str) -> bool {
    match container {
        "mp4" => matches!(codec, "h264" | "hevc" | "av1" | "mpeg4"),
        "mov" => matches!(codec, "h264" | "hevc" | "prores" | "mpeg4"),
        _ => false,
    }
}

fn validate_source_file(input: &Path, info: &MediaInfo) -> Result<(), TranscodeError> {
    // A demuxed video track does not establish that it represents a Flash scene.
    // Classify only this invocation's input, never other project references.
    if info.container.split(',').any(|name| name == "swf") {
        match fx_conv::classify_swf(input).map_err(|source| io_error(input, source))? {
            fx_conv::SwfClassification::EmbeddedVideoCandidate => {}
            fx_conv::SwfClassification::ExternalRenderRequired { reason }
            | fx_conv::SwfClassification::Unassessed { reason } => {
                return Err(TranscodeError::Policy(format!(
                    "SWF requires external rendering or assessment: {reason}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_source(info: &MediaInfo) -> Result<(), TranscodeError> {
    if info.video.is_none() && info.audio.is_none() {
        return Err(TranscodeError::Policy(
            "source has no audio or video stream".into(),
        ));
    }
    if let Some(video) = &info.video {
        if !video.constant_frame_rate
            || video.interlaced
            || video.sample_aspect_ratio.num != video.sample_aspect_ratio.den
            || video.rotation_degrees.abs() > 1e-6
            || video.start_seconds.abs() > 1e-6
        {
            return Err(TranscodeError::Policy("VFR, interlace, non-square pixels, rotation, or nonzero start is not safely normalized".into()));
        }
        if video.duration_seconds <= 0.0
            || video.frames == 0
            || video.frame_rate.num <= 0
            || video.frame_rate.den <= 0
        {
            return Err(TranscodeError::Policy(
                "video timing is unknown or invalid".into(),
            ));
        }
        if video.color.space == "gbr" {
            return Err(TranscodeError::Policy(
                "explicit RGB identity-matrix signaling has no verified YUV mapping".into(),
            ));
        }
        if matches!(video.color.transfer.as_str(), "smpte2084" | "arib-std-b67")
            || matches!(video.color.primaries.as_str(), "bt2020" | "smpte432")
        {
            return Err(TranscodeError::Policy(
                "HDR/wide-gamut conversion has no approved preservation path".into(),
            ));
        }
    }
    if let Some(audio) = &info.audio {
        if audio.duration_seconds <= 0.0
            || audio.sample_rate == 0
            || audio.channels == 0
            || audio.start_seconds.abs() > 1e-6
        {
            return Err(TranscodeError::Policy(
                "audio timing or layout is unknown or unsupported".into(),
            ));
        }
        if audio.channels > 2 && audio.channel_layout == "unknown" {
            return Err(TranscodeError::Policy(
                "multichannel audio with unknown layout cannot be preserved".into(),
            ));
        }
    }
    Ok(())
}

fn validate_output(
    profile: Profile,
    source: &MediaInfo,
    output: &MediaInfo,
) -> Result<(), TranscodeError> {
    let source_video = source.video.as_ref();
    let output_video = output.video.as_ref();
    if source_video.is_some() != output_video.is_some()
        || source.audio.is_some() != output.audio.is_some()
    {
        return Err(TranscodeError::Policy(
            "transcode changed the stream layout".into(),
        ));
    }
    if let (Some(source), Some(output)) = (source_video, output_video) {
        let frame_seconds = f64::from(source.frame_rate.den) / f64::from(source.frame_rate.num);
        if source.width != output.width
            || source.height != output.height
            || source.frame_rate != output.frame_rate
            || source.frames != output.frames
            || (source.duration_seconds - output.duration_seconds).abs() > frame_seconds
            || output.start_seconds.abs() > 1e-6
        {
            return Err(TranscodeError::Policy(
                "transcode changed video dimensions, frame rate, frame count, duration, or start"
                    .into(),
            ));
        }
        if source.color != output.color && source.color != unknown_color() {
            return Err(TranscodeError::Policy(
                "transcode changed asserted color metadata".into(),
            ));
        }
        match profile {
            Profile::RemuxVideo if output.codec != source.codec => {
                return Err(TranscodeError::Policy("remux changed video codec".into()))
            }
            Profile::H264
                if output.codec != "h264"
                    || output.alpha
                    || !output.first_keyframe
                    || output.max_keyframe_interval > gop_limit(output) =>
            {
                return Err(TranscodeError::Policy(
                    "H.264 output violates alpha/keyframe/GOP policy".into(),
                ))
            }
            Profile::Prores4444 if output.codec != "prores" || !output.alpha => {
                return Err(TranscodeError::Policy(
                    "ProRes 4444 output did not preserve alpha".into(),
                ))
            }
            _ => {}
        }
    }
    if let (Some(source), Some(output)) = (&source.audio, &output.audio) {
        let tolerance = 1.0 / f64::from(source.sample_rate);
        if source.sample_rate != output.sample_rate
            || source.channels != output.channels
            || source.channel_layout != output.channel_layout
            || (source.duration_seconds - output.duration_seconds).abs() > tolerance
            || output.start_seconds.abs() > 1e-6
        {
            return Err(TranscodeError::Policy(
                "transcode changed audio timing or layout".into(),
            ));
        }
        if profile == Profile::AudioPcm {
            let expected_codec = backend::pcm_encoder(source)?;
            if output.codec != expected_codec
                || !pcm_sample_format_matches(expected_codec, &output.sample_format)
            {
                return Err(TranscodeError::Policy(
                    "standalone audio output changed sample precision".into(),
                ));
            }
        }
    }
    Ok(())
}

fn pcm_sample_format_matches(encoder: &str, sample_format: &str) -> bool {
    let sample_format = sample_format.trim_end_matches('p');
    matches!(
        (encoder, sample_format),
        ("pcm_s16le", "s16") | ("pcm_s32le", "s32") | ("pcm_f32le", "flt")
    )
}

fn unknown_color() -> model::ColorInfo {
    model::ColorInfo {
        range: "unknown".into(),
        space: "unknown".into(),
        transfer: "unknown".into(),
        primaries: "unknown".into(),
    }
}

fn verify_source_freshness(input: &Path, expected_hash: &str) -> Result<(), TranscodeError> {
    if sha256_file(input)? != expected_hash {
        return Err(TranscodeError::Policy(format!(
            "source media changed during preparation: {input:?}"
        )));
    }
    Ok(())
}

fn ensure_fresh_destination(output: &Path) -> Result<(), TranscodeError> {
    match fs::symlink_metadata(output) {
        Ok(_) => Err(TranscodeError::Policy(format!(
            "output already exists: {output:?}"
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error(output, source)),
    }
}

fn publish_no_replace(staging: &Path, output: &Path) -> Result<(), TranscodeError> {
    fs::hard_link(staging, output).map_err(|source| io_error(output, source))
}

fn check_cancel(cancelled: &AtomicBool) -> Result<(), TranscodeError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(TranscodeError::Cancelled)
    } else {
        Ok(())
    }
}

fn select_encoder(
    backend: &Backend,
    source: &MediaInfo,
    profile: Profile,
    capabilities: &model::Capabilities,
) -> Result<String, TranscodeError> {
    let has = |name: &str| capabilities.encoders.iter().any(|encoder| encoder == name);
    let require = |name: &str| {
        has(name).then(|| name.to_owned()).ok_or_else(|| {
            TranscodeError::Policy(format!("selected backend lacks required encoder {name}"))
        })
    };
    match profile {
        Profile::RemuxVideo => Ok("copy".into()),
        Profile::H264 => match backend {
            Backend::External { .. } => require("libx264"),
            Backend::Library => ["h264_videotoolbox", "h264_mf"]
                .into_iter()
                .find(|name| has(name))
                .map(str::to_owned)
                .ok_or_else(|| {
                    TranscodeError::Policy(
                        "selected native backend has no approved H.264 encoder".into(),
                    )
                }),
        },
        Profile::Prores4444 => require("prores_ks"),
        Profile::AudioPcm => {
            require(backend::pcm_encoder(source.audio.as_ref().ok_or_else(
                || TranscodeError::Policy("PCM profile requires audio".into()),
            )?)?)
        }
    }
}

fn emit_status(progress: &mut dyn FnMut(&Progress), source: &Path, status: ProgressStatus) {
    progress(&Progress {
        source: source.to_owned(),
        processed_seconds: None,
        total_seconds: None,
        speed: None,
        eta_seconds: None,
        status,
    });
}

fn io_error(path: &Path, source: io::Error) -> TranscodeError {
    TranscodeError::Io {
        path: path.to_owned(),
        source,
    }
}

fn backend_name(backend: &Backend) -> &'static str {
    match backend {
        Backend::Library => "library",
        Backend::External { .. } => "external-ffmpeg-command",
    }
}

fn media_duration(info: &MediaInfo) -> f64 {
    info.video
        .as_ref()
        .map(|value| value.duration_seconds)
        .or_else(|| info.audio.as_ref().map(|value| value.duration_seconds))
        .unwrap_or(0.0)
}

fn gop_limit(video: &model::VideoInfo) -> u64 {
    ((f64::from(video.frame_rate.num) / f64::from(video.frame_rate.den)).ceil() as u64).max(1)
}

#[cfg(test)]
mod tests;
