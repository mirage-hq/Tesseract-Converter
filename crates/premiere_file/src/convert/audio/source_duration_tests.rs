//! Source-clock assertions, not native playback or audible fidelity proof.
use super::*;
use crate::schema::{AudioChannels, TICKS};
use serde_json::json;

fn sound_with_mapping(
    authored_millis: u64,
    active_millis: u64,
    source_start: u64,
    source_millis: u64,
    mapping: serde_json::Value,
) -> AudioLayer {
    serde_json::from_value(json!({
        "id":1, "name":"Sound", "activeRange":{"start":0,"duration":active_millis},
        "sourceRange":{"start":source_start,"duration":source_millis},
        "playback":{"type":"windowed", "inputRange":{"start":0,"duration":active_millis},
            "mapping":mapping, "inputOffsetMs":0},
        "sourceIntrinsicDuration":authored_millis,
        "source":{"assetId":"original"}, "volume":0.75
    }))
    .unwrap()
}

fn sound(authored_millis: u64) -> AudioLayer {
    sound_with_mapping(
        authored_millis,
        100,
        20,
        100,
        json!({"type":"linear", "input":{"start":0,"duration":100},
            "output":{"start":20,"duration":100}}),
    )
}

fn stream(samples: i64) -> PrAudioStream {
    PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: samples * (TICKS / 48_000),
        channels: AudioChannels::Mono,
        sample_rate: 48_000,
    }
}

#[test]
fn audio_duration_floor_preserves_exact_samples_and_playback_boundaries() {
    // Both measured endpoints have a 25/48 ms fractional tail, not an epsilon.
    for (samples, authored) in [(425_065, 8_855), (10_177_177, 212_024)] {
        let sound = sound(authored);
        let original = stream(samples);
        let facts = BTreeMap::from([("original".into(), SourceSound::Supported(original.clone()))]);
        let mut diagnostics = Vec::new();
        let (written, measured) = layer(&sound, None, None, &facts, &mut diagnostics)
            .unwrap()
            .unwrap();
        assert_eq!(measured, &original);
        assert_eq!(measured.intrinsic_ticks / (TICKS / 48_000), samples);
        assert_eq!(
            (written.start_ticks, written.end_ticks),
            (0, 100 * TICKS_PER_MILLISECOND)
        );
        assert_eq!(
            (written.in_ticks, written.out_ticks),
            (20 * TICKS_PER_MILLISECOND, 120 * TICKS_PER_MILLISECOND)
        );
        assert_eq!(written.playback_rate, 1.0);
        assert_eq!(written.volume.as_f64(), 0.75);
        assert_eq!(sound.source_intrinsic_duration.as_millis(), authored);
        assert!(diagnostics
            .iter()
            .any(|d| d.kind == crate::OmissionKind::Approximated
                && d.reason.contains("floor")
                && d.reason.contains("exact")));
    }
}

#[test]
fn audio_duration_nearest_and_padded_picture_representations_are_unchanged() {
    let file = stream(425_065);
    let supported = BTreeMap::from([("original".into(), SourceSound::Supported(file.clone()))]);
    let mut diagnostics = Vec::new();
    layer(&sound(8_856), None, None, &supported, &mut diagnostics)
        .unwrap()
        .unwrap();
    assert!(diagnostics.is_empty());
    let picture = PrAudioStream {
        intrinsic_ticks: 266 * (TICKS / 30),
        ..file.clone()
    };
    let padded = BTreeMap::from([(
        "original".into(),
        SourceSound::PaddedToPicture {
            stream: picture.clone(),
            file_ticks: file.intrinsic_ticks,
        },
    )]);
    for authored in [8_867, 8_856] {
        let (written, measured) = layer(&sound(authored), None, None, &padded, &mut Vec::new())
            .unwrap()
            .unwrap();
        assert_eq!(measured, &picture);
        assert_eq!(written.out_ticks, 120 * TICKS_PER_MILLISECOND);
    }
    // An unrelated picture-clock declaration does not veto the fully
    // available source window or replace the padded native descriptor.
    let mut diagnostics = Vec::new();
    let (written, measured) = layer(&sound(8_866), None, None, &padded, &mut diagnostics)
        .unwrap()
        .unwrap();
    assert_eq!(measured, &picture);
    assert_eq!(written.out_ticks, 120 * TICKS_PER_MILLISECOND);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .reason
            .contains("sourceIntrinsicDuration 8866 ms differs")
            && diagnostic
                .reason
                .contains("retained fully available mapped source")
    }));
}

