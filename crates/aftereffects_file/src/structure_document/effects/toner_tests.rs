use super::*;
use crate::structure::{ItemKind, read_project};

fn layer(bytes: &[u8], adjustment: bool) -> Layer {
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("native shape composition")
    };
    let mut layer = comp.layers[0].clone();
    layer.content = vec![crate::rifx::Chunk::list(
        *b"tdgp",
        crate::rifx::Rifx::parse_with(bytes, |_| false)
            .unwrap()
            .chunks()
            .to_vec(),
    )];
    if adjustment {
        let mut bytes = layer.record.encode();
        bytes[38] |= 2;
        layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    }
    layer
}
fn imported(layer: &Layer) -> ImportedEffects {
    import(
        &Default::default(),
        1,
        layer,
        [1920, 1080],
        [3840, 2160],
        &mut 999,
        &mut AnimationBudget::default(),
    )
}

#[test]
fn native_toner_tritone_and_sparse_pentone_become_ordered_editable_ramps() {
    for (bytes, adjustment, expected) in [
        (
            include_bytes!("../../../tests/fixtures/effects/cosmic-toner-tritone-controls.rifx")
                .as_slice(),
            false,
            vec![
                [9.0 / 255.0, 18.0 / 255.0, 20.0 / 255.0],
                [0.0; 3],
                [0.0; 3],
            ],
        ),
        (
            include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx")
                .as_slice(),
            true,
            vec![
                [0.0; 3],
                [36.0 / 255.0, 25.0 / 255.0, 98.0 / 255.0],
                [210.0 / 255.0, 79.0 / 255.0, 219.0 / 255.0],
                [230.0 / 255.0, 176.0 / 255.0, 230.0 / 255.0],
                [230.0 / 255.0, 176.0 / 255.0, 230.0 / 255.0],
            ],
        ),
    ] {
        let imported = imported(&layer(bytes, adjustment));
        assert_eq!(imported.effects.len(), 2, "{:?}", imported.warnings);
        let effects: Vec<_> = imported
            .effects
            .iter()
            .map(|effect| serde_json::to_value(effect).unwrap()["effect"].clone())
            .collect();
        assert_eq!(effects[0]["type"], "tintTritone");
        assert_eq!(effects[0]["amount"], 100.0);
        assert_eq!(effects[1]["type"], "colorCurves");
        for (channel, name) in ["red", "green", "blue"].into_iter().enumerate() {
            let points = effects[1]["curves"][name].as_array().unwrap();
            assert_eq!(points.len(), expected.len());
            for (index, (point, color)) in points.iter().zip(&expected).enumerate() {
                assert_eq!(
                    point["x"].as_f64(),
                    Some(index as f64 / (expected.len() - 1) as f64)
                );
                assert!((point["y"].as_f64().unwrap() - color[channel]).abs() < 1e-7);
            }
        }
        assert!(imported.warnings.iter().any(
            |warning| warning.contains("luma weights") && warning.contains("piecewise-linear")
        ));
        assert!(imported.animations.is_empty());
    }
}

fn named_leaf_mut<'a>(
    chunks: &'a mut [crate::rifx::Chunk],
    target: &str,
) -> Option<&'a mut Vec<crate::rifx::Chunk>> {
    let index = chunks.iter().enumerate().find_map(|(index, chunk)| {
        (chunk.id() == *b"tdmn"
            && chunk.data_payload().is_some_and(|bytes| {
                bytes
                    .strip_suffix(&[0])
                    .unwrap_or(bytes)
                    .starts_with(target.as_bytes())
            }))
        .then(|| {
            let end = chunks[index + 1..]
                .iter()
                .position(|chunk| chunk.id() == *b"tdmn")
                .map_or(chunks.len(), |offset| index + 1 + offset);
            (index + 1..end).find(|index| chunks[*index].list_kind() == Some(*b"tdbs"))
        })
        .flatten()
    });
    if let Some(index) = index {
        return chunks[index].children_mut();
    }
    for chunk in chunks {
        if let Some(children) = chunk.children_mut()
            && let Some(leaf) = named_leaf_mut(children, target)
        {
            return Some(leaf);
        }
    }
    None
}

