use super::*;
use crate::{
    rifx::Rifx,
    schema::layer_records::LayerRecord,
    structure::{ItemKind, SolidSource, read_project},
};
use fx_schema::LayerData;

const CONTROLS: &[u8] =
    include_bytes!("../../../tests/fixtures/effects/cosmic-self-mask-controls.rifx");
fn project() -> crate::structure::StructuralProject {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let item = project.items.iter().find(|i| i.id == 1).unwrap();
    let ItemKind::Composition(comp) = &item.kind else {
        panic!()
    };
    let mut layer = comp.layers[0].clone();
    layer.content = Rifx::parse_with(CONTROLS, |_| false)
        .unwrap()
        .chunks()
        .to_vec();
    layer.record =
        LayerRecord::decode(crate::properties::data(&layer.content, *b"ldta").unwrap()).unwrap();
    layer.name = "Native self inverse solid".into();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    comp.width = 3840;
    comp.height = 2160;
    comp.duration_secs = 6.0;
    comp.layers = vec![layer];
    let mut source = project.items[0].clone();
    source.id = 96;
    source.name = "Native white solid".into();
    source.kind = ItemKind::Footage;
    source.solid = Some(Ok(SolidSource {
        width: 3840,
        height: 2160,
        pixel_aspect: (1, 1),
        color: [1., 1., 1.],
    }));
    source.media = None;
    project.items.push(source);
    project
}
fn group(layer: &fx_schema::Layer) -> &GroupLayer {
    let LayerData::Group(g) = layer.data() else {
        panic!()
    };
    g
}
#[test]
fn native_self_inverse_mask_keeps_source_clock_and_effect_order() {
    let conversion =
        crate::structure_document::to_structural_fx_document(&project(), Some(1)).unwrap();
    let root = group(&conversion.document.composition().layers()[0]);
    let owner = group(&root.layers[0]);
    let shadows = group(&owner.layers[0]);
    assert_eq!(
        shadows.name, "Self-inverse matte shadow stage",
        "{:?}",
        conversion.diagnostics
    );
    let masked = group(&shadows.layers[0]);
    assert_eq!(masked.name, "Self-inverse matte source mask");
    assert_eq!(owner.masks.len(), 1);
    assert!(owner.masks[0].inverted);
    assert_eq!(masked.masks.len(), 1);
    assert!(!masked.masks[0].inverted);
    assert_eq!(owner.masks[0].layer, masked.masks[0].layer);
    assert!(shadows.masks.is_empty());
    assert!(masked.effects.is_empty());
    assert_eq!(shadows.effects.len(), 2);
    assert_eq!(owner.effects.len(), 2);
    let content = group(&masked.layers[0]);
    assert_eq!(content.playback.input_range().start.as_millis(), 333);
    assert!(
        content.playback.time_remap().is_none(),
        "constant Solid content retains its lifetime without a sampled source clock"
    );
}

