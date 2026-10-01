//! Audio stream facts, tested on real container bytes without conversion.
//!
//! Five 200 ms samples under `tests/fixtures` hold 48 kHz sine tones with no
//! recorded content: mono 440 Hz, or 440 Hz left and 880 Hz right. They cover
//! PCM WAV, MP3 with gapless padding, audio-only M4A whose edit list excludes
//! AAC priming, and six 1080p30 H.264 frames with one AAC stream. Invalid inputs
//! are small in-memory edits of these samples, so ordinary tests do not need
//! FFmpeg. The WAV files are exact output of:
//!
//! ```python
//! import math, struct, wave
//! for name, tones in [("audio-mono.wav", [440]), ("audio-stereo.wav", [440, 880])]:
//!     with wave.open(name, "wb") as output:
//!         output.setparams((len(tones), 2, 48000, 9600, "NONE", "not compressed"))
//!         output.writeframes(b"".join(
//!             struct.pack("<h", int(8000 * math.sin(2 * math.pi * tone * frame / 48000)))
//!             for frame in range(9600) for tone in tones))
//! ```
//!
//! The compressed samples were encoded from them offline. Encoder versions can
//! change these bytes; tests compare packaged media with the checked-in files.
//!
//! ```sh
//! ffmpeg -i audio-stereo.wav -c:a libmp3lame -b:a 128k audio-stereo.mp3
//! ffmpeg -i audio-stereo.wav -c:a aac -b:a 128k audio-stereo.m4a
//! ffmpeg -i video-30fps.mp4 -i audio-stereo.wav -map 0:v:0 -map 1:a:0 -t 0.2 \
//!   -c:v libx264 -preset ultrafast -crf 35 -pix_fmt yuv420p -bf 0 \
//!   -c:a aac -b:a 128k video-with-audio.mp4
//! ```

use crate::{
    audio_media::{
        inspect_audio_media, padded_to_picture, validate_source, AudioDurationMatch, PictureClock,
    },
    format::FrameRate,
    schema::{AudioChannels, PrAudioStream, TICKS},
};
use std::io::Cursor;

const MONO: &[u8] = include_bytes!("../../tests/fixtures/audio-mono.wav");
const STEREO: &[u8] = include_bytes!("../../tests/fixtures/audio-stereo.wav");
const MP3: &[u8] = include_bytes!("../../tests/fixtures/audio-stereo.mp3");
const M4A: &[u8] = include_bytes!("../../tests/fixtures/audio-stereo.m4a");
const EMBEDDED: &[u8] = include_bytes!("../../tests/fixtures/video-with-audio.mp4");
const VIDEO_ONLY: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");

/// 200 ms at 48 kHz.
const FIXTURE_TICKS: i64 = 50_803_200_000;

#[test]
fn audio_inspection_recovers_layout_and_presentation_duration() {
    for (bytes, extension, channels) in [
        (MONO, "wav", AudioChannels::Mono),
        (STEREO, "wav", AudioChannels::Stereo),
        (MP3, "mp3", AudioChannels::Stereo),
        (M4A, "m4a", AudioChannels::Stereo),
        (EMBEDDED, "mp4", AudioChannels::Stereo),
    ] {
        let stream = inspect_audio_media(Cursor::new(bytes), bytes.len() as u64, extension)
            .unwrap()
            .unwrap();
        assert_eq!(
            (stream.channels, stream.sample_rate),
            (channels, 48_000),
            "{extension}"
        );
        // MP3 padding and AAC priming must not lengthen the presentation.
        assert_eq!(stream.intrinsic_ticks, FIXTURE_TICKS, "{extension}");
    }
    assert!(
        inspect_audio_media(Cursor::new(VIDEO_ONLY), VIDEO_ONLY.len() as u64, "mp4")
            .unwrap()
            .is_none()
    );
}

#[test]
fn embedded_audio_does_not_hide_the_video_sample_description() {
    let video = crate::media::inspect_video_media(
        Cursor::new(EMBEDDED),
        Cursor::new(EMBEDDED),
        EMBEDDED.len() as u64,
    )
    .unwrap();
    assert_eq!(
        (
            video.width,
            video.height,
            video.timing.supported().unwrap().0,
            video.timing.supported().unwrap().1
        ),
        (1920, 1080, FrameRate::Fps30, FIXTURE_TICKS)
    );
}

