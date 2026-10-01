use fx_schema::{
    EditableFxCompositionDocument, FontAssetProperties, FontFaceMetadata, MotionBlurSettings,
};
use serde_json::json;
use std::io::{Read, Seek, SeekFrom};
use tesseract_file::{
    AssetKind, Generator, MaterializationCache, SaveStrategy, TesseractFile, TesseractFileBuilder,
};

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

fn font_properties() -> FontAssetProperties {
    FontAssetProperties {
        file_name: Some("Brand.ttf".to_owned()),
        mime_type: Some("font/ttf".to_owned()),
        faces: vec![FontFaceMetadata {
            face_index: 0,
            postscript_name: "Brand-Regular".to_owned(),
            full_name: "Brand Regular".to_owned(),
            family_name: "Brand".to_owned(),
            style_name: "Regular".to_owned(),
            typographic_family_name: None,
            typographic_style_name: None,
            weight: 400,
            width: 5,
            italic: false,
            is_serif: false,
            covered_scripts: Vec::new(),
            variation_axes: Vec::new(),
            variation_instances: Vec::new(),
            selection_names: vec!["Brand/Regular".to_owned()],
        }],
        ..FontAssetProperties::default()
    }
}

fn enable_motion_blur(file: &mut TesseractFile) {
    let mut candidate = file.project().clone();
    candidate
        .composition_mut()
        .set_motion_blur(MotionBlurSettings {
            enabled: true,
            ..MotionBlurSettings::default()
        })
        .unwrap();
    file.replace_project(candidate).unwrap();
}

#[test]
fn creates_reads_edits_saves_and_reopens() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    std::fs::write(&source, b"0123456789-user-video").unwrap();
    let output = directory.path().join("edit.tsrct");

    let mut file = TesseractFileBuilder::new(test_document())
        .add_asset("source_video", &source, AssetKind::Video)
        .unwrap()
        .write(&output)
        .unwrap();

    assert_eq!(file.metadata().format, "tesseract");
    assert_eq!(file.metadata().format_version, 2);
    assert_eq!(
        file.metadata().fx_schema_version,
        Some(fx_schema::FX_SCHEMA_REVISION)
    );
    assert_eq!(file.metadata().schema, "urn:tesseract:file:metadata:v2");
    assert_eq!(
        file.metadata().project.content_type,
        "application/vnd.tesseract.fx-composition+json"
    );
    let generator = serde_json::to_value(&file.metadata().generator).unwrap();
    assert_eq!(generator["name"], "tesseract");
    assert!(generator["engineVersion"].is_string());
    assert!(generator.get("jerboaVersion").is_none());
    assert_eq!(file.metadata().project.path, "project.json");
    assert_eq!(file.metadata().assets.len(), 1);
    assert!(file.project_json().unwrap().is_object());

    let asset = file.asset("source_video").unwrap();
    let mut reader = asset.open().unwrap();
    reader.seek(SeekFrom::Start(10)).unwrap();
    let mut suffix = String::new();
    reader.read_to_string(&mut suffix).unwrap();
    assert_eq!(suffix, "-user-video");

    let cache = MaterializationCache::new(directory.path().join("cache"));
    let materialized = asset.materialize(&cache).unwrap();
    assert_eq!(
        std::fs::read(materialized.path()).unwrap(),
        b"0123456789-user-video"
    );
    let cached = file
        .asset("source_video")
        .unwrap()
        .materialize(&cache)
        .unwrap();
    assert_eq!(cached.path(), materialized.path());

    enable_motion_blur(&mut file);
    let report = file.save().unwrap();
    #[cfg(target_os = "macos")]
    assert_eq!(report.strategy, SaveStrategy::ReflinkAppend);
    #[cfg(not(target_os = "macos"))]
    assert_eq!(report.strategy, SaveStrategy::RawCopyRewrite);
    assert_eq!(report.reused_assets, 1);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&output).unwrap()).unwrap();
    assert_eq!(archive.len(), 3);
    let metadata: serde_json::Value =
        serde_json::from_reader(archive.by_name("metadata.json").unwrap()).unwrap();
    assert_eq!(
        metadata["fxSchemaVersion"],
        json!(fx_schema::FX_SCHEMA_REVISION)
    );

    let reopened = TesseractFile::open(&output).unwrap();
    assert!(reopened.project().composition().motion_blur().enabled);
    assert_eq!(
        reopened.metadata().fx_schema_version,
        Some(fx_schema::FX_SCHEMA_REVISION)
    );
    assert_eq!(
        std::fs::read(
            reopened
                .asset("source_video")
                .unwrap()
                .materialize(&cache)
                .unwrap()
                .path()
        )
        .unwrap(),
        b"0123456789-user-video"
    );
}

