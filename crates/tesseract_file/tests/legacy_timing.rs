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

fn photographic_document(float_ranges: bool) -> Value {
    let mut value = document();
    value["duration"] = json!(17);
    let mut group = value["composition"]["layers"][0].take();
    let mut first = group["layers"][0].clone();
    let mut second = first.clone();
    group["activeRange"] = if float_ranges {
        json!({"start":11000.0,"duration":6000.0})
    } else {
        json!({"start":11000,"duration":6000})
    };
    first["id"] = json!(4);
    second["id"] = json!(5);
    first["activeRange"] = if float_ranges {
        json!({"start":0,"duration":4000.0})
    } else {
        json!({"start":0,"duration":4000})
    };
    second["activeRange"] = if float_ranges {
        json!({"start":4000.0,"duration":7000.0})
    } else {
        json!({"start":4000,"duration":7000})
    };
    first["sourceRange"] = first["activeRange"].clone();
    second["sourceRange"] = if float_ranges {
        json!({"start":0,"duration":7000.0})
    } else {
        json!({"start":0,"duration":7000})
    };
    value["composition"]["layers"] = json!([group, first, second]);
    value
}

#[test]
fn legacy_photographic_integral_float_ranges_match_integer_clocks() {
    let mut documents = Vec::new();
    for float_ranges in [false, true] {
        let value = photographic_document(float_ranges);
        let (_directory, path, bytes) = archive(&value);
        let file = TesseractFile::open(path).unwrap();
        let actual = file.project_json().unwrap();
        let layers = actual["composition"]["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0]["layers"].as_array().unwrap().len(), 2);
        for (layer, start, duration) in [
            (&layers[0], 11000, 6000),
            (&layers[1], 0, 4000),
            (&layers[2], 4000, 7000),
        ] {
            let input = json!({"start":start,"duration":duration});
            assert_eq!(layer["playback"]["inputRange"], input);
            assert_eq!(layer["playback"]["mapping"]["input"], input);
            assert_eq!(
                layer["playback"]["mapping"]["output"],
                json!({"start":0,"duration":duration})
            );
            assert_eq!(layer["playback"]["inputOffsetMs"], json!(0));
            assert!(layer.get("activeRange").is_none());
        }
        for index in [1, 2] {
            assert_eq!(
                layers[index]["sourceRange"],
                value["composition"]["layers"][index]["sourceRange"]
            );
        }
        assert_eq!(actual["futureEnvelope"], value["futureEnvelope"]);
        assert_eq!(layers[1]["futureLayer"], json!({"kept":true}));
        assert_eq!(file.project_json_bytes(), bytes);
        documents.push(actual);
    }
    // Retained sourceRange scalar spelling is not part of the canonical clock.
    let integer_layers = documents[0]["composition"]["layers"].as_array().unwrap();
    let float_layers = documents[1]["composition"]["layers"].as_array().unwrap();
    assert_eq!(integer_layers.len(), float_layers.len());
    for (integer_layer, float_layer) in integer_layers.iter().zip(float_layers) {
        assert_eq!(integer_layer["playback"], float_layer["playback"]);
    }
}

#[test]
fn legacy_range_numbers_reject_invalid_and_inexact_boundaries() {
    const MAX_EXACT_MS: u64 = (1_u64 << 53) - 1;
    for field in ["activeRange", "sourceRange"] {
        for scalar in ["start", "duration"] {
            for invalid in [
                json!(-1),
                json!(-1.0),
                json!(0.5),
                json!("1"),
                json!(true),
                Value::Null,
                json!(MAX_EXACT_MS + 1),
                json!((MAX_EXACT_MS + 1) as f64),
                json!(u64::MAX),
            ] {
                let mut value = document();
                value["composition"]["layers"][0]["layers"][0][field][scalar] = invalid.clone();
                let (_directory, path, _) = archive(&value);
                assert!(
                    TesseractFile::open(path).is_err(),
                    "accepted {field}.{scalar}={invalid}"
                );
            }
            let mut value = document();
            value["composition"]["layers"][0]["layers"][0][field]
                .as_object_mut()
                .unwrap()
                .remove(scalar);
            let (_directory, path, _) = archive(&value);
            assert!(TesseractFile::open(path).is_err());
        }
        for range in [
            json!({"start":0,"duration":0}),
            json!({"start":0.0,"duration":0.0}),
            json!({"start":MAX_EXACT_MS,"duration":1}),
            json!({"start":(MAX_EXACT_MS - 1) as f64,"duration":2.0}),
        ] {
            let mut value = document();
            value["composition"]["layers"][0]["layers"][0][field] = range;
            let (_directory, path, _) = archive(&value);
            assert!(TesseractFile::open(path).is_err());
        }
        for range in [
            json!({"start":0.0,"duration":1.0}),
            json!({"start":(MAX_EXACT_MS - 1) as f64,"duration":1.0}),
            json!({"start":0,"duration":MAX_EXACT_MS}),
        ] {
            let mut value = document();
            value["composition"]["layers"][0]["layers"][0][field] = range;
            let (_directory, path, bytes) = archive(&value);
            let file = TesseractFile::open(path).unwrap();
            assert_eq!(file.project_json_bytes(), bytes);
        }
    }
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

#[test]
fn review_legacy_checkout_edit_commit_preserves_bytes_and_clocks() {
    let value = document();
    let (directory, path, _) = archive(&value);
    let checkout = directory.path().join("project.json");
    let mut file = TesseractFile::open(&path).unwrap();
    let before = file.project_json().unwrap();
    file.checkout_project_json(&checkout).unwrap();
    // Even an unchanged checkout must be accepted by the same bounded reader.
    file.commit_project_json(&checkout).unwrap();
    let original = std::fs::read_to_string(&checkout).unwrap();
    let edited = original.replacen("\"Legacy\"", "\"Review edit\"", 1);
    assert_ne!(edited, original);
    let edited = format!("{edited}\n \n");
    std::fs::write(&checkout, edited.as_bytes()).unwrap();
    file.commit_project_json(&checkout).unwrap();
    assert_eq!(file.project_json_bytes(), edited.as_bytes());
    file.save().unwrap();

    let reopened = TesseractFile::open(&path).unwrap();
    assert_eq!(reopened.project_json_bytes(), edited.as_bytes());
    let mut actual = reopened.project_json().unwrap();
    assert_eq!(actual["composition"]["name"], "Review edit");
    actual["composition"]["name"] = before["composition"]["name"].clone();
    assert_eq!(actual, before);
}

#[test]
fn review_legacy_commit_rejects_conflicting_clocks_transactionally() {
    let value = document();
    let (directory, path, _) = archive(&value);
    let checkout = directory.path().join("project.json");
    let mut file = TesseractFile::open(&path).unwrap();
    let before = file.project_json().unwrap();
    let before_bytes = file.project_json_bytes().to_vec();
    let archive_bytes = std::fs::read(&path).unwrap();
    let mut conflicting = value.clone();
    conflicting["composition"]["layers"][0]["layers"][0]["playback"] = json!({"type":"windowed"});
    let mut missing_asset = value.clone();
    missing_asset["composition"]["layers"][0]["layers"][0]["source"]["assetId"] =
        json!("unpackaged-review-video");
    let mut unknown = value;
    unknown["composition"]["layers"][0]["activeRange"]["futureClockField"] = json!(true);

    for invalid in [conflicting, unknown, missing_asset] {
        std::fs::write(&checkout, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(file.commit_project_json(&checkout).is_err());
        assert_eq!(file.project_json().unwrap(), before);
        assert_eq!(file.project_json_bytes(), before_bytes);
        assert_eq!(std::fs::read(&path).unwrap(), archive_bytes);
    }
}
