use super::super::{to_structural_fx_document, to_structural_fx_document_with_animation_limit};
use super::*;
use crate::{
    rifx::{Chunk, Rifx},
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
};
use fx_schema::GroupLayer;

const CONTROLS: &[u8] =
    include_bytes!("../../../tests/fixtures/effects/cosmic-posterize-time-controls.rifx");

fn adjustment_with_controls(controls: &[u8]) -> Layer {
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!()
    };
    let mut layer = comp.layers[0].clone();
    layer.content = Rifx::parse_with(controls, |_| false)
        .unwrap()
        .chunks()
        .to_vec();
    let mut bytes = layer.record.encode();
    bytes[..4].copy_from_slice(&91_u32.to_be_bytes());
    bytes[38] |= 2; // Native Adjustment flag.
    for (offset, value) in [(12, 0_i32), (20, 0), (28, 456)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    for offset in [16, 24, 32] {
        bytes[offset..offset + 4].copy_from_slice(&96_u32.to_be_bytes());
    }
    layer.record = LayerRecord::decode(&bytes).unwrap();
    layer.name = "Native Hold adjustment".into();
    layer
}

fn native_adjustment() -> Layer {
    adjustment_with_controls(CONTROLS)
}

fn chunk_name(chunk: &Chunk, expected: &str) -> bool {
    chunk.id() == *b"tdmn"
        && chunk
            .data_payload()
            .is_some_and(|bytes| bytes.split(|byte| *byte == 0).next() == Some(expected.as_bytes()))
}

