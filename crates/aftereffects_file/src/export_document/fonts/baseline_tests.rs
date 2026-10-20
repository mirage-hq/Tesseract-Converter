use fx_schema::{EditableFxCompositionDocument, LayerData};
use serde_json::{Value, json};

use super::{ArchiveFonts, inferred_faces};
use crate::export_document::{AnimationIndex, text::lower_with_native_3d};

fn fixture() -> Value {
    json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1, "dimensions": {"width": 640, "height": 480}, "duration": 2,
        "composition": {"id": "main", "name": "Baseline", "dynamics": {"entries": []}, "layers": [{
            "type": "Text", "id": 1, "name": "Box baseline",
            "activeRange": {"start": 0, "duration": 2000},
            "transform": {"position": [20, 30], "anchorPoint": [0, 0],
                "scale": [100, 100], "rotation": 0, "opacity": 100},
            "sourceText": {"text": "Editable words", "fontFamily": "Arial", "fontStyle": "Bold",
                "fontSize": 48, "fillColor": [1, 1, 1, 1], "boxText": true,
                "boxSize": [240, 160], "boxPosition": [5, 9], "boxFirstBaseline": 71.5,
                "verticalAlign": "top", "leading": 56}
        }]}
    })
}

fn lower(value: Value, embedded: bool) -> crate::export_document::text::LoweredText {
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let LayerData::Text(text) = document.composition().layers()[0].data() else {
        panic!("Text fixture")
    };
    let fonts = ArchiveFonts {
        faces: inferred_faces(include_bytes!(
            "../../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf"
        )),
    };
    lower_with_native_3d(
        text,
        &text.transform,
        text.id,
        &AnimationIndex::new(document.composition().dynamics().entries()),
        None,
        false,
        embedded.then_some(&fonts),
    )
    .unwrap()
}

#[test]
fn authored_box_baseline_rebases_native_frame_using_embedded_cap_height() {
    let output = lower(fixture(), true);
    let text = &output.spec.documents.keys[0].document;
    // The pinned public Arial Bold physical face has capHeight=1466, UPM=2048.
    assert_eq!(
        text.box_position,
        Some([5.0, 71.5 - 48.0 * 1466.0 / 2048.0])
    );
    assert_eq!(text.box_size, Some([240.0, 169.0 - 37.140625]));
    assert_eq!(text.text, "Editable words");
    assert_eq!(text.leading, Some(56.0));
    assert_eq!(output.spec.transform.position, [20.0, 30.0]);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.contains("cap-height baseline approximation"))
    );
}

#[test]
fn authored_box_baseline_consumes_alignment_and_keeps_held_wording_editable() {
    for alignment in ["top", "center", "bottom"] {
        let mut value = fixture();
        value["composition"]["layers"][0]["sourceText"]["verticalAlign"] = alignment.into();
        value["composition"]["dynamics"]["entries"] = json!([{
            "target": {"kind": "layer", "layerId": 1, "propertyType": "textContent"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "before", "layerTime": 0, "easing": {"type": "hold"},
                    "value": {"type": "string", "value": "First"}},
                {"id": "after", "layerTime": 1000, "easing": {"type": "hold"},
                    "value": {"type": "string", "value": "Edited words"}}
            ]}
        }]);
        let output = lower(value, true);
        assert_eq!(output.spec.documents.keys.len(), 2);
        for key in &output.spec.documents.keys {
            assert_eq!(key.document.box_position, Some([5.0, 37.140625]));
            assert_eq!(
                key.document.vertical_align,
                Some(fx_schema::VerticalAlign::Top)
            );
        }
        assert_eq!(output.spec.documents.keys[1].document.text, "Edited words");
    }
}

#[test]
fn authored_box_baseline_size_animation_retains_ordinary_native_box() {
    let mut value = fixture();
    value["composition"]["dynamics"]["entries"] = json!([{
        "target": {"kind": "layer", "layerId": 1, "propertyType": "fontSize"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "small", "layerTime": 0, "easing": {"type": "hold"},
                "value": {"type": "float", "value": 48}},
            {"id": "large", "layerTime": 1000, "easing": {"type": "hold"},
                "value": {"type": "float", "value": 64}}
        ]}
    }]);
    let output = lower(value, true);
    for key in &output.spec.documents.keys {
        assert_eq!(key.document.box_position, Some([5.0, 9.0]));
    }
    assert_eq!(output.spec.documents.keys[1].document.font_size, 64.0);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.contains("changing font or size"))
    );
}

#[test]
fn authored_box_baseline_fallback_keeps_editable_text_without_metric_or_explicit_alignment() {
    for (mut value, embedded) in [(fixture(), false), (fixture(), true)] {
        if embedded {
            value["composition"]["layers"][0]["sourceText"]
                .as_object_mut()
                .unwrap()
                .remove("verticalAlign");
        }
        let output = lower(value, embedded);
        assert_eq!(
            output.spec.documents.keys[0].document.box_position,
            Some([5.0, 9.0])
        );
        assert_eq!(
            output.spec.documents.keys[0].document.text,
            "Editable words"
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.contains("boxFirstBaseline"))
        );
    }
}
