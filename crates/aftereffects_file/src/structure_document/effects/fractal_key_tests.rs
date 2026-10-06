use super::*;
use crate::{
    effects::fractal_noise,
    rifx::Chunk,
    structure::{ItemKind, read_project},
};

fn update_leaf(chunks: &mut [Chunk], name: &str, edit: &impl Fn(&mut Vec<Chunk>)) -> bool {
    for index in 0..chunks.len() {
        if chunks[index].id() == *b"tdmn"
            && chunks[index]
                .data_payload()
                .is_some_and(|data| data.split(|b| *b == 0).next() == Some(name.as_bytes()))
        {
            let end = chunks[index + 1..]
                .iter()
                .position(|chunk| chunk.id() == *b"tdmn")
                .map_or(chunks.len(), |offset| index + 1 + offset);
            // parT declarations repeat the name but own pard, not numeric metadata.
            if let Some(leaf) = chunks[index + 1..end]
                .iter_mut()
                .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
                .and_then(Chunk::children_mut)
            {
                edit(leaf);
                return true;
            }
        }
        if let Some(children) = chunks[index].children_mut()
            && update_leaf(children, name, edit)
        {
            return true;
        }
    }
    false
}

// Like the static Fractal fixture helper, materialize only an omitted scalar.
// The template is independently static: never copy the animated source Contrast.
fn derive_fractal_blend(layer: &mut crate::structure::Layer, blend: f64) {
    fn instance(chunks: &mut [Chunk]) -> Option<&mut Vec<Chunk>> {
        for index in 0..chunks.len() {
            if chunks[index].id() == *b"tdmn"
                && chunks[index].data_payload().is_some_and(|data| {
                    data.split(|b| *b == 0).next() == Some(b"ADBE Fractal Noise".as_slice())
                })
            {
                let end = chunks[index + 1..]
                    .iter()
                    .position(|chunk| chunk.id() == *b"tdmn")
                    .map_or(chunks.len(), |offset| index + 1 + offset);
                if let Some(descriptor) =
                    (index + 1..end).find(|i| chunks[*i].list_kind() == Some(*b"sspc"))
                {
                    return chunks[descriptor]
                        .children_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                        .and_then(Chunk::children_mut);
                }
            }
        }
        for chunk in chunks {
            if let Some(children) = chunk.children_mut()
                && let Some(body) = instance(children)
            {
                return Some(body);
            }
        }
        None
    }
    assert!(matches!(blend, 5. | 6.));
    let source = native::read_effects(&layer.content, [320., 180.])
        .0
        .remove(0);
    assert_eq!(source.match_name, fractal_noise::MATCH_NAME);
    // Omitted Blend resolves to Normal and must remain a positive import control.
    assert!(matches!(
        fractal_noise::lower(&source, layer, [320, 180], true)
            .unwrap()
            .0,
        LayerEffect::TurbulentNoise { .. }
    ));
    let body = instance(&mut layer.content).unwrap();
    assert!(
        !crate::properties::runs(body)
            .unwrap()
            .iter()
            .any(|(name, _)| *name == "ADBE Fractal Noise-0030")
    );
    let original = body.clone();
    let fixture = crate::rifx::Rifx::parse_with(
        include_bytes!("../../../tests/fixtures/effects/native-fractal-noise-controls.rifx"),
        |_| false,
    )
    .unwrap();
    let mut template_chunks = fixture.chunks()[0].children().unwrap().to_vec();
    let template = instance(&mut template_chunks).unwrap();
    let rows = crate::properties::runs(template).unwrap();
    let row = rows
        .iter()
        .find(|(name, _)| *name == "ADBE Fractal Noise-0004")
        .unwrap()
        .1;
    let mut leaf = row
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .unwrap()
        .clone();
    let children = leaf.children_mut().unwrap();
    let numeric = crate::properties::read_numeric(children).unwrap();
    assert!(!numeric.animated && numeric.keyframes.is_empty());
    assert!(!numeric.expression_present && !numeric.expression_enabled);
    assert_eq!(numeric.values.len(), 1);
    *children
        .iter_mut()
        .find(|chunk| chunk.id() == *b"cdat")
        .unwrap() = Chunk::data(*b"cdat", blend.to_be_bytes()).unwrap();
    let name = b"ADBE Fractal Noise-0030";
    let mut name_bytes = vec![0; 40];
    name_bytes[..name.len()].copy_from_slice(name);
    body.push(Chunk::data(*b"tdmn", name_bytes).unwrap());
    body.push(leaf);
    assert_eq!(&body[..original.len()], original.as_slice());
    let rows = crate::properties::runs(body).unwrap();
    let matches: Vec<_> = rows
        .iter()
        .filter(|(name, _)| *name == "ADBE Fractal Noise-0030")
        .collect();
    assert_eq!(matches.len(), 1);
    let numeric = crate::properties::read_numeric(
        crate::properties::unique_list(matches[0].1, *b"tdbs").unwrap(),
    )
    .unwrap();
    assert_eq!(numeric.values, [blend]);
    assert!(!numeric.animated && numeric.keyframes.is_empty());
    assert!(!numeric.expression_present && !numeric.expression_enabled);
}

