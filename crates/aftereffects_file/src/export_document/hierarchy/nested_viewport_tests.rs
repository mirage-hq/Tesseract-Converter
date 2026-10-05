use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::to_structural_fx_document;
use serde_json::{Value, json};

fn nested_value() -> Value {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/properties/transform_unseparated.aep"
    ))
    .unwrap();
    let mut value = to_structural_fx_document(&native, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    let mut child =
        value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    child["id"] = json!(90003);
    child["parent"] = json!(90002);
    child["name"] = json!("Retained oversized geometry");
    child["activeRange"] = json!({"start":0,"duration":1500});
    child["transform"]["rotationY"] = json!(0.0);
    child["transform"]["scale"] = json!([1_000_000.0, 100.0]);
    // A retained child Layer Style prevents collapsed-vector substitution, as
    // in P047. It is applied before the identity group's output is consumed.
    child["effects"] = json!([{"type":"outerGlow","enabled":true,
        "color":[0.3,0.8,0.2,1.0],"size":7.0,"spread":0.1,"range":0.6,
        "blendMode":"screen"}]);
    let mut inner = value["composition"]["layers"][0].clone();
    inner["id"] = json!(90002);
    inner["parent"] = Value::Null;
    inner["name"] = json!("Nested identity output");
    inner["transform"] = serde_json::to_value(super::super::identity_fx_transform()).unwrap();
    inner["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":0,"duration":1500}),
        json!({"start":0,"duration":1500}),
    );
    inner["effects"] = json!([]);
    inner["masks"] = json!([]);
    inner["trackMatte"] = Value::Null;
    inner["motionBlur"] = json!(false);
    inner["layers"] = json!([child]);
    let mut outer = inner.clone();
    outer["id"] = json!(90000);
    outer["name"] = json!("Final identity output");
    outer["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":0,"duration":2000}),
        json!({"start":0,"duration":2000}),
    );
    outer["layers"] = json!([inner]);
    value["duration"] = json!(2.0);
    value["dimensions"] = json!({"width":319,"height":179});
    value["composition"]["layers"] = json!([outer]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    value
}

fn eligible(value: Value, unknown: bool, shifted: bool) -> bool {
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let LayerData::Group(outer) = document.composition().layers()[0].data() else {
        panic!("outer group");
    };
    let LayerData::Group(group) = outer.layers[0].data() else {
        panic!("nested group");
    };
    let dynamics = super::super::AnimationIndex::new(document.composition().dynamics().entries());
    let mut inherited = root_demand(document.dimensions(), 2000);
    if unknown {
        inherited.full("unproved ancestor effect");
    }
    if shifted {
        inherited = Demand::root(
            Bounds {
                min: [-1.0, 0.0],
                max: [319.0, 179.0],
            },
            2000,
        );
    }
    let mut demand = child_demand(
        group,
        group,
        &[],
        &dynamics,
        document.dimensions(),
        &inherited,
        false,
    );
    demand.use_nested_output_viewport(group, &dynamics, &outer.layers, document.dimensions())
}

#[test]
fn nested_identity_viewport_rejects_unproved_or_different_consumer_domains() {
    assert!(eligible(nested_value(), false, false));
    assert!(!eligible(nested_value(), true, false));
    assert!(!eligible(nested_value(), false, true));
    for (field, replacement) in [
        ("motionBlur", json!(true)),
        (
            "effects",
            json!([{"type":"gaussianBlur","blurriness":10.0}]),
        ),
        (
            "masks",
            json!([{"id":90004,"mode":"add","inverted":false,
            "feather":[0.0,0.0],"opacity":100.0,"expansion":0.0,
            "path":{"commands":[{"type":"moveTo","x":0.0,"y":0.0},
                {"type":"lineTo","x":10.0,"y":0.0},{"type":"lineTo","x":0.0,"y":10.0},
                {"type":"close"}]}}]),
        ),
    ] {
        let mut value = nested_value();
        value["composition"]["layers"][0]["layers"][0][field] = replacement;
        assert!(!eligible(value, false, false), "{field}");
    }
    let mut transformed = nested_value();
    transformed["composition"]["layers"][0]["layers"][0]["transform"]["position"] =
        json!([1.0, 0.0]);
    assert!(!eligible(transformed, false, false));
    let mut spatial = nested_value();
    spatial["composition"]["layers"][0]["layers"][0]["layers"][0]["transform"]["rotationY"] =
        json!(0.1);
    assert!(!eligible(spatial, false, false));
}

#[test]
fn nested_identity_viewport_retains_opacity_keys_without_treating_them_as_spatial_demand() {
    let mut value = nested_value();
    value["composition"]["dynamics"]["entries"] = json!([{
        "target":{"kind":"layer","layerId":90002,"propertyType":"opacity"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"opaque","layerTime":0,"value":{"type":"float","value":100.0},"easing":{"type":"linear"}},
            {"id":"faded","layerTime":1000,"value":{"type":"float","value":25.0},"easing":{"type":"linear"}}
        ]}
    }]);
    assert!(eligible(value.clone(), false, false));
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(value.clone()).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let occurrence = native
        .items
        .iter()
        .find_map(|item| {
            let ItemKind::Composition(composition) = &item.kind else {
                return None;
            };
            composition
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == "Nested identity output")
        })
        .expect("animated identity occurrence retained");
    let properties = crate::properties::read_transform(&occurrence.content).unwrap();
    let opacity = properties
        .iter()
        .find(|p| p.match_name == "ADBE Opacity")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(
        opacity
            .keyframes
            .iter()
            .map(|k| k.time_secs)
            .collect::<Vec<_>>(),
        [0.0, 1.0]
    );
    assert_eq!(
        opacity
            .keyframes
            .iter()
            .map(|k| k.values[0])
            .collect::<Vec<_>>(),
        [1.0, 0.25]
    );
    value["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] = json!("positionX");
    assert!(
        !eligible(value, false, false),
        "spatial keys remain unsupported"
    );
}

