use super::support::{
    exported_root_sequence, first_project, premiere_to_tesseract, read_xml, tesseract_to_premiere,
};
use serde_json::json;
#[cfg(feature = "ffmpeg-library")]
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use tesseract_file::TesseractFile;

#[test]
fn successive_adjustment_wipes_keep_their_nested_guides() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = read_xml(&fixtures.join("feature_adjustment_motion_wipe_26_5_strict.prproj"));
    let native = roxmltree::Document::parse(&xml).unwrap();
    let item = native
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some("74"))
        .unwrap()
        .range();
    let reference = native
        .descendants()
        .find(|node| node.has_tag_name("TrackItem") && node.attribute("ObjectRef") == Some("74"))
        .unwrap()
        .range();
    let reference = xml[reference].to_owned();
    // Reuse the native static effect and source window; only add a second
    // placement at 8–9.5s. This is structural, not new native render evidence.
    let extra = xml[item.clone()]
        .replace("ObjectID=\"74\"", "ObjectID=\"10000\"")
        .replace("<ID>1000010</ID>", "<ID>1000100</ID>")
        .replace(
            "<Start>1524096000000</Start>",
            "<Start>2032128000000</Start>",
        )
        .replace("<End>1905120000000</End>", "<End>2413152000000</End>");
    xml.insert_str(item.end, &extra);
    xml = xml.replacen(
        &reference,
        &format!("{reference}<TrackItem Index=\"5\" ObjectRef=\"10000\"/>"),
        1,
    );
    let dir = tempfile::tempdir().unwrap();
    fs::copy(
        fixtures.join("a3_base_grid.png"),
        dir.path().join("a3_base_grid.png"),
    )
    .unwrap();
    let source = dir.path().join("two-wipes.prproj");
    crate::test_support::write_prproj(&source, &xml);
    let output = dir.path().join("import");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("b5dcf675-953c-4fa3-b318-3924d9e4f9c7"),
        false,
    )
    .unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.reason == "ClipTrackItem/TrackItem/Node not converted"),
        "{omissions:?}"
    );
    let archive = first_project(&output);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    let outer = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    let inner = outer["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(outer["masks"].as_array().unwrap().len(), 1);
    assert_eq!(inner["masks"].as_array().unwrap().len(), 1);
    assert_eq!(
        document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let omissions = tesseract_to_premiere(&archive, dir.path().join("export"), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[test]
fn native_adjustment_wipe_masks_the_lower_composite_and_exports_editable() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("adjustment");
    let omissions = premiere_to_tesseract(
        fixtures.join("feature_adjustment_motion_wipe_26_5_strict.prproj"),
        &output,
        Some("b5dcf675-953c-4fa3-b318-3924d9e4f9c7"),
        false,
    )
    .unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.reason == "ClipTrackItem/TrackItem/Node not converted"),
        "{omissions:?}"
    );
    let archive = first_project(&output);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let group = layers
        .iter()
        .find(|layer| layer["type"] == "Group" && layer["masks"].is_array())
        .unwrap();
    assert_eq!(
        crate::test_support::layer_range(group),
        &json!({"start":0,"duration":10000})
    );
    let children = group["layers"].as_array().unwrap();
    assert_eq!(
        children
            .iter()
            .filter(|layer| layer["type"] == "Image")
            .count(),
        2
    );
    let adjustment = children
        .iter()
        .find(|layer| layer["type"] == "Adjustment")
        .unwrap();
    assert_eq!(
        crate::test_support::layer_range(adjustment),
        &json!({"start":6000,"duration":1500})
    );
    assert_eq!(adjustment["effects"][0]["effect"]["type"], "levels");
    assert!(adjustment["masks"].as_array().is_none_or(Vec::is_empty));
    let guide_id = &group["masks"][0]["layer"];
    let guide = children
        .iter()
        .find(|layer| &layer["id"] == guide_id)
        .unwrap();
    assert_eq!(guide["transform"]["anchorPoint"], json!([1920.0, 0.0]));
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| &entry["target"]["layerId"] == guide_id)
        .unwrap();
    let keys: Vec<_> = entry["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(keys, [(0, 100.0), (6000, 50.0), (7500, 100.0)]);
    // Change only the editable J5 key: native export must use 65% completion,
    // while retaining full coverage outside the adjustment window.
    let mut edited = document.clone();
    let entry = edited["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| &entry["target"]["layerId"] == guide_id)
        .unwrap();
    let key = entry["animator"]["keyframes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|key| key["layerTime"] == 6000)
        .unwrap();
    key["value"]["value"] = json!(35.0);
    let file = TesseractFile::open(&archive).unwrap();
    let mut builder = tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&edited).unwrap(),
    )
    .unwrap();
    for (id, asset) in &file.metadata().assets {
        builder = builder
            .add_asset(
                id.as_str(),
                fixtures.join(Path::new(&asset.path).file_name().unwrap()),
                asset.kind,
            )
            .unwrap();
    }
    let archive = directory.path().join("edited.tsrct");
    builder.write(&archive).unwrap();
    let exported = directory.path().join("exported");
    let omissions = tesseract_to_premiere(&archive, &exported, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let xml = read_xml(&exported.join("project.prproj"));
    assert!(xml.contains("<MatchName>AE.ADBE Linear Wipe</MatchName>"));
    assert!(xml.contains("<AdjustmentLayer>true</AdjustmentLayer>"));
    let native_graph = roxmltree::Document::parse(&xml).unwrap();
    let completion = native_graph
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoComponentParam")
                && node.children().any(|child| {
                    child.has_tag_name("Name") && child.text() == Some("Transition Completion")
                })
        })
        .unwrap();
    let native_keys = completion
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap();
    assert!(
        native_keys.split(';').any(|key| key
            .split(',')
            .nth(1)
            .and_then(|value| value.parse::<f64>().ok())
            == Some(65.0)),
        "{native_keys}"
    );
    let reimported = directory.path().join("reimported");
    let native = exported.join("project.prproj");
    let root = exported_root_sequence(&native);
    let omissions = premiere_to_tesseract(&native, &reimported, Some(&root), false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let again = TesseractFile::open(first_project(&reimported))
        .unwrap()
        .project_json()
        .unwrap();
    let entries = again["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let again_keys: Vec<_> = entries[0]["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(again_keys, [(0, 100.0), (6000, 35.0), (7500, 100.0)]);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_static_linear_wipe_keeps_completion_angle_and_feather_through_export() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let donor = fixtures.join("feature_adjustment_motion_wipe_26_5_strict.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&donor).unwrap())),
        "a215535f7e21efac7fca84edc572f7b1d2814b5858f90c49a97044b1bc600a2e"
    );
    // Transplant J5's unchanged saved parameter values onto the native two-video
    // wipe fixture. Adjustment-host semantics are a separate regression.
    let donor_xml = read_xml(&donor);
    let donor_graph = roxmltree::Document::parse(&donor_xml).unwrap();
    let mut xml = read_xml(&fixtures.join("feature_linear_wipe_strict.prproj"));
    for (donor_id, target_id) in [("178", "155"), ("179", "156"), ("180", "157")] {
        let param = donor_graph
            .descendants()
            .find(|node| node.attribute("ObjectID") == Some(donor_id))
            .unwrap();
        let graph = roxmltree::Document::parse(&xml).unwrap();
        let target = graph
            .descendants()
            .find(|node| node.attribute("ObjectID") == Some(target_id))
            .unwrap();
        let range = target.range();
        let replacement = donor_xml[param.range()].replace(
            &format!("ObjectID=\"{donor_id}\""),
            &format!("ObjectID=\"{target_id}\""),
        );
        xml.replace_range(range, &replacement);
    }
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "feature_two_tracks_gap_clip_a.mp4",
        "feature_two_tracks_gap_clip_b.mp4",
    ] {
        fs::copy(fixtures.join(name), dir.path().join(name)).unwrap();
    }
    let source = dir.path().join("static-wipe.prproj");
    crate::test_support::write_prproj(&source, &xml);
    let output = dir.path().join("import");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("49b892d1-dfc3-4be2-a83a-93789626be7c"),
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let archive = first_project(&output);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let picture = layers
        .iter()
        .find(|layer| layer["type"] == "Video" && layer["masks"].is_array())
        .unwrap();
    let mask = &picture["masks"][0];
    let guide = layers
        .iter()
        .find(|layer| layer["id"] == mask["layer"])
        .unwrap();
    assert_eq!(guide["transform"]["anchorPoint"], json!([1920.0, 0.0]));
    assert_eq!(guide["transform"]["scale"], json!([50.0, 100.0]));
    assert_eq!(mask["feather"], json!([0.0, 0.0]));
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["layerId"] == guide["id"])
        .unwrap();
    assert_eq!(entry["target"]["propertyType"], "scaleX");
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[0]["value"], json!({"type": "float", "value": 50.0}));

    let exported = dir.path().join("export");
    let omissions = tesseract_to_premiere(&archive, &exported, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let written = read_xml(&exported.join("project.prproj"));
    let graph = roxmltree::Document::parse(&written).unwrap();
    for (name, value) in [
        ("Transition Completion", "50"),
        ("Wipe Angle", "90"),
        ("Feather", "0"),
    ] {
        let param = graph
            .descendants()
            .find(|node| {
                node.has_tag_name("VideoComponentParam")
                    && node
                        .children()
                        .any(|child| child.has_tag_name("Name") && child.text() == Some(name))
            })
            .unwrap();
        assert!(param
            .children()
            .all(|child| !child.has_tag_name("Keyframes")));
        let start = param
            .children()
            .find(|child| child.has_tag_name("StartKeyframe"))
            .unwrap();
        assert_eq!(start.text().unwrap().split(',').nth(1), Some(value));
    }
    let reimport = dir.path().join("reimport");
    let omissions =
        premiere_to_tesseract(exported.join("project.prproj"), &reimport, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    // Editing only the key must override the old static guide scale on export.
    let mut edited = document.clone();
    let entry = edited["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["layerId"] == guide["id"])
        .unwrap();
    entry["animator"]["keyframes"][0]["value"]["value"] = json!(35.0);
    let file = TesseractFile::open(&archive).unwrap();
    let mut builder = tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&edited).unwrap(),
    )
    .unwrap();
    for (id, asset) in &file.metadata().assets {
        let source = fixtures.join(Path::new(&asset.path).file_name().unwrap());
        builder = builder.add_asset(id.as_str(), source, asset.kind).unwrap();
    }
    let edited_archive = dir.path().join("edited.tsrct");
    builder.write(&edited_archive).unwrap();
    let edited_output = dir.path().join("edited-export");
    let omissions = tesseract_to_premiere(&edited_archive, &edited_output, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let written = read_xml(&edited_output.join("project.prproj"));
    let graph = roxmltree::Document::parse(&written).unwrap();
    let completion = graph
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoComponentParam")
                && node.children().any(|child| {
                    child.has_tag_name("Name") && child.text() == Some("Transition Completion")
                })
        })
        .unwrap();
    assert!(completion
        .children()
        .all(|child| !child.has_tag_name("Keyframes")));
    let start = completion
        .children()
        .find(|child| child.has_tag_name("StartKeyframe"))
        .unwrap();
    assert_eq!(start.text().unwrap().split(',').nth(1), Some("65"));
}
