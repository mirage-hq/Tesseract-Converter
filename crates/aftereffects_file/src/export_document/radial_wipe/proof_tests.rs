use super::super::*;
use super::tests::edited_half_plane;
use crate::{
    effects::native::read_effects,
    structure::{ItemKind, read_project},
};
use serde_json::{Value, json};

#[test]
fn edited_quadrant_half_plane_exports_current_center_and_hold_angles() {
    let mut input = edited_half_plane();
    input["backgroundColor"] = json!([0, 0, 0, 0]);
    let group = &mut input["composition"]["layers"][0];
    let template = group["layers"][0].clone();
    let mut guide = group["layers"][1].clone();
    guide["transform"]["position"] = json!([41, 17]);
    guide["transform"]["rotation"] = json!(30);
    let mut children = Vec::new();
    for (index, (position, color)) in [
        ([0, 0], [1, 0, 0, 1]),
        ([48, 0], [0, 1, 0, 1]),
        ([0, 32], [0, 0, 1, 1]),
        ([48, 32], [1, 1, 0, 1]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut paint = template.clone();
        paint["id"] = json!(820 + index);
        paint["name"] = json!(format!("Quadrant {index}"));
        paint["rect"]["size"] = json!([48, 32]);
        paint["rect"]["fillColor"] = json!(color);
        paint["transform"]["position"] = json!(position);
        children.push(paint);
    }
    children.push(guide);
    group["layers"] = Value::Array(children);
    let keys = input["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
        .as_array_mut()
        .unwrap();
    for (key, angle) in keys.iter_mut().zip([30, 120]) {
        key["easing"] = json!({"type":"hold"});
        key["value"]["value"] = json!(angle);
    }
    let document = EditableFxCompositionDocument::from_json_value(input.clone()).unwrap();
    let output = to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("omitted") && d.layer_id.is_some()),
        "{:?}",
        output.diagnostics
    );
    let project = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
        panic!("root")
    };
    let owner = &root.layers[0];
    let effects = read_effects(&owner.content, [96., 64.]).0;
    let wipe = &effects[0];
    assert_eq!(wipe.match_name, "ADBE Radial Wipe");
    assert_eq!(
        wipe.parameters
            .iter()
            .find(|p| p.match_name.ends_with("-0003"))
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values,
        [41., 17.]
    );
    let angle = wipe
        .parameters
        .iter()
        .find(|p| p.match_name.ends_with("-0002"))
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(
        angle
            .keyframes
            .iter()
            .map(|k| (k.time_secs, k.values[0]))
            .collect::<Vec<_>>(),
        [(0., 30.), (1., 120.)]
    );
    let reimported =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    assert!(
        reimported
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .any(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|p| p.property_type() == PropType::Rotation)
                    && matches!(entry.animator.data(), AnimatorData::Keyframes { track, .. }
                if track.keyframes().len() == 2
                    && track.keyframes()[0].value() == &PropertyValue::Float(30.0)
                    && track.keyframes()[1].value() == &PropertyValue::Float(120.0)
                    && track.keyframes()[1].easing() == PropertyKeyframeEasing::Hold)
            }),
        "exported direct native Angle keys must remain editable on reimport"
    );
    if let Ok(dir) = std::env::var("AEP_RADIAL_EXPORT_PROOF_DIR") {
        let dir = std::path::Path::new(&dir);
        std::fs::write(
            dir.join("explicit-quadrants.json"),
            serde_json::to_vec_pretty(&input).unwrap(),
        )
        .unwrap();
        let path = dir.join("exported.aep");
        assert!(!path.exists(), "use a fresh proof directory");
        std::fs::write(path, output.bytes).unwrap();
    }
}

