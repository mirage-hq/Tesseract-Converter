//! Nested typewriter preparation through the native Source Text/font export seam.
//! Supplementary CPU structure assertions; no independent Adobe render oracle.

use super::*;
use fx_conv::ExportFromTesseract;
use tesseract_file::AssetKind;

const TYPEWRITER: &str = "const t=input.time.milliseconds*2; if(t<500)return 'ABC'.slice(0,Math.max(0,Math.floor((t-100)/100))); if(t<900)return 'XYZ'.slice(0,Math.max(0,Math.floor((t-500)/100))); return '';";

fn text_control(layers: &[Layer]) -> Option<&Layer> {
    layers.iter().find_map(|layer| {
        if matches!(layer.data(), LayerData::Text(_)) {
            Some(layer)
        } else {
            layer.child_layers().and_then(text_control)
        }
    })
}

fn nested_typewriter() -> EditableFxCompositionDocument {
    let source = read_project(include_bytes!(
        "../../../../../tests/fixtures/text_controls_native_panel/native/fx-export-panel-text-style-hold.aep"
    ))
    .unwrap();
    let imported = crate::structure_document::to_structural_fx_document(&source, Some(1)).unwrap();
    let text =
        serde_json::to_value(text_control(imported.document.composition().layers()).unwrap())
            .unwrap();
    modify(&imported.document, |raw| {
        raw["duration"] = json!(4.5);
        raw["backgroundColor"] = json!([0, 0, 0, 0]);
        let mut text = text;
        text["id"] = json!(6100);
        text["name"] = json!("Typewritten caption");
        text["parent"] = json!(6001);
        text["activeRange"] = json!({"start": 0, "duration": 2000});
        text["sourceText"]["text"] = json!("");
        // Select the existing physical Bold control and its verified font bytes.
        // The actual project font is never substituted by this test preparation.
        text["sourceText"]["fontFamily"] = json!("Arial");
        text["sourceText"]["fontStyle"] = json!("Bold");
        text["sourceText"]["applyStroke"] = json!(false);
        text["sourceText"]["baselineShift"] = json!(0);
        let transform = json!({"position": [0, 0], "anchorPoint": [0, 0],
            "scale": [100, 100], "rotation": 0, "opacity": 100});
        let paint = json!({"type": "Rect", "id": 6200, "name": "Retained panel",
            "parent": 6001, "activeRange": {"start": 0, "duration": 2000},
            "transform": transform, "rect": {"size": [80, 40], "fillColor": [0.1, 0.8, 0.7, 1]}});
        let playback = json!({"type": "windowed", "inputRange": {"start": 0, "duration": 2000},
            "mapping": {"type": "linear", "input": {"start": 0, "duration": 2000},
                "output": {"start": 0, "duration": 2000}}, "inputOffsetMs": 0});
        let mut delayed = playback.clone();
        delayed["inputRange"]["start"] = json!(2500);
        delayed["mapping"]["input"]["start"] = json!(2500);
        let mut sibling = paint.clone();
        sibling["id"] = json!(6300);
        sibling["parent"] = Value::Null;
        sibling["name"] = json!("Independent picture");
        raw["composition"]["layers"] = json!([{
            "type": "Group", "id": 6000, "name": "Delayed caption scope", "playback": delayed,
            "transform": {"position": [30, 20], "anchorPoint": [0, 0],
                "scale": [100, 100], "rotation": 0, "opacity": 50},
            "layers": [{"type": "Group", "id": 6001, "name": "Caption source",
                "parent": 6000, "playback": playback, "transform": transform,
                "layers": [text, paint]}]
        }, sibling]);
        raw["composition"]["dynamics"]["entries"] = json!([{
            "target": {"kind": "layer", "layerId": 6100, "propertyType": "textContent"},
            "animator": {"type": "jsScript", "layerTimeJsCode": TYPEWRITER}
        }]);
    })
}

fn native_export(
    document: &EditableFxCompositionDocument,
) -> (EditableFxCompositionDocument, Vec<ExportDiagnostic>) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.tsrct");
    let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/references/premiere/fonts/Arial-BoldMT.ttf");
    TesseractFileBuilder::try_new(document.clone())
        .unwrap()
        .add_asset("physical-font", &font, AssetKind::Font)
        .unwrap()
        .write(&source)
        .unwrap();
    let output = directory.path().join("export");
    let report = AfterEffects
        .export_from_tesseract(
            &source,
            &output,
            &AfterEffectsExportOptions { fps: 30.0 },
            ConversionMode::Write,
        )
        .unwrap();
    let native = read_project(&std::fs::read(output.join("project.aep")).unwrap()).unwrap();
    let imported = crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    (imported.document, report.diagnostics)
}