fn list_mut(chunks: &mut [crate::rifx::Chunk], kind: [u8; 4]) -> &mut Vec<crate::rifx::Chunk> {
    chunks
        .iter_mut()
        .find(|c| c.list_kind() == Some(kind))
        .unwrap()
        .children_mut()
        .unwrap()
}
fn named_mut<'a>(
    chunks: &'a mut [crate::rifx::Chunk],
    name: &str,
    kind: [u8; 4],
) -> &'a mut Vec<crate::rifx::Chunk> {
    let start = chunks
        .iter()
        .position(|c| c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(name.as_bytes()))
        .unwrap();
    let end = chunks[start + 1..]
        .iter()
        .position(|c| c.id() == *b"tdmn")
        .map_or(chunks.len(), |i| start + 1 + i);
    list_mut(&mut chunks[start..end], kind)
}
fn matte_descriptor(layer: &mut Layer) -> &mut Vec<crate::rifx::Chunk> {
    let root = list_mut(&mut layer.content, *b"tdgp");
    let parade = named_mut(root, "ADBE Effect Parade", *b"tdgp");
    named_mut(parade, MATTE, *b"sspc")
}
fn matte_controls(layer: &mut Layer) -> &mut Vec<crate::rifx::Chunk> {
    list_mut(matte_descriptor(layer), *b"tdgp")
}
fn replace_data(chunks: &mut [crate::rifx::Chunk], tag: [u8; 4], bytes: impl Into<Vec<u8>>) {
    let c = chunks.iter_mut().find(|c| c.id() == tag).unwrap();
    *c = crate::rifx::Chunk::data(tag, bytes.into()).unwrap();
}
fn baseline() -> (
    crate::structure::StructuralProject,
    GroupLayer,
    Vec<AnimationGraphEntry>,
) {
    let mut p = project();
    let ItemKind::Composition(c) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind else {
        panic!()
    };
    let leaf = named_mut(
        matte_controls(&mut c.layers[0]),
        "ADBE Set Matte3-0003",
        *b"tdbs",
    );
    replace_data(leaf, *b"cdat", 0_f64.to_be_bytes());
    let converted = crate::structure_document::to_structural_fx_document(&p, Some(1)).unwrap();
    let owner = group(&group(&converted.document.composition().layers()[0]).layers[0]).clone();
    (
        project(),
        owner,
        converted
            .document
            .composition()
            .dynamics()
            .entries()
            .to_vec(),
    )
}
#[test]
fn native_default_table_pins_sparse_alpha_and_composite_assumptions() {
    let defaults = Rifx::parse_with(
        include_bytes!("../../../tests/fixtures/effects/cosmic-set-matte-alpha-defaults.rifx"),
        |_| false,
    )
    .unwrap();
    set_matte::validate_alpha_defaults(defaults.chunks()).unwrap();
    let mut p = project();
    let ItemKind::Composition(c) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind else {
        panic!()
    };
    *list_mut(matte_descriptor(&mut c.layers[0]), *b"parT") = defaults.chunks().to_vec();
    assert_eq!(
        raw_profile(
            &c.layers[0],
            Profile::SelfLayer {
                mask_supported: true
            }
        ),
        Ok(true)
    );
    let table = list_mut(matte_descriptor(&mut c.layers[0]), *b"parT");
    let slot = table.iter_mut().find(|c| c.list_kind() == Some(*b"pard"));
    assert!(slot.is_none());
    // The reused validator rejects a conflicting full table as well as malformed framing.
    table.pop();
    assert!(
        raw_profile(
            &c.layers[0],
            Profile::SelfLayer {
                mask_supported: true
            }
        )
        .is_err()
    );
}
#[test]
fn native_self_inverse_rejects_changed_stage_provider_controls_and_clock() {
    for kind in 0..9 {
        let (p, mut owner, animations) = baseline();
        let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
            panic!()
        };
        let mut layer = comp.layers[0].clone();
        match kind {
            0 => {
                let leaf = named_mut(matte_controls(&mut layer), "ADBE Set Matte3-0001", *b"tdbs");
                replace_data(leaf, *b"tdps", (-1_i32).to_be_bytes());
            }
            1 => {
                let leaf = named_mut(matte_controls(&mut layer), "ADBE Set Matte3-0001", *b"tdbs");
                replace_data(leaf, *b"tdpi", 999_u32.to_be_bytes());
            }
            2 => {
                let controls = matte_controls(&mut layer);
                let start = controls
                    .iter()
                    .position(|c| {
                        c.id() == *b"tdmn"
                            && c.data_payload()
                                .unwrap()
                                .starts_with(b"ADBE Set Matte3-0003")
                    })
                    .unwrap();
                let end = controls[start + 1..]
                    .iter()
                    .position(|c| c.id() == *b"tdmn")
                    .map_or(controls.len(), |i| start + 1 + i);
                let copy = controls[start..end].to_vec();
                controls.extend(copy);
            }
            3 => {
                owner.masks[0].opacity = fx_schema::NonNegativeProperty::new(0.5).unwrap();
            }
            4 => {
                let mut bytes = layer.record.encode();
                bytes[132..136].copy_from_slice(&99_u32.to_be_bytes());
                layer.record = LayerRecord::decode(&bytes).unwrap();
            }
            6 => {
                let mut leaf =
                    named_mut(matte_controls(&mut layer), "ADBE Set Matte3-0003", *b"tdbs").clone();
                replace_data(&mut leaf, *b"cdat", 0.5_f64.to_be_bytes());
                let root = list_mut(&mut layer.content, *b"tdgp");
                let parade = named_mut(root, "ADBE Mask Parade", *b"tdgp");
                let atom = named_mut(parade, "ADBE Mask Atom", *b"tdgp");
                let mut name = b"ADBE Mask Opacity".to_vec();
                name.resize(40, 0);
                atom.push(crate::rifx::Chunk::data(*b"tdmn", name).unwrap());
                atom.push(crate::rifx::Chunk::list(*b"tdbs", leaf));
            }
            7 => {
                let root = list_mut(&mut layer.content, *b"tdgp");
                let parade = named_mut(root, "ADBE Mask Parade", *b"tdgp");
                let atom = named_mut(parade, "ADBE Mask Atom", *b"tdgp");
                let shape = named_mut(atom, "ADBE Mask Shape", *b"om-s");
                let leaf = list_mut(shape, *b"tdbs");
                leaf.push(crate::rifx::Chunk::data(*b"Utf8", b"time".to_vec()).unwrap());
            }
            8 => {
                let leaf =
                    named_mut(matte_controls(&mut layer), "ADBE Set Matte3-0003", *b"tdbs").clone();
                let controls = matte_controls(&mut layer);
                let mut name = b"ADBE Set Matte3-9999".to_vec();
                name.resize(40, 0);
                controls.push(crate::rifx::Chunk::data(*b"tdmn", name).unwrap());
                controls.push(crate::rifx::Chunk::list(*b"tdbs", leaf));
            }
            _ => {
                let options = named_mut(
                    matte_controls(&mut layer),
                    "ADBE Effect Built In Params",
                    *b"tdgp",
                );
                options.push(crate::rifx::Chunk::data(*b"tdmn", b"Private\0".to_vec()).unwrap());
            }
        }
        let before = owner.clone();
        let mut next = 1000;
        assert!(
            apply(
                &layer,
                Context {
                    source: p.item(96),
                    items: &p.items.iter().map(|i| (i.id, i)).collect(),
                    depth: 0
                },
                &mut owner,
                &mut animations.clone(),
                &mut next,
                true,
                &mut super::super::animation_budget::AnimationBudget::default()
            )
            .is_err(),
            "{kind}"
        );
        assert_eq!(owner, before);
        assert_eq!(next, 1000);
    }
}
#[test]
fn staged_helpers_preserve_all_existing_ids_entries_and_rollback_allocations() {
    let (p, baseline, animations) = baseline();
    let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
        panic!()
    };
    let layer = &comp.layers[0];
    for (depth, cursor) in [(MAX_GROUP_DEPTH - 5, 1000), (0, u64::MAX - 2)] {
        let mut owner = baseline.clone();
        let mut next = cursor;
        assert!(
            apply(
                layer,
                Context {
                    source: p.item(96),
                    items: &p.items.iter().map(|i| (i.id, i)).collect(),
                    depth
                },
                &mut owner,
                &mut animations.clone(),
                &mut next,
                true,
                &mut super::super::animation_budget::AnimationBudget::default()
            )
            .is_err()
        );
        assert_eq!(owner, baseline);
        assert_eq!(next, cursor);
    }
    let mut owner = baseline.clone();
    let mut next = 1000;
    assert_eq!(
        apply(
            layer,
            Context {
                source: p.item(96),
                items: &p.items.iter().map(|i| (i.id, i)).collect(),
                depth: 0
            },
            &mut owner,
            &mut animations.clone(),
            &mut next,
            true,
            &mut super::super::animation_budget::AnimationBudget::default()
        ),
        Ok(true)
    );
    assert_eq!(next, 1003);
    let shadows = group(&owner.layers[0]);
    let masked = group(&shadows.layers[0]);
    let mut restored = owner.clone();
    let mut source = group(&masked.layers[0]).clone();
    source.parent = Some(owner.id);
    restored.layers = stored_layers(vec![
        LayerData::Group(source),
        baseline.layers[1].data().clone(),
    ])
    .unwrap();
    restored.effects = shadows
        .effects
        .iter()
        .chain(owner.effects.iter())
        .cloned()
        .collect();
    restored.masks = masked.masks.clone();
    assert_eq!(
        restored, baseline,
        "reverse staging must recover every original field and typed ID"
    );
    // Applying changes no graph entries or tracks; every original target still exists.
    assert!(!animations.is_empty());
    let mut ids = HashSet::new();
    fn collect(l: &LayerData, ids: &mut HashSet<u64>) {
        ids.insert(u64::from(l.id()));
        if let Some(c) = l.child_layers() {
            for l in c {
                collect(l.data(), ids);
            }
        }
    }
    collect(&LayerData::Group(owner), &mut ids);
    assert!(
        animations
            .iter()
            .filter_map(|e| e.target.layer_id())
            .all(|id| ids.contains(&u64::from(id)))
    );
}

