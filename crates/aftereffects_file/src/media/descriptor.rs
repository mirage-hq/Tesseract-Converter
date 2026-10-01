use thiserror::Error;

use crate::{
    alias::{self, RelativeLocation},
    rifx::Chunk,
};

/// Photoshop footage identity pinned against Adobe-authored `sspc` records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhotoshopSource {
    /// The stored composite, not a recomposition of Photoshop layers.
    Merged,
    /// Persistent Photoshop layer ID plus its source-record index.
    Layer { id: u32, index: u32 },
}

const SSPC_MIN_LEN: usize = 184;

/// Rational source duration stored by AE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaDuration {
    pub numerator: u32,
    pub denominator: u32,
}

impl MediaDuration {
    pub fn seconds(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }
}

/// AE's integer plus 16-bit fractional frame-rate representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MediaFrameRate {
    pub integer: u32,
    pub fractional: u16,
}

impl MediaFrameRate {
    pub fn as_f64(self) -> f64 {
        f64::from(self.integer) + f64::from(self.fractional) / 65_536.0
    }
}

/// Render-relevant file-footage category derived from `sspc` source settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    StillImage,
    ImageSequence,
    Video,
    Audio,
    AudioVideo,
}

/// Main-source file metadata retained without accessing the local filesystem.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaDescriptor {
    pub source_format: [u8; 4],
    /// Native PSD footage selection; never inferred from a filename or layer name.
    pub photoshop_source: Option<PhotoshopSource>,
    pub width: u16,
    pub height: u16,
    pub duration: MediaDuration,
    pub native_frame_rate: MediaFrameRate,
    pub conform_frame_rate: MediaFrameRate,
    pub display_frame_rate: MediaFrameRate,
    pub pixel_aspect: (u32, u32),
    pub missing_at_save: bool,
    pub audio_sample_rate: f64,
    pub sequence_start_frame: u32,
    pub sequence_end_frame: u32,
    pub sequence_frame_padding: u32,
    pub sequence_frame_range_set: bool,
    pub authored_path: String,
    pub target_is_folder: bool,
    /// AE's native hint for finding `authored_path` after the project moved.
    pub relative_location: Option<RelativeLocation>,
    pub sequence_names: Vec<String>,
    pub kind: MediaKind,
}

/// Unsupported or malformed optional media metadata.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum MediaDecodeError {
    #[error("missing or duplicate media {0} chunk")]
    Chunk(&'static str),
    #[error("invalid media {0}")]
    Invalid(&'static str),
}

fn unique_data<'a>(
    children: &'a [Chunk],
    id: [u8; 4],
    label: &'static str,
) -> Result<&'a [u8], MediaDecodeError> {
    let mut matches = children.iter().filter(|chunk| chunk.id() == id);
    let Some(chunk) = matches.next() else {
        return Err(MediaDecodeError::Chunk(label));
    };
    if matches.next().is_some() {
        return Err(MediaDecodeError::Chunk(label));
    }
    chunk.data_payload().ok_or(MediaDecodeError::Invalid(label))
}

fn optional_unique_list<'a>(
    children: &'a [Chunk],
    kind: [u8; 4],
    label: &'static str,
) -> Result<Option<&'a [Chunk]>, MediaDecodeError> {
    let mut matches = children
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(kind));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(MediaDecodeError::Chunk(label));
    }
    first
        .map(|chunk| chunk.children().ok_or(MediaDecodeError::Invalid(label)))
        .transpose()
}

fn be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// The nonempty authored full path, and the rest of the alias metadata.
fn decode_alias(children: &[Chunk]) -> Result<(String, alias::AliasMetadata), MediaDecodeError> {
    let alias =
        optional_unique_list(children, *b"Als2", "Als2")?.ok_or(MediaDecodeError::Chunk("Als2"))?;
    let bytes = unique_data(alias, *b"alas", "alas")?;
    let mut metadata = alias::decode(bytes).map_err(|_| MediaDecodeError::Invalid("alas JSON"))?;
    let authored_path = metadata
        .fullpath
        .take()
        .filter(|path| !path.is_empty())
        .ok_or(MediaDecodeError::Invalid("alas fullpath"))?;
    Ok((authored_path, metadata))
}

fn decode_sequence_names(children: &[Chunk]) -> Result<Vec<String>, MediaDecodeError> {
    let Some(sequence) = optional_unique_list(children, *b"StVc", "StVc")? else {
        return Ok(Vec::new());
    };
    sequence
        .iter()
        .filter(|chunk| chunk.id() == *b"Utf8")
        .map(|chunk| {
            let bytes = chunk
                .data_payload()
                .ok_or(MediaDecodeError::Invalid("StVc Utf8 shape"))?;
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| MediaDecodeError::Invalid("StVc UTF-8"))
        })
        .collect()
}