fn synthetic_name(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn effect_run(chunks: &[Chunk], match_name: &str) -> Option<Vec<Chunk>> {
    if let Some(index) = chunks
        .iter()
        .position(|chunk| chunk_name(chunk, match_name))
    {
        let end = chunks[index + 1..]
            .iter()
            .position(|chunk| chunk.id() == *b"tdmn")
            .map_or(chunks.len(), |next| index + 1 + next);
        return Some(chunks[index + 1..end].to_vec());
    }
    chunks.iter().find_map(|chunk| {
        chunk
            .children()
            .and_then(|children| effect_run(children, match_name))
    })
}

fn insert_before_effect(
    chunks: &mut [Chunk],
    before: &str,
    match_name: &str,
    run: &[Chunk],
) -> bool {
    for chunk in chunks {
        let Some(children) = chunk.children_mut() else {
            continue;
        };
        if let Some(index) = children.iter().position(|child| chunk_name(child, before)) {
            children.splice(
                index..index,
                std::iter::once(synthetic_name(match_name)).chain(run.iter().cloned()),
            );
            return true;
        }
        if insert_before_effect(children, before, match_name, run) {
            return true;
        }
    }
    false
}

fn prepend_effect(layer: &mut Layer, match_name: &str, run: Vec<Chunk>) {
    assert!(insert_before_effect(
        &mut layer.content,
        MATCH_NAME,
        match_name,
        &run,
    ));
}

fn malformed_disabled_effect() -> Vec<Chunk> {
    vec![Chunk::list(
        *b"sspc",
        vec![Chunk::list(
            *b"tdgp",
            vec![
                Chunk::data(*b"tdsb", vec![0, 0, 0, 0]).unwrap(),
                Chunk::data(*b"tdmn", b"malformed".to_vec()).unwrap(),
            ],
        )],
    )]
}

fn retain_first_rate_key(chunks: &mut [Chunk]) -> bool {
    for chunk in chunks {
        if chunk.list_kind() == Some(*b"list") {
            let children = chunk.children_mut().unwrap();
            for child in children {
                if child.id() == *b"lhd3" {
                    let mut bytes = child.data_payload().unwrap().to_vec();
                    bytes[10..12].copy_from_slice(&1_u16.to_be_bytes());
                    *child = Chunk::data(*b"lhd3", bytes).unwrap();
                } else if child.id() == *b"ldat" {
                    *child = Chunk::data(*b"ldat", child.data_payload().unwrap()[..48].to_vec())
                        .unwrap();
                }
            }
            return true;
        }
        if let Some(children) = chunk.children_mut()
            && retain_first_rate_key(children)
        {
            return true;
        }
    }
    false
}

fn static_native_adjustment() -> Layer {
    let mut layer = native_adjustment();
    assert!(retain_first_rate_key(&mut layer.content));
    assert_eq!(schedule(&layer, 4.75).unwrap().unwrap().len(), 1);
    layer
}

fn synthetic_numeric(values: &[f64]) -> Chunk {
    let mut metadata = vec![0; 124];
    metadata[..2].copy_from_slice(&[0xdb, 0x99]);
    metadata[3] = values.len() as u8;
    Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdb4", metadata).unwrap(),
            Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
            Chunk::data(
                *b"cdat",
                values
                    .iter()
                    .flat_map(|value| value.to_be_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        ],
    )
}

fn synthetic_split_controls() -> Vec<Chunk> {
    let mut controls = Vec::new();
    for (name, values) in [
        ("CC Split 2-0001", vec![10.0, 45.0]),
        ("CC Split 2-0002", vec![500.0, 45.0]),
        ("CC Split 2-0003", vec![20.0]),
        ("CC Split 2-0004", vec![20.0]),
    ] {
        controls.extend([synthetic_name(name), synthetic_numeric(&values)]);
    }
    let profile = [
        0_u32.to_le_bytes().to_vec(),
        vec![1_f32; 256]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
        [0_u32, 1, 1, 1]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect(),
    ]
    .concat();
    vec![Chunk::list(
        *b"tdgp",
        vec![
            synthetic_name("ADBE Effect Parade"),
            Chunk::list(
                *b"tdgp",
                vec![
                    synthetic_name("CC Split 2"),
                    Chunk::list(
                        *b"sspc",
                        vec![
                            Chunk::list(*b"parT", vec![]),
                            Chunk::list(*b"tdgp", controls),
                            Chunk::data(*b"sdat", profile).unwrap(),
                        ],
                    ),
                ],
            ),
        ],
    )]
}

fn split_above_posterize_project() -> crate::structure::StructuralProject {
    let mut project = project();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    comp.width = 320;
    comp.height = 180;
    let below = comp.layers[2].clone();
    let mut split = below.clone();
    let mut record = split.record.encode();
    record[..4].copy_from_slice(&90_u32.to_be_bytes());
    record[38] |= 2;
    record[131] = 0;
    record[40..44].copy_from_slice(&99_u32.to_be_bytes());
    for (offset, value) in [(12, 0_i32), (20, 0), (28, 456)] {
        record[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    for offset in [16, 24, 32] {
        record[offset..offset + 4].copy_from_slice(&96_u32.to_be_bytes());
    }
    split.record = LayerRecord::decode(&record).unwrap();
    split.name = "Synthetic supported Split above Posterize".into();
    split.content = synthetic_split_controls();
    comp.layers = vec![split, static_native_adjustment(), below];
    project.items.push(crate::structure::ProjectItem {
        id: 99,
        name: "Synthetic composition-sized Adjustment source".into(),
        parent_folder: None,
        kind: ItemKind::Footage,
        footage: None,
        media: None,
        native_media: None,
        solid: Some(Ok(crate::structure::SolidSource {
            width: 320,
            height: 180,
            pixel_aspect: (1, 1),
            color: [0.0; 3],
        })),
    });
    project
}

fn project() -> crate::structure::StructuralProject {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    let below = comp.layers[0].clone();
    let mut above = below.clone();
    let mut bytes = above.record.encode();
    bytes[..4].copy_from_slice(&92_u32.to_be_bytes());
    above.record = LayerRecord::decode(&bytes).unwrap();
    above.name = "Above stays live".into();
    comp.duration_secs = 4.75;
    comp.layers = vec![above, native_adjustment(), below];
    project
}

fn groups(converted: &super::super::StructuralConversion) -> &GroupLayer {
    let FxLayer::Group(root) = converted.document.composition().layers()[0].data() else {
        panic!()
    };
    root
}
fn group_data(layer: &fx_schema::Layer) -> &GroupLayer {
    let FxLayer::Group(group) = layer.data() else {
        panic!()
    };
    group
}

#[test]
fn native_hold_schedule_uses_ceiling_boundary_and_independent_source_zero_grids() {
    assert_eq!(
        schedule(&native_adjustment(), 4.75).unwrap().unwrap(),
        [
            Interval {
                start: 0,
                end: 334,
                rate: 24.0
            },
            Interval {
                start: 334,
                end: 4750,
                rate: 12.0
            },
        ]
    );
    let project = project();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = groups(&converted);
    assert_eq!(root.layers.len(), 3, "{:?}", converted.diagnostics);
    assert_eq!(root.layers[0].data().name(), "Above stays live");
    let mut identity_sets = Vec::new();
    for (branch, (start, end, rate)) in [(0, 334, 24.0), (334, 4750, 12.0)].into_iter().enumerate()
    {
        let gate = group_data(&root.layers[branch + 1]);
        assert_eq!(gate.playback.input_range().start.as_millis(), start);
        assert_eq!(gate.playback.input_range().end().as_millis(), end);
        assert!(gate.effects.is_empty());
        let Some(playback) = gate.playback.time_remap() else {
            panic!()
        };
        assert_eq!(
            playback
                .keyframes()
                .iter()
                .map(|k| (k.time.as_millis(), k.value.as_millis()))
                .collect::<Vec<_>>(),
            [(start, start), (end, end)]
        );
        let held = group_data(&gate.layers[0]);
        assert_eq!(held.playback.input_range().start, Time::ZERO);
        assert_eq!(held.playback.input_range().end().as_millis(), 4750);
        assert_eq!(
            held.playback,
            super::super::identity_playback(held.playback.input_range())
        );
        assert!(
            matches!(held.effects[0].data(), EffectData::Identified { effect: EffectPayload::Known(LayerEffect::PosterizeTime { frame_rate: Some(fps) }), .. } if *fps == rate)
        );
        let mut ids = HashSet::new();
        collect_layers(&FxLayer::Group(held.clone()), &mut ids, 0).unwrap();
        identity_sets.push(ids);
    }
    assert!(identity_sets[0].is_disjoint(&identity_sets[1]));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("unheld interval gates"))
    );
    assert!(
        !converted
            .diagnostics
            .iter()
            .any(|note| note.layer_id == Some(91)
                && note.message.contains("frameRate cannot animate"))
    );
    // Input 333 ms still belongs to 24 fps; rounding the gate down to 333 ms
    // would choose 12 fps and incorrectly jump backward from 291 2/3 to 250 ms.
    assert!(
        (333.0_f64 * 24.0 / 1000.0).floor() / 24.0 > (333.0_f64 * 12.0 / 1000.0).floor() / 12.0
    );
}

#[test]
fn synthetic_omitted_and_malformed_siblings_keep_posterize_and_their_diagnostics() {
    // Independently synthetic siblings exercise ordering and local recovery around
    // the existing publishable Cosmic Posterize controls; they are not native proof.
    let mut layer = native_adjustment();
    let unsupported = effect_run(&layer.content, MATCH_NAME).unwrap();
    prepend_effect(&mut layer, "Synthetic Unsupported Effect", unsupported);
    prepend_effect(
        &mut layer,
        "Synthetic Undecodable Effect",
        vec![Chunk::list(*b"sspc", vec![])],
    );
    let (_, warnings) = native::read_effects(&layer.content, [1.0, 1.0]);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("Synthetic Undecodable Effect"))
    );
    assert_eq!(
        schedule(&layer, 4.75).unwrap().unwrap(),
        [
            Interval {
                start: 0,
                end: 334,
                rate: 24.0,
            },
            Interval {
                start: 334,
                end: 4750,
                rate: 12.0,
            },
        ]
    );

    let mut project = project();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    comp.layers[1] = layer;
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = groups(&converted);
    assert_eq!(root.layers.len(), 3, "{:?}", converted.diagnostics);
    assert_eq!(root.layers[0].data().name(), "Above stays live");
    for layer in &root.layers[1..] {
        assert_eq!(group_data(layer).layers.len(), 1);
    }
    for name in [
        "Synthetic Unsupported Effect",
        "Synthetic Undecodable Effect",
    ] {
        assert!(
            converted
                .diagnostics
                .iter()
                .any(|note| note.message.contains(name)),
            "{name} loss must remain explicit: {:?}",
            converted.diagnostics
        );
    }
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("unheld interval gates"))
    );
    assert!(
        !converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("Posterize Time adjustment omitted"))
    );
}

