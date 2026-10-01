use super::super::tests::{data, list, name, numeric};
use super::*;
use crate::structure_document::{
    animation, animation_budget::AnimationBudget, camera_normalization, control_links, transform,
};
use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
};
use fx_schema::LayerId;

const EXPR: &str = "rot = thisComp.layer(\"Scaler\").effect(\"Rotater_Cntrl\")(\"Angle\");\rtransform.rotation-rot;";

fn fixture(animated: bool) -> (Layer, Composition) {
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/property_1D_opacity.aep"
    ))
    .unwrap();
    let mut comp = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(c) => Some(c.clone()),
            _ => None,
        })
        .unwrap();
    let mut owner = comp.layers[0].clone();
    let root = properties::root_runs(&owner.content).unwrap();
    let run = unique_run(&root, "ADBE Transform Group").unwrap();
    let runs = properties::runs(properties::unique_list(run, *b"tdgp").unwrap()).unwrap();
    let mut angle = if animated {
        properties::unique_list(unique_run(&runs, "ADBE Opacity").unwrap(), *b"tdbs")
            .unwrap()
            .to_vec()
    } else {
        numeric(&[45.], None).children().unwrap().to_vec()
    };
    angle.retain(|chunk| chunk.id() != *b"tdsn");
    angle.push(name("Angle"));
    let mut controller = owner.clone();
    controller.name = "Scaler".into();
    controller.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Effect Parade"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Angle Control"),
                    list(
                        b"sspc",
                        vec![list(
                            b"tdgp",
                            vec![
                                name("Rotater_Cntrl"),
                                data(b"tdmn", b"ADBE Angle Control-0001"),
                                list(b"tdbs", angle),
                            ],
                        )],
                    ),
                ],
            ),
        ],
    )];
    owner.name = "Box".into();
    owner.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![data(b"tdmn", b"ADBE Rotate Z"), numeric(&[10.], Some(EXPR))],
            ),
        ],
    )];
    comp.layers = vec![owner.clone(), controller];
    (owner, *comp)
}

fn rotation(layer: &Layer) -> NumericProperty {
    properties::read_transform(&layer.content)
        .unwrap()
        .into_iter()
        .find(|p| p.match_name == "ADBE Rotate Z")
        .unwrap()
        .numeric
        .unwrap()
}

fn slider_position_fixture() -> (Layer, Composition) {
    fn rewrite(chunks: &mut [crate::rifx::Chunk]) {
        for chunk in chunks {
            let replacement = chunk.data_payload().and_then(|bytes| match &chunk.id() {
                b"tdmn" => match std::str::from_utf8(bytes).unwrap().trim_end_matches('\0') {
                    "ADBE Rotate Z" => Some(data(b"tdmn", b"ADBE Position_0")),
                    "ADBE Angle Control" => Some(data(b"tdmn", b"ADBE Slider Control")),
                    "ADBE Angle Control-0001" => Some(data(b"tdmn", b"ADBE Slider Control-0001")),
                    _ => None,
                },
                b"Utf8" => Some(data(
                    b"Utf8",
                    std::str::from_utf8(bytes)
                        .unwrap()
                        .replace("transform.rotation", "transform.xPosition")
                        .replace("\"Angle\"", "\"Slider\"")
                        .into_bytes(),
                )),
                b"tdsn" if bytes.ends_with(b"Angle") => Some(name("Slider")),
                _ => None,
            });
            if let Some(replacement) = replacement {
                *chunk = replacement;
            }
            if let Some(children) = chunk.children_mut() {
                rewrite(children);
            }
        }
    }
    let (mut layer, mut comp) = fixture(true);
    rewrite(&mut layer.content);
    rewrite(&mut comp.layers[1].content);
    let mut position = numeric(&[0., 0.], None);
    position.children_mut().unwrap()[1] = data(b"tdsb", [0, 0, 8, 1]);
    layer.content[0].children_mut().unwrap()[1]
        .children_mut()
        .unwrap()
        .extend([data(b"tdmn", b"ADBE Position"), position]);
    (layer, comp)
}

