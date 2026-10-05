//! Premiere-saved linked Set Matte, not a synthetic Premiere wrapper.
use super::*;
use sha2::{Digest, Sha256};

fn projections<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    if value["effects"].as_array().is_some_and(|effects| {
        effects
            .iter()
            .any(|e| e["effect"]["type"] == "shiftChannels")
    }) {
        out.push(value);
    }
    if let Some(children) = value["layers"].as_array() {
        for child in children {
            projections(child, out);
        }
    }
}

#[test]
fn premiere_saved_channel_matte_import_retains_green_projection() {
    let (_source_dir, source) = pinned_source();
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("import");
    import_premiere(&request(&source, &output, ConversionMode::Write)).unwrap();
    let document = TesseractFile::open(output.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    let mut found = Vec::new();
    projections(&document["composition"]["layers"][0], &mut found);
    assert_eq!(
        found.len(),
        1,
        "saved Green Set Matte must become editable channel projection"
    );
    let shift = &found[0]["effects"][0]["effect"];
    assert_eq!(shift["takeRedFrom"], "fullOff");
    assert_eq!(shift["takeGreenFrom"], "green");
    assert_eq!(shift["takeBlueFrom"], "fullOff");
}

fn current_edit(value: &mut Value, bypass: bool) {
    if value.get("name").is_some() {
        value["name"] = json!(format!("Current layer {}", value["id"]));
    }
    if let Some(effects) = value.get_mut("effects").and_then(Value::as_array_mut) {
        for effect in effects {
            if effect["effect"]["type"] == "shiftChannels" {
                effect["effect"]["takeGreenFrom"] = json!("fullOff");
                effect["effect"]["takeBlueFrom"] = json!("blue");
            }
        }
    }
    if let Some(children) = value.get_mut("layers").and_then(Value::as_array_mut) {
        if bypass {
            let ids: Vec<_> = children
                .iter()
                .filter(|child| {
                    child["effects"].as_array().is_some_and(|effects| {
                        effects
                            .iter()
                            .any(|e| e["effect"]["type"] == "shiftChannels")
                    })
                })
                .map(|child| child["id"].clone())
                .collect();
            for child in children.iter_mut() {
                if ids.contains(&child["trackMatte"]["layer"]) {
                    child.as_object_mut().unwrap().remove("trackMatte");
                }
            }
            children.retain(|child| !ids.contains(&child["id"]));
        }
        for child in children {
            current_edit(child, bypass);
        }
    }
}

#[test]
fn premiere_saved_channel_matte_trimmed_video_current_edit_and_bypass_export() {
    let (_source_dir, source) = pinned_source();
    let temp = tempfile::tempdir().unwrap();
    let imported = temp.path().join("import");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let input = imported.join("project.tsrct");
    let original = TesseractFile::open(&input).unwrap().project_json().unwrap();
    assert_trimmed_media(&original);
    for bypass in [false, true] {
        let mut current = original.clone();
        current_edit(&mut current["composition"], bypass);
        assert_trimmed_media(&current);
        let json_path = temp.path().join(format!("edit-{bypass}.json"));
        fs::write(&json_path, serde_json::to_vec(&current).unwrap()).unwrap();
        let mut archive = TesseractFile::open(&input).unwrap();
        archive.commit_project_json(&json_path).unwrap();
        let edited = temp.path().join(format!("edit-{bypass}.tsrct"));
        archive.save_as(&edited).unwrap();
        assert_eq!(
            archive
                .asset("premiere-aep-1-item-1")
                .unwrap()
                .read_verified_bytes(10_000_000)
                .unwrap(),
            fs::read(fixture("premiere_channel_matte/video.mp4")).unwrap()
        );
        let output = temp.path().join(format!("export-{bypass}"));
        let report = export(
            &request(&edited, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        assert!(output.join("project.prproj").is_file(), "{report:?}");
        let native_path = output.join("media/ae-0001/compositions.aep");
        let native =
            aftereffects_file::structure::read_project(&fs::read(&native_path).unwrap()).unwrap();
        let mut controls = 0;
        let mut luma = 0;
        for item in &native.items {
            let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind else {
                continue;
            };
            for layer in &comp.layers {
                if layer.record.track_matte_type() == 3 {
                    luma += 1;
                    let provider = comp
                        .layers
                        .iter()
                        .find(|provider| {
                            Some(provider.record.id()) == layer.record.matte_layer_id()
                        })
                        .expect("same-scope Luma reference");
                    assert!(
                        !provider.record.flags().enabled,
                        "consumed provider cannot paint independently"
                    );
                }
                let red = native_values(&layer.content, "ADBE Shift Channels-0002");
                let green = native_values(&layer.content, "ADBE Shift Channels-0003");
                let blue = native_values(&layer.content, "ADBE Shift Channels-0004");
                controls += red.len();
                assert_eq!(red, vec![10.0; blue.len()]);
                assert_eq!(green, vec![10.0; blue.len()]);
                assert_eq!(blue, vec![4.0; blue.len()]);
                assert!(
                    native_values(&layer.content, "ADBE Set Matte3-0002").is_empty(),
                    "no native channel replay"
                );
            }
        }
        if bypass {
            assert_eq!(controls, 0);
            assert_eq!(luma, 0);
        } else {
            assert!(controls > 0);
            assert!(luma > 0);
        }
        let reopened = temp.path().join(format!("reimport-{bypass}"));
        import_premiere(&request(
            &output.join("project.prproj"),
            &reopened,
            ConversionMode::Write,
        ))
        .unwrap();
        let reopened_archive = TesseractFile::open(reopened.join("project.tsrct")).unwrap();
        let document = reopened_archive.project_json().unwrap();
        assert_source_bytes(&document["composition"], &reopened_archive);
        assert_trimmed_media(&document);
        let mut found = Vec::new();
        projections(&document["composition"], &mut found);
        if bypass {
            assert!(found.is_empty());
        } else {
            assert!(!found.is_empty());
            for projection in found {
                assert_eq!(projection["effects"][0]["effect"]["takeBlueFrom"], "blue");
            }
        }
    }
}

fn native_values(chunks: &[aftereffects_file::rifx::Chunk], name: &str) -> Vec<f64> {
    let mut values = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        if chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|bytes| bytes.starts_with(format!("{name}\0").as_bytes()))
        {
            if let Some(storage) = chunks[index + 1..]
                .iter()
                .take_while(|c| c.id() != *b"tdmn")
                .find(|c| c.list_kind() == Some(*b"tdbs"))
                .and_then(aftereffects_file::rifx::Chunk::children)
            {
                values.extend(
                    aftereffects_file::properties::read_numeric(storage)
                        .unwrap()
                        .values,
                );
            }
        }
        if let Some(children) = chunk.children() {
            values.extend(native_values(children, name));
        }
    }
    values
}

// This fixture only has static affine/two-key Linear clocks. Evaluate that
// bounded authored structure to detect a trim applied twice on fresh reimport.
fn media_times(value: &Value, time: f64, kind: &str, out: &mut Vec<f64>) {
    if value["isHidden"] == true {
        return;
    }
    let mut time = time;
    if let Some(playback) = value.get("playback") {
        let start = playback["inputRange"]["start"].as_f64().unwrap();
        let end = start + playback["inputRange"]["duration"].as_f64().unwrap();
        if time < start || time >= end {
            return;
        }
        time += playback["inputOffsetMs"].as_f64().unwrap();
        let mapping = &playback["mapping"];
        if mapping["type"] == "linear" {
            time = mapping["output"]["start"].as_f64().unwrap()
                + (time - mapping["input"]["start"].as_f64().unwrap())
                    * mapping["output"]["duration"].as_f64().unwrap()
                    / mapping["input"]["duration"].as_f64().unwrap();
        } else {
            let keys = mapping["property"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2);
            assert!(keys.iter().all(|k| k["easing"]["type"] == "linear"));
            let t0 = keys[0]["time"].as_f64().unwrap();
            let t1 = keys[1]["time"].as_f64().unwrap();
            let v0 = keys[0]["value"].as_f64().unwrap();
            let v1 = keys[1]["value"].as_f64().unwrap();
            time = v0 + (time - t0) / (t1 - t0) * (v1 - v0);
        }
    }
    if value["type"] == kind {
        out.push(time);
    }
    if let Some(children) = value["layers"].as_array() {
        for child in children {
            media_times(child, time, kind, out);
        }
    }
}

fn assert_trimmed_media(value: &Value) {
    for kind in ["Video", "Audio"] {
        for (time, expected) in [(250.0, 750.0), (750.0, 1250.0)] {
            let mut times = Vec::new();
            media_times(&value["composition"], time, kind, &mut times);
            assert_eq!(times.len(), 1, "exactly one live {kind}: {times:?}");
            assert!(
                (times[0] - expected).abs() < 1.0,
                "{kind} clock at{time}: {times:?}, expected{expected}"
            );
        }
    }
}

fn pinned_source() -> (tempfile::TempDir, PathBuf) {
    let source = fixture("premiere_channel_matte/native.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        "57602c5967104c8e15047092e3e6610d1462f5459ce5833e6643e80747c486ec"
    );
    relinked_native_fixture(&source)
}

fn assert_source_bytes(value: &Value, archive: &TesseractFile) {
    if value["type"] == "Video" {
        let id = value["source"]["assetId"].as_str().unwrap();
        assert_eq!(
            archive
                .asset(id)
                .unwrap()
                .read_verified_bytes(10_000_000)
                .unwrap(),
            fs::read(fixture("premiere_channel_matte/video.mp4")).unwrap()
        );
    }
    if let Some(children) = value["layers"].as_array() {
        for child in children {
            assert_source_bytes(child, archive);
        }
    }
}
