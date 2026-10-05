use super::*;
use crate::{properties, rifx::Chunk};
use sha2::{Digest, Sha256};

fn path_storage(chunks: &[Chunk]) -> Option<&[Chunk]> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == "ADBE Vector Shape" {
                return properties::unique_list(run, *b"om-s").ok();
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(path_storage)
}

fn explicit_input() -> Value {
    let bytes = include_bytes!("../../../tests/fixtures/path-animation/mixed_one_second.fx.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "2b4d70c714b97c9344dfc2896677377fa789f0202513c8ed4f0a9e0c2f331159"
    );
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn mixed_one_second_path_full_export_uses_explicit_owner_local_keys_and_edited_geometry() {
    let value = explicit_input();
    let document = EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let entries = document.composition().dynamics().entries();
    assert_eq!(entries.len(), 2);
    for (layer, entry) in document.composition().layers().iter().zip(entries) {
        assert_eq!(layer.active_range().start.as_millis(), 2_000);
        assert_eq!(layer.active_range().duration.as_millis(), 3_000);
        let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
            panic!("explicit input must retain editable keys")
        };
        assert_eq!(track.keyframes()[0].layer_time().as_millis(), 250);
        assert_eq!(track.keyframes()[1].layer_time().as_millis(), 1_250);
        assert_eq!(
            entry.target,
            fx_schema::PropertyTarget::layer(layer.id(), PropType::ShapePath)
        );
    }
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let owners = layers(&native);
    assert_eq!(owners.len(), 2, "{:?}", output.diagnostics);
    for (owner, expected_name) in owners.iter().zip(["shape base", "shape +30X"]) {
        assert_eq!(owner.name.as_ref(), expected_name);
        assert_eq!(owner.record.start_time(), Some(2.0));
        assert_eq!(owner.record.in_point(), Some(0.0));
        assert_eq!(owner.record.out_point(), Some(3.0));
        let storage = path_storage(&owner.content).unwrap();
        let metadata =
            properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
                .unwrap();
        assert_eq!(metadata.keyframes.len(), 2);
        assert_eq!(metadata.keyframes[0].time_secs, 0.25);
        assert_eq!(metadata.keyframes[1].time_secs, 1.25);
        assert_eq!(metadata.keyframes[0].out_interpolation, 1);
        assert_eq!(metadata.keyframes[1].in_interpolation, 2);
        assert_eq!(metadata.keyframes[1].in_speed, [0.0]);
        assert!((metadata.keyframes[1].in_influence[0] - 90.0).abs() < 1e-10);
    }
    // Re-export an actual input edit, not a rewritten native file or donor replay.
    let mut edited = explicit_input();
    for key in edited["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
    {
        for command in key["value"]["value"]["commands"].as_array_mut().unwrap() {
            for field in ["x", "c1x", "c2x"] {
                if let Some(x) = command.get_mut(field) {
                    *x = json!(x.as_f64().unwrap() + 30.0);
                }
            }
        }
    }
    edited["composition"]["layers"][0]["shape"]["path"] =
        edited["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][0]
            ["value"]["value"]
            .clone();
    let edited_output = export(edited);
    let edited_native = read_project(&edited_output.bytes).unwrap();
    assert_eq!(
        layers(&edited_native).len(),
        2,
        "{:?}",
        edited_output.diagnostics
    );
    assert_eq!(layers(&edited_native)[0].name.as_ref(), "shape base");
    let baseline = path_storage(&layers(&native)[0].content).unwrap();
    let translated = path_storage(&layers(&edited_native)[0].content).unwrap();
    let baseline_keys = properties::unique_list(baseline, *b"omks").unwrap();
    let translated_keys = properties::unique_list(translated, *b"omks").unwrap();
    assert_eq!(baseline_keys.len(), 2);
    assert_eq!(translated_keys.len(), 2);
    for (a, b) in baseline_keys.iter().zip(translated_keys) {
        let a = a.children().unwrap();
        let b = b.children().unwrap();
        // Equal normalized vertex/handle coordinates plus translated X bounds
        // prove that the whole contour moves, not merely its bounding box.
        assert_eq!(
            properties::data(properties::unique_list(a, *b"list").unwrap(), *b"ldat").unwrap(),
            properties::data(properties::unique_list(b, *b"list").unwrap(), *b"ldat").unwrap()
        );
        let a = properties::data(a, *b"shph").unwrap();
        let b = properties::data(b, *b"shph").unwrap();
        let float = |bytes: &[u8], offset| {
            f32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
        };
        assert_eq!(float(b, 4) - float(a, 4), 30.0);
        assert_eq!(float(b, 8), float(a, 8));
        assert_eq!(float(b, 12) - float(a, 12), 30.0);
        assert_eq!(float(b, 16), float(a, 16));
    }
}

#[test]
fn mixed_one_second_path_native_source_clock_is_characterized_without_import_repair() {
    let bytes = include_bytes!("../../../tests/fixtures/path-animation/mixed_one_second.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "2307e4551a9f13646678834be5c124e0a21439caa1f5c2e224e5922b598b74a1"
    );
    let projection =
        include_bytes!("../../../tests/fixtures/path-animation/mixed_one_second_readback.json");
    assert_eq!(
        format!("{:x}", Sha256::digest(projection)),
        "54135f4e38054c617d2c556ada981d5a53a1cb9ac338ace50fa043e4c98fd4cc"
    );
    let native = read_project(bytes).unwrap();
    let composition = &native.item(1).unwrap().kind;
    let crate::structure::ItemKind::Composition(composition) = composition else {
        panic!("pinned comp1 must be a composition")
    };
    let source = composition
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "shape base")
        .unwrap();
    assert_eq!(source.record.start_time(), Some(2.0));
    let storage = path_storage(&source.content).unwrap();
    let keys = properties::read_path_metadata(properties::unique_list(storage, *b"tdbs").unwrap())
        .unwrap()
        .keyframes;
    assert_eq!(keys[0].time_secs, 0.25);
    assert_eq!(keys[1].time_secs, 1.25);
    assert_eq!(
        source.record.start_time().unwrap() + keys[0].time_secs,
        2.25
    );
    assert_eq!(
        source.record.start_time().unwrap() + keys[1].time_secs,
        3.25
    );
    // Preserve the independently authored source's stored clock, including the
    // earlier RED1663. Import admission/clock equivalence is deliberately unchanged.
    let imported = crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    assert!(
        imported
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("mixed"))
    );
    assert!(imported.document.composition().dynamics().entries().iter().all(|entry| {
        !matches!(&entry.target, fx_schema::PropertyTarget::LayerProperty(property) if property.property_type() == PropType::ShapePath)
    }));
}
