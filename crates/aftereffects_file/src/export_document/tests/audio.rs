//! Native-source import assertions and supplementary fresh-export audio tests.
//! Own-reader export assertions are not Adobe acceptance or audio fidelity proof.

use super::*;
use crate::{
    properties::{read_numeric, root_runs, runs, unique_list},
    structure_document::to_structural_fx_document_with_assets,
    writer::footage::{NativeFrameRate, NativeSourceFormat, NativeWaveMetadata, RelativeMediaPath},
};
use fx_schema::animator::{KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack};

fn audio_layer(id: u64) -> Value {
    json!({
        "type":"Audio", "id":id, "name":format!("audio-{id}"), "parent":null,
        "playback":fixture_linear_playback(json!({"start":1000,"duration":2000}), json!({"start":500,"duration":2000})),
        "sourceRange":{"start":500,"duration":1000},
        "sourceIntrinsicDuration":4000, "volume":0.5,
        "source":{"assetId":"sound"}
    })
}

fn source(duration_millis: u64) -> media::ResolvedMediaSource {
    media::ResolvedMediaSource {
        asset_id: fx_schema::AssetId::new("sound").unwrap(),
        path: RelativeMediaPath::new("media/sound.wav").unwrap(),
        format: NativeSourceFormat::Wave,
        dimensions: [0, 0],
        duration_millis,
        duration_millis_floor: duration_millis,
        duration_native_ticks: None,
        frame_rate: NativeFrameRate::integer(0),
        audio_sample_rate: 48_000.0,
        wave_metadata: None,
        native_duration: None,
    }
}

fn document(
    children: Vec<Value>,
    entries: Vec<AnimationGraphEntry>,
) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["composition"]["layers"] = json!(children);
    value["composition"]["dynamics"] = json!({"entries":entries});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn gain_keys(id: u64, easing: PropertyKeyframeEasing) -> AnimationGraphEntry {
    gain_keys_at(id, easing, [(0, 0.5), (1000, 1.0)])
}

fn gain_keys_at(
    id: u64,
    easing: PropertyKeyframeEasing,
    keys: [(i64, f64); 2],
) -> AnimationGraphEntry {
    let keys = keys
        .into_iter()
        .enumerate()
        .map(|(index, (time, gain))| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("audio-{id}-{index}")),
                fx_schema::TimeOffset::from_millis(time),
                PropertyValue::Float(gain),
                easing,
            )
        })
        .collect();
    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(LayerId::new(id), PropType::AudioVolume),
        animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn export_audio(document: &EditableFxCompositionDocument) -> ExportedDocument {
    to_aep_with_media(
        document,
        &BTreeMap::from([("sound".to_owned(), source(4000))]),
    )
    .unwrap()
}

fn audio_levels(layer: &crate::structure::Layer) -> crate::properties::NumericProperty {
    let groups = root_runs(&layer.content).unwrap();
    let (_, group) = groups
        .iter()
        .find(|(name, _)| *name == "ADBE Audio Group")
        .unwrap();
    let properties = runs(unique_list(group, *b"tdgp").unwrap()).unwrap();
    let (_, levels) = properties
        .iter()
        .find(|(name, _)| *name == "ADBE Audio Levels")
        .unwrap();
    read_numeric(unique_list(levels, *b"tdbs").unwrap()).unwrap()
}

fn find_audio(layers: &[Layer]) -> &fx_schema::AudioLayer {
    find_audio_optional(layers).expect("editable Audio layer")
}

fn find_audio_optional(layers: &[Layer]) -> Option<&fx_schema::AudioLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Audio(audio) => Some(audio),
        LayerData::Group(group) => find_audio_optional(&group.layers),
        _ => None,
    })
}

#[test]
fn audio_native_import_retains_source_duration_and_independent_switches() {
    // Unchanged pinned Adobe-native bytes; media is admitted structurally only.
    // The original wav.wav is not bundled, so no actual audio decoding is claimed.
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/media/audioEnabled.aep"
    ))
    .unwrap();
    for (composition, enabled) in [(1, true), (15, false)] {
        let imported =
            to_structural_fx_document_with_assets(&native, Some(composition), &mut |_| true)
                .unwrap();
        let audio = find_audio(imported.document.composition().layers());
        assert_eq!(audio.source.asset_id.as_str(), "aep-local-item-13");
        assert_eq!(audio.source_intrinsic_duration.as_millis(), 5943);
        assert_eq!(audio.source_range.start.as_millis(), 0);
        assert_eq!(audio.source_range.duration.as_millis(), 5943);
        assert_eq!(audio.volume.as_f64(), if enabled { 1.0 } else { 0.0 });
        assert_eq!(imported.assets.len(), 1);
    }
}

