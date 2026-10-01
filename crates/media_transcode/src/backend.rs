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
        Backend::Library => library_probe(input, cancelled),
        Backend::External { ffprobe, .. } => external_probe(ffprobe, input, cancelled),
    }
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

#[cfg(feature = "ffmpeg-library")]
fn library_capabilities(cancelled: &AtomicBool) -> Result<Capabilities, TranscodeError> {
    crate::native::capabilities(cancelled)
}

#[cfg(not(feature = "ffmpeg-library"))]
fn library_capabilities(_cancelled: &AtomicBool) -> Result<Capabilities, TranscodeError> {
    Err(library_unavailable())
}

#[cfg(feature = "ffmpeg-library")]
fn library_probe(input: &Path, cancelled: &AtomicBool) -> Result<MediaInfo, TranscodeError> {
    crate::native::probe(input, cancelled)
}

#[cfg(not(feature = "ffmpeg-library"))]
fn library_probe(_input: &Path, _cancelled: &AtomicBool) -> Result<MediaInfo, TranscodeError> {
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
    if video_streams.len() > 1
        || audio_streams.len() > 1
        || data_streams.len() > 1
        || other_streams > 0
    {
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
    let timecode = data_streams
        .first()
        .map(|stream| recognized_timecode(format_name, stream))
        .transpose()?;
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
        timecode,
    })
}

fn recognized_timecode(container: &str, stream: &ProbeStream) -> Result<String, TranscodeError> {
    if !container.split(',').any(|name| name == "mov")
        || stream.codec_tag_string.as_deref() != Some("tmcd")
    {
        return Err(TranscodeError::Policy(
            "only a MOV tmcd data stream is supported".into(),
        ));
    }
    stream
        .tags
        .get("timecode")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| TranscodeError::Policy("MOV tmcd stream has no timecode label".into()))
}

fn scan_video(
    ffprobe: &Path,
    input: &Path,
    stream: &ProbeStream,
    format_start: Option<f64>,
    format_duration: Option<f64>,
    cancelled: &AtomicBool,
) -> Result<VideoInfo, TranscodeError> {
    reject_probe_side_data(&stream.side_data_list)?;
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
            "frame=key_frame,best_effort_timestamp_time:frame_side_data=side_data_type,displaymatrix",
            "-of",
            "compact=p=0:nk=0",
            "--",
        ])
        .arg(input);
    let mut frames = 0_u64;
    let mut first_keyframe = false;
    let mut previous_timestamp: Option<f64> = None;
    let mut first_delta: Option<f64> = None;
    let mut constant_frame_rate = true;
    let mut last_keyframe = None;
    let mut max_keyframe_interval = 0_u64;
    let time_base = parse_ratio(stream.time_base.as_deref())?;
    let timestamp_tolerance = (f64::from(time_base.num) / f64::from(time_base.den)).abs() * 1.01;
    let (status, stderr) = process::lines(&mut command, cancelled, |line| {
        reject_compact_frame_side_data(line)?;
        let Some(keyframe) = compact_field(line, "key_frame") else {
            return Ok(());
        };
        let timestamp = compact_field(line, "best_effort_timestamp_time")
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| {
                TranscodeError::Policy("decoded video frame has no finite timestamp".into())
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
            let delta = timestamp - previous;
            if delta <= 0.0 {
                constant_frame_rate = false;
            } else if let Some(expected) = first_delta {
                if (delta - expected).abs() > timestamp_tolerance.max(expected.abs() * 1e-5) {
                    constant_frame_rate = false;
                }
            } else {
                first_delta = Some(delta);
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
    let frame_rate = parse_ratio(
        stream
            .avg_frame_rate
            .as_deref()
            .or(stream.r_frame_rate.as_deref()),
    )?;
    let rotation_degrees = stream
        .tags
        .get("rotate")
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            stream
                .side_data_list
                .iter()
                .find_map(|value| value.rotation)
        })
        .unwrap_or(0.0);
    let pixel_format = required(stream.pix_fmt.as_deref(), "video pixel format")?.to_owned();
    let alpha = pixel_format.starts_with("yuva")
        || pixel_format.starts_with("gbrap")
        || pixel_format.starts_with("ya")
        || matches!(pixel_format.as_str(), "rgba" | "bgra" | "argb" | "abgr");
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

const IDENTITY_DISPLAY_MATRIX: [i64; 9] = [1 << 16, 0, 0, 0, 1 << 16, 0, 0, 0, 1 << 30];

fn reject_probe_side_data(side_data: &[ProbeSideData]) -> Result<(), TranscodeError> {
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
            reject_nonidentity_display_matrix(matrix)?;
        }
    }
    Ok(())
}

