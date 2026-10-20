use crate::{effects::native, rifx::Chunk, structure::ItemKind};

fn named(name: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn slider(plugin: Vec<Chunk>) -> Vec<Chunk> {
    vec![Chunk::list(
        *b"tdgp",
        vec![
            named("ADBE Effect Parade"),
            Chunk::list(
                *b"tdgp",
                vec![named("ADBE Slider Control"), Chunk::list(*b"sspc", plugin)],
            ),
        ],
    )]
}

#[test]
fn native_effect_declaration_order_keeps_evaluated_editable_position() {
    use crate::properties;

    let source = crate::structure::read_project(include_bytes!(
        "../../tests/fixtures/essential/multiple_controllers.aep"
    ))
    .unwrap();
    let ItemKind::Composition(source_comp) = &source.item(1).unwrap().kind else {
        panic!("native composition 1")
    };
    let roots = properties::root_runs(&source_comp.layers[0].content).unwrap();
    let (_, parade) = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Parade")
        .unwrap();
    let instances = properties::runs(properties::unique_list(parade, *b"tdgp").unwrap()).unwrap();
    let (_, fill) = instances
        .iter()
        .find(|(name, _)| *name == "ADBE Fill")
        .unwrap();
    let plugin = properties::unique_list(fill, *b"sspc").unwrap();
    let declarations =
        properties::runs(properties::unique_list(plugin, *b"parT").unwrap()).unwrap();
    assert_eq!(declarations[2].0, "ADBE Fill-0007");
    assert_eq!(declarations[3].0, "ADBE Fill-0002");
    let projection = crate::test_fixtures::read("essential/multiple_controllers.json.gz");
    let projection: serde_json::Value = serde_json::from_slice(&projection).unwrap();
    fn color(value: &serde_json::Value) -> Option<&serde_json::Value> {
        if value["matchName"] == "ADBE Fill-0002" {
            return Some(value);
        }
        match value {
            serde_json::Value::Object(fields) => fields.values().find_map(color),
            serde_json::Value::Array(values) => values.iter().find_map(color),
            _ => None,
        }
    }
    let oracle = color(&projection).unwrap();
    assert_eq!(oracle["propertyIndex"], 3);
    assert_eq!(oracle["value"], serde_json::json!([1, 0, 0, 1]));

    for sparse in [false, true] {
        let mut project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/expression_samples/sampled_position_expression.aep"
        ))
        .unwrap();
        project.effect_definitions = source.effect_definitions.clone();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!("receiving composition 1")
        };
        fn replace(chunks: &mut [Chunk]) -> usize {
            let mut count = 0;
            for chunk in chunks {
                if chunk.list_kind() == Some(*b"tdbs") {
                    for leaf in chunk.children_mut().unwrap() {
                        if leaf.id() == *b"Utf8" {
                            *leaf = Chunk::data(
                                *b"Utf8",
                                b"[effect('ADBE Fill')(3)[0]*100+time, effect('ADBE Fill')(3)[1]*100,0]"
                                    .to_vec(),
                            )
                            .unwrap();
                            count += 1;
                        }
                    }
                } else if let Some(children) = chunk.children_mut() {
                    count += replace(children);
                }
            }
            count
        }
        let layer = &mut comp.layers[0];
        assert_eq!(replace(&mut layer.content), 1);
        let mut plugin = plugin.to_vec();
        if sparse {
            plugin
                .iter_mut()
                .find(|c| c.list_kind() == Some(*b"parT"))
                .unwrap()
                .children_mut()
                .unwrap()
                .clear();
        }
        let body = plugin
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        assert!(!body.iter().any(|c| {
            c.data_payload()
                .is_some_and(|b| b.starts_with(b"ADBE Fill-0002"))
        }));
        let mut meta = vec![0; 124];
        meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 4]);
        meta[59] = 1;
        body.extend([
            named("ADBE Fill-0002"),
            Chunk::list(
                *b"tdbs",
                vec![
                    Chunk::data(*b"tdb4", meta).unwrap(),
                    Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                    Chunk::data(
                        *b"cdat",
                        [255_f64, 255., 0., 0.]
                            .into_iter()
                            .flat_map(f64::to_be_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                ],
            ),
        ]);
        let root = layer
            .content
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        root.splice(
            0..0,
            [
                named("ADBE Effect Parade"),
                Chunk::list(
                    *b"tdgp",
                    vec![named("ADBE Fill"), Chunk::list(*b"sspc", plugin)],
                ),
            ],
        );
        let converted =
            crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
        let (_, error) = super::persisted_position_errors(
            &converted,
            &[0., 0.5, 1.],
            &[vec![100., 0., 0.], vec![100.5, 0., 0.], vec![101., 0., 0.]],
        )
        .unwrap_or_else(|| {
            panic!(
                "editable Position omitted (sparse={sparse}): {:?}",
                converted.diagnostics
            )
        });
        assert!(error < 1e-8, "Position error {error}, sparse={sparse}");
    }
}

