use fx_schema::{EditableFxCompositionDocument, Layer, LayerData, LayerId, TextLayer};
use serde_json::{Value, json};

use super::{ArchiveFonts, inferred_faces};
use crate::export_document::{AnimationIndex, ExportDocumentViews};

fn fonts() -> ArchiveFonts {
    ArchiveFonts {
        faces: inferred_faces(include_bytes!(
            "../../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf"
        )),
    }
}

fn fixture() -> Value {
    let playback = json!({"type": "windowed",
        "inputRange": {"start": 0, "duration": 2000},
        "mapping": {"type": "linear", "input": {"start": 0, "duration": 2000},
            "output": {"start": 0, "duration": 2000}}, "inputOffsetMs": 0});
    let transform = json!({"position": [0, 0], "anchorPoint": [0, 0],
        "scale": [100, 100], "rotation": 0, "opacity": 100});
    let text = json!({"type": "Text", "id": 101, "name": "Retained prompt", "parent": 100,
        "activeRange": {"start": 0, "duration": 2000}, "transform": transform,
        "sourceText": {"text": "KEEP", "fontFamily": "Arial", "fontStyle": "Bold",
            "fontSize": 48, "fillColor": [1, 1, 1, 1]}});
    let mut other_text = text.clone();
    other_text["id"] = 201.into();
    other_text["parent"] = Value::Null;
    other_text["name"] = "Independent Text layout".into();
    other_text["sourceText"]["text"] = "ENABLE".into();
    let paint = json!({"type": "Rect", "id": 102, "name": "Retained button", "parent": 100,
        "activeRange": {"start": 0, "duration": 2000}, "transform": transform,
        "rect": {"size": [120, 60], "fillColor": [0.1, 0.8, 0.7, 1]}});
    let mut sibling = paint.clone();
    sibling["id"] = 103.into();
    sibling["parent"] = Value::Null;
    sibling["name"] = "Independent sibling".into();
    json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1, "dimensions": {"width": 1280, "height": 720}, "duration": 2.0,
        "composition": {"id": "main", "name": "Text bounds retention",
            "dynamics": {"entries": []}, "layers": [{
                "type": "Group", "id": 100, "name": "Mixed source",
                "playback": playback,
                "transform": {"position": [30, 20], "anchorPoint": [0, 0],
                    "scale": [100, 100], "rotation": 0, "opacity": 50},
                "layers": [text, {"type": "Group", "id": 200, "name": "Dialog", "parent": 100,
                    "playback": playback,
                    "transform": transform, "layers": [other_text]}, paint]
            }, sibling]}
    })
}

fn text_layers(layers: &[Layer]) -> Vec<&TextLayer> {
    layers
        .iter()
        .flat_map(|layer| match layer.data() {
            LayerData::Text(text) => vec![text],
            LayerData::Group(group) => text_layers(&group.layers),
            _ => Vec::new(),
        })
        .collect()
}

fn has_layer(layers: &[Layer], name: &str) -> bool {
    layers.iter().any(|layer| {
        layer.name() == name
            || matches!(layer.data(), LayerData::Group(group) if has_layer(&group.layers, name))
    })
}

#[test]
fn emitted_point_text_holds_and_constant_size_retain_native_source_text() {
    let fonts = fonts();
    let mut fixture = fixture();
    let prompt = &mut fixture["composition"]["layers"][0]["layers"][0]["sourceText"];
    prompt["text"] = "".into();
    prompt["fontSize"] = 12.into();
    fixture["composition"]["dynamics"]["entries"] = json!([
        {"target": {"kind": "layer", "layerId": 101, "propertyType": "textContent"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "empty-prompt", "layerTime": 0,
                    "value": {"type": "string", "value": ""}, "easing": {"type": "hold"}},
                {"id": "typed-prompt", "layerTime": 1000,
                    "value": {"type": "string", "value": "KEEP"}, "easing": {"type": "hold"}}
            ]}},
        {"target": {"kind": "layer", "layerId": 101, "propertyType": "fontSize"},
            "animator": {"type": "constant", "value": {"type": "float", "value": 84}}}
    ]);
    let document = EditableFxCompositionDocument::from_json_value(fixture).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("mixed Group fixture")
    };
    let dynamics = AnimationIndex::new(document.composition().dynamics().entries());
    let projected = fonts.bounds_geometry(group, &dynamics).unwrap();
    assert!(matches!(projected.layers[0].data(), LayerData::Rect(rect)
        if !rect.is_hidden && rect.rect.size[0] > 0.0 && rect.rect.size[1] > 12.0));
    let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
        ExportDocumentViews::unchanged(&document).with_fonts(&fonts),
        &Default::default(),
        30.0,
    )
    .unwrap();
    assert!(
        output.omitted_layer_ids.is_empty(),
        "{:?}",
        output.diagnostics
    );
    let native = crate::structure::read_project(&output.bytes).unwrap();
    let imported = crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    let texts = text_layers(imported.document.composition().layers());
    assert!(texts.iter().any(|text| text.source_text.text.is_empty()));
    assert!(texts.iter().any(|text| {
        text.source_text.text == "KEEP"
            && text.source_text.font_size.value() == 84.0
            && text.source_text.font_family.as_ref() == "Arial"
            && text.source_text.font_style.as_ref() == "BoldMT"
    }));
    assert!(has_layer(
        imported.document.composition().layers(),
        "Retained button"
    ));
    assert!(has_layer(
        imported.document.composition().layers(),
        "Independent sibling"
    ));
}

