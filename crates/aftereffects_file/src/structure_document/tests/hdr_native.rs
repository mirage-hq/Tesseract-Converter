//! Pinned Adobe-authored points, not converter-generated roundtrip evidence.
use std::io::Read;

use super::*;

fn descendants<'a>(value: &'a Value, result: &mut Vec<&'a Value>) {
    if let Some(layers) = value["layers"].as_array() {
        for layer in layers {
            result.push(layer);
            descendants(layer, result);
        }
    }
}

#[test]
fn native_ingamut_chromatic_import_preserves_saturation_headroom_and_geometry() {
    let declared: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/hdr/ingamut-chromatic-case-v1.json"
    ))
    .unwrap();
    let base = declared["linear_bt709_base_colors"].as_array().unwrap();
    let patches = declared["patches"].as_array().unwrap();
    for (compressed, expected_hash) in [
        (
            include_bytes!("../../../tests/fixtures/hdr/ingamut-chromatic-pq-v1.aep.gz").as_slice(),
            "9ebe1a704e756c77674191dfde0313c54806f3f9382aa53d23670fab947c6d9b",
        ),
        (
            include_bytes!("../../../tests/fixtures/hdr/ingamut-chromatic-hlg-v1.aep.gz")
                .as_slice(),
            "b3d2f9e3ba8e3d29bcd2ce2ec0b75fc06b9fa7927e8ad85ec8895d2d7d9c8c75",
        ),
    ] {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(compressed)
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected_hash);
        let project = read_project(&bytes).unwrap();
        let imported = to_structural_fx_document(&project, Some(14)).unwrap();
        let value = imported.document.to_json_value().unwrap();
        assert_eq!(
            value["dimensions"],
            serde_json::json!({"width":1280,"height":512})
        );
        assert_eq!(value["duration"], 1.0);
        assert_eq!(value["hdrGraphicsWhiteNits"], 609.0);
        let mut layers = Vec::new();
        descendants(&value["composition"], &mut layers);
        assert_eq!(
            layers
                .iter()
                .filter(|layer| layer["type"] == "Rect")
                .count(),
            10
        );
        for (index, patch) in patches.iter().enumerate() {
            let name = patch["name"].as_str().unwrap();
            let owner = layers.iter().find(|layer| layer["name"] == name).unwrap();
            // Ten declared paints bound both indices to0..4, exactly in f64.
            assert_eq!(
                owner["transform"]["position"],
                serde_json::json!([
                    128.0 + 256.0 * (index % 5) as f64,
                    128.0 + 256.0 * (index / 5) as f64
                ])
            );
            assert_eq!(
                owner["transform"]["anchorPoint"],
                serde_json::json!([128.0, 128.0])
            );
            assert_eq!(owner["transform"]["opacity"], 100.0);
            let mut children = Vec::new();
            descendants(owner, &mut children);
            let rect = children
                .iter()
                .find(|child| child["type"] == "Rect")
                .unwrap();
            assert_eq!(rect["rect"]["size"], serde_json::json!([256.0, 256.0]));
            assert_eq!(
                rect["activeRange"],
                serde_json::json!({"start":0,"duration":1000000000000_i64})
            );
            let paint = rect["rect"]["fillColor"].as_array().unwrap();
            for (channel, actual) in paint[..3].iter().enumerate() {
                let linear = if index % 5 == 4 {
                    if index < 5 { 0.5 } else { 3.0 }
                } else {
                    base[index % 5][channel].as_f64().unwrap() * if index < 5 { 1.0 } else { 4.0 }
                };
                // Independent source meaning: native signals use203nit gain;
                // import normalizes to FX609. Do not compare against a roundtrip.
                let normalized = linear / 3.0;
                let expected = if normalized <= 0.0031308 {
                    normalized * 12.92
                } else {
                    1.055 * normalized.powf(1.0 / 2.4) - 0.055
                };
                assert!(
                    (actual.as_f64().unwrap() - expected).abs() < 0.00002,
                    "{name} channel{channel}: {actual} != {expected}"
                );
            }
            assert_eq!(paint[3], 1.0);
            if name == "red-above-white" {
                assert!(paint[0].as_f64().unwrap() > 1.0);
            }
        }
        assert!(!value.to_string().contains("JsScript"));
        assert!(!imported.diagnostics.iter().any(|d| {
            d.message
                .contains("chroma reduced toward the achromatic axis")
                || d.message.contains("unsupported numeric colour")
        }));
    }
}

