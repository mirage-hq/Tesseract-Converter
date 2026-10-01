//! Import admission follows the sound placements, not an unused embedded stream.
//! These model tests supplement the private, unchanged Bonsa native-source repro.

use super::support::{nest_of, video_media, video_sequence};
use crate::{
    format::{FrameRate, MediaId, PrMedia, PrSequence},
    schema::{AudioChannels, PrAudioOccurrence, PrAudioStream, PrVideoTrack, TICKS},
    tesseract_output::{convert_premiere_sequence, PendingTesseractFile},
    Omission,
};
use fx_schema::LinearGain;
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
use tesseract_file::TesseractFile;

const EMBEDDED: &[u8] = include_bytes!("../../tests/fixtures/video-with-audio.mp4");
const VIDEO_ONLY: &[u8] = include_bytes!("../../tests/fixtures/video-30fps.mp4");
const AUDIO_TICKS: i64 = TICKS / 5;

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

fn sound() -> PrAudioOccurrence {
    PrAudioOccurrence {
        id: Some("sound".into()),
        media: MediaId("source".into()),
        start_ticks: 0,
        end_ticks: AUDIO_TICKS,
        in_ticks: 0,
        out_ticks: AUDIO_TICKS,
        volume: LinearGain::UNITY,
        volume_keys: None,
    }
}

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
