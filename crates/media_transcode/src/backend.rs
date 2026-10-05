use crate::{
    model::{AudioInfo, Capabilities, ColorInfo, Job, MediaInfo, Profile, Ratio, VideoInfo},
    process, Backend, TranscodeError,
};
use serde::Deserialize;
use std::{path::Path, process::Command, sync::atomic::AtomicBool};

pub(crate) fn capabilities(
    backend: &Backend,
    cancelled: &AtomicBool,
) -> Result<Capabilities, TranscodeError> {
    match backend {
        Backend::Library => library_capabilities(cancelled),
        Backend::External { ffmpeg, .. } => external_capabilities(ffmpeg, cancelled),
    }
}

pub(crate) fn probe(
    backend: &Backend,
    input: &Path,
    cancelled: &AtomicBool,
) -> Result<MediaInfo, TranscodeError> {
    match backend {
        Backend::Library => library_probe(input, cancelled, false),
        Backend::External { ffprobe, .. } => external_probe(ffprobe, input, cancelled),
    }
}

pub(crate) fn probe_for_destination(
    backend: &Backend,
    input: &Path,
    cancelled: &AtomicBool,
    destination: crate::model::Destination,
) -> Result<MediaInfo, TranscodeError> {
    let exact = destination == crate::model::Destination::AfterEffects;
    if !exact {
        return probe(backend, input, cancelled);
    }
    match backend {
        Backend::Library => library_probe(input, cancelled, exact),
        Backend::External { .. } => Err(TranscodeError::Policy(
            "AE preparation requires the library backend".into(),
        )),
    }
}

pub(crate) fn probe_ae_mp3(
    backend: &Backend,
    input: &Path,
    cancelled: &AtomicBool,
) -> Result<MediaInfo, TranscodeError> {
    if *backend != Backend::Library {
        return Err(TranscodeError::Policy(
            "AE MP3 preparation requires the library backend".into(),
        ));
    }
    library_probe_ae_mp3(input, cancelled)
}

#[cfg(feature = "ffmpeg-library")]
fn library_probe_ae_mp3(input: &Path, cancelled: &AtomicBool) -> Result<MediaInfo, TranscodeError> {
    crate::native::probe_ae_mp3(input, cancelled)
}

#[cfg(not(feature = "ffmpeg-library"))]
fn library_probe_ae_mp3(
    _input: &Path,
    _cancelled: &AtomicBool,
) -> Result<MediaInfo, TranscodeError> {
    Err(library_unavailable())
}