#[test]
fn native_saturated_bt2020_import_keeps_six_nonblack_editable_paints() {
    for (compressed, expected_hash) in [
        (
            include_bytes!("../../../tests/fixtures/hdr/chromatic-pq.aep.gz").as_slice(),
            "756fff6d075cde6c4169ac31460c2a3b9df0e07e2aed2ec9fa728dc91cc0fb96",
        ),
        (
            include_bytes!("../../../tests/fixtures/hdr/chromatic-hlg.aep.gz").as_slice(),
            "e76b7168970626fd436860769de2dbe016502b3cd69a5ee99de371f2714c98a3",
        ),
    ] {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(compressed)
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected_hash);
        let project = read_project(&bytes).unwrap();
        let imported = to_structural_fx_document(&project, Some(14)).unwrap();
        let value = imported.document.to_json_value().unwrap();
        let mut layers = Vec::new();
        descendants(&value["composition"], &mut layers);
        assert_eq!(
            layers
                .iter()
                .filter(|layer| layer["type"] == "Rect")
                .count(),
            1030
        );
        for index in 0..6 {
            let name = format!("BT2020 chromatic {index}");
            let layer = layers.iter().find(|layer| layer["name"] == name).unwrap();
            let mut children = Vec::new();
            descendants(layer, &mut children);
            let rect = children
                .iter()
                .find(|child| child["type"] == "Rect")
                .unwrap();
            let paint = rect["rect"]["fillColor"].as_array().unwrap();
            assert!(paint[..3].iter().all(|v| v.as_f64().unwrap() >= 0.0));
            assert!(
                paint[..3].iter().any(|v| v.as_f64().unwrap() > 0.0),
                "{name} became black"
            );
            assert_eq!(paint[3], 1.0);
        }
        assert!(!value.to_string().contains("JsScript"));
        assert_eq!(
            imported
                .diagnostics
                .iter()
                .filter(|d| d
                    .message
                    .contains("chroma reduced toward the achromatic axis"))
                .count(),
            6
        );
    }
}