#[test]
fn slider_offset_restores_separated_position_and_parent_animation_in_pixels() {
    let (layer, comp) = slider_position_fixture();
    let (properties, _) = control_links::read_layer_transform(&layer, &comp).unwrap();
    let value = properties
        .iter()
        .find(|p| p.match_name == "ADBE Position_0")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert!(
        !value.expression_enabled,
        "Slider offset was left as an unsupported expression"
    );
    let (rotation_layer, rotation_comp) = fixture(true);
    let expected = lower(&rotation_layer, &rotation_comp, &rotation(&rotation_layer)).unwrap();
    assert_eq!(
        value.keyframes, expected.keyframes,
        "Slider pixels must not be converted to Scale percentages"
    );
    let id = LayerId::new(88);
    let (owner, _) = animation::transform_entries(
        &layer,
        &comp,
        id,
        animation::AnimationTargetClock::ParentIdentity,
        [1.; 2],
        &mut AnimationBudget::default(),
    );
    let (parent, _) = animation::transform_parent_entries(
        &layer,
        &comp,
        id,
        [1.; 2],
        &mut AnimationBudget::default(),
    );
    assert_eq!(owner.len(), 1);
    assert_eq!(owner, parent);
    assert_eq!(
        owner[0].target,
        fx_schema::PropertyTarget::layer(id, fx_schema::PropType::PositionX)
    );
}

#[test]
fn direct_signed_slider_restores_separated_position_without_adding_authored_base() {
    fn replace_expression(chunks: &mut [crate::rifx::Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"Utf8"
                && chunk.data_payload().is_some_and(|value| {
                    value
                        .windows(b"transform.xPosition".len())
                        .any(|window| window == b"transform.xPosition")
                })
            {
                *chunk = data(
                    b"Utf8",
                    b"-thisComp.layer(\"Scaler\").effect(\"Rotater_Cntrl\")(\"Slider\")".to_vec(),
                );
                return;
            }
            if let Some(children) = chunk.children_mut() {
                replace_expression(children);
            }
        }
    }

    let (mut layer, comp) = slider_position_fixture();
    replace_expression(&mut layer.content);
    let (properties, warnings) = control_links::read_layer_transform(&layer, &comp).unwrap();
    let value = properties
        .iter()
        .find(|property| property.match_name == "ADBE Position_0")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    let (rotation_layer, rotation_comp) = fixture(true);
    let offset = lower(&rotation_layer, &rotation_comp, &rotation(&rotation_layer)).unwrap();
    assert_eq!(value.keyframes.len(), offset.keyframes.len());
    for (direct, with_base) in value.keyframes.iter().zip(&offset.keyframes) {
        assert_eq!(direct.time_secs, with_base.time_secs);
        assert_eq!(direct.values[0], with_base.values[0] - 10.0);
        assert_eq!(direct.in_speed, with_base.in_speed);
        assert_eq!(direct.out_speed, with_base.out_speed);
        assert_eq!(direct.in_influence, with_base.in_influence);
        assert_eq!(direct.out_interpolation, with_base.out_interpolation);
    }
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("direct Slider reference"))
    );
}

#[test]
fn angle_binding_grammar_rejects_execution_and_wrong_variables() {
    assert_eq!(parse(EXPR).unwrap().sign, -1.);
    assert_eq!(
        parse(&format!("var {}", EXPR.replace("rotation-", "rotation+")))
            .unwrap()
            .sign,
        1.
    );
    for expr in [
        format!("{EXPR} execute();"),
        EXPR.replace("-rot;", "-other;"),
        EXPR.replace("rotation-rot", "rotation-rot*time"),
        EXPR.replace("thisComp.layer", "otherComp.layer"),
    ] {
        assert!(parse(&expr).is_none(), "{expr}");
    }
}

#[test]
fn angle_binding_lowers_static_base_without_changing_raw_reader() {
    let (layer, comp) = fixture(false);
    let base = rotation(&layer);
    assert!(base.expression_enabled);
    let resolved = lower(&layer, &comp, &base).unwrap();
    assert_eq!(resolved.values, vec![-35.]);
    assert!(!resolved.expression_enabled);
    assert_eq!(resolved.value_kind, NumericValueKind::Continuous);
    let (t, _) = transform::static_transform(&layer, [1920, 1080], &comp);
    assert_eq!(t.rotation, -35.);
}

