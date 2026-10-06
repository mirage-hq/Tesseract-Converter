use super::*;

const NATIVE: &[u8] = include_bytes!("../../../tests/fixtures/essential/multiple_controllers.aep");

fn native_controller(template: &SavedGraphicTemplate, leaf: &str) -> essential::Controller {
    template
        .project
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return None;
            };
            Some(comp.essential_properties.values.iter())
        })
        .flatten()
        .find(|controller| {
            controller
                .path
                .last()
                .is_some_and(|path| path.match_name == leaf)
        })
        .unwrap()
        .clone()
}

fn numeric(
    template: &SavedGraphicTemplate,
    controller: &essential::Controller,
) -> properties::NumericProperty {
    let ItemKind::Composition(comp) = &template
        .project
        .item(controller.source_comp_id.unwrap())
        .unwrap()
        .kind
    else {
        panic!()
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == controller.source_layer_id.unwrap())
        .unwrap();
    match storage(layer, &controller.path) {
        Ok(storage) => {
            properties::read_numeric(properties::unique_list(storage, *b"tdbs").unwrap()).unwrap()
        }
        Err(_) => {
            effect_default(
                layer,
                &controller.path,
                [f64::from(comp.width), f64::from(comp.height)],
            )
            .unwrap()
            .unwrap()
            .numeric
        }
    }
}

#[test]
fn capsule_native_uuid_numeric_override_changes_only_instance_property() {
    let template = SavedGraphicTemplate::decode(NATIVE).unwrap();
    let opacity = native_controller(&template, "ADBE Opacity");
    assert_eq!(
        (opacity.source_comp_id, opacity.source_layer_id),
        (Some(1), Some(15))
    );
    let original = template.source().clone();
    let (mut instance, id, _) = template.instantiate(&[&opacity.uuid]).unwrap();
    assert_eq!(id, 1);
    instance
        .apply_numeric(&opacity.uuid, &SavedGraphicNumeric::Scalar(37.0))
        .unwrap();
    assert_eq!(numeric(&instance, &opacity).values, vec![37.0]);
    assert_eq!(template.source(), &original);
}

#[test]
fn capsule_explicit_composition_rejects_forged_text_bindings() {
    let template = SavedGraphicTemplate::decode(NATIVE).unwrap();
    let opacity = native_controller(&template, "ADBE Opacity");
    let mut text = SavedGraphicText {
        controller_uuid: opacity.uuid,
        composition_id: opacity.source_comp_id.unwrap(),
        layer_id: opacity.source_layer_id.unwrap(),
        document: serde_json::from_value(serde_json::json!({
            "text":"forged","fontFamily":"ArialMT","fontStyle":"",
            "fontSize":24.0,"fillColor":[0.0,0.0,0.0,1.0]
        }))
        .unwrap(),
        postscript_font: "ArialMT".into(),
        diagnostics: Vec::new(),
    };
    // A real numeric-controller UUID is not a Source Text binding.
    assert!(
        template
            .editable_layers_in_composition(1, std::slice::from_ref(&text))
            .is_err()
    );
    let error = template
        .editable_layers_in_composition(2, std::slice::from_ref(&text))
        .unwrap_err();
    assert!(
        error.to_string().contains("outside requested composition"),
        "{error}"
    );
    text.controller_uuid = "absent UUID".into();
    assert!(template.editable_layers_in_composition(1, &[text]).is_err());
}

#[test]
fn capsule_legacy_composition_selection_rejects_spanning_values() {
    let template = SavedGraphicTemplate::decode(NATIVE).unwrap();
    let document = serde_json::from_value(serde_json::json!({
        "text":"synthetic override","fontFamily":"ArialMT","fontStyle":"",
        "fontSize":24.0,"fillColor":[0.0,0.0,0.0,1.0]
    }))
    .unwrap();
    let first = SavedGraphicText {
        controller_uuid: "synthetic first".into(),
        composition_id: 1,
        layer_id: 1,
        document,
        postscript_font: "ArialMT".into(),
        diagnostics: Vec::new(),
    };
    let mut second = first.clone();
    second.composition_id = 2;
    second.controller_uuid = "synthetic second".into();
    let error = template.editable_layers(&[first, second]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("controllers span several compositions"),
        "{error}"
    );
}

