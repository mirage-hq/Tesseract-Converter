use super::super::tests::{data, list, name, numeric};
use super::*;

#[test]
fn native_hold_endpoint_flags_are_admitted_without_weakening_curve_guards() {
    let curve = super::super::tests::native_hold_endpoint_curve("control-0");
    assert_eq!(
        curve
            .keyframes
            .iter()
            .map(|key| (
                key.time_secs,
                key.values[0],
                key.in_interpolation,
                key.out_interpolation
            ))
            .collect::<Vec<_>>(),
        vec![(-0.5, 0.0, 1, 3), (0.5, 1.0, 1, 3), (1.5, 0.4, 1, 1)]
    );
    validate_scalar_curve(&curve).unwrap();
    let mut invalid = curve.clone();
    invalid.keyframes[1].in_interpolation = 0;
    assert!(validate_scalar_curve(&invalid).is_err());
    invalid = curve.clone();
    invalid.keyframes[0].out_interpolation = 1;
    invalid.keyframes[1].in_interpolation = 3;
    assert!(validate_scalar_curve(&invalid).is_err());
    invalid = curve;
    invalid.keyframes[0].out_interpolation = 2;
    invalid.keyframes[1].in_interpolation = 3;
    assert!(validate_scalar_curve(&invalid).is_err());
}
use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
};

#[test]
fn native_hold_endpoint_flags_public_render_alias_lowers_saved_native_expression() {
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/hold_endpoint_render.aep"
    ))
    .unwrap();
    let composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some(composition.as_ref()),
            _ => None,
        })
        .unwrap();
    let alias = composition
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "rotation-alias")
        .unwrap();
    let base = property(alias, "ADBE Rotate Z");
    assert_eq!(
        property_expression(alias, ScalarProperty::Rotation).unwrap(),
        "thisComp.layer(\"driver\").transform.rotation"
    );
    let lowered = lower(alias, composition, "ADBE Rotate Z", &base)
        .unwrap()
        .unwrap();
    assert!(!lowered.expression_enabled);
    assert!(!lowered.expression_present);
    assert_eq!(
        lowered
            .keyframes
            .iter()
            .map(|key| (
                key.time_secs,
                key.values[0],
                key.in_interpolation,
                key.out_interpolation
            ))
            .collect::<Vec<_>>(),
        vec![(-0.5, 0.0, 1, 3), (0.5, 90.0, 1, 3), (1.5, 180.0, 1, 1)]
    );
}

fn template() -> (Layer, Composition) {
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    let mut composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some((**composition).clone()),
            _ => None,
        })
        .unwrap();
    let layer = composition.layers[0].clone();
    composition.layers.clear();
    (layer, composition)
}

fn record_with_id(record: &LayerRecord, id: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[..4].copy_from_slice(&id.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn with_start_time(record: &LayerRecord, numerator: i32, denominator: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[12..16].copy_from_slice(&numerator.to_be_bytes());
    bytes[16..20].copy_from_slice(&denominator.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn transform_content(leaves: Vec<(&str, crate::rifx::Chunk)>) -> Vec<crate::rifx::Chunk> {
    let leaves = leaves
        .into_iter()
        .flat_map(|(property, numeric)| [data(b"tdmn", property.as_bytes()), numeric])
        .collect();
    vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(b"tdgp", leaves),
        ],
    )]
}

fn layer(template: &Layer, id: u32, name: &str, leaves: Vec<(&str, crate::rifx::Chunk)>) -> Layer {
    let mut layer = template.clone();
    layer.name = name.into();
    layer.record = record_with_id(&layer.record, id);
    layer.content = transform_content(leaves);
    layer
}

fn property(layer: &Layer, name: &str) -> NumericProperty {
    properties::read_transform(&layer.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == name)
        .unwrap()
        .numeric
        .unwrap()
}

fn opacity_numeric(template: &Layer) -> crate::rifx::Chunk {
    let root = properties::root_runs(&template.content).unwrap();
    let transform = unique_run(&root, "ADBE Transform Group").unwrap();
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").unwrap()).unwrap();
    let body = properties::unique_list(unique_run(&leaves, "ADBE Opacity").unwrap(), *b"tdbs")
        .unwrap()
        .to_vec();
    list(b"tdbs", body)
}

fn scalar_control(
    kind: &[u8],
    parameter: &[u8],
    display_name: &str,
    label: &str,
    value: f64,
) -> Vec<crate::rifx::Chunk> {
    let mut scalar = numeric(&[value], None);
    let children = scalar.children_mut().unwrap();
    children.retain(|chunk| chunk.id() != *b"tdsn");
    children.push(name(display_name));
    vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Effect Parade"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", kind),
                    list(
                        b"sspc",
                        vec![list(
                            b"tdgp",
                            vec![name(label), data(b"tdmn", parameter), scalar],
                        )],
                    ),
                ],
            ),
        ],
    )]
}

