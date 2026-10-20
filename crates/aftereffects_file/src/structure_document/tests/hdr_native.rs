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
