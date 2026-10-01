use fx_schema::{AnimationGraph, CompositionId, EditableFxCompositionDocument, FXComposition};
use serde_json::json;
use std::io::{Seek, SeekFrom, Write};
use tesseract_file::{
    AssetKind, Generator, MaterializationCache, TesseractFile, TesseractFileBuilder,
    METADATA_JSON_SCHEMA,
};
use zip::write::SimpleFileOptions;

fn patch_central_sizes(bytes: &mut [u8], name: &str, compressed: u32, uncompressed: u32) {
    let signature = [0x50, 0x4b, 0x01, 0x02];
    let mut offset = 0;
    while let Some(relative) = bytes[offset..]
        .windows(signature.len())
        .position(|window| window == signature)
    {
        let header = offset + relative;
        let name_length = u16::from_le_bytes([bytes[header + 28], bytes[header + 29]]) as usize;
        let name_start = header + 46;
        if &bytes[name_start..name_start + name_length] == name.as_bytes() {
            bytes[header + 20..header + 24].copy_from_slice(&compressed.to_le_bytes());
            bytes[header + 24..header + 28].copy_from_slice(&uncompressed.to_le_bytes());
            return;
        }
        offset = name_start + name_length;
    }
    panic!("missing central-directory entry {name:?}");
}

fn test_document() -> EditableFxCompositionDocument {
    EditableFxCompositionDocument::from_json_value(json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1,
        "dimensions": { "width": 1080, "height": 1920 },
        "duration": 6,
        "composition": { "id": "composition-1", "name": "Test", "layers": [] }
    }))
    .unwrap()
}

#[test]
fn rejects_stored_entry_with_mismatched_sizes_before_reading_document() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mismatched-sizes.tsrct");
    TesseractFileBuilder::new(test_document())
        .write(&path)
        .unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    patch_central_sizes(&mut bytes, "metadata.json", 2, 3);
    std::fs::write(&path, bytes).unwrap();

    let error = TesseractFile::open(&path).unwrap_err().to_string();
    assert!(
        error.contains("Stored sizes differ"),
        "unexpected error: {error}"
    );
}

#[test]
fn rejects_stored_entry_outside_physical_data_range_before_reading_document() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("out-of-range.tsrct");
    TesseractFileBuilder::new(test_document())
        .write(&path)
        .unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    patch_central_sizes(&mut bytes, "metadata.json", 4096, 4096);
    std::fs::write(&path, bytes).unwrap();

    let error = TesseractFile::open(&path).unwrap_err().to_string();
    assert!(
        error.contains("outside the physical ZIP data range"),
        "unexpected error: {error}"
    );
}

#[test]
fn projects_above_previous_writer_limit_are_publishable() {
    let directory = tempfile::tempdir().unwrap();
    let document = test_document();
    let composition = FXComposition::try_from_parts(
        CompositionId::new("large"),
        "\u{1}".repeat(12 * 1024 * 1024),
        AnimationGraph::default(),
        Vec::new(),
    )
    .unwrap();
    let project = EditableFxCompositionDocument::new(
        document.dimensions(),
        document.duration(),
        None,
        composition,
    )
    .unwrap();
    let byte_len = project.to_json_vec().unwrap().len() as u64;
    assert!(byte_len > 64 * 1024 * 1024);

    let path = directory.path().join("large.tsrct");
    let file = TesseractFileBuilder::try_new(project)
        .unwrap()
        .write(&path)
        .unwrap();
    assert_eq!(file.metadata().project.byte_length, byte_len);
    assert_eq!(file.project_json_bytes().len() as u64, byte_len);
}

#[test]
fn embedded_metadata_schema_is_valid_json() {
    let schema: serde_json::Value = serde_json::from_str(METADATA_JSON_SCHEMA).unwrap();
    assert!(schema["$defs"]["project"]["properties"]["byteLength"]
        .get("maximum")
        .is_none());
    assert!(schema["properties"]["assets"]
        .get("maxProperties")
        .is_none());
    assert!(schema["properties"]["assets"]["propertyNames"]
        .get("maxLength")
        .is_none());
    assert!(schema["$defs"]["archivePath"].get("maxLength").is_none());
    assert_eq!(schema["$id"], "urn:tesseract:file:metadata:v2");
    assert_eq!(schema["properties"]["fxSchemaVersion"]["type"], "integer");
    assert!(!schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "fxSchemaVersion"));
}