#[test]
fn undecodable_second_posterize_keeps_original_stack_and_reports_schedule_loss() {
    let mut posterize = native_adjustment();
    prepend_effect(
        &mut posterize,
        MATCH_NAME,
        vec![Chunk::list(*b"sspc", vec![])],
    );
    let (effects, warnings) = native::read_effects(&posterize.content, [1.0, 1.0]);
    assert_eq!(
        effects
            .iter()
            .filter(|effect| effect.match_name == MATCH_NAME)
            .count(),
        1,
        "the malformed raw sibling must reproduce the decoded-list ambiguity"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("native plugin controls missing"))
    );
    assert!(
        schedule(&posterize, 4.75)
            .unwrap_err()
            .contains("additional enabled or undecodable Posterize Time")
    );

    let mut project = project();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    comp.layers[1] = posterize;
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = groups(&converted);
    assert_eq!(root.layers.len(), 3, "{:?}", converted.diagnostics);
    assert_eq!(root.layers[0].data().name(), "Above stays live");
    assert!(matches!(root.layers[1].data(), FxLayer::Adjustment(_)));
    assert!(matches!(root.layers[2].data(), FxLayer::Group(_)));
    assert!(converted.diagnostics.iter().any(|note| {
        note.message.contains("Posterize Time adjustment omitted")
            && note
                .message
                .contains("additional enabled or undecodable Posterize Time")
    }));
    assert!(
        !converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("unheld interval gates"))
    );
}