/// The natively authored AE 26.5 project with the built-in "Rec.2100 PQ"
/// working space (32 bpc, non-linearized) and one Solid authored as [2, 2, 2].
#[test]
fn native_built_in_rec2100_pq_project_imports_as_hdr_with_profile_white() {
    let bytes = include_bytes!("../../../tests/fixtures/color_ae_rec2100_pq_project.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "1be8348f26fdca009d01fce9f9efac4f26d840ae942dfcbc8f2305caa42cb465"
    );
    let project = read_project(bytes).unwrap();
    assert_eq!(
        project.color,
        crate::structure::ProjectColorContext::Hdr {
            transfer: fx_conv::HdrTransfer::Pq,
            linear_blending: false,
            binding: crate::structure::HdrProfileBinding::AdobeRec2100Pq,
        }
    );
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    let value = imported.document.to_json_value().unwrap();
    assert_eq!(value["hdrGraphicsWhiteNits"], 100.0);
    assert!(
        imported.diagnostics.iter().any(|d| {
            d.limitation == Limitation::ColorManagement
                && d.message.contains("AE built-in \"Rec.2100 PQ\"")
                && d.message.contains("FX graphics white 100 nits")
        }),
        "{:#?}",
        imported.diagnostics
    );
    assert!(!imported.diagnostics.iter().any(|d| {
        d.message.contains("is not bound") || d.message.contains("legacy numeric colour")
    }));
    // The authored 32 bpc super-white Solid [2, 2, 2] lies past the PQ peak code
    // 1.0 (10000 cd/m²), where the transfer curve ends. It imports as an editable
    // solid whose light continues linearly past the peak (twice the peak), with
    // a colour-management diagnostic, not as a non-rendering placeholder.
    let mut layers = Vec::new();
    descendants(&value["composition"], &mut layers);
    let rects: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Rect")
        .collect();
    assert_eq!(rects.len(), 1, "{value}");
    // The solid source geometry stays decoded, so its occurrence keeps the
    // source-size default anchor and covers the comp.
    let occurrence = layers
        .iter()
        .find(|layer| layer["type"] == "Group" && layer["name"] == "Solid")
        .unwrap();
    assert_eq!(
        occurrence["transform"]["anchorPoint"],
        serde_json::json!([960.0, 540.0])
    );
    assert_eq!(
        occurrence["transform"]["position"],
        serde_json::json!([960.0, 540.0])
    );
    let bridge = fx_conv::HdrColorBridge::new(fx_conv::HdrTransfer::Pq, Some(100.0))
        .unwrap()
        .with_super_white_headroom();
    let expected = bridge.to_fx([2.0, 2.0, 2.0, 1.0]).unwrap();
    let peak = bridge.to_fx([1.0, 1.0, 1.0, 1.0]).unwrap();
    let fill: [f64; 4] = serde_json::from_value(rects[0]["rect"]["fillColor"].clone()).unwrap();
    assert!(
        fill.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-9),
        "{fill:?} expected {expected:?}"
    );
    assert!(
        fill[0] > peak[0],
        "{fill:?} must lie above the PQ peak {peak:?}"
    );
    assert!(
        imported.diagnostics.iter().any(|d| {
            d.limitation == Limitation::ColorManagement
                && d.message
                    .contains("native super-white code values [2.0, 2.0, 2.0]")
                && d.message.contains("continuing linearly past the peak")
        }),
        "{:#?}",
        imported.diagnostics
    );
    assert!(
        !imported
            .diagnostics
            .iter()
            .any(|d| d.message.contains("invalid solid color")
                || d.message.contains("replaced by black")),
        "{:#?}",
        imported.diagnostics
    );
}

/// A pre-`PwCs` project (format 93, After Effects 17.0) without a working
/// space converts with the legacy interpretation, a named producer version and
/// no unreadable-declaration fallback.
#[test]
fn pre_declaration_project_converts_as_declared_legacy_with_the_producer_named() {
    let bytes = crate::hdr::pre_declaration_project().encode().unwrap();
    let project = read_project(&bytes).unwrap();
    assert_eq!(project.format_version, 93);
    assert_eq!(project.producer.to_string(), "After Effects 17.0 build 26");
    assert!(matches!(
        project.color,
        crate::structure::ProjectColorContext::LegacyDeclared { .. }
    ));
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    let messages: Vec<_> = imported
        .diagnostics
        .iter()
        .map(|d| (d.limitation, d.message.as_str()))
        .collect();
    assert!(
        messages.contains(&(
            Limitation::Version,
            "project saved by After Effects 17.0 build 26 (project format 93), older than the After Effects 26 reference layout (format 97, measured from 26.0 to 26.5 saves); records are parsed using the known record layouts and unread fields are diagnosed where they occur"
        )),
        "{messages:#?}"
    );
    assert!(
        messages.contains(&(
            Limitation::ColorManagement,
            "working space admitted as legacy: project format 93 predates the PwCs working-profile declaration; its cpid declares no working space (After Effects \"None\"); point colours are imported as unchanged display-referred numeric values, the same interpretation as a project without colour management"
        )),
        "{messages:#?}"
    );
    assert!(
        !messages
            .iter()
            .any(|(_, m)| m.contains("unreadable working declaration")),
        "{messages:#?}"
    );
    // The legacy interpretation leaves the document's colour domain unbound.
    let value = imported.document.to_json_value().unwrap();
    assert!(
        value.get("hdrGraphicsWhiteNits").is_none_or(Value::is_null),
        "{value}"
    );
}
