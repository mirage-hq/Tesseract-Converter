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

/// Shape-paint colours of an exported AEP read back without inverse
/// interpretation (the project colour context forced to Legacy).
fn exported_paint_colors(bytes: &[u8]) -> Vec<[f64; 4]> {
    fn paint_colors(value: &Value, result: &mut Vec<[f64; 4]>) {
        match value {
            Value::Object(object) => {
                if let Some(color) = object
                    .get("paint")
                    .and_then(|paint| serde_json::from_value(paint["color"].clone()).ok())
                {
                    result.push(color);
                }
                object
                    .values()
                    .for_each(|value| paint_colors(value, result));
            }
            Value::Array(values) => values.iter().for_each(|value| paint_colors(value, result)),
            _ => {}
        }
    }
    let mut structural = read_project(bytes).unwrap();
    structural.color = crate::structure::ProjectColorContext::Legacy;
    let raw = to_structural_fx_document(&structural, None)
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut result = Vec::new();
    paint_colors(&raw, &mut result);
    result
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

/// `color_ae_rec2100_pq_solid.aep` (AE 26.5 built-in "Rec.2100 PQ", 32 bpc,
/// one Solid at PQ code values [0.55, 0.45, 0.35]): an `--hdr pq` export of
/// its fresh import writes the same PQ code values. AME renders that export
/// decoded-frame identical to the native project
/// (`color_ae_fixtures.provenance.json`).
#[test]
fn native_rec2100_pq_solid_exports_its_pq_code_values_under_hdr_pq() {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("../../../tests/fixtures/color_ae_rec2100_pq_solid.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "7ff0f899a8ee174799a29b58ac269e42a6326e1773db17217334b1cfc07fe6ac"
    );
    let imported = to_structural_fx_document(&read_project(bytes).unwrap(), Some(1)).unwrap();
    let value = imported.document.to_json_value().unwrap();
    assert_eq!(value["hdrGraphicsWhiteNits"], 100.0);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let exported = to_aep_with_document_views_and_media_and_fps(
        ExportDocumentViews::unchanged(&document).with_hdr(Some(HdrTransfer::Pq)),
        &BTreeMap::new(),
        30.0,
    )
    .unwrap();
    // The Solid exports as a native shape layer; its paint carries the colour.
    let actual = exported_paint_colors(&exported.bytes);
    let expected = [0.55_f32, 0.45, 0.35, 1.0].map(f64::from);
    assert!(
        actual.iter().any(|color| color
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-6)),
        "{actual:?} expected {expected:?}; diagnostics {:?}",
        exported.diagnostics
    );
}

/// `color_ae_rec2100_pq_project.aep` (AE 26.5 built-in "Rec.2100 PQ", 32 bpc,
/// one super-white Solid [2, 2, 2]): an `--hdr pq` export of its fresh import
/// writes the same super-white code values, not the peak or black. AE renders
/// of that export are compared with the native references in
/// `color_ae_fixtures.provenance.json`.
#[test]
fn native_rec2100_pq_super_white_solid_exports_its_super_white_codes_under_hdr_pq() {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("../../../tests/fixtures/color_ae_rec2100_pq_project.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "1be8348f26fdca009d01fce9f9efac4f26d840ae942dfcbc8f2305caa42cb465"
    );
    let imported = to_structural_fx_document(&read_project(bytes).unwrap(), Some(1)).unwrap();
    let document =
        EditableFxCompositionDocument::from_json_value(imported.document.to_json_value().unwrap())
            .unwrap();
    let exported = to_aep_with_document_views_and_media_and_fps(
        ExportDocumentViews::unchanged(&document).with_hdr(Some(HdrTransfer::Pq)),
        &BTreeMap::new(),
        30.0,
    )
    .unwrap();
    let actual = exported_paint_colors(&exported.bytes);
    assert!(
        actual
            .iter()
            .any(|color| color[..3].iter().all(|v| (v - 2.0).abs() < 1e-6) && color[3] == 1.0),
        "{actual:?} expected the super-white [2, 2, 2]; diagnostics {:?}",
        exported.diagnostics
    );
    assert!(
        !exported
            .diagnostics
            .iter()
            .any(|d| d.message.contains("replaced RGB with black")),
        "{:?}",
        exported.diagnostics
    );
    assert!(
        exported
            .diagnostics
            .iter()
            .any(|d| d.message.contains("32 bpc super-white code values")),
        "{:?}",
        exported.diagnostics
    );
}
