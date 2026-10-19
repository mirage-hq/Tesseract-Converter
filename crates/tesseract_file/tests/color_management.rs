use std::io::Write;

use fx_schema::EditableFxCompositionDocument;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tesseract_file::{TesseractFile, TesseractFileBuilder};
use zip::write::SimpleFileOptions;

fn archive(path: &std::path::Path, policy: Option<Value>) {
    let mut value = json!({"$schema":fx_schema::EDITABLE_FX_DOCUMENT_SCHEMA_URL,"formatVersion":1,
        "dimensions":{"width":32,"height":32},"duration":1,
        "future":{"colorManagement":"unrelated metadata"},
        "composition":{"id":"main","name":"Main","layers":[]}});
    let document = EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let file = TesseractFileBuilder::new(document).write(path).unwrap();
    let mut metadata = serde_json::to_value(file.metadata()).unwrap();
    drop(file);
    if let Some(policy) = policy {
        value["colorManagement"] = policy;
    }
    let bytes = serde_json::to_vec(&value).unwrap();
    metadata["project"]["byteLength"] = json!(bytes.len());
    metadata["project"]["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("metadata.json", options).unwrap();
    writer
        .write_all(&serde_json::to_vec(&metadata).unwrap())
        .unwrap();
    writer.start_file("project.json", options).unwrap();
    writer.write_all(&bytes).unwrap();
    writer.finish().unwrap();
}

#[test]
fn color_management_historical_checkout_and_save_normalize_without_mutating_source_on_open() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.tsrct");
    let checkout = directory.path().join("project.json");
    archive(&path, None);
    let original = std::fs::read(&path).unwrap();
    let mut file = TesseractFile::open(&path).unwrap();
    assert!(!serde_json::from_slice::<Value>(file.project_json_bytes())
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("colorManagement"));
    file.checkout_project_json(&checkout).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let checked: Value = serde_json::from_slice(&std::fs::read(checkout).unwrap()).unwrap();
    assert_eq!(checked["colorManagement"]["workingSpace"], "legacyDisplay");
    file.save().unwrap();
    let reopened = TesseractFile::open(path).unwrap();
    let bytes = reopened.project_json_bytes();
    assert_eq!(reopened.metadata().project.byte_length, bytes.len() as u64);
    assert_eq!(
        reopened.metadata().project.sha256,
        format!("{:x}", Sha256::digest(bytes))
    );
    assert_eq!(
        serde_json::from_slice::<Value>(bytes).unwrap()["colorManagement"],
        json!({"workingSpace":"legacyDisplay"})
    );
}

#[test]
fn color_management_historical_raw_noop_and_typed_edit_writes() {
    for policy in [
        json!({"version":1,"workingSpace":"legacyDisplay"}),
        json!({"version":2,"workingSpace":"legacyDisplay"}),
        json!({"version":"1","workingSpace":"legacyDisplay"}),
        json!({"version":null,"workingSpace":"legacyDisplay"}),
        json!({"version":1.5,"workingSpace":"legacyDisplay"}),
        json!({"version":{"future":true},"workingSpace":"legacyDisplay"}),
        json!({"workingSpace":"legacyDisplay"}),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("explicit.tsrct");
        archive(&path, Some(policy));
        let mut file = TesseractFile::open(&path).unwrap();
        let original = file.project_json_bytes().to_vec();
        file.save().unwrap();
        assert_eq!(file.project_json_bytes(), original);
        assert_eq!(
            TesseractFile::open(&path).unwrap().project_json_bytes(),
            original
        );

        let mut typed = file.project_json().unwrap();
        typed["composition"]["name"] = json!("Edited");
        let document = EditableFxCompositionDocument::from_json_value(typed).unwrap();
        let output = directory.path().join("edited.tsrct");
        TesseractFileBuilder::new(document).write(&output).unwrap();
        let saved = TesseractFile::open(output).unwrap().project_json().unwrap();
        assert_eq!(saved["composition"]["name"], "Edited");
        assert_eq!(
            saved["colorManagement"],
            json!({"workingSpace":"legacyDisplay"})
        );
    }
}

#[test]
fn color_management_aces_archive_noop_commit_and_save_are_structural_only() {
    for compositing in ["footageOnly", "wholeProgram", "perClip"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("aces.tsrct");
        let policy =
            json!({"workingSpace":"acescct","outputTransform":"aces2.0","compositing":compositing});
        archive(&path, Some(policy.clone()));
        let mut file = TesseractFile::open(&path).unwrap();
        let original = file.project_json_bytes().to_vec();
        file.save().unwrap();
        assert_eq!(file.project_json_bytes(), original);
        let checkout = directory.path().join("project.json");
        file.checkout_project_json(&checkout).unwrap();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&checkout).unwrap()).unwrap();
        value["composition"]["name"] = json!("Edited ACES");
        std::fs::write(&checkout, serde_json::to_vec(&value).unwrap()).unwrap();
        file.commit_project_json(&checkout).unwrap();
        file.save().unwrap();
        let saved = TesseractFile::open(path).unwrap().project_json().unwrap();
        assert_eq!(saved["colorManagement"], policy);
        assert_eq!(saved["composition"]["name"], "Edited ACES");
    }
}

#[test]
fn color_management_valid_zip_hash_does_not_make_future_policy_safe() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("future.tsrct");
    archive(
        &path,
        Some(json!({"version":1,"workingSpace":"futureSpace"})),
    );
    let error = TesseractFile::open(path).unwrap_err().to_string();
    assert!(
        error.contains(
            "unsupported color management workingSpace \"futureSpace\", requires a newer Jerboa"
        ),
        "{error}"
    );
}
