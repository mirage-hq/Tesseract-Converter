use super::*;
use crate::effects::native::read_effects;

const INPUT: &str =
    include_str!("../../../tests/fixtures/effects/directional-export-plane.fx.json");

fn check(input: Value, expected: [f64; 2]) {
    let output = export(input.clone());
    let native = read_project(&output.bytes).unwrap();
    let [layer] = layers(&native) else {
        panic!("expected one editable native solid");
    };
    let (effects, _) = read_effects(&layer.content, [12.0, 12.0]);
    let [effect] = effects.as_slice() else {
        panic!("expected sole native directional blur");
    };
    assert_eq!(effect.match_name, "ADBE Motion Blur");
    assert_eq!(
        effect.enabled,
        input["composition"]["layers"][0]["effects"][0]["enabled"]
    );
    for (name, expected) in ["ADBE Motion Blur-0001", "ADBE Motion Blur-0002"]
        .into_iter()
        .zip(expected)
    {
        let value = effect
            .parameters
            .iter()
            .find(|p| p.match_name == name)
            .unwrap();
        let numeric = value.numeric.as_ref().unwrap();
        assert!(!numeric.animated);
        assert_eq!(numeric.values, vec![expected], "{name}");
    }
}

#[test]
fn independent_directional_export_oracle_keeps_both_native_targets() {
    let native = read_project(include_bytes!(
        "../../../tests/fixtures/effects/directional-export-plane.aep"
    ))
    .unwrap();
    for (id, expected) in [(1, [0.0, 20.0]), (16, [60.0, 30.0])] {
        let ItemKind::Composition(comp) = &native.item(id).unwrap().kind else {
            panic!("pinned independent native target");
        };
        let [layer] = comp.layers.as_slice() else {
            panic!("independently authored sole owner");
        };
        let (effects, _) = read_effects(&layer.content, [12.0, 12.0]);
        let [effect] = effects.as_slice() else {
            panic!("independently authored sole effect");
        };
        assert_eq!(effect.match_name, "ADBE Motion Blur");
        assert!(effect.enabled);
        for (name, expected) in ["ADBE Motion Blur-0001", "ADBE Motion Blur-0002"]
            .into_iter()
            .zip(expected)
        {
            let property = effect
                .parameters
                .iter()
                .find(|p| p.match_name == name)
                .unwrap();
            assert_eq!(property.numeric.as_ref().unwrap().values, vec![expected]);
        }
    }
}

#[test]
fn fresh_directional_solid_export_inverts_static_screen_plane_and_fx_edits() {
    let input: Value = serde_json::from_str(INPUT).unwrap();
    check(input.clone(), [0.0, 20.0]);
    let mut edited = input;
    let layer = &mut edited["composition"]["layers"][0];
    layer["transform"]["scale"] = json!([300, 300]);
    layer["transform"]["rotation"] = json!(-45);
    layer["effects"][0]["effect"]["direction"] = json!(15);
    layer["effects"][0]["effect"]["blurLength"] = json!(90);
    check(edited, [60.0, 30.0]);
}

#[test]
fn directional_solid_export_declines_nonuniform_reflected_disabled_and_vector_profiles() {
    for scale in [[200, 300], [-200, -200]] {
        let mut input: Value = serde_json::from_str(INPUT).unwrap();
        input["composition"]["layers"][0]["transform"]["scale"] = json!(scale);
        check(input, [90.0, 40.0]);
    }
    let mut input: Value = serde_json::from_str(INPUT).unwrap();
    input["composition"]["layers"][0]["effects"][0]["enabled"] = json!(false);
    check(input, [90.0, 40.0]);
    let mut input: Value = serde_json::from_str(INPUT).unwrap();
    input["composition"]["layers"][0]["description"] = json!("Explicit editable vector");
    check(input, [90.0, 40.0]);
}

#[test]
fn directional_solid_export_declines_animated_owner_transform() {
    let mut input: Value = serde_json::from_str(INPUT).unwrap();
    input["composition"]["dynamics"]["entries"] = json!([keyed_entry(
        LayerId::new(900),
        PropType::Rotation,
        [
            (0, PropertyValue::Float(90.0)),
            (1000, PropertyValue::Float(180.0))
        ],
    )]);
    check(input, [90.0, 40.0]);
}
