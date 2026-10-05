//! Safe single-file media transcoding with library and external-command backends.

mod backend;
pub mod inspect;
pub mod model;
#[cfg(feature = "ffmpeg-library")]
mod native;
mod process;

use fx_conv::sha256_file;
use model::{Destination, Job, MediaInfo, Profile};
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
    /// Explicit losses in the prepared file; the source remains untouched.
    #[serde(default)]
    pub warnings: Vec<String>,
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
    run_for_destination(request, Destination::General, progress, false)
}

/// Prepares a single audio-only MP3 as PCM WAVE for an AE source occurrence.
/// Unlike ordinary `run`, accepts only bounded MP3 decoder priming on the
/// in-process FFmpeg backend; output samples are re-probed before publication.
/// This is not an Adobe audio-fidelity or edit-clock guarantee.
pub fn run_for_after_effects_audio(
    request: TranscodeRequest<'_>,
    progress: &mut dyn FnMut(&Progress),
) -> Result<TranscodeResult, TranscodeError> {
    if request.backend != Backend::Library
        || !request
            .output
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    {
        return Err(TranscodeError::Policy(
            "AE MP3 preparation requires the library backend and a .wav output".into(),
        ));
    }
    run_for_destination(request, Destination::General, progress, true)
}

/// Prepare a whole video source for the bounded native AE QuickTime profile.
///
/// This preserves the existing source safety policy and never drops audio or
/// timecode tracks to manufacture admission. The project-aware caller must
/// revalidate the prepared bytes with its ordinary native metadata parser. Only
/// the library backend supports this destination's sample-entry contract.
///
/// `Policy` reports only an unsupported request or source. An existing
/// destination, an unavailable encoder and a source change before publication
/// are operational errors.
pub fn run_for_after_effects(
    request: TranscodeRequest<'_>,
    progress: &mut dyn FnMut(&Progress),
) -> Result<TranscodeResult, TranscodeError> {
    run_for_destination(request, Destination::AfterEffects, progress, false)
}

fn run_for_destination(
    request: TranscodeRequest<'_>,
    destination: Destination,
    progress: &mut dyn FnMut(&Progress),
    ae_mp3: bool,
) -> Result<TranscodeResult, TranscodeError> {
    check_cancel(request.cancelled)?;
    if destination == Destination::AfterEffects && !matches!(request.backend, Backend::Library) {
        return Err(TranscodeError::Policy(
            "AE preparation requires the library backend".into(),
        ));
    }
    validate_paths(request.input, request.output)?;
    ensure_fresh_destination(request.output, destination)?;

    let input =
        fs::canonicalize(request.input).map_err(|source| io_error(request.input, source))?;
    let input_sha256 = sha256_file(&input)?;
    let source = if ae_mp3 {
        backend::probe_ae_mp3(&request.backend, &input, request.cancelled)?
    } else {
        backend::probe_for_destination(&request.backend, &input, request.cancelled, destination)?
    };
    validate_source_file(&input, &source)?;
    validate_source(&source, ae_mp3)?;
    let plan = if destination == Destination::AfterEffects {
        plan_after_effects(&request, &input, &source)?
    } else {
        plan(&request.backend, request.output, &source, request.cancelled)?
    };

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
            destination,
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
    let media = backend::probe_for_destination(
        &request.backend,
        &staged_output,
        request.cancelled,
        destination,
    )
    .map_err(|error| output_validation_error(destination, error))?;
    let output_sha256 = sha256_file(&staged_output)?;
    if plan.operation == Operation::Copy {
        if output_sha256 != input_sha256 {
            return Err(TranscodeError::Policy(
                "byte copy changed source bytes".into(),
            ));
        }
    } else {
        validate_output(plan.profile, &source, &media)
            .map_err(|error| output_validation_error(destination, error))?;
    }
    if source.timecode != media.timecode {
        return Err(output_validation_error(
            destination,
            TranscodeError::Policy("transcode changed recognized timecode metadata".into()),
        ));
    }
    if destination == Destination::AfterEffects
        && !backend::direct_video_clock(
            &request.backend,
            &staged_output,
            media.video.as_ref().ok_or_else(|| {
                TranscodeError::Protocol("prepared AE source lost its video stream".into())
            })?,
            request.cancelled,
        )
        .map_err(|error| output_validation_error(destination, error))?
    {
        return Err(TranscodeError::Protocol(
            "prepared AE source has no exact zero-origin presentation/decode clock".into(),
        ));
    }
    verify_source_freshness(&input, &input_sha256)
        .map_err(|error| output_validation_error(destination, error))?;
    check_cancel(request.cancelled)?;
    publish_no_replace(&staged_output, request.output)?;
    emit_status(progress, &input, ProgressStatus::Complete);

    let warnings = metadata_omission_warnings(&source, plan.operation);
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
        warnings,
    })
}

