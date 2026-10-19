use super::*;

#[test]
fn linked_planar_geometry_precedes_the_opacity_composite() {
    assert_composite_transform([0, 0], [0, 0]);
}

#[test]
fn linked_planar_geometry_preserves_nonzero_crop_and_anchor() {
    assert_composite_transform([120, -80], [7, 9]);
}

fn assert_composite_transform(parent_position: [i32; 2], anchor: [i32; 2]) {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    let mut first = value["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == 10)
        .unwrap()
        .clone();
    first["activeRange"] = json!({"start":0,"duration":1000});
    first["parent"] = json!(30);
    let mut second = first.clone();
    second["id"] = json!(11);
    second["effects"] = json!([]);
    second["transform"]["position"] = json!([40, 50]);
    value["duration"] = json!(1.0);
    value["composition"]["dynamics"]["entries"] = json!([{
        "target":{"kind":"layer","layerId":30,"propertyType":"positionX"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"start","layerTime":0,"easing":{"type":"linear"},"value":{"type":"float","value":10}},
            {"id":"end","layerTime":500,"easing":{"type":"linear"},"value":{"type":"float","value":30}}
        ]}
    }, {
        "target":{"kind":"layer","layerId":30,"propertyType":"positionY"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"y-start","layerTime":0,"easing":{"type":"linear"},"value":{"type":"float","value":20}},
            {"id":"y-end","layerTime":1000,"easing":{"type":"linear"},"value":{"type":"float","value":60}}
        ]}
    }]);
    value["composition"]["layers"] = json!([{
        "type":"Group", "id":30, "name":"Opacity composite",
        "playback":{"type":"windowed", "inputRange":{"start":0,"duration":1000},
            "inputOffsetMs":0, "mapping":{"type":"linear", "input":{"start":0,"duration":1000},
                "output":{"start":0,"duration":1000}}},
        "transform":{"anchorPoint":anchor,"position":[10,20],"scale":[150,75],
            "rotation":0,"opacity":37.5},
        "layers":[first,second,{
            "type":"Text", "id":12, "parent":30, "name":"Unbounded title",
            "activeRange":{"start":0,"duration":1000},
            "transform":{"anchorPoint":[0,0],"position":[100,100],"scale":[100,100],
                "rotation":0,"opacity":100},
            "sourceText":{"text":"Editable title", "fontFamily":"Arial", "fontStyle":"Bold",
                "fontSize":42,"fillColor":[1,1,1,1]}
        }]
    }]);
    let mut group = value["composition"]["layers"][0].clone();
    group["parent"] = json!(40);
    let mut sibling = group["layers"][1].clone();
    sibling["id"] = json!(50);
    sibling["parent"] = json!(40);
    value["composition"]["layers"] = json!([{
        "type":"Group", "id":40, "name":"Component clock",
        "playback":group["playback"],
        "transform":{"anchorPoint":[0,0],"position":parent_position,"scale":[100,100],
            "rotation":0,"opacity":100},
        "layers":[group,sibling]
    }]);
    archive(&value, &input);
    let original = fs::read(&input).unwrap();
    let output = parent.path().join("output");
    let mut conversion = request(&input, &output, ConversionMode::Write);
    conversion.fps = Some("30");
    let report = crate::formats::export_premiere(&conversion).unwrap();
    let path = output.join("media/ae-0001/compositions.aep");
    assert!(path.exists(), "{report:?}");
    let native = aftereffects_file::structure::read_project(&fs::read(path).unwrap()).unwrap();
    let layers: Vec<_> = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            aftereffects_file::structure::ItemKind::Composition(comp) => Some(comp),
            _ => None,
        })
        .flat_map(|comp| &comp.layers)
        .collect();
    let outer = layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Opacity composite")
        .unwrap();
    assert!(!outer.record.flags().null_layer);
    let scale = numeric_properties(&outer.content, "ADBE Scale");
    assert_eq!(
        scale[0].values,
        [1.0, 1.0, 1.0],
        "opacity must not rescale a previously rasterized source"
    );
    let opacity = numeric_properties(&outer.content, "ADBE Opacity");
    assert_eq!(opacity[0].values, [0.375]);
    let inner = layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Opacity composite geometry")
        .unwrap();
    assert!(inner.record.flags().null_layer);
    let scale = numeric_properties(&inner.content, "ADBE Scale");
    assert_eq!(scale[0].values, [1.5, 0.75, 1.0]);
    let position = numeric_properties(&inner.content, "ADBE Position_0");
    let placement = numeric_properties(&outer.content, "ADBE Position");
    let outer_anchor = numeric_properties(&outer.content, "ADBE Anchor Point");
    assert_eq!(outer_anchor[0].values, [0.0, 0.0, 0.0]);
    if parent_position != [0, 0] {
        assert_ne!(placement[0].values[..2], [0.0, 0.0]);
    }
    let inner_anchor = numeric_properties(&inner.content, "ADBE Anchor Point");
    assert_eq!(
        inner_anchor[0].values,
        [
            f64::from(anchor[0]) / 100.0,
            f64::from(anchor[1]) / 100.0,
            0.0
        ]
    );
    assert_eq!(position[0].keyframes.len(), 2);
    assert_eq!(
        position[0].keyframes[0].values[0] + placement[0].values[0],
        10.0
    );
    assert_eq!(
        position[0].keyframes[1].values[0] + placement[0].values[0],
        30.0
    );
    assert!((position[0].keyframes[1].time_secs - 0.5).abs() < 1e-4);
    let control = aftereffects_file::structure::read_project(include_bytes!(
        "../../../../../crates/aftereffects_file/tests/fixtures/parenting/import_parenting_cases.aep"
    )).unwrap();
    let aftereffects_file::structure::ItemKind::Composition(comp) = &control.item(20).unwrap().kind
    else {
        panic!("native parent-scale control");
    };
    let parent = comp
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "parent_scale")
        .unwrap();
    assert!(parent.record.flags().null_layer);
    let native_properties = aftereffects_file::properties::read_transform(&parent.content).unwrap();
    let native_values = |name: &str| {
        &native_properties
            .iter()
            .find(|property| property.match_name == name)
            .unwrap_or_else(|| panic!("Native {name}: {native_properties:?}"))
            .numeric
            .as_ref()
            .unwrap()
            .values
    };
    assert_eq!(*native_values("ADBE Scale"), scale[0].values);
    assert_eq!(
        numeric_properties(&inner.content, "ADBE Opacity")[0].values,
        [1.0]
    );
    assert!(layers
        .iter()
        .any(|layer| layer.name.as_ref() == "Unbounded title"));
    assert_eq!(fs::read(input).unwrap(), original);
}
