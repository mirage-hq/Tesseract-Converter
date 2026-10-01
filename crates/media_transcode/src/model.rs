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
    pub color: ColorInfo,
    pub first_keyframe: bool,
    pub max_keyframe_interval: u64,
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

/// AIFF and WAVE define ordinary mono/stereo channel order even when a
/// demuxer omits the layout label. Never infer a multichannel or explicit layout.
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

pub fn canonical_audio_layout(container: &str, layout: &str, channels: u32) -> String {
    if matches!(container, "aiff" | "wav")
        && matches!(layout, "" | "unknown" | "1 channels" | "2 channels")
    {
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
    /// Label carried by a sole recognized MOV `tmcd` data stream.
    #[serde(default)]
    pub timecode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    RemuxVideo,
    H264,
    Prores4444,
    AudioPcm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub input: PathBuf,
    pub output: PathBuf,
    pub profile: Profile,
    pub source: MediaInfo,
    pub encoder: Option<String>,
}
