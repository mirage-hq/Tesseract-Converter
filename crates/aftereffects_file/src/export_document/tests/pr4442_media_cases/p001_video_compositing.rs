//! Source-derived P001 Video compositing admission regression.
use super::*;

fn video_compositing_document(end_opacity: f64) -> EditableFxCompositionDocument {
    let mut layers = Vec::new();
    for (id, name, mode, provider) in [
        (21, "Luma video", Some("luma"), 111),
        (22, "Inverted video", Some("lumaInverted"), 112),
        (23, "Screen video", None, 0),
    ] {
        let mut owner = video(id, "movie");
        owner["name"] = json!(name);
        owner.as_object_mut().unwrap().remove("activeRange");
        owner["sourceRange"] = json!({"start":0,"duration":2000});
        owner["sourceIntrinsicDuration"] = json!(8000);
        owner["playback"] = fixture_linear_playback(
            json!({"start":0,"duration":2000}),
            json!({"start":0,"duration":2000}),
        );
        if let Some(mode) = mode {
            owner["trackMatte"] = json!({"layer":provider,"mode":mode});
            let mut matte = rect(&imported(), provider);
            matte["name"] = json!(if provider == 111 {
                "Luma matte"
            } else {
                "Inverted matte"
            });
            matte["transform"] = transform();
            matte["activeRange"] = json!({"start":0,"duration":2000});
            matte["rect"]["size"] = json!([320, 180]);
            layers.push(matte);
        } else {
            owner["blendMode"] = json!("screen");
        }
        layers.push(owner);
    }
    let entry = serde_json::from_value(json!({
        "target":{"kind":"layer","layerId":21,"propertyType":"opacity"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"p001-opacity-start","layerTime":0,"value":{"type":"float","value":100},"easing":{"type":"linear"}},
            {"id":"p001-opacity-end","layerTime":1000,"value":{"type":"float","value":end_opacity},"easing":{"type":"linear"}}
        ]}
    })).unwrap();
    let mut value = document(layers, vec![entry]).to_json_value().unwrap();
    value["dimensions"] = json!({"width":320,"height":180});
    value["duration"] = json!(2);
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

#[test]
fn p001_native_video_luma_screen_source_and_edited_export() {
    let source = include_bytes!("../../../../tests/fixtures/properties/p001_video_compositing.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        "653e0fcdbd8d580689481cbde1241b17f38fedc1baac227c1622fc34c6e0731b"
    );
    let native = read_project(source).unwrap();
    let ItemKind::Composition(comp) = &native.item(2).unwrap().kind else {
        panic!("pinned native comp2");
    };
    assert_eq!(comp.layers.len(), 5);
    for (name, mode) in [("Luma video", 3), ("Inverted video", 4)] {
        let layer = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == name)
            .unwrap();
        assert_eq!(layer.record.track_matte_type(), mode);
        assert!(layer.record.matte_layer_id().is_some());
    }
    let screen = comp
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Screen video")
        .unwrap();
    assert_eq!(screen.record.blend_mode(), 6);
    let imported = crate::structure_document::to_structural_fx_document_with_assets(
        &native,
        Some(2),
        &mut |_| true,
    )
    .unwrap();
    let imported_json = imported.document.to_json_value().unwrap();
    let occurrences = imported_json["composition"]["layers"][0]["layers"]
        .as_array()
        .unwrap();
    for (name, mode) in [("Luma video", "luma"), ("Inverted video", "lumaInverted")] {
        let occurrence = occurrences
            .iter()
            .find(|layer| layer["name"] == name)
            .unwrap();
        assert_eq!(occurrence["trackMatte"]["mode"], mode);
    }
    assert_eq!(
        occurrences
            .iter()
            .find(|layer| layer["name"] == "Screen video")
            .unwrap()["blendMode"],
        "screen"
    );
    let sources = BTreeMap::from([(
        "movie".into(),
        resolved(
            "movie",
            "media/movie.mov",
            NativeSourceFormat::QuickTime,
            [320, 180],
            8000,
            NativeFrameRate::integer(24),
            48000.0,
        ),
    )]);
    for end in [50.0, 20.0] {
        let output = to_aep_with_media(&video_compositing_document(end), &sources).unwrap();
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let comp = native
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .unwrap();
        assert_eq!(comp.layers.len(), 5);
        let luma = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Luma video")
            .unwrap();
        let opacity = super::super::stroke_keys::numeric(&luma.content, "ADBE Opacity").unwrap();
        assert_eq!(opacity.keyframes.len(), 2);
        assert_eq!(opacity.keyframes[0].values, [1.0]);
        assert_eq!(opacity.keyframes[1].values, [end / 100.0]);
        for (name, mode, provider_name) in [
            ("Luma video", 3, "Luma matte"),
            ("Inverted video", 4, "Inverted matte"),
        ] {
            let owner = comp
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == name)
                .unwrap();
            let provider = comp
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == provider_name)
                .unwrap();
            assert_eq!(owner.record.track_matte_type(), mode);
            assert_eq!(owner.record.matte_layer_id(), Some(provider.record.id()));
            assert!(!provider.record.flags().enabled);
            assert_ne!(owner.record.source_id(), 0);
        }
        assert_eq!(
            comp.layers
                .iter()
                .find(|layer| layer.name.as_ref() == "Screen video")
                .unwrap()
                .record
                .blend_mode(),
            6
        );
    }
}
