use super::super::inverse_matte_transform::descriptor;
use super::*;
use crate::{
    rifx::Rifx,
    schema::layer_records::LayerRecord,
    structure::{ItemKind, SolidSource, read_project},
};
const FIXTURE: &[u8] =
    include_bytes!("../../../tests/fixtures/effects/cosmic-foreign-mask-controls.rifx");
fn project() -> crate::structure::StructuralProject {
    let mut p = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let parsed = Rifx::parse_with(FIXTURE, |_| false).unwrap();
    let ItemKind::Composition(comp) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    let template = comp.layers[0].clone();
    comp.width = 3840;
    comp.height = 2160;
    comp.duration_secs = 6.;
    comp.layers = parsed
        .chunks()
        .iter()
        .filter(|c| c.list_kind() == Some(*b"Layr"))
        .map(|c| {
            let mut l = template.clone();
            l.content = c.children().unwrap().to_vec();
            l.record =
                LayerRecord::decode(properties::data(&l.content, *b"ldta").unwrap()).unwrap();
            l.name = std::str::from_utf8(properties::data(&l.content, *b"Utf8").unwrap())
                .unwrap()
                .trim_end_matches('\0')
                .into();
            l
        })
        .collect();
    let table = parsed
        .chunks()
        .iter()
        .find(|c| c.list_kind() == Some(*b"parT"))
        .unwrap()
        .children()
        .unwrap();
    let own = descriptor(&comp.layers[0]).unwrap().unwrap();
    assert!(properties::unique_list(own, *b"parT").unwrap().is_empty());
    // Supplementary unused composition holds the exact same-project declaration;
    // both native consumers retain their original sparse controls.
    let mut defaults = p.items.iter().find(|i| i.id == 1).unwrap().clone();
    defaults.id = 2;
    let ItemKind::Composition(definition_comp) = &mut defaults.kind else {
        panic!()
    };
    definition_comp.layers.truncate(1);
    replace_table(&mut definition_comp.layers[0], table.to_vec());
    p.items.push(defaults);
    let mut source = p.items[0].clone();
    source.id = 96;
    source.kind = ItemKind::Footage;
    source.solid = Some(Ok(SolidSource {
        width: 3840,
        height: 2160,
        pixel_aspect: (1, 1),
        color: [1., 1., 1.],
    }));
    source.media = None;
    p.items.push(source);
    p
}
fn replace_table(layer: &mut Layer, table: Vec<Chunk>) {
    let root = list_mut(&mut layer.content, *b"tdgp").unwrap();
    let parade = named_list_mut(root, "ADBE Effect Parade", *b"tdgp").unwrap();
    let descriptor = named_list_mut(parade, "ADBE Geometry2", *b"sspc").unwrap();
    *list_mut(descriptor, *b"parT").unwrap() = table;
}
fn group(l: &fx_schema::Layer) -> &GroupLayer {
    let LayerData::Group(g) = l.data() else {
        panic!()
    };
    g
}
#[test]
fn native_foreign_alias_copies_keys_and_preserves_destination_clock() {
    let conversion =
        crate::structure_document::to_structural_fx_document(&project(), Some(1)).unwrap();
    let root = group(&conversion.document.composition().layers()[0]);
    let owner = group(&root.layers[0]);
    let rotation = group(&owner.layers[0]);
    assert_eq!(
        rotation.name, "Foreign matte post-Glow Transform",
        "{:?}",
        conversion.diagnostics
    );
    assert_eq!(rotation.transform.anchor_point, [1920., 1080.]);
    assert_eq!(rotation.transform.rotation, 180.);
    assert!(matches!(
        owner.effects[0].data(),
        fx_schema::EffectData::Identified {
            effect: fx_schema::EffectPayload::Known(fx_schema::LayerEffect::InnerShadow { .. }),
            ..
        }
    ));
    let shadows = group(&rotation.layers[0]);
    let masked = group(&shadows.layers[0]);
    let content = group(&masked.layers[0]);
    assert_eq!(content.playback.input_range().start.as_millis(), 4000);
    assert_eq!(content.playback.input_range().duration.as_millis(), 583);
    assert!(content.playback.time_remap().is_some());
    assert!(masked.effects.is_empty());
    assert!(shadows.masks.is_empty());
    assert!(rotation.masks[0].inverted);
    assert_eq!(rotation.masks[0].layer, masked.masks[0].layer);
    let guide = rotation.layers[1].id();
    let entries = conversion.document.composition().dynamics().entries();
    assert!(entries.iter().any(|e| e.target.layer_id() == Some(guide)));
}
#[test]
fn repeated_sparse_transform_instances_do_not_ambiguate_project_defaults() {
    let mut p = project();
    let mut unrelated = composition(&mut p, 1).layers[0].clone();
    let root = list_mut(&mut unrelated.content, *b"tdgp").unwrap();
    let parade = named_list_mut(root, "ADBE Effect Parade", *b"tdgp").unwrap();
    let start = parade
        .iter()
        .position(|c| {
            c.id() == *b"tdmn"
                && c.data_payload()
                    .is_some_and(|v| v.starts_with(b"ADBE Geometry2\0"))
        })
        .unwrap();
    let end = parade[start + 1..]
        .iter()
        .position(|c| c.id() == *b"tdmn")
        .map_or(parade.len(), |i| start + 1 + i);
    let duplicate = parade[start..end].to_vec();
    parade.extend(duplicate);
    assert!(
        descriptor(&unrelated).is_err(),
        "consumer selector must remain unique"
    );
    composition(&mut p, 2).layers.push(unrelated);
    let conversion = crate::structure_document::to_structural_fx_document(&p, Some(1)).unwrap();
    assert_eq!(
        group(&group(&group(&conversion.document.composition().layers()[0]).layers[0]).layers[0])
            .name,
        "Foreign matte post-Glow Transform",
        "{:?}",
        conversion.diagnostics
    );
}
fn composition(p: &mut crate::structure::StructuralProject, id: u32) -> &mut Composition {
    let ItemKind::Composition(c) = &mut p.items.iter_mut().find(|i| i.id == id).unwrap().kind
    else {
        panic!()
    };
    c
}
fn effect_control<'a>(layer: &'a mut Layer, effect: &str, control: &str) -> &'a mut Vec<Chunk> {
    let root = list_mut(&mut layer.content, *b"tdgp").unwrap();
    let parade = named_list_mut(root, "ADBE Effect Parade", *b"tdgp").unwrap();
    let descriptor = named_list_mut(parade, effect, *b"sspc").unwrap();
    let controls = list_mut(descriptor, *b"tdgp").unwrap();
    named_list_mut(controls, control, *b"tdbs").unwrap()
}
fn leaf(layer: &mut Layer) -> &mut Vec<Chunk> {
    let root = list_mut(&mut layer.content, *b"tdgp").unwrap();
    let parade = named_list_mut(root, "ADBE Mask Parade", *b"tdgp").unwrap();
    let atom = named_list_mut(parade, "ADBE Mask Atom", *b"tdgp").unwrap();
    let outline = named_list_mut(atom, "ADBE Mask Shape", *b"om-s").unwrap();
    list_mut(outline, *b"tdbs").unwrap()
}
fn replace_data(chunks: &mut Vec<Chunk>, tag: [u8; 4], bytes: impl Into<Vec<u8>>) {
    let chunk = Chunk::data(tag, bytes.into()).unwrap();
    if let Some(index) = chunks.iter().position(|c| c.id() == tag) {
        chunks[index] = chunk;
    } else {
        chunks.push(chunk);
    }
}
fn baseline() -> (GroupLayer, Vec<AnimationGraphEntry>) {
    let mut p = project();
    replace_data(
        effect_control(
            &mut composition(&mut p, 1).layers[0],
            "ADBE Geometry2",
            "ADBE Geometry2-0007",
        ),
        *b"cdat",
        179_f64.to_be_bytes(),
    );
    let converted = crate::structure_document::to_structural_fx_document(&p, Some(1)).unwrap();
    (
        group(&group(&converted.document.composition().layers()[0]).layers[0]).clone(),
        converted
            .document
            .composition()
            .dynamics()
            .entries()
            .to_vec(),
    )
}
#[test]
fn copied_path_has_exact_native_values_times_easing_and_independent_ids() {
    let converted =
        crate::structure_document::to_structural_fx_document(&project(), Some(1)).unwrap();
    let root = group(&converted.document.composition().layers()[0]);
    let destination = group(&group(&root.layers[0]).layers[0]).layers[1].id();
    let provider = group(&root.layers[1]).layers[1].id();
    let entries = converted.document.composition().dynamics().entries();
    let find = |id| {
        serde_json::to_value(
            entries
                .iter()
                .find(|e| e.target.layer_id() == Some(id))
                .unwrap(),
        )
        .unwrap()
    };
    let d = find(destination);
    let p = find(provider);
    let d = d["animator"]["keyframes"].as_array().unwrap();
    let p = p["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(d.len(), 8);
    assert_eq!(d.len(), p.len());
    for (d, p) in d.iter().zip(p) {
        assert_ne!(d["id"], p["id"]);
        for name in ["layerTime", "value", "easing"] {
            assert_eq!(d[name], p[name]);
        }
    }
}
#[test]
fn foreign_alias_guards_fail_without_mutating_cached_owner_ids_or_budget() {
    let (original, animations) = baseline();
    for case in 0..11 {
        let mut p = project();
        let c = composition(&mut p, 1);
        match case {
            0 => {
                let duplicate = c.layers[1].clone();
                c.layers.push(duplicate);
            }
            1 => {
                c.layers[1].name = "Unreferenced provider".into();
            }
            2 => replace_data(
                leaf(&mut c.layers[0]),
                *b"Utf8",
                b"thisComp.layer(\"door 3\").mask(\"Absent\").maskPath".to_vec(),
            ),
            3 => replace_data(
                effect_control(&mut c.layers[0], "ADBE Set Matte3", "ADBE Set Matte3-0001"),
                *b"tdpi",
                999_u32.to_be_bytes(),
            ),
            4 => replace_data(
                leaf(&mut c.layers[0]),
                *b"Utf8",
                b"thisComp.layer(\"door 3\").mask(\"Mask 1\").maskPath+1".to_vec(),
            ),
            5 => replace_data(
                leaf(&mut c.layers[1]),
                *b"Utf8",
                b"thisComp.layer(\"door 2\").mask(\"Mask 1\").maskPath".to_vec(),
            ),
            6 => {
                c.layers[1].record = c.layers[1]
                    .record
                    .clone()
                    .with_active_range(1, 24576)
                    .unwrap();
            }
            7 => replace_data(
                effect_control(&mut c.layers[0], "ADBE Geometry2", "ADBE Geometry2-0007"),
                *b"cdat",
                179_f64.to_be_bytes(),
            ),
            8 => {
                let mut raw = c.layers[1].record.encode();
                raw[40..44].copy_from_slice(&97_u32.to_be_bytes());
                c.layers[1].record = LayerRecord::decode(&raw).unwrap();
            }
            9 => {
                let duplicate = properties::data(leaf(&mut c.layers[0]), *b"Utf8")
                    .unwrap()
                    .to_vec();
                leaf(&mut c.layers[0]).push(Chunk::data(*b"Utf8", duplicate).unwrap());
            }
            10 => {
                let controls =
                    effect_control(&mut c.layers[0], "ADBE Geometry2", "ADBE Geometry2-0007");
                let value = properties::data(controls, *b"cdat").unwrap().to_vec();
                controls.push(Chunk::data(*b"cdat", value).unwrap());
            }
            _ => unreachable!(),
        }
        let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
            panic!()
        };
        let items = p.items.iter().map(|i| (i.id, i)).collect();
        let mut owner = original.clone();
        let mut next_id = 1000;
        let mut budget = AnimationBudget::default();
        budget.reserve(7).unwrap();
        assert!(
            apply(
                Context {
                    comp,
                    items: &items,
                    depth: 0
                },
                &comp.layers[0],
                &mut owner,
                &mut animations.clone(),
                &mut next_id,
                &mut budget
            )
            .is_err(),
            "case{case}"
        );
        assert_eq!(owner, original);
        assert_eq!(next_id, 1000);
        assert_eq!(budget.used(), 7);
    }
}
#[test]
fn modern_or_conflicting_defaults_and_explicit_points_do_not_use_legacy_center() {
    let (original, animations) = baseline();
    for case in 0..3 {
        let mut p = project();
        if case < 2 {
            let layer = &mut composition(&mut p, 2).layers[0];
            let mut table = properties::unique_list(descriptor(layer).unwrap().unwrap(), *b"parT")
                .unwrap()
                .to_vec();
            let start = table
                .iter()
                .position(|c| {
                    c.id() == *b"tdmn"
                        && c.data_payload()
                            .unwrap()
                            .starts_with(b"ADBE Geometry2-0001")
                })
                .unwrap();
            let index = start
                + 1
                + table[start + 1..]
                    .iter()
                    .position(|c| c.id() == *b"pard")
                    .unwrap();
            let mut bytes = table[index].data_payload().unwrap().to_vec();
            for offset in [56, 60, 68, 72] {
                bytes[offset..offset + 4].copy_from_slice(&32768_u32.to_be_bytes());
            }
            table[index] = Chunk::data(*b"pard", bytes).unwrap();
            replace_table(layer, table);
            if case == 1 {
                let mut other = p.item(2).unwrap().clone();
                other.id = 3;
                p.items.push(other);
                let native = Rifx::parse_with(FIXTURE, |_| false).unwrap();
                let table = native
                    .chunks()
                    .iter()
                    .find(|c| c.list_kind() == Some(*b"parT"))
                    .unwrap()
                    .children()
                    .unwrap()
                    .to_vec();
                replace_table(&mut composition(&mut p, 2).layers[0], table);
            }
        } else {
            let layer = &mut composition(&mut p, 1).layers[0];
            let root = list_mut(&mut layer.content, *b"tdgp").unwrap();
            let parade = named_list_mut(root, "ADBE Effect Parade", *b"tdgp").unwrap();
            let descriptor = named_list_mut(parade, "ADBE Geometry2", *b"sspc").unwrap();
            let controls = list_mut(descriptor, *b"tdgp").unwrap();
            controls.push(Chunk::data(*b"tdmn", b"ADBE Geometry2-0001\0".to_vec()).unwrap());
            controls.push(Chunk::list(*b"tdbs", vec![]));
        }
        let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
            panic!()
        };
        let items = p.items.iter().map(|i| (i.id, i)).collect();
        let mut owner = original.clone();
        let mut id = 1000;
        let mut budget = AnimationBudget::default();
        assert!(
            apply(
                Context {
                    comp,
                    items: &items,
                    depth: 0
                },
                &comp.layers[0],
                &mut owner,
                &mut animations.clone(),
                &mut id,
                &mut budget
            )
            .is_err()
        );
        assert_eq!(owner, original);
        assert_eq!(id, 1000);
        assert_eq!(budget.used(), 0);
    }
}
#[test]
fn foreign_alias_depth_identity_and_animation_exhaustion_rollback() {
    let p = project();
    let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
        panic!()
    };
    let items = p.items.iter().map(|i| (i.id, i)).collect();
    let (original, animations) = baseline();
    for case in 0..3 {
        let mut owner = original.clone();
        let mut id = if case == 1 { u64::MAX - 2 } else { 1000 };
        let initial = id;
        let mut budget = if case == 2 {
            AnimationBudget::with_limit(7)
        } else {
            AnimationBudget::default()
        };
        budget.reserve(7).unwrap();
        assert!(
            apply(
                Context {
                    comp,
                    items: &items,
                    depth: if case == 0 {
                        super::super::MAX_GROUP_DEPTH - 6
                    } else {
                        0
                    }
                },
                &comp.layers[0],
                &mut owner,
                &mut animations.clone(),
                &mut id,
                &mut budget
            )
            .is_err()
        );
        assert_eq!(owner, original);
        assert_eq!(id, initial);
        assert_eq!(budget.used(), 7);
    }
}
#[test]
fn all_effects_matte_provider_uses_the_same_foreign_stage() {
    let mut p = project();
    let c = composition(&mut p, 1);
    let mut consumer = c.layers[0].clone();
    let mut bytes = consumer.record.encode();
    bytes[..4].copy_from_slice(&2000_u32.to_be_bytes());
    consumer.record = LayerRecord::decode(&bytes)
        .unwrap()
        .with_export_options(true, false, 2, 0, 1037, 1)
        .unwrap();
    consumer.content.clear();
    consumer.name = "Independent consumer".into();
    c.layers.insert(0, consumer);
    let converted = crate::structure_document::to_structural_fx_document(&p, Some(1)).unwrap();
    let root = group(&converted.document.composition().layers()[0]);
    let consumer = group(&root.layers[0]);
    let sampled = consumer.track_matte.as_ref().unwrap().layer;
    let sample = group(root.layers.iter().find(|l| l.id() == sampled).unwrap());
    let visible = group(&root.layers[1]);
    for owner in [sample, visible] {
        let rotation = group(&owner.layers[0]);
        assert_eq!(rotation.name, "Foreign matte post-Glow Transform");
        assert!(rotation.masks[0].inverted);
    }
}

#[test]
#[ignore = "requires private original source via BONSA_COSMIC_AEP; no Adobe invocation"]
fn pinned_bonsa_foreign_mask_instances_are_admitted() {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(std::env::var("BONSA_COSMIC_AEP").expect("private source path")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "6d632ac99c9e081746d51b83a651cb311dc063f0541bbe43ba1a241bd8870fdd"
    );
    let project = read_project(&bytes).unwrap();
    let conversion = crate::structure_document::to_structural_fx_document_with_assets(
        &project,
        Some(883),
        &mut |_| true,
    )
    .unwrap();
    fn count(layer: &fx_schema::Layer) -> usize {
        let LayerData::Group(g) = layer.data() else {
            return 0;
        };
        usize::from(g.name == "Foreign matte post-Glow Transform")
            + g.layers.iter().map(count).sum::<usize>()
    }
    assert_eq!(
        conversion
            .document
            .composition()
            .layers()
            .iter()
            .map(count)
            .sum::<usize>(),
        4,
        "{:?}",
        conversion
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("Foreign"))
            .collect::<Vec<_>>()
    );
}