pub(crate) fn transcode(
    backend: &Backend,
    job: &Job,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<(), TranscodeError> {
    match backend {
        Backend::Library => library_transcode(job, progress, cancelled),
        Backend::External { ffmpeg, .. } => external_transcode(ffmpeg, job, cancelled, progress),
    }
}

pub(crate) fn direct_video_clock(
    backend: &Backend,
    input: &Path,
    video: &VideoInfo,
    cancelled: &AtomicBool,
) -> Result<bool, TranscodeError> {
    match backend {
        Backend::Library => {
            #[cfg(feature = "ffmpeg-library")]
            {
                crate::native::direct_video_clock(input, video, cancelled)
            }
            #[cfg(not(feature = "ffmpeg-library"))]
            {
                let _ = (input, video, cancelled);
                Err(library_unavailable())
            }
        }
        Backend::External { .. } => Err(TranscodeError::Policy(
            "AE preparation requires the library backend".into(),
        )),
    }
}

#[cfg(feature = "ffmpeg-library")]
fn library_capabilities(cancelled: &AtomicBool) -> Result<Capabilities, TranscodeError> {
    crate::native::capabilities(cancelled)
}

#[cfg(not(feature = "ffmpeg-library"))]
fn library_capabilities(_cancelled: &AtomicBool) -> Result<Capabilities, TranscodeError> {
    Err(library_unavailable())
}

#[cfg(feature = "ffmpeg-library")]
fn library_probe(
    input: &Path,
    cancelled: &AtomicBool,
    exact: bool,
) -> Result<MediaInfo, TranscodeError> {
    crate::native::probe(input, cancelled, exact)
}

#[cfg(not(feature = "ffmpeg-library"))]
fn library_probe(
    _input: &Path,
    _cancelled: &AtomicBool,
    _exact: bool,
) -> Result<MediaInfo, TranscodeError> {
    Err(library_unavailable())
}

#[cfg(feature = "ffmpeg-library")]
fn library_transcode(
    job: &Job,
    progress: &mut dyn FnMut(f64),
    cancelled: &AtomicBool,
) -> Result<(), TranscodeError> {
    crate::native::transcode(job, progress, cancelled).map(|_| ())
}

#[cfg(not(feature = "ffmpeg-library"))]
fn library_transcode(
    _job: &Job,
    _progress: &mut dyn FnMut(f64),
    _cancelled: &AtomicBool,
) -> Result<(), TranscodeError> {
    Err(library_unavailable())
}

#[cfg(not(feature = "ffmpeg-library"))]
fn library_unavailable() -> TranscodeError {
    TranscodeError::Backend {
        stderr: "library backend requires the `ffmpeg-library` feature".into(),
    }
}

fn external_capabilities(
    ffmpeg: &Path,
    cancelled: &AtomicBool,
) -> Result<Capabilities, TranscodeError> {
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-encoders"]);
    let output = process::capture(&mut command, cancelled)?;
    if !output.status.success() {
        return Err(TranscodeError::Backend {
            stderr: output.stderr,
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let encoders = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?;
            let name = fields.next()?;
            (flags.len() >= 6 && (flags.starts_with('V') || flags.starts_with('A')))
                .then(|| name.to_owned())
        })
        .collect();
    Ok(Capabilities {
        version: "external-ffmpeg".into(),
        encoders,
    })
}

#[derive(Deserialize)]
struct ProbeDocument {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
    #[serde(default)]
    program_version: Option<ProbeVersion>,
}

#[derive(Deserialize)]
struct ProbeVersion {
    version: String,
}

#[derive(Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    start_time: Option<String>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    index: Option<usize>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    codec_tag_string: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    time_base: Option<String>,
    start_time: Option<String>,
    duration: Option<String>,
    nb_frames: Option<String>,
    sample_aspect_ratio: Option<String>,
    field_order: Option<String>,
    color_range: Option<String>,
    color_space: Option<String>,
    color_transfer: Option<String>,
    color_primaries: Option<String>,
    sample_fmt: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    channel_layout: Option<String>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
    #[serde(default)]
    side_data_list: Vec<ProbeSideData>,
}

#[derive(Debug, Deserialize)]
struct ProbeSideData {
    side_data_type: Option<String>,
    displaymatrix: Option<String>,
    rotation: Option<f64>,
}

fn external_probe(
    ffprobe: &Path,
    input: &Path,
    cancelled: &AtomicBool,
) -> Result<MediaInfo, TranscodeError> {
    let mut command = Command::new(ffprobe);
    command
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_program_version",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            "--",
        ])
        .arg(input);
    let output = process::capture(&mut command, cancelled)?;
    if !output.status.success() {
        return Err(TranscodeError::Backend {
            stderr: output.stderr,
        });
    }
    let document: ProbeDocument = serde_json::from_slice(&output.stdout)?;
    let video_streams: Vec<_> = document
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("video"))
        .collect();
    let audio_streams: Vec<_> = document
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("audio"))
        .collect();
    let data_streams: Vec<_> = document
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("data"))
        .collect();
    let other_streams = document
        .streams
        .iter()
        .filter(|stream| {
            !matches!(
                stream.codec_type.as_deref(),
                Some("video" | "audio" | "data")
            )
        })
        .count();
    if video_streams.len() > 1 || audio_streams.len() > 1 || other_streams > 0 {
        return Err(TranscodeError::Policy(format!(
            "unsupported stream layout: {} video, {} audio, {} data, {} other",
            video_streams.len(),
            audio_streams.len(),
            data_streams.len(),
            other_streams
        )));
    }
    let format_name = document
        .format
        .as_ref()
        .and_then(|format| format.format_name.as_deref())
        .unwrap_or_default();
    let major_brand = document
        .format
        .as_ref()
        .and_then(|format| format.tags.get("major_brand"))
        .map(String::as_str);
    let container = crate::model::detected_container(format_name, major_brand);
    let mut data = crate::model::DataStreams::default();
    for stream in data_streams {
        data.observe(
            format_name,
            stream
                .index
                .ok_or_else(|| TranscodeError::Policy("data stream has no index".into()))?,
            stream
                .codec_tag_string
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
            stream.tags.get("timecode").map(String::as_str),
        )
        .map_err(|reason| TranscodeError::Policy(reason.into()))?;
    }
    if video_streams.is_empty() && audio_streams.is_empty() {
        return Err(TranscodeError::Policy(
            "media contains no video or audio stream".into(),
        ));
    }
    let format_start = parse_optional_f64(
        document
            .format
            .as_ref()
            .and_then(|format| format.start_time.as_deref()),
    )?;
    let format_duration = parse_optional_f64(
        document
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref()),
    )?;
    let video = match video_streams.first() {
        Some(stream) => Some(scan_video(
            ffprobe,
            input,
            stream,
            format_start,
            format_duration,
            cancelled,
        )?),
        None => None,
    };
    let audio = audio_streams
        .first()
        .map(|stream| {
            parse_audio(
                stream,
                document
                    .format
                    .as_ref()
                    .and_then(|format| format.format_name.as_deref())
                    .unwrap_or_default(),
                format_start,
                format_duration,
            )
        })
        .transpose()?;
    Ok(MediaInfo {
        version: document
            .program_version
            .map(|version| version.version)
            .unwrap_or_else(|| "unknown".into()),
        container,
        video,
        audio,
        timecode: data.timecode,
        timecode_stream_index: data.timecode_stream_index,
        camera_metadata: data.camera_metadata,
    })
}