#[test]
fn malformed_confirmed_disabled_second_posterize_stays_inactive() {
    let mut posterize = native_adjustment();
    prepend_effect(&mut posterize, MATCH_NAME, malformed_disabled_effect());
    let (_, warnings) = native::read_effects(&posterize.content, [1.0, 1.0]);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("explicit control table malformed"))
    );
    assert_eq!(schedule(&posterize, 4.75).unwrap().unwrap().len(), 2);

    let mut project = project();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    comp.layers[1] = posterize;
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = groups(&converted);
    assert_eq!(root.layers.len(), 3, "{:?}", converted.diagnostics);
    assert_eq!(root.layers[0].data().name(), "Above stays live");
    for layer in &root.layers[1..] {
        assert_eq!(group_data(layer).layers.len(), 1);
    }
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("unheld interval gates"))
    );
    assert!(
        !converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("Posterize Time adjustment omitted"))
    );
}

#[test]
fn mapped_effect_keeps_the_original_adjustment_with_a_specific_reason() {
    let donor = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(donor_comp) = &donor.item(1).unwrap().kind else {
        panic!()
    };
    let gaussian = effect_run(&donor_comp.layers[0].content, "ADBE Gaussian Blur 2").unwrap();
    let mut posterize = native_adjustment();
    prepend_effect(&mut posterize, "ADBE Gaussian Blur 2", gaussian);
    let mut project = project();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    comp.layers[1] = posterize;

    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let FxLayer::Adjustment(adjustment) = groups(&converted).layers[1].data() else {
        panic!("mapped sibling effect must keep the original Adjustment")
    };
    assert_eq!(adjustment.effects.len(), 2, "{:?}", converted.diagnostics);
    assert!(adjustment.effects.iter().any(|effect| matches!(
        effect.data(),
        EffectData::Identified {
            effect: EffectPayload::Known(LayerEffect::GaussianBlur { .. }),
            ..
        }
    )));
    assert!(converted.diagnostics.iter().any(|note| {
        note.message
            .contains("another editable effect shares the Adjustment")
    }));
    assert!(
        !converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("masked, styled, animated-opacity"))
    );
}