#[test]
fn angle_binding_keeps_timing_easing_and_signed_speeds_in_all_transform_paths() {
    let (layer, comp) = fixture(true);
    let base = rotation(&layer);
    let original = lower(&layer, &comp, &base).unwrap();
    assert!(!original.keyframes.is_empty());
    let mut native = original.clone();
    native.expression_enabled = false;
    native.value_kind = NumericValueKind::Integer;
    for key in &mut native.keyframes {
        key.in_speed = vec![12.];
        key.out_speed = vec![24.];
    }
    let mapped = offset_curve(native.clone(), &base, -1.).unwrap();
    for (a, b) in native.keyframes.iter().zip(&mapped.keyframes) {
        assert_eq!(a.time_secs, b.time_secs);
        assert_eq!(b.values[0], 10. - a.values[0]);
        assert_eq!(b.in_speed, vec![-12.]);
        assert_eq!(b.out_speed, vec![-24.]);
        assert_eq!(a.in_influence, b.in_influence);
        assert_eq!(a.out_interpolation, b.out_interpolation);
    }
    let id = LayerId::new(77);
    let (owner, _) = animation::transform_entries(
        &layer,
        &comp,
        id,
        animation::AnimationTargetClock::ParentIdentity,
        [1.; 2],
        &mut AnimationBudget::default(),
    );
    let (parent, _) = animation::transform_parent_entries(
        &layer,
        &comp,
        id,
        [1.; 2],
        &mut AnimationBudget::default(),
    );
    let (camera, _) = camera_normalization::corrected_transform_entries(
        &layer,
        &comp,
        id,
        camera_normalization::LayerCorrection {
            position: Some([10., 20.]),
            anchor: None,
        },
        [1.; 2],
        &mut AnimationBudget::default(),
    );
    assert_eq!(owner.len(), 1);
    assert_eq!(owner, parent);
    assert_eq!(owner, camera);
    let (properties, _) = control_links::read_layer_transform(&layer, &comp).unwrap();
    assert!(
        !properties
            .into_iter()
            .find(|p| p.match_name == "ADBE Rotate Z")
            .unwrap()
            .numeric
            .unwrap()
            .expression_enabled
    );
}

#[test]
fn angle_binding_rebases_controller_keys_to_owner_clock() {
    let (mut layer, comp) = fixture(true);
    let before = lower(&layer, &comp, &rotation(&layer)).unwrap();
    let mut bytes = layer.record.raw_bytes().to_vec();
    bytes[12..16].copy_from_slice(&11_i32.to_be_bytes());
    bytes[16..20].copy_from_slice(&24_u32.to_be_bytes());
    layer.record = LayerRecord::decode(&bytes).unwrap();
    let after = lower(&layer, &comp, &rotation(&layer)).unwrap();
    for (a, b) in before.keyframes.iter().zip(&after.keyframes) {
        let source_time = comp.layers[1].record.start_time().unwrap()
            + a.time_secs * comp.layers[1].record.stretch().unwrap();
        let owner_time =
            layer.record.start_time().unwrap() + b.time_secs * layer.record.stretch().unwrap();
        assert!((source_time - owner_time).abs() < 1e-10);
        assert_eq!(a.values, b.values);
        assert_eq!(a.in_speed, b.in_speed);
    }
}

#[test]
fn angle_binding_rejects_ambiguous_layers_clock_changes_and_animated_base() {
    let (layer, mut comp) = fixture(false);
    let base = rotation(&layer);
    comp.layers.push(comp.layers[1].clone());
    assert!(lower(&layer, &comp, &base).is_err());
    comp.layers.pop();
    let mut bytes = comp.layers[1].record.raw_bytes().to_vec();
    bytes[8..12].copy_from_slice(&99_i32.to_be_bytes());
    bytes[108..112].copy_from_slice(&1_u32.to_be_bytes());
    comp.layers[1].record = LayerRecord::decode(&bytes).unwrap();
    assert!(lower(&layer, &comp, &base).is_err());
    let mut animated_base = base.clone();
    animated_base.animated = true;
    let mut curve = base;
    curve.expression_enabled = false;
    assert!(offset_curve(curve, &animated_base, 1.).is_err());
}