#[test]
fn audio_duration_declaration_does_not_reject_available_playback() {
    let forward = sound(750);
    let fast = sound_with_mapping(
        750,
        100,
        20,
        200,
        json!({"type":"linear", "input":{"start":0,"duration":100},
            "output":{"start":20,"duration":200}}),
    );
    let reverse = sound_with_mapping(
        750,
        100,
        20,
        100,
        json!({"type":"timeRemap", "property":{
            "keyframes":[
                {"id":"start", "time":0, "value":120, "easing":{"type":"linear"}},
                {"id":"end", "time":100, "value":20, "easing":{"type":"linear"}}
            ], "before":"inactive", "after":"inactive"}}),
    );
    let facts = BTreeMap::from([("original".into(), SourceSound::Supported(stream(48_000)))]);
    for (sound, expected_rate) in [(forward, 1.0), (fast, 2.0), (reverse, -1.0)] {
        let mut diagnostics = Vec::new();
        let (written, _) = layer(&sound, None, None, &facts, &mut diagnostics)
            .unwrap()
            .unwrap();
        assert_eq!(written.playback_rate, expected_rate);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == crate::OmissionKind::Approximated
                && diagnostic
                    .reason
                    .contains("sourceIntrinsicDuration 750 ms differs")
                && diagnostic
                    .reason
                    .contains("retained fully available mapped source")
                && diagnostic.reason.contains("exact sample clock unchanged")
        }));
    }
}

#[test]
fn unavailable_audio_is_omitted_locally_unless_unit_forward() {
    let forward = sound_with_mapping(
        400,
        200,
        0,
        200,
        json!({"type":"linear", "input":{"start":0,"duration":200},
            "output":{"start":0,"duration":200}}),
    );
    let fast = sound_with_mapping(
        400,
        100,
        0,
        200,
        json!({"type":"linear", "input":{"start":0,"duration":100},
            "output":{"start":0,"duration":200}}),
    );
    let reverse = sound_with_mapping(
        400,
        100,
        0,
        200,
        json!({"type":"timeRemap", "property":{
            "keyframes":[
                {"id":"start", "time":0, "value":200, "easing":{"type":"linear"}},
                {"id":"end", "time":100, "value":0, "easing":{"type":"linear"}}
            ], "before":"inactive", "after":"inactive"}}),
    );
    let unavailable = sound_with_mapping(
        400,
        100,
        200,
        100,
        json!({"type":"linear", "input":{"start":0,"duration":100},
            "output":{"start":200,"duration":100}}),
    );
    let facts = BTreeMap::from([("original".into(), SourceSound::Supported(stream(4_928)))]);
    let mut diagnostics = Vec::new();
    let (written, _) = layer(&forward, None, None, &facts, &mut diagnostics)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            written.start_ticks / TICKS_PER_MILLISECOND,
            written.end_ticks / TICKS_PER_MILLISECOND,
            written.in_ticks / TICKS_PER_MILLISECOND,
            written.out_ticks / TICKS_PER_MILLISECOND,
        ),
        (0, 103, 0, 103)
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .reason
            .contains("retained available source [0,103) ms")
            && diagnostic
                .reason
                .contains("unavailable 97 ms authored tail")
    }));

    for sound in [fast, reverse, unavailable] {
        let mut diagnostics = Vec::new();
        assert!(layer(&sound, None, None, &facts, &mut diagnostics)
            .unwrap()
            .is_none());
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.scope == OmissionScope::Occurrence
                && diagnostic
                    .reason
                    .contains("only a partially available unit-forward tail")
        }));
    }
}

#[test]
fn audio_duration_rejects_invalid_sample_clocks() {
    for measured in [
        PrAudioStream {
            sample_rate: 0,
            ..stream(425_065)
        },
        PrAudioStream {
            sample_rate: 48_001,
            ..stream(425_065)
        },
        PrAudioStream {
            intrinsic_ticks: stream(425_065).intrinsic_ticks + 1,
            ..stream(425_065)
        },
        stream(0),
        stream(-1),
    ] {
        let facts = BTreeMap::from([("original".into(), SourceSound::Supported(measured))]);
        let error = layer(&sound(8_855), None, None, &facts, &mut Vec::new()).unwrap_err();
        assert!(error
            .to_string()
            .contains("invalid positive whole-sample clock"));
    }
}

#[test]
fn audio_duration_floor_checks_the_active_replacement_not_the_original() {
    let mut sound = sound(8_855);
    sound.source.enhancement = Some(
        serde_json::from_value(json!({
            "enabled":false, "enhancedAssetId":"replacement"
        }))
        .unwrap(),
    );
    let facts = BTreeMap::from([
        ("original".into(), SourceSound::Supported(stream(425_065))),
        (
            "replacement".into(),
            SourceSound::Supported(stream(425_113)),
        ),
    ]);
    layer(&sound, None, None, &facts, &mut Vec::new())
        .unwrap()
        .unwrap();
    sound.source.enhancement.as_mut().unwrap().enabled = true;
    let mut diagnostics = Vec::new();
    let (written, measured) = layer(&sound, None, None, &facts, &mut diagnostics)
        .unwrap()
        .unwrap();
    assert_eq!(written.media.0, "replacement");
    assert_eq!(measured.intrinsic_ticks, stream(425_113).intrinsic_ticks);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .reason
            .contains("active audio enhancement output")
            && diagnostic
                .reason
                .contains("retained fully available mapped source")
    }));
    sound.source_intrinsic_duration = fx_schema::Duration::from_millis(8_856);
    let (written, measured) = layer(&sound, None, None, &facts, &mut Vec::new())
        .unwrap()
        .unwrap();
    assert_eq!(written.media.0, "replacement");
    assert_eq!(measured.intrinsic_ticks, stream(425_113).intrinsic_ticks);
}