#[test]
fn capsule_sparse_native_point_default_keeps_relative_framing_and_pixel_value() {
    let mut template = SavedGraphicTemplate::decode(include_bytes!(
        "../../../tests/fixtures/effects/static_point_controls.aep"
    ))
    .unwrap();
    let mut point = None;
    for item in &template.project.items {
        let ItemKind::Composition(comp) = &item.kind else {
            continue;
        };
        let size = [f64::from(comp.width), f64::from(comp.height)];
        for layer in &comp.layers {
            let (effects, _) = crate::effects::native::read_effects(&layer.content, size);
            if let Some((effect, parameter)) = effects.iter().find_map(|effect| {
                effect
                    .parameters
                    .iter()
                    .find(|parameter| parameter.declared_kind == Ok(Some(6)))
                    .map(|parameter| (effect, parameter))
            }) {
                point = Some(essential::Controller {
                    uuid: "synthetic-native-point".into(),
                    controller_type: 2,
                    source_comp_id: Some(item.id),
                    source_layer_id: Some(layer.record.id()),
                    path: vec![
                        essential::SourcePropertyRef {
                            match_name: "ADBE Effect Parade".into(),
                            child_index: None,
                        },
                        essential::SourcePropertyRef {
                            match_name: effect.match_name.clone(),
                            child_index: Some(u32::try_from(effect.index - 1).unwrap()),
                        },
                        essential::SourcePropertyRef {
                            match_name: parameter.match_name.clone(),
                            child_index: None,
                        },
                    ],
                });
                break;
            }
        }
        if point.is_some() {
            break;
        }
    }
    let point = point.expect("native fixture declares a point parameter");
    let name = &point.path.last().unwrap().match_name;
    let ItemKind::Composition(comp) = &mut template
        .project
        .items
        .iter_mut()
        .find(|item| item.id == point.source_comp_id.unwrap())
        .unwrap()
        .kind
    else {
        panic!()
    };
    let size = [f64::from(comp.width), f64::from(comp.height)];
    let layer = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == point.source_layer_id.unwrap())
        .unwrap();
    // Supplemental mutation removes only explicit storage. The native parameter's
    // declaration/default remains; its exposing controller UUID is synthetic.
    fn remove_explicit(chunks: &mut Vec<Chunk>, name: &str) {
        for chunk in chunks.iter_mut() {
            if chunk.list_kind() != Some(*b"parT")
                && let Some(children) = chunk.children_mut()
            {
                remove_explicit(children, name);
            }
        }
        let marker = format!("{name}\0");
        if let Some(start) = chunks.iter().position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(marker.as_bytes()))
        }) {
            let end = (start + 1..chunks.len())
                .find(|index| chunks[*index].id() == *b"tdmn")
                .unwrap_or(chunks.len());
            chunks.drain(start..end);
        }
    }
    remove_explicit(&mut layer.content, name);
    assert!(storage(layer, &point.path).is_err());
    let original = effect_default(layer, &point.path, size).unwrap().unwrap();
    assert!(original.effect_point);
    assert_eq!(original.numeric.values.len(), 2);
    let (before, _) = crate::effects::native::read_effects(&layer.content, size);
    comp.essential_properties.values.push(point.clone());
    template
        .apply_numeric(&point.uuid, &SavedGraphicNumeric::Point([64.0, 32.0]))
        .unwrap();
    let ItemKind::Composition(comp) = &template
        .project
        .item(point.source_comp_id.unwrap())
        .unwrap()
        .kind
    else {
        panic!()
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == point.source_layer_id.unwrap())
        .unwrap();
    let leaf = properties::unique_list(storage(layer, &point.path).unwrap(), *b"tdbs").unwrap();
    assert_eq!(properties::data(leaf, *b"tdb4").unwrap()[59], 4);
    let encoded: Vec<_> = [64.0 / size[0], 32.0 / size[1]]
        .into_iter()
        .flat_map(f64::to_be_bytes)
        .collect();
    assert_eq!(properties::data(leaf, *b"cdat").unwrap(), encoded);
    assert_eq!(
        properties::read_effect_point(leaf).unwrap().values,
        vec![64.0 / size[0], 32.0 / size[1]]
    );
    let (after, _) = crate::effects::native::read_effects(&layer.content, size);
    let decoded = after
        .iter()
        .flat_map(|effect| &effect.parameters)
        .find(|parameter| parameter.match_name == *name)
        .unwrap();
    assert_eq!(decoded.numeric.as_ref().unwrap().values, vec![64.0, 32.0]);
    let siblings = |effects: Vec<crate::effects::native::DecodedEffect>| {
        effects
            .into_iter()
            .flat_map(|effect| effect.parameters)
            .filter(|parameter| parameter.match_name != *name)
            .collect::<Vec<_>>()
    };
    assert_eq!(siblings(before), siblings(after));
    // Updating the now-explicit point must use the same source-pixel units.
    template
        .apply_numeric(&point.uuid, &SavedGraphicNumeric::Point([128.0, 72.0]))
        .unwrap();
    let ItemKind::Composition(comp) = &template
        .project
        .item(point.source_comp_id.unwrap())
        .unwrap()
        .kind
    else {
        panic!()
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == point.source_layer_id.unwrap())
        .unwrap();
    let (effects, _) = crate::effects::native::read_effects(&layer.content, size);
    let point = effects
        .iter()
        .flat_map(|effect| &effect.parameters)
        .find(|parameter| parameter.match_name == *name)
        .unwrap();
    assert_eq!(point.numeric.as_ref().unwrap().values, vec![128.0, 72.0]);
}