#[test]
fn unsafe_half_plane_edits_keep_content_and_do_not_emit_a_native_wipe() {
    for (name, path, value) in [
        (
            "inversion",
            "/composition/layers/0/masks/0/inverted",
            json!(true),
        ),
        (
            "feather",
            "/composition/layers/0/masks/0/feather",
            json!([1, 0]),
        ),
        (
            "opacity",
            "/composition/layers/0/masks/0/opacity",
            json!(0.5),
        ),
        (
            "expansion",
            "/composition/layers/0/masks/0/expansion",
            json!(1),
        ),
        (
            "guide scale",
            "/composition/layers/0/layers/1/transform/scale",
            json!([50, 100]),
        ),
        (
            "guide clock",
            "/composition/layers/0/layers/1/activeRange/duration",
            json!(1000),
        ),
        (
            "owner clock",
            "/composition/layers/0/playback",
            super::super::tests::fixture_linear_playback(
                json!({"start":100,"duration":2000}),
                json!({"start":0,"duration":2000}),
            ),
        ),
        (
            "hidden guide",
            "/composition/layers/0/layers/1/isHidden",
            json!(true),
        ),
        (
            "other animator",
            "/composition/dynamics/entries/0/target/propertyType",
            json!("positionX"),
        ),
    ] {
        let mut input = edited_half_plane();
        if name == "hidden guide" {
            input["composition"]["layers"][0]["layers"][1]["isHidden"] = value;
        } else {
            *input.pointer_mut(path).unwrap() = value;
        }
        let output =
            to_aep(&EditableFxCompositionDocument::from_json_value(input).unwrap()).unwrap();
        let project = read_project(&output.bytes).unwrap();
        let ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
            panic!("root")
        };
        assert!(!root.layers.is_empty(), "{name}: {:?}", output.diagnostics);
        assert!(
            root.layers
                .iter()
                .all(|l| read_effects(&l.content, [96., 64.])
                    .0
                    .iter()
                    .all(|e| e.match_name != "ADBE Radial Wipe")),
            "{name}"
        );
        assert!(
            output.diagnostics.iter().any(|d| d.layer_id.is_some()),
            "{name}: rejection needs context"
        );
    }
}

#[test]
fn half_plane_uses_rebased_source_origin_and_precedes_owner_blur() {
    let mut input = edited_half_plane();
    let group = &mut input["composition"]["layers"][0];
    group["transform"]["position"] = json!([20, 12]);
    group["layers"][0]["transform"]["position"] = json!([-20, -12]);
    group["effects"] =
        json!([{"id":9001,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":4}}]);
    let output = to_aep(&EditableFxCompositionDocument::from_json_value(input).unwrap()).unwrap();
    let project = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &project.item(1).unwrap().kind else {
        panic!("root")
    };
    let effects = read_effects(&root.layers[0].content, [96., 64.]).0;
    assert_eq!(
        effects
            .iter()
            .map(|e| e.match_name.as_str())
            .collect::<Vec<_>>(),
        ["ADBE Radial Wipe", "ADBE Gaussian Blur 2"],
        "{:?}",
        output.diagnostics
    );
    let center = effects[0]
        .parameters
        .iter()
        .find(|p| p.match_name.ends_with("-0003"))
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(center.values, [60., 42.]);
}

#[test]
fn half_plane_shared_guide_is_not_consumed_into_an_effect() {
    let mut input = edited_half_plane();
    input["composition"]["layers"][0]["layers"][0]["masks"] = json!([{
        "id":814,"mode":"add","layer":812,"inverted":false,"feather":[0,0],"expansion":0,"opacity":1
    }]);
    let document = EditableFxCompositionDocument::from_json_value(input).unwrap();
    let LayerData::Group(group) = document.composition().layers()[0].data() else {
        panic!("group")
    };
    assert!(
        super::recognize(
            group,
            document.composition().dynamics().entries(),
            Time::from_millis(2000)
        )
        .is_err()
    );
}

#[test]
fn independent_native_half_plane_import_remains_native_exportable() {
    let source = include_bytes!("../../../tests/fixtures/effects/radial_wipe_half_plane.aep");
    let project = read_project(source).unwrap();
    let imported =
        crate::structure_document::to_structural_fx_document(&project, Some(22)).unwrap();
    let output = to_aep(&imported.document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert!(
        native.items.iter().any(|item| match &item.kind {
            ItemKind::Composition(comp) =>
                comp.layers
                    .iter()
                    .any(|layer| read_effects(&layer.content, [96., 64.])
                        .0
                        .iter()
                        .any(|e| e.match_name == "ADBE Radial Wipe")),
            _ => false,
        }),
        "{:?}",
        output.diagnostics
    );
}
