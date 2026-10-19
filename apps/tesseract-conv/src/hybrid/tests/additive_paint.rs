use super::*;

#[test]
fn linked_additive_shape_keeps_paint_layer_opacity_product() {
    let native = aftereffects_file::structure::read_project(include_bytes!(
        "../../../../../crates/aftereffects_file/tests/fixtures/render/export_add_blend.aep"
    ))
    .unwrap();
    let aftereffects_file::structure::ItemKind::Composition(source) = &native.item(1).unwrap().kind
    else {
        panic!("native composition");
    };
    assert!(matches!(source.layers[0].record.blend_mode(), 4 | 29));
    let mut value =
        aftereffects_file::structure_document::to_structural_fx_document(&native, Some(1))
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
    value["composition"]["layers"] = json!([{
        "type":"Shape", "id":400, "name":"Additive radial flash",
        "activeRange":{"start":0,"duration":1000}, "blendMode":"add",
        "transform":{"position":[0,0],"anchorPoint":[0,0],"scale":[100,100],
            "rotation":0,"opacity":0.35},
        "shape":{
            "path":{"commands":[
                {"type":"moveTo","x":0,"y":0},
                {"type":"lineTo","x":640,"y":0},
                {"type":"lineTo","x":640,"y":360},
                {"type":"lineTo","x":0,"y":360},{"type":"close"}
            ]},
            "fills":[{"opacity":100.0,"paint":{
                "type":"gradient","gradientType":"radial",
                "start":[320,180],"end":[640,180],"stops":[
                    {"offset":0,"color":[1,1,1,1]},
                    {"offset":1,"color":[0.12,0.1,0.18,1]}
                ]
            }}]
        }
    }]);
    value["composition"]["dynamics"] = json!({"entries":[{
        "target":{"kind":"layer","layerId":400,"propertyType":"opacity"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"start","layerTime":0,"value":{"type":"float","value":0.35},
                "easing":{"type":"linear"}},
            {"id":"end","layerTime":250,"value":{"type":"float","value":0.0},
                "easing":{"type":"linear"}}
        ]}
    }]});
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .write(&input)
        .unwrap();
    let output = temp.path().join("output");
    export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert!(xml(&output.join("project.prproj")).contains("./media/ae-0001/compositions.aep"));
    let linked = aftereffects_file::structure::read_project(
        &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
    )
    .unwrap();
    let aftereffects_file::structure::ItemKind::Composition(comp) = &linked.item(1).unwrap().kind
    else {
        panic!("linked composition");
    };
    assert_eq!(comp.layers.len(), 1);
    assert!(matches!(comp.layers[0].record.blend_mode(), 4 | 29));
    let contents = &comp.layers[0].content;
    assert_eq!(
        numeric_properties(contents, "ADBE Vector Fill Opacity")[0].values,
        [100.0]
    );
    let opacity = &numeric_properties(contents, "ADBE Opacity")[0];
    assert_eq!(opacity.keyframes.len(), 2);
    for (key, (time, expected)) in opacity.keyframes.iter().zip([(0.0, 0.35), (0.25, 0.0)]) {
        assert_eq!(key.time_secs, time);
        assert!((key.values[0] - expected).abs() < 1e-6, "{opacity:?}");
    }
    assert_eq!(
        TesseractFile::open(&input).unwrap().project_json().unwrap(),
        value
    );
}
