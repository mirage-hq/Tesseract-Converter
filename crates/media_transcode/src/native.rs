//! In-process FFmpeg media probing and transcoding.
//!
//! Cancellation is cooperative between FFmpeg calls. Individual demuxer, decoder,
//! encoder, and muxer calls are not preemptible once entered.
//! Adapted in part from Captions' `media_conversion/ffmpeg.rs` implementation
//! (PR 4601); probing, alpha output, and progress are local.

use crate::model::{AudioInfo, Capabilities, ColorInfo, Job, MediaInfo, Profile, Ratio, VideoInfo};
use ffmpeg_next::{
    self as ffmpeg, codec, ffi, format, frame, media, software, Dictionary, Packet, Rational,
};
use std::{
    ffi::CStr,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};
use thiserror::Error;

const H264_BITS_PER_PIXEL_FRAME: f64 = 0.3;
const AAC_BITS_PER_CHANNEL: usize = 96_000;
const PROGRESS_INTERVAL_SECONDS: f64 = 0.25;

#[cfg(target_os = "macos")]
const H264_ENCODER: Option<&str> = Some("h264_videotoolbox");
#[cfg(target_os = "windows")]
const H264_ENCODER: Option<&str> = Some("h264_mf");
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const H264_ENCODER: Option<&str> = None;

#[derive(Debug, Error)]
enum Error {
    #[error("operation cancelled")]
    Cancelled,
    #[error("{0}")]
    Unsupported(String),
    #[error("{context}: {source}")]
    Ffmpeg {
        context: &'static str,
        #[source]
        source: ffmpeg::Error,
    },
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },
}

type Result<T> = std::result::Result<T, Error>;

fn check_cancel(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

/// The next demuxed packet and its stream index, or `None` at end of input.
///
/// `Input::packets` silently retries every non-EOF read error, so a persistent
/// demux or I/O failure would spin forever without observing cancellation.
/// This propagates such errors; only `EAGAIN` is retried, and each attempt
/// first checks cancellation.
fn read_packet(
    input: &mut format::context::Input,
    cancelled: &AtomicBool,
) -> Result<Option<(usize, Packet)>> {
    loop {
        check_cancel(cancelled)?;
        let mut packet = Packet::empty();
        match packet.read(input) {
            Ok(()) => return Ok(Some((packet.stream(), packet))),
            Err(ffmpeg::Error::Eof) => return Ok(None),
            Err(ffmpeg::Error::Other {
                errno: ffmpeg::util::error::EAGAIN,
            }) => continue,
            Err(source) => {
                return Err(Error::Ffmpeg {
                    context: "read demuxed packet",
                    source,
                })
            }
        }
    }
}

fn public_result<T>(result: Result<T>) -> std::result::Result<T, super::TranscodeError> {
    result.map_err(|error| match error {
        Error::Cancelled => super::TranscodeError::Cancelled,
        error => super::TranscodeError::Backend {
            stderr: error.to_string(),
        },
    })
}

fn init() -> Result<()> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        let result = ffmpeg::init().map_err(|error| error.to_string());
        ffmpeg::log::set_level(ffmpeg::log::Level::Error);
        result
    })
    .clone()
    .map_err(|message| Error::Unsupported(format!("initialize FFmpeg: {message}")))
}

pub(super) fn capabilities(
    cancelled: &AtomicBool,
) -> std::result::Result<Capabilities, super::TranscodeError> {
    public_result((|| {
        check_cancel(cancelled)?;
        init()?;
        check_cancel(cancelled)?;
        let names = [
            H264_ENCODER,
            Some("prores_ks"),
            Some("aac"),
            Some("pcm_s16le"),
            Some("pcm_s32le"),
            Some("pcm_f32le"),
            Some("pcm_f64le"),
        ];
        let encoders = names
            .into_iter()
            .flatten()
            .filter(|name| ffmpeg::encoder::find_by_name(name).is_some())
            .map(str::to_owned)
            .collect();
        Ok(Capabilities {
            version: version(),
            encoders,
        })
    })())
}

pub(super) fn probe(
    path: &Path,
    cancelled: &AtomicBool,
    exact: bool,
) -> std::result::Result<MediaInfo, super::TranscodeError> {
    // Initialization is operational even when probing an unsupported AE source.
    public_result(check_cancel(cancelled))?;
    public_result(init())?;
    match probe_inner(path, cancelled, exact, false) {
        Err(Error::Unsupported(reason)) if exact => Err(super::TranscodeError::Policy(reason)),
        result => public_result(result),
    }
}

pub(super) fn probe_ae_mp3(
    path: &Path,
    cancelled: &AtomicBool,
) -> std::result::Result<MediaInfo, super::TranscodeError> {
    public_result(check_cancel(cancelled))?;
    public_result(init())?;
    public_result(probe_inner(path, cancelled, false, true))
}

