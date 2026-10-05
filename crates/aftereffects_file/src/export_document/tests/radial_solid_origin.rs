//! Fresh forward export point-space assertions; native kernel fidelity is separate.
use super::*;
use crate::effects::native::read_effects;
use sha2::{Digest, Sha256};

#[test]
fn radial_solid_origin_export_native_oracle_is_pinned_and_feature_specific() {
    let bytes = include_bytes!("../../../tests/fixtures/effects/radial-solid-origin.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "e3f37f8ba1e79f8e338b093225cc5787325b1d38339a5c780e778f5242d85471"
    );
    let project = read_project(bytes).unwrap();
    for (id, expected) in [(1, vec![13.0, 20.0]), (16, vec![20.0, 3.0])] {
        let ItemKind::Composition(comp) = &project.item(id).unwrap().kind else {
            panic!("independently authored composition")
        };
        assert_eq!(comp.layers.len(), 1);
        let (effects, _) = read_effects(&comp.layers[0].content, [40.0, 30.0]);
        assert_eq!(effects.len(), 1);
        let center = effects[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Radial Blur-0002")
            .unwrap();
        assert_eq!(center.numeric.as_ref().unwrap().values, expected);
    }
}

fn input() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/effects/radial-solid-origin.fx.json"
    ))
    .unwrap()
}

fn edited_input() -> Value {
    let mut value = input();
    let layer = &mut value["composition"]["layers"][0];
    layer["rect"]["position"] = json!([12, 3]);
    layer["transform"]["anchorPoint"] = json!([32, 18]);
    layer["effects"][0]["effect"]["centerX"] = json!(0.8);
    layer["effects"][0]["effect"]["centerY"] = json!(0.2);
    value
}

fn native_center(output: &ExportedDocument) -> Vec<f64> {
    let project = read_project(&output.bytes).unwrap();
    let (effects, warnings) = read_effects(&layers(&project)[0].content, [40.0, 30.0]);
    assert!(
        warnings.iter().all(|warning| warning == "ADBE Radial Blur/ADBE Radial Blur-0005: unsupported or malformed property: unsupported effect default kind"),
        "{warnings:?}"
    );
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].match_name, "ADBE Radial Blur");
    let property = effects[0]
        .parameters
        .iter()
        .find(|property| property.match_name == "ADBE Radial Blur-0002")
        .unwrap();
    assert!(!property.numeric.as_ref().unwrap().animated);
    property.numeric.as_ref().unwrap().values.clone()
}

#[test]
fn radial_solid_origin_export_rebases_point_and_responds_to_input_edits() {
    let cases = [
        ("base", input(), vec![13.0, 20.0]),
        ("edited", edited_input(), vec![20.0, 3.0]),
    ];
    let outputs: Vec<_> = cases
        .into_iter()
        .map(|(name, value, expected)| {
            let output = export(value);
            (name, output, expected)
        })
        .collect();
    for (name, output, expected) in outputs {
        assert_eq!(
            native_center(&output),
            expected,
            "{name}: native source-relative center"
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("matching Anchor rebasing"))
        );
    }
}

#[test]
fn radial_solid_origin_export_declines_nonuniform_gated_and_vector_profiles() {
    for (field, value) in [("scale", json!([200, 100])), ("opacity", json!(50))] {
        let mut value_input = input();
        value_input["composition"]["layers"][0]["transform"][field] = value;
        let output = export(value_input);
        assert_eq!(native_center(&output), vec![20.0, 15.0]);
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("outside the static isolated point-rebasing profile")
        }));
    }
    let mut animated = input();
    let mut amount_entry = serde_json::to_value(keyed_entry(
        LayerId::new(900),
        PropType::Rotation,
        [
            (0, PropertyValue::Float(20.0)),
            (1_000, PropertyValue::Float(40.0)),
        ],
    ))
    .unwrap();
    amount_entry["target"] = serde_json::to_value(fx_schema::PropertyTarget::effect_param(
        fx_schema::EffectId::new(901),
        "amount",
    ))
    .unwrap();
    animated["composition"]["dynamics"]["entries"] = json!([amount_entry]);
    let output = export(animated);
    assert_eq!(native_center(&output), vec![20.0, 15.0]);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("outside the static isolated point-rebasing profile")
    }));
    let mut vector = input();
    vector["composition"]["layers"][0]["description"] = json!("Editable native vector");
    let mut zero_origin_vector = vector.clone();
    zero_origin_vector["composition"]["layers"][0]["rect"]["position"] = json!([0, 0]);
    let unchanged_center = native_center(&export(zero_origin_vector));
    let output = export(vector);
    assert_eq!(native_center(&output), unchanged_center);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("matching Anchor rebasing"))
    );
}
