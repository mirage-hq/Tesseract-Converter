use std::collections::HashSet;

use super::*;
use crate::{
    effects::{fractal_noise, native},
    properties,
    rifx::Rifx,
    schema::layer_records::LayerRecord,
    structure::{ItemKind, StructuralProject, read_project},
};
use fx_schema::{Duration, EffectId, Time};

fn native_project() -> StructuralProject {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let fixture = Rifx::parse_with(
        include_bytes!("../../../tests/fixtures/effects/native-fractal-noise-controls.rifx"),
        |_| false,
    )
    .unwrap();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    let mut layer = comp.layers[0].clone();
    layer.content = fixture.chunks()[0].children().unwrap().to_vec();
    layer.record =
        LayerRecord::decode(properties::data(&layer.content, *b"ldta").unwrap()).unwrap();
    layer.name = "Renamed native Fractal occurrence".into();
    let source_id = layer.record.source_id();
    comp.layers = vec![layer];
    comp.width = 1920;
    comp.height = 1080;
    comp.duration_secs = 6.;
    let mut source = project.items[0].clone();
    source.id = source_id;
    source.name = "Renamed opaque source plane".into();
    source.kind = ItemKind::Unknown(0);
    source.solid = Some(Ok(SolidSource {
        width: 1280,
        height: 720,
        pixel_aspect: (1, 1),
        color: [0.; 3],
    }));
    project.items.push(source);
    project
}

fn group_data(layer: &fx_schema::Layer) -> &GroupLayer {
    let LayerData::Group(group) = layer.data() else {
        panic!("expected Group")
    };
    group
}

fn identified(id: u64, effect: LayerEffect) -> EffectRecord {
    EffectRecord::from_data(&EffectData::Identified {
        id: EffectId::new(id),
        enabled: true,
        effect: EffectPayload::Known(effect),
    })
    .unwrap()
}

fn effect_id(effect: &EffectRecord) -> EffectId {
    let EffectData::Identified { id, .. } = effect.data() else {
        panic!()
    };
    *id
}

fn stage(id: u64, ordinal: usize, mode: BlendMode, opacity: f64) -> Stage {
    let project = native_project();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!()
    };
    let layer = &comp.layers[0];
    let sources = native::read_effects(&layer.content, [1280., 720.]).0;
    let source = sources
        .iter()
        .filter(|source| source.match_name == fractal_noise::MATCH_NAME)
        .nth(2)
        .unwrap();
    let (generator, _, _) = fractal_noise::blend_stage(layer, source, [1280, 720], true).unwrap();
    Stage {
        native_ordinal: ordinal,
        generator: identified(id, generator),
        animations: Vec::new(),
        blend_mode: mode,
        opacity: PercentageProperty::new(opacity).unwrap(),
    }
}

fn owner() -> GroupLayer {
    let span = TimeRangeProperty::new(Time::ZERO, Duration::from_secs(6.));
    let mut owner = super::super::group(LayerId::new(1), "Occurrence".into(), None, span);
    owner.transform.position = fx_schema::Position::TwoD([20., 30.]);
    owner.transform.opacity = PercentageProperty::new(65.).unwrap();
    let mut content = super::super::group(
        LayerId::new(2),
        "Original animated source clock".into(),
        Some(owner.id),
        span,
    );
    let rect = transform::solid_rect(
        &SolidSource {
            width: 1280,
            height: 720,
            pixel_aspect: (1, 1),
            color: [0.3; 3],
        },
        &content,
        LayerId::new(3),
        content.transform,
    );
    content.layers = stored_layers(vec![LayerData::Rect(rect)]).unwrap();
    owner.layers = stored_layers(vec![LayerData::Group(content)]).unwrap();
    owner.effects = [201, 202, 203, 204]
        .into_iter()
        .map(|id| {
            identified(
                id,
                LayerEffect::Exposure {
                    exposure: Some(0.5),
                    offset: Some(0.),
                    gamma_correction: Some(1.),
                },
            )
        })
        .collect();
    owner
}