#[derive(Deserialize)]
struct PixelFormats {
    pixel_formats: Vec<PixelFormat>,
}

#[derive(Deserialize)]
struct PixelFormat {
    name: String,
    flags: PixelFormatFlags,
}

#[derive(Deserialize)]
struct PixelFormatFlags {
    alpha: u8,
}

fn pixel_format_alpha(document: &[u8], name: &str) -> Result<bool, TranscodeError> {
    let document: PixelFormats = serde_json::from_slice(document).map_err(|error| {
        TranscodeError::Policy(format!("invalid pixel-format descriptors: {error}"))
    })?;
    let mut matches = document
        .pixel_formats
        .iter()
        .filter(|format| format.name == name);
    let format = matches.next().ok_or_else(|| {
        TranscodeError::Policy(format!("unknown pixel-format descriptor: {name}"))
    })?;
    if matches.next().is_some() {
        return Err(TranscodeError::Policy(format!(
            "duplicate pixel-format descriptor: {name}"
        )));
    }
    match format.flags.alpha {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(TranscodeError::Policy(
            "invalid pixel-format alpha flag".into(),
        )),
    }
}

fn external_pixel_format_alpha(
    ffprobe: &Path,
    name: &str,
    cancelled: &AtomicBool,
) -> Result<bool, TranscodeError> {
    let mut command = Command::new(ffprobe);
    command.args(["-v", "error", "-show_pixel_formats", "-of", "json"]);
    let output = process::capture(&mut command, cancelled)?;
    if !output.status.success() {
        return Err(TranscodeError::Backend {
            stderr: output.stderr,
        });
    }
    pixel_format_alpha(&output.stdout, name)
}