pub(super) fn direct_video_clock(
    path: &Path,
    video: &VideoInfo,
    cancelled: &AtomicBool,
) -> std::result::Result<bool, super::TranscodeError> {
    public_result((|| {
        check_cancel(cancelled)?;
        init()?;
        validate_input(path)?;
        let mut input = open_local_input(path)?;
        let index = input
            .streams()
            .find(|stream| stream.parameters().medium() == media::Type::Video)
            .map(|stream| stream.index())
            .ok_or_else(|| Error::Unsupported("video stream disappeared".into()))?;
        let mut clock = crate::DirectVideoClock::new(video);
        while let Some((stream, packet)) = read_packet(&mut input, cancelled)? {
            if stream == index {
                clock.observe(packet.pts(), packet.dts(), packet.duration());
            }
        }
        Ok(clock.complete())
    })())
}

pub(super) fn transcode(
    job: &Job,
    progress: &mut dyn FnMut(f64),
    cancelled: &AtomicBool,
) -> std::result::Result<String, super::TranscodeError> {
    public_result((|| {
        check_cancel(cancelled)?;
        init()?;
        check_cancel(cancelled)?;
        validate_job(job)?;
        transcode_inner(job, progress, cancelled)?;
        check_cancel(cancelled)?;
        Ok(version())
    })())
}

fn version() -> String {
    // SAFETY: FFmpeg returns a process-lifetime NUL-terminated version string.
    unsafe { CStr::from_ptr(ffi::av_version_info()) }
        .to_string_lossy()
        .into_owned()
}

fn validate_absolute_regular(path: &Path, label: &'static str) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::Unsupported(format!("{label} path must be absolute")));
    }
    let metadata = fs::metadata(path).map_err(|source| Error::Io {
        context: "inspect an input path",
        source,
    })?;
    if !metadata.is_file() {
        return Err(Error::Unsupported(format!(
            "{label} path must name a regular file"
        )));
    }
    Ok(())
}

fn validate_input(path: &Path) -> Result<()> {
    validate_absolute_regular(path, "input")?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "mp4"
            | "mov"
            | "m4v"
            | "mkv"
            | "avi"
            | "webm"
            | "wav"
            | "m4a"
            | "aac"
            | "mp3"
            | "aiff"
            | "aif"
            | "aifc"
            | "flac"
            | "ogg"
            | "opus"
            | "swf"
    ) {
        return Err(Error::Unsupported(format!(
            "input extension .{extension} is not in the local media whitelist"
        )));
    }
    Ok(())
}

fn open_local_input(path: &Path) -> Result<format::context::Input> {
    let mut options = Dictionary::new();
    options.set("protocol_whitelist", "file,pipe");
    options.set(
        "format_whitelist",
        "mov,matroska,webm,avi,wav,aiff,aac,mp3,flac,ogg,swf",
    );
    format::input_with_dictionary(path, options).map_err(ffmpeg_error("open local media input"))
}

fn validate_job(job: &Job) -> Result<()> {
    validate_input(&job.input)?;
    if !job.output.is_absolute() {
        return Err(Error::Unsupported(
            "output path must be absolute".to_owned(),
        ));
    }
    if job.output.exists() {
        return Err(Error::Unsupported("output already exists".to_owned()));
    }
    let parent = job
        .output
        .parent()
        .ok_or_else(|| Error::Unsupported("output has no parent directory".to_owned()))?;
    if !parent.is_dir() {
        return Err(Error::Unsupported(
            "output parent is not a directory".to_owned(),
        ));
    }
    let expected_encoder = expected_profile_encoder(job)?;
    match (&job.encoder, expected_encoder) {
        (Some(requested), Some(expected)) if requested == expected => {}
        (Some(requested), Some(expected)) => {
            return Err(Error::Unsupported(format!(
                "job selected encoder {requested}; profile requires {expected}"
            )))
        }
        (None, Some(expected)) => {
            return Err(Error::Unsupported(format!(
                "job must select advertised encoder {expected}"
            )))
        }
        (Some(requested), None) => {
            return Err(Error::Unsupported(format!(
                "profile does not accept encoder {requested}"
            )))
        }
        (None, None) => {}
    }
    match job.profile {
        Profile::AudioPcm if job.source.audio.is_none() => Err(Error::Unsupported(
            "audio_pcm requires an audio stream".to_owned(),
        )),
        Profile::AudioPcm if job.source.video.is_some() => Err(Error::Unsupported(
            "audio_pcm accepts standalone audio only".to_owned(),
        )),
        Profile::RemuxVideo | Profile::H264 | Profile::Prores4444 if job.source.video.is_none() => {
            Err(Error::Unsupported(
                "video profile requires a video stream".to_owned(),
            ))
        }
        Profile::Prores4444
            if !job
                .source
                .video
                .as_ref()
                .is_some_and(|video| video.has_eight_bit_alpha()) =>
        {
            Err(Error::Unsupported(
                "prores4444 requires a verified 8-bit alpha source format".to_owned(),
            ))
        }
        _ => Ok(()),
    }
}

