use crate::{
    format::inspect_project_with_omissions,
    schema::{PrAudioOccurrence, PrKeyframeEasing, TICKS},
    tests::support::prproj_xml,
};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn native_record<'a>(xml: &'a str, tag: &str, id: &str) -> &'a str {
    let start = xml.find(&format!(r#"<{tag} ObjectID="{id}""#)).unwrap();
    let end = start + xml[start..].find(&format!("</{tag}>")).unwrap() + tag.len() + 3;
    &xml[start..end]
}

fn insert_before_volume(xml: &str, volume_id: &str, filter: &str) -> String {
    let reference = format!(r#"<Component Index="0" ObjectRef="{volume_id}"/>"#);
    assert_eq!(xml.matches(&reference).count(), 1);
    let start = xml.find(&reference).unwrap();
    let end = start + xml[start..].find("</Components>").unwrap();
    let components = xml[start..end]
        .replace("Index=\"1\"", "Index=\"2\"")
        .replace("Index=\"0\"", "Index=\"1\"");
    let edited = xml.replacen(
        &xml[start..end],
        &format!("<Component Index=\"0\" ObjectRef=\"990001\"/>\n{components}"),
        1,
    );
    let donor = native_record(xml, "AudioFilterComponent", volume_id);
    edited.replacen(donor, &format!("{filter}\n{donor}"), 1)
}

fn unknown_intrinsic_insert(xml: &str, volume_id: &str, level_id: &str) -> String {
    // Clone just one native filter, leaving its first control but removing Level.
    // The resulting unknown intrinsic layout is supplementary mutation evidence,
    // not a claim that Premiere authored this unsupported processor.
    let donor = native_record(xml, "AudioFilterComponent", volume_id);
    let filter = donor
        .replacen(
            &format!(r#"ObjectID="{volume_id}""#),
            "ObjectID=\"990001\"",
            1,
        )
        .replace("Internal Volume Stereo", "Unknown intrinsic insert")
        .replace(&format!(r#"<Param Index="1" ObjectRef="{level_id}"/>"#), "");
    assert!(!filter.contains(&format!(r#"ObjectRef="{level_id}""#)));
    insert_before_volume(xml, volume_id, &filter)
}

fn assert_retained_audio(actual: &[PrAudioOccurrence], expected: &[PrAudioOccurrence]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.id, expected.id);
        assert_eq!(actual.media, expected.media);
        assert_eq!(actual.source_channel, expected.source_channel);
        assert_eq!(actual.start_ticks, expected.start_ticks);
        assert_eq!(actual.end_ticks, expected.end_ticks);
        assert_eq!(actual.in_ticks, expected.in_ticks);
        assert_eq!(actual.out_ticks, expected.out_ticks);
        assert_eq!(actual.playback_rate, expected.playback_rate);
        assert_eq!(actual.preserve_audio_pitch, expected.preserve_audio_pitch);
        assert_eq!(actual.volume, expected.volume);
        match (&actual.volume_keys, &expected.volume_keys) {
            (Some(actual), Some(expected)) => {
                assert_eq!(actual.gain, expected.gain);
                assert_eq!(actual.keys, expected.keys);
            }
            (None, None) => {}
            _ => panic!("retained Volume keys differ: {actual:?} / {expected:?}"),
        }
    }
}

#[test]
fn intrinsic_volume_unknown_insert_keeps_native_keys_mute_and_source_owners() {
    let xml = prproj_xml(&fixture("feature_audio_volume_keys_strict.prproj"));
    for muted in [false, true] {
        let baseline = if muted {
            let mute = native_record(&xml, "AudioComponentParam", "141");
            xml.replacen(
                mute,
                &mute.replace(
                    "<Name>Mute</Name>",
                    "<Name>Mute</Name><StartKeyframe>-91445760000000000,1,0,0,0,0,0,0</StartKeyframe>",
                ),
                1,
            )
        } else {
            xml.clone()
        };
        let (expected, expected_notes) = inspect_project_with_omissions(&baseline, None).unwrap();
        let expected_audio = &expected.sequences[0].audio;
        assert_eq!(expected_audio.len(), 4);
        assert_eq!(
            expected_audio[0].volume_keys.as_ref().unwrap().keys.len(),
            2
        );
        assert_eq!(
            expected_audio[0].volume_keys.as_ref().unwrap().gain,
            if muted { 0.0 } else { 1.0 }
        );
        let edited = unknown_intrinsic_insert(&baseline, "118", "142");
        let (actual, notes) = inspect_project_with_omissions(&edited, None).unwrap();
        assert_retained_audio(&actual.sequences[0].audio, expected_audio);
        assert_eq!(
            actual.media.keys().collect::<Vec<_>>(),
            expected.media.keys().collect::<Vec<_>>()
        );
        let insert_notes: Vec<_> = notes
            .iter()
            .filter(|note| note.record == "AudioFilterComponent:990001")
            .collect();
        assert_eq!(insert_notes.len(), 1, "{notes:?}");
        assert_eq!(insert_notes[0].scope, crate::OmissionScope::Feature);
        assert_eq!(
            insert_notes[0].reason,
            "audio filter \"Unknown intrinsic insert\" not converted: unsupported conversion: no editable processing equivalent"
        );
        assert_eq!(notes.len(), expected_notes.len() + 1, "{notes:?}");
        assert!(
            notes
                .iter()
                .all(|note| !note.reason.contains("duplicate clip Volume")),
            "{notes:?}"
        );
    }
}

#[test]
fn intrinsic_volume_unknown_insert_keeps_legacy_static_silence_and_siblings() {
    let xml = prproj_xml(&fixture("feature_audio_volume_legacy_strict.prproj"));
    let (expected, expected_notes) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_eq!(expected.sequences[0].audio.len(), 11);
    assert_eq!(expected.sequences[0].audio[2].volume.as_f64(), 0.0);
    let edited = unknown_intrinsic_insert(&xml, "1491", "1494");
    let (actual, notes) = inspect_project_with_omissions(&edited, None).unwrap();
    assert_retained_audio(&actual.sequences[0].audio, &expected.sequences[0].audio);
    assert_eq!(notes.len(), expected_notes.len() + 1, "{notes:?}");
    assert!(
        notes.iter().any(|note| note.record == "AudioFilterComponent:990001"
            && note.reason == "audio filter \"Unknown intrinsic insert\" not converted: unsupported conversion: no editable processing equivalent"),
        "{notes:?}"
    );
}

#[test]
fn intrinsic_volume_duplicate_recognized_filters_still_diagnose_the_chain() {
    let xml = prproj_xml(&fixture("feature_audio_volume_keys_strict.prproj"));
    for unsupported_keys in [false, true] {
        let baseline = if unsupported_keys {
            // The layout is still Volume even when its Level keys cannot convert.
            xml.replacen(
                "444528000000,0.177827939391,0,",
                "444528000000,0.177827939391,5,",
                1,
            )
        } else {
            xml.clone()
        };
        let duplicate = native_record(&baseline, "AudioFilterComponent", "118").replacen(
            "ObjectID=\"118\"",
            "ObjectID=\"990001\"",
            1,
        );
        let edited = insert_before_volume(&baseline, "118", &duplicate);
        let (project, notes) = inspect_project_with_omissions(&edited, None).unwrap();
        assert_eq!(project.sequences[0].audio.len(), 4);
        assert!(project.sequences[0].audio[0].volume_keys.is_none());
        assert!(
            notes
                .iter()
                .any(|note| note.reason.contains("duplicate clip Volume")),
            "{notes:?}"
        );
        if unsupported_keys {
            assert!(
                notes
                    .iter()
                    .any(|note| note.record == "AudioFilterComponent:990001"
                        && note
                            .reason
                            .contains("Bezier Volume keyframes are not converted")),
                "{notes:?}"
            );
        }
    }
}

#[test]
fn intrinsic_volume_display_metadata_preserves_native_level_keys_and_siblings() {
    // Public Premiere 26.5.1 save; only the intrinsic Volume display name differs.
    let xml = prproj_xml(&fixture("feature_audio_volume_keys_strict.prproj"));
    for name in ["Internal Volume", "Saved Volume display metadata"] {
        let edited = xml.replace("Internal Volume Stereo", name);
        assert_ne!(edited, xml);
        let (project, omissions) = inspect_project_with_omissions(&edited, None).unwrap();
        let sequence = &project.sequences[0];
        assert_eq!(sequence.audio.len(), 4);
        let first = &sequence.audio[0];
        let keys = &first.volume_keys.as_ref().unwrap().keys;
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].source_ticks, 7 * TICKS / 4);
        assert_eq!(keys[0].value, 1.0);
        assert_eq!(keys[1].source_ticks, 11 * TICKS / 4);
        assert!((20.0 * keys[1].value.log10() + 12.0).abs() < 1e-3);
        assert_eq!(keys[1].easing, PrKeyframeEasing::Linear);
        assert_eq!(sequence.audio[3].in_ticks, TICKS);
        for sound in &sequence.audio {
            assert!(sound
                .volume_keys
                .as_ref()
                .is_some_and(|keys| !keys.keys.is_empty()));
            let media = &project.media[&sound.media];
            assert_eq!(media.audio.as_ref().unwrap().sample_rate, 48_000);
            assert!(sound.end_ticks > sound.start_ticks);
            assert!(sound.out_ticks > sound.in_ticks);
        }
        assert!(
            omissions.iter().all(|note| !note.reason.contains("Volume")),
            "{omissions:?}"
        );
    }
}

#[test]
fn intrinsic_volume_display_metadata_keeps_legacy_scale_and_static_silence() {
    let xml = prproj_xml(&fixture("feature_audio_volume_legacy_strict.prproj"));
    let edited = xml.replace("Internal Volume Stereo", "Saved Volume display metadata");
    assert_ne!(edited, xml);
    let (project, _) = inspect_project_with_omissions(&edited, None).unwrap();
    let audio = &project.sequences[0].audio;
    assert_eq!(audio.len(), 11);
    assert_eq!(audio[2].volume.as_f64(), 0.0);
    assert!((20.0 * audio[3].volume.as_f64().log10() + 30.0).abs() < 1e-3);
    assert!((20.0 * audio[4].volume.as_f64().log10() + 20.0).abs() < 1e-3);
    assert!(audio[10].volume_keys.is_some());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn intrinsic_volume_display_metadata_publishes_editable_audio_and_verified_assets() {
    use serde_json::json;
    use tesseract_file::TesseractFile;

    let temp = tempfile::tempdir().unwrap();
    for name in [
        "feature_linked_av_source.mp4",
        "feature_audio_tone_right.wav",
        "feature_audio_click_left.wav",
    ] {
        std::fs::copy(fixture(name), temp.path().join(name)).unwrap();
    }
    let xml = prproj_xml(&fixture("feature_audio_volume_keys_strict.prproj"))
        .replace("Internal Volume Stereo", "Internal Volume");
    let source = temp.path().join("volume.prproj");
    std::fs::write(&source, xml).unwrap();
    let (native, _) = crate::format::PrProjectFile::load(&source).unwrap();
    assert_eq!(native.sequences[0].audio.len(), 4);
    let output = temp.path().join("converted");
    let omissions = crate::premiere_to_tesseract(
        &source,
        &output,
        Some("99b9c6d0-8795-4dfd-9a1c-8a45dd221b3d"),
        false,
    )
    .unwrap();
    assert!(
        omissions.iter().all(|note| !note.reason.contains("Volume")),
        "{omissions:?}"
    );
    let file = TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document = file.project_json().unwrap();
    let audio: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .collect();
    assert_eq!(audio.len(), 4);
    assert_eq!(
        audio[0]["sourceRange"],
        json!({"start": 0, "duration": 5000})
    );
    for layer in audio {
        let id = layer["source"]["assetId"].as_str().unwrap();
        let asset = file.asset(id).unwrap();
        let name = Path::new(&asset.descriptor().path).file_name().unwrap();
        let expected = std::fs::read(temp.path().join(name)).unwrap();
        assert_eq!(
            asset.read_verified_bytes(expected.len() as u64).unwrap(),
            expected
        );
        let volume = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["target"]
                    == json!({"kind": "layer", "layerId": layer["id"], "propertyType": "volume"})
            })
            .unwrap();
        assert!(volume["animator"]["keyframes"].as_array().unwrap().len() >= 2);
    }
}