fn scan_video(
    ffprobe: &Path,
    input: &Path,
    stream: &ProbeStream,
    format_start: Option<f64>,
    format_duration: Option<f64>,
    cancelled: &AtomicBool,
) -> Result<VideoInfo, TranscodeError> {
    let width = stream
        .width
        .ok_or_else(|| TranscodeError::Policy("unknown video width".into()))?;
    let height = stream
        .height
        .ok_or_else(|| TranscodeError::Policy("unknown video height".into()))?;
    let display_matrix = reject_probe_side_data(&stream.side_data_list, width, height)?;
    let mut command = Command::new(ffprobe);
    command
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-select_streams",
            "v:0",
            "-show_frames",
            "-show_entries",
            "frame=key_frame,best_effort_timestamp:frame_side_data=side_data_type,displaymatrix",
            "-of",
            "compact=p=0:nk=0",
            "--",
        ])
        .arg(input);
    let mut frames = 0_u64;
    let mut first_keyframe = false;
    let mut previous_timestamp: Option<i64> = None;
    let mut constant_frame_rate = true;
    let mut last_keyframe = None;
    let mut max_keyframe_interval = 0_u64;
    let time_base = parse_ratio(stream.time_base.as_deref())?;
    let frame_rate = parse_ratio(
        stream
            .avg_frame_rate
            .as_deref()
            .or(stream.r_frame_rate.as_deref()),
    )?;
    let tick_seconds = f64::from(time_base.num) / f64::from(time_base.den);
    let expected_ticks = f64::from(frame_rate.den) / f64::from(frame_rate.num) / tick_seconds;
    let (status, stderr) = process::lines(&mut command, cancelled, |line| {
        reject_compact_frame_side_data(line, width, height, &display_matrix)?;
        let Some(keyframe) = compact_field(line, "key_frame") else {
            return Ok(());
        };
        let timestamp = compact_field(line, "best_effort_timestamp")
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| *value != i64::MIN)
            .ok_or_else(|| {
                TranscodeError::Policy("decoded video frame has no integer timestamp".into())
            })?;
        let keyframe = keyframe == "1";
        if frames == 0 {
            first_keyframe = keyframe;
        }
        if keyframe {
            if let Some(last) = last_keyframe {
                max_keyframe_interval = max_keyframe_interval.max(frames.saturating_sub(last));
            }
            last_keyframe = Some(frames);
        }
        if let Some(previous) = previous_timestamp {
            let delta = (i128::from(timestamp) - i128::from(previous)) as f64;
            if !crate::model::nominal_frame_delta(delta, expected_ticks) {
                constant_frame_rate = false;
            }
        }
        previous_timestamp = Some(timestamp);
        frames = frames
            .checked_add(1)
            .ok_or_else(|| TranscodeError::Policy("frame count overflow".into()))?;
        Ok(())
    })?;
    if !status.success() {
        return Err(TranscodeError::Backend { stderr });
    }
    if frames == 0 {
        return Err(TranscodeError::Policy(
            "video decoder produced no frames".into(),
        ));
    }
    if let Some(declared) = stream
        .nb_frames
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
    {
        if declared != frames {
            return Err(TranscodeError::Policy(format!(
                "declared video frame count {declared} disagrees with decoded count {frames}"
            )));
        }
    }
    if let Some(last) = last_keyframe {
        max_keyframe_interval = max_keyframe_interval.max(frames.saturating_sub(last));
    }
    let rotation_degrees = crate::model::display_matrix_rotation(&display_matrix, width, height)
        .ok_or_else(|| TranscodeError::Policy("unsupported display matrix".into()))?;
    if let Some(value) = stream.tags.get("rotate") {
        let tag: f64 = value
            .parse()
            .map_err(|_| TranscodeError::Policy("invalid rotation tag".into()))?;
        if !tag.is_finite() || (tag - rotation_degrees).rem_euclid(360.0) != 0.0 {
            return Err(TranscodeError::Policy(
                "rotation tag disagrees with display matrix".into(),
            ));
        }
    }
    let pixel_format = required(stream.pix_fmt.as_deref(), "video pixel format")?.to_owned();
    let alpha = external_pixel_format_alpha(ffprobe, &pixel_format, cancelled)?;
    Ok(VideoInfo {
        codec: required(stream.codec_name.as_deref(), "video codec")?.to_owned(),
        width: stream
            .width
            .ok_or_else(|| TranscodeError::Policy("unknown video width".into()))?,
        height: stream
            .height
            .ok_or_else(|| TranscodeError::Policy("unknown video height".into()))?,
        pixel_format,
        frame_rate,
        time_base: parse_ratio(stream.time_base.as_deref())?,
        start_seconds: parse_optional_f64(stream.start_time.as_deref())?
            .or(format_start)
            .unwrap_or(0.0),
        duration_seconds: parse_optional_f64(stream.duration.as_deref())?
            .or(format_duration)
            .ok_or_else(|| TranscodeError::Policy("unknown video duration".into()))?,
        frames,
        constant_frame_rate,
        alpha,
        interlaced: !matches!(
            stream.field_order.as_deref(),
            None | Some("unknown" | "progressive")
        ),
        sample_aspect_ratio: parse_ratio(stream.sample_aspect_ratio.as_deref().or(Some("1:1")))?,
        rotation_degrees,
        display_matrix,
        color: ColorInfo {
            range: normalized(stream.color_range.as_deref()),
            space: normalized(stream.color_space.as_deref()),
            transfer: normalized(stream.color_transfer.as_deref()),
            primaries: normalized(stream.color_primaries.as_deref()),
        },
        first_keyframe,
        max_keyframe_interval,
    })
}