fn angle_control(label: &str, value: f64) -> Vec<crate::rifx::Chunk> {
    scalar_control(
        b"ADBE Angle Control",
        b"ADBE Angle Control-0001",
        "Angle",
        label,
        value,
    )
}

fn slider_control(label: &str, value: f64) -> Vec<crate::rifx::Chunk> {
    scalar_control(
        b"ADBE Slider Control",
        b"ADBE Slider Control-0001",
        "Slider",
        label,
        value,
    )
}

#[test]
fn grammar_is_only_an_optionally_negative_direct_scalar_transform_reference() {
    assert_eq!(
        parse(" - thisComp.layer ( 'M_01' ).transform.yPosition; ").unwrap(),
        Link {
            layer: "M_01",
            property: ScalarProperty::PositionY,
            sign: -1.0,
        }
    );
    assert_eq!(
        parse("thisComp.layer(\"C_03\").transform.rotation").unwrap(),
        Link {
            layer: "C_03",
            property: ScalarProperty::Rotation,
            sign: 1.0,
        }
    );
    for expression in [
        "+thisComp.layer('M').transform.yPosition",
        "thisComp.layer('').transform.rotation",
        "thisComp.layer('M').transform.position[1]",
        "thisComp.layer('M').transform.zPosition",
        "thisComp.layer('M').transform.yPosition.valueAtTime(time)",
        "thisComp.layer('M').transform.yPosition + 1",
        "--thisComp.layer('M').transform.rotation",
        "thisComp.layer(name).transform.rotation",
        "thisComp.layer('M').transform.rotation; execute()",
    ] {
        assert!(parse(expression).is_none(), "{expression}");
    }

    assert!(is_same_member_identity(
        " transform.yPosition; ",
        ScalarProperty::PositionY
    ));
    for expression in [
        "transform.xPosition",
        "-transform.yPosition",
        "transform.yPosition + 1",
        "transform.yPosition.valueAtTime(time)",
    ] {
        assert!(
            !is_same_member_identity(expression, ScalarProperty::PositionY),
            "{expression}"
        );
    }
}

#[test]
fn same_member_identities_lower_before_sibling_alias_parsing() {
    let (template, mut composition) = template();
    for (name, expression) in [
        ("ADBE Position_0", "transform.xPosition"),
        ("ADBE Position_1", "transform.yPosition"),
        ("ADBE Rotate Z", "transform.rotation"),
    ] {
        let owner = layer(
            &template,
            1,
            "Identity",
            vec![(name, numeric(&[17.0], Some(expression)))],
        );
        composition.layers = vec![owner.clone()];

        let lowered = lower(&owner, &composition, name, &property(&owner, name))
            .unwrap_or_else(|| panic!("identity {name} was not recognized"))
            .unwrap();
        assert_eq!(lowered.values, vec![17.0]);
        assert!(!lowered.expression_enabled);
        assert!(!lowered.expression_present);
    }

    for (name, mismatched_expression) in [
        ("ADBE Position_0", "transform.yPosition"),
        ("ADBE Position_1", "transform.rotation"),
        ("ADBE Rotate Z", "transform.xPosition"),
    ] {
        let owner = layer(
            &template,
            1,
            "Mismatch",
            vec![(name, numeric(&[17.0], Some(mismatched_expression)))],
        );
        composition.layers = vec![owner.clone()];
        assert!(lower(&owner, &composition, name, &property(&owner, name)).is_none());
    }
}

