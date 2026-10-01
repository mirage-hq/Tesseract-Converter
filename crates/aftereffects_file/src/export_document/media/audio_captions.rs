//! Export-only audio caption-metadata regressions from a pinned Adobe-native source.

use fx_schema::{AudioLayer, Dimensions, Layer, LayerData};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;
use crate::{
    structure::read_project,
    structure_document::to_structural_fx_document_with_assets,
    writer::footage::{NativeFrameRate, NativeSourceFormat, RelativeMediaPath},
};

const SOURCE_SHA256: &str = "799aa17ad253c73edc6c3e9ac8e2f9b1c7cd8bfa9794cdfcd917d4e42c28a72c";

fn find_audio(layers: &[Layer]) -> Option<&AudioLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Audio(audio) => Some(audio),
        LayerData::Group(group) => find_audio(&group.layers),
        _ => None,
    })
}

fn imported_wave_static() -> AudioLayer {
    let bytes = include_bytes!("../../../tests/fixtures/audio_e2e/audio_cases.aep");
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), SOURCE_SHA256);
    let project = read_project(bytes).unwrap();
    let imported = to_structural_fx_document_with_assets(&project, Some(4), &mut |_| true).unwrap();
    let audio = find_audio(imported.document.composition().layers()).unwrap();
    assert_eq!(audio.name, "Source content clock");
    assert_eq!(audio.source_intrinsic_duration.as_millis(), 4_000);
    audio.clone()
}

fn resolved_wave(audio: &AudioLayer) -> ResolvedMediaSource {
    ResolvedMediaSource {
        asset_id: audio.source.asset_id.clone(),
        path: RelativeMediaPath::new("media/sound.wav").unwrap(),
        format: NativeSourceFormat::Wave,
        dimensions: [0, 0],
        duration_millis: 4_000,
        frame_rate: NativeFrameRate::integer(0),
        audio_sample_rate: 48_000.0,
        wave_metadata: None,
    }
}

fn with_field(audio: &AudioLayer, field: &str, value: Value) -> AudioLayer {
    let mut wire = serde_json::to_value(audio).unwrap();
    wire[field] = value;
    serde_json::from_value(wire).unwrap()
}

fn with_source_enhancement(audio: &AudioLayer) -> AudioLayer {
    let mut wire = serde_json::to_value(audio).unwrap();
    wire["source"]["enhancement"] = json!({
        "enabled": false,
        "enhancedAssetId": "enhanced-audio"
    });
    serde_json::from_value(wire).unwrap()
}

#[test]
fn disabled_captions_match_absent_export_footage_spec() {
    let absent = imported_wave_static();
    assert_eq!(absent.captions_enabled, None);
    let source = resolved_wave(&absent);
    let absent_spec = lower_audio(&absent, &source, Dimensions::new(320, 180)).unwrap();

    let mut disabled = absent.clone();
    disabled.captions_enabled = Some(false);
    let disabled_spec = lower_audio(&disabled, &source, Dimensions::new(320, 180)).unwrap();

    assert_eq!(disabled_spec, absent_spec);
    assert_eq!(disabled_spec.source.path.as_str(), "media/sound.wav");
    assert_eq!(disabled_spec.source.duration_millis, 4_000);
    assert_eq!(
        disabled_spec.audio_levels_db,
        [gain_to_db(absent.volume.as_f64()).unwrap(); 2]
    );
    assert!(disabled_spec.audio_enabled);
    assert_eq!(disabled_spec.frame_blending, NativeFrameBlending::Disabled);
    assert_eq!(disabled_spec.audio_levels_animation, None);
    assert_eq!(disabled_spec.static_source_time_secs, None);
    assert!(!disabled_spec.time_remap_requires_source_owned_transform);
}

#[test]
fn active_captions_and_neighboring_unsupported_audio_metadata_still_reject() {
    let audio = imported_wave_static();
    let source = resolved_wave(&audio);
    let cases = [
        (
            "captionsEnabled=true",
            with_field(&audio, "captionsEnabled", json!(true)),
        ),
        (
            "autoDucking",
            with_field(
                &audio,
                "autoDucking",
                json!({"duckedGain": 0.5, "mergeGapMs": 100}),
            ),
        ),
        (
            "metadata",
            with_field(
                &audio,
                "metadata",
                json!({"modality": "music", "source": "uploaded"}),
            ),
        ),
        (
            "captionPresentation",
            with_field(&audio, "captionPresentation", json!({})),
        ),
        ("source.enhancement", with_source_enhancement(&audio)),
        (
            "preserveAudioPitch",
            with_field(&audio, "preserveAudioPitch", json!(true)),
        ),
    ];

    for (name, case) in cases {
        assert!(
            lower_audio(&case, &source, Dimensions::new(320, 180)).is_err(),
            "{name} unexpectedly lowered"
        );
    }
}