#[test]
fn capsule_native_colour_override_preserves_known_alpha_and_siblings() {
    let template = SavedGraphicTemplate::decode(NATIVE).unwrap();
    let colour = native_controller(&template, "ADBE Fill-0002");
    let brightness = native_controller(&template, "ADBE Brightness & Contrast 2-0001");
    let expected = numeric(&template, &colour).values;
    let sibling = numeric(&template, &brightness).values;
    let (mut instance, _, _) = template.instantiate(&[&colour.uuid]).unwrap();
    instance
        .apply_numeric(
            &colour.uuid,
            &SavedGraphicNumeric::ColourRgb([0.0, 1.0, 0.25]),
        )
        .unwrap();
    assert_eq!(
        numeric(&instance, &colour).values,
        vec![0.0, 1.0, 0.25, expected[3]]
    );
    assert_eq!(numeric(&instance, &brightness).values, sibling);
    assert_eq!(numeric(&template, &colour).values, expected);
}

#[test]
fn capsule_unsupported_numeric_override_retains_native_property_and_binding_guards() {
    let mut template = SavedGraphicTemplate::decode(NATIVE).unwrap();
    let colour = native_controller(&template, "ADBE Fill-0002");
    let original = template.source().clone();
    assert!(
        template
            .apply_numeric(&colour.uuid, &SavedGraphicNumeric::Scalar(5.0))
            .is_err()
    );
    assert!(
        template
            .apply_numeric("absent UUID", &SavedGraphicNumeric::Scalar(5.0))
            .is_err()
    );
    assert!(
        template
            .apply_numeric(
                &colour.uuid,
                &SavedGraphicNumeric::ColourRgb([f64::NAN, 0.0, 0.0])
            )
            .is_err()
    );
    assert_eq!(template.source(), &original);
    let ItemKind::Composition(comp) = &mut template
        .project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    comp.essential_properties.values.push(colour.clone());
    assert!(template.instantiate(&[&colour.uuid]).is_err());
    assert!(
        template
            .apply_numeric(&colour.uuid, &SavedGraphicNumeric::Scalar(5.0))
            .is_err()
    );
}