fn classify(descriptor: &MediaDescriptor) -> Result<MediaKind, MediaDecodeError> {
    let visual = descriptor.width != 0 && descriptor.height != 0;
    if (descriptor.width == 0) != (descriptor.height == 0) {
        return Err(MediaDecodeError::Invalid("dimensions"));
    }
    let audio = descriptor.audio_sample_rate > 0.0;
    let sequence = descriptor.sequence_frame_padding > 0
        || descriptor.sequence_names.len() > 1
        || descriptor.target_is_folder;
    // AE gives still footage a display/native rate in some versions; a nonzero
    // source duration, not the rate alone, distinguishes a moving source.
    let moving = descriptor.duration.numerator > 0;
    match (visual, audio, sequence, moving) {
        (true, _, true, _) => Ok(MediaKind::ImageSequence),
        (true, true, false, true) => Ok(MediaKind::AudioVideo),
        (true, false, false, true) => Ok(MediaKind::Video),
        (true, _, false, false) => Ok(MediaKind::StillImage),
        (false, true, false, _) => Ok(MediaKind::Audio),
        _ => Err(MediaDecodeError::Invalid("source classification")),
    }
}

pub(super) fn decode(pin: &Chunk) -> Result<MediaDescriptor, MediaDecodeError> {
    let mut descriptor = decode_native(pin)?;
    if descriptor.source_format == *b"8BPS" {
        let children = pin
            .children()
            .ok_or(MediaDecodeError::Invalid("Pin shape"))?;
        let sspc = unique_data(children, *b"sspc", "sspc")?;
        if sspc.len() < 200 {
            return Err(MediaDecodeError::Invalid("Photoshop source selector"));
        }
        let id = be_u32(sspc, 188);
        let index = be_u32(sspc, 192);
        descriptor.photoshop_source = Some(match (id, index, be_u32(sspc, 196)) {
            (u32::MAX, u32::MAX, 1) => PhotoshopSource::Merged,
            (id, index, 0) if id != u32::MAX && index != u32::MAX => {
                PhotoshopSource::Layer { id, index }
            }
            _ => return Err(MediaDecodeError::Invalid("Photoshop source selector")),
        });
    }
    Ok(descriptor)
}

