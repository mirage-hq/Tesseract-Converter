//! Source-clock assertions, not native playback or audible fidelity proof.
use super::*;
use crate::schema::{AudioChannels, TICKS};
use serde_json::json;

fn sound(authored_millis: u64) -> AudioLayer {
    serde_json::from_value(json!({
        "id":1, "name":"Sound", "activeRange":{"start":0,"duration":100},
        "sourceRange":{"start":20,"duration":100},
        "playback":{"type":"windowed", "inputRange":{"start":0,"duration":100},
            "mapping":{"type":"linear", "input":{"start":0,"duration":100},
                "output":{"start":20,"duration":100}}, "inputOffsetMs":0},
        "sourceIntrinsicDuration":authored_millis,
        "source":{"assetId":"original"}, "volume":0.75
    }))
    .unwrap()
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
    // A picture's floor is not a newly admitted audio-file representation.
    assert!(layer(&sound(8_866), None, None, &padded, &mut Vec::new()).is_err());
}

#[test]
fn audio_duration_rejects_integer_boundary_near_misses_and_invalid_sample_clocks() {
    for (measured, authored) in [
        (stream(425_065), 8_854),
        (stream(425_065), 8_857),
        (stream(425_088), 8_855), // Exact 8856 ms does not admit 8855 ms.
        (stream(425_063), 8_856), // 8855.479... rounds down, not up.
        (
            PrAudioStream {
                sample_rate: 0,
                ..stream(425_065)
            },
            8_855,
        ),
        (
            PrAudioStream {
                sample_rate: 48_001,
                ..stream(425_065)
            },
            8_855,
        ),
        (
            PrAudioStream {
                intrinsic_ticks: stream(425_065).intrinsic_ticks + 1,
                ..stream(425_065)
            },
            8_855,
        ),
        (stream(0), 0),
        (stream(-1), 0),
    ] {
        let facts = BTreeMap::from([("original".into(), SourceSound::Supported(measured))]);
        assert!(layer(&sound(authored), None, None, &facts, &mut Vec::new()).is_err());
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
    let error = layer(&sound, None, None, &facts, &mut Vec::new()).unwrap_err();
    assert!(error
        .to_string()
        .contains("active audio enhancement output"));
    sound.source_intrinsic_duration = fx_schema::Duration::from_millis(8_856);
    let (written, measured) = layer(&sound, None, None, &facts, &mut Vec::new())
        .unwrap()
        .unwrap();
    assert_eq!(written.media.0, "replacement");
    assert_eq!(measured.intrinsic_ticks, stream(425_113).intrinsic_ticks);
}