#[test]
fn capsule_picture_numeric_snapshot_reaches_the_full_editable_mapper() {
    let template = SavedGraphicTemplate::decode(NATIVE).unwrap();
    let original = template.source().clone();
    let opacity = native_controller(&template, "ADBE Opacity");
    let (mut instance, composition, _) = template.instantiate(&[&opacity.uuid]).unwrap();
    // This API uses native units: AEP Transform Opacity stores a fraction.
    instance
        .apply_numeric(&opacity.uuid, &SavedGraphicNumeric::Scalar(0.37))
        .unwrap();
    let mut picture = instance
        .import_editable_picture(
            std::path::Path::new("native.aep"),
            composition,
            100,
            "capsule-snapshot",
            &[],
        )
        .unwrap();
    let document = picture.take_document().unwrap();
    fn has_opacity(layers: &[fx_schema::Layer]) -> bool {
        layers.iter().any(|layer| {
            matches!(layer.data(), fx_schema::LayerData::Group(group)
            if group.transform.opacity.value() == 37.0 || has_opacity(&group.layers))
        })
    }
    assert!(
        has_opacity(document.composition().layers()),
        "picture: {}",
        serde_json::to_string(&document).unwrap()
    );
    assert_eq!(template.source(), &original);
}

#[test]
fn capsule_picture_saved_child_text_uses_native_identity_not_layer_names() {
    // Supplemental typed assembly of two unchanged native fixture payloads.
    // This tests the validated Text hook, not an independently authored oracle.
    let mut template = SavedGraphicTemplate::decode(include_bytes!(
        "../../../tests/fixtures/layers/import_precomp_structure.aep"
    ))
    .unwrap();
    let point = SavedGraphicTemplate::decode(include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/text_document_point.aep"
    ))
    .unwrap();
    let ItemKind::Composition(source) = &point
        .project
        .items
        .iter()
        .find(|item| matches!(&item.kind, ItemKind::Composition(_)))
        .unwrap()
        .kind
    else {
        panic!()
    };
    let text_layer = source
        .layers
        .iter()
        .find(|layer| layer.record.layer_type() == 3)
        .unwrap()
        .clone();
    let layer_id = text_layer.record.id();
    let ItemKind::Composition(child) = &mut template
        .project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    child.layers = vec![text_layer];
    let ItemKind::Composition(parent) = &mut template
        .project
        .items
        .iter_mut()
        .find(|item| item.id == 31)
        .unwrap()
        .kind
    else {
        panic!()
    };
    parent
        .essential_properties
        .values
        .push(essential::Controller {
            uuid: "capsule-child-text".into(),
            controller_type: 6,
            source_comp_id: Some(1),
            source_layer_id: Some(layer_id),
            path: vec![
                essential::SourcePropertyRef {
                    match_name: "ADBE Text Properties".into(),
                    child_index: None,
                },
                essential::SourcePropertyRef {
                    match_name: "ADBE Text Document".into(),
                    child_index: None,
                },
            ],
        });
    let mut saved = template.text("capsule-child-text").unwrap();
    saved
        .set_value("Saved child 🦊", "ArialMT", 48.0, false)
        .unwrap();
    let mut picture = template
        .import_editable_picture(
            std::path::Path::new("native.aep"),
            31,
            100,
            "capsule-child",
            &[saved.clone()],
        )
        .unwrap();
    let document = picture.take_document().unwrap();
    fn text_count(layers: &[fx_schema::Layer]) -> usize {
        layers
            .iter()
            .map(|layer| match layer.data() {
                fx_schema::LayerData::Group(group) => text_count(&group.layers),
                fx_schema::LayerData::Text(text) => {
                    assert_eq!(text.source_text.text, "Saved child 🦊");
                    assert_eq!(text.source_text.font_size.value(), 48.0);
                    1
                }
                _ => 0,
            })
            .sum()
    }
    assert_eq!(text_count(document.composition().layers()), 2);
    let mut forged = saved.clone();
    forged.layer_id += 1;
    assert!(
        template
            .import_editable_picture(
                std::path::Path::new("native.aep"),
                31,
                100,
                "capsule-child",
                &[forged]
            )
            .is_err()
    );
    assert!(
        template
            .import_editable_picture(
                std::path::Path::new("native.aep"),
                31,
                100,
                "capsule-child",
                &[saved.clone(), saved]
            )
            .is_err()
    );
}