#[test]
fn newly_written_windowed_layers_reopen_with_independent_source_selection() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("windowed.tsrct");
    let window = |start, duration| json!({"start": start, "duration": duration});
    let mapping = json!({
        "type": "linear", "input": window(0, 1000), "output": window(0, 1000)
    });
    let playback = |start, duration| {
        json!({
            "type": "windowed", "inputRange": window(start, duration),
            "mapping": mapping, "inputOffsetMs": 0
        })
    };
    let document = json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1,
        "dimensions": {"width": 1080, "height": 1920},
        "duration": 1,
        "composition": {"id": "windowed", "name": "Windowed", "layers": [{
            "type": "Group", "id": 1, "name": "group",
            "playback": playback(0, 1000),
            "transform": {"anchorPoint": [0, 0], "position": [0, 0],
                "scale": [100, 100], "rotation": 0, "opacity": 100},
            "layers": [{
                "type": "Video", "id": 2, "name": "video", "parent": 1,
                "sourceRange": window(0, 1000), "sourceIntrinsicDuration": 1000,
                "source": {"assetId": ""},
                "playback": playback(250, 500),
                "transform": {"anchorPoint": [0, 0], "position": [0, 0],
                    "scale": [100, 100], "rotation": 0, "opacity": 100}
            }, {
                "type": "Audio", "id": 3, "name": "audio", "parent": 1,
                "sourceRange": window(250, 500), "sourceIntrinsicDuration": 1000,
                "source": {"assetId": ""}, "playback": playback(0, 1000)
            }]
        }]}
    });
    let project_bytes = serde_json::to_vec(&document).unwrap();
    TesseractFileBuilder::from_project_json(&project_bytes)
        .unwrap()
        .write(&output)
        .unwrap();
    let reopened = TesseractFile::open(&output).unwrap();
    let layers = &reopened.project_json().unwrap()["composition"]["layers"][0]["layers"];
    assert_eq!(layers[0]["playback"]["inputRange"], window(250, 500));
    assert_eq!(layers[0]["sourceRange"], window(0, 1000));
    assert_eq!(layers[1]["playback"]["inputRange"], window(0, 1000));
    assert_eq!(layers[1]["sourceRange"], window(250, 500));
    assert!(reopened.project_json().unwrap()["composition"]["layers"][0]
        .get("activeRange")
        .is_none());
}

#[test]
fn font_registry_round_trips_separately_from_asset_descriptors() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("Brand.ttf");
    let package = directory.path().join("font.tsrct");
    std::fs::write(
        &source,
        b"font bytes are validated by the consuming inspector",
    )
    .unwrap();

    let file = TesseractFileBuilder::new(test_document())
        .add_font_asset("opaque-key", &source, font_properties())
        .unwrap()
        .write(&package)
        .unwrap();

    assert_eq!(file.metadata().assets["opaque-key"].kind, AssetKind::Font);
    assert_eq!(file.metadata().fonts["opaque-key"], font_properties());
    let metadata_json = serde_json::to_value(file.metadata()).unwrap();
    assert!(metadata_json["assets"]["opaque-key"].get("faces").is_none());
    assert_eq!(
        metadata_json["fonts"]["opaque-key"]["faces"][0]["familyName"],
        "Brand"
    );
}