fn metadata_omission_warnings(source: &MediaInfo, operation: Operation) -> Vec<String> {
    if operation == Operation::Copy {
        return Vec::new();
    }
    source.camera_metadata.iter().map(|track| {
        let timecode = if track.has_timecode_label { " including its timecode label" } else { "" };
        format!("Omitted MOV/MP4 {} camera metadata stream {}{}: valid metadata remuxing is unsupported; original source is unchanged",
            track.codec_tag, track.stream_index, timecode)
    }).collect()
}

/// Unsupported source policy can fall back at the project boundary, but a
/// prepared AE output that violates its selected contract, or whose source
/// changed before publication, is a fatal result. Keep the standalone
/// command's established error classification.
fn output_validation_error(destination: Destination, error: TranscodeError) -> TranscodeError {
    match (destination, error) {
        (Destination::AfterEffects, TranscodeError::Policy(reason)) => {
            TranscodeError::Protocol(format!("prepared AE output failed validation: {reason}"))
        }
        (_, error) => error,
    }
}

fn plan_after_effects(
    request: &TranscodeRequest<'_>,
    input: &Path,
    source: &MediaInfo,
) -> Result<Plan, TranscodeError> {
    if request.output.extension().and_then(|value| value.to_str()) != Some("mov")
        || source.audio.is_some()
        || source.timecode.is_some()
        || !source.camera_metadata.is_empty()
    {
        return Err(TranscodeError::Policy(
            "AE preparation requires video-only .mov without audio priming or data tracks".into(),
        ));
    }
    let video = source
        .video
        .as_ref()
        .ok_or_else(|| TranscodeError::Policy("AE preparation requires video".into()))?;
    if video.display_matrix != model::identity_display_matrix() {
        return Err(TranscodeError::Policy(
            "AE preparation requires an identity display matrix".into(),
        ));
    }
    let direct = backend::direct_video_clock(&request.backend, input, video, request.cancelled)?;
    let remux = direct && (compatible_h264(video) || video.codec == "prores" && video.alpha);
    let profile = if remux {
        Profile::RemuxVideo
    } else if video.alpha {
        if !video.has_eight_bit_alpha() {
            return Err(TranscodeError::Policy(
                "AE preparation cannot re-encode unverified alpha precision".into(),
            ));
        }
        Profile::Prores4444
    } else {
        Profile::H264
    };
    let capabilities = backend::capabilities(&request.backend, request.cancelled)?;
    Ok(Plan {
        profile,
        operation: if remux {
            Operation::Remux
        } else {
            Operation::Transcode
        },
        encoder: select_encoder(
            &request.backend,
            source,
            profile,
            &capabilities,
            Destination::AfterEffects,
        )?,
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
        (Some(video), _, "mp4") if compatible_video_codec("mp4", video) => {
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
        (Some(video), _, "mov") if compatible_video_codec("mov", video) => {
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
    let encoder = select_encoder(
        backend,
        source,
        profile,
        &capabilities,
        Destination::General,
    )?;
    Ok(Plan {
        profile,
        operation,
        encoder,
    })
}

// Both probes report decoded pixel formats. The deprecated yuvj spelling is
// the same 8-bit 4:2:0 layout with full range, not higher chroma/depth.
fn compatible_h264(video: &model::VideoInfo) -> bool {
    video.codec == "h264"
        && !video.alpha
        && !video.interlaced
        && matches!(video.pixel_format.as_str(), "yuv420p" | "yuvj420p")
}

fn compatible_video_codec(container: &str, video: &model::VideoInfo) -> bool {
    if video.codec == "h264" && !compatible_h264(video) {
        return false;
    }
    let codec = video.codec.as_str();
    match container {
        "mp4" => matches!(codec, "h264" | "hevc" | "av1" | "mpeg4"),
        "mov" => matches!(codec, "h264" | "hevc" | "prores" | "mpeg4"),
        _ => false,
    }
}

/// Constant, unreordered packet timing permits an edit-list-free stream copy.
/// Comparing exact integer ticks avoids accepting a one-frame shift hidden by
/// ordinary duration tolerances in the AE library destination.
#[cfg(any(feature = "ffmpeg-library", test))]
struct DirectVideoClock {
    step: Option<i128>,
    frames: u64,
    count: u64,
    valid: bool,
}

#[cfg(any(feature = "ffmpeg-library", test))]
fn exact_frame_time(
    pts: Option<i64>,
    index: u64,
    time_base: &model::Ratio,
    rate: &model::Ratio,
) -> bool {
    if time_base.num <= 0 || time_base.den <= 0 || rate.num <= 0 || rate.den <= 0 {
        return false;
    }
    pts.is_some_and(|pts| {
        i128::from(pts) * i128::from(time_base.num) * i128::from(rate.num)
            == i128::from(index) * i128::from(time_base.den) * i128::from(rate.den)
    })
}

#[cfg(any(feature = "ffmpeg-library", test))]
impl DirectVideoClock {
    fn new(video: &model::VideoInfo) -> Self {
        let numerator = i128::from(video.frame_rate.den) * i128::from(video.time_base.den);
        let denominator = i128::from(video.frame_rate.num) * i128::from(video.time_base.num);
        let step = (numerator > 0 && denominator > 0 && numerator % denominator == 0)
            .then(|| numerator / denominator);
        Self {
            step,
            frames: video.frames,
            count: 0,
            valid: step.is_some(),
        }
    }

    fn observe(&mut self, pts: Option<i64>, dts: Option<i64>, duration: i64) {
        let expected = self
            .step
            .and_then(|step| step.checked_mul(i128::from(self.count)));
        self.valid &= pts.map(i128::from) == expected
            && dts.map(i128::from) == expected
            && Some(i128::from(duration)) == self.step;
        match self.count.checked_add(1) {
            Some(count) => self.count = count,
            None => self.valid = false,
        }
    }

    fn complete(&self) -> bool {
        self.valid && self.count > 0 && self.count == self.frames
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

fn validate_source(info: &MediaInfo, ae_mp3: bool) -> Result<(), TranscodeError> {
    if ae_mp3
        && (info.video.is_some()
            || info.container != "mp3"
            || info
                .audio
                .as_ref()
                .and_then(model::ae_mp3_priming_samples)
                .is_none())
    {
        return Err(TranscodeError::Policy(
            "AE audio preparation requires one bounded primed MP3 track".into(),
        ));
    }
    if info.video.is_none() && info.audio.is_none() {
        return Err(TranscodeError::Policy(
            "source has no audio or video stream".into(),
        ));
    }
    if let Some(video) = &info.video {
        let rotation =
            model::display_matrix_rotation(&video.display_matrix, video.width, video.height)
                .ok_or_else(|| TranscodeError::Policy("unsupported display matrix".into()))?;
        if !video.rotation_degrees.is_finite()
            || (video.rotation_degrees - rotation).rem_euclid(360.0) != 0.0
        {
            return Err(TranscodeError::Policy(
                "rotation disagrees with display matrix".into(),
            ));
        }
        if !video.constant_frame_rate
            || video.interlaced
            || video.sample_aspect_ratio.num != video.sample_aspect_ratio.den
            || video.start_seconds.abs() > 1e-6
        {
            return Err(TranscodeError::Policy(
                "VFR, interlace, non-square pixels, or nonzero start is not safely normalized"
                    .into(),
            ));
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
            || (!ae_mp3 && audio.start_seconds.abs() > 1e-6)
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
        if source.display_matrix != output.display_matrix
            || !output.rotation_degrees.is_finite()
            || (source.rotation_degrees - output.rotation_degrees).rem_euclid(360.0) != 0.0
        {
            return Err(TranscodeError::Policy(
                "transcode changed video display transform".into(),
            ));
        }
        let frame_seconds = f64::from(source.frame_rate.den) / f64::from(source.frame_rate.num);
        if source.width != output.width
            || source.height != output.height
            || source.frame_rate != output.frame_rate
            || source.frames != output.frames
            || (source.duration_seconds - output.duration_seconds).abs() > frame_seconds
            || output.start_seconds.abs() > 1e-6
            || source.alpha != output.alpha
        {
            return Err(TranscodeError::Policy(
                "transcode changed video dimensions, frame rate, frame count, duration, start, or alpha presence"
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
                if !compatible_h264(output)
                    || !output.first_keyframe
                    || output.max_keyframe_interval > gop_limit(output) =>
            {
                return Err(TranscodeError::Policy(
                    "H.264 output violates progressive 8-bit 4:2:0/alpha/keyframe/GOP policy"
                        .into(),
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
                format!(
                    "transcode changed audio timing or layout: source rate={} channels={} layout={:?} duration={:.12} start={:.12}; output rate={} channels={} layout={:?} duration={:.12} start={:.12}",
                    source.sample_rate, source.channels, source.channel_layout,
                    source.duration_seconds, source.start_seconds,
                    output.sample_rate, output.channels, output.channel_layout,
                    output.duration_seconds, output.start_seconds,
                ),
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

fn ensure_fresh_destination(output: &Path, destination: Destination) -> Result<(), TranscodeError> {
    match fs::symlink_metadata(output) {
        Ok(_) => Err(match destination {
            Destination::General => {
                TranscodeError::Policy(format!("output already exists: {output:?}"))
            }
            // The same no-clobber conflict that publication reports as I/O. An
            // AE project caller recovers from `Policy` as unsupported media.
            Destination::AfterEffects => io_error(output, io::ErrorKind::AlreadyExists.into()),
        }),
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
    destination: Destination,
) -> Result<String, TranscodeError> {
    let has = |name: &str| capabilities.encoders.iter().any(|encoder| encoder == name);
    // The native backend reports an unavailable platform encoder as a backend
    // failure. An AE project caller recovers from `Policy` as unsupported media.
    let unavailable = |reason: String| match destination {
        Destination::General => TranscodeError::Policy(reason),
        Destination::AfterEffects => TranscodeError::Backend { stderr: reason },
    };
    let require = |name: &str| {
        has(name)
            .then(|| name.to_owned())
            .ok_or_else(|| unavailable(format!("selected backend lacks required encoder {name}")))
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
                    unavailable("selected native backend has no approved H.264 encoder".into())
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

/// Independently inspected raw AAC source facts, excluding all movie edits.
#[derive(Debug, Clone, Copy)]
pub struct RawMovieAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: u64,
}

/// Decode one complete AAC track with movie edits disabled into zero-origin PCM.
/// No placement, trim, gain or gap is baked. Only the existing library backend is
/// used; decoded continuity/layout/length and source identity are checked before
/// atomic publication. The caller must independently validate the movie edit map.
pub fn prepare_raw_movie_audio(
    input: &Path,
    output: &Path,
    expected: RawMovieAudio,
) -> Result<(), TranscodeError> {
    #[cfg(not(feature = "ffmpeg-library"))]
    {
        let _ = (input, output, expected);
        Err(TranscodeError::Policy(
            "raw movie audio preparation requires ffmpeg-library".into(),
        ))
    }
    #[cfg(feature = "ffmpeg-library")]
    {
        validate_paths(input, output)?;
        if !(8_000..=192_000).contains(&expected.sample_rate)
            || !matches!(expected.channels, 1 | 2)
            || expected.samples == 0
            || expected.samples > i64::MAX as u64
            || !output
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
        {
            return Err(TranscodeError::Policy(
                "invalid raw movie audio request".into(),
            ));
        }
        if output.exists() {
            return Err(TranscodeError::Policy(
                "raw audio output already exists".into(),
            ));
        }
        let before = sha256_file(input)?;
        let directory = output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!(".raw-audio-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).map_err(|source| io_error(&directory, source))?;
        let staging = Staging { directory };
        let staged = staging.directory.join("audio.wav");
        native::prepare_raw_movie_audio(input, &staged, expected)?;
        if before != sha256_file(input)? {
            return Err(TranscodeError::Policy(
                "source changed during raw audio preparation".into(),
            ));
        }
        fs::hard_link(&staged, output).map_err(|source| io_error(output, source))?;
        Ok(())
    }
}