fn expected_profile_encoder(job: &Job) -> Result<Option<&'static str>> {
    match job.profile {
        Profile::RemuxVideo => Ok(None),
        Profile::H264 => H264_ENCODER.map(Some).ok_or_else(|| {
            Error::Unsupported("this platform has no supported native H.264 encoder".to_owned())
        }),
        Profile::Prores4444 => Ok(Some("prores_ks")),
        Profile::AudioPcm => {
            let format = &job
                .source
                .audio
                .as_ref()
                .ok_or_else(|| Error::Unsupported("audio_pcm requires an audio stream".to_owned()))?
                .sample_format;
            match format.trim_end_matches('p') {
                "u8" | "s16" => Ok(Some("pcm_s16le")),
                "s32" => Ok(Some("pcm_s32le")),
                "flt" => Ok(Some("pcm_f32le")),
                "dbl" => Ok(Some("pcm_f64le")),
                _ => Err(Error::Unsupported(format!(
                    "PCM cannot preserve source sample format {format}"
                ))),
            }
        }
    }
}

fn ffmpeg_error(context: &'static str) -> impl FnOnce(ffmpeg::Error) -> Error {
    move |source| Error::Ffmpeg { context, source }
}

fn seconds(value: i64, time_base: Rational) -> Option<f64> {
    (value != ffi::AV_NOPTS_VALUE)
        .then(|| value as f64 * f64::from(time_base.0) / f64::from(time_base.1))
}

fn rational(value: Rational) -> Ratio {
    Ratio {
        num: value.0,
        den: value.1,
    }
}