#[test]
fn native_fractal_turbulent_keys_targets_clocks_and_disabled_expression() {
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/native-fractal-turbulent-keys.aep"
    ))
    .unwrap();
    let items: HashMap<_, _> = project.items.iter().map(|item| (item.id, item)).collect();
    for comp_id in [1, 16] {
        let ItemKind::Composition(comp) = &project.item(comp_id).unwrap().kind else {
            panic!()
        };
        let source = &comp.layers[0];
        assert_eq!(source.record.id(), if comp_id == 1 { 15 } else { 29 });
        for (start, stretch) in [(0_i32, 1_i32), (2, 2)] {
            let mut layer = source.clone();
            let mut record = layer.record.encode();
            record[12..16].copy_from_slice(&start.to_be_bytes());
            record[16..20].copy_from_slice(&1_u32.to_be_bytes());
            record[8..12].copy_from_slice(&stretch.to_be_bytes());
            record[108..112].copy_from_slice(&1_u32.to_be_bytes());
            layer.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
            let result = import_with_context(
                ImportContext {
                    evaluations: &Default::default(),
                    composition_id: comp_id,
                    composition: Some(comp),
                    items: Some(&items),
                },
                &layer,
                [320, 180],
                [320, 180],
                &mut 100,
                &mut AnimationBudget::default(),
            );
            assert_eq!(result.effects.len(), 1, "{:?}", result.warnings);
            assert_eq!(result.native_ordinals, [1]);
            assert_eq!(result.animations.len(), 6, "{:?}", result.warnings);
            for (name, expected) in [
                ("contrast", [100., 130.]),
                ("brightness", [0., 10.]),
                ("scale", [100., 140.]),
                ("offsetX", [0., 5.]),
                ("offsetY", [0., -2.5]),
                ("evolution", [0., 45.]),
            ] {
                let entry = result.animations.iter().find(|entry| matches!(&entry.target, PropertyTarget::EffectProperty(target) if target.param_name()==name)).unwrap();
                let json = serde_json::to_value(entry).unwrap();
                for (index, value) in expected.into_iter().enumerate() {
                    assert!(
                        (json["animator"]["keyframes"][index]["value"]["value"]
                            .as_f64()
                            .unwrap()
                            - value)
                            .abs()
                            < 1e-8
                    );
                    assert_eq!(
                        json["animator"]["keyframes"][index]["layerTime"],
                        serde_json::json!((start + index as i32 * stretch) * 1000)
                    );
                }
            }
        }
        if comp_id != 1 {
            continue;
        }
        for live in [false, true] {
            let mut layer = source.clone();
            assert!(update_leaf(
                &mut layer.content,
                "ADBE Fractal Noise-0004",
                &|leaf| {
                    let meta = leaf.iter_mut().find(|c| c.id() == *b"tdb4").unwrap();
                    let mut bytes = meta.data_payload().unwrap().to_vec();
                    bytes[120] |= 1;
                    if live {
                        bytes[119] &= !1;
                    } else {
                        bytes[119] |= 1;
                    }
                    *meta = Chunk::data(*b"tdb4", bytes).unwrap();
                    leaf.push(Chunk::data(*b"Utf8", b"value + 5\0".to_vec()).unwrap());
                }
            ));
            let result = import_with_context(
                ImportContext {
                    evaluations: &Default::default(),
                    composition_id: comp_id,
                    composition: Some(comp),
                    items: Some(&items),
                },
                &layer,
                [320, 180],
                [320, 180],
                &mut 100,
                &mut AnimationBudget::default(),
            );
            assert_eq!(
                result.effects.len(),
                usize::from(!live),
                "{:?}",
                result.warnings
            );
            assert_eq!(result.animations.len(), if live { 0 } else { 6 });
        }
        let mut staged = source.clone();
        derive_fractal_blend(&mut staged, 5.);
        let result = import_with_context(
            ImportContext {
                evaluations: &Default::default(),
                composition_id: comp_id,
                composition: Some(comp),
                items: Some(&items),
            },
            &staged,
            [320, 180],
            [320, 180],
            &mut 100,
            &mut AnimationBudget::default(),
        );
        assert_eq!(result.fractal_blends.len(), 1, "{:?}", result.warnings);
        assert!(result.animations.is_empty());
        assert_eq!(result.fractal_blends[0].animations.len(), 6);

        for value in [0., f64::NAN] {
            let mut layer = source.clone();
            assert!(update_leaf(
                &mut layer.content,
                "ADBE Fractal Noise-0010",
                &|leaf| {
                    leaf.retain(|chunk| chunk.id() != *b"cdat");
                    leaf.push(Chunk::data(*b"cdat", value.to_be_bytes()).unwrap());
                }
            ));
            let result = import_with_context(
                ImportContext {
                    evaluations: &Default::default(),
                    composition_id: comp_id,
                    composition: Some(comp),
                    items: Some(&items),
                },
                &layer,
                [320, 180],
                [320, 180],
                &mut 100,
                &mut AnimationBudget::default(),
            );
            assert!(
                result.effects.is_empty(),
                "invalid scale {value}: {:?}",
                result.warnings
            );
            assert!(result.animations.is_empty());
        }
        let mut layer = source.clone();
        let mut record = layer.record.encode();
        record[108..112].copy_from_slice(&0_u32.to_be_bytes());
        layer.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
        let result = import_with_context(
            ImportContext {
                evaluations: &Default::default(),
                composition_id: comp_id,
                composition: Some(comp),
                items: Some(&items),
            },
            &layer,
            [320, 180],
            [320, 180],
            &mut 100,
            &mut AnimationBudget::default(),
        );
        assert!(result.effects.is_empty());
        assert!(result.animations.is_empty());
    }
}