#[test]
fn font_registry_rejects_missing_and_non_font_asset_keys() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("registry-errors.tsrct");
    TesseractFileBuilder::new(test_document())
        .write(&package)
        .unwrap();
    let mut file = TesseractFile::open(&package).unwrap();

    let missing = file
        .set_font_properties("missing", font_properties())
        .unwrap_err();
    assert!(missing.to_string().contains("missing from assets"));

    let image = directory.path().join("image.bin");
    std::fs::write(&image, b"not relevant to structural metadata validation").unwrap();
    file.add_asset("not-a-font", &image, AssetKind::Image)
        .unwrap();
    let wrong_kind = file
        .set_font_properties("not-a-font", font_properties())
        .unwrap_err();
    assert!(wrong_kind
        .to_string()
        .contains("does not identify a font asset"));
}

#[test]
fn removing_font_asset_removes_its_registry_entry() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("Brand.ttf");
    let package = directory.path().join("remove-font.tsrct");
    std::fs::write(&source, b"font bytes").unwrap();
    TesseractFileBuilder::new(test_document())
        .add_font_asset("opaque-key", &source, font_properties())
        .unwrap()
        .write(&package)
        .unwrap();

    let mut file = TesseractFile::open(&package).unwrap();
    file.remove_asset("opaque-key").unwrap();
    file.save().unwrap();
    let reopened = TesseractFile::open(&package).unwrap();
    assert!(!reopened.metadata().assets.contains_key("opaque-key"));
    assert!(!reopened.metadata().fonts.contains_key("opaque-key"));
}

#[test]
fn media_only_metadata_omits_empty_font_registry() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("media-only.tsrct");
    let file = TesseractFileBuilder::new(test_document())
        .write(&package)
        .unwrap();
    let metadata = serde_json::to_value(file.metadata()).unwrap();
    assert!(metadata.get("fonts").is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn project_only_save_does_not_rewrite_stored_media_on_apfs() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("large.mp4");
    std::fs::write(&source, vec![0x5a; 8 * 1024 * 1024]).unwrap();
    let output = directory.path().join("large.tsrct");
    let mut file = TesseractFileBuilder::new(test_document())
        .add_asset("video", &source, AssetKind::Video)
        .unwrap()
        .write(&output)
        .unwrap();

    enable_motion_blur(&mut file);
    let report = file.save().unwrap();

    assert_eq!(report.strategy, SaveStrategy::ReflinkAppend);
    assert!(report.bytes_written < 1024 * 1024, "{report:?}");
    assert_eq!(
        file.asset("video").unwrap().descriptor().byte_length,
        8 * 1024 * 1024
    );
}

#[test]
fn legacy_records_and_required_asset_survive_archive_edit_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mp4");
    std::fs::write(&source, b"stored-media-bytes").unwrap();
    let output = directory.path().join("legacy.tsrct");
    let original = json!({
        "$schema": "https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion": 1, "dimensions": {"width": 1080, "height": 1920}, "duration": 6,
        "futureEnvelope": {"preserved": true},
        "composition": {
            "id": "composition-1", "name": "Legacy", "futureComposition": [1, 1],
            "layers": [{
                "type": "Media", "id": 1, "name": "source", "activeRange": {"start": 0, "duration": 6000},
                "source": {"assetId": "source_video", "kind": "video", "fit": "none", "futureSource": null},
                "transform": {"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100},
                "effects": [{"type": "shaderPreset", "presetId": "historical"}]
            }],
            "dynamics": {"entries": [{
                "property": fx_schema::Property::new(fx_schema::LayerId::new(1), fx_schema::PropType::Rotation),
                "animator": {"type": "jsScript", "code": "legacy source", "layerTimeJsCode": "other source"}
            }]}
        }
    });
    let bytes = serde_json::to_vec_pretty(&original).unwrap();
    let mut file = TesseractFileBuilder::from_project_json(&bytes)
        .unwrap()
        .add_asset("source_video", &source, AssetKind::Video)
        .unwrap()
        .write(&output)
        .unwrap();
    assert_eq!(file.project_json_bytes(), bytes);
    enable_motion_blur(&mut file);
    file.save().unwrap();
    let reopened = TesseractFile::open(&output).unwrap();
    assert!(reopened.asset("source_video").is_ok());
    let mut saved = reopened.project_json().unwrap();
    assert_eq!(saved["composition"]["motionBlur"]["enabled"], true);
    saved["composition"]
        .as_object_mut()
        .unwrap()
        .remove("motionBlur");
    assert_eq!(saved, original);
}

