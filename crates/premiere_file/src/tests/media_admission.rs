//! Import admission follows the sound placements, not an unused embedded stream.
//! Model tests cover embedded sound selection and nested placements.

#[cfg(feature = "ffmpeg-library")]
use super::support::{nest_of, video_media, video_sequence};
#[cfg(feature = "ffmpeg-library")]
use crate::{
    format::FrameRate,
    schema::{AudioChannels, PrAudioOccurrence, PrAudioStream, PrVideoTrack, TICKS},
};
use crate::{
    format::{MediaId, PrMedia, PrSequence},
    tesseract_output::{convert_premiere_sequence, PendingTesseractFile},
    Omission,
};
#[cfg(feature = "ffmpeg-library")]
use fx_schema::LinearGain;
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;

#[cfg(feature = "ffmpeg-library")]
const EMBEDDED: &[u8] = include_bytes!("../../tests/fixtures/video-with-audio.mp4");
#[cfg(feature = "ffmpeg-library")]
const VIDEO_ONLY: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");
#[cfg(feature = "ffmpeg-library")]
const AUDIO_TICKS: i64 = TICKS / 5;

#[cfg(feature = "ffmpeg-library")]
fn fixture(bytes: &[u8]) -> (tempfile::TempDir, PrSequence, BTreeMap<MediaId, PrMedia>) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("source.mp4"), bytes).unwrap();
    let mut media = video_media();
    let source = media.get_mut(&MediaId("source".into())).unwrap();
    source.relative_path = Some("source.mp4".into());
    source.relative_paths = vec!["source.mp4".into()];
    source.video.as_mut().unwrap().intrinsic_ticks = if bytes == EMBEDDED {
        AUDIO_TICKS
    } else {
        TICKS
    };
    source.audio = Some(PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: AUDIO_TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    });
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.id = Some("picture".into());
    clip.start_ticks = FrameRate::Fps30.ticks_per_frame();
    clip.end_ticks = AUDIO_TICKS;
    clip.in_ticks = FrameRate::Fps30.ticks_per_frame();
    clip.out_ticks = AUDIO_TICKS;
    sequence.timeline_end_ticks = AUDIO_TICKS;
    (directory, sequence, media)
}

#[cfg(feature = "ffmpeg-library")]
fn sound() -> PrAudioOccurrence {
    PrAudioOccurrence {
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        id: Some("sound".into()),
        media: MediaId("source".into()),
        start_ticks: 0,
        end_ticks: AUDIO_TICKS,
        in_ticks: 0,
        out_ticks: AUDIO_TICKS,
        volume: LinearGain::UNITY,
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    }
}

#[cfg(feature = "ffmpeg-library")]
fn nested(sequence: PrSequence) -> PrSequence {
    let mut outer = video_sequence();
    outer.video_tracks = vec![PrVideoTrack {
        items: Vec::new(),
        nests: vec![nest_of(sequence, 0..AUDIO_TICKS, 0)],
        transitions: Vec::new(),
    }];
    outer.timeline_end_ticks = AUDIO_TICKS;
    outer
}