#[test]
fn native_track_matte_consumer_samples_the_same_self_inverse_all_effects_stage() {
    let mut p = project();
    let ItemKind::Composition(comp) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    let mut consumer = comp.layers[0].clone();
    let mut bytes = consumer.record.encode();
    bytes[..4].copy_from_slice(&949_u32.to_be_bytes());
    consumer.record = LayerRecord::decode(&bytes)
        .unwrap()
        .with_export_options(true, false, 2, 0, 948, 1)
        .unwrap();
    consumer.content.clear();
    consumer.name = "Independent matte consumer".into();
    comp.layers.insert(0, consumer);
    let converted = crate::structure_document::to_structural_fx_document(&p, Some(1)).unwrap();
    let root = group(&converted.document.composition().layers()[0]);
    let consumer = group(&root.layers[0]);
    let sampled_id = consumer.track_matte.as_ref().unwrap().layer;
    let sampled = group(root.layers.iter().find(|l| l.id() == sampled_id).unwrap());
    let ordinary = group(&root.layers[1]);
    for owner in [ordinary, sampled] {
        assert!(owner.masks[0].inverted, "{:?}", converted.diagnostics);
        let shadows = group(&owner.layers[0]);
        assert_eq!(shadows.name, "Self-inverse matte shadow stage");
        let masked = group(&shadows.layers[0]);
        assert!(shadows.masks.is_empty());
        assert!(masked.effects.is_empty());
        assert_eq!(owner.masks[0].layer, masked.masks[0].layer);
        assert!(!masked.masks[0].inverted);
    }
    assert_ne!(ordinary.id, sampled.id);
}