/// Decodes source identity and render kind without image-specific metadata.
pub(super) fn decode_native(pin: &Chunk) -> Result<MediaDescriptor, MediaDecodeError> {
    let children = pin
        .children()
        .ok_or(MediaDecodeError::Invalid("Pin shape"))?;
    let sspc = unique_data(children, *b"sspc", "sspc")?;
    if sspc.len() < SSPC_MIN_LEN {
        return Err(MediaDecodeError::Invalid("sspc layout"));
    }
    let duration = MediaDuration {
        numerator: be_u32(sspc, 38),
        denominator: be_u32(sspc, 42),
    };
    if duration.denominator == 0 {
        return Err(MediaDecodeError::Invalid("duration denominator"));
    }
    let pixel_aspect = (be_u32(sspc, 136), be_u32(sspc, 140));
    if pixel_aspect.0 == 0 || pixel_aspect.1 == 0 {
        return Err(MediaDecodeError::Invalid("pixel aspect"));
    }
    let audio_sample_rate = f64::from_be_bytes(
        sspc[160..168]
            .try_into()
            .map_err(|_| MediaDecodeError::Invalid("audio sample rate"))?,
    );
    if !audio_sample_rate.is_finite() || audio_sample_rate < 0.0 {
        return Err(MediaDecodeError::Invalid("audio sample rate"));
    }
    let (authored_path, alias) = decode_alias(children)?;
    let sequence_names = decode_sequence_names(children)?;
    let mut descriptor = MediaDescriptor {
        photoshop_source: None,
        source_format: sspc[22..26]
            .try_into()
            .map_err(|_| MediaDecodeError::Invalid("source format"))?,
        width: be_u16(sspc, 32),
        height: be_u16(sspc, 36),
        duration,
        native_frame_rate: MediaFrameRate {
            integer: be_u32(sspc, 56),
            fractional: be_u16(sspc, 60),
        },
        conform_frame_rate: MediaFrameRate {
            integer: u32::from(be_u16(sspc, 148)),
            fractional: be_u16(sspc, 150),
        },
        display_frame_rate: MediaFrameRate {
            integer: u32::from(be_u16(sspc, 152)),
            fractional: be_u16(sspc, 154),
        },
        pixel_aspect,
        missing_at_save: sspc[115] != 0,
        audio_sample_rate,
        sequence_start_frame: be_u32(sspc, 172),
        sequence_end_frame: be_u32(sspc, 176),
        sequence_frame_padding: be_u32(sspc, 180),
        sequence_frame_range_set: sspc.get(185).is_some_and(|value| *value != 0),
        authored_path,
        target_is_folder: alias.target_is_folder,
        relative_location: alias.relative_location,
        sequence_names,
        kind: MediaKind::StillImage,
    };
    descriptor.kind = classify(&descriptor)?;
    Ok(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media_pin(
        path: &str,
        dimensions: [u16; 2],
        duration: (u32, u32),
        sample_rate: f64,
    ) -> Chunk {
        let mut sspc = vec![0; 186];
        sspc[22..26].copy_from_slice(b"MOoV");
        sspc[32..34].copy_from_slice(&dimensions[0].to_be_bytes());
        sspc[36..38].copy_from_slice(&dimensions[1].to_be_bytes());
        sspc[38..42].copy_from_slice(&duration.0.to_be_bytes());
        sspc[42..46].copy_from_slice(&duration.1.to_be_bytes());
        sspc[56..60].copy_from_slice(&24u32.to_be_bytes());
        sspc[136..140].copy_from_slice(&1u32.to_be_bytes());
        sspc[140..144].copy_from_slice(&1u32.to_be_bytes());
        sspc[152..154].copy_from_slice(&24u16.to_be_bytes());
        sspc[160..168].copy_from_slice(&sample_rate.to_be_bytes());
        let alias = serde_json::json!({
            "fullpath": path,
            "target_is_folder": false,
        })
        .to_string();
        Chunk::list(
            *b"Pin ",
            vec![
                Chunk::data(*b"sspc", sspc).unwrap(),
                Chunk::data(*b"opti", Vec::new()).unwrap(),
                Chunk::list(*b"Als2", vec![Chunk::data(*b"alas", alias).unwrap()]),
            ],
        )
    }

    #[test]
    fn psd_short_or_inconsistent_selectors_are_not_guessed() {
        for (length, id, index, merged) in [
            (186, 0, 0, 0),
            (199, 0, 0, 0),
            (200, u32::MAX, 0, 1),
            (200, u32::MAX, u32::MAX, 0),
            (200, 101, 0, 1),
            (200, 101, 0, 2),
        ] {
            let mut pin = media_pin("source.psd", [64, 48], (0, 1), 0.0);
            let children = pin.children_mut().unwrap();
            let mut settings = children[0].data_payload().unwrap().to_vec();
            settings.resize(200, 0);
            settings[22..26].copy_from_slice(b"8BPS");
            settings[188..192].copy_from_slice(&id.to_be_bytes());
            settings[192..196].copy_from_slice(&index.to_be_bytes());
            settings[196..200].copy_from_slice(&u32::to_be_bytes(merged));
            settings.truncate(length);
            children[0] = Chunk::data(*b"sspc", settings).unwrap();
            assert_eq!(
                decode(&pin).unwrap_err(),
                MediaDecodeError::Invalid("Photoshop source selector")
            );
            let native = decode_native(&pin).unwrap();
            assert_eq!(native.kind, MediaKind::StillImage);
            assert_eq!(native.source_format, *b"8BPS");
            assert_eq!(native.photoshop_source, None);
        }
    }

    #[test]
    fn decodes_video_audio_and_authored_path_without_opening_it() {
        let descriptor = decode(&media_pin(
            r"C:\source\clip.mov",
            [1920, 1080],
            (5, 1),
            48_000.0,
        ))
        .unwrap();
        assert_eq!(descriptor.kind, MediaKind::AudioVideo);
        assert_eq!(descriptor.source_format, *b"MOoV");
        assert_eq!((descriptor.width, descriptor.height), (1920, 1080));
        assert_eq!(descriptor.duration.seconds(), 5.0);
        assert_eq!(descriptor.native_frame_rate.as_f64(), 24.0);
        assert_eq!(descriptor.audio_sample_rate, 48_000.0);
        assert_eq!(descriptor.authored_path, r"C:\source\clip.mov");
    }

    #[test]
    fn authored_alias_text_above_the_former_limit_is_retained() {
        let oversized_path = format!("/tmp/{}", "x".repeat(64 * 1024 + 1));
        let pin = media_pin(&oversized_path, [640, 480], (0, 1), 0.0);

        assert_eq!(decode(&pin).unwrap().authored_path, oversized_path);
    }

    #[test]
    fn sequence_names_above_the_former_count_and_byte_limits_are_retained() {
        let names = (0..=10_000)
            .map(|index| Chunk::data(*b"Utf8", format!("{index}.png").into_bytes()).unwrap())
            .collect();
        let count_sequence = Chunk::list(*b"StVc", names);
        let decoded = decode_sequence_names(&[count_sequence]).unwrap();
        assert_eq!(decoded.len(), 10_001);
        assert_eq!(decoded.last().unwrap(), "10000.png");

        let long_name = "x".repeat(1_048_576 + 1);
        let byte_sequence = Chunk::list(
            *b"StVc",
            vec![Chunk::data(*b"Utf8", long_name.as_bytes().to_vec()).unwrap()],
        );
        assert_eq!(
            decode_sequence_names(&[byte_sequence]).unwrap(),
            [long_name]
        );
    }

    #[test]
    fn classifies_still_audio_and_sequence_sources() {
        let still = decode(&media_pin("/tmp/frame.png", [640, 480], (0, 1), 0.0)).unwrap();
        assert_eq!(still.kind, MediaKind::StillImage);

        let audio = media_pin("/tmp/sound.wav", [0, 0], (3, 1), 44_100.0);
        let descriptor = decode(&audio).unwrap();
        assert_eq!(descriptor.kind, MediaKind::Audio);

        let mut sequence = media_pin("/tmp/frames", [640, 480], (0, 1), 0.0);
        sequence.children_mut().unwrap().push(Chunk::list(
            *b"StVc",
            vec![
                Chunk::data(*b"Utf8", b"f001.png".to_vec()).unwrap(),
                Chunk::data(*b"Utf8", b"f002.png".to_vec()).unwrap(),
            ],
        ));
        assert_eq!(decode(&sequence).unwrap().kind, MediaKind::ImageSequence);
    }

    #[test]
    fn rejects_duplicate_missing_and_malformed_optional_chunks() {
        let mut duplicate = media_pin("/tmp/clip.mov", [10, 10], (1, 1), 0.0);
        duplicate.children_mut().unwrap().push(Chunk::list(
            *b"Als2",
            vec![Chunk::data(*b"alas", br#"{"fullpath":"other"}"#.to_vec()).unwrap()],
        ));
        assert_eq!(decode(&duplicate), Err(MediaDecodeError::Chunk("Als2")));

        let missing = Chunk::list(
            *b"Pin ",
            vec![Chunk::data(*b"sspc", vec![0; SSPC_MIN_LEN]).unwrap()],
        );
        assert!(matches!(
            decode(&missing),
            Err(MediaDecodeError::Invalid("duration denominator"))
        ));

        let mut malformed = media_pin("/tmp/clip.mov", [10, 10], (1, 1), 0.0);
        let alias = malformed
            .children_mut()
            .unwrap()
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"Als2"))
            .unwrap();
        *alias.children_mut().unwrap().first_mut().unwrap() =
            Chunk::data(*b"alas", b"[]".to_vec()).unwrap();
        assert_eq!(
            decode(&malformed),
            Err(MediaDecodeError::Invalid("alas JSON"))
        );
    }

    #[test]
    fn retains_missing_state_and_sequence_fields() {
        let mut pin = media_pin("/tmp/frames", [100, 50], (2, 1), 0.0);
        let sspc = pin.children_mut().unwrap()[0].children_mut();
        assert!(sspc.is_none());
        let children = pin.children_mut().unwrap();
        let bytes = children[0].data_payload().unwrap().to_vec();
        let mut bytes = bytes;
        bytes[115] = 1;
        bytes[172..176].copy_from_slice(&7u32.to_be_bytes());
        bytes[176..180].copy_from_slice(&9u32.to_be_bytes());
        bytes[180..184].copy_from_slice(&4u32.to_be_bytes());
        bytes[185] = 1;
        children[0] = Chunk::data(*b"sspc", bytes).unwrap();
        let descriptor = decode(&pin).unwrap();
        assert!(descriptor.missing_at_save);
        assert_eq!(
            (
                descriptor.sequence_start_frame,
                descriptor.sequence_end_frame
            ),
            (7, 9)
        );
        assert_eq!(descriptor.sequence_frame_padding, 4);
        assert!(descriptor.sequence_frame_range_set);
    }
}