fn context(hidden: bool) -> Context<'static> {
    Context {
        ordinals: &[1, 3, 5],
        size: [1280, 720],
        visibility: Visibility {
            range: TimeRangeProperty::new(Time::from_secs(2.), Duration::from_secs(1.)),
            hidden,
        },
        parent_depth: 0,
    }
}

#[test]
fn native_generator_retains_editable_prefix_and_reports_spatial_omissions() {
    let project = native_project();
    let imported = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
    let root = group_data(&imported.document.composition().layers()[0]);
    let owner = group_data(&root.layers[0]);
    let output = group_data(&owner.layers[0]);
    assert_eq!(output.name, "Fractal blend output");
    let noise = group_data(&output.layers[0]);
    let prefix = group_data(&output.layers[1]);
    assert_eq!(noise.blend_mode, BlendMode::Multiply);
    assert_eq!(noise.transform.opacity.value(), 100.);
    assert_eq!(prefix.effects.len(), 1);
    assert!(matches!(
        prefix.effects[0].data(),
        EffectData::Identified {
            effect: EffectPayload::Known(LayerEffect::TurbulentNoise { .. }),
            ..
        }
    ));
    let LayerData::Rect(rect) = noise.layers[0].data() else {
        panic!()
    };
    assert_eq!(rect.rect.size, [1280., 720.]);
    assert_eq!(rect.rect.fill_color, [1.; 4]);
    assert!(matches!(
        rect.effects[0].data(),
        EffectData::Identified {
            effect: EffectPayload::Known(LayerEffect::TurbulentNoise {
                scale: Some(666.),
                contrast: Some(122.),
                blend: Some(1.),
                ..
            }),
            ..
        }
    ));
    assert_eq!(prefix.layers.len(), 1);
    assert_eq!(group_data(&prefix.layers[0]).name, "Source content clock");
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Geometry2") && d.message.contains("effect omitted"))
    );

    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Fractal") && d.message.contains("approximat"))
    );
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Fractal") && d.message.contains("omitted"))
    );
}

#[test]
fn multiple_stages_preserve_original_targets_prefix_order_and_outer_suffix() {
    let mut owner = owner();
    let original = group_data(&owner.layers[0]).clone();
    let original_transform = owner.transform;
    let mut next = 300;
    let mut budget = OutputBudget::default();
    let stages = [
        stage(101, 2, BlendMode::Screen, 25.),
        stage(102, 4, BlendMode::Multiply, 100.),
    ];
    assert!(
        apply(
            &mut owner,
            &stages,
            context(false),
            State {
                animation_budget: &mut AnimationBudget::default(),
                animations: &mut Vec::new(),
                next: &mut next,
                budget: &mut budget
            }
        )
        .unwrap()
    );
    assert_eq!(next, 308);
    assert_eq!(owner.id, LayerId::new(1));
    assert_eq!(owner.transform, original_transform);
    // Native suffix and trailing styles stay on the unchanged occurrence owner.
    assert_eq!(
        owner.effects.iter().map(effect_id).collect::<Vec<_>>(),
        [EffectId::new(203), EffectId::new(204)]
    );
    let last = group_data(&owner.layers[0]);
    let last_noise = group_data(&last.layers[0]);
    let between = group_data(&last.layers[1]);
    assert_eq!(last_noise.blend_mode, BlendMode::Multiply);
    assert_eq!(
        between.effects.iter().map(effect_id).collect::<Vec<_>>(),
        [EffectId::new(202)]
    );
    let first = group_data(&between.layers[0]);
    let first_noise = group_data(&first.layers[0]);
    let prefix = group_data(&first.layers[1]);
    assert_eq!(first_noise.blend_mode, BlendMode::Screen);
    assert_eq!(first_noise.transform.opacity.value(), 25.);
    assert_eq!(
        prefix.effects.iter().map(effect_id).collect::<Vec<_>>(),
        [EffectId::new(201)]
    );
    let retained = group_data(&prefix.layers[0]);
    let mut expected = original;
    expected.parent = Some(prefix.id);
    assert_eq!(retained, &expected);
    let mut pending = vec![&owner];
    let mut layer_ids = HashSet::new();
    let mut effect_ids = HashSet::new();
    while let Some(group) = pending.pop() {
        assert!(layer_ids.insert(group.id));
        for effect in &group.effects {
            assert!(effect_ids.insert(effect_id(effect)));
        }
        for layer in &group.layers {
            if let LayerData::Group(child) = layer.data() {
                pending.push(child);
            } else {
                assert!(layer_ids.insert(layer.id()));
                for effect in layer.effects() {
                    assert!(effect_ids.insert(effect_id(effect)));
                }
            }
        }
    }
    assert_eq!(effect_ids.len(), 6);
    assert!(layer_ids.contains(&LayerId::new(2)));
    assert!(layer_ids.contains(&LayerId::new(3)));
}