fn post_transform_project() -> crate::structure::StructuralProject {
    let mut project = project();
    let parsed = Rifx::parse_with(
        include_bytes!("../../../tests/fixtures/effects/self-inverse-post-transform-controls.rifx"),
        |_| false,
    )
    .unwrap();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    let template = comp.layers[0].clone();
    comp.layers = parsed
        .chunks()
        .iter()
        .filter(|c| c.list_kind() == Some(*b"Layr"))
        .map(|chunk| {
            let mut layer = template.clone();
            layer.content = chunk.children().unwrap().to_vec();
            layer.record =
                LayerRecord::decode(properties::data(&layer.content, *b"ldta").unwrap()).unwrap();
            layer.name = format!("Independent matte {}", layer.record.id()).into();
            layer
        })
        .collect();
    let table = parsed
        .chunks()
        .iter()
        .find(|c| c.list_kind() == Some(*b"parT"))
        .unwrap()
        .children()
        .unwrap();
    // The consumer keeps its native sparse table; the other same-project instance
    // carries the exact native legacy declarations from the pinned source.
    let mut defaults = project.item(1).unwrap().clone();
    defaults.id = 2;
    let ItemKind::Composition(comp) = &mut defaults.kind else {
        panic!()
    };
    let root = list_mut(&mut comp.layers[0].content, *b"tdgp");
    let parade = named_mut(root, "ADBE Effect Parade", *b"tdgp");
    let descriptor = named_mut(parade, "ADBE Geometry2", *b"sspc");
    *list_mut(descriptor, *b"parT") = table.to_vec();
    project.items.push(defaults);
    project
}