fn enum_name(pointer: *const std::os::raw::c_char) -> String {
    if pointer.is_null() {
        "unknown".to_owned()
    } else {
        // SAFETY: FFmpeg enum-name helpers return static NUL-terminated strings.
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

fn channel_layout_name(layout: &ffi::AVChannelLayout) -> String {
    let mut buffer = [0_i8; 128];
    // SAFETY: layout is initialized by FFmpeg and buffer is writable for its full size.
    let result =
        unsafe { ffi::av_channel_layout_describe(layout, buffer.as_mut_ptr(), buffer.len()) };
    if result < 0 {
        "unknown".to_owned()
    } else {
        // SAFETY: a successful describe call writes a NUL-terminated string.
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }
}

fn probe_inner(
    path: &Path,
    cancelled: &AtomicBool,
    exact: bool,
    ae_mp3: bool,
) -> Result<MediaInfo> {
    check_cancel(cancelled)?;
    validate_input(path)?;
    check_cancel(cancelled)?;
    let mut input = open_local_input(path)?;
    let format_name = input.format().name().to_owned();
    let container =
        crate::model::detected_container(&format_name, input.metadata().get("major_brand"));
    let mut video_indices = Vec::new();
    let mut audio_indices = Vec::new();
    let mut data_indices = Vec::new();
    let mut unsupported = Vec::new();
    for stream in input.streams() {
        match stream.parameters().medium() {
            media::Type::Video => video_indices.push(stream.index()),
            media::Type::Audio => audio_indices.push(stream.index()),
            media::Type::Data => data_indices.push(stream.index()),
            media::Type::Subtitle => unsupported.push("subtitle"),
            media::Type::Attachment => unsupported.push("attachment"),
            _ => unsupported.push("unknown"),
        }
    }
    if video_indices.len() > 1 || audio_indices.len() > 1 {
        return Err(Error::Unsupported(format!(
            "unsupported stream layout: {} video, {} audio, and {} data streams",
            video_indices.len(),
            audio_indices.len(),
            data_indices.len()
        )));
    }
    if !unsupported.is_empty() {
        return Err(Error::Unsupported(format!(
            "unsupported additional streams: {}",
            unsupported.join(", ")
        )));
    }
    let data = probe_data_streams(&input, &format_name)?;
    if video_indices.is_empty() && audio_indices.is_empty() {
        return Err(Error::Unsupported(
            "input has no video or audio stream".to_owned(),
        ));
    }
    let video = video_indices
        .first()
        .copied()
        .map(|index| probe_video(&mut input, index, cancelled, exact))
        .transpose()?;
    let audio = audio_indices
        .first()
        .copied()
        .map(|index| probe_audio(path, index, cancelled, ae_mp3))
        .transpose()?;
    let info = MediaInfo {
        version: version(),
        container,
        video,
        audio,
        timecode: data.timecode,
        timecode_stream_index: data.timecode_stream_index,
        camera_metadata: data.camera_metadata,
    };
    validate_probe_timing(&info, ae_mp3)?;
    Ok(info)
}

fn probe_data_streams(
    input: &format::context::common::Context,
    format_name: &str,
) -> Result<crate::model::DataStreams> {
    let mut data = crate::model::DataStreams::default();
    for stream in input
        .streams()
        .filter(|stream| stream.parameters().medium() == media::Type::Data)
    {
        // SAFETY: codec parameters belong to the live input stream.
        let tag = unsafe { (*stream.parameters().as_ptr()).codec_tag }.to_le_bytes();
        data.observe(
            format_name,
            stream.index(),
            &tag,
            stream.metadata().get("timecode"),
        )
        .map_err(|reason| Error::Unsupported(reason.into()))?;
    }
    Ok(data)
}

fn validate_probe_timing(info: &MediaInfo, ae_mp3: bool) -> Result<()> {
    if let Some(video) = &info.video {
        if video.start_seconds.abs() > 1e-6 {
            return Err(Error::Unsupported(format!(
                "video starts at {} seconds; only zero-start media is supported",
                video.start_seconds
            )));
        }
        if !video.constant_frame_rate {
            return Err(Error::Unsupported(
                "variable or discontinuous frame timing is unsupported".to_owned(),
            ));
        }
        if !video.first_keyframe {
            return Err(Error::Unsupported(
                "video does not begin with an independently decodable keyframe".to_owned(),
            ));
        }
        if video.interlaced {
            return Err(Error::Unsupported(
                "interlaced video is unsupported".to_owned(),
            ));
        }
        if video.sample_aspect_ratio != (Ratio { num: 1, den: 1 }) {
            return Err(Error::Unsupported(
                "non-square video pixels are unsupported".to_owned(),
            ));
        }
        if matches!(video.color.transfer.as_str(), "smpte2084" | "arib-std-b67")
            || video.color.primaries == "bt2020"
        {
            return Err(Error::Unsupported("HDR video is unsupported".to_owned()));
        }
    }
    if let Some(audio) = &info.audio {
        if audio.start_seconds.abs() > 1e-6
            && !(ae_mp3 && crate::model::ae_mp3_priming_samples(audio).is_some())
        {
            return Err(Error::Unsupported(format!(
                "audio content starts at {} seconds; only zero-start media is supported",
                audio.start_seconds
            )));
        }
        if !audio.duration_seconds.is_finite() || audio.duration_seconds <= 0.0 {
            return Err(Error::Unsupported("audio duration is unknown".to_owned()));
        }
    }
    Ok(())
}

fn validate_stream_side_data(
    stream: &format::stream::Stream<'_>,
    width: u32,
    height: u32,
) -> Result<[i32; 9]> {
    let mut matrix = None;
    for side_data in stream.side_data() {
        match side_data.kind() {
            ffmpeg::codec::packet::side_data::Type::ICC_PROFILE => {
                return Err(Error::Unsupported(
                    "ICC-profiled video has no approved color preservation path".to_owned(),
                ));
            }
            ffmpeg::codec::packet::side_data::Type::DisplayMatrix => {
                if matrix.is_some() {
                    return Err(Error::Unsupported("duplicate display matrix".into()));
                }
                matrix = Some(validate_display_matrix(side_data.data(), width, height)?);
            }
            _ => {}
        }
    }
    Ok(matrix.unwrap_or_else(crate::model::identity_display_matrix))
}

fn validate_display_matrix(data: &[u8], width: u32, height: u32) -> Result<[i32; 9]> {
    if data.len() != 9 * std::mem::size_of::<i32>() {
        return Err(Error::Unsupported(
            "display matrix coefficients are unavailable".into(),
        ));
    }
    let mut matrix = [0; 9];
    for (value, bytes) in matrix.iter_mut().zip(data.chunks_exact(4)) {
        *value = i32::from_ne_bytes(bytes.try_into().expect("i32-sized chunk"));
    }
    crate::model::display_matrix_rotation(&matrix, width, height)
        .ok_or_else(|| Error::Unsupported("unsupported display matrix".into()))?;
    Ok(matrix)
}

fn probe_video(
    input: &mut format::context::Input,
    index: usize,
    cancelled: &AtomicBool,
    exact: bool,
) -> Result<VideoInfo> {
    check_cancel(cancelled)?;
    let stream = input
        .stream(index)
        .ok_or_else(|| Error::Unsupported("video stream disappeared".to_owned()))?;
    let parameters = stream.parameters();
    let codec_name = parameters.id().name().to_owned();
    // SAFETY: copy scalar codec metadata before transferring parameters to the decoder.
    let (field_order, color_range, color_space, color_trc, color_primaries) = unsafe {
        let raw = &*parameters.as_ptr();
        (
            raw.field_order,
            raw.color_range,
            raw.color_space,
            raw.color_trc,
            raw.color_primaries,
        )
    };
    let mut decoder = codec::Context::from_parameters(parameters)
        .and_then(|context| context.decoder().video())
        .map_err(ffmpeg_error("open video decoder"))?;
    let rate = stream.avg_frame_rate().reduce();
    if rate.0 <= 0 || rate.1 <= 0 {
        return Err(Error::Unsupported(
            "video has no positive frame rate".to_owned(),
        ));
    }
    let time_base = stream.time_base();
    let start_seconds = seconds(stream.start_time(), time_base).unwrap_or(0.0);
    let declared_duration = seconds(stream.duration(), time_base);
    let sar = {
        let value = decoder.aspect_ratio();
        if value.0 > 0 && value.1 > 0 {
            value.reduce()
        } else {
            Rational(1, 1)
        }
    };
    let pixel_format = decoder.format();
    let alpha = pixel_format.descriptor().is_some_and(|descriptor| unsafe {
        (*descriptor.as_ptr()).flags & ffi::AV_PIX_FMT_FLAG_ALPHA as u64 != 0
    });
    let display_matrix = validate_stream_side_data(&stream, decoder.width(), decoder.height())?;
    let rotation_degrees =
        crate::model::display_matrix_rotation(&display_matrix, decoder.width(), decoder.height())
            .ok_or_else(|| Error::Unsupported("unsupported display matrix".into()))?;
    if let Some(value) = stream.metadata().get("rotate") {
        let tag: f64 = value
            .parse()
            .map_err(|_| Error::Unsupported("invalid rotation tag".into()))?;
        if !tag.is_finite() || (tag - rotation_degrees).rem_euclid(360.0) != 0.0 {
            return Err(Error::Unsupported(
                "rotation tag disagrees with display matrix".into(),
            ));
        }
    }
    let color = ColorInfo {
        range: enum_name(unsafe { ffi::av_color_range_name(color_range) }),
        space: enum_name(unsafe { ffi::av_color_space_name(color_space) }),
        transfer: enum_name(unsafe { ffi::av_color_transfer_name(color_trc) }),
        primaries: enum_name(unsafe { ffi::av_color_primaries_name(color_primaries) }),
    };
    let mut decoded = frame::Video::empty();
    let mut frames = 0_u64;
    let mut first_pts = None;
    let mut last_pts = None;
    let mut constant_frame_rate = true;
    let mut interlaced = !matches!(
        field_order,
        ffi::AVFieldOrder::AV_FIELD_PROGRESSIVE | ffi::AVFieldOrder::AV_FIELD_UNKNOWN
    );
    let mut first_keyframe = false;
    let mut max_keyframe_interval = 0_u64;
    let mut since_keyframe = 0_u64;
    let expected_ticks = (i128::from(time_base.1) * i128::from(rate.1)) as f64
        / (i128::from(time_base.0) * i128::from(rate.0)) as f64;
    while let Some((packet_stream, packet)) = read_packet(input, cancelled)? {
        if packet_stream != index {
            continue;
        }
        if packet.is_key() {
            if frames == 0 {
                first_keyframe = true;
            }
            max_keyframe_interval = max_keyframe_interval.max(since_keyframe);
            since_keyframe = 0;
        }
        decoder
            .send_packet(&packet)
            .map_err(ffmpeg_error("decode video packet"))?;
        drain_probe_frames(
            &mut decoder,
            &mut decoded,
            expected_ticks,
            &mut frames,
            &mut first_pts,
            &mut last_pts,
            &mut constant_frame_rate,
            &mut interlaced,
            &mut since_keyframe,
            cancelled,
            exact.then_some((time_base, rate)),
            &display_matrix,
        )?;
    }
    check_cancel(cancelled)?;
    decoder
        .send_eof()
        .map_err(ffmpeg_error("flush video decoder"))?;
    drain_probe_frames(
        &mut decoder,
        &mut decoded,
        expected_ticks,
        &mut frames,
        &mut first_pts,
        &mut last_pts,
        &mut constant_frame_rate,
        &mut interlaced,
        &mut since_keyframe,
        cancelled,
        exact.then_some((time_base, rate)),
        &display_matrix,
    )?;
    max_keyframe_interval = max_keyframe_interval.max(since_keyframe);
    if frames == 0 {
        return Err(Error::Unsupported(
            "video decoder produced no frames".to_owned(),
        ));
    }
    let duration_seconds =
        declared_duration.unwrap_or(frames as f64 * f64::from(rate.1) / f64::from(rate.0));
    Ok(VideoInfo {
        codec: codec_name,
        width: decoder.width(),
        height: decoder.height(),
        pixel_format: pixel_format
            .descriptor()
            .map_or("unknown", |value| value.name())
            .to_owned(),
        frame_rate: rational(rate),
        time_base: rational(time_base),
        start_seconds,
        duration_seconds,
        frames,
        constant_frame_rate,
        alpha,
        interlaced,
        sample_aspect_ratio: rational(sar),
        rotation_degrees,
        display_matrix,
        color,
        first_keyframe,
        max_keyframe_interval,
    })
}

#[allow(clippy::too_many_arguments)]
fn drain_probe_frames(
    decoder: &mut codec::decoder::Video,
    decoded: &mut frame::Video,
    expected_ticks: f64,
    frames: &mut u64,
    first_pts: &mut Option<i64>,
    last_pts: &mut Option<i64>,
    cfr: &mut bool,
    interlaced: &mut bool,
    since_keyframe: &mut u64,
    cancelled: &AtomicBool,
    exact_clock: Option<(Rational, Rational)>,
    display_matrix: &[i32; 9],
) -> Result<()> {
    loop {
        check_cancel(cancelled)?;
        match decoder.receive_frame(decoded) {
            Ok(()) => {
                if decoded
                    .side_data(ffmpeg::util::frame::side_data::Type::IccProfile)
                    .is_some()
                {
                    return Err(Error::Unsupported(
                        "ICC-profiled video has no approved color preservation path".to_owned(),
                    ));
                }
                if let Some(matrix) =
                    decoded.side_data(ffmpeg::util::frame::side_data::Type::DisplayMatrix)
                {
                    if validate_display_matrix(matrix.data(), decoder.width(), decoder.height())?
                        != *display_matrix
                    {
                        return Err(Error::Unsupported(
                            "frame display matrix differs from stream".into(),
                        ));
                    }
                }
                let pts = decoded.timestamp().or(decoded.pts()).ok_or_else(|| {
                    Error::Unsupported("decoded video frame has no timestamp".to_owned())
                })?;
                if let Some((time_base, rate)) = exact_clock {
                    if !crate::exact_frame_time(
                        Some(pts),
                        *frames,
                        &rational(time_base),
                        &rational(rate),
                    ) {
                        return Err(Error::Unsupported(
                            "AE preparation requires exact zero-origin decoded frame timestamps"
                                .into(),
                        ));
                    }
                }
                if first_pts.is_none() {
                    *first_pts = Some(pts);
                }
                if let Some(previous) = *last_pts {
                    let delta = (i128::from(pts) - i128::from(previous)) as f64;
                    if !crate::model::nominal_frame_delta(delta, expected_ticks) {
                        *cfr = false;
                    }
                }
                *last_pts = Some(pts);
                *interlaced |= decoded.is_interlaced();
                *frames += 1;
                *since_keyframe += 1;
            }
            Err(ffmpeg::Error::Eof)
            | Err(ffmpeg::Error::Other {
                errno: ffmpeg::util::error::EAGAIN,
            }) => break,
            Err(source) => {
                return Err(Error::Ffmpeg {
                    context: "receive decoded video frame",
                    source,
                });
            }
        }
    }
    Ok(())
}

fn open_audio_decoder(stream: &format::stream::Stream<'_>) -> Result<codec::decoder::Audio> {
    let mut decoder = codec::Context::from_parameters(stream.parameters())
        .map_err(ffmpeg_error("open audio decoder"))?
        .decoder();
    // libavcodec shifts timestamps past skipped priming samples (for example
    // an AAC edit list) only when it knows the packet time base.
    decoder.set_packet_time_base(stream.time_base());
    decoder.audio().map_err(ffmpeg_error("open audio decoder"))
}

fn probe_audio(
    path: &Path,
    index: usize,
    cancelled: &AtomicBool,
    ae_mp3: bool,
) -> Result<AudioInfo> {
    check_cancel(cancelled)?;
    let mut input = open_local_input(path)?;
    let stream = input
        .stream(index)
        .ok_or_else(|| Error::Unsupported("audio stream disappeared".to_owned()))?;
    let parameters = stream.parameters();
    let raw = unsafe { &*parameters.as_ptr() };
    let codec_name = parameters.id().name().to_owned();
    let sample_rate = raw.sample_rate.max(0) as u32;
    let channels = raw.ch_layout.nb_channels.max(0) as u32;
    let channel_layout = crate::model::canonical_audio_layout(
        input.format().name(),
        &codec_name,
        &channel_layout_name(&raw.ch_layout),
        channels,
    );
    let time_base = stream.time_base();
    // Container duration is the content duration; decoded AAC sample totals can
    // include codec priming/padding and must not silently lengthen the source.
    let declared_duration = seconds(stream.duration(), time_base).filter(|value| *value > 0.0);
    let mut decoder = open_audio_decoder(&stream)?;
    let sample_format = decoder.format().name().to_owned();
    if sample_rate == 0 || channels == 0 {
        return Err(Error::Unsupported(
            "audio sample rate or channel count is unknown".to_owned(),
        ));
    }
    let mut decoded = frame::Audio::empty();
    let mut timing = AudioProbeTiming::default();
    let mut first_audio_packet = true;
    let mut priming_samples = None;
    while let Some((packet_stream, packet)) = read_packet(&mut input, cancelled)? {
        if packet_stream != index {
            continue;
        }
        if first_audio_packet && ae_mp3 && codec_name == "mp3" {
            first_audio_packet = false;
            priming_samples = packet.side_data().find_map(|side_data| {
                if side_data.kind() != ffmpeg::codec::packet::side_data::Type::SkipSamples {
                    return None;
                }
                let data = side_data.data();
                (data.len() == 10)
                    .then(|| u32::from_le_bytes(data[0..4].try_into().expect("4-byte skip field")))
            });
        }
        decoder
            .send_packet(&packet)
            .map_err(ffmpeg_error("decode audio packet"))?;
        drain_probe_audio(
            &mut decoder,
            &mut decoded,
            time_base,
            sample_rate,
            &mut timing,
            cancelled,
        )?;
    }
    check_cancel(cancelled)?;
    decoder
        .send_eof()
        .map_err(ffmpeg_error("flush audio decoder"))?;
    drain_probe_audio(
        &mut decoder,
        &mut decoded,
        time_base,
        sample_rate,
        &mut timing,
        cancelled,
    )?;
    if timing.samples == 0 {
        return Err(Error::Unsupported(
            "audio decoder produced no samples".to_owned(),
        ));
    }
    let decoded_duration = timing.samples as f64 / f64::from(sample_rate);
    let duration_seconds = if ae_mp3 && codec_name == "mp3" {
        if priming_samples.map(i64::from) != timing.first_position || priming_samples == Some(0) {
            return Err(Error::Unsupported(
                "MP3 initial decoded timestamp does not match its first packet's skip-samples metadata".into(),
            ));
        }
        if declared_duration.is_some_and(|declared| {
            declared < decoded_duration
                || declared - decoded_duration > 2.0 * 1_152.0 / f64::from(sample_rate)
        }) {
            return Err(Error::Unsupported(
                "MP3 declared duration differs from decoded samples beyond priming and one frame of padding".into(),
            ));
        }
        decoded_duration
    } else {
        declared_duration.unwrap_or(decoded_duration)
    };
    Ok(AudioInfo {
        codec: codec_name,
        sample_format,
        sample_rate,
        channels,
        channel_layout,
        start_seconds: timing.first_position.unwrap_or(0) as f64 / f64::from(sample_rate),
        duration_seconds,
    })
}

#[derive(Default)]
struct AudioProbeTiming {
    first_position: Option<i64>,
    expected_end: Option<i64>,
    samples: u64,
}

fn drain_probe_audio(
    decoder: &mut codec::decoder::Audio,
    decoded: &mut frame::Audio,
    time_base: Rational,
    sample_rate: u32,
    timing: &mut AudioProbeTiming,
    cancelled: &AtomicBool,
) -> Result<()> {
    loop {
        check_cancel(cancelled)?;
        match decoder.receive_frame(decoded) {
            Ok(()) => {
                let timestamp = decoded.timestamp().or(decoded.pts()).ok_or_else(|| {
                    Error::Unsupported("decoded audio frame has no timestamp".to_owned())
                })?;
                let position =
                    i128::from(timestamp) * i128::from(time_base.0) * i128::from(sample_rate)
                        / i128::from(time_base.1.max(1));
                let position = i64::try_from(position).map_err(|_| {
                    Error::Unsupported("audio timestamp exceeds supported range".to_owned())
                })?;
                timing.first_position.get_or_insert(position);
                if let Some(expected) = timing.expected_end {
                    let tolerance = (i128::from(sample_rate) / 1000).max(1);
                    // Extreme hostile clocks must not overflow the validation itself.
                    let discontinuity = i128::from(position) - i128::from(expected);
                    if discontinuity.abs() > tolerance {
                        return Err(Error::Unsupported(format!(
                            "audio timestamp discontinuity of {discontinuity} samples"
                        )));
                    }
                }
                let count = i64::try_from(decoded.samples())
                    .map_err(|_| Error::Unsupported("audio frame is too large".to_owned()))?;
                timing.expected_end = Some(position.checked_add(count).ok_or_else(|| {
                    Error::Unsupported("audio timestamp exceeds supported range".to_owned())
                })?);
                timing.samples += u64::try_from(count)
                    .map_err(|_| Error::Unsupported("negative audio sample count".to_owned()))?;
            }
            Err(ffmpeg::Error::Eof)
            | Err(ffmpeg::Error::Other {
                errno: ffmpeg::util::error::EAGAIN,
            }) => break,
            Err(source) => {
                return Err(Error::Ffmpeg {
                    context: "receive decoded audio frame",
                    source,
                })
            }
        }
    }
    Ok(())
}

fn transcode_inner(job: &Job, progress: &mut dyn FnMut(f64), cancelled: &AtomicBool) -> Result<()> {
    check_cancel(cancelled)?;
    match job.profile {
        Profile::RemuxVideo => transcode_video(job, VideoMode::Copy, progress, cancelled),
        Profile::H264 => transcode_video(job, VideoMode::H264, progress, cancelled),
        Profile::Prores4444 => transcode_video(job, VideoMode::Prores, progress, cancelled),
        Profile::AudioPcm => transcode_audio_only(job, progress, cancelled),
    }
}

#[derive(Clone, Copy)]
enum VideoMode {
    Copy,
    H264,
    Prores,
}

include!("native/transcode.rs");

#[cfg(test)]
mod probe_tests {
    use super::*;

    #[test]
    fn operational_unsupported_is_fatal_but_exact_source_policy_can_fall_back() {
        for reason in ["initialize FFmpeg failed", "video stream disappeared"] {
            let error = public_result::<()>(Err(Error::Unsupported(reason.into()))).unwrap_err();
            assert!(matches!(
                error,
                super::super::TranscodeError::Backend { .. }
            ));
        }
        let cancelled = AtomicBool::new(false);
        let error = probe(Path::new("relative.mp4"), &cancelled, true).unwrap_err();
        assert!(matches!(error, super::super::TranscodeError::Policy(_)));
        let error = probe(Path::new("relative.mp4"), &cancelled, false).unwrap_err();
        assert!(matches!(
            error,
            super::super::TranscodeError::Backend { .. }
        ));
        let error = probe(Path::new("missing.mp4"), &AtomicBool::new(true), true).unwrap_err();
        assert!(matches!(error, super::super::TranscodeError::Cancelled));
    }

    #[test]
    fn profile_names_are_the_protocol_names() {
        assert_eq!(
            serde_json::to_string(&Profile::Prores4444).unwrap(),
            "\"prores4444\""
        );
        assert_eq!(
            serde_json::to_string(&Profile::AudioPcm).unwrap(),
            "\"audio_pcm\""
        );
    }

    #[test]
    fn relative_input_is_rejected_before_ffmpeg() {
        let error = validate_input(Path::new("clip.mp4")).unwrap_err();
        assert!(error.to_string().contains("absolute"));
    }

    #[test]
    fn already_cancelled_probe_does_not_touch_input() {
        let cancelled = AtomicBool::new(true);
        let error = probe(Path::new("definitely-missing.mp4"), &cancelled, false).unwrap_err();
        assert!(matches!(error, super::super::TranscodeError::Cancelled));
    }

    #[test]
    fn native_quarter_turn_matrices_are_accepted() {
        for descriptor in crate::tests::native_quarter_turn_descriptors() {
            let bytes: Vec<u8> = descriptor
                .display_matrix
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect();
            assert_eq!(
                validate_display_matrix(&bytes, descriptor.width, descriptor.height).unwrap(),
                descriptor.display_matrix
            );
            assert_eq!(
                crate::model::display_matrix_rotation(
                    &descriptor.display_matrix,
                    descriptor.width,
                    descriptor.height
                ),
                Some(descriptor.rotation_degrees)
            );
        }
    }

    #[test]
    fn malformed_display_matrix_length_is_rejected() {
        let bytes: Vec<u8> = crate::model::identity_display_matrix()
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        assert!(validate_display_matrix(&bytes[..35], 1920, 1080).is_err());
        let mut oversized = bytes;
        oversized.push(0);
        assert!(validate_display_matrix(&oversized, 1920, 1080).is_err());
    }

    #[test]
    fn zero_angle_mirrored_display_matrix_is_rejected() {
        let mirrored = [-(1 << 16), 0, 0, 0, 1 << 16, 0, 0, 0, 1 << 30];
        let bytes: Vec<u8> = mirrored
            .iter()
            .flat_map(|coefficient| i32::to_ne_bytes(*coefficient))
            .collect();
        let error = validate_display_matrix(&bytes, 1920, 1080).unwrap_err();
        assert!(error.to_string().contains("unsupported display matrix"));
    }
}