#[test]
fn supported_split_above_keeps_original_correspondence_and_posterize_fallback() {
    let converted = to_structural_fx_document(&split_above_posterize_project(), Some(1)).unwrap();
    let root = groups(&converted);
    assert_eq!(root.layers.len(), 4, "{:?}", converted.diagnostics);
    assert!(matches!(root.layers[0].data(), FxLayer::Adjustment(_)));
    assert_eq!(root.layers[1].data().name(), "CC Split unchanged support");
    assert_eq!(root.layers[2].data().name(), "CC Split editable half");
    assert_eq!(root.layers[3].data().name(), "CC Split editable half");
    for layer in &root.layers[1..] {
        let group = group_data(layer);
        assert!(matches!(group.layers[0].data(), FxLayer::Adjustment(_)));
        assert!(!layer.data().name().starts_with("Posterize Time"));
    }
    assert!(converted.diagnostics.iter().any(|note| {
        note.message
            .contains("CC Split 2 stage above requires original sibling correspondence")
    }));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("CC Split 2: flat horizontal profile"))
    );
}

#[test]
fn synthetic_same_owner_split_loss_is_explicit_while_posterize_survives() {
    // The cloned controls only establish an enabled same-owner Split identity.
    // They deliberately do not claim a native-valid Split profile.
    let mut posterize = static_native_adjustment();
    let synthetic_split = effect_run(&posterize.content, MATCH_NAME).unwrap();
    prepend_effect(&mut posterize, "CC Split 2", synthetic_split);
    let mut project = project();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    comp.layers[1] = posterize;

    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = groups(&converted);
    assert_eq!(root.layers.len(), 2, "{:?}", converted.diagnostics);
    assert_eq!(root.layers[1].data().name(), "Posterize Time interval");
    assert!(converted.diagnostics.iter().any(|note| {
        note.message.contains("CC Split 2 omitted")
            && note.message.contains("same Adjustment as Posterize Time")
    }));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("unheld interval gates"))
    );
}

#[test]
fn partial_or_parented_or_nonunit_adjustment_keeps_original_stack() {
    for (offset, raw) in [(28, 455_i32), (12, 1), (8, 2), (132, 92)] {
        let mut project = project();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        let mut bytes = comp.layers[1].record.encode();
        bytes[offset..offset + 4].copy_from_slice(&raw.to_be_bytes());
        comp.layers[1].record = LayerRecord::decode(&bytes).unwrap();
        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        assert!(matches!(
            groups(&converted).layers[1].data(),
            FxLayer::Adjustment(_)
        ));
    }
}

#[test]
fn gate_budget_failure_restores_ids_animation_accounting_and_original_siblings() {
    let project = project();
    let converted = to_structural_fx_document_with_animation_limit(&project, Some(1), 1).unwrap();
    assert!(matches!(
        groups(&converted).layers[1].data(),
        FxLayer::Adjustment(_)
    ));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|note| note.message.contains("original siblings retained"))
    );
    assert_eq!(
        converted.animation_budget_used,
        converted.committed_animation_bytes
    );
}

#[test]
fn rate_schedule_rejects_expression_nonhold_nonfinite_and_colliding_windows() {
    let (effects, _) = native::read_effects(&native_adjustment().content, [1.0, 1.0]);
    let original = effects[0].parameters[0].numeric.as_ref().unwrap();
    for case in 0..10 {
        let mut rate = original.clone();
        match case {
            0 => rate.expression_present = true,
            1 => rate.keyframes[1].out_interpolation = 1,
            2 => rate.keyframes[1].values[0] = 0.0,
            3 => rate.keyframes[1].values[0] = f64::NAN,
            4 => rate.keyframes[1].time_secs = 0.0,
            5 => rate.keyframes[0].time_secs = 0.0001,
            6 => {
                rate.keyframes[0].time_secs = 0.0;
                rate.keyframes[1].time_secs = 4.75;
            }
            7 => rate.keyframes[1].values[0] = 0.5,
            8 => rate.keyframes[1].values[0] = 61.0,
            9 => rate.expression_enabled = true,
            _ => unreachable!(),
        }
        assert!(intervals(&rate, 4.75).is_err(), "case {case}");
    }
    let mut rate = original.clone();
    rate.keyframes.push(rate.keyframes[1].clone());
    rate.keyframes[1].time_secs = 0.3331;
    rate.keyframes[2].time_secs = 0.3332;
    assert!(intervals(&rate, 4.75).unwrap_err().contains("collide"));
}