#[test]
fn native_toner_invalid_and_dynamic_consumed_controls_are_omitted_atomically() {
    use crate::rifx::Chunk;
    for (target, value, expression) in [
        ("CC Toner-0005", 1.0_f64, false),
        ("CC Toner-0004", 1.1, false),
        ("CC Toner-0002", 2.0, false),
        ("CC Toner-0002", 0.5, true),
    ] {
        let mut layer = layer(
            include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx"),
            false,
        );
        let leaf = named_leaf_mut(&mut layer.content, target).unwrap();
        if expression {
            let meta = leaf
                .iter_mut()
                .find(|chunk| chunk.id() == *b"tdb4")
                .unwrap();
            let mut bytes = meta.data_payload().unwrap().to_vec();
            bytes[120] |= 1;
            bytes[119] &= !1;
            *meta = Chunk::data(*b"tdb4", bytes).unwrap();
        } else {
            let data = leaf
                .iter_mut()
                .find(|chunk| chunk.id() == *b"cdat")
                .unwrap();
            let mut bytes = data.data_payload().unwrap().to_vec();
            bytes[..8].copy_from_slice(&value.to_be_bytes());
            *data = Chunk::data(*b"cdat", bytes).unwrap();
        }
        let mut next_id = 999;
        let imported = import(
            &Default::default(),
            1,
            &layer,
            [1920, 1080],
            [1920, 1080],
            &mut next_id,
            &mut AnimationBudget::default(),
        );
        assert!(
            imported.effects.is_empty(),
            "{target}: {:?}",
            imported.warnings
        );
        assert!(imported.adjustment_opacity.is_none());
        assert_eq!(next_id, 999);
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("effect omitted"))
        );
    }
}

/// Repeats one explicit control record: its `tdmn` name and `tdbs` value.
fn repeat_record(chunks: &mut Vec<crate::rifx::Chunk>, target: &str) -> bool {
    let record = chunks.iter().enumerate().find_map(|(start, chunk)| {
        (chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|bytes| bytes.starts_with(target.as_bytes())))
        .then(|| {
            let end = chunks[start + 1..]
                .iter()
                .position(|chunk| chunk.id() == *b"tdmn")
                .map_or(chunks.len(), |offset| start + 1 + offset);
            chunks[start + 1..end]
                .iter()
                .any(|chunk| chunk.list_kind() == Some(*b"tdbs"))
                .then_some(start..end)
        })
        .flatten()
    });
    if let Some(range) = record {
        let copy = chunks[range.clone()].to_vec();
        chunks.splice(range.end..range.end, copy);
        return true;
    }
    chunks.iter_mut().any(|chunk| {
        chunk
            .children_mut()
            .is_some_and(|children| repeat_record(children, target))
    })
}

#[test]
fn native_toner_duplicate_explicit_record_is_omitted_atomically() {
    let mut layer = layer(
        include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx"),
        false,
    );
    assert!(repeat_record(&mut layer.content, "CC Toner-0005"));
    let mut next_id = 999;
    let imported = import(
        &Default::default(),
        1,
        &layer,
        [1920, 1080],
        [1920, 1080],
        &mut next_id,
        &mut AnimationBudget::default(),
    );
    assert!(imported.effects.is_empty(), "{:?}", imported.warnings);
    assert_eq!(next_id, 999);
    assert!(
        imported.warnings.iter().any(|warning| {
            warning.starts_with("Effect CC Toner: CC Toner-0005")
                && warning.contains("duplicate Toner control")
                && warning.contains("effect omitted")
        }),
        "{:?}",
        imported.warnings
    );
}

