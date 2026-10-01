//! Supplementary editable-export regressions, not independent Adobe render proof.

use super::*;

fn all_layers(native: &StructuralProject) -> impl Iterator<Item = &crate::structure::Layer> {
    native.items.iter().flat_map(|item| match &item.kind {
        ItemKind::Composition(comp) => comp.layers.as_slice(),
        _ => &[],
    })
}

fn named<'a>(native: &'a StructuralProject, name: &str) -> &'a crate::structure::Layer {
    all_layers(native)
        .find(|layer| layer.name.as_ref() == name)
        .unwrap()
}

#[test]
fn consumed_rect_mattes_are_sampleable_without_painting_over_their_owners() {
    for (mode, native_mode) in [
        ("alpha", 1),
        ("alphaInverted", 2),
        ("luma", 3),
        ("lumaInverted", 4),
    ] {
        let mut value = imported();
        let provider = rect(&value, 700);
        let mut owner = rect(&value, 701);
        owner["trackMatte"] = json!({"mode": mode, "layer": 700});
        let unrelated = rect(&value, 702);
        value["composition"]["layers"] = json!([provider, owner, unrelated]);
        value["composition"]["dynamics"] = json!({"entries": []});
        let native = read_project(&export(value).bytes).unwrap();
        let provider = named(&native, "Current solid 700");
        let owner = named(&native, "Current solid 701");
        assert!(
            !provider.record.flags().enabled,
            "{mode} provider must not paint independently"
        );
        assert!(owner.record.flags().enabled);
        assert!(named(&native, "Current solid 702").record.flags().enabled);
        assert_eq!(owner.record.matte_layer_id(), Some(provider.record.id()));
        assert_eq!(owner.record.track_matte_type(), native_mode);
        let opacity = crate::properties::read_transform(&provider.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Opacity")
            .unwrap()
            .numeric
            .unwrap();
        assert_eq!(
            opacity.values,
            [1.0],
            "hiding must not zero the sampled alpha"
        );
    }
}

#[test]
fn shared_group_matte_hides_only_the_occurrence_not_its_source_children() {
    let mut value = imported();
    let child = rect(&value, 710);
    let mut transform = json!(identity_fx_transform());
    transform["opacity"] = json!(75.0);
    let provider = json!({"type":"Group", "id":711, "name":"Shared matte",
        "playback":fixture_linear_playback(json!({"start":0,"duration":1000}), json!({"start":0,"duration":1000})),
        "transform":transform, "layers":[child]});
    let mut first = rect(&value, 712);
    first["trackMatte"] = json!({"mode":"alpha","layer":711});
    let mut second = rect(&value, 713);
    second["trackMatte"] = json!({"mode":"luma","layer":711});
    value["composition"]["layers"] = json!([provider, first, second]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let native = read_project(&export(value).bytes).unwrap();
    let provider = named(&native, "Shared matte");
    assert!(!provider.record.flags().enabled);
    assert_ne!(provider.record.source_id(), 0);
    for name in ["Current solid 712", "Current solid 713"] {
        let owner = named(&native, name);
        assert!(owner.record.flags().enabled);
        assert_eq!(owner.record.matte_layer_id(), Some(provider.record.id()));
    }
    let ItemKind::Composition(source) = &native.item(provider.record.source_id()).unwrap().kind
    else {
        panic!("matte must keep its editable precomposition");
    };
    assert!(source.layers.iter().any(|layer| layer.name.as_ref() == "Current solid 710" && layer.record.flags().enabled));
}

#[test]
fn animated_multichild_group_matte_retains_composited_source_and_inverted_link() {
    let mut value = imported();
    let full_span = value["composition"]["layers"][0]["playback"]["inputRange"].clone();
    let mut gaps = Vec::new();
    for id in 40050..=40056 {
        let mut gap = rect(&value, id);
        gap["name"] = json!(format!("Sun gap {id}"));
        gap["activeRange"] = full_span.clone();
        gaps.push(gap);
    }
    let provider = json!({"type":"Group", "id":40042, "name":"Sun stripe matte",
        "playback":fixture_linear_playback(full_span.clone(), full_span),
        "transform":identity_fx_transform(), "layers":gaps});
    let mut owner = rect(&value, 40043);
    owner["name"] = json!("Sun disc");
    owner["trackMatte"] = json!({"mode":"alphaInverted","layer":40042});
    value["composition"]["layers"] = json!([provider, owner]);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(
            LayerId::new(40050),
            PropType::PositionY,
            [(0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(10.0))],
        )
    ]});

    let mut without_matte = value.clone();
    without_matte["composition"]["layers"][1]["trackMatte"] = Value::Null;
    let unreferenced = read_project(&export(without_matte).bytes).unwrap();
    assert!(
        named(&unreferenced, "Sun stripe matte")
            .record
            .flags()
            .null_layer
    );

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let provider = named(&native, "Sun stripe matte");
    let owner = named(&native, "Sun disc");
    assert!(
        !provider.record.flags().enabled,
        "matte must not paint independently"
    );
    assert!(
        !provider.record.flags().null_layer,
        "a Null is not a sampled composite"
    );
    assert_ne!(
        provider.record.source_id(),
        0,
        "the seven gaps need a composited source"
    );
    assert_eq!(owner.record.matte_layer_id(), Some(provider.record.id()));
    assert_eq!(owner.record.track_matte_type(), 2);
    let ItemKind::Composition(source) = &native.item(provider.record.source_id()).unwrap().kind
    else {
        panic!("matte provider must reference a composition");
    };
    assert_eq!(source.layers.len(), 7);
    assert!(
        source
            .layers
            .iter()
            .all(|child| child.record.flags().enabled)
    );
    assert!(
        source
            .layers
            .iter()
            .any(|child| child.name.as_ref() == "Sun gap 40050")
    );
}

#[test]
fn native_alpha_and_luma_matte_inputs_keep_hidden_sampleable_export_providers() {
    let source = read_project(include_bytes!(
        "../../../tests/fixtures/compositing/trackMatteType.aep"
    ))
    .unwrap();
    for (composition, provider_id) in [(1, 15), (18, 31)] {
        let ItemKind::Composition(native_comp) = &source.item(composition).unwrap().kind else {
            panic!("composition")
        };
        assert!(
            !native_comp
                .layers
                .iter()
                .find(|layer| layer.record.id() == provider_id)
                .unwrap()
                .record
                .flags()
                .enabled
        );
        let mut value = to_structural_fx_document(&source, Some(composition))
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
        // FX consumes referenced matte layers regardless of their display switch.
        // Make that explicit in this edited export input; the native oracle hides them.
        fn show(layers: &mut Value) {
            for layer in layers.as_array_mut().unwrap() {
                layer["isHidden"] = json!(false);
                if layer.get("layers").is_some() {
                    show(&mut layer["layers"]);
                }
            }
        }
        show(&mut value["composition"]["layers"]);
        let native = read_project(&export(value).bytes).unwrap();
        let mut matte_count = 0;
        for item in &native.items {
            let ItemKind::Composition(comp) = &item.kind else {
                continue;
            };
            for owner in &comp.layers {
                let Some(provider_id) = owner.record.matte_layer_id() else {
                    continue;
                };
                let provider = comp
                    .layers
                    .iter()
                    .find(|layer| layer.record.id() == provider_id)
                    .unwrap();
                assert!(!provider.record.flags().enabled, "{}", provider.name);
                matte_count += 1;
            }
        }
        assert!(
            matte_count > 0,
            "native target {composition} must retain its matte"
        );
    }
}
