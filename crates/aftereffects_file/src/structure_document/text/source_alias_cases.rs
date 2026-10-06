//! Native-fixture-derived supplementary cases; mutations are not Adobe proof.
use super::*;
use crate::structure::{Composition, ItemKind, read_project};

fn storage(chunks: &mut [Chunk]) -> &mut Vec<Chunk> {
    for chunk in chunks {
        if chunk.list_kind() == Some(*b"btds") {
            return chunk.children_mut().unwrap();
        }
        if let Some(children) = chunk.children_mut()
            && contains_storage(children)
        {
            return storage(children);
        }
    }
    panic!("native Source Text storage")
}

fn contains_storage(chunks: &[Chunk]) -> bool {
    chunks.iter().any(|chunk| {
        chunk.list_kind() == Some(*b"btds") || chunk.children().is_some_and(contains_storage)
    })
}

fn expression(layer: &mut Layer, text: &str, disabled: bool, keyed: bool) {
    let chunks = storage(&mut layer.content);
    let metadata = chunks
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .unwrap()
        .children_mut()
        .unwrap();
    let flags = metadata
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdb4")
        .unwrap();
    let mut bytes = flags.data_payload().unwrap().to_vec();
    bytes[119] = u8::from(disabled);
    bytes[120] = 1;
    bytes[68] = u8::from(keyed);
    *flags = Chunk::data(*b"tdb4", bytes).unwrap();
    metadata.retain(|chunk| !matches!(&chunk.id(), b"Utf8" | b"expr"));
    metadata.push(Chunk::data(*b"Utf8", text.as_bytes().to_vec()).unwrap());
}

fn text_path(chunks: &mut [Chunk]) -> bool {
    for index in 0..chunks.len() {
        if chunks[index].id() == *b"tdmn"
            && chunks[index]
                .data_payload()
                .is_some_and(|bytes| bytes.starts_with(b"ADBE Text Properties\0"))
        {
            fn name(text: &str) -> Chunk {
                let mut bytes = text.as_bytes().to_vec();
                bytes.resize(40, 0);
                Chunk::data(*b"tdmn", bytes).unwrap()
            }
            let mut flags = vec![0; 124];
            flags[..2].copy_from_slice(&[0xdb, 0x99]);
            flags[3] = 1;
            flags[12..16].copy_from_slice(&1000_u32.to_be_bytes());
            let properties = chunks[index + 1].children_mut().unwrap();
            properties.extend([
                name("ADBE Text Path Options"),
                Chunk::list(
                    *b"tdgp",
                    vec![
                        Chunk::data(*b"tdsb", vec![0, 0, 0, 3]).unwrap(),
                        name("ADBE Text Path"),
                        Chunk::list(
                            *b"tdbs",
                            vec![
                                Chunk::data(*b"tdb4", flags).unwrap(),
                                Chunk::data(*b"cdat", 1.0_f64.to_be_bytes().to_vec()).unwrap(),
                            ],
                        ),
                    ],
                ),
            ]);
            return true;
        }
        if chunks[index]
            .children_mut()
            .is_some_and(|children| text_path(children))
        {
            return true;
        }
    }
    false
}

fn fixture() -> (crate::structure::StructuralProject, usize, usize) {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/text/text_ranges.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    let owner = comp
        .layers
        .iter()
        .position(|layer| layer.record.id() == 21)
        .unwrap();
    let target = comp
        .layers
        .iter()
        .position(|layer| layer.record.id() == 14)
        .unwrap();
    comp.layers[target].name = "Alias target".into();
    expression(
        &mut comp.layers[owner],
        "thisComp.layer(\"Alias target\").text.sourceText",
        false,
        false,
    );
    (project, owner, target)
}