#[test]
fn json_layout_rejects_the_protobuf_archive_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("protobuf-version.tsrct");
    let metadata = json!({
        "$schema": "urn:tesseract:file:metadata:v2",
        "format": "tesseract",
        "formatVersion": 1,
        "documentId": "00000000-0000-0000-0000-000000000001",
        "createdAt": "2026-01-01T00:00:00Z",
        "modifiedAt": "2026-01-01T00:00:00Z",
        "generator": {
            "name": "test",
            "version": "1",
            "engineVersion": "1"
        },
        "project": {
            "path": "project.json",
            "contentType": "application/vnd.tesseract.fx-composition+json",
            "byteLength": 0,
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        },
        "assets": {}
    });
    let output = std::fs::File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(output);
    writer
        .start_file("metadata.json", SimpleFileOptions::default())
        .unwrap();
    writer
        .write_all(&serde_json::to_vec(&metadata).unwrap())
        .unwrap();
    writer.finish().unwrap();

    let error = TesseractFile::open(&path).unwrap_err().to_string();
    assert!(error.contains("unsupported formatVersion 1; expected 2"));
}

#[test]
fn metadata_above_previous_writer_limit_is_publishable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large-metadata.tsrct");
    let generator = Generator {
        name: "x".repeat(1024 * 1024),
        version: "1".to_owned(),
        engine_version: "1".to_owned(),
        git_revision: None,
    };

    let file = TesseractFileBuilder::new(test_document())
        .generator(generator)
        .unwrap()
        .write(&path)
        .unwrap();
    assert!(serde_json::to_vec_pretty(file.metadata()).unwrap().len() > 1024 * 1024);
    assert_eq!(file.metadata().generator.name.len(), 1024 * 1024);
}

#[test]
fn asset_and_entry_counts_above_previous_limits_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("empty.bin");
    std::fs::write(&source, []).unwrap();
    let mut builder = TesseractFileBuilder::new(test_document());
    for index in 0..4_097 {
        builder = builder
            .add_asset(format!("asset-{index}"), &source, AssetKind::Other)
            .unwrap();
    }

    let path = directory.path().join("many-assets.tsrct");
    let file = builder.write(&path).unwrap();
    assert_eq!(file.metadata().assets.len(), 4_097);
    assert_eq!(
        TesseractFile::open(&path).unwrap().metadata().assets.len(),
        4_097
    );
}

#[test]
fn uppercase_digests_are_accepted_for_open_and_materialization() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    std::fs::write(&source, b"asset bytes").unwrap();
    let original_path = directory.path().join("original.tsrct");
    let project = test_document();
    let file = TesseractFileBuilder::new(project)
        .add_asset("video", &source, AssetKind::Video)
        .unwrap()
        .write(&original_path)
        .unwrap();
    let mut metadata = file.metadata().clone();
    metadata.project.sha256.make_ascii_uppercase();
    metadata
        .assets
        .get_mut("video")
        .unwrap()
        .sha256
        .make_ascii_uppercase();
    drop(file);

    let uppercase_path = directory.path().join("uppercase.tsrct");
    let input = std::fs::File::open(&original_path).unwrap();
    let mut archive = zip::ZipArchive::new(input).unwrap();
    let output = std::fs::File::create(&uppercase_path).unwrap();
    let mut writer = zip::ZipWriter::new(output);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_string();
        writer
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        if name == "metadata.json" {
            writer
                .write_all(&serde_json::to_vec_pretty(&metadata).unwrap())
                .unwrap();
        } else {
            std::io::copy(&mut entry, &mut writer).unwrap();
        }
    }
    writer.finish().unwrap();

    let file = TesseractFile::open(&uppercase_path).unwrap();
    let cache = MaterializationCache::new(directory.path().join("cache"));
    let materialized = file.asset("video").unwrap().materialize(&cache).unwrap();
    assert_eq!(std::fs::read(materialized.path()).unwrap(), b"asset bytes");
}

