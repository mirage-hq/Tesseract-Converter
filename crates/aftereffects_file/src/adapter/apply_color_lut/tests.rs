//! Import → export → import of the human-authored AE 26.5 Apply Color LUT
//! fixture through the public adapters: the embedded LUT is packaged as a
//! `.cube` asset, exported as a fresh Apply Color LUT with the same CLUT and
//! the `.cube` published beside the project, and re-imported unchanged.

use std::fs;

use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
use tesseract_file::TesseractFile;

use crate::{
    AfterEffectsExportOptions, AfterEffectsImportOptions,
    adapter::{AfterEffects, OUTPUT_NAME},
    effects::apply_color_lut::{self, Lut3d},
    properties,
    structure::{ItemKind, StructuralProject, read_project},
    structure_document::to_structural_fx_document,
};

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/color_ae_apply_color_lut.aep");
const CHART: &[u8] = include_bytes!("../../../../premiere_file/tests/fixtures/a1_bg_chart.png");
const CUBE: &str = include_str!("../../../../premiere_file/tests/fixtures/color_warm_tint.cube");
/// Native composition `chart` (29.97 fps, one layer).
const COMPOSITION: u32 = 2;

/// The look transforms of the composition holding the first Apply Color
/// LUT, as JSON values.
fn look_transforms(project: &StructuralProject) -> Vec<serde_json::Value> {
    let (composition, _) = first_instance(project);
    let converted = to_structural_fx_document(project, Some(composition)).unwrap();
    let value = converted.document.to_json_value().unwrap();
    let mut found = Vec::new();
    fn walk(value: &serde_json::Value, found: &mut Vec<serde_json::Value>) {
        for layer in value["layers"].as_array().into_iter().flatten() {
            for record in layer["effects"].as_array().into_iter().flatten() {
                if record["effect"]["type"] == "lookTransform" {
                    found.push(record["effect"].clone());
                }
            }
            walk(layer, found);
        }
    }
    walk(&value["composition"], &mut found);
    found
}

/// The composition and `sspc` of the first Apply Color LUT instance.
fn first_instance(project: &StructuralProject) -> (u32, Vec<crate::rifx::Chunk>) {
    for item in &project.items {
        let ItemKind::Composition(composition) = &item.kind else {
            continue;
        };
        for layer in &composition.layers {
            let roots = properties::root_runs(&layer.content).unwrap();
            let Some((_, parade)) = roots.iter().find(|(name, _)| *name == "ADBE Effect Parade")
            else {
                continue;
            };
            let instances =
                properties::runs(properties::unique_list(parade, *b"tdgp").unwrap()).unwrap();
            if let Some((_, run)) = instances
                .iter()
                .find(|(name, _)| *name == apply_color_lut::MATCH_NAME)
            {
                return (
                    item.id,
                    properties::unique_list(run, *b"sspc").unwrap().to_vec(),
                );
            }
        }
    }
    panic!("no Apply Color LUT instance");
}

fn sequence_data(plugin: &[crate::rifx::Chunk]) -> Vec<u8> {
    properties::data(plugin, *b"sdat").unwrap().to_vec()
}

#[test]
fn apply_color_lut_round_trips_through_tesseract_with_its_cube() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("color_ae_apply_color_lut.aep");
    fs::write(&input, FIXTURE).unwrap();
    fs::write(root.path().join("chart.png"), CHART).unwrap();
    let imported = root.path().join("imported");
    let report = AfterEffects
        .import_to_tesseract(
            &input,
            &imported,
            &AfterEffectsImportOptions {
                composition: Some(COMPOSITION),
                ..AfterEffectsImportOptions::default()
            },
            ConversionMode::Write,
        )
        .unwrap();
    assert!(
        report.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("Effect ADBE Apply Color LUT2 (\"color_warm_tint.cube\", 17³)")),
        "{:?}",
        report.diagnostics
    );
    let archive = TesseractFile::open(imported.join(OUTPUT_NAME)).unwrap();
    let source_look = look_transforms(&read_project(FIXTURE).unwrap()).remove(0);
    let asset_id = source_look["transform"]["assetId"]
        .as_str()
        .unwrap()
        .to_owned();
    let asset = archive.asset(&asset_id).unwrap();
    assert!(asset.descriptor().path.ends_with("/color_warm_tint.cube"));
    let cube = String::from_utf8(asset.read_verified_bytes(1 << 20).unwrap()).unwrap();
    let packaged = Lut3d::parse_cube(&cube).unwrap();
    let picked = Lut3d::parse_cube(CUBE).unwrap();
    assert_eq!(packaged.size, picked.size);
    for (a, b) in packaged.samples.iter().zip(&picked.samples) {
        for (a, b) in a.iter().zip(b) {
            assert!((a - b).abs() <= 1e-5, "{a} vs {b}");
        }
    }
    let effects = archive.project().composition().asset_refs();
    assert!(
        effects
            .iter()
            .any(|reference| reference.asset_id == asset_id)
    );

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
    // The .cube is published beside the project, byte-equal to the asset.
    assert_eq!(
        fs::read_to_string(exported.join("color_warm_tint.cube")).unwrap(),
        cube
    );
    assert!(
        export
            .artifacts
            .iter()
            .any(|artifact| artifact.path.ends_with("color_warm_tint.cube"))
    );
    let written = read_project(&fs::read(exported.join("project.aep")).unwrap()).unwrap();
    // Same CLUT as AE wrote, same file label, and the same content-addressed asset on reimport.
    let (_, native) = first_instance(&read_project(FIXTURE).unwrap());
    let (_, fresh) = first_instance(&written);
    let decoded = apply_color_lut::decode_sequence_data(&sequence_data(&fresh)).unwrap();
    assert_eq!(
        decoded,
        apply_color_lut::decode_sequence_data(&sequence_data(&native)).unwrap()
    );
    let label = |plugin: &[crate::rifx::Chunk]| {
        let table = properties::unique_list(plugin, *b"parT").unwrap();
        let runs = properties::runs(table).unwrap();
        let (_, run) = runs
            .iter()
            .find(|(name, _)| *name == apply_color_lut::FILE_LABEL_CONTROL)
            .unwrap();
        let pard = properties::data(run, *b"pard").unwrap();
        String::from_utf8(
            pard[16..48]
                .split(|byte| *byte == 0)
                .next()
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    };
    assert_eq!(label(&fresh), "Using: color_warm_tint.cube");
    assert_eq!(label(&fresh), label(&native));
    let reimported = look_transforms(&written);
    assert_eq!(reimported, vec![source_look]);
}