fn imported(comp: &Composition, owner: usize) -> TextImport {
    let (mut project, _, _) = fixture();
    project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind = ItemKind::Composition(Box::new(comp.clone()));
    let converted = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
    fn find(layers: &[fx_schema::Layer], owner: &str) -> Option<FxLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Text(text) if owner.is_empty() || text.name.as_str() == owner => {
                Some(layer.data().clone())
            }
            FxLayer::Group(group) => {
                find(&group.layers, if group.name == owner { "" } else { owner })
            }
            _ => None,
        })
    }
    TextImport {
        layers: vec![
            find(
                converted.document.composition().layers(),
                &comp.layers[owner].name,
            )
            .expect("converter emitted consumer Text"),
        ],
        warnings: converted
            .diagnostics
            .into_iter()
            .filter(|note| {
                note.composition_id == Some(1)
                    && note.layer_id == Some(comp.layers[owner].record.id())
            })
            .map(|note| note.message)
            .collect(),
        animations: Vec::new(),
    }
}

#[test]
fn source_text_alias_native_fixture_target_and_consumer_rejections() {
    let (project, owner, target) = fixture();
    let ItemKind::Composition(original) = &project.item(1).unwrap().kind else {
        panic!()
    };
    let accepted = imported(original, owner);
    let [FxLayer::Text(text)] = accepted.layers.as_slice() else {
        panic!("{:?}", accepted.warnings)
    };
    assert_eq!(
        text.source_text.text, "Hello World\nSecond Paragraph\nEnd",
        "{:?}",
        accepted.warnings
    );
    assert!(
        accepted
            .warnings
            .iter()
            .any(|warning| warning.contains("unmapped paragraph fields")),
        "uniform native paragraph controls must not disappear silently: {:?}",
        accepted.warnings
    );
    assert!(cached_caption_width(&original.layers[owner]).is_err());
    let mut source = read_source(&original.layers[owner]).unwrap().unwrap();
    source_alias::lower(&original.layers[owner], original, &mut source);
    assert!(
        !source.document_is_static,
        "copied expression content is not cached layout"
    );
    assert!(source.frame.is_none());
    for case in [
        "missing",
        "duplicate",
        "self",
        "target-expression",
        "target-keyed",
        "target-malformed-signature",
        "consumer-keyed",
        "disabled",
        "box",
        "mixed-character",
        "path",
    ] {
        let mut comp = original.clone();
        match case {
            "missing" => comp.layers[target].name = "Missing".into(),
            "duplicate" => {
                comp.layers.push(comp.layers[target].clone());
            }
            "self" => {
                comp.layers[target].name = "Missing".into();
                comp.layers[owner].name = "Alias target".into();
            }
            "target-expression" => expression(&mut comp.layers[target], "value", false, false),
            "target-keyed" => expression(&mut comp.layers[target], "value", true, true),
            "target-malformed-signature" => {
                let metadata = storage(&mut comp.layers[target].content)
                    .iter_mut()
                    .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
                    .unwrap()
                    .children_mut()
                    .unwrap();
                let flags = metadata
                    .iter_mut()
                    .find(|chunk| chunk.id() == *b"tdb4")
                    .unwrap();
                let mut bytes = flags.data_payload().unwrap().to_vec();
                bytes[..2].copy_from_slice(&[0, 0]);
                *flags = Chunk::data(*b"tdb4", bytes).unwrap();
            }
            "consumer-keyed" => expression(
                &mut comp.layers[owner],
                "thisComp.layer(\"Alias target\").text.sourceText",
                false,
                true,
            ),
            "path" => assert!(
                text_path(&mut comp.layers[owner].content),
                "native path property"
            ),
            "box" | "mixed-character" => {
                let id = if case == "box" { 15 } else { 14 };
                comp.layers[owner].content = original
                    .layers
                    .iter()
                    .find(|layer| layer.record.id() == id)
                    .unwrap()
                    .content
                    .clone();
                expression(
                    &mut comp.layers[owner],
                    "thisComp.layer(\"Alias target\").text.sourceText",
                    false,
                    false,
                );
            }
            "disabled" => expression(
                &mut comp.layers[owner],
                "thisComp.layer(\"Alias target\").text.sourceText",
                true,
                false,
            ),
            _ => unreachable!(),
        }
        let baseline = read_source(&comp.layers[owner])
            .unwrap()
            .unwrap()
            .held_document(None, 0)
            .0
            .text;
        let result = imported(&comp, owner);
        if case == "target-expression" {
            // The alias reads a target whose own Source Text expression the
            // converter evaluates, so the owner shows the target's evaluated
            // text as held content instead of the authored fallback.
            let target_text = read_source(&comp.layers[target])
                .unwrap()
                .unwrap()
                .held_document(None, 0)
                .0
                .text;
            let texts: Vec<_> = result
                .layers
                .iter()
                .filter_map(|layer| match layer {
                    FxLayer::Text(text) => Some(text.source_text.text.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts, vec![target_text], "{case}: {:?}", result.warnings);
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning
                        .contains("Source Text expression evaluated by the converter")),
                "{case}: {:?}",
                result.warnings
            );
            continue;
        }
        let [FxLayer::Text(text)] = result.layers.as_slice() else {
            panic!("{case}: {:?}", result.warnings)
        };
        assert_eq!(
            text.source_text.text, baseline,
            "{case}: {:?}",
            result.warnings
        );
        if case != "disabled" {
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("direct content alias not lowered")),
                "{case}: {:?}",
                result.warnings
            );
        }
    }
}

