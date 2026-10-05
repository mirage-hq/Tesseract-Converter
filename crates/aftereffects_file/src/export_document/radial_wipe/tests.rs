use super::super::tests::{imported, rect};
use super::super::*;
use crate::{
    effects::native::read_effects,
    structure::{ItemKind, read_project},
};
use serde_json::{Value, json};

pub(super) fn edited_half_plane() -> Value {
    // Explicit current FX authored from unrelated generic Group/Rect/Shape
    // templates, not from the Radial Wipe importer or the new AEP writer.
    let mut input = imported();
    input["dimensions"] = json!({"width":96,"height":64});
    input["duration"] = json!(2.0);
    let mut group = input["composition"]["layers"][0].clone();
    group["id"] = json!(810);
    group["parent"] = Value::Null;
    group["name"] = json!("Edited canvas");
    group["playback"] = super::super::tests::fixture_linear_playback(
        json!({"start":0,"duration":2000}),
        json!({"start":0,"duration":2000}),
    );
    group["effects"] = json!([]);
    group["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    let mut paint = rect(&input, 811);
    paint["parent"] = json!(810);
    paint["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    paint["rect"]["position"] = json!([0.0, 0.0]);
    paint["activeRange"] = json!({"start":0,"duration":2000});
    paint["rect"]["size"] = json!([96.0, 64.0]);
    paint["transform"]["position"] = json!([0.0, 0.0]);
    let mut guide = paint.clone();
    guide["id"] = json!(812);
    guide["type"] = json!("Shape");
    guide["name"] = json!("Arbitrary edited guide");
    guide.as_object_mut().unwrap().remove("rect");
    guide["transform"]["position"] = json!([40.0, 30.0]);
    guide["transform"]["rotation"] = json!(25.0);
    guide["activeRange"] = json!({"start":0,"duration":2000});
    guide["shape"] = json!({"path":{"commands":[
        {"type":"moveTo","x":-100.0,"y":-100.0},
        {"type":"lineTo","x":0.0,"y":-100.0},
        {"type":"lineTo","x":0.0,"y":100.0},
        {"type":"lineTo","x":-100.0,"y":100.0},
        {"type":"close"}
    ]},"fills":[],"strokes":[]});
    group["layers"] = json!([paint, guide]);
    group["masks"] = json!([{"id":813,"mode":"add","layer":812,
        "inverted":false,"feather":[0,0],"expansion":0,"opacity":1}]);
    input["composition"]["layers"] = json!([group]);
    input["composition"]["dynamics"] = json!({"entries": [
        serde_json::to_value(super::super::tests::keyed_entry(
            LayerId::new(812), PropType::Rotation,
            [(0, PropertyValue::Float(25.0)),(1000, PropertyValue::Float(115.0))]
        )).unwrap()
    ]});
    input
}

#[test]
fn edited_fx_half_plane_writes_native_wipe_before_effects_without_painting_guide() {
    let input = edited_half_plane();
    let output = to_aep(&EditableFxCompositionDocument::from_json_value(input).unwrap()).unwrap();
    let project = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
        panic!("root")
    };
    let owner = root
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Edited canvas")
        .unwrap_or_else(|| panic!("owner omitted: {:?}", output.diagnostics));
    let ItemKind::Composition(source) = &project.item(owner.record.source_id()).unwrap().kind
    else {
        panic!("source")
    };
    assert_eq!(source.width, 96, "{:?}", output.diagnostics);
    assert_eq!(source.height, 64);
    assert!(
        !source
            .layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Arbitrary edited guide")
    );
    assert!(
        source
            .layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 811")
    );
    let (effects, warnings) = read_effects(&owner.content, [96.0, 64.0]);
    assert!(warnings.is_empty(), "{warnings:?}");
    let wipe = effects
        .first()
        .expect("native effect before ordinary owner effects");
    assert_eq!(wipe.match_name, "ADBE Radial Wipe");
    for (suffix, expected) in [
        ("0001", vec![50.0]),
        ("0003", vec![40.0, 30.0]),
        ("0004", vec![1.0]),
        ("0005", vec![0.0]),
    ] {
        let prop = wipe
            .parameters
            .iter()
            .find(|prop| prop.match_name == format!("ADBE Radial Wipe-{suffix}"))
            .unwrap();
        assert_eq!(prop.numeric.as_ref().unwrap().values, expected);
    }
    let angle = wipe
        .parameters
        .iter()
        .find(|prop| prop.match_name == "ADBE Radial Wipe-0002")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(
        angle
            .keyframes
            .iter()
            .map(|key| (key.time_secs, key.values[0]))
            .collect::<Vec<_>>(),
        [(0.0, 25.0), (1.0, 115.0)]
    );
}

#[test]
fn finite_half_plane_that_does_not_cover_source_falls_back_without_consuming_guide() {
    let mut input = edited_half_plane();
    input["composition"]["layers"][0]["layers"][1]["shape"]["path"]["commands"] = json!([
        {"type":"moveTo","x":-40.0,"y":-40.0},
        {"type":"lineTo","x":0.0,"y":-40.0},
        {"type":"lineTo","x":0.0,"y":40.0},
        {"type":"lineTo","x":-40.0,"y":40.0},{"type":"close"}
    ]);
    let output = to_aep(&EditableFxCompositionDocument::from_json_value(input).unwrap()).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Radial Wipe export not admitted")),
        "{:?}",
        output.diagnostics
    );
    let project = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
        panic!("root")
    };
    let owner = root
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Edited canvas")
        .unwrap();
    assert!(
        read_effects(&owner.content, [96.0, 64.0])
            .0
            .iter()
            .all(|e| e.match_name != "ADBE Radial Wipe")
    );
}

#[test]
fn warp_nested_static_half_plane_keeps_native_masks() {
    let mut input = edited_half_plane();
    input["composition"]["dynamics"] = json!({"entries": []});
    let mut child = input["composition"]["layers"][0].clone();
    child["parent"] = json!(900);
    child["transform"]["scale"] = json!([-100, 100]);
    child["transform"]["rotation"] = json!(80);
    let mut outer = child.clone();
    outer["id"] = json!(900);
    outer["parent"] = Value::Null;
    outer["name"] = json!("Unrelated nested mask owner");
    outer["transform"] = serde_json::to_value(identity_fx_transform()).unwrap();
    outer["masks"] = json!([]);
    outer["layers"] = json!([child]);
    input["composition"]["layers"] = json!([outer]);
    let output = to_aep(&EditableFxCompositionDocument::from_json_value(input).unwrap()).unwrap();
    let project = read_project(&output.bytes).unwrap();
    let reopened = crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    fn find_owner(value: &Value) -> Option<&Value> {
        if value.get("name").and_then(Value::as_str) == Some("Edited canvas") {
            return Some(value);
        }
        value.get("layers")?.as_array()?.iter().find_map(find_owner)
    }
    let json = reopened.document.to_json_value().unwrap();
    let owner = find_owner(&json["composition"]).expect("nested owner survives");
    let mask = &owner["masks"][0];
    assert_eq!(mask["mode"], "add", "{owner}");
    assert_eq!(mask["opacity"], 1.0);
    assert_eq!(mask["inverted"], false);
    assert!(
        mask["layer"].as_u64().is_some(),
        "native editable mask guide"
    );
    assert!(
        !reopened
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("Radial Wipe omitted") }),
        "{:?}",
        reopened.diagnostics
    );
}