/// Import the fixture's composition into `root/imported`.
fn import_fixture(root: &std::path::Path) -> std::path::PathBuf {
    let input = root.join("color_ae_apply_color_lut.aep");
    fs::write(&input, FIXTURE).unwrap();
    fs::write(root.join("chart.png"), CHART).unwrap();
    let imported = root.join("imported");
    AfterEffects
        .import_to_tesseract(
            &input,
            &imported,
            &AfterEffectsImportOptions {
                composition: Some(COMPOSITION),
                ..AfterEffectsImportOptions::default()
            },
            ConversionMode::Write,
        )
        .unwrap();
    imported.join(OUTPUT_NAME)
}

#[test]
fn source_input_transform_lut_exports_as_a_head_of_stack_apply_color_lut() {
    let root = tempfile::tempdir().unwrap();
    let archive_path = import_fixture(root.path());
    let source_look = look_transforms(&read_project(FIXTURE).unwrap()).remove(0);

    // Move the imported LUT from its wrapper Group to the footage's
    // source.inputTransform and give the footage an authored Channel Mixer,
    // which the LUT must precede.
    let mut archive = TesseractFile::open(&archive_path).unwrap();
    let mut document = archive.project_json().unwrap();
    fn edit(layers: &mut serde_json::Value, look: &serde_json::Value) {
        for layer in layers.as_array_mut().into_iter().flatten() {
            if let Some(effects) = layer.get_mut("effects").and_then(|v| v.as_array_mut()) {
                effects.retain(|record| record["effect"]["type"] != "lookTransform");
            }
            if layer["type"] == "Image" {
                layer["source"]["inputTransform"] = serde_json::json!({
                    "semanticVersion": 1,
                    "transform": look["transform"],
                });
                layer["effects"] = serde_json::json!([{"id": 9100, "enabled": true, "effect": {
                    "type": "channelMixer", "semanticVersion": 1,
                    "red": {"red": 0.7, "green": 0.3, "blue": 0.0, "constant": 0.0},
                    "green": {"red": 0.0, "green": 1.0, "blue": 0.0, "constant": 0.0},
                    "blue": {"red": 0.0, "green": 0.0, "blue": 1.0, "constant": 0.1},
                    "monochrome": false
                }}]);
            }
            if let Some(children) = layer.get_mut("layers") {
                edit(children, look);
            }
        }
    }
    edit(&mut document["composition"]["layers"], &source_look);
    archive
        .replace_project(
            fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap(),
        )
        .unwrap();
    archive.save().unwrap();

    let exported = root.path().join("exported");
    let export = AfterEffects
        .export_from_tesseract(
            &archive_path,
            &exported,
            &AfterEffectsExportOptions {
                fps: 30.0,
                hdr: None,
            },
            ConversionMode::Write,
        )
        .unwrap();
    let messages: Vec<_> = export
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect();
    assert!(
        messages.iter().any(|message| message
            .contains("source.inputTransform written as a head-of-stack Apply Color LUT")),
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.contains("source.inputTransform omitted")),
        "{messages:?}"
    );
    let written = read_project(&fs::read(exported.join("project.aep")).unwrap()).unwrap();
    // The first Apply Color LUT of the stack carries the same CLUT AE wrote.
    let (_, native) = first_instance(&read_project(FIXTURE).unwrap());
    let (_, head) = first_instance(&written);
    assert_eq!(
        apply_color_lut::decode_sequence_data(&sequence_data(&head)).unwrap(),
        apply_color_lut::decode_sequence_data(&sequence_data(&native)).unwrap()
    );
    // The footage layer's stack: the source LUT first, then the authored effect.
    assert_eq!(
        effect_stack(&written),
        [apply_color_lut::MATCH_NAME, "ADBE CHANNEL MIXER"]
    );
    // Re-import reads the source LUT as the layer's first, full-strength lookTransform.
    assert_eq!(look_transforms(&written), vec![source_look]);
}

/// Match names of the effect stack of the first layer holding an Apply Color LUT.
fn effect_stack(project: &StructuralProject) -> Vec<String> {
    for item in &project.items {
        let ItemKind::Composition(composition) = &item.kind else {
            continue;
        };
        for layer in &composition.layers {
            let roots = properties::root_runs(&layer.content).unwrap();
            let Some((_, parade)) = roots.iter().find(|(name, _)| *name == "ADBE Effect Parade")
            else {
                continue;
            };
            let names: Vec<String> =
                properties::runs(properties::unique_list(parade, *b"tdgp").unwrap())
                    .unwrap()
                    .iter()
                    .map(|(name, _)| (*name).to_owned())
                    .collect();
            if names.iter().any(|name| name == apply_color_lut::MATCH_NAME) {
                return names;
            }
        }
    }
    panic!("no Apply Color LUT instance");
}