#[test]
fn native_equivalent_foreign_mask_post_wipe_transform_preserves_stages_and_source_center() {
    let conversion =
        crate::structure_document::to_structural_fx_document(&post_transform_project(), Some(1))
            .unwrap();
    let owner = group(&group(&conversion.document.composition().layers()[0]).layers[0]);
    let rotation = group(&owner.layers[0]);
    assert_eq!(
        rotation.transform.rotation, 180.,
        "{:?}",
        conversion.diagnostics
    );
    assert_eq!(rotation.transform.anchor_point, [1920., 1080.]);
    assert_eq!(
        rotation.transform.position,
        fx_schema::Position::xy(1920., 1080.)
    );
    assert_eq!(rotation.effects.len(), 1);
    assert!(matches!(
        known(&rotation.effects[0]),
        Ok(LayerEffect::Glow { .. })
    ));
    assert_eq!(owner.effects.len(), 1);
    assert!(matches!(
        known(&owner.effects[0]),
        Ok(LayerEffect::InnerShadow { .. })
    ));
    let shadows = group(&rotation.layers[0]);
    assert_eq!(shadows.effects.len(), 2);
    assert!(shadows.masks.is_empty());
    let masked = group(&shadows.layers[0]);
    assert!(masked.effects.is_empty());
    assert!(!masked.masks[0].inverted);
    assert!(rotation.masks[0].inverted);
    assert_eq!(rotation.masks[0].layer, masked.masks[0].layer);
    assert_eq!(rotation.layers[1].id(), rotation.masks[0].layer.unwrap());
    let content = group(&masked.layers[0]);
    assert_eq!(content.playback.input_range().start.as_millis(), 750);
    assert!(
        content.playback.time_remap().is_none(),
        "constant Solid content retains its lifetime without a sampled source clock"
    );
    assert!(
        !conversion
            .document
            .composition()
            .dynamics()
            .entries()
            .is_empty()
    );
}

#[test]
fn native_post_effect_half_turn_rotates_screen_space_shadow_offsets_and_keys() {
    let converted =
        crate::structure_document::to_structural_fx_document(&post_transform_project(), Some(1))
            .unwrap();
    let owner = group(&group(&converted.document.composition().layers()[0]).layers[0]);
    let shadows = group(&group(&owner.layers[0]).layers[0]);
    for (record, distances) in shadows.effects.iter().zip([[2., 4., 8.], [3., 6., 20.]]) {
        let EffectData::Identified {
            id,
            effect: EffectPayload::Known(LayerEffect::DropShadow(shadow)),
            ..
        } = record.data()
        else {
            panic!()
        };
        assert_eq!(shadow.offset, [0., distances[0]]);
        let target = fx_schema::PropertyTarget::effect_param(*id, "offset");
        let entry = converted
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|e| e.target == target)
            .unwrap();
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
        else {
            panic!()
        };
        for (key, distance) in track.keyframes().iter().zip(distances) {
            assert_eq!(
                *key.value(),
                fx_schema::PropertyValue::Vector2([0., distance])
            );
        }
    }
}

#[test]
fn equivalent_foreign_mask_uses_native_references_and_source_bounds_after_renaming() {
    let mut p = post_transform_project();
    for item in &mut p.items {
        if item.id == 96 {
            item.id = 17;
            item.name = "Different solid name".into();
            let solid = item.solid.as_mut().unwrap().as_mut().unwrap();
            solid.width = 1280;
            solid.height = 720;
        }
        if let ItemKind::Composition(comp) = &mut item.kind {
            comp.width = 1280;
            comp.height = 720;
            for layer in &mut comp.layers {
                let mut bytes = layer.record.encode();
                let id = if layer.record.id() == 948 {
                    5001_u32
                } else {
                    5002_u32
                };
                bytes[..4].copy_from_slice(&id.to_be_bytes());
                bytes[40..44].copy_from_slice(&17_u32.to_be_bytes());
                layer.record = LayerRecord::decode(&bytes).unwrap();
                layer.name = format!("Changed title {id}").into();
                let leaf = named_mut(matte_controls(layer), "ADBE Set Matte3-0001", *b"tdbs");
                replace_data(leaf, *b"tdpi", 5001_u32.to_be_bytes());
            }
        }
    }
    let converted = crate::structure_document::to_structural_fx_document(&p, Some(1)).unwrap();
    let owner = group(&group(&converted.document.composition().layers()[0]).layers[0]);
    let rotation = group(&owner.layers[0]);
    assert_eq!(
        rotation.transform.rotation, 180.,
        "{:?}",
        converted.diagnostics
    );
    assert_eq!(rotation.transform.anchor_point, [640., 360.]);
    assert_eq!(
        rotation.transform.position,
        fx_schema::Position::xy(640., 360.)
    );
    assert!(rotation.masks[0].inverted);
}