#[test]
fn json_builder_preserves_unknown_fields_and_generator() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("from-json.tsrct");
    let project_json = br#"{
        "$schema":"https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion":1,
        "dimensions":{"width":1080,"height":1920},
        "duration":6,
        "composition":{"id":"composition-1","name":"Test","layers":[],"futureFx":7},
        "futureEnvelope":{"kept":true}
    }"#;
    let generator = Generator {
        name: "test-writer".to_string(),
        version: "1.2.3".to_string(),
        engine_version: "0.271.0".to_string(),
        git_revision: Some("abcdef0123456789".to_string()),
    };

    let mut file = TesseractFileBuilder::from_project_json(project_json)
        .unwrap()
        .generator(generator.clone())
        .unwrap()
        .write(&output)
        .unwrap();
    assert_eq!(file.project_json_bytes(), project_json);
    enable_motion_blur(&mut file);
    assert_ne!(file.project_json_bytes(), project_json);
    file.save().unwrap();

    let saved = file.project_json().unwrap();
    assert_eq!(saved["composition"]["futureFx"], 7);
    assert_eq!(saved["futureEnvelope"]["kept"], true);
    assert_eq!(file.metadata().generator, generator);
}

#[test]
fn agent_can_edit_a_checked_out_json_file_without_format_churn() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("agent-edit.tsrct");
    let checkout = directory.path().join("project.json");
    let mut file = TesseractFileBuilder::new(test_document())
        .write(&output)
        .unwrap();
    file.checkout_project_json(&checkout).unwrap();
    let original = std::fs::read_to_string(&checkout).unwrap();
    let edited = original.replacen("\"name\": \"Test\"", "\"name\":\"Agent edit\"", 1);
    assert_ne!(edited, original);
    std::fs::write(&checkout, edited.as_bytes()).unwrap();

    file.commit_project_json(&checkout).unwrap();
    assert_eq!(file.project_json_bytes(), edited.as_bytes());
    file.save().unwrap();

    let reopened = TesseractFile::open(output).unwrap();
    assert_eq!(reopened.project_json_bytes(), edited.as_bytes());
    assert_eq!(
        reopened.project_json().unwrap()["composition"]["name"],
        "Agent edit"
    );
}

#[test]
fn agent_json_requires_packaged_or_explicit_runtime_assets() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("agent-assets.tsrct");
    let mut file = TesseractFileBuilder::new(test_document())
        .write(&output)
        .unwrap();
    let before = file.project_json().unwrap();
    let mut edited = before.clone();
    edited["composition"]["layers"] = json!([{
        "id": 1,
        "name": "Runtime video",
        "type": "Video",
        "blendMode": "normal",
        "playback": {
            "type": "windowed",
            "inputRange": { "start": 0, "duration": 6000 },
            "mapping": {
                "type": "linear",
                "input": { "start": 0, "duration": 6000 },
                "output": { "start": 0, "duration": 6000 }
            },
            "inputOffsetMs": 0
        },
        "sourceRange": { "start": 0, "duration": 6000 },
        "sourceIntrinsicDuration": 6000,
        "transform": {
            "anchorPoint": [0, 0],
            "position": [0, 0],
            "scale": [100, 100],
            "rotation": 0,
            "opacity": 100
        },
        "source": { "assetId": "runtime-video", "fit": "contain" }
    }]);

    let checkout = directory.path().join("asset-project.json");
    std::fs::write(&checkout, serde_json::to_vec_pretty(&edited).unwrap()).unwrap();
    assert!(file.commit_project_json(&checkout).is_err());
    assert_eq!(file.project_json().unwrap(), before);
    file.commit_project_json_with_runtime_assets(&checkout, |asset| {
        asset.asset_id == "runtime-video"
    })
    .unwrap();
    assert!(file.save().is_err());
    file.save_with_runtime_assets(|asset| asset.asset_id == "runtime-video")
        .unwrap();
    assert_eq!(
        TesseractFile::open(output).unwrap().project_json().unwrap()["composition"]["layers"][0]
            ["source"]["assetId"],
        "runtime-video"
    );
}