#[test]
fn mixed_point_and_box_text_use_outline_and_established_bulge_domains() {
    let fonts = fonts();
    let mut fixture = fixture();
    fixture["composition"]["layers"][0]["motionBlur"] = true.into();
    let dialog = &mut fixture["composition"]["layers"][0]["layers"][1];
    dialog["effects"] = json!([{"id": 210, "enabled": true, "effect": {
        "type": "bulge", "centerX": 0.5, "centerY": 0.5,
        "horizontalRadius": 0.6, "verticalRadius": 0.6, "bulgeHeight": 0.1, "pinning": false
    }}]);
    let text = &mut dialog["layers"][0]["sourceText"];
    text["boxText"] = true.into();
    text["boxSize"] = json!([240, 68]);
    text["boxPosition"] = json!([0, 0]);
    text["justification"] = "center".into();
    text["verticalAlign"] = "center".into();
    let document = EditableFxCompositionDocument::from_json_value(fixture).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("mixed Group fixture")
    };
    let projected = fonts
        .bounds_geometry(group, &AnimationIndex::new(&[]))
        .unwrap();
    assert!(matches!(projected.layers[0].data(), LayerData::Rect(_)));
    let LayerData::Group(dialog) = projected.layers[1].data() else {
        panic!("dialog Group")
    };
    assert!(matches!(dialog.layers[0].data(), LayerData::Text(text) if text.source_text.box_text));
    let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
        ExportDocumentViews::unchanged(&document).with_fonts(&fonts),
        &Default::default(),
        30.0,
    )
    .unwrap();
    assert!(
        output.omitted_layer_ids.is_empty(),
        "{:?}",
        output.diagnostics
    );
    let native = crate::structure::read_project(&output.bytes).unwrap();
    let imported = crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    let texts = text_layers(imported.document.composition().layers());
    assert!(texts.iter().any(|text| text.source_text.text == "KEEP"));
    assert!(
        texts
            .iter()
            .any(|text| text.source_text.text == "ENABLE" && text.source_text.box_text)
    );
    assert!(has_layer(
        imported.document.composition().layers(),
        "Retained button"
    ));
    assert!(has_layer(
        imported.document.composition().layers(),
        "Independent sibling"
    ));
}

#[test]
fn text_only_recovery_keeps_verified_text_and_omits_only_unbounded_sibling() {
    let fonts = fonts();
    let mut fixture = fixture();
    fixture["composition"]["layers"][0]["layers"][1]["layers"][0]["sourceText"]["fontFamily"] =
        "Unavailable physical face".into();
    // This consumer samples beyond one pixel, so unknown Text cannot borrow a
    // viewport. The independently qualified prompt still has real outlines.
    fixture["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type": "Adjustment", "id": 104, "name": "Nonpointwise consumer",
            "activeRange": {"start": 0, "duration": 2000},
            "transform": {"position": [0, 0], "anchorPoint": [0, 0],
                "scale": [100, 100], "rotation": 0, "opacity": 100},
            "effects": [{"id": 105, "enabled": true,
                "effect": {"type": "gaussianBlur", "blurriness": 7}}]
        }));
    let document = EditableFxCompositionDocument::from_json_value(fixture).unwrap();
    let output = crate::export_document::to_aep_with_document_views_and_media_and_fps(
        ExportDocumentViews::unchanged(&document).with_fonts(&fonts),
        &Default::default(),
        30.0,
    )
    .unwrap();
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(100)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(101)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(102)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(103)));
    assert!(output.omitted_layer_ids.contains(&LayerId::new(201)));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(200))
            && diagnostic.message.contains("Text-only branch omitted")
    }));
    let native = crate::structure::read_project(&output.bytes).unwrap();
    let imported = crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    let texts = text_layers(imported.document.composition().layers());
    assert!(texts.iter().any(|text| text.source_text.text == "KEEP"));
    assert!(!texts.iter().any(|text| text.source_text.text == "ENABLE"));
    assert!(has_layer(
        imported.document.composition().layers(),
        "Retained button"
    ));
    assert!(has_layer(
        imported.document.composition().layers(),
        "Independent sibling"
    ));
}