fn rotation_control(layer: &mut Layer) -> &mut Vec<Chunk> {
    let root = list_mut(&mut layer.content, *b"tdgp");
    let parade = named_mut(root, "ADBE Effect Parade", *b"tdgp");
    let descriptor = named_mut(parade, "ADBE Geometry2", *b"sspc");
    named_mut(
        list_mut(descriptor, *b"tdgp"),
        "ADBE Geometry2-0007",
        *b"tdbs",
    )
}

#[test]
fn equivalent_foreign_mask_rejects_conflicting_paths_defaults_order_and_budget() {
    let mut p = post_transform_project();
    let ItemKind::Composition(comp) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    replace_data(
        rotation_control(&mut comp.layers[0]),
        *b"cdat",
        179_f64.to_be_bytes(),
    );
    let converted = crate::structure_document::to_structural_fx_document(&p, Some(1)).unwrap();
    let original = group(&group(&converted.document.composition().layers()[0]).layers[0]).clone();
    let animations = converted.document.composition().dynamics().entries();
    for case in 0..6 {
        let mut p = post_transform_project();
        if case == 0 {
            p.items.retain(|i| i.id != 2);
        }
        let ItemKind::Composition(comp) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        if case == 1 {
            let root = list_mut(&mut comp.layers[0].content, *b"tdgp");
            let parade = named_mut(root, "ADBE Mask Parade", *b"tdgp");
            let atom = named_mut(parade, "ADBE Mask Atom", *b"tdgp");
            let outline = named_mut(atom, "ADBE Mask Shape", *b"om-s");
            let shape = list_mut(list_mut(outline, *b"omks"), *b"shap");
            let mut bytes = properties::data(shape, *b"shph").unwrap().to_vec();
            bytes[4..8].copy_from_slice(&0.125_f32.to_be_bytes());
            replace_data(shape, *b"shph", bytes);
        }
        if case == 2 {
            replace_data(
                rotation_control(&mut comp.layers[0]),
                *b"cdat",
                179_f64.to_be_bytes(),
            );
        }
        if case == 3 {
            let root = list_mut(&mut comp.layers[0].content, *b"tdgp");
            let parade = named_mut(root, "ADBE Effect Parade", *b"tdgp");
            let start = parade
                .iter()
                .position(|c| {
                    c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(b"ADBE Geometry2")
                })
                .unwrap();
            let end = parade[start + 1..]
                .iter()
                .position(|c| c.id() == *b"tdmn")
                .map_or(parade.len(), |i| start + 1 + i);
            let transform: Vec<_> = parade.drain(start..end).collect();
            let before_wipes = parade
                .iter()
                .position(|c| {
                    c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(b"ADBE Linear Wipe")
                })
                .unwrap();
            parade.splice(before_wipes..before_wipes, transform);
        }
        let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
            panic!()
        };
        let items = p.items.iter().map(|i| (i.id, i)).collect();
        let mut owner = original.clone();
        let mut next = if case == 4 { u64::MAX - 2 } else { 1000 };
        let initial_next = next;
        let mut budget = super::super::animation_budget::AnimationBudget::default();
        let result = super::super::foreign_inverse_matte::apply(
            super::super::foreign_inverse_matte::Context {
                comp,
                items: &items,
                depth: if case == 5 { MAX_GROUP_DEPTH - 6 } else { 0 },
            },
            &comp.layers[0],
            &mut owner,
            &mut animations.to_vec(),
            &mut next,
            &mut budget,
        );
        assert!(result.is_err(), "case {case}: {result:?}");
        assert_eq!(owner, original);
        assert_eq!(next, initial_next);
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn native_fixed_direction_shadow_distance_becomes_editable_vector2_keys() {
    use fx_schema::{PropertyTarget, PropertyValue};
    let p = project();
    let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
        panic!()
    };
    let layer = &comp.layers[0];
    let imported = super::super::effects::import(
        &Default::default(),
        1,
        layer,
        [3840, 2160],
        [3840, 2160],
        &mut 1000,
        &mut super::super::animation_budget::AnimationBudget::default(),
    );
    let clock = super::super::animation::NumericAnimationClock::parent_identity(layer).unwrap();
    for (effect, distances) in imported
        .effects
        .iter()
        .take(2)
        .zip([[2., 4., 8.], [3., 6., 20.]])
    {
        let EffectData::Identified { id, .. } = effect.data() else {
            panic!()
        };
        let target = PropertyTarget::effect_param(*id, "offset");
        let entry = imported
            .animations
            .iter()
            .find(|e| e.target == target)
            .unwrap_or_else(|| panic!("distance animation missing: {:?}", imported.warnings));
        let keys = entry.animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.len(), 3);
        for (index, (key, distance)) in keys.iter().zip(distances).enumerate() {
            assert_eq!(key.value(), &PropertyValue::Vector2([0., -distance]));
            let local = index as f64 / 12.;
            assert_eq!(
                key.layer_time().as_millis(),
                (clock.seconds(local) * 1000.).round() as i64
            );
        }
    }
    assert!(
        !imported
            .warnings
            .iter()
            .any(|w| w.contains("-0004: only the initial value"))
    );
}