#[test]
fn native_toner_original_mixing_requires_sole_normal_ungated_adjustment() {
    use crate::rifx::Chunk;
    let bytes =
        include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx");
    let valid = layer(bytes, true);
    let accepted = imported(&valid);
    assert_eq!(accepted.adjustment_opacity.unwrap().value(), 60.0);
    for variant in [
        "ordinary",
        "blend",
        "matte",
        "opacity",
        "enabled sibling",
        "malformed opacity",
    ] {
        let mut layer = valid.clone();
        let mut record = layer.record.encode();
        match variant {
            "ordinary" => record[38] &= !2,
            "blend" => record[99] = 5,
            "matte" => record[107] = 1,
            "opacity" | "malformed opacity" => {
                // A native effect scalar leaf supplies the same tdbs numeric envelope.
                let scalar = named_leaf_mut(&mut layer.content, "CC Toner-0004")
                    .unwrap()
                    .clone();
                let mut body = scalar;
                let data = body
                    .iter_mut()
                    .find(|chunk| chunk.id() == *b"cdat")
                    .unwrap();
                *data = Chunk::data(
                    *b"cdat",
                    if variant == "opacity" {
                        [50.0_f64, 0.0, 0.0, 0.0, 0.0]
                            .into_iter()
                            .flat_map(f64::to_be_bytes)
                            .collect()
                    } else {
                        vec![0]
                    },
                )
                .unwrap();
                layer.content[0].children_mut().unwrap().extend([
                    match_name("ADBE Transform Group"),
                    Chunk::list(
                        *b"tdgp",
                        vec![match_name("ADBE Opacity"), Chunk::list(*b"tdbs", body)],
                    ),
                ]);
            }
            "enabled sibling" => {
                let root = layer.content[0].children_mut().unwrap();
                let parade = root
                    .iter_mut()
                    .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                    .unwrap()
                    .children_mut()
                    .unwrap();
                let mut sibling = parade.clone();
                let name = sibling
                    .iter_mut()
                    .find(|chunk| chunk.id() == *b"tdmn")
                    .unwrap();
                *name = match_name("Unknown Enabled Effect");
                parade.extend(sibling);
            }
            _ => unreachable!(),
        }
        layer.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
        let rejected = imported(&layer);
        assert!(
            rejected.effects.is_empty(),
            "{variant}: {:?}",
            rejected.warnings
        );
        assert!(rejected.adjustment_opacity.is_none());
        assert!(
            rejected
                .warnings
                .iter()
                .any(|warning| warning.contains("Blend with Original requires")),
            "{variant}: {:?}",
            rejected.warnings
        );
    }
}

#[test]
fn native_toner_disabled_sibling_preserves_prior_wet_gate_and_pair_identity() {
    use crate::rifx::Chunk;
    let mut layer = layer(
        include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx"),
        true,
    );
    let root = layer.content[0].children_mut().unwrap();
    let parade = root
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    let mut sibling = parade.clone();
    let sspc = sibling
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"sspc"))
        .unwrap()
        .children_mut()
        .unwrap();
    let body = sspc
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    let flags = body
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdsb")
        .unwrap();
    *flags = Chunk::data(*b"tdsb", [0_u8; 4]).unwrap();
    parade.extend(sibling);
    let imported = imported(&layer);
    assert_eq!(imported.effects.len(), 4, "{:?}", imported.warnings);
    assert_eq!(imported.adjustment_opacity.unwrap().value(), 60.0);
    let records: Vec<_> = imported
        .effects
        .iter()
        .map(|effect| serde_json::to_value(effect).unwrap())
        .collect();
    assert_eq!(records[0]["enabled"], true);
    assert_eq!(records[1]["enabled"], true);
    assert_eq!(records[2]["enabled"], false);
    assert_eq!(records[3]["enabled"], false);
    assert_ne!(records[0]["id"], records[1]["id"]);
}

fn match_name(name: &str) -> crate::rifx::Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    crate::rifx::Chunk::data(*b"tdmn", bytes).unwrap()
}

#[test]
fn native_toner_partial_adjustment_emits_wet_opacity_on_actual_stack_gate() {
    use fx_schema::LayerData;
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(composition) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!("native composition")
    };
    composition.layers[0] = layer(
        include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx"),
        true,
    );
    let converted = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
    fn adjustments(layers: &[fx_schema::Layer], output: &mut Vec<fx_schema::AdjustmentLayer>) {
        for layer in layers {
            match layer.data() {
                LayerData::Adjustment(adjustment) => output.push(adjustment.clone()),
                LayerData::Group(group) => adjustments(&group.layers, output),
                _ => {}
            }
        }
    }
    let mut gates = Vec::new();
    adjustments(converted.document.composition().layers(), &mut gates);
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].transform.opacity.value(), 60.0);
    assert_eq!(gates[0].effects.len(), 2);
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|warning| warning.message.contains("dry-plus-wet alpha"))
    );
}