#[test]
fn verified_wave_metadata_survives_fx_document_lowering() {
    let mut resolved = source(4_000);
    let pinned = include_bytes!("../../../tests/fixtures/audio_e2e/sound.wav");
    assert_eq!(pinned.len(), 768_044);
    resolved.wave_metadata = Some(NativeWaveMetadata {
        sample_frames: 192_000,
        file_length: u32::try_from(pinned.len()).unwrap(),
    });
    let exported = to_aep_with_media(
        &document(vec![audio_layer(700)], Vec::new()),
        &BTreeMap::from([("sound".to_owned(), resolved)]),
    )
    .unwrap();
    fn wave_settings(chunks: &[crate::rifx::Chunk]) -> Option<&[u8]> {
        for chunk in chunks {
            if chunk.id() == *b"sspc"
                && let Some(data) = chunk.data_payload()
                && data.get(22..26) == Some(b"WAVE".as_slice())
            {
                return Some(data);
            }
            if let Some(children) = chunk.children()
                && let Some(data) = wave_settings(children)
            {
                return Some(data);
            }
        }
        None
    }
    let rifx = crate::rifx::Rifx::parse_with(&exported.bytes, |kind| kind == *b"tdgp").unwrap();
    let settings = wave_settings(rifx.chunks()).expect("fresh WAVE file source");
    assert_eq!(&settings[38..42], &192_000_u32.to_be_bytes());
    assert_eq!(&settings[208..212], &768_044_u32.to_be_bytes());
}

#[test]
fn audio_native_import_fresh_export_keeps_audio_only_hierarchy() {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/media/audioEnabled.aep"
    ))
    .unwrap();
    let imported = to_structural_fx_document_with_assets(&native, Some(1), &mut |_| true).unwrap();
    let mut resolved = source(5943);
    resolved.asset_id = fx_schema::AssetId::new("aep-local-item-13").unwrap();
    let exported = to_aep_with_media(
        &imported.document,
        &BTreeMap::from([("aep-local-item-13".to_owned(), resolved)]),
    )
    .unwrap();
    let fresh = read_project(&exported.bytes).unwrap();
    assert!(fresh.items.iter().any(|item| matches!(&item.kind, ItemKind::Composition(comp) if comp.layers.iter().any(|layer| fresh.item(layer.record.source_id()).is_some_and(|source| source.media.is_some())))), "{:?}", exported.diagnostics);
}

#[test]
fn audio_export_explicit_remap_keeps_trim_and_stretch_but_rejects_gain_keys() {
    let mut audio = audio_layer(700);
    audio["playback"] = fixture_remapped_playback(
        json!({"start":1000,"duration":2000}),
        json!({"keyframes":[
        {"id":"start","time":1000,"value":500,"easing":{"type":"linear"}},
        {"id":"end","time":3000,"value":1500,"easing":{"type":"linear"}}
    ],"before":"inactive","after":"inactive"}),
    );
    let exported = export_audio(&document(vec![audio.clone()], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let layer = &layers(&fresh)[0];
    assert!(
        layer.record.flags().enabled,
        "Native WAVE retains its general layer switch; it has no video stream"
    );
    assert!(layer.record.flags().audio_enabled);
    assert_eq!(layer.record.stretch(), Some(2.0));
    assert_eq!(layer.record.in_point(), Some(0.5));
    assert_eq!(layer.record.out_point(), Some(1.5));
    let levels = audio_levels(layer);
    assert!(levels.keyframes.is_empty());
    assert!((levels.values[0] - 20.0 * 0.5_f64.log10()).abs() < 1e-9);
    // Source key 1000 ms is this mapping's fixed point, so an occurrence-local
    // rebase cannot move it. Source-clock gain keys are still unsupported.
    let keyed = export_audio(&document(
        vec![audio, audio_layer(701)],
        vec![gain_keys(700, PropertyKeyframeEasing::Hold)],
    ));
    let fresh = read_project(&keyed.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", keyed.diagnostics);
    assert_eq!(layers(&fresh)[0].name.as_ref(), "audio-701");
    assert!(
        keyed.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(700))
                && diagnostic
                    .message
                    .contains("Time Remap cannot drive occurrence-owned Audio Levels")
        }),
        "{:?}",
        keyed.diagnostics
    );
}

#[test]
fn audio_export_rejects_explicit_remap_source_clock_gain_keys_and_retains_its_sibling() {
    // Explicit TimeRemap gain samples the mapped source clock, which reaches
    // this 750 ms Hold switch at parent 1500 ms. An occurrence-local rebase
    // through the affine 2x native record would move the switch to 1750 ms.
    let mut remapped = audio_layer(700);
    remapped["playback"] = fixture_remapped_playback(
        json!({"start":1000,"duration":2000}),
        json!({"keyframes":[
        {"id":"start","time":1000,"value":500,"easing":{"type":"linear"}},
        {"id":"end","time":3000,"value":1500,"easing":{"type":"linear"}}
    ],"before":"inactive","after":"inactive"}),
    );
    let gain = |id| gain_keys_at(id, PropertyKeyframeEasing::Hold, [(0, 0.5), (750, 1.0)]);
    let exported = export_audio(&document(
        vec![remapped, audio_layer(701)],
        vec![gain(700), gain(701)],
    ));
    let fresh = read_project(&exported.bytes).unwrap();
    let parent_key_secs = |layer: &crate::structure::Layer| {
        let start = layer.record.start_time().unwrap();
        let stretch = layer.record.stretch().unwrap();
        audio_levels(layer)
            .keyframes
            .iter()
            .map(|key| start + key.time_secs * stretch)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        layers(&fresh)
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<&str>>(),
        ["audio-701"],
        "parent-clock gain keys: {:?}; diagnostics: {:?}",
        layers(&fresh)
            .iter()
            .map(|layer| (layer.name.as_ref(), parent_key_secs(layer)))
            .collect::<Vec<_>>(),
        exported.diagnostics
    );
    assert!(
        exported.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(700))
                && diagnostic
                    .message
                    .contains("Time Remap cannot drive occurrence-owned Audio Levels")
        }),
        "{:?}",
        exported.diagnostics
    );
    // Ordinary Linear Audio keeps the same keys on its occurrence clock.
    assert_eq!(parent_key_secs(&layers(&fresh)[0]), vec![1.0, 1.75]);
}