fn reject_probe_side_data(
    side_data: &[ProbeSideData],
    width: u32,
    height: u32,
) -> Result<[i32; 9], TranscodeError> {
    let mut display_matrix = None;
    for data in side_data {
        let kind = data.side_data_type.as_deref().unwrap_or_default();
        if kind.eq_ignore_ascii_case("ICC profile") {
            return Err(TranscodeError::Policy(
                "ICC-profiled video has no approved color preservation path".into(),
            ));
        }
        if kind.eq_ignore_ascii_case("Display Matrix") {
            let matrix = data.displaymatrix.as_deref().ok_or_else(|| {
                TranscodeError::Policy("display matrix coefficients are unavailable".into())
            })?;
            if display_matrix.is_some() {
                return Err(TranscodeError::Policy("duplicate display matrix".into()));
            }
            let matrix = parse_display_matrix(matrix, width, height)?;
            if let Some(rotation) = data.rotation {
                let expected = crate::model::display_matrix_rotation(&matrix, width, height)
                    .ok_or_else(|| TranscodeError::Policy("unsupported display matrix".into()))?;
                if !rotation.is_finite() || (rotation - expected).rem_euclid(360.0) != 0.0 {
                    return Err(TranscodeError::Policy(
                        "rotation disagrees with display matrix".into(),
                    ));
                }
            }
            display_matrix = Some(matrix);
        }
    }
    Ok(display_matrix.unwrap_or_else(crate::model::identity_display_matrix))
}

fn reject_compact_frame_side_data(
    line: &str,
    width: u32,
    height: u32,
    display_matrix: &[i32; 9],
) -> Result<(), TranscodeError> {
    // A decoded frame can carry several side-data records. Inspect every one,
    // rather than trusting only the first (often an unrelated codec SEI).
    for record in line.split("side_data_type=").skip(1) {
        let kind = record.split('|').next().unwrap_or_default();
        if kind.eq_ignore_ascii_case("ICC profile") {
            return Err(TranscodeError::Policy(
                "ICC-profiled video has no approved color preservation path".into(),
            ));
        }
        if kind.eq_ignore_ascii_case("Display Matrix") {
            let matrix = compact_field(record, "displaymatrix").ok_or_else(|| {
                TranscodeError::Policy("display matrix coefficients are unavailable".into())
            })?;
            if parse_display_matrix(matrix, width, height)? != *display_matrix {
                return Err(TranscodeError::Policy(
                    "frame display matrix differs from stream".into(),
                ));
            }
        }
    }
    Ok(())
}

fn compact_field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split('|')
        .find_map(|field| field.strip_prefix(name)?.strip_prefix('='))
}

fn parse_display_matrix(value: &str, width: u32, height: u32) -> Result<[i32; 9], TranscodeError> {
    let decoded = value.replace("\\n", "\n");
    let coefficients: Vec<i32> = decoded
        .lines()
        .flat_map(|line| {
            line.split_once(':')
                .map_or(line, |(_, values)| values)
                .split_whitespace()
        })
        .map(|value| value.parse::<i32>())
        .collect::<Result<_, _>>()
        .map_err(|_| TranscodeError::Policy("invalid display matrix coefficients".into()))?;
    let matrix: [i32; 9] = coefficients
        .try_into()
        .map_err(|_| TranscodeError::Policy("invalid display matrix length".into()))?;
    crate::model::display_matrix_rotation(&matrix, width, height)
        .ok_or_else(|| TranscodeError::Policy("unsupported display matrix".into()))?;
    Ok(matrix)
}

fn parse_audio(
    stream: &ProbeStream,
    container: &str,
    format_start: Option<f64>,
    format_duration: Option<f64>,
) -> Result<AudioInfo, TranscodeError> {
    Ok(AudioInfo {
        codec: required(stream.codec_name.as_deref(), "audio codec")?.to_owned(),
        sample_format: required(stream.sample_fmt.as_deref(), "audio sample format")?.to_owned(),
        sample_rate: required(stream.sample_rate.as_deref(), "audio sample rate")?
            .parse()
            .map_err(|_| TranscodeError::Policy("invalid audio sample rate".into()))?,
        channels: stream
            .channels
            .ok_or_else(|| TranscodeError::Policy("unknown audio channels".into()))?,
        channel_layout: crate::model::canonical_audio_layout(
            container,
            required(stream.codec_name.as_deref(), "audio codec")?,
            &normalized(stream.channel_layout.as_deref()),
            stream.channels.unwrap_or(0),
        ),
        start_seconds: parse_optional_f64(stream.start_time.as_deref())?
            .or(format_start)
            .unwrap_or(0.0),
        duration_seconds: parse_optional_f64(stream.duration.as_deref())?
            .or(format_duration)
            .ok_or_else(|| TranscodeError::Policy("unknown audio duration".into()))?,
    })
}