#[test]
fn typed_scope_and_graph_checks_reject_crossing_and_time_based_content() {
    use fx_schema::{
        PropType, PropertyAnimator, PropertyValue, TimeOffset,
        animator::{PropertyKeyframe, PropertyKeyframeTrack},
    };
    let id = LayerId::new(10);
    let mut child = group(
        id,
        "held visual".into(),
        Some(LayerId::new(1)),
        TimeRangeProperty::new(Time::ZERO, fx_schema::Duration::from_millis(4750)),
    );
    let ids = HashSet::from([id]);
    assert!(closed_visual(
        &FxLayer::Group(child.clone()),
        &ids,
        LayerId::new(1),
        true
    ));
    let video: fx_schema::VideoLayer = serde_json::from_value(serde_json::json!({
        "id":10,"name":"unheld physical source","parent":1,"playback":child.playback,
        "sourceRange":{"start":0,"duration":4750},"sourceIntrinsicDuration":4750,
        "transform":child.transform,
        "source":fx_schema::VideoSource::from_asset(fx_schema::AssetId::from_trusted("source"),None,fx_schema::MediaFit::Stretch)
    })).unwrap();
    assert!(!closed_visual(
        &FxLayer::Video(video.clone()),
        &ids,
        LayerId::new(1),
        true
    ));
    assert!(
        closed_visual(&FxLayer::Video(video), &ids, LayerId::new(1), false),
        "above video remains at its live clock"
    );
    child.track_matte = Some(fx_schema::TrackMatte {
        layer: LayerId::new(2),
        mode: fx_schema::TrackMatteType::Alpha,
    });
    assert!(!closed_visual(
        &FxLayer::Group(child),
        &ids,
        LayerId::new(1),
        true
    ));
    let mut project = project();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    let mut record = comp.layers[2].record.encode();
    record[132..136].copy_from_slice(&92_u32.to_be_bytes());
    comp.layers[2].record = LayerRecord::decode(&record).unwrap();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    assert!(
        matches!(groups(&converted).layers[1].data(), FxLayer::Adjustment(_)),
        "native baked parenting must also reject"
    );
    let entry = AnimationGraphEntry {
        target: fx_schema::Property::new(id, PropType::Opacity).into(),
        animator: PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
                KeyframeId::new("held-key"),
                TimeOffset::from_millis(0),
                PropertyValue::Float(100.0),
                PropertyKeyframeEasing::Linear,
            )])
            .unwrap(),
        ),
        dependencies: vec![],
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    assert!(animations_closed(std::slice::from_ref(&entry), &(10..20)));
    let mut crossing = entry.clone();
    crossing.target = fx_schema::Property::new(LayerId::new(2), PropType::Opacity).into();
    crossing.dependencies.push(entry.target.clone());
    assert!(!animations_closed(&[entry.clone(), crossing], &(10..20)));
    let mut scripted = entry;
    scripted.animator = PropertyAnimator::constant(PropertyValue::Float(100.0)).unwrap();
    assert!(!animations_closed(&[scripted], &(10..20)));
}

