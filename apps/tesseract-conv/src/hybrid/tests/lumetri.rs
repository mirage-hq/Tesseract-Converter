//! Saved native controls in a synthetic host; structural linked export, not
//! independent Adobe acceptance, original placement or colour fidelity.
use super::*;

fn graded_layers(value: &Value) -> Vec<&Value> {
    let mut found = Vec::new();
    if let Some(layers) = value["layers"].as_array() {
        for layer in layers {
            if layer["effects"].as_array().is_some_and(|effects| {
                effects
                    .iter()
                    .any(|effect| effect["effect"]["type"] == "exposure")
            }) {
                found.push(layer);
            }
            found.extend(graded_layers(layer));
        }
    }
    found
}

#[test]
fn human_lumetri_import_and_edited_linked_export_keep_controls_and_saturation_keys() {
    let parent = tempfile::tempdir().unwrap();
    let source = parent.path().join("source.prproj");
    let bytes = fs::read(fixture("human_lumetri_contrast.xml")).unwrap();
    fs::write(&source, &bytes).unwrap();
    fs::create_dir(parent.path().join("media")).unwrap();
    // The host declares 1920x1080/30fps/10s; supply matching synthetic footage,
    // not a Lumetri oracle or a reauthored native component.
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        parent.path().join("media/source.mp4"),
    )
    .unwrap();
    let imported = parent.path().join("imported");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let archive = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let original = archive.project_json().unwrap();
    for edited in [false, true] {
        let mut document = original.clone();
        let effects = document["composition"]["layers"][0]["effects"]
            .as_array_mut()
            .unwrap();
        assert_eq!(effects.len(), 6);
        assert_eq!(effects[0]["effect"]["exposure"], 1.0);
        assert_eq!(effects[1]["effect"]["contrast"], 25.0);
        assert_eq!(effects[2]["effect"]["saturation"], 30.0);
        if edited {
            effects[0]["effect"]["exposure"] = json!(2.0);
            effects[1]["effect"]["contrast"] = json!(40.0);
            effects[2]["effect"]["saturation"] = json!(45.0);
        }
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["target"]["paramName"], "saturation");
        let keys = entries[0]["animator"]["keyframes"].as_array_mut().unwrap();
        assert_eq!(keys[0]["value"]["value"], 30.0);
        assert_eq!(keys[1]["value"]["value"], -40.0);
        if edited {
            keys[0]["value"]["value"] = json!(45.0);
            keys[0]["layerTime"] = json!(500);
            keys[1]["value"]["value"] = json!(-65.0);
            keys[1]["layerTime"] = json!(1750);
        }

        let input = parent.path().join(format!("edited-{edited}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset(
                "premiere-video-1",
                fixture("video-30fps-10s.mp4"),
                AssetKind::Video,
            )
            .unwrap()
            .write(&input)
            .unwrap();
        let output = parent.path().join(format!("export-{edited}"));
        let report = export(
            &request(&input, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "HYBRID-EXPERIMENTAL"),
            "{report:?}"
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Animated saturation-only")),
            "{report:?}"
        );
        assert!(
            !report.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains("hueSaturation / saturation")
                && diagnostic.message.contains("animation omitted")),
            "{report:?}"
        );
        let project = output.join("project.prproj");
        assert!(xml(&project).contains("./media/ae-0001/compositions.aep"));
        assert!(!xml(&project).contains("AE.ADBE Lumetri"));
        let reimported = parent.path().join(format!("reimport-{edited}"));
        import_premiere(&request(&project, &reimported, ConversionMode::Write)).unwrap();
        let result = TesseractFile::open(reimported.join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap();
        let selected = graded_layers(&result["composition"]);
        assert_eq!(selected.len(), 1, "{result}");
        let effects = &selected[0]["effects"];
        assert_eq!(
            effects[0]["effect"]["exposure"],
            if edited { 2.0 } else { 1.0 }
        );
        assert_eq!(
            effects[1]["effect"]["contrast"],
            if edited { 40.0 } else { 25.0 }
        );
        assert_eq!(
            effects[2]["effect"]["saturation"],
            if edited { 45.0 } else { 30.0 }
        );
        assert_eq!(effects[2]["effect"]["type"], "vibrance");
        assert_eq!(effects[2]["effect"]["vibrance"], 0.0);
        fn tracks(value: &Value, result: &mut Vec<Value>) {
            if let Some(object) = value.as_object() {
                if object
                    .get("target")
                    .is_some_and(|target| target["paramName"] == "saturation")
                {
                    result.push(value["animator"]["keyframes"].clone());
                }
                for value in object.values() {
                    tracks(value, result);
                }
            } else if let Some(array) = value.as_array() {
                for value in array {
                    tracks(value, result);
                }
            }
        }
        let mut actual = Vec::new();
        tracks(&result, &mut actual);
        assert_eq!(actual.len(), 1, "{result}");
        let keys = actual[0].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        for (key, time, value) in [
            (
                &keys[0],
                if edited { 500 } else { 0 },
                if edited { 45.0 } else { 30.0 },
            ),
            (
                &keys[1],
                if edited { 1750 } else { 2513 },
                if edited { -65.0 } else { -40.0 },
            ),
        ] {
            assert_eq!(key["layerTime"], time);
            assert_eq!(key["value"]["value"], value);
        }
        assert!(!result.to_string().contains("JsScript"));
    }
    assert_eq!(fs::read(source).unwrap(), bytes);
}