#[test]
fn native_position_alias_rebases_and_negates_values_and_temporal_speeds() {
    let (template, mut composition) = template();
    let source = layer(
        &template,
        2129,
        "M_01",
        vec![("ADBE Position_1", opacity_numeric(&template))],
    );
    let mut owner = layer(
        &template,
        2130,
        "B_03",
        vec![(
            "ADBE Position_1",
            numeric(
                &[574.604_309_082_030_8],
                Some("-thisComp.layer(\"M_01\").transform.yPosition"),
            ),
        )],
    );
    owner.record = with_start_time(&owner.record, 1, 2);
    composition.layers = vec![owner.clone(), source.clone()];

    let mut expected = property(&source, "ADBE Position_1");
    let offset = (source.record.start_time().unwrap() - owner.record.start_time().unwrap())
        / owner.record.stretch().unwrap();
    for value in expected.values.iter_mut().chain(
        expected
            .keyframes
            .iter_mut()
            .flat_map(|key| key.values.iter_mut()),
    ) {
        *value = -*value;
    }
    for key in &mut expected.keyframes {
        key.time_secs += offset;
        for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
            *speed = -*speed;
        }
    }

    let lowered = lower(
        &owner,
        &composition,
        "ADBE Position_1",
        &property(&owner, "ADBE Position_1"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(lowered, expected);
}

#[test]
fn long_alias_chains_are_iterative_and_keep_signs_and_cycle_detection() {
    let (template, mut composition) = template();
    composition.layers = (0..128)
        .map(|index| {
            let expression = format!(
                "-thisComp.layer(\"chain-{}\").transform.rotation",
                index + 1
            );
            layer(
                &template,
                index + 1000,
                &format!("chain-{index}"),
                vec![(
                    "ADBE Rotate Z",
                    numeric(&[7.0], (index < 127).then_some(expression.as_str())),
                )],
            )
        })
        .collect();
    let owner = &composition.layers[0];
    let lowered = lower(
        owner,
        &composition,
        "ADBE Rotate Z",
        &property(owner, "ADBE Rotate Z"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(lowered.values, vec![-7.0]);
    composition.layers[127] = layer(
        &template,
        1127,
        "chain-127",
        vec![(
            "ADBE Rotate Z",
            numeric(
                &[7.0],
                Some("thisComp.layer(\"chain-0\").transform.rotation"),
            ),
        )],
    );
    let owner = &composition.layers[0];
    let error = lower(
        owner,
        &composition,
        "ADBE Rotate Z",
        &property(owner, "ADBE Rotate Z"),
    )
    .unwrap()
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cyclic transform property alias")
    );
}

#[test]
fn alias_chain_crosses_transform_members_and_resolves_an_existing_angle_source() {
    let (template, mut composition) = template();
    let owner = layer(
        &template,
        2126,
        "C_03",
        vec![(
            "ADBE Rotate Z",
            numeric(
                &[0.0],
                Some("-thisComp.layer(\"Rotater_Top\").transform.rotation"),
            ),
        )],
    );
    let top = layer(
        &template,
        2162,
        "Rotater_Top",
        vec![(
            "ADBE Rotate Z",
            numeric(
                &[-2.0],
                Some("thisComp.layer(\"Rotater_Btm\").transform.rotation"),
            ),
        )],
    );
    let bottom_expression = "rot = thisComp.layer(\"Controller\").effect(\"Rotater_Cntrl\")(\"Angle\"); transform.rotation-rot;";
    let bottom = layer(
        &template,
        2161,
        "Rotater_Btm",
        vec![("ADBE Rotate Z", numeric(&[10.0], Some(bottom_expression)))],
    );
    let mut controller = template.clone();
    controller.name = "Controller".into();
    controller.record = record_with_id(&controller.record, 2200);
    controller.content = angle_control("Rotater_Cntrl", 45.0);
    composition.layers = vec![owner.clone(), top, bottom, controller];

    let lowered = lower(
        &owner,
        &composition,
        "ADBE Rotate Z",
        &property(&owner, "ADBE Rotate Z"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(lowered.values, vec![35.0]);
    assert!(!lowered.expression_enabled);
    assert!(!lowered.expression_present);
}

#[test]
fn same_layer_cross_member_alias_uses_the_actual_source_leaf() {
    let (template, mut composition) = template();
    let owner = layer(
        &template,
        2100,
        "Cross_Component",
        vec![
            (
                "ADBE Position_0",
                numeric(&[17.0], Some("transform.xPosition")),
            ),
            (
                "ADBE Position_1",
                numeric(
                    &[0.0],
                    Some("thisComp.layer(\"Cross_Component\").transform.xPosition"),
                ),
            ),
        ],
    );
    composition.layers = vec![owner.clone()];

    let lowered = lower(
        &owner,
        &composition,
        "ADBE Position_1",
        &property(&owner, "ADBE Position_1"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(lowered.values, vec![17.0]);
}

#[test]
fn cross_member_alias_resolves_an_existing_direct_slider_source() {
    let (template, mut composition) = template();
    let owner = layer(
        &template,
        2131,
        "B_02",
        vec![(
            "ADBE Position_1",
            numeric(
                &[-574.604_309_082_031],
                Some("thisComp.layer(\"M_01\").transform.xPosition"),
            ),
        )],
    );
    let source = layer(
        &template,
        2129,
        "M_01",
        vec![(
            "ADBE Position_0",
            numeric(
                &[12.0],
                Some("-thisComp.layer(\"Controller\").effect(\"X\")(\"Slider\")"),
            ),
        )],
    );
    let mut controller = template.clone();
    controller.name = "Controller".into();
    controller.record = record_with_id(&controller.record, 2200);
    controller.content = slider_control("X", 12.0);
    composition.layers = vec![owner.clone(), source, controller];

    let lowered = lower(
        &owner,
        &composition,
        "ADBE Position_1",
        &property(&owner, "ADBE Position_1"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(lowered.values, vec![-12.0]);
}

#[test]
fn alias_resolution_rejects_cycles_ambiguous_names_and_unsafe_clocks() {
    let (template, mut composition) = template();
    let first = layer(
        &template,
        1,
        "First",
        vec![(
            "ADBE Rotate Z",
            numeric(
                &[0.0],
                Some("thisComp.layer(\"Second\").transform.rotation"),
            ),
        )],
    );
    let second = layer(
        &template,
        2,
        "Second",
        vec![(
            "ADBE Rotate Z",
            numeric(&[0.0], Some("thisComp.layer(\"First\").transform.rotation")),
        )],
    );
    composition.layers = vec![first.clone(), second.clone()];
    assert!(
        lower(
            &first,
            &composition,
            "ADBE Rotate Z",
            &property(&first, "ADBE Rotate Z"),
        )
        .unwrap()
        .is_err()
    );

    let mut duplicate = second.clone();
    duplicate.record = record_with_id(&duplicate.record, 3);
    composition.layers = vec![first.clone(), second.clone(), duplicate];
    assert!(
        lower(
            &first,
            &composition,
            "ADBE Rotate Z",
            &property(&first, "ADBE Rotate Z"),
        )
        .unwrap()
        .is_err()
    );

    let mut raw = second;
    raw.content = transform_content(vec![("ADBE Rotate Z", numeric(&[12.0], None))]);
    let mut bytes = raw.record.encode();
    bytes[8..12].copy_from_slice(&2_i32.to_be_bytes());
    raw.record = LayerRecord::decode(&bytes).unwrap();
    composition.layers = vec![first.clone(), raw];
    assert!(
        lower(
            &first,
            &composition,
            "ADBE Rotate Z",
            &property(&first, "ADBE Rotate Z"),
        )
        .unwrap()
        .is_err()
    );
}
