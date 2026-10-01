use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use tesseract_file::TesseractFile;

fn archive(document: &Value) -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.tsrct");
    let bytes = serde_json::to_vec_pretty(document).unwrap();
    let metadata = json!({
        "$schema": "urn:tesseract:file:metadata:v2", "format": "tesseract", "formatVersion": 2,
        "documentId": "00000000-0000-0000-0000-000000000001",
        "createdAt": "2026-01-01T00:00:00Z", "modifiedAt": "2026-01-01T00:00:00Z",
        "generator": {"name": "tesseract", "version": "0.3.0", "engineVersion": "0.3.0"},
        "project": {"path": "project.json", "contentType": "application/vnd.tesseract.fx-composition+json",
            "byteLength": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes))},
        "assets": {}
    });
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("metadata.json", options).unwrap();
    zip.write_all(&serde_json::to_vec(&metadata).unwrap())
        .unwrap();
    zip.start_file("project.json", options).unwrap();
    zip.write_all(&bytes).unwrap();
    zip.finish().unwrap();
    (directory, path, bytes)
}

fn document() -> Value {
    json!({
        "$schema":"https://jerboa.dev/schemas/fx-composition/editable/v1/document.schema.json",
        "formatVersion":1,"dimensions":{"width":1080,"height":1080},"duration":5,
        "futureEnvelope":{"preserved":true},
        "composition":{"id":"main","name":"Legacy", "layers":[{
            "type":"Group","id":1,"name":"Local group","activeRange":{"start":1000,"duration":2000},
            "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},"layers":[{
                "type":"Video","id":2,"name":"Trimmed video","activeRange":{"start":0,"duration":1000},
                "sourceRange":{"start":200,"duration":2000},"sourceIntrinsicDuration":5000,
                "source":{"assetId":"","fit":"contain"},"transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},"futureLayer":{"kept":true}
            },{
                "type":"Audio","id":3,"name":"Normal speed audio","activeRange":{"start":0,"duration":1000},
                "sourceRange":{"start":200,"duration":2000},"sourceIntrinsicDuration":5000,
                "source":{"assetId":""},"volume":1
            }]
        }]}
    })
}

#[test]
fn legacy_archive_preserves_video_audio_group_clocks_and_original_bytes() {
    let value = document();
    let (_directory, path, bytes) = archive(&value);
    let file = TesseractFile::open(&path).unwrap();
    let actual = file.project_json().unwrap();
    let group = &actual["composition"]["layers"][0];
    assert_eq!(
        group["playback"]["mapping"]["output"],
        json!({"start":0,"duration":2000})
    );
    let video = &group["layers"][0];
    assert_eq!(
        video["playback"]["mapping"]["output"],
        json!({"start":200,"duration":2000})
    );
    let audio = &group["layers"][1];
    assert_eq!(
        audio["playback"]["mapping"]["output"],
        json!({"start":200,"duration":1000})
    );
    assert_eq!(audio["sourceRange"], json!({"start":200,"duration":2000}));
    assert!(group.get("activeRange").is_none());
    assert!(video.get("activeRange").is_none());
    assert_eq!(video["futureLayer"], json!({"kept":true}));
    assert_eq!(actual["futureEnvelope"], value["futureEnvelope"]);
    assert_eq!(file.project_json_bytes(), bytes);
}

#[test]
fn legacy_clocks_inside_ai_edit_are_migrated_without_changing_the_container() {
    let mut value = document();
    let mut group = value["composition"]["layers"][0].take();
    group["parent"] = json!(10);
    for child in group["layers"].as_array_mut().unwrap() {
        child["parent"] = json!(1);
    }
    value["composition"]["layers"][0] = json!({
        "type": "AiEdit", "id": 10, "name": "Semantic shot",
        "activeRange": {"start": 0, "duration": 5000},
        "styleId": "test", "sourceLayerId": 2, "layers": [group]
    });
    let (_directory, path, bytes) = archive(&value);
    let file = TesseractFile::open(path).unwrap();
    let actual = file.project_json().unwrap();
    let ai_edit = &actual["composition"]["layers"][0];
    assert_eq!(
        ai_edit["activeRange"],
        json!({"start": 0, "duration": 5000})
    );
    assert_eq!(ai_edit["styleId"], json!("test"));
    let group = &ai_edit["layers"][0];
    assert_eq!(
        group["playback"]["mapping"]["output"],
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        group["layers"][0]["playback"]["mapping"]["output"],
        json!({"start": 200, "duration": 2000})
    );
    assert_eq!(
        group["layers"][1]["playback"]["mapping"]["output"],
        json!({"start": 200, "duration": 1000})
    );
    assert_eq!(file.project_json_bytes(), bytes);
}

#[test]
fn legacy_active_range_with_unknown_clock_fields_is_rejected() {
    let mut value = document();
    value["composition"]["layers"][0]["activeRange"]["futureClockField"] = json!(true);
    let (_directory, path, _) = archive(&value);
    let error = TesseractFile::open(path).unwrap_err().to_string();
    assert!(
        error.contains("activeRange contains unsupported clock fields"),
        "{error}"
    );
}

#[test]
fn legacy_remap_uses_its_inactive_key_domain_without_changing_keys() {
    let mut value = document();
    let remap = json!({"before":"inactive","after":"inactive","keyframes":[
        {"id":"in","time":250,"value":300,"easing":{"type":"linear"}},
        {"id":"out","time":750,"value":900,"easing":{"type":"linear"}}
    ]});
    value["composition"]["layers"][0]["layers"][0]["playback"] = remap.clone();
    let (_directory, path, _) = archive(&value);
    let file = TesseractFile::open(path).unwrap();
    let actual = file.project_json().unwrap();
    let playback = &actual["composition"]["layers"][0]["layers"][0]["playback"];
    assert_eq!(playback["inputRange"], json!({"start":250,"duration":500}));
    assert_eq!(playback["mapping"]["property"], remap);
}

#[test]
fn legacy_reader_does_not_hide_conflicting_or_invalid_clocks() {
    for playback in [
        json!({"type":"windowed"}),
        json!({"keyframes":[]}),
        json!({"rate":2}),
    ] {
        let mut value = document();
        value["composition"]["layers"][0]["layers"][0]["playback"] = playback;
        let (_directory, path, _) = archive(&value);
        assert!(TesseractFile::open(path).is_err());
    }
}
