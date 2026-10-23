#![cfg(feature = "ffmpeg-library")]

//! FX `hdrGraphicsWhiteNits` through an explicit HDR export and a fresh
//! import of the generated project. Our own writer's project is the native
//! input here, so this is supplementary round-trip evidence, not Adobe proof.

use super::support::*;
use fx_conv::{ConversionMode, ExportFromTesseract};
use premiere_file::{Premiere, PremiereExportOptions};
use serde_json::json;
use tesseract_file::{TesseractFile, TesseractFileBuilder};

#[test]
fn hdr_graphics_white_round_trips_through_native_sequence_settings() {
    for (transfer, flag) in [
        (fx_conv::HdrTransfer::Pq, "pq"),
        (fx_conv::HdrTransfer::Hlg, "hlg"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut document = crate::test_support::editable_document();
        document["hdrGraphicsWhiteNits"] = json!(300.0);
        document["composition"]["layers"] = json!([{
            "id": 7, "type": "Rect", "name": "graphics white",
            "activeRange": {"start": 0, "duration": 1000},
            "transform": {"anchorPoint": [50, 50], "position": [500, 500],
                "scale": [100, 100], "rotation": 0, "opacity": 100},
            "rect": {"size": [100, 100], "fillColor": [1.0, 1.0, 1.0, 1.0],
                "strokeEnabled": false}
        }]);
        let archive = dir.path().join("hdr.tsrct");
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .write(&archive)
            .unwrap();
        let package = dir.path().join("native");
        Premiere
            .export_from_tesseract(
                &archive,
                &package,
                &PremiereExportOptions {
                    hdr: Some(transfer),
                    ..Default::default()
                },
                ConversionMode::Write,
            )
            .unwrap();
        let xml = read_xml(&package.join("project.prproj"));
        // Project and sequence colour management both carry the FX white.
        assert_eq!(xml.matches("\"graphicsWhiteLuminance\":300,").count(), 2);

        let reimported = dir.path().join("reimported");
        let omissions =
            premiere_to_tesseract(package.join("project.prproj"), &reimported, None, false)
                .unwrap();
        let rebuilt = TesseractFile::open(first_project(&reimported))
            .unwrap()
            .project_json()
            .unwrap();
        assert_eq!(rebuilt["hdrGraphicsWhiteNits"], json!(300.0));
        let hint = format!(
            "hint: sequence output is Rec.2100 {}; FX documents carry no HDR delivery target, so use `tsrct export --hdr {flag}` to deliver it",
            flag.to_uppercase()
        );
        assert!(
            omissions.iter().any(|omission| omission.reason == hint),
            "{omissions:?}"
        );
        // FX white exported against 300 cd/m² decodes back to FX white.
        let shape = rebuilt["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Shape")
            .expect("HDR rectangle exports as an editable Graphic Shape");
        let fill: [f64; 4] =
            serde_json::from_value(shape["shape"]["fills"][0]["paint"]["color"].clone()).unwrap();
        for channel in &fill[..3] {
            assert!((channel - 1.0).abs() < 0.01, "{fill:?}");
        }
    }
}
