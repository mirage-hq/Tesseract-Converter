//! Media descriptions and jobs shared by the supported backends.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ratio {
    pub num: i32,
    pub den: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorInfo {
    pub range: String,
    pub space: String,
    pub transfer: String,
    pub primaries: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub frame_rate: Ratio,
    pub time_base: Ratio,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub frames: u64,
    pub constant_frame_rate: bool,
    pub alpha: bool,
    pub interlaced: bool,
    pub sample_aspect_ratio: Ratio,
    pub rotation_degrees: f64,
    /// Exact FFmpeg fixed-point display transform; absent legacy metadata means identity.
    #[serde(default = "identity_display_matrix")]
    pub display_matrix: [i32; 9],
    pub color: ColorInfo,
    pub first_keyframe: bool,
    pub max_keyframe_interval: u64,
}

pub(crate) const fn identity_display_matrix() -> [i32; 9] {
    [65_536, 0, 0, 0, 65_536, 0, 0, 0, 1 << 30]
}

/// Accept only unit quarter turns, with zero or the translation that puts the
/// transformed coded rectangle in positive bounds. Coefficients use FFmpeg's
/// row-vector layout (16.16 linear/translation, 2.30 perspective). This matches
/// Premiere import's bounded `media_metadata::video_orientation` contract.
pub(crate) fn display_matrix_rotation(matrix: &[i32; 9], width: u32, height: u32) -> Option<f64> {
    let [a, b, u, c, d, v, x, y, w] = *matrix;
    if width == 0 || height == 0 || u != 0 || v != 0 || w != 1 << 30 {
        return None;
    }
    // Match ffprobe / av_display_rotation_get, not its negation.
    let rotation = match [a, b, c, d] {
        [65_536, 0, 0, 65_536] => 0.0,
        [0, 65_536, -65_536, 0] => -90.0,
        [-65_536, 0, 0, -65_536] => -180.0,
        [0, -65_536, 65_536, 0] => 90.0,
        _ => return None,
    };
    let bounds_x =
        -i64::from(a.min(0)) * i64::from(width) - i64::from(c.min(0)) * i64::from(height);
    let bounds_y =
        -i64::from(b.min(0)) * i64::from(width) - i64::from(d.min(0)) * i64::from(height);
    ((x == 0 && y == 0) || (i64::from(x) == bounds_x && i64::from(y) == bounds_y))
        .then_some(rotation)
}

/// A quantized nominal clock may alternate between floor/ceil tick intervals.
/// The tolerance covers floating-point ratio arithmetic, not timestamp serialization or missing frames.
pub(crate) fn nominal_frame_delta(delta: f64, expected_ticks: f64) -> bool {
    if !delta.is_finite() || !expected_ticks.is_finite() || delta <= 0.0 || expected_ticks <= 0.0 {
        return false;
    }
    let tolerance = expected_ticks.abs() * 1e-5;
    (delta - expected_ticks.floor()).abs() <= tolerance
        || (delta - expected_ticks.ceil()).abs() <= tolerance
}