fn convert(
    root: &Path,
    sequence: PrSequence,
    media: BTreeMap<MediaId, PrMedia>,
    omissions: &mut Vec<Omission>,
) -> Option<PendingTesseractFile> {
    convert_premiere_sequence(
        &root.canonicalize().unwrap().join("project.prproj"),
        sequence,
        Arc::new(media),
        omissions,
    )
    .unwrap()
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unconsumed_audio_keeps_valid_picture_including_nested_placements() {
    for bytes in [EMBEDDED, VIDEO_ONLY] {
        for in_nest in [false, true] {
            let (directory, sequence, mut media) = fixture(bytes);
            // A source may declare a different audio layout/rate or have no
            // embedded sound at all. Neither changes this picture-only timeline.
            media
                .get_mut(&MediaId("source".into()))
                .unwrap()
                .audio
                .as_mut()
                .unwrap()
                .sample_rate = 44_100;
            let sequence = if in_nest { nested(sequence) } else { sequence };
            let mut omissions = Vec::new();
            let pending = convert(directory.path(), sequence, media, &mut omissions)
                .expect("unused sound must not omit the picture");
            assert!(omissions.is_empty(), "{omissions:?}");
            let path = directory.path().join("converted.tsrct");
            pending.write_to_staging(&path).unwrap();
            let file = TesseractFile::open(path).unwrap();
            let document = file.project_json().unwrap();
            let layers = document["composition"]["layers"].as_array().unwrap();
            let layers = if in_nest {
                layers[0]["layers"].as_array().unwrap()
            } else {
                layers
            };
            let picture = layers
                .iter()
                .find(|layer| layer["type"] == "Video")
                .unwrap();
            assert_eq!(picture["source"]["assetId"], "premiere-video-1");
            assert_eq!(picture["volume"], 0.0);
            assert_eq!(picture["playback"]["inputRange"]["start"], 33);
            assert_eq!(picture["playback"]["inputRange"]["duration"], 167);
            assert_eq!(picture["sourceRange"]["start"], 33);
            assert_eq!(picture["sourceRange"]["duration"], 167);
            assert!(layers.iter().all(|layer| layer["type"] != "Audio"));
            let mut packaged = Vec::new();
            std::io::Read::read_to_end(
                &mut file.asset("premiere-video-1").unwrap().open().unwrap(),
                &mut packaged,
            )
            .unwrap();
            assert_eq!(packaged, bytes);
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_audio_still_checks_rate_duration_and_presence_including_nests() {
    for (bytes, sample_rate, duration, reason) in [
        (
            EMBEDDED,
            44_100,
            AUDIO_TICKS,
            "layout or sample rate differs",
        ),
        (EMBEDDED, 48_000, AUDIO_TICKS + TICKS, "Duration"),
        (VIDEO_ONLY, 48_000, AUDIO_TICKS, "audio stream is missing"),
    ] {
        for in_nest in [false, true] {
            let (directory, mut sequence, mut media) = fixture(bytes);
            sequence.audio.push(sound());
            let audio = media
                .get_mut(&MediaId("source".into()))
                .unwrap()
                .audio
                .as_mut()
                .unwrap();
            audio.sample_rate = sample_rate;
            audio.intrinsic_ticks = duration;
            let sequence = if in_nest { nested(sequence) } else { sequence };
            let mut omissions = Vec::new();
            // Invalid media must fail before archive staging, including
            // selected audio streams.
            let error = match convert_premiere_sequence(
                &directory
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("project.prproj"),
                sequence,
                Arc::new(media),
                &mut omissions,
            ) {
                Err(error) => error.to_string(),
                Ok(_) => panic!("selected unsupported audio passed admission"),
            };
            assert!(
                error.contains("failed admission") && error.contains(reason),
                "{error}"
            );
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn valid_picture_and_sound_keep_one_original_asset() {
    let (directory, mut sequence, media) = fixture(EMBEDDED);
    sequence.audio.push(sound());
    let mut omissions = Vec::new();
    let pending = convert(directory.path(), sequence, media, &mut omissions).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let path = directory.path().join("converted.tsrct");
    pending.write_to_staging(&path).unwrap();
    let file = TesseractFile::open(path).unwrap();
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video" || layer["type"] == "Audio")
            .count(),
        2
    );
    let picture = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let sound = layers
        .iter()
        .find(|layer| layer["type"] == "Audio")
        .unwrap();
    assert_eq!(
        picture["source"]["assetId"], "premiere-video-1",
        "{layers:?}"
    );
    assert_eq!(picture["volume"], 0.0);
    assert_eq!(sound["source"]["assetId"], "premiere-video-1");
    assert_eq!(sound["volume"], 1.0);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn compressed_channel_movie_keeps_picture_and_packages_full_aac_sound() {
    for channel in 0..2 {
        let (directory, mut sequence, media) = fixture(EMBEDDED);
        sequence.audio.push(PrAudioOccurrence {
            source_channel: Some(crate::schema::PrAudioSourceChannel::Mono(channel)),
            ..sound()
        });
        let mut omissions = Vec::new();
        let pending = convert(directory.path(), sequence, media, &mut omissions).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let path = directory.path().join("converted.tsrct");
        pending.write_to_staging(&path).unwrap();
        let file = TesseractFile::open(path).unwrap();
        assert_eq!(file.metadata().assets.len(), 2);
        let document = file.project_json().unwrap();
        let layers = document["composition"]["layers"].as_array().unwrap();
        assert_eq!(
            layers
                .iter()
                .filter(|layer| matches!(layer["type"].as_str(), Some("Video" | "Audio")))
                .count(),
            2
        );
        let picture = layers
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        let audio = layers
            .iter()
            .find(|layer| layer["type"] == "Audio")
            .unwrap();
        assert_eq!(picture["volume"], 0.0);
        assert_eq!(
            picture["sourceRange"],
            serde_json::json!({"start":33,"duration":167})
        );
        assert_eq!(audio["volume"], 1.0);
        assert_eq!(
            audio["sourceRange"],
            serde_json::json!({"start":0,"duration":200})
        );
        assert_eq!(
            crate::test_support::layer_range(audio),
            &serde_json::json!({"start":0,"duration":200})
        );
        let picture_id = picture["source"]["assetId"].as_str().unwrap();
        let audio_id = audio["source"]["assetId"].as_str().unwrap();
        assert_ne!(picture_id, audio_id);
        assert_eq!(
            file.metadata().assets[picture_id].kind,
            tesseract_file::AssetKind::Video
        );
        assert_eq!(
            file.metadata().assets[audio_id].kind,
            tesseract_file::AssetKind::Audio
        );
        let mut original = Vec::new();
        std::io::Read::read_to_end(
            &mut file.asset(picture_id).unwrap().open().unwrap(),
            &mut original,
        )
        .unwrap();
        assert_eq!(original, EMBEDDED);
        let mut mono = Vec::new();
        std::io::Read::read_to_end(
            &mut file.asset(audio_id).unwrap().open().unwrap(),
            &mut mono,
        )
        .unwrap();
        assert_eq!(mono.len(), 44 + 9600 * 4);
        assert_eq!(u16::from_le_bytes(mono[20..22].try_into().unwrap()), 3);
        let size = mono.len() as u64;
        let stream =
            crate::audio_media::inspect_audio_media(std::io::Cursor::new(mono), size, "wav")
                .unwrap()
                .unwrap();
        assert_eq!(
            (stream.channels, stream.sample_rate, stream.intrinsic_ticks),
            (AudioChannels::Mono, 48_000, AUDIO_TICKS)
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_invalid_channel_does_not_fall_back_to_the_stereo_source() {
    let (directory, mut sequence, media) = fixture(EMBEDDED);
    sequence.audio.push(PrAudioOccurrence {
        source_channel: Some(crate::schema::PrAudioSourceChannel::Mono(2)),
        ..sound()
    });
    let mut omissions = Vec::new();
    let pending = convert(directory.path(), sequence, media, &mut omissions).unwrap();
    assert!(
        omissions.iter().any(|item| item.record == "sound"
            && item
                .reason
                .contains("source-channel extraction requires stereo channel 0 or 1")),
        "{omissions:?}"
    );
    let path = directory.path().join("converted.tsrct");
    pending.write_to_staging(&path).unwrap();
    let file = TesseractFile::open(path).unwrap();
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
    assert!(!layers.iter().any(|layer| layer["type"] == "Audio"));
}

#[test]
fn compressed_channel_assets_still_verify_the_original_stereo_source() {
    let directory = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "feature_compressed_channel.prproj",
        "channel-selection-stereo.mp3",
    ] {
        fs::copy(fixtures.join(name), directory.path().join(name)).unwrap();
    }
    let source = directory.path().join("feature_compressed_channel.prproj");
    let (project, mut omissions) =
        crate::PrProjectFile::load_selected(&source, Some("d40c25d5-0359-473e-974e-24f423007763"))
            .unwrap();
    let (mut sequences, media) = project.into_parts();
    let pending = convert(directory.path(), sequences.remove(0), media, &mut omissions).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.contains("source-channel")),
        "{omissions:?}"
    );
    let mp3 = directory.path().join("channel-selection-stereo.mp3");
    let mut changed = fs::read(&mp3).unwrap();
    *changed.last_mut().unwrap() ^= 1;
    fs::write(mp3, changed).unwrap();
    let error = pending
        .write_to_staging(&directory.path().join("changed.tsrct"))
        .unwrap_err();
    assert!(
        error.to_string().contains("native media identity changed"),
        "{error}"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn merged_channel_assets_still_verify_the_original_stereo_source() {
    let directory = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "feature_merged_offset.prproj",
        "feature_two_tracks_gap_clip_a.mp4",
        "nest_tone_stereo_8s.wav",
    ] {
        fs::copy(fixtures.join(name), directory.path().join(name)).unwrap();
    }
    let source = directory.path().join("feature_merged_offset.prproj");
    let (project, mut omissions) =
        crate::PrProjectFile::load_selected(&source, Some("b532b029-e1d0-4a6c-ae91-f429e5e1b771"))
            .unwrap();
    let (mut sequences, media) = project.into_parts();
    let pending = convert(directory.path(), sequences.remove(0), media, &mut omissions).unwrap();
    let wav = directory.path().join("nest_tone_stereo_8s.wav");
    let mut changed = fs::read(&wav).unwrap();
    *changed.last_mut().unwrap() ^= 1;
    fs::write(wav, changed).unwrap();
    let error = pending
        .write_to_staging(&directory.path().join("changed.tsrct"))
        .unwrap_err();
    assert!(
        error.to_string().contains("native media identity changed"),
        "{error}"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_clock_compressed_mono_reverse_uses_full_presentation_end() {
    for channel in 0..2 {
        let (directory, mut sequence, media) = fixture(EMBEDDED);
        let mut clip = sound();
        clip.source_channel = Some(crate::schema::PrAudioSourceChannel::Mono(channel));
        clip.playback_rate = -2.0;
        clip.end_ticks = clip.start_ticks + AUDIO_TICKS / 2;
        sequence.audio.push(clip);
        let mut notes = Vec::new();
        let pending = convert(directory.path(), sequence, media, &mut notes).unwrap();
        let path = directory.path().join("reverse.tsrct");
        pending.write_to_staging(&path).unwrap();
        let file = TesseractFile::open(path).unwrap();
        let document = file.project_json().unwrap();
        let layer = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Audio")
            .unwrap();
        assert_eq!(layer["sourceIntrinsicDuration"], 200);
        assert_eq!(layer["playback"]["inputRange"]["duration"], 100);
        let keys = &layer["playback"]["mapping"]["property"]["keyframes"];
        assert_eq!(
            (keys[0]["value"].as_i64(), keys[1]["value"].as_i64()),
            (Some(200), Some(0))
        );
        let bytes = file
            .asset(layer["source"]["assetId"].as_str().unwrap())
            .unwrap()
            .read_verified_bytes(1 << 20)
            .unwrap();
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1);
        assert_eq!(bytes.len(), 44 + 9600 * 4);
        assert!(
            !notes
                .iter()
                .any(|note| note.scope == crate::OmissionScope::Occurrence),
            "{notes:?}"
        );
    }
}
