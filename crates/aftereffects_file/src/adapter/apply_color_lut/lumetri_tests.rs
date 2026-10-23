//! Fresh import of the human-authored AE 26.5 Lumetri custom-LUT fixture
//! (`color_ae_lumetri_custom_lut.aep`: comp 2 browses `color_warm_tint.cube`
//! in the Creative Look, comp 15 in the Basic Correction Input LUT) through
//! the public adapters, then export and reimport.

use std::fs;

use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
use tesseract_file::TesseractFile;

use crate::{
    AfterEffectsExportOptions, AfterEffectsImportOptions,
    adapter::{AfterEffects, OUTPUT_NAME},
    effects::apply_color_lut::Lut3d,
};

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/color_ae_lumetri_custom_lut.aep");
const CHART: &[u8] = include_bytes!("../../../../premiere_file/tests/fixtures/a1_bg_chart.png");
const CUBE: &str = include_str!("../../../../premiere_file/tests/fixtures/color_warm_tint.cube");
const LOOK: u32 = 2;
const INPUT_LUT: u32 = 15;

/// The effects of every layer of `archive`'s root composition, as JSON.
fn effects(archive: &TesseractFile) -> Vec<serde_json::Value> {
    let value = archive.project().to_json_value().unwrap();
    let mut found = Vec::new();
    fn walk(value: &serde_json::Value, found: &mut Vec<serde_json::Value>) {
        for layer in value["layers"].as_array().into_iter().flatten() {
            for record in layer["effects"].as_array().into_iter().flatten() {
                found.push(record["effect"].clone());
            }
            walk(layer, found);
        }
    }
    walk(&value["composition"], &mut found);
    found
}

/// The packaged `.cube` of a `lookTransform` effect: archive path, text and
/// table.
fn cube(archive: &TesseractFile, look: &serde_json::Value) -> (String, String, Lut3d) {
    let asset = archive
        .asset(look["transform"]["assetId"].as_str().unwrap())
        .unwrap();
    let text = String::from_utf8(asset.read_verified_bytes(1 << 20).unwrap()).unwrap();
    let lut = Lut3d::parse_cube(&text).unwrap();
    (asset.descriptor().path.clone(), text, lut)
}

fn assert_lut(lut: &Lut3d, tolerance: f64) {
    let picked = Lut3d::parse_cube(CUBE).unwrap();
    assert_eq!(lut.size, picked.size);
    for (a, b) in lut.samples.iter().zip(&picked.samples) {
        for (a, b) in a.iter().zip(b) {
            assert!((a - b).abs() <= tolerance, "{a} vs {b}");
        }
    }
}

/// The pinned bytes (`color_ae_fixtures.provenance.json`) keep no host path:
/// every authoring path is under `/fixture-source/`.
#[test]
fn the_custom_lut_fixture_is_pinned_and_sanitized() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(FIXTURE)),
        "ad190b3c3545ef6b0f92a5f6836fc0b70a6ed07b98906709aeca5b411623afda"
    );
    let utf16 =
        |text: &str| -> Vec<u8> { text.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    for needle in [b"/Users/".to_vec(), utf16("/Users/")] {
        assert!(!FIXTURE.windows(needle.len()).any(|window| window == needle));
    }
    let sanitized = utf16("/fixture-source/");
    assert_eq!(
        FIXTURE
            .windows(sanitized.len())
            .filter(|window| *window == sanitized)
            .count(),
        3
    );
}

#[test]
fn ui_browsed_lumetri_luts_import_as_editable_look_transforms_and_export_as_apply_color_lut() {
    for composition in [LOOK, INPUT_LUT] {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("color_ae_lumetri_custom_lut.aep");
        fs::write(&input, FIXTURE).unwrap();
        fs::write(root.path().join("a1_bg_chart.png"), CHART).unwrap();
        let imported = root.path().join("imported");
        let report = AfterEffects
            .import_to_tesseract(
                &input,
                &imported,
                &AfterEffectsImportOptions {
                    composition: Some(composition),
                    ..AfterEffectsImportOptions::default()
                },
                ConversionMode::Write,
            )
            .unwrap();
        let messages: Vec<&str> = report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect();
        assert!(
            messages
                .iter()
                .any(|message| message
                    .contains("Lumetri custom LUT imported as an FX lookTransform")),
            "{messages:?}"
        );
        assert!(
            !messages
                .iter()
                .any(|message| message.contains("not imported") || message.contains("popup")),
            "{messages:?}"
        );
        // The neutral grade is dropped, and so is its approximation note.
        assert!(
            !messages
                .iter()
                .any(|message| message.contains("Lumetri Basic/Creative scalars approximated")),
            "{messages:?}"
        );
        let archive = TesseractFile::open(imported.join(OUTPUT_NAME)).unwrap();
        // The neutral Basic/Creative grade adds no effect: one full-mix LUT.
        let [look] = effects(&archive)
            .try_into()
            .unwrap_or_else(|effects: Vec<_>| {
                panic!("one lookTransform expected in comp {composition}: {effects:?}")
            });
        assert_eq!(look["type"], "lookTransform");
        assert_eq!(look["mix"], 1.0);
        assert_eq!(look["transform"]["type"], "lut3d");
        let (path, text, lut) = cube(&archive, &look);
        assert!(path.ends_with("/color_warm_tint.cube"), "{path}");
        // The embedded copy is the file's values as f32; six decimals give
        // back the in-house file byte for byte.
        assert_lut(&lut, 1e-7);
        assert_eq!(text, CUBE);

        let exported = root.path().join("exported");
        let export = AfterEffects
            .export_from_tesseract(
                &imported.join(OUTPUT_NAME),
                &exported,
                &AfterEffectsExportOptions {
                    fps: 30.0,
                    hdr: None,
                },
                ConversionMode::Write,
            )
            .unwrap();
        assert!(
            export
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("written as Apply Color LUT")),
            "{:?}",
            export.diagnostics
        );
        let reimported = root.path().join("reimported");
        AfterEffects
            .import_to_tesseract(
                &exported.join("project.aep"),
                &reimported,
                &AfterEffectsImportOptions {
                    composition: Some(1),
                    ..AfterEffectsImportOptions::default()
                },
                ConversionMode::Write,
            )
            .unwrap();
        let archive = TesseractFile::open(reimported.join(OUTPUT_NAME)).unwrap();
        let [again] = effects(&archive).try_into().unwrap();
        assert_eq!(again["type"], "lookTransform");
        // Apply Color LUT keeps 16-bit codes.
        assert_lut(&cube(&archive, &again).2, 1.0 / 65535.0);
    }
}