#[test]
fn generic_fixed_shadow_angle_preserves_scalar_ease_in_vector2_projection() {
    use super::super::animation::{self, NumericAnimationClock, NumericAnimationTarget};
    use super::super::animation_budget::AnimationBudget;
    use fx_schema::{PropertyTarget, PropertyValue};
    let p = project();
    let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
        panic!()
    };
    let mut layer = comp.layers[0].clone();
    layer.name = "Different shadow carrier".into();
    let root = list_mut(&mut layer.content, *b"tdgp");
    let parade = named_mut(root, "ADBE Effect Parade", *b"tdgp");
    let descriptor = named_mut(parade, "ADBE Drop Shadow", *b"sspc");
    let controls = list_mut(descriptor, *b"tdgp");
    replace_data(
        named_mut(controls, "ADBE Drop Shadow-0003", *b"tdbs"),
        *b"cdat",
        45_f64.to_be_bytes(),
    );
    let imported = super::super::effects::import(
        &Default::default(),
        1,
        &layer,
        [3840, 2160],
        [3840, 2160],
        &mut 2222,
        &mut AnimationBudget::default(),
    );
    let EffectData::Identified { id, .. } = imported.effects[0].data() else {
        panic!()
    };
    let target = PropertyTarget::effect_param(*id, "offset");
    let entry = imported
        .animations
        .iter()
        .find(|e| e.target == target)
        .unwrap();
    let (native, _) = crate::effects::native::read_effects(&layer.content, [3840., 2160.]);
    let distance = native[0]
        .parameters
        .iter()
        .find(|p| p.match_name == "ADBE Drop Shadow-0004")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    let (scalar, warnings) = animation::numeric_entries(
        "Distance",
        distance,
        &[NumericAnimationTarget::float(
            PropertyTarget::effect_param(*id, "probe"),
            0,
            1.,
        )],
        NumericAnimationClock::parent_identity(&layer).unwrap(),
        &mut AnimationBudget::default(),
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    for (vector, scalar) in entry
        .animator
        .keyframe_track()
        .unwrap()
        .keyframes()
        .iter()
        .zip(scalar[0].animator.keyframe_track().unwrap().keyframes())
    {
        let PropertyValue::Float(distance) = scalar.value() else {
            panic!()
        };
        let PropertyValue::Vector2(offset) = vector.value() else {
            panic!()
        };
        assert!((offset[0] - distance * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-10);
        assert!((offset[1] + distance * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-10);
        assert_eq!(vector.easing(), scalar.easing());
        assert_eq!(vector.layer_time(), scalar.layer_time());
    }
}