fn oversized_input_value() -> Value {
    let mut value = nested_value();
    value["composition"]["layers"][0]["layers"][0]["layers"] = json!([{
        "type":"Shape", "id":90003, "name":"Unchanged offscreen source path",
        "blendMode":"normal", "activeRange":{"start":0,"duration":1500},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
        "shape":{"path":{"commands":[
            {"type":"moveTo","x":-70000,"y":-70000},
            {"type":"lineTo","x":70000,"y":-70000},
            {"type":"lineTo","x":70000,"y":70000},
            {"type":"lineTo","x":-70000,"y":70000},{"type":"close"}
        ]},"fills":[{"paint":{"type":"solid","color":[1,1,1,1]},
            "fillRule":"nonZeroWinding","blendMode":"normal","opacity":1}]}
    }]);
    value
}

fn bounded_source_dimensions(value: Value) -> [u16; 2] {
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let occurrence = native
        .items
        .iter()
        .find_map(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return None;
            };
            comp.layers
                .iter()
                .find(|layer| layer.name.as_ref() == "Nested identity output")
        })
        .expect("bounded occurrence retained");
    let ItemKind::Composition(comp) = &native.item(occurrence.record.source_id()).unwrap().kind
    else {
        panic!("bounded source composition");
    };
    assert_eq!(comp.duration_secs, 1.5);
    assert_eq!(comp.layers.len(), 1);
    assert_eq!(
        comp.layers[0].name.as_ref(),
        "Unchanged offscreen source path"
    );
    [comp.width, comp.height]
}

#[test]
fn nested_effect_input_intersects_content_with_animated_kernel_demand() {
    let mut value = oversized_input_value();
    value["composition"]["layers"][0]["layers"][0]["effects"] = json!([
        {"id":90011,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":32,"repeatEdgePixels":false}},
        {"id":90012,"enabled":true,"effect":{"type":"outerGlow","enabled":true,
            "color":[0.3,0.8,0.2,1],"size":7,"spread":0.1,"range":0.6,"blendMode":"screen"}}
    ]);
    value["composition"]["dynamics"]["entries"] = json!([{
        "target":{"kind":"effectProperty","effectId":90011,"paramName":"blurriness"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"small","layerTime":0,"value":{"type":"float","value":32},"easing":{"type":"linear"}},
            {"id":"large","layerTime":1000,"value":{"type":"float","value":64},"easing":{"type":"linear"}}
        ]}
    }]);
    assert_eq!(bounded_source_dimensions(value), [461, 321]);
}

#[test]
fn nested_effect_input_keeps_smaller_content_and_rejects_unknown_support() {
    let mut value = oversized_input_value();
    value["composition"]["layers"][0]["layers"][0]["effects"] = json!([
        {"type":"outerGlow","enabled":true,"size":7,"spread":0.1,"range":0.6}
    ]);
    value["composition"]["layers"][0]["layers"][0]["layers"][0]["shape"]["path"]["commands"] = json!([
        {"type":"moveTo","x":50,"y":50},{"type":"lineTo","x":100,"y":50},
        {"type":"lineTo","x":100,"y":100},{"type":"lineTo","x":50,"y":100},{"type":"close"}
    ]);
    assert_eq!(bounded_source_dimensions(value), [50, 50]);
    let dynamics = super::super::AnimationIndex::new(&[]);
    for effect in [
        json!({"type":"gaussianBlur","blurriness":12,"repeatEdgePixels":true}),
        json!({"type":"gaussianBlur","blurriness":12,"layerSize":[100,100]}),
        json!({"type":"unsupportedSpatialEffect"}),
    ] {
        let record = serde_json::from_value(effect).unwrap();
        assert!(effect_support::input_reach(&[record], &dynamics).is_err());
    }
}

#[test]
fn nested_matted_input_uses_parent_demand_without_changing_matte() {
    let mut value = oversized_input_value();
    let mut matte = value["composition"]["layers"][0]["layers"][0]["layers"][0].clone();
    matte["id"] = json!(90010);
    matte["name"] = json!("Native alpha matte");
    value["composition"]["layers"][0]["layers"][0]["trackMatte"] =
        json!({"mode":"alpha","layer":90010});
    value["composition"]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .push(matte);
    assert_eq!(bounded_source_dimensions(value), [319, 179]);
}

#[test]
fn nested_identity_viewport_retains_short_source_and_oversized_editable_geometry() {
    let document =
        fx_schema::EditableFxCompositionDocument::from_json_value(nested_value()).unwrap();
    let output = super::super::to_aep(&document).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let occurrence = native
        .items
        .iter()
        .find_map(|item| {
            let ItemKind::Composition(composition) = &item.kind else {
                return None;
            };
            composition
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == "Nested identity output")
        })
        .expect("nested editable occurrence retained");
    let ItemKind::Composition(source) = &native.item(occurrence.record.source_id()).unwrap().kind
    else {
        panic!("nested editable source expected");
    };
    assert_eq!([source.width, source.height], [319, 179]);
    assert_eq!(source.duration_secs, 1.5);
    assert_eq!(source.layers.len(), 1);
    assert_eq!(
        source.layers[0].name.as_ref(),
        "Retained oversized geometry"
    );
    let properties = crate::properties::read_transform(&source.layers[0].content).unwrap();
    let scale = properties
        .iter()
        .find(|p| p.match_name == "ADBE Scale")
        .unwrap();
    assert_eq!(scale.numeric.as_ref().unwrap().values[..2], [10_000.0, 1.0]);
}