#[test]
fn audio_inspection_ignores_video_only_display_policy() {
    let mut rotated = EMBEDDED.to_vec();
    let tkhd = rotated
        .windows(4)
        .position(|bytes| bytes == b"tkhd")
        .unwrap();
    assert_eq!(rotated[tkhd + 4], 0, "version-0 track header");
    rotated[tkhd + 44] ^= 1;

    let audio = inspect_audio_media(Cursor::new(rotated.clone()), rotated.len() as u64, "mp4")
        .unwrap()
        .unwrap();
    assert_eq!(audio.channels, AudioChannels::Stereo);
    let error = crate::media::inspect_video_media(
        Cursor::new(rotated.clone()),
        Cursor::new(rotated.clone()),
        rotated.len() as u64,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("display transform"), "{error}");
}

#[test]
fn audio_inspection_rejects_surround_and_retimed_edit_lists() {
    let mut surround = STEREO.to_vec();
    surround[22..24].copy_from_slice(&6_u16.to_le_bytes());
    surround[28..32].copy_from_slice(&(48_000_u32 * 12).to_le_bytes());
    surround[32..34].copy_from_slice(&12_u16.to_le_bytes());
    let mut retimed = M4A.to_vec();
    let edit = retimed
        .windows(4)
        .position(|bytes| bytes == b"elst")
        .unwrap();
    retimed[edit + 20..edit + 22].copy_from_slice(&2_u16.to_be_bytes());
    for (bytes, extension, reason) in [
        (surround, "wav", "mono/stereo"),
        (retimed, "m4a", "edit-list retiming"),
        (b"not audio".to_vec(), "mp3", "audio"),
        // An AAC file saved under a WAV name, as Premiere projects can link it.
        (M4A.to_vec(), "wav", "MP4 container data"),
    ] {
        let error = inspect_audio_media(Cursor::new(bytes.clone()), bytes.len() as u64, extension)
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{extension}: {error}");
    }
}

/// Bytes 4..8 hold an MP4 file's first box type, `ftyp`, but a WAV's
/// little-endian RIFF size, which spells `ftyp` at 0x70797466 bytes. The
/// demuxer bounds chunks by that size, not by the stream, so a short edit
/// stands in for the 1,887,007,846-byte file.
#[test]
fn a_riff_size_that_spells_ftyp_is_not_mp4_data() {
    let mut wav = STEREO.to_vec();
    wav[4..8].copy_from_slice(&0x7079_7466_u32.to_le_bytes());
    assert_eq!(&wav[4..8], b"ftyp");
    let stream = inspect_audio_media(Cursor::new(wav.clone()), wav.len() as u64, "wav")
        .unwrap()
        .unwrap();
    assert_eq!(
        (stream.channels, stream.sample_rate, stream.intrinsic_ticks),
        (AudioChannels::Stereo, 48_000, FIXTURE_TICKS)
    );
    // A RIFF file that the demuxer rejects keeps the demuxer's reason.
    wav[8..12].copy_from_slice(b"WAVX");
    let error = inspect_audio_media(Cursor::new(wav.clone()), wav.len() as u64, "wav")
        .unwrap_err()
        .to_string();
    assert!(error.contains("riff form is not wave"), "{error}");
}

/// Muxers often give the sound the picture's duration, so the edit segment can
/// end after the last AAC sample. Premiere rounds that segment to whole samples
/// and stores the count as the stream duration, and so does inspection.
#[test]
fn audio_edit_list_may_run_past_the_last_sample() {
    let mut overrun = M4A.to_vec();
    let edit = overrun
        .windows(4)
        .position(|bytes| bytes == b"elst")
        .unwrap();
    // The fixture holds 10624 samples and skips 1024 of priming; 9744 runs 144 samples past the end.
    overrun[edit + 12..edit + 16].copy_from_slice(&9744_u32.to_be_bytes());
    let stream = inspect_audio_media(Cursor::new(overrun.clone()), overrun.len() as u64, "m4a")
        .unwrap()
        .unwrap();
    assert_eq!(stream.intrinsic_ticks, 9744 * 5_292_000);

    // A 90 kHz movie clock: 9601 units are 5120.53 samples, stored as 5121.
    let mut coarse = M4A.to_vec();
    let movie = coarse
        .windows(4)
        .position(|bytes| bytes == b"mvhd")
        .unwrap();
    coarse[movie + 16..movie + 20].copy_from_slice(&90_000_u32.to_be_bytes());
    coarse[edit + 12..edit + 16].copy_from_slice(&9601_u32.to_be_bytes());
    let stream = inspect_audio_media(Cursor::new(coarse.clone()), coarse.len() as u64, "m4a")
        .unwrap()
        .unwrap();
    assert_eq!(stream.intrinsic_ticks, 5121 * 5_292_000);

    let mut late_start = M4A.to_vec();
    late_start[edit + 16..edit + 20].copy_from_slice(&10624_u32.to_be_bytes());
    let error = inspect_audio_media(
        Cursor::new(late_start.clone()),
        late_start.len() as u64,
        "m4a",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("starts past the last sample"), "{error}");
}

