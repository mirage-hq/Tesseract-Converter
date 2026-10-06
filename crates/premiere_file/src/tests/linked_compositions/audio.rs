//! The AEP was saved by AE 26.5x89; Premiere occurrences below are typed,
//! supplementary source records, not a native linked-audio Premiere fixture.

use super::*;
use crate::schema::{PrKeyframeEasing, PrScalarKeyframe, PrVolumeKeys};

fn package(root: &Path) {
    let wav = fs::read(fixture(
        "../aftereffects_file/tests/fixtures/audio_e2e/sound.wav",
    ))
    .unwrap();
    relocated_audio_composition(
        root,
        &fixture("tests/fixtures/hybrid/linked-audio.aep"),
        &wav,
    );
}

fn sound(id: &str, start: i64, source: i64, gain: f64) -> PrAudioOccurrence {
    PrAudioOccurrence {
        id: Some(id.into()),
        media: MediaId("linked".into()),
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        start_ticks: start * TICKS,
        end_ticks: (start + 2) * TICKS,
        in_ticks: source * TICKS,
        out_ticks: (source + 2) * TICKS,
        volume: LinearGain::new(gain).unwrap(),
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    }
}

fn media() -> BTreeMap<MediaId, PrMedia> {
    let mut source = linked("controls.aep", UNITY, [320, 180]);
    source.video.as_mut().unwrap().intrinsic_ticks = 4 * TICKS;
    source.audio = Some(PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: 4 * TICKS,
        sample_rate: 48_000,
        channels: AudioChannels::Stereo,
    });
    BTreeMap::from([(MediaId("linked".into()), source)])
}

fn roots(document: &Value) -> Vec<&Value> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["name"] == "Premiere linked AE audio")
        .collect()
}

fn audible_audio(root: &Value) -> Vec<&Value> {
    all_layers(root)
        .into_iter()
        .filter(|layer| layer["type"] == "Audio" && layer["isHidden"] != true)
        .collect()
}

#[test]
fn linked_audio_placements_keep_independent_clocks_gain_and_shared_media() {
    let root = tempfile::tempdir().unwrap();
    package(root.path());
    let mut sequence = sequence_of(
        "Independent linked sound",
        vec![PrVideoTrack::media([clip_of("linked", 0..4 * TICKS, 0)])],
    );
    sequence.audio = vec![
        sound("first sound", 1, 1, 0.5),
        sound("second sound", 4, 0, 0.25),
    ];
    let (mut document, omissions) = convert(root.path(), sequence, media());
    assert!(
        !omissions
            .iter()
            .any(|o| o.scope == OmissionScope::Occurrence && o.record.ends_with("sound")),
        "{omissions:?}"
    );
    assert_unique_identities(&document);
    let groups = roots(&document);
    assert_eq!(groups.len(), 2);
    for (group, start, source, expected_gain) in
        [(groups[0], 1000, 1000, 0.25), (groups[1], 4000, 0, 0.125)]
    {
        assert_eq!(
            group["playback"]["inputRange"],
            serde_json::json!({"start": start, "duration": 2000})
        );
        assert_eq!(
            group["playback"]["mapping"]["output"],
            serde_json::json!({"start": source, "duration": 2000})
        );
        let audio = audible_audio(group);
        assert_eq!(audio.len(), 1);
        assert!((audio[0]["volume"].as_f64().unwrap() - expected_gain).abs() < 1e-6);
        assert_eq!(audio[0]["source"]["assetId"], "premiere-aep-1-item-1");
        assert!(all_layers(group)
            .iter()
            .filter(|layer| !matches!(layer["type"].as_str(), Some("Group" | "Audio")))
            .all(|layer| layer["isHidden"] == true));
    }
    let picture = linked_groups(&document);
    assert_eq!(picture.len(), 1);
    assert!(audible_audio(picture[0]).is_empty());
    let archive = TesseractFile::open(root.path().join("converted.tsrct")).unwrap();
    assert_eq!(
        archive
            .asset("premiere-aep-1-item-1")
            .unwrap()
            .read_verified_bytes(1 << 20)
            .unwrap(),
        fs::read(fixture(
            "../aftereffects_file/tests/fixtures/audio_e2e/sound.wav"
        ))
        .unwrap()
    );
    let first = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["name"] == "Premiere linked AE audio")
        .unwrap();
    first["playback"]["inputRange"]["start"] = 2000.into();
    first["playback"]["inputOffsetMs"] = (-1000).into();
    fn edit_gain(layer: &mut Value) {
        if layer["type"] == "Audio" {
            layer["volume"] = 0.75.into();
        }
        if let Some(children) = layer["layers"].as_array_mut() {
            for child in children {
                edit_gain(child);
            }
        }
    }
    edit_gain(first);
    let native = export(root.path(), &document);
    let sequence = native.single_sequence().unwrap();
    assert_eq!(sequence.audio.len(), 2);
    let mut sounds: Vec<_> = sequence.audio.iter().collect();
    sounds.sort_by_key(|sound| sound.start_ticks);
    for (sound, start, end, source, gain) in [
        (sounds[0], 2 * TICKS, 4 * TICKS, TICKS / 2, 0.75),
        (sounds[1], 9 * TICKS / 2, 6 * TICKS, 0, 0.125),
    ] {
        assert_eq!(
            (sound.start_ticks, sound.end_ticks, sound.in_ticks),
            (start, end, source)
        );
        assert!((sound.volume.as_f64() - gain).abs() < 1e-6);
    }
}