#[test]
fn generated_plane_keeps_nonzero_visibility_and_neutral_disabled_paint() {
    for (mode, neutral) in [
        (BlendMode::Screen, [0., 0., 0., 1.]),
        (BlendMode::Multiply, [1.; 4]),
    ] {
        for hidden in [false, true] {
            let mut owner = owner();
            let visibility = context(hidden).visibility;
            let mut next = 300;
            let mut budget = OutputBudget::default();
            apply(
                &mut owner,
                &[stage(101, 2, mode, 25.)],
                context(hidden),
                State {
                    animation_budget: &mut AnimationBudget::default(),
                    animations: &mut Vec::new(),
                    next: &mut next,
                    budget: &mut budget,
                },
            )
            .unwrap();
            let output = group_data(&owner.layers[0]);
            let noise = group_data(&output.layers[0]);
            assert_eq!(noise.transform.opacity.value(), 25.);
            assert_eq!(
                noise.playback,
                super::super::identity_playback(owner.playback.input_range())
            );
            let LayerData::Rect(rect) = noise.layers[0].data() else {
                panic!()
            };
            assert_eq!(rect.active_range, visibility.range);
            assert_eq!(rect.is_hidden, hidden);
            assert_eq!(rect.transform.opacity.value(), 100.);
            assert_eq!(rect.rect.fill_color, neutral);
            let mut disabled = rect.effects[0].data().clone();
            let EffectData::Identified { enabled, .. } = &mut disabled else {
                panic!()
            };
            *enabled = false;
            let disabled = EffectRecord::from_data(&disabled).unwrap();
            assert_eq!(effect_id(&disabled), EffectId::new(101));
            assert!(matches!(
                disabled.data(),
                EffectData::Identified { enabled: false, .. }
            ));
        }
    }
}

#[test]
fn helper_failures_restore_owner_identity_cursor_and_serialized_accounting() {
    let stages = [
        stage(101, 2, BlendMode::Screen, 25.),
        stage(102, 4, BlendMode::Multiply, 100.),
    ];
    let original = owner();
    let mut exhausted = original.clone();
    let mut next = u64::MAX - 6;
    let saved_next = next;
    let mut budget = OutputBudget::default();
    assert!(
        apply(
            &mut exhausted,
            &stages,
            context(false),
            State {
                animation_budget: &mut AnimationBudget::default(),
                animations: &mut Vec::new(),
                next: &mut next,
                budget: &mut budget
            }
        )
        .is_err()
    );
    assert_eq!(exhausted, original);
    assert_eq!(next, saved_next);
    assert_eq!(budget.checkpoint(), 0);
    let mut exhausted = original.clone();
    let mut next = 300;
    let mut budget = OutputBudget::with_limit(0);
    assert!(
        apply(
            &mut exhausted,
            &stages,
            context(false),
            State {
                animation_budget: &mut AnimationBudget::default(),
                animations: &mut Vec::new(),
                next: &mut next,
                budget: &mut budget
            }
        )
        .is_err()
    );
    assert_eq!(exhausted, original);
    assert_eq!(next, 300);
    assert_eq!(budget.checkpoint(), 0);
    let mut exhausted = original.clone();
    let mut next = 300;
    let mut budget = OutputBudget::default();
    let mut deep = context(false);
    deep.parent_depth = MAX_GROUP_DEPTH - 4;
    assert!(
        apply(
            &mut exhausted,
            &stages[..1],
            deep,
            State {
                animation_budget: &mut AnimationBudget::default(),
                animations: &mut Vec::new(),
                next: &mut next,
                budget: &mut budget
            }
        )
        .is_err()
    );
    assert_eq!(exhausted, original);
    assert_eq!(next, 300);
    assert_eq!(budget.checkpoint(), 0);
}