#[test]
fn invalid_agent_json_edit_is_transactional() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("invalid-agent-edit.tsrct");
    let mut file = TesseractFileBuilder::new(test_document())
        .write(&output)
        .unwrap();
    let before = file.project_json().unwrap();
    let before_bytes = file.project_json_bytes().to_vec();
    let mut invalid = before.clone();
    invalid["dimensions"]["width"] = json!(0);
    let checkout = directory.path().join("invalid-project.json");
    std::fs::write(&checkout, serde_json::to_vec(&invalid).unwrap()).unwrap();

    assert!(file.commit_project_json(&checkout).is_err());
    assert_eq!(file.project_json().unwrap(), before);
    assert_eq!(file.project_json_bytes(), before_bytes);
}

#[test]
fn typed_project_replacement_rejects_unpackaged_assets_transactionally() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("invalid-typed-replacement.tsrct");
    let mut file = TesseractFileBuilder::new(test_document())
        .write(&output)
        .unwrap();
    let before_archive = std::fs::read(&output).unwrap();
    let before_project = file.project().clone();
    let before_bytes = file.project_json_bytes().to_vec();
    let mut candidate = file.project_json().unwrap();
    candidate["composition"]["layers"] = json!([{
        "id": 1,
        "name": "Unpackaged video",
        "type": "Video",
        "blendMode": "normal",
        "playback": {
            "type": "windowed",
            "inputRange": { "start": 0, "duration": 6000 },
            "mapping": {
                "type": "linear",
                "input": { "start": 0, "duration": 6000 },
                "output": { "start": 0, "duration": 6000 }
            },
            "inputOffsetMs": 0
        },
        "sourceRange": { "start": 0, "duration": 6000 },
        "sourceIntrinsicDuration": 6000,
        "transform": {
            "anchorPoint": [0, 0],
            "position": [0, 0],
            "scale": [100, 100],
            "rotation": 0,
            "opacity": 100
        },
        "source": { "assetId": "missing-video", "fit": "contain" }
    }]);
    let candidate = EditableFxCompositionDocument::from_json_value(candidate).unwrap();

    assert!(file.replace_project(candidate).is_err());
    assert_eq!(file.project(), &before_project);
    assert_eq!(file.project_json_bytes(), before_bytes);
    assert_eq!(std::fs::read(&output).unwrap(), before_archive);
}

#[test]
fn save_rejects_a_source_changed_after_open() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("changed.tsrct");
    TesseractFileBuilder::new(test_document())
        .write(&path)
        .unwrap();
    let mut file = TesseractFile::open(&path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(0);
    std::fs::write(&path, bytes).unwrap();

    assert!(file
        .save()
        .unwrap_err()
        .to_string()
        .contains("changed after it was opened"));
}

#[test]
fn save_as_preserves_source_file() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.tsrct");
    let revised_path = directory.path().join("revised.tsrct");
    let mut file = TesseractFileBuilder::new(test_document())
        .write(&source_path)
        .unwrap();
    let original = std::fs::read(&source_path).unwrap();

    enable_motion_blur(&mut file);
    file.save_as(&revised_path).unwrap();

    assert_eq!(std::fs::read(&source_path).unwrap(), original);
    assert_ne!(std::fs::read(&revised_path).unwrap(), original);
}