impl VideoInfo {
    /// Formats whose alpha plane can be preserved by the verified 8-bit ProRes profile.
    pub fn has_eight_bit_alpha(&self) -> bool {
        self.alpha
            && matches!(
                self.pixel_format.as_str(),
                "argb"
                    | "rgba"
                    | "abgr"
                    | "bgra"
                    | "gbrap"
                    | "ya8"
                    | "yuva420p"
                    | "yuva422p"
                    | "yuva444p"
                    | "vuya"
            )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub codec: String,
    pub sample_format: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_layout: String,
    pub start_seconds: f64,
    pub duration_seconds: f64,
}

/// Bounds the MP3 decoder's first decoded timestamp to one 1152-sample
/// MPEG-1 Layer III frame. Only the AE destination route accepts this origin;
/// ordinary transcoding still requires a zero-start source.
pub(crate) fn ae_mp3_priming_samples(audio: &AudioInfo) -> Option<i64> {
    if audio.codec != "mp3" || audio.sample_rate == 0 || !audio.start_seconds.is_finite() {
        return None;
    }
    let frames = audio.start_seconds * f64::from(audio.sample_rate);
    (frames > 0.0
        && frames <= 1_152.0
        && (frames - frames.round()).abs() < 0.001
        && audio.duration_seconds.is_finite()
        && audio.duration_seconds > 0.0)
        .then_some(frames.round() as i64)
}

pub fn detected_container(format_name: &str, major_brand: Option<&str>) -> String {
    if !format_name.split(',').any(|name| name == "mov") {
        return format_name.to_owned();
    }

    let brand = major_brand.map(str::trim).unwrap_or_default();
    if brand == "qt" {
        return "mov".to_owned();
    }
    if matches!(
        brand,
        "isom"
            | "iso2"
            | "iso3"
            | "iso4"
            | "iso5"
            | "iso6"
            | "avc1"
            | "mp41"
            | "mp42"
            | "M4A"
            | "M4B"
            | "M4P"
            | "M4V"
            | "MSNV"
            | "dash"
    ) {
        return "mp4".to_owned();
    }

    format_name.to_owned()
}

/// WAVE/AIFF and QuickTime PCM's ordinary one/two-channel sound descriptions
/// imply mono/stereo when the optional channel-layout atom is absent. Do not
/// reinterpret explicit layouts or infer speaker positions for multichannel PCM.
/// See [QuickTime Sound Sample Descriptions](https://developer.apple.com/library/archive/documentation/QuickTime/QTFF/QTFFChap3/qtff3.html).
pub fn canonical_audio_layout(container: &str, codec: &str, layout: &str, channels: u32) -> String {
    let implicit_pcm = codec.starts_with("pcm_")
        && container
            .split(',')
            .any(|name| matches!(name, "mov" | "mp4"));
    let unspecified = matches!(layout, "" | "unknown")
        || (channels == 1 && layout == "1 channels")
        || (channels == 2 && layout == "2 channels");
    if (matches!(container, "aiff" | "wav") || implicit_pcm) && unspecified {
        match channels {
            1 => return "mono".into(),
            2 => return "stereo".into(),
            _ => {}
        }
    }
    layout.to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub version: String,
    pub encoders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub version: String,
    /// Demuxer-reported format names, without inferring from the file suffix.
    #[serde(default)]
    pub container: String,
    pub video: Option<VideoInfo>,
    pub audio: Option<AudioInfo>,
    /// Label carried by the recognized MOV `tmcd` data stream.
    #[serde(default)]
    pub timecode: Option<String>,
    /// Absolute input stream index, not the ordinal among data streams.
    #[serde(default)]
    pub timecode_stream_index: Option<usize>,
    /// Recognized ancillary tracks; only byte-for-byte copies retain their payloads.
    #[serde(default)]
    pub camera_metadata: Vec<CameraMetadata>,
}

/// Camera data that FFmpeg's MOV muxer cannot preserve as valid sample entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraMetadata {
    pub stream_index: usize,
    pub codec_tag: String,
    /// A label in this track is not evidence of a separate, preservable tmcd track.
    pub has_timecode_label: bool,
}

#[derive(Debug, Default)]
pub(crate) struct DataStreams {
    pub timecode: Option<String>,
    pub timecode_stream_index: Option<usize>,
    pub camera_metadata: Vec<CameraMetadata>,
}

impl DataStreams {
    pub(crate) fn observe(
        &mut self,
        container: &str,
        index: usize,
        tag: &[u8],
        timecode: Option<&str>,
    ) -> Result<(), &'static str> {
        if !container.split(',').any(|name| name == "mov") {
            return Err("data streams require the MOV/MP4 demuxer");
        }
        let label = timecode.filter(|value| !value.is_empty());
        match tag {
            b"tmcd" => {
                if self.timecode_stream_index.is_some() {
                    return Err("multiple MOV tmcd streams are unsupported");
                }
                self.timecode = Some(label.ok_or("MOV tmcd stream has no timecode label")?.into());
                self.timecode_stream_index = Some(index);
            }
            b"rtmd" | b"mebx" => self.camera_metadata.push(CameraMetadata {
                stream_index: index,
                codec_tag: String::from_utf8_lossy(tag).into_owned(),
                has_timecode_label: label.is_some(),
            }),
            _ => return Err("only MOV tmcd, rtmd and mebx data streams are supported"),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    RemuxVideo,
    H264,
    Prores4444,
    AudioPcm,
}

/// Container timing policy selected by the project-aware caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    #[default]
    General,
    /// Zero-origin video-only QuickTime; the caller must still validate native admission.
    AfterEffects,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub input: PathBuf,
    pub output: PathBuf,
    pub profile: Profile,
    pub source: MediaInfo,
    pub encoder: Option<String>,
    #[serde(default)]
    pub destination: Destination,
}