#[test]
fn one_native_prefix_stage_can_expand_into_multiple_identified_records() {
    let mut owner = owner();
    let mut next = 300;
    let mut budget = OutputBudget::default();
    let mut context = context(false);
    context.ordinals = &[1, 1, 5];
    apply(
        &mut owner,
        &[stage(101, 2, BlendMode::Multiply, 100.)],
        context,
        State {
            animation_budget: &mut AnimationBudget::default(),
            animations: &mut Vec::new(),
            next: &mut next,
            budget: &mut budget,
        },
    )
    .unwrap();
    let output = group_data(&owner.layers[0]);
    let prefix = group_data(&output.layers[1]);
    assert_eq!(
        prefix.effects.iter().map(effect_id).collect::<Vec<_>>(),
        [EffectId::new(201), EffectId::new(202)]
    );
    assert_eq!(
        owner.effects.iter().map(effect_id).collect::<Vec<_>>(),
        [EffectId::new(203), EffectId::new(204)]
    );
}

#[test]
fn native_visibility_gates_generated_plane_and_disabled_effects_add_no_stage() {
    for (enabled, effects_active) in [(true, true), (false, true), (true, false)] {
        let mut project = native_project();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        let layer = &mut comp.layers[0];
        let mut bytes = layer.record.encode();
        // Preserve the native structural fixture while varying only occurrence
        // visibility, the master effect switch, and the source-clock rationals.
        bytes[39] = (bytes[39] & !5) | u8::from(enabled) | (u8::from(effects_active) << 2);
        for (offset, numerator, denominator_offset, denominator) in [
            (8, 1_i32, 108, 1_u32),
            (12, 2, 16, 1),
            (20, 0, 24, 1),
            (28, 1, 32, 1),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&numerator.to_be_bytes());
            bytes[denominator_offset..denominator_offset + 4]
                .copy_from_slice(&denominator.to_be_bytes());
        }
        layer.record = LayerRecord::decode(&bytes).unwrap();
        *layer
            .content
            .iter_mut()
            .find(|c| c.id() == *b"ldta")
            .unwrap() = crate::rifx::Chunk::data(*b"ldta", bytes).unwrap();
        let imported = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
        let root = group_data(&imported.document.composition().layers()[0]);
        let owner = group_data(&root.layers[0]);
        let content = group_data(&owner.layers[0]);
        if effects_active {
            assert_eq!(content.name, "Fractal blend output");
            let noise = group_data(&content.layers[0]);
            let LayerData::Rect(rect) = noise.layers[0].data() else {
                panic!()
            };
            assert_eq!(
                rect.active_range,
                TimeRangeProperty::new(Time::from_secs(2.), Duration::from_secs(1.))
            );
            assert_eq!(rect.is_hidden, !enabled);
        } else {
            assert_eq!(content.name, "Source content clock");
            assert!(owner.effects.iter().all(|effect| matches!(
                effect.data(),
                EffectData::Identified { enabled: false, .. }
            )));
        }
    }
}

#[test]
fn independent_native_generator_uses_canvas_proof_without_relaxing_in_place_alpha_gate() {
    let project = native_project();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!()
    };
    let layer = &comp.layers[0];
    let sources = native::read_effects(&layer.content, [1280., 720.]).0;
    let third = sources
        .iter()
        .filter(|source| source.match_name == fractal_noise::MATCH_NAME)
        .nth(2)
        .unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    assert!(!fractal_noise::opaque_ordinals(layer, &sources, Some(&items)).contains(&third.index));
    assert!(fractal_noise::blend_canvas(layer, Some(&items)));
    assert!(fractal_noise::lower(third, layer, [1280, 720], false).is_err());
    assert!(fractal_noise::blend_stage(layer, third, [1280, 720], true).is_ok());
}