#[test]
fn audio_export_rejects_shifted_input_clock_gain_keys_and_retains_its_sibling() {
    let mut shifted = audio_layer(700);
    shifted["playback"] = json!({
        "type":"windowed", "inputRange":{"start":1200,"duration":2000},
        "mapping":{"type":"linear", "input":{"start":1000,"duration":8000},
            "output":{"start":500,"duration":8000}}, "inputOffsetMs":-100
    });
    let exported = export_audio(&document(
        vec![shifted, audio_layer(701)],
        vec![gain_keys(700, PropertyKeyframeEasing::Hold)],
    ));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(
        layers(&fresh).len(),
        1,
        "levels: {:?}; diagnostics: {:?}",
        layers(&fresh)
            .iter()
            .map(|layer| audio_levels(layer)
                .keyframes
                .iter()
                .map(|key| key.time_secs)
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        exported.diagnostics
    );
    assert!(
        exported.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(700))
                && diagnostic.message.contains(
                    "shifted Audio input clock cannot preserve native Audio Levels key timing",
                )
        }),
        "{:?}",
        exported.diagnostics
    );
    assert_eq!(layers(&fresh)[0].record.in_point(), Some(0.5));
    assert_eq!(layers(&fresh)[0].record.out_point(), Some(2.5));
}

#[test]
fn audio_export_shifted_input_clock_keeps_static_gain_and_signed_offset_cancellation_keys() {
    for with_keys in [false, true] {
        let mut audio = audio_layer(700);
        audio["playback"] = json!({
            "type":"windowed", "inputRange":{"start":1200,"duration":2000},
            "mapping":{"type":"linear", "input":{"start":1000,"duration":8000},
                "output":{"start":500,"duration":8000}},
            "inputOffsetMs": if with_keys {-200} else {-100}
        });
        let entries = if with_keys {
            vec![gain_keys(700, PropertyKeyframeEasing::Hold)]
        } else {
            Vec::new()
        };
        let exported = export_audio(&document(vec![audio], entries));
        let fresh = read_project(&exported.bytes).unwrap();
        assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
        assert!(
            !exported
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(700))),
            "{:?}",
            exported.diagnostics
        );
        if with_keys {
            assert_eq!(
                audio_levels(&layers(&fresh)[0])
                    .keyframes
                    .iter()
                    .map(|key| key.time_secs)
                    .collect::<Vec<_>>(),
                vec![0.5, 1.5]
            );
        }
    }
}

