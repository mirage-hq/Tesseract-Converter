use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use fx_conv::ExportFromTesseract;
use fx_schema::EditableFxCompositionDocument;
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

use super::*;

fn transform() -> serde_json::Value {
    serde_json::json!({
        "anchorPoint":[0.0,0.0], "position":[0.0,0.0], "scale":[100.0,100.0],
        "rotation":0.0, "opacity":100.0
    })
}

fn archive_with_assets(
    root: &Path,
    layers: Vec<serde_json::Value>,
    assets: &[(&str, &Path, AssetKind)],
) -> PathBuf {
    let imported = import_solid(root);
    let source = TesseractFile::open(imported).unwrap();
    let mut value = source.project_json().unwrap();
    value["composition"]["layers"] = layers.into();
    value["composition"]["dynamics"] = serde_json::json!({"entries":[]});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let mut builder = TesseractFileBuilder::try_new(document).unwrap();
    for (id, path, kind) in assets {
        builder = builder.add_asset(*id, path, *kind).unwrap();
    }
    let path = root.join("media-input.tsrct");
    drop(builder.write(&path).unwrap());
    path
}

fn exr(width: i32, height: i32) -> Vec<u8> {
    let mut bytes = 20_000_630_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(b"dataWindow\0box2i\0");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&(width - 1).to_le_bytes());
    bytes.extend_from_slice(&(height - 1).to_le_bytes());
    bytes.push(0);
    bytes
}

fn image_layer(id: u64, asset_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type":"Image", "id":id, "name":asset_id, "parent":null,
        "activeRange":{"start":0,"duration":1000}, "transform":transform(),
        "source":{"assetId":asset_id,"fit":"contain"}
    })
}

fn assert_collision_resistant_asset_pair(left_id: &str, right_id: &str) {
    let root = tempfile::tempdir().unwrap();
    let left_path = root.path().join("left.exr");
    let right_path = root.path().join("right.exr");
    let left_bytes = exr(11, 13);
    let right_bytes = exr(17, 19);
    fs::write(&left_path, &left_bytes).unwrap();
    fs::write(&right_path, &right_bytes).unwrap();
    let input = archive_with_assets(
        root.path(),
        vec![image_layer(820, left_id), image_layer(821, right_id)],
        &[
            (left_id, &left_path, AssetKind::Image),
            (right_id, &right_path, AssetKind::Image),
        ],
    );
    let output = root.path().join("output");

    let checked = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    let written = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    assert_eq!(checked, written);

    let mut names = Vec::new();
    let mut contents = Vec::new();
    for entry in fs::read_dir(output.join("media")).unwrap() {
        let entry = entry.unwrap();
        names.push(entry.file_name().into_string().unwrap());
        contents.push(fs::read(entry.path()).unwrap());
    }
    assert_eq!(names.len(), 2);
    let case_folded: BTreeSet<_> = names.iter().map(|name| name.to_ascii_lowercase()).collect();
    assert_eq!(case_folded.len(), 2, "package paths: {names:?}");
    names.sort();
    let mut sorted_ids = [left_id, right_id];
    sorted_ids.sort_unstable();
    let expected_names: Vec<_> = sorted_ids
        .iter()
        .enumerate()
        .map(|(ordinal, asset_id)| {
            let readable: String = asset_id
                .chars()
                .take(48)
                .map(|value| {
                    if value.is_ascii_alphanumeric() || matches!(value, '-' | '_') {
                        value
                    } else {
                        '_'
                    }
                })
                .collect();
            format!("{ordinal:06}-{readable}.exr")
        })
        .collect();
    assert_eq!(names, expected_names);
    contents.sort();
    let mut expected = vec![left_bytes, right_bytes];
    expected.sort();
    assert_eq!(contents, expected);
    assert!(!output.join(".asset-materialization-cache").exists());
}

#[test]
fn review_package_distinguishes_punctuation_collisions_in_check_and_write() {
    assert_collision_resistant_asset_pair("asset.a", "asset_a");
}

#[test]
fn review_package_distinguishes_truncation_collisions_in_check_and_write() {
    let prefix = "x".repeat(64);
    assert_collision_resistant_asset_pair(&format!("{prefix}a"), &format!("{prefix}b"));
}

#[test]
fn review_package_distinguishes_case_collisions_in_check_and_write() {
    assert_collision_resistant_asset_pair("CaseSensitiveAsset", "casesensitiveasset");
}

#[test]
fn review_package_rejects_integrity_mismatch_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let asset_path = root.path().join("corrupt.exr");
    let asset_bytes = exr(23, 29);
    fs::write(&asset_path, &asset_bytes).unwrap();
    let input = archive_with_assets(
        root.path(),
        vec![image_layer(822, "corrupt-image")],
        &[("corrupt-image", &asset_path, AssetKind::Image)],
    );
    let mut archive_bytes = fs::read(&input).unwrap();
    let matches: Vec<_> = archive_bytes
        .windows(asset_bytes.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == asset_bytes).then_some(offset))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "asset payload must be unique in the test archive"
    );
    archive_bytes[matches[0] + asset_bytes.len() - 2] ^= 1;
    fs::write(&input, archive_bytes).unwrap();

    let output = root.path().join("output");
    let error = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap_err();
    assert!(error.to_string().contains("integrity fields"), "{error}");
    assert!(!output.exists());
    assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".conversion-aftereffects-")
    }));
}