#[test]
fn effect_only_disable_retains_opaque_carrier_over_transparent_prefix() {
    for mode in [BlendMode::Multiply, BlendMode::Screen] {
        let mut owner = owner();
        let LayerData::Group(mut source) = owner.layers[0].data().clone() else {
            panic!()
        };
        source.transform.opacity = PercentageProperty::new(0.).unwrap();
        let source_id = source.id;
        owner.layers = stored_layers(vec![LayerData::Group(source)]).unwrap();
        let mut next = 300;
        let mut budget = OutputBudget::default();
        apply(
            &mut owner,
            &[stage(101, 2, mode, 25.)],
            context(false),
            State {
                animation_budget: &mut AnimationBudget::default(),
                animations: &mut Vec::new(),
                next: &mut next,
                budget: &mut budget,
            },
        )
        .unwrap();
        let output = group_data(&owner.layers[0]);
        let noise = group_data(&output.layers[0]);
        let retained = group_data(&group_data(&output.layers[1]).layers[0]);
        assert_eq!(retained.id, source_id);
        assert_eq!(retained.transform.opacity.value(), 0.);
        let LayerData::Rect(rect) = noise.layers[0].data() else {
            panic!()
        };
        let mut disabled = rect.clone();
        let mut effect = disabled.effects[0].data().clone();
        let EffectData::Identified { enabled, .. } = &mut effect else {
            panic!()
        };
        *enabled = false;
        disabled.effects[0] = EffectRecord::from_data(&effect).unwrap();
        assert_eq!(disabled.rect.fill_color[3], 1.);
        assert_eq!(noise.transform.opacity.value(), 25.);
        // Source-over of a 25-percent opaque carrier onto transparent input
        // retains 25-percent alpha even when its generator effect is disabled.
        let prefix_alpha = retained.transform.opacity.value() / 100.;
        let carrier_alpha = disabled.rect.fill_color[3] * noise.transform.opacity.value() / 100.;
        assert_eq!(carrier_alpha + prefix_alpha * (1. - carrier_alpha), 0.25);
    }
}

#[test]
fn keyed_fractal_stage_commits_or_rolls_back_tracks_with_wrappers() {
    let mut keyed = stage(101, 2, BlendMode::Multiply, 100.);
    keyed.animations.push(serde_json::from_value(serde_json::json!({
        "target":{"kind":"effectProperty","effectId":101,"paramName":"contrast"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"fractal-a","layerTime":0,"value":{"type":"float","value":100.},"easing":{"type":"linear"}},
            {"id":"fractal-b","layerTime":1000,"value":{"type":"float","value":130.},"easing":{"type":"linear"}}
        ]}
    })).unwrap());
    for failure in [None, Some("shapes"), Some("animations")] {
        let original = owner();
        let mut candidate = original.clone();
        let mut next = 300;
        let mut shapes = if failure == Some("shapes") {
            OutputBudget::with_limit(0)
        } else {
            OutputBudget::default()
        };
        let mut animation_budget = if failure == Some("animations") {
            AnimationBudget::with_limit(0)
        } else {
            AnimationBudget::default()
        };
        let mut animations = Vec::new();
        let result = apply(
            &mut candidate,
            std::slice::from_ref(&keyed),
            context(false),
            State {
                next: &mut next,
                budget: &mut shapes,
                animation_budget: &mut animation_budget,
                animations: &mut animations,
            },
        );
        if failure.is_some() {
            assert!(result.is_err());
            assert_eq!(candidate, original);
            assert_eq!(next, 300);
            assert!(animations.is_empty());
            assert_eq!(animation_budget.used(), 0);
            assert_eq!(shapes.checkpoint(), 0);
        } else {
            assert_eq!(result, Ok(true));
            assert_eq!(animations, keyed.animations);
            assert_eq!(
                animation_budget.used(),
                committed_entry_reservation_bytes(&animations[0]).unwrap()
            );
            let output = group_data(&candidate.layers[0]);
            let noise = group_data(&output.layers[0]);
            let LayerData::Rect(rect) = noise.layers[0].data() else {
                panic!()
            };
            assert_eq!(effect_id(&rect.effects[0]), EffectId::new(101));
        }
    }
}