fn external_transcode(
    ffmpeg: &Path,
    job: &Job,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<(), TranscodeError> {
    let mut command = Command::new(ffmpeg);
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-noautorotate",
            "-i",
        ])
        .arg(&job.input);
    if job.profile == Profile::Prores4444 {
        // Scale alpha independently as full-range luma: packed RGB -> YUVA scaling
        // in FFmpeg 7 biases high 8-bit alpha codes before ProRes quantization.
        command.args([
            "-filter_complex",
            "[0:v:0]split[c][a];[c]format=yuv444p10le[c10];[a]alphaextract,scale=in_range=full:out_range=full,format=yuv444p10le[a10];[c10][a10]mergeplanes=0x00010210:format=yuva444p10le[prepared_video]",
            "-map", "[prepared_video]", "-map", "0:a:0?",
        ]);
    } else {
        command.args(["-map", "0:v:0?", "-map", "0:a:0?"]);
    }
    if let Some(index) = job.source.timecode_stream_index {
        command.args([
            "-map",
            &format!("0:{index}"),
            "-c:d",
            "copy",
            "-map_metadata:s:d:0",
            &format!("0:s:{index}"),
        ]);
    } else if job.source.video.is_some() && !job.source.camera_metadata.is_empty() {
        // Never synthesize tmcd from a camera metadata label.
        command.args(["-write_tmcd", "0"]);
    }
    match job.profile {
        Profile::RemuxVideo => {
            let timescale = job
                .source
                .video
                .as_ref()
                .and_then(|video| remux_video_timescale(&video.time_base))
                .ok_or_else(|| TranscodeError::Policy("invalid remux video time base".into()))?;
            command
                .args(["-c", "copy", "-video_track_timescale"])
                .arg(timescale.to_string());
        }
        Profile::H264 => {
            command
                .args([
                    "-c:v",
                    "libx264",
                    "-preset",
                    "slow",
                    "-crf",
                    "12",
                    "-profile:v",
                    "high",
                    "-pix_fmt",
                    "yuv420p",
                    "-bf",
                    "0",
                    "-g",
                ])
                .arg(gop_size(&job.source)?.to_string())
                .args(["-force_key_frames", "expr:gte(t,n_forced*1)"]);
            if let Some(audio) = &job.source.audio {
                if audio.codec == "aac" {
                    command.args(["-c:a", "copy"]);
                } else {
                    command.args(["-c:a", "aac", "-b:a", "320k"]);
                }
            }
        }
        Profile::Prores4444 => {
            command.args([
                "-c:v",
                "prores_ks",
                "-profile:v",
                "4",
                "-pix_fmt",
                "yuva444p10le",
                "-alpha_bits",
                "8",
            ]);
            if let Some(audio) = &job.source.audio {
                command.args([
                    "-c:a",
                    if audio.codec == "aac" {
                        "copy"
                    } else {
                        pcm_encoder(audio)?
                    },
                ]);
            }
        }
        Profile::AudioPcm => {
            let audio = job
                .source
                .audio
                .as_ref()
                .ok_or_else(|| TranscodeError::Policy("PCM profile requires audio".into()))?;
            command.args(["-vn", "-c:a", pcm_encoder(audio)?]);
        }
    }
    if job.profile != Profile::AudioPcm {
        let video = job
            .source
            .video
            .as_ref()
            .ok_or_else(|| TranscodeError::Policy("movie output requires video".into()))?;
        let clock = if job.profile == Profile::RemuxVideo {
            video.time_base.clone()
        } else {
            Ratio {
                num: video.frame_rate.den,
                den: video.frame_rate.num,
            }
        };
        let timescale = movie_timescale(&clock, job.source.audio.as_ref().map(|a| a.sample_rate))
            .ok_or_else(|| {
            TranscodeError::Policy("movie clock exceeds supported timescale".into())
        })?;
        command.args(["-movie_timescale", &timescale.to_string()]);
    }
    command
        .args(["-progress", "pipe:1", "-nostats", "-n"])
        .arg(&job.output);
    let mut processed = 0.0_f64;
    let (status, stderr) = process::lines(&mut command, cancelled, |line| {
        if let Some(value) = line
            .strip_prefix("out_time_us=")
            .and_then(|value| value.parse::<f64>().ok())
        {
            processed = (value / 1_000_000.0).max(processed);
            progress(processed);
        }
        Ok(())
    })?;
    if !status.success() {
        return Err(TranscodeError::Backend { stderr });
    }
    Ok(())
}