#[test]
fn coverage_contract_audio_export_static_gain_and_offset_clock() {
    let exported = export_audio(&document(vec![audio_layer(700)], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let layer = &layers(&fresh)[0];
    assert!(layer.record.flags().enabled);
    assert!(layer.record.flags().audio_enabled);
    assert_eq!(layer.record.stretch(), Some(1.0));
    assert_eq!(layer.record.start_time(), Some(0.5));
    // Native in/out points are layer-relative, not composition timestamps.
    assert_eq!(layer.record.in_point(), Some(0.5));
    assert_eq!(layer.record.out_point(), Some(2.5));
    let start = layer.record.start_time().unwrap();
    let stretch = layer.record.stretch().unwrap();
    assert_eq!(start + layer.record.in_point().unwrap() * stretch, 1.0);
    assert_eq!(start + layer.record.out_point().unwrap() * stretch, 3.0);
    let levels = audio_levels(layer);
    assert!(levels.keyframes.is_empty());
    let expected_db = 20.0 * 0.5_f64.log10();
    assert_eq!(levels.values.len(), 2);
    assert!((levels.values[0] - expected_db).abs() < 1e-9);
    assert!((levels.values[1] - expected_db).abs() < 1e-9);
}

#[test]
fn audio_export_hidden_layer_keeps_editable_source_with_audio_disabled() {
    let mut audio = audio_layer(700);
    audio["isHidden"] = json!(true);
    let exported = export_audio(&document(vec![audio], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let flags = layers(&fresh)[0].record.flags();
    assert!(!flags.enabled);
    assert!(!flags.audio_enabled);
    assert!((audio_levels(&layers(&fresh)[0]).values[0] - 20.0 * 0.5_f64.log10()).abs() < 1e-9);
}

#[test]
fn video_export_exact_zero_mutes_without_disabling_picture_and_gain_edits_restore_audio() {
    let mut video = json!({
        "type":"Video", "id":700, "name":"movie", "parent":null,
        "playback":fixture_linear_playback(json!({"start":1000,"duration":2000}), json!({"start":500,"duration":2000})),
        "sourceRange":{"start":500,"duration":2000},
        "sourceIntrinsicDuration":8000,
        "transform":serde_json::to_value(identity_fx_transform()).unwrap(),
        "source":{"assetId":"sound","fit":"contain"}, "volume":0.0
    });
    let mut movie = source(8000);
    movie.path = RelativeMediaPath::new("media/movie.mov").unwrap();
    movie.format = NativeSourceFormat::QuickTime;
    movie.dimensions = [320, 180];
    movie.frame_rate = NativeFrameRate::integer(24);
    let mut sources = BTreeMap::from([("sound".to_owned(), movie)]);
    let zero_override = AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(LayerId::new(700), PropType::AudioVolume),
        animator: PropertyAnimator::constant(PropertyValue::Float(0.0)).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    for (gain, entries, expected_enabled, expected_keys) in [
        (0.5, vec![zero_override], false, 1),
        (0.0, Vec::new(), false, 0),
        (0.5, Vec::new(), true, 0),
        (
            0.0,
            vec![gain_keys(700, PropertyKeyframeEasing::Hold)],
            true,
            2,
        ),
        (
            0.5,
            vec![gain_keys_at(
                700,
                PropertyKeyframeEasing::Hold,
                [(0, 0.0), (1000, 0.0)],
            )],
            false,
            2,
        ),
    ] {
        video["volume"] = json!(gain);
        let exported =
            to_aep_with_media(&document(vec![video.clone()], entries), &sources).unwrap();
        let fresh = read_project(&exported.bytes).unwrap();
        assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
        let layer = &layers(&fresh)[0];
        assert!(
            layer.record.flags().enabled,
            "picture remains editable and enabled"
        );
        assert_eq!(
            layer.record.flags().audio_enabled,
            expected_enabled,
            "gain {gain}"
        );
        if !expected_enabled {
            let levels = audio_levels(layer);
            if levels.keyframes.is_empty() {
                assert_eq!(levels.values, vec![-192.0, -192.0]);
            } else {
                assert_eq!(levels.keyframes.len(), expected_keys);
                assert!(
                    levels
                        .keyframes
                        .iter()
                        .all(|key| key.values == vec![-192.0, -192.0])
                );
            }
        }
    }
    // Native visibility and missing source audio are independent of positive gain.
    for (hidden, sample_rate, gain) in [(true, 48_000.0, 0.5), (false, 0.0, 0.5), (false, 0.0, 0.0)]
    {
        video["volume"] = json!(gain);
        video["isHidden"] = json!(hidden);
        sources.get_mut("sound").unwrap().audio_sample_rate = sample_rate;
        let exported =
            to_aep_with_media(&document(vec![video.clone()], Vec::new()), &sources).unwrap();
        let fresh = read_project(&exported.bytes).unwrap();
        if hidden {
            // Hidden Video remains unsupported; gain cannot create source audio.
            assert!(layers(&fresh).is_empty());
            assert!(
                exported
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(700)))
            );
        } else {
            assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
            assert!(!layers(&fresh)[0].record.flags().audio_enabled);
            assert!(layers(&fresh)[0].record.flags().enabled);
        }
    }
}

#[test]
fn audio_export_static_zero_is_exact_mute_but_gain_keys_can_unmute() {
    let mut audio = audio_layer(700);
    audio["volume"] = json!(0.0);
    let muted = export_audio(&document(vec![audio.clone()], Vec::new()));
    let fresh = read_project(&muted.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1);
    assert!(!layers(&fresh)[0].record.flags().audio_enabled);
    let keyed = export_audio(&document(
        vec![audio],
        vec![gain_keys(700, PropertyKeyframeEasing::Hold)],
    ));
    let fresh = read_project(&keyed.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1);
    assert!(layers(&fresh)[0].record.flags().audio_enabled);
}

#[test]
fn audio_export_constant_zero_track_is_exact_mute_with_zero_or_nonzero_base() {
    for base in [0.0, 0.5] {
        let mut audio = audio_layer(700);
        audio["volume"] = json!(base);
        let entry = AnimationGraphEntry {
            target: fx_schema::PropertyTarget::layer(LayerId::new(700), PropType::AudioVolume),
            animator: PropertyAnimator::constant(PropertyValue::Float(0.0)).unwrap(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let exported = export_audio(&document(vec![audio], vec![entry]));
        let fresh = read_project(&exported.bytes).unwrap();
        let layer = &layers(&fresh)[0];
        assert!(!layer.record.flags().audio_enabled, "base gain {base}");
        assert!(
            audio_levels(layer)
                .keyframes
                .iter()
                .all(|key| key.values == vec![-192.0, -192.0])
        );
    }
}

#[test]
fn audio_export_all_zero_keyframes_are_exact_mute() {
    for count in [1, 2] {
        let mut audio = audio_layer(700);
        audio["volume"] = json!(0.5);
        let keys = (0..count)
            .map(|index| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("silent-{index}")),
                    fx_schema::TimeOffset::from_millis(index * 1000),
                    PropertyValue::Float(0.0),
                    PropertyKeyframeEasing::Linear,
                )
            })
            .collect();
        let entry = AnimationGraphEntry {
            target: fx_schema::PropertyTarget::layer(LayerId::new(700), PropType::AudioVolume),
            animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let exported = export_audio(&document(vec![audio], vec![entry]));
        let fresh = read_project(&exported.bytes).unwrap();
        let layer = &layers(&fresh)[0];
        assert!(!layer.record.flags().audio_enabled, "key count {count}");
        assert_eq!(audio_levels(layer).keyframes.len(), count as usize);
        assert!(
            audio_levels(layer)
                .keyframes
                .iter()
                .all(|key| key.values == vec![-192.0, -192.0])
        );
    }
}

#[test]
fn audio_export_past_eof_preserves_audible_prefix_without_stretching() {
    let mut audio = audio_layer(700);
    audio["playback"] = fixture_linear_playback(
        json!({"start":1000,"duration":5000}),
        json!({"start":500,"duration":5000}),
    );
    let exported = export_audio(&document(vec![audio], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let record = &layers(&fresh)[0].record;
    assert_eq!(record.stretch(), Some(1.0));
    assert_eq!(record.start_time(), Some(0.5));
    assert_eq!(record.in_point(), Some(0.5));
    assert_eq!(record.out_point(), Some(4.0));
    assert!(
        exported
            .diagnostics
            .iter()
            .any(|warning| warning.layer_id == Some(LayerId::new(700))
                && warning.message.contains("retains the audible prefix"))
    );
}

#[test]
fn audio_export_unit_mapping_offset_clips_eof_and_diagnoses_its_actual_silent_tail() {
    let mut audio = audio_layer(700);
    audio["playback"] = json!({
        "type": "windowed",
        "inputRange": {"start":1000,"duration":2000},
        "mapping": {"type":"linear",
            "input": {"start":0,"duration":5000},
            "output": {"start":2000,"duration":5000}},
        "inputOffsetMs": 500
    });
    let exported = export_audio(&document(vec![audio], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let record = &layers(&fresh)[0].record;
    assert_eq!(record.start_time_fraction(), (-5, 2));
    assert_eq!(record.in_point_fraction(), (7, 2));
    assert_eq!(record.out_point_fraction(), (4, 1));
    assert_eq!(record.stretch_fraction(), (1, 1));
    assert!(exported.diagnostics.iter().any(|warning| {
        warning.layer_id == Some(LayerId::new(700))
            && warning.message.contains("retains the audible prefix")
    }));
}

#[test]
fn audio_export_slow_linear_mapping_does_not_diagnose_an_unclipped_silent_tail() {
    let mut audio = audio_layer(700);
    audio["playback"] = fixture_linear_playback(
        json!({"start":1000,"duration":6000}),
        json!({"start":500,"duration":3000}),
    );
    audio["sourceRange"] = json!({"start":500,"duration":3000});
    let exported = export_audio(&document(vec![audio], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let record = &layers(&fresh)[0].record;
    assert_eq!(record.start_time_fraction(), (0, 1));
    assert_eq!(record.in_point_fraction(), (1, 2));
    assert_eq!(record.out_point_fraction(), (7, 2));
    assert_eq!(record.stretch_fraction(), (2, 1));
    assert!(!exported.diagnostics.iter().any(|warning| {
        warning.layer_id == Some(LayerId::new(700)) && warning.message.contains("source EOF")
    }));
}

#[test]
fn audio_export_options_preserve_source_specific_native_switches() {
    for (movie, hidden, muted, expected) in [
        (false, false, false, 0x07),
        (false, false, true, 0x05),
        (false, true, false, 0x04),
        (true, false, false, 0x06),
    ] {
        let mut media = source(4000);
        if movie {
            media.format = NativeSourceFormat::QuickTime;
            media.path = RelativeMediaPath::new("media/sound.mov").unwrap();
            media.dimensions = [1920, 1080];
            media.frame_rate = NativeFrameRate::integer(24);
        }
        let mut layer = audio_layer(700);
        layer["isHidden"] = json!(hidden);
        if muted {
            layer["volume"] = json!(0.0);
        }
        let exported = to_aep_with_media(
            &document(vec![layer], Vec::new()),
            &BTreeMap::from([("sound".to_owned(), media)]),
        )
        .unwrap();
        let fresh = read_project(&exported.bytes).unwrap();
        assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
        assert_eq!(
            layers(&fresh)[0].record.encode()[39],
            expected,
            "movie={movie}, hidden={hidden}, muted={muted}"
        );
    }
}

#[test]
fn audio_export_movie_audio_does_not_enable_its_video_channel() {
    let mut movie = source(4000);
    movie.format = NativeSourceFormat::QuickTime;
    movie.path = RelativeMediaPath::new("media/sound.mov").unwrap();
    movie.dimensions = [1920, 1080];
    movie.frame_rate = NativeFrameRate::integer(24);
    let exported = to_aep_with_media(
        &document(vec![audio_layer(700)], Vec::new()),
        &BTreeMap::from([("sound".to_owned(), movie)]),
    )
    .unwrap();
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    assert!(!layers(&fresh)[0].record.flags().enabled);
    assert!(layers(&fresh)[0].record.flags().audio_enabled);
}

#[test]
fn audio_export_clocked_group_keeps_audio_without_visual_bounds() {
    let mut group = imported()["composition"]["layers"][0].clone();
    group["id"] = json!(701);
    group["name"] = json!("Audio clock");
    group["parent"] = Value::Null;
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    group["playback"] = fixture_linear_playback(
        json!({"start":1000,"duration":2000}),
        json!({"start":0,"duration":2000}),
    );
    let mut audio = audio_layer(700);
    audio["parent"] = json!(701);
    group["layers"] = json!([audio]);
    let exported = export_audio(&document(vec![group], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let child = fresh.item(layers(&fresh)[0].record.source_id()).unwrap();
    let ItemKind::Composition(comp) = &child.kind else {
        panic!("native audio precomposition")
    };
    assert_eq!(comp.layers.len(), 1, "{:?}", exported.diagnostics);
    assert!(comp.layers[0].record.flags().audio_enabled);
}

#[test]
fn audio_export_clocked_group_with_empty_nested_groups_keeps_audio() {
    let mut group = imported()["composition"]["layers"][0].clone();
    group["id"] = json!(701);
    group["parent"] = Value::Null;
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    group["playback"] = fixture_linear_playback(
        json!({"start":1000,"duration":2000}),
        json!({"start":0,"duration":2000}),
    );
    let mut empty = group.clone();
    empty["id"] = json!(702);
    empty["parent"] = json!(701);
    empty["layers"] = json!([]);
    let mut audio = audio_layer(700);
    audio["parent"] = json!(701);
    group["layers"] = json!([audio, empty]);
    let document = document(vec![group], Vec::new());
    assert!(super::super::hierarchy::audio_only(
        document.composition().layers()
    ));
    let exported = export_audio(&document);
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let child = fresh.item(layers(&fresh)[0].record.source_id()).unwrap();
    let ItemKind::Composition(comp) = &child.kind else {
        panic!("native audio precomposition")
    };
    assert!(
        comp.layers
            .iter()
            .any(|layer| layer.record.flags().audio_enabled)
    );
}

#[test]
fn empty_audio_hierarchy_is_not_audio_only() {
    assert!(!super::super::hierarchy::audio_only(&[]));
}

#[test]
fn audio_export_continuous_gain_keys_keep_editable_curves_with_approximation() {
    for easing in [
        PropertyKeyframeEasing::Linear,
        PropertyKeyframeEasing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.75,
            y2: 0.9,
        },
    ] {
        let exported = export_audio(&document(
            vec![audio_layer(700)],
            vec![gain_keys(700, easing)],
        ));
        let fresh = read_project(&exported.bytes).unwrap();
        assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
        let levels = audio_levels(&layers(&fresh)[0]);
        assert_eq!(levels.keyframes.len(), 2, "no baked sample keys");
        assert_eq!(levels.keyframes[0].time_secs, 0.5);
        assert_eq!(levels.keyframes[1].time_secs, 1.5);
        assert_eq!(levels.keyframes[0].out_interpolation, 2);
        assert_eq!(levels.keyframes[1].in_interpolation, 2);
        assert!(exported.diagnostics.iter().any(|warning| {
            warning.layer_id == Some(LayerId::new(700))
                && warning
                    .message
                    .contains("continuous gain keys are approximated")
        }));
        // Same temporal Bezier parameter means the same timestamp on both
        // curves. This bounds error for these two chosen CPU examples only.
        let (y1, y2) = match easing {
            PropertyKeyframeEasing::Linear => (1.0 / 3.0, 2.0 / 3.0),
            PropertyKeyframeEasing::CubicBezier { y1, y2, .. } => (y1, y2),
            _ => unreachable!(),
        };
        let start = &levels.keyframes[0];
        let end = &levels.keyframes[1];
        let db1 = start.values[0] + start.out_speed[0] * start.out_influence[0] / 100.0;
        let db2 = end.values[0] - end.in_speed[0] * end.in_influence[0] / 100.0;
        for sample in 0..=32 {
            let u = f64::from(sample) / 32.0;
            let cubic = |a: f64, b: f64, c: f64, d: f64| {
                (1.0 - u).powi(3) * a
                    + 3.0 * (1.0 - u).powi(2) * u * b
                    + 3.0 * (1.0 - u) * u * u * c
                    + u.powi(3) * d
            };
            let expected = cubic(0.5, 0.5 + 0.5 * y1, 0.5 + 0.5 * y2, 1.0);
            let approximate = 10_f64.powf(cubic(start.values[0], db1, db2, end.values[0]) / 20.0);
            assert!(
                (expected - approximate).abs() < 0.04,
                "sample {sample}: {expected} vs {approximate}"
            );
        }
    }
}

#[test]
fn audio_export_source_switch_preserves_trim_and_gain_keys() {
    let selector = AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(LayerId::new(700), PropType::AudioSourceAssetId),
        animator: PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(
                [(0, "sound"), (1000, "replacement")]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (time, asset))| {
                        PropertyKeyframe::new(
                            KeyframeId::new(format!("switch-{i}")),
                            fx_schema::TimeOffset::from_millis(time),
                            PropertyValue::String(asset.into()),
                            PropertyKeyframeEasing::Hold,
                        )
                    })
                    .collect(),
            )
            .unwrap(),
        ),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let mut replacement = source(4000);
    replacement.asset_id = fx_schema::AssetId::new("replacement").unwrap();
    replacement.path = RelativeMediaPath::new("media/replacement.wav").unwrap();
    let mut audio = audio_layer(700);
    audio["playback"] = fixture_linear_playback(
        json!({"start":2000,"duration":2000}),
        json!({"start":500,"duration":2000}),
    );
    audio["startTime"] = json!(0.5);
    let doc = document(
        vec![audio],
        vec![selector, gain_keys(700, PropertyKeyframeEasing::Hold)],
    );
    let exported = to_aep_with_media(
        &doc,
        &BTreeMap::from([
            ("sound".into(), source(4000)),
            ("replacement".into(), replacement),
        ]),
    )
    .unwrap();
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 2, "{:?}", exported.diagnostics);
    for layer in layers(&fresh) {
        assert_eq!(layer.record.stretch(), Some(1.0));
        assert_eq!(layer.record.start_time(), Some(1.5));
        assert!(layer.record.flags().enabled);
        assert!(layer.record.flags().audio_enabled);
        let levels = audio_levels(layer);
        assert_eq!(
            levels
                .keyframes
                .iter()
                .map(|key| key.time_secs)
                .collect::<Vec<_>>(),
            vec![0.5, 1.5]
        );
    }
    let paths: std::collections::BTreeSet<_> = layers(&fresh)
        .iter()
        .map(|layer| {
            fresh
                .item(layer.record.source_id())
                .unwrap()
                .media
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .authored_path
                .as_str()
        })
        .collect();
    assert_eq!(
        paths,
        std::collections::BTreeSet::from(["./media/sound.wav", "./media/replacement.wav"])
    );
}

#[test]
fn audio_export_playback_remap_retains_keys_and_diagnoses_conflicting_gain_clock() {
    let mut audio = audio_layer(700);
    audio["sourceRange"] = json!({"start":500,"duration":1000});
    audio["playback"] = fixture_remapped_playback(
        json!({"start":1000,"duration":1000}),
        json!({"keyframes":[
        {"id":"before","time":0,"value":500,"easing":{"type":"linear"}},
        {"id":"a","time":1250,"value":750,"easing":{"type":"linear"}},
        {"id":"b","time":1750,"value":1000,"easing":{"type":"linear"}},
        {"id":"after","time":3000,"value":1250,"easing":{"type":"linear"}}
    ],"before":"inactive","after":"inactive"}),
    );
    let exported = export_audio(&document(vec![audio.clone()], Vec::new()));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1, "{:?}", exported.diagnostics);
    let roots = root_runs(&layers(&fresh)[0].content).unwrap();
    let (_, remap) = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Time Remapping")
        .unwrap();
    let remap = read_numeric(unique_list(remap, *b"tdbs").unwrap()).unwrap();
    assert_eq!(remap.keyframes.len(), 4);
    let rejected = export_audio(&document(
        vec![audio, audio_layer(701)],
        vec![gain_keys(700, PropertyKeyframeEasing::Hold)],
    ));
    let fresh = read_project(&rejected.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1);
    assert_eq!(layers(&fresh)[0].name.as_ref(), "audio-701");
    assert!(rejected.diagnostics.iter().any(|warning| {
        warning.layer_id == Some(LayerId::new(700))
            && warning
                .message
                .contains("Time Remap cannot drive occurrence-owned Audio Levels")
    }));
}

#[test]
fn audio_native_keyed_levels_import_and_fresh_export_retain_editable_gain() {
    use fx_schema::animator::AnimatorData;
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("../../../tests/fixtures/media/import_audio_media_controls.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "36920d6bdc07dbb6292cc305327ace44efbacacb182dc18da3c979439f5473a8"
    );
    let native = read_project(bytes).unwrap();
    let ItemKind::Composition(comp) = &native.item(63).unwrap().kind else {
        panic!("composition 63")
    };
    assert_eq!(native.item(63).unwrap().name.as_str(), "AUDIO_GAIN_KEYED");
    let native_audio = comp
        .layers
        .iter()
        .find(|layer| {
            native
                .item(layer.record.source_id())
                .is_some_and(|item| item.media.is_some())
        })
        .unwrap();
    let native_levels = audio_levels(native_audio);
    assert!(
        native_levels.keyframes.len() >= 2,
        "discriminating native keyed source"
    );
    let converted =
        to_structural_fx_document_with_assets(&native, Some(63), &mut |_| true).unwrap();
    let audio = find_audio(converted.document.composition().layers());
    let entry = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| {
            entry.target == fx_schema::PropertyTarget::layer(audio.id, PropType::AudioVolume)
        })
        .unwrap();
    let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
        panic!("editable AudioVolume keys")
    };
    assert_eq!(track.keyframes().len(), native_levels.keyframes.len());
    for (actual, expected) in track.keyframes().iter().zip(&native_levels.keyframes) {
        assert_eq!(
            actual.layer_time().as_millis(),
            (expected.time_secs * 1000.0).round() as i64
        );
        let db = expected
            .values
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        let expected_gain = if db <= -192.0 {
            0.0
        } else {
            10_f64.powf(db / 20.0)
        };
        assert!((float_value(actual.value()).unwrap() - expected_gain).abs() < 1e-9);
    }
    let mut resolved = source(audio.source_intrinsic_duration.as_millis());
    resolved.asset_id = audio.source.asset_id.clone();
    let exported = to_aep_with_media(
        &converted.document,
        &BTreeMap::from([(audio.source.asset_id.as_str().to_owned(), resolved)]),
    )
    .unwrap();
    let fresh = read_project(&exported.bytes).unwrap();
    let exported_levels: Vec<_> = fresh
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .filter(|layer| {
            fresh
                .item(layer.record.source_id())
                .is_some_and(|item| item.media.is_some())
        })
        .map(audio_levels)
        .collect();
    assert_eq!(exported_levels.len(), 1, "{:?}", exported.diagnostics);
    assert_eq!(
        exported_levels[0].keyframes.len(),
        native_levels.keyframes.len()
    );
    assert!(exported.diagnostics.iter().any(|warning| {
        warning
            .message
            .contains("continuous gain keys are approximated")
    }));
}

#[test]
fn audio_export_hidden_group_mutes_descendant_without_losing_gain_keys() {
    let mut group = imported()["composition"]["layers"][0].clone();
    group["id"] = json!(701);
    group["parent"] = Value::Null;
    group["isHidden"] = json!(true);
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    group["playback"] = fixture_linear_playback(
        json!({"start":0,"duration":30000}),
        json!({"start":0,"duration":30000}),
    );
    let mut audio = audio_layer(700);
    audio["parent"] = json!(701);
    group["layers"] = json!([audio]);
    let exported = export_audio(&document(
        vec![group],
        vec![gain_keys(700, PropertyKeyframeEasing::Hold)],
    ));
    let fresh = read_project(&exported.bytes).unwrap();
    let audio_layers: Vec<_> = fresh
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .filter(|layer| {
            fresh
                .item(layer.record.source_id())
                .is_some_and(|item| item.media.is_some())
        })
        .collect();
    assert_eq!(audio_layers.len(), 1, "{:?}", exported.diagnostics);
    assert!(!audio_layers[0].record.flags().audio_enabled);
    assert_eq!(audio_levels(audio_layers[0]).keyframes.len(), 2);
}

#[test]
fn audio_export_unsupported_gain_hull_retains_supported_sibling() {
    let easing = PropertyKeyframeEasing::CubicBezier {
        x1: 0.3,
        y1: -3.0,
        x2: 0.7,
        y2: 1.0,
    };
    let exported = export_audio(&document(
        vec![audio_layer(700), audio_layer(701)],
        vec![gain_keys(700, easing)],
    ));
    let fresh = read_project(&exported.bytes).unwrap();
    assert_eq!(layers(&fresh).len(), 1);
    assert_eq!(layers(&fresh)[0].name.as_ref(), "audio-701");
    assert!(
        exported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(700))
                && diagnostic.message.contains("gain control hull is negative"))
    );
}