#[test]
fn native_toner_original_gate_ignores_only_proven_neutral_hls_stage() {
    use crate::rifx::Chunk;
    for lightness in [0.0_f64, 1.0] {
        let mut owner = layer(
            include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx"),
            true,
        );
        let scalar = named_leaf_mut(&mut owner.content, "CC Toner-0004")
            .unwrap()
            .clone();
        let mut leaves = Vec::new();
        for (parameter, value) in [
            ("ADBE Color Balance (HLS)-0001", 0.0_f64),
            ("ADBE Color Balance (HLS)-0002", lightness),
            ("ADBE Color Balance (HLS)-0003", 0.0_f64),
        ] {
            let mut leaf = scalar.clone();
            let value_chunk = leaf
                .iter_mut()
                .find(|chunk| chunk.id() == *b"cdat")
                .unwrap();
            *value_chunk = Chunk::data(*b"cdat", value.to_be_bytes()).unwrap();
            leaves.extend([match_name(parameter), Chunk::list(*b"tdbs", leaf)]);
        }
        let root = owner.content[0].children_mut().unwrap();
        let parade = root
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        parade.extend([
            match_name("ADBE Color Balance (HLS)"),
            Chunk::list(
                *b"sspc",
                vec![Chunk::list(*b"parT", vec![]), Chunk::list(*b"tdgp", leaves)],
            ),
        ]);
        let imported = imported(&owner);
        if lightness == 0.0 {
            assert_eq!(imported.effects.len(), 2, "{:?}", imported.warnings);
            assert_eq!(imported.adjustment_opacity.unwrap().value(), 60.0);
        } else {
            assert!(imported.effects.is_empty());
            assert!(imported.adjustment_opacity.is_none());
        }
    }
}

#[test]
fn toner_original_gate_rejects_malformed_adjustment_styles() {
    let mut owner = layer(
        include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx"),
        true,
    );
    owner.content[0]
        .children_mut()
        .unwrap()
        .push(match_name("ADBE Layer Styles"));
    let result = imported(&owner);
    assert!(result.effects.is_empty());
    assert!(result.adjustment_opacity.is_none());
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Adjustment Layer Styles"))
    );
}

#[test]
fn toner_original_gate_uses_native_unit_opacity_and_declines_partial_or_animated() {
    for (opacity, animated, accepted) in [
        (1.0_f64, false, true),
        (0.5, false, false),
        (1.0, true, false),
    ] {
        let mut owner = layer(
            include_bytes!("../../../tests/fixtures/effects/cosmic-toner-pentone-controls.rifx"),
            true,
        );
        let mut leaf = named_leaf_mut(&mut owner.content, "CC Toner-0004")
            .unwrap()
            .clone();
        let cdat = leaf.iter_mut().find(|c| c.id() == *b"cdat").unwrap();
        *cdat = crate::rifx::Chunk::data(*b"cdat", opacity.to_be_bytes()).unwrap();
        if animated {
            let meta = leaf.iter_mut().find(|c| c.id() == *b"tdb4").unwrap();
            let mut bytes = meta.data_payload().unwrap().to_vec();
            bytes[68] = 1;
            *meta = crate::rifx::Chunk::data(*b"tdb4", bytes).unwrap();
        }
        owner.content[0].children_mut().unwrap().extend([
            match_name("ADBE Transform Group"),
            crate::rifx::Chunk::list(
                *b"tdgp",
                vec![
                    match_name("ADBE Opacity"),
                    crate::rifx::Chunk::list(*b"tdbs", leaf),
                ],
            ),
        ]);
        let result = imported(&owner);
        assert_eq!(
            result.adjustment_opacity.is_some(),
            accepted,
            "{opacity}/{animated}: {:?}",
            result.warnings
        );
    }
}