fn reject_compact_frame_side_data(line: &str) -> Result<(), TranscodeError> {
    let Some(kind) = compact_field(line, "side_data_type") else {
        return Ok(());
    };
    if kind.eq_ignore_ascii_case("ICC profile") {
        return Err(TranscodeError::Policy(
            "ICC-profiled video has no approved color preservation path".into(),
        ));
    }
    if kind.eq_ignore_ascii_case("Display Matrix") {
        let matrix = compact_field(line, "displaymatrix").ok_or_else(|| {
            TranscodeError::Policy("display matrix coefficients are unavailable".into())
        })?;
        reject_nonidentity_display_matrix(matrix)?;
    }
    Ok(())
}

fn compact_field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split('|')
        .find_map(|field| field.strip_prefix(name)?.strip_prefix('='))
}

fn reject_nonidentity_display_matrix(value: &str) -> Result<(), TranscodeError> {
    let decoded = value.replace("\\n", "\n");
    let coefficients: Vec<i64> = decoded
        .lines()
        .flat_map(|line| {
            line.split_once(':')
                .map_or(line, |(_, values)| values)
                .split_whitespace()
        })
        .map(|value| value.parse::<i64>())
        .collect::<Result<_, _>>()
        .map_err(|_| TranscodeError::Policy("invalid display matrix coefficients".into()))?;
    if coefficients.as_slice() != IDENTITY_DISPLAY_MATRIX {
        return Err(TranscodeError::Policy(
            "non-identity display matrix is not safely normalized".into(),
        ));
    }
    Ok(())
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
    if job.source.timecode.is_some() {
        command.args([
            "-map",
            "0:d:0",
            "-c:d",
            "copy",
            "-map_metadata:s:d:0",
            "0:s:d:0",
        ]);
    }
    match job.profile {
        Profile::RemuxVideo => {
            command.args(["-c", "copy"]);
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
    fn rejects_zero_angle_mirrored_display_matrix() {
        let mirrored = "00000000: -65536 0 0\\n00000001: 0 65536 0\\n00000002: 0 0 1073741824";
        let error = reject_nonidentity_display_matrix(mirrored).unwrap_err();
        assert!(error.to_string().contains("non-identity display matrix"));
    }

    #[test]
    fn rejects_icc_profile_even_without_asserted_color_metadata() {
        let side_data = [ProbeSideData {
            side_data_type: Some("ICC profile".into()),
            displaymatrix: None,
            rotation: None,
        }];
        let error = reject_probe_side_data(&side_data).unwrap_err();
        assert!(error.to_string().contains("ICC-profiled video"));
    }

    #[test]
    fn accepts_full_identity_display_matrix() {
        let identity = "00000000: 65536 0 0\n00000001: 0 65536 0\n00000002: 0 0 1073741824";
        reject_nonidentity_display_matrix(identity).unwrap();
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

    #[test]
    fn recognizes_only_labeled_mov_timecode() {
        let stream: ProbeStream = serde_json::from_value(serde_json::json!({
            "codec_type": "data",
            "codec_tag_string": "tmcd",
            "tags": { "timecode": "01:02:03:04" }
        }))
        .unwrap();

        assert_eq!(
            recognized_timecode("mov,mp4,m4a,3gp,3g2,mj2", &stream).unwrap(),
            "01:02:03:04"
        );
        assert!(recognized_timecode("matroska,webm", &stream).is_err());
    }

    #[test]
    fn rejects_unlabeled_mov_timecode() {
        let stream: ProbeStream = serde_json::from_value(serde_json::json!({
            "codec_type": "data",
            "codec_tag_string": "tmcd"
        }))
        .unwrap();

        assert!(recognized_timecode("mov,mp4,m4a,3gp,3g2,mj2", &stream).is_err());
    }
}