#[test]
fn review_hidden_group_mutes_video_without_losing_gain_keys() {
    for hidden in [false, true] {
        for opacity in [100.0, 75.0] {
            let mut group = imported()["composition"]["layers"][0].clone();
            group["id"] = json!(701);
            group["parent"] = Value::Null;
            group["isHidden"] = json!(hidden);
            group["transform"] = json!(identity_fx_transform());
            group["transform"]["opacity"] = json!(opacity);
            group["playback"] = fixture_linear_playback(
                json!({"start":0,"duration":30000}),
                json!({"start":0,"duration":30000}),
            );
            group["layers"] = json!([{
                "type":"Video", "id":700, "name":"review movie", "parent":701,
                "playback":fixture_linear_playback(json!({"start":1000,"duration":2000}), json!({"start":500,"duration":2000})),
                "sourceRange":{"start":500,"duration":2000},
                "sourceIntrinsicDuration":8000,
                "transform":identity_fx_transform(),
                "source":{"assetId":"sound","fit":"contain"}, "volume":0.5
            }]);
            let mut movie = source(8000);
            movie.path = RelativeMediaPath::new("media/movie.mov").unwrap();
            movie.format = NativeSourceFormat::QuickTime;
            movie.dimensions = [320, 180];
            movie.frame_rate = NativeFrameRate::integer(24);
            let exported = to_aep_with_media(
                &document(
                    vec![group],
                    vec![gain_keys(700, PropertyKeyframeEasing::Hold)],
                ),
                &BTreeMap::from([("sound".to_owned(), movie)]),
            )
            .unwrap();
            let fresh = read_project(&exported.bytes).unwrap();
            let movies: Vec<_> = fresh
                .items
                .iter()
                .flat_map(|item| match &item.kind {
                    ItemKind::Composition(comp) => comp.layers.as_slice(),
                    _ => &[],
                })
                .filter(|layer| layer.name.as_ref() == "review movie")
                .collect();
            assert_eq!(movies.len(), 1, "{:?}", exported.diagnostics);
            assert_eq!(movies[0].record.flags().audio_enabled, !hidden);
            assert_eq!(audio_levels(movies[0]).keyframes.len(), 2);
        }
    }
}
