//! Supplementary editable native bindings; not independent Adobe fidelity proof.
use super::*;
use fx_conv::{HdrColorBridge, HdrTransfer};

fn colors(value: &Value, result: &mut Vec<[f64; 4]>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "fillColor"
                    && let Ok(color) = serde_json::from_value(value.clone())
                {
                    result.push(color);
                }
                colors(value, result);
            }
        }
        Value::Array(values) => {
            for value in values {
                colors(value, result);
            }
        }
        _ => {}
    }
}

#[test]
fn explicit_hdr_vector_binds_meaning_before_native_serialization_without_fx_mutation() {
    for transfer in [HdrTransfer::Pq, HdrTransfer::Hlg] {
        let mut value = imported();
        let mut layer = rect(&value, 700);
        layer["rect"]["fillColor"] = json!([1.2, 0.5, 0.25, 1.0]);
        layer["rect"]["strokeColor"] = Value::Null;
        value["composition"]["layers"] = json!([layer]);
        value["composition"]["dynamics"] = json!({"entries":[]});
        value["hdrGraphicsWhiteNits"] = json!(203.0);
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let original = document.to_json_value().unwrap();
        let exported = to_aep_with_document_views_and_media_and_fps(
            ExportDocumentViews::unchanged(&document).with_hdr(Some(transfer)),
            &BTreeMap::new(),
            24.0,
        )
        .unwrap();
        assert_eq!(document.to_json_value().unwrap(), original);
        assert_eq!(
            crate::hdr::owned_transfer(&crate::aep::Project::parse(&exported.bytes).unwrap()),
            Some(transfer)
        );
        let mut structural = read_project(&exported.bytes).unwrap();
        let decoded = to_structural_fx_document(&structural, None).unwrap();
        assert_eq!(decoded.document.hdr_graphics_white_nits(), Some(609.0));
        let native = HdrColorBridge::new(transfer, Some(203.0))
            .unwrap()
            .to_native([1.2, 0.5, 0.25, 1.0])
            .unwrap();
        let recovered = HdrColorBridge::new(transfer, None)
            .unwrap()
            .to_fx(native)
            .unwrap();
        let mut decoded_colors = Vec::new();
        colors(
            &decoded.document.to_json_value().unwrap(),
            &mut decoded_colors,
        );
        assert!(
            decoded_colors.iter().any(|color| color
                .iter()
                .zip(recovered)
                .all(|(a, b)| (a - b).abs() < 2e-6)),
            "{decoded_colors:?} expected recovered {recovered:?}"
        );
        assert!(
            decoded
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.limitation == crate::Limitation::ColorManagement)
        );
        // Raw structure is supplementary export evidence, explicitly disabling
        // inverse interpretation rather than confusing imported FX with native RGB.
        structural.color = crate::structure::ProjectColorContext::Legacy;
        let raw = to_structural_fx_document(&structural, None)
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
        let expected = HdrColorBridge::new(transfer, Some(203.0))
            .unwrap()
            .to_native([1.2, 0.5, 0.25, 1.0])
            .unwrap();
        let mut actual = Vec::new();
        colors(&raw, &mut actual);
        assert!(
            actual.iter().any(|color| color
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-6)),
            "{actual:?} expected {expected:?}; diagnostics {:?}",
            exported.diagnostics
        );
        assert!(
            !exported
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("was clamped"))
        );
    }
}