/// Serves `bytes` until `fail_at`, then fails every read, as a removable or
/// network volume does when it drops.
struct FailingReader {
    bytes: Cursor<&'static [u8]>,
    fail_at: u64,
}

impl std::io::Read for FailingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let available = self.fail_at.saturating_sub(self.bytes.position());
        if available == 0 {
            return Err(std::io::Error::other("volume dropped"));
        }
        let length = buffer.len().min(usize::try_from(available).unwrap());
        self.bytes.read(&mut buffer[..length])
    }
}

impl std::io::Seek for FailingReader {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        self.bytes.seek(position)
    }
}

#[test]
fn a_failed_audio_read_stays_fatal() {
    use crate::{error::BuildError, media::unsupported_media_reason};
    for (bytes, fail_at) in [
        // The header is readable; the packets after it are not.
        (STEREO, 2048),
        // MP4 data under a WAV name whose first bytes cannot be read.
        (M4A, 0),
        // An MP4 header, then a RIFF marker prefix whose remaining bytes
        // cannot be read: the probe returns that read's error.
        (&b"\0\0\0\x18ftypmp42\0\0\0\0RIFF"[..], 18),
    ] {
        let reader = FailingReader {
            bytes: Cursor::new(bytes),
            fail_at,
        };
        let error = inspect_audio_media(reader, bytes.len() as u64, "wav").unwrap_err();
        assert!(
            matches!(
                unsupported_media_reason(error),
                Err(BuildError::Io(ref error)) if error.kind() == std::io::ErrorKind::Other
            ),
            "fail at {fail_at}"
        );
    }
}

#[test]
fn embedded_sound_may_carry_the_picture_duration_premiere_records() {
    let frame = FrameRate::Fps30.ticks_per_frame();
    let picture = PictureClock {
        duration_ticks: 400 * frame,
        frame_ticks: frame,
    };
    let sound = |ticks| PrAudioStream {
        intrinsic_ticks: ticks,
        channels: AudioChannels::Mono,
        sample_rate: 44_100,
    };
    // iPhone file: picture 13.333 s, AAC 586,530 samples (13.300 s).
    let file = sound(586_530 * (TICKS / 44_100));
    assert_eq!(
        validate_source(&file, &file, Some(picture)).unwrap(),
        AudioDurationMatch::Exact
    );
    assert_eq!(
        validate_source(&file, &sound(picture.duration_ticks), Some(picture)).unwrap(),
        AudioDurationMatch::PaddedToPicture
    );
    // Two AAC frames (46 ms) exceed one 30 fps frame and remain accepted.
    let two_aac_frames = sound(picture.duration_ticks - 2048 * (TICKS / 44_100));
    assert!(validate_source(
        &two_aac_frames,
        &sound(picture.duration_ticks),
        Some(picture)
    )
    .is_ok());
    let longer_gap = sound(picture.duration_ticks - 2049 * (TICKS / 44_100));
    assert!(validate_source(&longer_gap, &sound(picture.duration_ticks), Some(picture)).is_err());
    // Export writes the picture duration Premiere records for the same file.
    assert_eq!(
        padded_to_picture(&file, Some(picture)).unwrap(),
        Some(picture.duration_ticks)
    );
    assert_eq!(padded_to_picture(&longer_gap, Some(picture)).unwrap(), None);
    assert_eq!(padded_to_picture(&file, None).unwrap(), None);
    // The native duration must be the exact picture duration, never another length.
    assert!(validate_source(&file, &sound(picture.duration_ticks - 1), Some(picture)).is_err());
    // A longer file sound, or no measured picture, keeps the exact rule.
    assert!(validate_source(
        &sound(picture.duration_ticks + 1),
        &sound(picture.duration_ticks),
        Some(picture)
    )
    .is_err());
    assert!(validate_source(&file, &sound(picture.duration_ticks), None).is_err());
}