#[test]
fn cache_separates_content_and_marker_names() {
    let directory = tempfile::tempdir().unwrap();
    let source_dir = directory.path().join("sources");
    std::fs::create_dir(&source_dir).unwrap();
    let first_source = source_dir.join("foo");
    let second_source = source_dir.join("foo.verified");
    std::fs::write(&first_source, b"same bytes").unwrap();
    std::fs::write(&second_source, b"same bytes").unwrap();
    let path = directory.path().join("collision.tsrct");
    let project = test_document();
    let file = TesseractFileBuilder::new(project)
        .add_asset("first", &first_source, AssetKind::Other)
        .unwrap()
        .add_asset("second", &second_source, AssetKind::Other)
        .unwrap()
        .write(&path)
        .unwrap();
    let cache = MaterializationCache::new(directory.path().join("cache"));
    let first = file.asset("first").unwrap().materialize(&cache).unwrap();
    let second = file.asset("second").unwrap().materialize(&cache).unwrap();
    assert_ne!(first.path(), second.path());
    assert_eq!(std::fs::read(first.path()).unwrap(), b"same bytes");
    assert_eq!(std::fs::read(second.path()).unwrap(), b"same bytes");
}

#[test]
fn rejects_non_normalized_paths_before_extracting_anything() {
    let directory = tempfile::tempdir().unwrap();
    for (index, archive_path) in ["../escape", "assets//clip.mp4", "assets/./clip.mp4"]
        .into_iter()
        .enumerate()
    {
        let path = directory.path().join(format!("hostile-{index}.tsrct"));
        let output = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(output);
        writer
            .start_file(archive_path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"hostile").unwrap();
        writer.finish().unwrap();

        let error = TesseractFile::open(&path).unwrap_err().to_string();
        assert!(
            error.contains("unsafe archive path") || error.contains("non-normalized"),
            "unexpected rejection: {error}"
        );
    }
    assert!(!directory.path().join("escape").exists());
}

#[test]
fn corrupt_asset_is_rejected_when_lazily_materialized() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    std::fs::write(&source, b"original asset bytes").unwrap();
    let path = directory.path().join("corrupt.tsrct");
    let project = test_document();
    let file = TesseractFileBuilder::new(project)
        .add_asset("video", &source, AssetKind::Video)
        .unwrap()
        .write(&path)
        .unwrap();
    let entry_path = file.metadata().assets["video"].path.clone();
    drop(file);

    let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let data_start = archive.by_name(&entry_path).unwrap().data_start();
    drop(archive);
    let mut output = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    output.seek(SeekFrom::Start(data_start)).unwrap();
    output.write_all(b"X").unwrap();
    drop(output);

    let file = TesseractFile::open(&path).unwrap();
    let cache = MaterializationCache::new(directory.path().join("cache"));
    assert!(file
        .asset("video")
        .unwrap()
        .materialize(&cache)
        .unwrap_err()
        .to_string()
        .contains("integrity fields"));
}

#[test]
fn rejects_case_colliding_entry_names() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collision.tsrct");
    let output = std::fs::File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(output);
    writer
        .start_file("metadata.json", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"{}").unwrap();
    writer
        .start_file("METADATA.JSON", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"{}").unwrap();
    writer.finish().unwrap();

    assert!(TesseractFile::open(&path)
        .unwrap_err()
        .to_string()
        .contains("case-colliding"));
}

#[test]
fn rejects_previous_container_identity() {
    let directory = tempfile::tempdir().unwrap();
    let valid = directory.path().join("valid.tsrct");
    let file = TesseractFileBuilder::new(test_document())
        .write(&valid)
        .unwrap();
    for (field, value, expected) in [
        ("format", "jerboa", "unsupported format"),
        (
            "$schema",
            "https://jerboa.dev/schemas/file/v2/metadata.schema.json",
            "unsupported metadata schema",
        ),
    ] {
        let mut metadata = serde_json::to_value(file.metadata()).unwrap();
        metadata[field] = json!(value);
        let path = directory.path().join("old.tsrct");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        writer
            .start_file("metadata.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(&metadata).unwrap())
            .unwrap();
        writer.finish().unwrap();
        assert!(TesseractFile::open(path)
            .unwrap_err()
            .to_string()
            .contains(expected));
    }
}