#[test]
fn sparse_slider_default_keeps_evaluated_editable_position() {
    // Code-mutated public fixture, not a new independent Adobe oracle.
    let mut project = crate::structure::read_project(include_bytes!(
        "../../tests/fixtures/expression_samples/sampled_position_expression.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!("composition 1")
    };
    fn replace(chunks: &mut [Chunk]) -> usize {
        let mut count = 0;
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"tdbs") {
                for leaf in chunk.children_mut().unwrap() {
                    if leaf.id() == *b"Utf8" {
                        *leaf = Chunk::data(
                            *b"Utf8",
                            b"[100+time*10+effect('ADBE Slider Control')(1),200+time*20,0]"
                                .to_vec(),
                        )
                        .unwrap();
                        count += 1;
                    }
                }
            } else if let Some(children) = chunk.children_mut() {
                count += replace(children);
            }
        }
        count
    }
    let layer = &mut comp.layers[0];
    assert_eq!(replace(&mut layer.content), 1);
    let mut sparse = slider(vec![
        Chunk::list(*b"parT", vec![]),
        Chunk::list(*b"tdgp", vec![]),
    ])
    .remove(0);
    let root = layer
        .content
        .iter_mut()
        .find(|c| c.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    root.splice(0..0, sparse.children_mut().unwrap().drain(..));
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    let times = [0.0, 0.5, 1.0];
    let values = [
        vec![100.0, 200.0, 0.0],
        vec![105.0, 210.0, 0.0],
        vec![110.0, 220.0, 0.0],
    ];
    let (_, error) = super::persisted_position_errors(&converted, &times, &values)
        .unwrap_or_else(|| panic!("editable Position omitted: {:?}", converted.diagnostics));
    assert!(error < 1e-8, "persisted Position error {error}");
}

#[test]
fn sparse_slider_default_does_not_replace_explicit_or_malformed_controls() {
    fn value(plugin: Vec<Chunk>) -> Result<Vec<f64>, crate::properties::PropertyError> {
        let (effects, _) = native::read_effects(&slider(plugin), [100.0, 100.0]);
        effects[0]
            .parameters
            .iter()
            .find(|p| p.match_name == "ADBE Slider Control-0001")
            .expect("slider parameter")
            .numeric
            .clone()
            .map(|p| p.values)
    }
    let empty = || Chunk::list(*b"tdgp", vec![]);
    assert_eq!(value(vec![empty()]).unwrap(), [0.0]);
    assert_eq!(
        value(vec![Chunk::list(*b"parT", vec![]), empty()]).unwrap(),
        [0.0]
    );
    assert!(
        value(vec![
            Chunk::list(*b"parT", vec![]),
            Chunk::list(*b"parT", vec![]),
            empty()
        ])
        .is_err()
    );
    assert!(
        value(vec![
            Chunk::list(*b"parT", vec![Chunk::data(*b"tdmn", vec![0xff]).unwrap()]),
            empty()
        ])
        .is_err()
    );
    let control = || {
        Chunk::list(
            *b"tdgp",
            vec![
                named("ADBE Slider Control-0001"),
                Chunk::list(*b"tdbs", vec![]),
            ],
        )
    };
    assert!(value(vec![control()]).is_err());
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    let explicit = || {
        Chunk::list(
            *b"tdgp",
            vec![
                named("ADBE Slider Control-0001"),
                Chunk::list(
                    *b"tdbs",
                    vec![
                        Chunk::data(*b"tdb4", meta.clone()).unwrap(),
                        Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                        Chunk::data(*b"cdat", 2.5_f64.to_be_bytes().to_vec()).unwrap(),
                    ],
                ),
            ],
        )
    };
    assert_eq!(value(vec![explicit()]).unwrap(), [2.5]);
    let mut duplicate = explicit();
    let children = duplicate.children_mut().unwrap();
    children.extend(children.clone());
    assert!(value(vec![duplicate]).is_err());
    let local = |bytes| {
        Chunk::list(
            *b"parT",
            vec![
                named("ADBE Slider Control-0001"),
                Chunk::data(*b"pard", bytes).unwrap(),
            ],
        )
    };
    assert!(value(vec![local(vec![0; 5]), empty()]).is_err());
    let mut declaration = vec![0; 148];
    declaration[12..16].copy_from_slice(&10_u32.to_be_bytes());
    declaration[56..64].copy_from_slice(&7.25_f64.to_be_bytes());
    assert_eq!(value(vec![local(declaration), empty()]).unwrap(), [7.25]);
}