fn export(root: &Path, document: &Value) -> crate::format::PrProjectFile {
    use tesseract_file::{AssetKind, TesseractFileBuilder};
    let edited = root.join("edited.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-aep-1-item-1",
            root.join("source.wav"),
            AssetKind::Audio,
        )
        .unwrap()
        .write(&edited)
        .unwrap();
    let output = root.join("native");
    let omissions = crate::tesseract_to_premiere(&edited, &output, false).unwrap();
    assert!(
        omissions.iter().any(|o| o
            .reason
            .contains("individually editable native audio clips")),
        "{omissions:?}"
    );
    let path = fs::read_dir(output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "prproj")
        })
        .unwrap();
    crate::format::PrProjectFile::load_import(&path, None)
        .unwrap()
        .0
}

#[test]
fn linked_audio_hold_gain_uses_the_audio_leafs_input_clock() {
    let root = tempfile::tempdir().unwrap();
    package(root.path());
    let mut clip = sound("keyed sound", 1, 1, 0.25);
    clip.volume_keys = Some(PrVolumeKeys {
        gain: 1.0,
        keys: vec![
            PrScalarKeyframe {
                source_ticks: TICKS,
                value: 0.25,
                easing: PrKeyframeEasing::Hold,
            },
            PrScalarKeyframe {
                source_ticks: 2 * TICKS,
                value: 1.0,
                easing: PrKeyframeEasing::Hold,
            },
        ],
    });
    let mut sequence = sequence_of("Audio-only linked placement", Vec::new());
    sequence.audio = vec![clip];
    let (document, omissions) = convert(root.path(), sequence, media());
    assert!(
        !omissions
            .iter()
            .any(|o| o.scope == OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let group = roots(&document)[0];
    let audio = audible_audio(group)[0];
    let id = audio["id"].clone();
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let entry = entries
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == id && entry["target"]["propertyType"] == "volume"
        })
        .unwrap();
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    // Native AE sound starts at composition 0.5 s. Premiere's selected source
    // starts at 1 s, so the first outer gain key is at leaf-input time 0.5 s.
    assert_eq!(keys[0]["layerTime"], 500);
    assert_eq!(keys[1]["layerTime"], 1500);
    assert!((keys[0]["value"]["value"].as_f64().unwrap() - 0.125).abs() < 1e-6);
    assert!((keys[1]["value"]["value"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    assert_eq!(keys[1]["easing"]["type"], "hold");
    let native = export(root.path(), &document);
    let sound = &native.single_sequence().unwrap().audio[0];
    let keys = sound.volume_keys.as_ref().unwrap();
    assert_eq!(
        keys.keys
            .iter()
            .map(|key| key.source_ticks)
            .collect::<Vec<_>>(),
        [TICKS / 2, 3 * TICKS / 2]
    );
    assert!((keys.keys[0].value * keys.gain - 0.125).abs() < 1e-6);
    assert!((keys.keys[1].value * keys.gain - 0.5).abs() < 1e-6);
}

#[test]
fn audio_clock_linked_unit_pitch_on_nonzero_in_keeps_level_and_fade_clock() {
    let root = tempfile::tempdir().unwrap();
    package(root.path());
    let mut clip = sound("pitch sound", 1, 1, 0.25);
    clip.preserve_audio_pitch = true;
    clip.volume_keys = Some(PrVolumeKeys {
        gain: 1.0,
        keys: vec![
            PrScalarKeyframe {
                source_ticks: 3 * TICKS / 2,
                value: 0.25,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: 2 * TICKS,
                value: 1.0,
                easing: PrKeyframeEasing::Hold,
            },
        ],
    });
    clip.fade_in = Some(crate::schema::PrAudioFade {
        id: None,
        curve: crate::schema::PrFadeCurve::ConstantGain,
        duration_ticks: TICKS / 2,
    });
    let mut sequence = sequence_of("Pitch source clock", Vec::new());
    sequence.audio = vec![clip];
    let (document, notes) = convert(root.path(), sequence, media());
    assert!(
        !notes
            .iter()
            .any(|note| note.scope == OmissionScope::Occurrence),
        "{notes:?}"
    );
    let group = roots(&document)[0];
    assert_eq!(group["playback"]["mapping"]["type"], "timeRemap");
    let leaf = audible_audio(group)[0];
    assert_eq!(leaf["preserveAudioPitch"], true);
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == leaf["id"] && entry["target"]["propertyType"] == "volume"
        })
        .unwrap();
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    // Leaf input clock starts at source composition 0.5s: absolute source
    // 1.0/1.5/2.0s become 0.5/1.0/1.5s, never another source In later.
    assert_eq!(
        keys.iter()
            .map(|key| key["layerTime"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [500, 1000, 1500]
    );
    assert_eq!(keys[0]["value"]["value"], 0.0);
    // AE's native float gain carries its existing float32 rounding.
    assert!((keys[1]["value"]["value"].as_f64().unwrap() - 0.125).abs() < 1e-6);
}

#[test]
fn audio_clock_linked_reverse_physical_start_clipping_is_reported_once() {
    let root = tempfile::tempdir().unwrap();
    package(root.path());
    let mut clip = sound("clipped reverse sound", 0, 2, 0.25);
    clip.playback_rate = -1.0;
    clip.in_ticks += crate::schema::TICKS_PER_MILLISECOND;
    clip.out_ticks += crate::schema::TICKS_PER_MILLISECOND;
    let mut sequence = sequence_of("Reverse source allowance", Vec::new());
    sequence.audio = vec![clip];
    let (document, notes) = convert(root.path(), sequence, media());
    assert!(
        !notes
            .iter()
            .any(|note| note.scope == OmissionScope::Occurrence),
        "{notes:?}"
    );
    let clipped: Vec<_> = notes
        .iter()
        .filter(|note| {
            note.reason
                .contains("clips its physical source start to zero")
        })
        .collect();
    assert_eq!(clipped.len(), 1, "{notes:?}");
    assert_eq!(clipped[0].record, "clipped reverse sound");
    assert_eq!(clipped[0].kind, crate::OmissionKind::Approximated);
    assert_eq!(clipped[0].reason, "reverse sound's native media-end rounding allowance clips its physical source start to zero");
    let group = roots(&document)[0];
    let clock = &group["playback"]["mapping"]["property"]["keyframes"];
    assert_eq!(clock[0]["value"], 1999);
    assert_eq!(clock[1]["value"], 0);
}
