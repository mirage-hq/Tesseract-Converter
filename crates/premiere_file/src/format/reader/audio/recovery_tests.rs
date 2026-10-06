use crate::{
    format::inspect_project_with_omissions,
    schema::{PrKeyframeEasing, TICKS},
    tests::support::prproj_xml,
};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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