/// MOV edit durations use the movie clock, not the audio sample clock. The
/// default 1000 Hz clock can truncate an otherwise complete AAC tail. Use the
/// smallest clock representing both video ticks and audio samples exactly;
/// FFmpeg's movie_timescale option is bounded by a positive signed 32-bit int.
/// MOV/MP4 audio tracks use the sample rate even when packets are copied.
pub(crate) fn movie_timescale(video_clock: &Ratio, audio_rate: Option<u32>) -> Option<i32> {
    fn gcd(mut a: u64, mut b: u64) -> u64 {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    }
    let num = u64::try_from(video_clock.num).ok().filter(|n| *n > 0)?;
    let den = u64::try_from(video_clock.den).ok().filter(|n| *n > 0)?;
    let mut scale = den / gcd(num, den);
    if let Some(rate) = audio_rate {
        let rate = u64::from(rate);
        if rate == 0 {
            return None;
        }
        scale = (scale / gcd(scale, rate)).checked_mul(rate)?;
    }
    i32::try_from(scale).ok()
}

/// MOV defaults to increasing small video timescales. Preserve unit-numerator
/// source ticks so a quantized nominal cadence stays nominal on a fresh probe.
/// For nonunit numerators, denominator ticks still represent every source tick
/// exactly (integer multiplication); the existing output timing gate still applies.
/// FFmpeg's option accepts positive signed 32-bit timescales. Never use this for
/// encoding, whose independently selected clock may require a different scale.
pub(super) fn remux_video_timescale(time_base: &Ratio) -> Option<i32> {
    (time_base.num > 0 && time_base.den > 0).then_some(time_base.den)
}

pub(crate) fn pcm_encoder(audio: &AudioInfo) -> Result<&'static str, TranscodeError> {
    let format = audio.sample_format.trim_end_matches('p');
    match format {
        "u8" | "s16" => Ok("pcm_s16le"),
        "s32" => Ok("pcm_s32le"),
        "flt" => Ok("pcm_f32le"),
        _ => Err(TranscodeError::Policy(format!(
            "audio sample format {} has no non-truncating PCM policy",
            audio.sample_format
        ))),
    }
}

fn gop_size(info: &MediaInfo) -> Result<u64, TranscodeError> {
    let rate = info
        .video
        .as_ref()
        .ok_or_else(|| TranscodeError::Policy("H.264 profile requires video".into()))?
        .frame_rate
        .clone();
    if rate.den <= 0 || rate.num <= 0 {
        return Err(TranscodeError::Policy("invalid frame rate".into()));
    }
    Ok(((f64::from(rate.num) / f64::from(rate.den)).ceil() as u64).max(1))
}

fn parse_ratio(value: Option<&str>) -> Result<Ratio, TranscodeError> {
    let value = required(value, "ratio")?;
    let (num, den) = value
        .split_once('/')
        .or_else(|| value.split_once(':'))
        .ok_or_else(|| TranscodeError::Policy(format!("invalid ratio {value}")))?;
    let ratio = Ratio {
        num: num
            .parse()
            .map_err(|_| TranscodeError::Policy(format!("invalid ratio {value}")))?,
        den: den
            .parse()
            .map_err(|_| TranscodeError::Policy(format!("invalid ratio {value}")))?,
    };
    if ratio.den == 0 {
        return Err(TranscodeError::Policy(format!("invalid ratio {value}")));
    }
    Ok(ratio)
}

fn parse_optional_f64(value: Option<&str>) -> Result<Option<f64>, TranscodeError> {
    value
        .map(|value| {
            value
                .parse()
                .map_err(|_| TranscodeError::Policy(format!("invalid numeric metadata {value}")))
        })
        .transpose()
}

fn required<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, TranscodeError> {
    value
        .filter(|value| !value.is_empty() && *value != "N/A")
        .ok_or_else(|| TranscodeError::Policy(format!("unknown {name}")))
}