#[test]
fn nested_copy_identity_failure_restores_converter_context_and_original_output() {
    use super::super::{
        animation_budget::AnimationBudget,
        media::{AssetNamespace, MediaResolution},
        shapes,
    };
    let mut project = project();
    let mut nested = project
        .items
        .iter()
        .find(|item| item.id == 1)
        .unwrap()
        .clone();
    nested.id = 32;
    let ItemKind::Composition(comp) = &mut nested.kind else {
        panic!()
    };
    comp.layers = vec![comp.layers[2].clone()];
    project.items.push(nested);
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    let mut record = comp.layers[2].record.encode();
    record[131] = 0; // Ordinary AV precomp.
    record[40..44].copy_from_slice(&32_u32.to_be_bytes());
    comp.layers[2].record = LayerRecord::decode(&record).unwrap();
    let expression_samples = Default::default();
    let mut resolver = |_: &super::super::MediaAssetRequest| MediaResolution::Unavailable;
    let first_id = u64::MAX - 8;
    let mut converter = Converter {
        text_overrides: Default::default(),
        expression_samples: &expression_samples,
        expression_evaluations: Default::default(),
        items: project.items.iter().map(|item| (item.id, item)).collect(),
        camera_normalizations: Default::default(),
        diagnostics: vec![],
        next_id: first_id,
        linked: false,
        asset_namespace: AssetNamespace::STANDALONE,
        stack: vec![1],
        visited_compositions: HashSet::from([1]),
        animations: vec![],
        animation_budget: AnimationBudget::default(),
        committed_inline_remap_bytes: 0,
        unavailable_cutouts: 0,
        overrides: vec![],
        media_resolver: &mut resolver,
        assets: vec![],
        shape_budget: shapes::OutputBudget::default(),
        mapped_shape_expressions: Default::default(),
        root_progress: fx_conv::Progress::default().phase("test", "layers", 0),
    };
    let ItemKind::Composition(comp) = &project.items.iter().find(|item| item.id == 1).unwrap().kind
    else {
        panic!()
    };
    let layer_indices = super::super::index_layers(&comp.layers);
    let context = LayerContext {
        expression_samples: &expression_samples,
        comp_id: 1,
        comp,
        parent: LayerId::new(1),
        depth: 0,
        solo: false,
        layer_indices: &layer_indices,
        camera_normalization: None,
    };
    let mut adjustment: fx_schema::AdjustmentLayer = serde_json::from_value(serde_json::json!({
        "id":2,"name":"hold","parent":1,"activeRange":{"start":0,"duration":4750},
        "transform":group(LayerId::new(2),"".into(),None,TimeRangeProperty::new(Time::ZERO,fx_schema::Duration::from_millis(4750))).transform,
        "effects":[{"id":3,"enabled":true,"effect":{"type":"posterizeTime","frameRate":24}}]
    })).unwrap();
    adjustment.transform.opacity = fx_schema::PercentageProperty::new(100.0).unwrap();
    let layers = vec![
        FxLayer::Adjustment(adjustment),
        FxLayer::Group(group(
            LayerId::new(10),
            "original".into(),
            Some(LayerId::new(1)),
            TimeRangeProperty::new(Time::ZERO, fx_schema::Duration::from_millis(4750)),
        )),
    ];
    let before = layers.clone();
    let windows = schedule(&comp.layers[1], 4.75).unwrap().unwrap();
    converter.animations.push(AnimationGraphEntry {
        target: fx_schema::Property::new(LayerId::new(99), fx_schema::PropType::Opacity).into(),
        animator: fx_schema::PropertyAnimator::constant(fx_schema::PropertyValue::Float(100.0))
            .unwrap(),
        dependencies: vec![
            fx_schema::Property::new(LayerId::new(2), fx_schema::PropType::Opacity).into(),
        ],
        random_seed_target: None,
        layer_refs: Default::default(),
    });
    assert!(
        converter
            .posterized_stack(&context, &[1, 2], 20, &layers, 0, &windows)
            .unwrap_err()
            .contains("removed adjustment")
    );
    converter.animations.clear();
    assert!(
        converter
            .posterized_stack(&context, &[1, 2], 20, &layers, 0, &windows)
            .is_err()
    );
    assert_eq!(
        converter.stack,
        [1],
        "failed nested composition cannot leak its pushed stack entry"
    );
    assert!(converter.overrides.is_empty());
    assert_eq!(converter.next_id, first_id);
    assert_eq!(converter.animation_budget.used(), 0);
    assert_eq!(converter.committed_inline_remap_bytes, 0);
    assert!(converter.animations.is_empty());
    assert_eq!(layers, before);

    // A direct Set Matte wrapper can be allocated after emitted_end while
    // preserving the sibling count. Its new target would escape the original
    // interval, especially when a static rate creates no second-copy check.
    let mut static_comp = comp.clone();
    fn first_key_only(chunks: &mut [crate::rifx::Chunk]) -> bool {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"list") {
                let children = chunk.children_mut().unwrap();
                for child in children {
                    if child.id() == *b"lhd3" {
                        let mut bytes = child.data_payload().unwrap().to_vec();
                        bytes[10..12].copy_from_slice(&1_u16.to_be_bytes());
                        *child = crate::rifx::Chunk::data(*b"lhd3", bytes).unwrap();
                    } else if child.id() == *b"ldat" {
                        *child = crate::rifx::Chunk::data(
                            *b"ldat",
                            child.data_payload().unwrap()[..48].to_vec(),
                        )
                        .unwrap();
                    }
                }
                return true;
            }
            if let Some(children) = chunk.children_mut()
                && first_key_only(children)
            {
                return true;
            }
        }
        false
    }
    assert!(first_key_only(&mut static_comp.layers[1].content));
    assert_eq!(
        schedule(&static_comp.layers[1], 4.75)
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    let static_layer_indices = super::super::index_layers(&static_comp.layers);
    let static_context = LayerContext {
        comp: &static_comp,
        layer_indices: &static_layer_indices,
        ..context
    };
    let mut wrapper = group(
        LayerId::new(20),
        "Set Matte wrapper".into(),
        Some(LayerId::new(1)),
        TimeRangeProperty::new(Time::ZERO, fx_schema::Duration::from_millis(4750)),
    );
    let mut child = layers[1].clone();
    reparent(&mut child, wrapper.id).unwrap();
    wrapper
        .layers
        .push(fx_schema::Layer::from_data(&child).unwrap());
    let mut post_helper = vec![layers[0].clone(), FxLayer::Group(wrapper)];
    let original_helpers = post_helper.clone();
    converter.next_id = 21;
    converter.animations.push(AnimationGraphEntry {
        target: fx_schema::Property::new(LayerId::new(20), fx_schema::PropType::Opacity).into(),
        animator: fx_schema::PropertyAnimator::constant(fx_schema::PropertyValue::Float(100.0))
            .unwrap(),
        dependencies: vec![],
        random_seed_target: None,
        layer_refs: Default::default(),
    });
    converter
        .apply_posterize_time(&static_context, &[1, 2], 20, &mut post_helper)
        .unwrap();
    assert_eq!(
        post_helper, original_helpers,
        "post-occurrence helper ownership must reject even for one static interval"
    );
    assert_eq!(converter.next_id, 21);
}