#[test]
fn native_fractal_keyed_multiply_screen_publish_tracks_on_generated_effect() {
    fn effect_count(layers: &serde_json::Value, id: &serde_json::Value) -> usize {
        layers
            .as_array()
            .unwrap()
            .iter()
            .map(|layer| {
                let own = layer["effects"].as_array().map_or(0, |effects| {
                    effects.iter().filter(|effect| &effect["id"] == id).count()
                });
                own + if layer["layers"].is_array() {
                    effect_count(&layer["layers"], id)
                } else {
                    0
                }
            })
            .sum()
    }
    for blend in [5_f64, 6.] {
        // Multiply is independently Adobe-authored; Screen remains supplementary.
        let source: &[u8] = if blend == 5. {
            include_bytes!("../../../tests/fixtures/effects/native-fractal-multiply-keys.aep")
        } else {
            include_bytes!("../../../tests/fixtures/effects/native-fractal-turbulent-keys.aep")
        };
        let mut project = read_project(source).unwrap();
        let ItemKind::Composition(comp) = &mut project
            .items
            .iter_mut()
            .find(|item| item.id == 1)
            .unwrap()
            .kind
        else {
            panic!()
        };
        assert_eq!(comp.layers[0].record.id(), 15);
        if blend == 6. {
            derive_fractal_blend(&mut comp.layers[0], blend);
        }
        let native = native::read_effects(&comp.layers[0].content, [320., 180.]).0;
        let control = native[0]
            .parameters
            .iter()
            .find(|control| control.match_name == "ADBE Fractal Noise-0030")
            .unwrap();
        assert_eq!(control.numeric.as_ref().unwrap().values, [blend]);
        let imported =
            crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
        let doc = imported.document.to_json_value().unwrap();
        let entries = doc["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let effect_entries: Vec<_> = entries
            .iter()
            .filter(|entry| entry["target"]["kind"] == "effectProperty")
            .collect();
        assert_eq!(effect_entries.len(), 6, "{:?}", imported.diagnostics);
        let target = &effect_entries[0]["target"]["effectId"];
        assert_eq!(effect_count(&doc["composition"]["layers"], target), 1);
        for entry in effect_entries {
            assert_eq!(&entry["target"]["effectId"], target);
            assert_eq!(
                entry["animator"]["keyframes"][0]["layerTime"],
                serde_json::json!(0)
            );
            assert_eq!(
                entry["animator"]["keyframes"][1]["layerTime"],
                serde_json::json!(1000)
            );
        }
    }
}
