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

#[cfg(feature = "ffmpeg-library")]
use crate::tests::support::delayed_aac_bytes;
use crate::{
    audio_media::{
        inspect_audio_media, padded_to_picture, validate_source, AudioDurationMatch, PictureClock,
    },
    format::FrameRate,
    schema::{AudioChannels, PrAudioStream, TICKS},
};
use std::io::Cursor;

#[cfg(feature = "ffmpeg-library")]
const MONO: &[u8] = include_bytes!("../../tests/fixtures/audio-mono.wav");
const STEREO: &[u8] = include_bytes!("../../tests/fixtures/audio-stereo.wav");
#[cfg(feature = "ffmpeg-library")]
const MP3: &[u8] = include_bytes!("../../tests/fixtures/audio-stereo.mp3");
const M4A: &[u8] = include_bytes!("../../tests/fixtures/audio-stereo.m4a");
#[cfg(feature = "ffmpeg-library")]
const EMBEDDED: &[u8] = include_bytes!("../../tests/fixtures/video-with-audio.mp4");
#[cfg(feature = "ffmpeg-library")]
const VIDEO_ONLY: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");

/// 200 ms at 48 kHz.
const FIXTURE_TICKS: i64 = 50_803_200_000;

#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
#[test]
fn compressed_channel_failures_leave_no_partial_file_or_overwritten_output() {
    use crate::audio_media::copy_source_channel;
    let root = tempfile::tempdir().unwrap();
    for (bytes, extension) in [(MP3, "mp3"), (M4A, "m4a"), (STEREO, "wav")] {
        let source = root.path().join(format!("source.{extension}"));
        std::fs::write(&source, bytes).unwrap();
        let expected = inspect_audio_media(Cursor::new(bytes), bytes.len() as u64, extension)
            .unwrap()
            .unwrap();
        let output = root.path().join("channel.wav");
        let mut wrong_rate = expected.clone();
        wrong_rate.sample_rate = 44_100;
        let mut wrong_duration = expected.clone();
        wrong_duration.intrinsic_ticks += TICKS;
        for (channel, stream) in [(2, &expected), (0, &wrong_rate), (1, &wrong_duration)] {
            let error = copy_source_channel(&source, &output, channel, stream).unwrap_err();
            if extension == "m4a" && stream == &wrong_duration {
                assert!(
                    error.to_string().contains(
                        "native duration 57600 samples differs from file presentation 9600 samples"
                    ),
                    "{error}"
                );
                assert!(
                    error
                        .to_string()
                        .contains("cannot pad or shorten the full source"),
                    "{error}"
                );
            }
            assert!(!output.exists());
        }
        std::fs::write(&output, b"existing output").unwrap();
        assert!(copy_source_channel(&source, &output, 0, &expected).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"existing output");
        std::fs::remove_file(&output).unwrap();
        // A packet missing one source byte cannot be silently skipped or padded.
        std::fs::write(&source, &bytes[..bytes.len() - 1]).unwrap();
        assert!(copy_source_channel(&source, &output, 1, &expected).is_err());
        assert!(!output.exists());
        std::fs::remove_file(source).unwrap();
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn compressed_channel_aac_rejects_unproved_edit_origins_and_tail_padding() {
    let root = tempfile::tempdir().unwrap();
    let expected = inspect_audio_media(Cursor::new(M4A), M4A.len() as u64, "m4a")
        .unwrap()
        .unwrap();
    let edit = M4A.windows(4).position(|tag| tag == b"elst").unwrap();
    for (offset, replacement) in [
        (edit + 16, (-1_i32).to_be_bytes().to_vec()),
        (edit + 20, 2_i16.to_be_bytes().to_vec()),
        (edit + 16, 1025_i32.to_be_bytes().to_vec()),
    ] {
        let mut bytes = M4A.to_vec();
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
        let source = root.path().join("source.m4a");
        std::fs::write(&source, bytes).unwrap();
        let output = root.path().join("channel.wav");
        // The 200 ms presentation plus a 1025-sample origin exceeds the 10624
        // source samples. Do not infer a one-sample silence pad or shorten it.
        assert!(crate::audio_media::copy_source_channel(&source, &output, 0, &expected).is_err());
        assert!(!output.exists());
    }
}

#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
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
#[cfg(feature = "ffmpeg-library")]
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
pub(super) struct FailingReader {
    pub(super) bytes: Cursor<Vec<u8>>,
    pub(super) fail_at: u64,
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
            bytes: Cursor::new(bytes.to_vec()),
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
fn fractional_native_audio_duration_matches_only_its_measured_sample_ceiling() {
    let sound = |ticks, sample_rate| PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: ticks,
        channels: AudioChannels::Stereo,
        sample_rate,
    };
    // Unchanged Adobe Change Color sources, AudioStream 262 and 264:
    // native 122522.4 / 257857.6 samples; original AAC edit inspection
    // measures 122523 / 257858 whole samples at 48 kHz.
    for (native_ticks, file_ticks) in [
        (648_388_540_800, 648_391_716_000),
        (1_364_582_419_200, 1_364_584_536_000),
    ] {
        let native = sound(native_ticks, 48_000);
        let file = sound(file_ticks, 48_000);
        assert_eq!(
            validate_source(&file, &native, None).unwrap(),
            AudioDurationMatch::RoundedUpToSample
        );
    }
    for rate in [8_000, 44_100, 48_000, 192_000] {
        let sample = TICKS / i64::from(rate);
        let file = sound(TICKS, rate);
        // A fractional declaration occupies the same last physical sample.
        for delta in [1, sample / 2, sample - 1] {
            assert_eq!(
                validate_source(&file, &sound(TICKS - delta, rate), None).unwrap(),
                AudioDurationMatch::RoundedUpToSample
            );
        }
        // Whole-sample shifts, the opposite direction and a measured file
        // endpoint off the sample grid are not a ceiling identity.
        for (file_ticks, native_ticks) in [
            (TICKS, TICKS - sample),
            (TICKS, TICKS - sample - 1),
            (TICKS, TICKS + 1),
            (TICKS + 1, TICKS),
            (TICKS, 0),
            (TICKS, -1),
            (TICKS, i64::MIN),
        ] {
            assert!(
                validate_source(&sound(file_ticks, rate), &sound(native_ticks, rate), None)
                    .is_err(),
                "rate {rate}, file {file_ticks}, native {native_ticks}"
            );
        }
        let mut different_layout = sound(TICKS - 1, rate);
        different_layout.channels = AudioChannels::Mono;
        assert!(validate_source(&file, &different_layout, None).is_err());
        assert!(validate_source(&file, &sound(TICKS - 1, rate + 1), None).is_err());
    }
    for rate in [0, 7_999, 48_001, 192_001] {
        assert!(validate_source(&sound(TICKS, rate), &sound(TICKS - 1, rate), None).is_err());
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
        prepared_clock: None,
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

/// Synthetic edit-list discriminator over the existing AAC tone; not an Adobe oracle.

#[test]
#[cfg(feature = "ffmpeg-library")]
fn delayed_aac_import_retains_shared_editable_sound() {
    use crate::{
        format::MediaId,
        schema::PrAudioOccurrence,
        tests::support::{video_media, video_sequence},
    };
    let root = tempfile::tempdir().unwrap();
    let bytes = delayed_aac_bytes();
    let path = root.path().join("delayed.m4a");
    std::fs::write(&path, &bytes).unwrap();
    let mut media = video_media();
    let id = MediaId("source".into());
    let source = media.get_mut(&id).unwrap();
    source.video = None;
    source.relative_path = Some("delayed.m4a".into());
    source.relative_paths = vec!["delayed.m4a".into()];
    source.absolute_paths.clear();
    source.audio = Some(PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: FIXTURE_TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    });
    let mut sequence = video_sequence();
    sequence.video_tracks.clear();
    sequence.audio = (0..2)
        .map(|index| PrAudioOccurrence {
            id: Some(format!("sound-{index}")),
            media: id.clone(),
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            start_ticks: index * FIXTURE_TICKS,
            end_ticks: (index + 1) * FIXTURE_TICKS,
            in_ticks: 0,
            out_ticks: FIXTURE_TICKS,
            volume: fx_schema::LinearGain::new(0.5).unwrap(),
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        })
        .collect();
    sequence.timeline_end_ticks = 2 * FIXTURE_TICKS;
    let mut omissions = Vec::new();
    let pending = crate::tesseract_output::convert_premiere_sequence(
        &root.path().canonicalize().unwrap().join("input.prproj"),
        sequence,
        std::sync::Arc::new(media),
        &mut omissions,
    )
    .unwrap()
    .expect("delayed sound must not disappear");
    let archive = root.path().join("import.tsrct");
    pending.write_to_staging(&archive).unwrap();
    let file = tesseract_file::TesseractFile::open(&archive).unwrap();
    let doc = file.project_json().unwrap();
    let sounds: Vec<_> = doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .collect();
    assert_eq!(sounds.len(), 2, "{omissions:?}");
    assert_eq!(file.metadata().assets.len(), 1);
    for (index, sound) in sounds.iter().enumerate() {
        assert_eq!(
            sound["sourceRange"],
            serde_json::json!({"start":0,"duration":150})
        );
        assert_eq!(
            sound["playback"]["inputRange"],
            serde_json::json!({"start":index*200+50,"duration":150})
        );
        assert_eq!(sound["volume"], 0.5);
        assert_eq!(sound["sourceIntrinsicDuration"], 221);
    }
    let native = root.path().join("native");
    crate::tesseract_to_premiere(&archive, &native, false).unwrap();
    let reimport = root.path().join("reimport");
    crate::premiere_to_tesseract(native.join("project.prproj"), &reimport, None, false).unwrap();
    let reopened = tesseract_file::TesseractFile::open(
        std::fs::read_dir(reimport)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    let reopened_doc = reopened.project_json().unwrap();
    let reopened_sounds: Vec<_> = reopened_doc["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .collect();
    assert_eq!(reopened_sounds.len(), 2);
    for (before, after) in sounds.iter().zip(reopened_sounds) {
        assert_eq!(before["sourceRange"], after["sourceRange"]);
        assert_eq!(before["playback"], after["playback"]);
        assert_eq!(before["volume"], after["volume"]);
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn delayed_aac_clock_rejects_unsafe_edits_and_bounds() {
    use crate::audio_media::inspect_delayed_audio;
    let bytes = delayed_aac_bytes();
    let native = PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: FIXTURE_TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    };
    let clock = inspect_delayed_audio(Cursor::new(&bytes), bytes.len() as u64, &native)
        .unwrap()
        .unwrap();
    assert_eq!(clock.raw_ticks(), 10624 * 5_292_000);
    for (field, value) in [("duration", 1), ("channels", 1), ("rate", 1)] {
        let mut invalid = native.clone();
        match field {
            "duration" => invalid.intrinsic_ticks += TICKS / 1000,
            "channels" => invalid.channels = AudioChannels::Mono,
            "rate" => invalid.sample_rate = 44_100,
            _ => unreachable!(),
        }
        assert!(
            inspect_delayed_audio(Cursor::new(&bytes), bytes.len() as u64, &invalid).is_err(),
            "{field}/{value}"
        );
    }
    let edit = bytes.windows(4).position(|tag| tag == b"elst").unwrap();
    for (offset, payload) in [
        (24, 20000_u32.to_be_bytes().to_vec()), // playable segment exceeds raw media
        (28, 1_i32.to_be_bytes().to_vec()),     // not zero-origin
        (32, 2_u16.to_be_bytes().to_vec()),     // rate change
        (34, 1_u16.to_be_bytes().to_vec()),     // fractional rate
    ] {
        let mut invalid = bytes.clone();
        invalid[edit + offset..edit + offset + payload.len()].copy_from_slice(&payload);
        assert!(
            inspect_delayed_audio(Cursor::new(&invalid), invalid.len() as u64, &native).is_err(),
            "offset {offset}"
        );
    }
    let mut clip = crate::schema::PrAudioOccurrence {
        id: None,
        media: crate::schema::MediaId("sound".into()),
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        start_ticks: TICKS,
        end_ticks: TICKS + FIXTURE_TICKS,
        in_ticks: 0,
        out_ticks: FIXTURE_TICKS,
        volume: fx_schema::LinearGain::UNITY,
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    };
    let (timeline, source) = clock.window(&clip).unwrap().unwrap();
    assert_eq!(timeline, TICKS + TICKS / 20..TICKS + FIXTURE_TICKS);
    assert_eq!(source, 0..3 * TICKS / 20);
    for rate in [2.0, -2.0] {
        let mut retimed = clip.clone();
        retimed.playback_rate = rate;
        retimed.end_ticks = retimed.start_ticks + FIXTURE_TICKS / 2;
        let (timeline, raw) = clock.window(&retimed).unwrap().unwrap();
        assert_eq!(raw, 0..3 * TICKS / 20);
        assert_eq!(
            timeline,
            if rate > 0.0 {
                TICKS + TICKS / 40..TICKS + FIXTURE_TICKS / 2
            } else {
                TICKS..TICKS + 3 * TICKS / 40
            }
        );
        retimed.media = crate::schema::MediaId("source".into());
        retimed.volume_keys = Some(crate::schema::PrVolumeKeys {
            gain: 1.0,
            keys: vec![
                crate::schema::PrScalarKeyframe {
                    source_ticks: TICKS / 20,
                    value: 0.25,
                    easing: crate::schema::PrKeyframeEasing::Linear,
                },
                crate::schema::PrScalarKeyframe {
                    source_ticks: 3 * TICKS / 20,
                    value: 1.0,
                    easing: crate::schema::PrKeyframeEasing::Linear,
                },
            ],
        });
        let mut media = crate::tests::support::video_media();
        let source = media.get_mut(&retimed.media).unwrap();
        source.video = None;
        source.audio = Some(PrAudioStream {
            prepared_clock: Some(clock.clone()),
            intrinsic_ticks: clock.raw_ticks(),
            ..native.clone()
        });
        let mut sequence = crate::tests::support::video_sequence();
        sequence.video_tracks.clear();
        sequence.audio = vec![retimed];
        let wire = crate::tests::support::project_document_with_media(&sequence, &media);
        let keys = wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
            .as_array()
            .unwrap();
        let key_time = if rate > 0.0 { 0 } else { 100 };
        assert!(
            keys.iter()
                .any(|key| key["layerTime"] == key_time && key["value"]["value"] == 0.25),
            "{keys:?}"
        );
    }
    clip.out_ticks = TICKS / 40;
    clip.end_ticks = clip.start_ticks + clip.out_ticks;
    assert!(clock.window(&clip).unwrap().is_none());
    clip.in_ticks = TICKS / 10;
    clip.out_ticks = 3 * TICKS / 20;
    clip.end_ticks = clip.start_ticks + clip.out_ticks - clip.in_ticks;
    assert_eq!(
        clock.window(&clip).unwrap().unwrap().1,
        TICKS / 20..TICKS / 10
    );
    clip.out_ticks = FIXTURE_TICKS + 1;
    clip.end_ticks = clip.start_ticks + clip.out_ticks - clip.in_ticks;
    assert!(clock.window(&clip).is_err());
}

#[test]
#[cfg(feature = "ffmpeg-library")]
fn delayed_aac_preparation_is_deterministic_and_preserves_channel_samples() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let source = root.join("sound.m4a");
    std::fs::write(&source, delayed_aac_bytes()).unwrap();
    let expected = media_transcode::RawMovieAudio {
        sample_rate: 48_000,
        channels: 2,
        samples: 10624,
    };
    let first = root.join("first.wav");
    let second = root.join("second.wav");
    media_transcode::prepare_raw_movie_audio(&source, &first, expected).unwrap();
    media_transcode::prepare_raw_movie_audio(&source, &second, expected).unwrap();
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    let file = std::fs::File::open(&first).unwrap();
    let size = file.metadata().unwrap().len();
    let raw = inspect_audio_media(file, size, "wav").unwrap().unwrap();
    let mono = root.join("channel.wav");
    crate::audio_media::copy_source_channel(&first, &mono, 1, &raw).unwrap();
    let bytes = std::fs::read(&first).unwrap();
    let data = bytes.windows(4).position(|tag| tag == b"data").unwrap() + 8;
    let channel = std::fs::read(mono).unwrap();
    assert_eq!(channel.len(), 44 + 10624 * 4);
    let expected_channel: Vec<_> = bytes[data..data + 10624 * 8]
        .chunks_exact(8)
        .flat_map(|frame| frame[4..].iter().copied())
        .collect();
    assert_eq!(&channel[44..], expected_channel);
    let invalid = root.join("invalid.wav");
    assert!(media_transcode::prepare_raw_movie_audio(
        &source,
        &invalid,
        media_transcode::RawMovieAudio {
            channels: 1,
            ..expected
        }
    )
    .is_err());
    assert!(!invalid.exists());
    assert!(media_transcode::prepare_raw_movie_audio(
        &source,
        &invalid,
        media_transcode::RawMovieAudio {
            samples: 20000,
            ..expected
        }
    )
    .is_err());
    assert!(!invalid.exists());
    assert!(media_transcode::prepare_raw_movie_audio(
        &root.join("missing.m4a"),
        &invalid,
        expected
    )
    .is_err());
    assert!(!invalid.exists());
}