fn normalized(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_pixel_format_descriptors_fail_closed() {
        let document = br#"{"pixel_formats":[{"name":"vuya","flags":{"alpha":1}},{"name":"rgb24","flags":{"alpha":0}}]}"#;
        assert!(pixel_format_alpha(document, "vuya").unwrap());
        assert!(!pixel_format_alpha(document, "rgb24").unwrap());
        assert!(pixel_format_alpha(document, "unknown").is_err());
        assert!(
            pixel_format_alpha(br#"{"pixel_formats":[{"name":"vuya","flags":{}}]}"#, "vuya")
                .is_err()
        );
    }

    #[test]
    fn rejects_zero_angle_mirrored_display_matrix() {
        let mirrored = "00000000: -65536 0 0\\n00000001: 0 65536 0\\n00000002: 0 0 1073741824";
        let error = parse_display_matrix(mirrored, 1920, 1080).unwrap_err();
        assert!(error.to_string().contains("unsupported display matrix"));
    }

    #[test]
    fn rejects_icc_profile_even_without_asserted_color_metadata() {
        let side_data = [ProbeSideData {
            side_data_type: Some("ICC profile".into()),
            displaymatrix: None,
            rotation: None,
        }];
        let error = reject_probe_side_data(&side_data, 1920, 1080).unwrap_err();
        assert!(error.to_string().contains("ICC-profiled video"));
    }

    #[test]
    fn native_quarter_turn_probe_matrix_is_preserved_and_malformed_data_rejected() {
        for descriptor in crate::tests::native_quarter_turn_descriptors() {
            let text = descriptor
                .display_matrix
                .chunks_exact(3)
                .enumerate()
                .map(|(row, values)| {
                    format!("{row:08}: {} {} {}\n", values[0], values[1], values[2])
                })
                .collect::<String>();
            let data = [ProbeSideData {
                side_data_type: Some("Display Matrix".into()),
                displaymatrix: Some(text.clone()),
                rotation: Some(descriptor.rotation_degrees),
            }];
            assert_eq!(
                reject_probe_side_data(&data, descriptor.width, descriptor.height).unwrap(),
                descriptor.display_matrix
            );
            for invalid in [
                text.replace("65536", "NaN"),
                text.replace("65536", "2147483648"),
                format!("{text} 0"),
                "0 65536".into(),
            ] {
                assert!(
                    parse_display_matrix(&invalid, descriptor.width, descriptor.height).is_err()
                );
            }
        }
    }

    #[test]
    fn frame_side_data_cannot_hide_another_transform_or_icc_profile() {
        let identity = crate::model::identity_display_matrix();
        for line in [
            "frame|side_data_type=unrelated|side_data_type=ICC profile",
            "frame|side_data_type=unrelated|side_data_type=Display Matrix|displaymatrix=0 65536 0 -65536 0 0 0 0 1073741824",
            "frame|side_data_type=Display Matrix",
        ] {
            assert!(reject_compact_frame_side_data(line, 1920, 1080, &identity).is_err());
        }
    }

    #[test]
    fn accepts_full_identity_display_matrix() {
        let identity = "00000000: 65536 0 0\n00000001: 0 65536 0\n00000002: 0 0 1073741824";
        parse_display_matrix(identity, 1920, 1080).unwrap();
    }

    #[test]
    fn reads_major_brand_from_format_tags_for_container_detection() {
        let format: ProbeFormat = serde_json::from_value(serde_json::json!({
            "format_name": "mov,mp4,m4a,3gp,3g2,mj2",
            "tags": { "major_brand": "qt  " }
        }))
        .unwrap();
        assert_eq!(
            crate::model::detected_container(
                format.format_name.as_deref().unwrap(),
                format.tags.get("major_brand").map(String::as_str),
            ),
            "mov"
        );
    }

    fn recognize_test_timecode(
        container: &str,
        stream: &ProbeStream,
    ) -> Result<String, &'static str> {
        let mut data = crate::model::DataStreams::default();
        data.observe(
            container,
            2,
            stream.codec_tag_string.as_deref().unwrap().as_bytes(),
            stream.tags.get("timecode").map(String::as_str),
        )?;
        Ok(data.timecode.unwrap())
    }

    #[test]
    fn recognizes_only_labeled_mov_timecode() {
        let stream: ProbeStream = serde_json::from_value(serde_json::json!({
            "codec_type": "data",
            "codec_tag_string": "tmcd",
            "tags": { "timecode": "01:02:03:04" }
        }))
        .unwrap();

        assert_eq!(
            recognize_test_timecode("mov,mp4,m4a,3gp,3g2,mj2", &stream).unwrap(),
            "01:02:03:04"
        );
        assert!(recognize_test_timecode("matroska,webm", &stream).is_err());
    }

    #[test]
    fn rejects_unlabeled_mov_timecode() {
        let stream: ProbeStream = serde_json::from_value(serde_json::json!({
            "codec_type": "data",
            "codec_tag_string": "tmcd"
        }))
        .unwrap();

        assert!(recognize_test_timecode("mov,mp4,m4a,3gp,3g2,mj2", &stream).is_err());
    }
}