#[test]
fn raw_nondefault_compositing_options_and_duplicate_rates_are_rejected() {
    use crate::rifx::Chunk;
    fn controls(chunks: &mut [Chunk]) -> Option<&mut Vec<Chunk>> {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"sspc") {
                return chunk
                    .children_mut()?
                    .iter_mut()
                    .find(|child| child.list_kind() == Some(*b"tdgp"))?
                    .children_mut();
            }
            if let Some(children) = chunk.children_mut()
                && let Some(found) = controls(children)
            {
                return Some(found);
            }
        }
        None
    }
    for duplicate in [false, true] {
        let mut layer = native_adjustment();
        let controls = controls(&mut layer.content).unwrap();
        if duplicate {
            let start = controls
                .iter()
                .position(|chunk| {
                    chunk.id() == *b"tdmn"
                        && chunk.data_payload().unwrap().starts_with(RATE.as_bytes())
                })
                .unwrap();
            let end = controls[start + 1..]
                .iter()
                .position(|chunk| chunk.id() == *b"tdmn")
                .map_or(controls.len(), |i| start + 1 + i);
            let extra = controls[start..end].to_vec();
            controls.splice(start..start, extra);
        } else {
            let start = controls
                .iter()
                .position(|chunk| {
                    chunk.id() == *b"tdmn"
                        && chunk
                            .data_payload()
                            .unwrap()
                            .starts_with(b"ADBE Effect Built In Params")
                })
                .unwrap();
            let options = controls[start + 1..]
                .iter_mut()
                .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                .unwrap()
                .children_mut()
                .unwrap();
            options.push(Chunk::data(*b"tdmn", b"ADBE Effect Mask Opacity\0".to_vec()).unwrap());
            options.push(Chunk::list(
                *b"tdbs",
                vec![Chunk::data(*b"cdat", 50.0_f64.to_be_bytes().to_vec()).unwrap()],
            ));
        }
        assert!(schedule(&layer, 4.75).is_err());
    }
}