#[test]
fn source_text_alias_converter_paragraph_codes_and_unknown_fields() {
    let (project, owner, _) = fixture();
    let ItemKind::Composition(original) = &project.item(1).unwrap().kind else {
        panic!()
    };
    for (styles, admitted) in [
        (vec!["/0 2 /9001 7", "/0 2 /9001 7"], true),
        (vec!["/0 2 /9001 7", "/0 2 /9001 8"], true),
        (vec!["/0 2", "/0 1"], false),
        (vec!["/0 2", "/9001 7"], false),
        (vec!["/0 4"], false),
        (vec!["/0 (2)"], false),
    ] {
        let mut comp = original.clone();
        let records = styles
            .iter()
            .map(|style| format!("<< /0 << /0 << /5 << {style} >> >> >> >>"))
            .collect::<Vec<_>>()
            .join(" ");
        // Supplemental COS controls replace only the fixture's document payload.
        // Native layer framing, expression metadata and Converter entry point stay real.
        let payload = format!(
            "<< /1 << /1 [ << /0 << /0 (Authored) /6 << /0 [ << /0 << /0 << /6 << /0 0 /1 42 >> >> >> >> ] >> /5 << /0 [{records}] >> >> >> ] >> >>"
        );
        let blob = storage(&mut comp.layers[owner].content)
            .iter_mut()
            .find(|chunk| chunk.list_kind() == Some(*b"btdk"))
            .unwrap();
        *blob = Chunk::opaque_list(*b"btdk", payload.into_bytes());
        let converted = imported(&comp, owner);
        let [FxLayer::Text(text)] = converted.layers.as_slice() else {
            panic!()
        };
        assert_eq!(
            text.source_text.text,
            if admitted {
                "Hello World\nSecond Paragraph\nEnd"
            } else {
                "Authored"
            },
            "{styles:?}: {:?}",
            converted.warnings
        );
        assert!(
            converted
                .warnings
                .iter()
                .any(|warning| warning.contains(if admitted {
                    "unmapped paragraph fields"
                } else {
                    "direct content alias not lowered"
                })),
            "{styles:?}: {:?}",
            converted.warnings
        );
        if admitted && styles[0] != styles[1] {
            assert!(
                converted
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("unmapped paragraph differences"))
            );
        }
    }
}