fn text_contents(layers: &[Layer]) -> Vec<String> {
    layers
        .iter()
        .flat_map(|layer| match layer.data() {
            LayerData::Text(text) => vec![text.source_text.text.clone()],
            LayerData::Group(group) => text_contents(&group.layers),
            _ => Vec::new(),
        })
        .collect()
}

fn has_layer(layers: &[Layer], name: &str) -> bool {
    layers.iter().any(|layer| {
        layer.name() == name
            || layer
                .child_layers()
                .is_some_and(|children| has_layer(children, name))
    })
}

#[test]
fn nested_typewriter_retains_empty_base_successive_native_text_and_picture() {
    let original = nested_typewriter();
    let before = original.to_json_value().unwrap();
    let prepared = prepare_with_progress(
        &original,
        &AfterEffectsExportOptions { fps: 30.0 },
        Progress::default(),
    )
    .unwrap();
    let keys = track(&prepared, 0).keyframes();
    assert_eq!(
        keys.iter()
            .map(|key| (key.layer_time().as_millis(), key.value().clone()))
            .collect::<Vec<_>>(),
        [0, 100, 150, 200, 250, 300, 350, 400, 450]
            .into_iter()
            .zip(
                ["", "A", "AB", "ABC", "", "X", "XY", "XYZ", ""]
                    .map(|text| PropertyValue::String(text.into()))
            )
            .collect::<Vec<_>>()
    );
    assert!(
        keys.iter()
            .all(|key| key.easing() == PropertyKeyframeEasing::Hold)
    );
    assert_eq!(
        prepared.document.to_json_value().unwrap()["composition"]["layers"],
        before["composition"]["layers"]
    );
    assert_eq!(original.to_json_value().unwrap(), before);
    let (native, diagnostics) = native_export(&original);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("omitted")),
        "{diagnostics:?}"
    );
    let retained = text_contents(native.composition().layers());
    for text in ["", "A", "AB", "ABC", "X", "XY", "XYZ"] {
        assert!(retained.iter().any(|value| value == text), "{retained:?}");
    }
    assert!(has_layer(native.composition().layers(), "Retained panel"));
    assert!(has_layer(
        native.composition().layers(),
        "Independent picture"
    ));
}

#[test]
fn refused_text_script_publishes_no_partial_keys_and_keeps_independent_caption_scope() {
    let original = modify(&nested_typewriter(), |raw| {
        let mut refused = raw["composition"]["layers"][0]["layers"][0]["layers"][0].clone();
        refused["id"] = json!(6102);
        refused["parent"] = Value::Null;
        refused["name"] = json!("Refused caption");
        raw["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(refused);
        raw["composition"]["dynamics"]["entries"].as_array_mut().unwrap().push(json!({
            "target": {"kind": "layer", "layerId": 6102, "propertyType": "textContent"},
            "animator": {"type": "jsScript", "layerTimeJsCode": "if(input.time.milliseconds>=600)return 42; return 'PART';"}
        }));
    });
    let before = original.to_json_value().unwrap();
    let prepared = prepare(&original).unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .keyframe_track()
            .is_some()
    );
    assert!(
        prepared.document.composition().dynamics().entries()[1]
            .animator
            .is_js_script()
    );
    assert!(prepared.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(6102))
            && diagnostic.message.contains("valid Unicode string")
    }));
    assert_eq!(original.to_json_value().unwrap(), before);
    let (native, diagnostics) = native_export(&original);
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(6102))
            && diagnostic.message.contains("layer and its subtree omitted")
    }));
    assert!(
        !diagnostics.iter().any(|diagnostic| {
            matches!(diagnostic.layer_id, Some(id) if [6000, 6001, 6100, 6200, 6300].contains(&id.value()))
                && diagnostic.message.contains("omitted")
        }),
        "{diagnostics:?}"
    );
    assert!(
        text_contents(native.composition().layers())
            .iter()
            .any(|text| text == "XYZ")
    );
    assert!(!has_layer(native.composition().layers(), "Refused caption"));
    assert!(has_layer(native.composition().layers(), "Retained panel"));
    assert!(has_layer(
        native.composition().layers(),
        "Independent picture"
    ));
}
