//! Neutral saved white balance plus explicitly edited FX; structural proof only.
use super::*;

#[test]
fn human_lumetri_white_balance_edited_linked_export_retains_native_offset_keys() {
    let parent = tempfile::tempdir().unwrap();
    let source = parent.path().join("source.prproj");
    let bytes = fs::read(fixture("human_lumetri_contrast.xml")).unwrap();
    fs::write(&source, &bytes).unwrap();
    fs::create_dir(parent.path().join("media")).unwrap();
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        parent.path().join("media/source.mp4"),
    )
    .unwrap();
    let imported = parent.path().join("imported");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let mut document = TesseractFile::open(imported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    let effects = document["composition"]["layers"][0]["effects"]
        .as_array_mut()
        .unwrap();
    effects.retain(|effect| effect["effect"]["type"] == "temperatureTint");
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[0]["effect"]["temperature"], 0.0);
    assert_eq!(effects[1]["effect"]["tint"], 0.0);
    let ids = [effects[0]["id"].clone(), effects[1]["id"].clone()];
    effects[0]["effect"]["temperature"] = json!(40.0);
    effects[1]["effect"]["tint"] = json!(-25.0);
    document["composition"]["dynamics"]["entries"] = json!(ids.into_iter().zip([("temperature",40.0,500),("tint",-25.0,750)]).map(|(id,(name,start,end))| json!({
        "target":{"kind":"effectProperty","effectId":id,"paramName":name},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":format!("{name}-start"),"layerTime":0,"value":{"type":"float","value":start},"easing":{"type":"linear"}},
            {"id":format!("{name}-end"),"layerTime":end,"value":{"type":"float","value":-start},"easing":{"type":"linear"}}
        ]}})).collect::<Vec<_>>());
    let archive = parent.path().join("edited.tsrct");
    archive_with_video(&document, &archive, "video-30fps-10s.mp4");
    let output = parent.path().join("exported");
    let report = export(
        &request(&archive, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|note| note.message.contains("intermediate clipping")),
        "{report:?}"
    );
    assert!(
        !report.diagnostics.iter().any(
            |note| note.message.contains("temperatureTint") && note.message.contains("omitted")
        ),
        "{report:?}"
    );
    let native = aftereffects_file::aep::Project::parse(
        &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
    )
    .unwrap();
    let selectors = numeric_properties(&native.chunks, "ADBE Exposure2-0001");
    assert_eq!(selectors.len(), 4);
    assert!(selectors.iter().all(|property| property.values == [2.0]));
    // Each FX TemperatureTint emits temperature then tint, including its
    // neutral sibling. Verify every RGB projection, not just red offsets.
    for (name, values) in [
        ("ADBE Exposure2-0009", [0.048, 0.0, 0.0, 0.015]),
        ("ADBE Exposure2-0014", [0.0, 0.0, 0.0, -0.03]),
        ("ADBE Exposure2-0019", [-0.048, 0.0, 0.0, 0.015]),
    ] {
        let properties = numeric_properties(&native.chunks, name);
        assert_eq!(properties.len(), 4);
        for (index, (property, expected)) in properties.iter().zip(values).enumerate() {
            if expected == 0.0 {
                assert_eq!(property.values, [0.0]);
                assert!(property.keyframes.is_empty());
                continue;
            }
            let keys = &property.keyframes;
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0].time_secs, 0.0);
            assert!((keys[0].values[0] - expected).abs() < 1e-9);
            assert!((keys[1].values[0] + expected).abs() < 1e-9);
            assert!((keys[1].time_secs - if index == 0 { 0.5 } else { 0.75 }).abs() < 1e-9);
        }
    }
    assert_eq!(fs::read(source).unwrap(), bytes);
}