#[test]
fn source_text_alias_converter_applies_occurrence_override_without_source_mutation() {
    use super::super::*;
    let (project, owner, target) = fixture();
    let item = project.item(1).unwrap();
    let ItemKind::Composition(comp) = &item.kind else {
        panic!()
    };
    let mut donor = comp.layers[owner].clone();
    // A disabled native expression is static and supplies its authored content.
    expression(&mut donor, "value", true, false);
    let replacement = storage(&mut donor.content).clone();
    let property_override = crate::essential::Override {
        source_comp_id: 1,
        source_layer_id: comp.layers[target].record.id(),
        value: crate::essential::OverrideValue::Property {
            path: ["ADBE Text Properties", "ADBE Text Document"]
                .into_iter()
                .map(|name| crate::essential::SourcePropertyRef {
                    match_name: name.into(),
                    child_index: None,
                })
                .collect(),
            chunks: vec![Chunk::list(*b"btds", replacement)],
        },
    };
    let samples = ExpressionSamples::default();
    let mut resolver = |_: &MediaAssetRequest| MediaResolution::Unavailable;
    let mut converter = Converter {
        text_overrides: Default::default(),
        expression_samples: &samples,
        expression_evaluations: Default::default(),
        items: project.items.iter().map(|item| (item.id, item)).collect(),
        camera_normalizations: HashMap::new(),
        diagnostics: Vec::new(),
        next_id: 1,
        linked: false,
        asset_namespace: AssetNamespace::STANDALONE,
        stack: Vec::new(),
        visited_compositions: HashSet::new(),
        animations: Vec::new(),
        animation_budget: AnimationBudget::default(),
        committed_inline_remap_bytes: 0,
        unavailable_cutouts: 0,
        overrides: Vec::new(),
        media_resolver: &mut resolver,
        assets: Vec::new(),
        shape_budget: shapes::OutputBudget::default(),
        mapped_shape_expressions: Default::default(),
        root_progress: Progress::default().phase("alias test", "layers", 0),
    };
    fn text(layers: &[FxLayer], name: &str) -> fx_schema::TextDocument {
        for layer in layers {
            match layer {
                FxLayer::Text(layer) if layer.name.as_str() == name => {
                    return layer.source_text.clone();
                }
                FxLayer::Group(group) if group.name == name => {
                    if let Some(found) = find(&group.layers) {
                        return found;
                    }
                }
                FxLayer::Group(group) => {
                    for child in &group.layers {
                        if let FxLayer::Group(nested) = child.data()
                            && nested.name == name
                        {
                            return find(&nested.layers).unwrap();
                        }
                    }
                }
                _ => {}
            }
        }
        panic!("consumer text {name:?} missing: {layers:?}")
    }
    fn find(layers: &[fx_schema::Layer]) -> Option<fx_schema::TextDocument> {
        layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Text(layer) => Some(layer.source_text.clone()),
            FxLayer::Group(group) => find(&group.layers),
            _ => None,
        })
    }
    let name = comp.layers[owner].name.as_ref();
    let first = text(
        &converter
            .composition_layers(item, LayerId::new(9000), 0)
            .unwrap(),
        name,
    );
    converter.overrides.push(property_override);
    let second = text(
        &converter
            .composition_layers(item, LayerId::new(9001), 0)
            .unwrap(),
        name,
    );
    converter.overrides.clear();
    let third = text(
        &converter
            .composition_layers(item, LayerId::new(9002), 0)
            .unwrap(),
        name,
    );
    assert_eq!(first.text, "Hello World\nSecond Paragraph\nEnd");
    assert_eq!(
        second.text, "New longer\nText here",
        "{:?}",
        converter.diagnostics
    );
    assert_eq!(third, first);
    assert_eq!(first.font_family, second.font_family);
    assert_eq!(first.font_size, second.font_size);
    assert_eq!(first.leading, second.leading);
    assert_eq!(first.box_first_baseline, None);
    assert_eq!(second.box_first_baseline, None);
    let source_after = read_source(&comp.layers[target])
        .unwrap()
        .unwrap()
        .held_document(None, 0)
        .0;
    assert_eq!(source_after.text, "Hello World\nSecond Paragraph\nEnd");
}
