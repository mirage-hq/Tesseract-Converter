//! Native saved Offset -> edited MotionTile -> linked native AEP structure.
use super::*;

fn collect(value: &Value, effects: &mut Vec<Value>, keys: &mut Vec<Value>) {
    if let Some(object) = value.as_object() {
        if value["type"] == "motionTile" {
            effects.push(value.clone());
        }
        if value["target"]["paramName"] == "tileCenterX" {
            keys.push(value["animator"]["keyframes"].clone());
        }
        for child in object.values() {
            collect(child, effects, keys);
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            collect(child, effects, keys);
        }
    }
}

#[test]
fn native_offset_edited_linked_export_preserves_current_tile_center_and_keys() {
    let parent = tempfile::tempdir().unwrap();
    let source = parent.path().join("offset.prproj");
    let bytes = fs::read(fixture("native_offset_static.xml")).unwrap();
    fs::write(&source, &bytes).unwrap();
    fs::create_dir(parent.path().join("media")).unwrap();
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        parent.path().join("media/source.mp4"),
    )
    .unwrap();
    let imported = parent.path().join("import");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let original = TesseractFile::open(imported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    for edited in [false, true] {
        let mut document = original.clone();
        let record = &mut document["composition"]["layers"][0]["effects"][0];
        assert_eq!(record["effect"]["tileCenterX"], 0.5093749761581421);
        let id = record["id"].clone();
        if edited {
            record["effect"]["tileCenterX"] = json!(0.75);
            record["effect"]["tileCenterY"] = json!(0.25);
            document["composition"]["dynamics"] = json!({"entries":[{
                "target":{"kind":"effectProperty","effectId":id,"paramName":"tileCenterX"},
                "animator":{"type":"keyframes","enabled":true,"keyframes":[
                    {"id":"offset-edit-a","layerTime":0,"value":{"type":"float","value":0.75},"easing":{"type":"linear"}},
                    {"id":"offset-edit-b","layerTime":500,"value":{"type":"float","value":0.2},"easing":{"type":"linear"}}
                ]}
            }]});
        }
        let input = parent.path().join(format!("input-{edited}.tsrct"));
        archive_with_video(&document, &input, "video-30fps-10s.mp4");
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
        let prproj = output.join("project.prproj");
        assert!(xml(&prproj).contains("./media/ae-0001/compositions.aep"));
        assert!(!xml(&prproj).contains("AE.ADBE Offset"));
        let reimport = parent.path().join(format!("reimport-{edited}"));
        import_premiere(&request(&prproj, &reimport, ConversionMode::Write)).unwrap();
        let result = TesseractFile::open(reimport.join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap();
        let (mut effects, mut tracks) = (Vec::new(), Vec::new());
        collect(&result, &mut effects, &mut tracks);
        assert_eq!(effects.len(), 1, "{result}");
        let x = effects[0]["tileCenterX"].as_f64().unwrap();
        assert!((x - if edited { 0.75 } else { 0.5093749761581421 }).abs() < 1e-7);
        assert_eq!(effects[0]["tileCenterY"], if edited { 0.25 } else { 0.5 });
        assert_eq!(effects[0]["tileWidth"], 100.0);
        if edited {
            assert_eq!(tracks.len(), 1, "{result}");
            let keys = tracks[0].as_array().unwrap();
            assert_eq!(keys.len(), 2);
            for (key, time, value) in [(&keys[0], 0, 0.75), (&keys[1], 500, 0.2)] {
                assert_eq!(key["layerTime"], time);
                assert!((key["value"]["value"].as_f64().unwrap() - value).abs() < 1e-7);
            }
        } else {
            assert!(tracks.is_empty());
        }
        assert!(!result.to_string().contains("JsScript"));
    }
    assert_eq!(fs::read(source).unwrap(), bytes);
}
