use super::super::tests::{data, list, numeric};
use super::*;

#[test]
fn native_hold_endpoint_flags_are_admitted_without_weakening_curve_guards() {
    let curve = super::super::tests::native_hold_endpoint_curve("control-0");
    validate_raw_rotation(&curve).unwrap();
    let mut invalid = curve.clone();
    invalid.keyframes[1].in_interpolation = 4;
    assert!(validate_raw_rotation(&invalid).is_err());
    invalid = curve;
    invalid.keyframes[0].out_interpolation = 2;
    invalid.keyframes[1].in_interpolation = 3;
    assert!(validate_raw_rotation(&invalid).is_err());
}
use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
};

const EXPRESSION: &str = r#"90 + thisComp.layer("Angle_Box_01").transform.rotation"#;

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

fn with_stretch(record: &LayerRecord, numerator: i32, denominator: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[8..12].copy_from_slice(&numerator.to_be_bytes());
    bytes[108..112].copy_from_slice(&denominator.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn rotation(layer: &Layer) -> NumericProperty {
    properties::read_transform(&layer.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == "ADBE Rotate Z")
        .unwrap()
        .numeric
        .unwrap()
}

fn fixture(animated: bool) -> (Layer, Composition) {
    fixture_with_expression(animated, EXPRESSION)
}

fn fixture_with_expression(animated: bool, expression: &str) -> (Layer, Composition) {
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
    let template = composition.layers[0].clone();
    let source_numeric = if animated {
        let root = properties::root_runs(&template.content).unwrap();
        let transform = unique_run(&root, "ADBE Transform Group").unwrap();
        let leaves =
            properties::runs(properties::unique_list(transform, *b"tdgp").unwrap()).unwrap();
        let opacity =
            properties::unique_list(unique_run(&leaves, "ADBE Opacity").unwrap(), *b"tdbs")
                .unwrap()
                .to_vec();
        list(b"tdbs", opacity)
    } else {
        numeric(&[90.0], None)
    };

    let mut owner = template.clone();
    owner.name = "Angle_Box_02".into();
    owner.record = record_with_id(&owner.record, 392);
    owner.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Rotate Z"),
                    numeric(&[-55.0], Some(expression)),
                ],
            ),
        ],
    )];

    let mut source = template;
    source.name = "Angle_Box_01".into();
    source.record = record_with_id(&source.record, 391);
    source.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![data(b"tdmn", b"ADBE Rotate Z"), source_numeric],
            ),
        ],
    )];
    composition.layers = vec![owner.clone(), source];
    (owner, composition)
}

#[test]
fn grammar_accepts_only_a_finite_constant_plus_direct_sibling_rotation() {
    assert_eq!(parse(EXPRESSION).unwrap().constant, 90.0);
    assert_eq!(
        parse(" -9.0e1 + thisComp.layer('left').transform.rotation; ").unwrap(),
        Link {
            constant: -90.0,
            layer: "left",
            sign: 1.0,
        }
    );
    assert_eq!(
        parse("thisComp.layer('left').transform.rotation + 180").unwrap(),
        Link {
            constant: 180.0,
            layer: "left",
            sign: 1.0,
        }
    );
    assert_eq!(
        parse("thisComp.layer('left').transform.rotation + -90").unwrap(),
        Link {
            constant: -90.0,
            layer: "left",
            sign: 1.0
        }
    );
    assert!(parse("-thisComp.layer('left').transform.rotation - 90").is_some());
    assert!(parse("-thisComp.layer('left').transform.rotation + 90").is_some());
    for expression in [
        "Infinity + thisComp.layer('left').transform.rotation",
        "1e309 + thisComp.layer('left').transform.rotation",
        "90 - thisComp.layer('left').transform.rotation",
        "90 + thisComp.layer('left').transform.xRotation",
        "90 + thisComp.layer('left').transform.rotation.valueAtTime(time)",
        "90 + thisComp.layer('left').transform.rotation + 1",
        "45 + 45 + thisComp.layer('left').transform.rotation",
        "thisComp.layer('left').transform.rotation + 90 + 1",
        "--thisComp.layer('left').transform.rotation - 90",
        "+thisComp.layer('left').transform.rotation - 90",
        "-thisComp.layer('left').transform.rotation",
        "-thisComp.layer('left').transform.rotation * 90",
        "-thisComp.layer('left').transform.rotation - Infinity",
        "-thisComp.layer('left').transform.rotation - 1e309",
        "-thisComp.layer('left').transform.rotation - -90",
        "-thisComp.layer('left').transform.rotation - 90 + 1",
        "90 + -thisComp.layer('left').transform.rotation",
        "90 + thisComp.layer('left\\'over').transform.rotation",
        "90 + thisComp.layer('').transform.rotation",
        "90 + thisComp.layer('left').transform.rotation; execute()",
    ] {
        assert!(parse(expression).is_none(), "{expression}");
    }
}

#[test]
fn lower_adds_only_the_constant_and_preserves_native_easing_and_signed_speeds() {
    let (owner, composition) = fixture(false);
    let mut lowered = rotation(&owner);
    lower(&owner, &composition, &mut lowered).unwrap().unwrap();
    assert_eq!(lowered.values, vec![180.0]);
    assert!(!lowered.expression_enabled);
    assert!(!lowered.expression_present);

    let (owner, composition) = fixture(true);
    let source = rotation(&composition.layers[1]);
    let mut expected = source.clone();
    for value in expected.values.iter_mut().chain(
        expected
            .keyframes
            .iter_mut()
            .flat_map(|key| key.values.iter_mut()),
    ) {
        *value += 90.0;
    }
    expected.value_kind = NumericValueKind::Continuous;
    expected.expression_enabled = false;
    expected.expression_present = false;
    let mut lowered = rotation(&owner);
    lower(&owner, &composition, &mut lowered).unwrap().unwrap();
    assert_eq!(lowered, expected);
    // The Adobe fixture has zero temporal speeds. Exercise nonzero signed
    // metadata separately through the same production offset operation.
    let mut signed = source.clone();
    for key in &mut signed.keyframes {
        key.in_speed = vec![-12.0];
        key.out_speed = vec![24.0];
    }
    let original = signed.clone();
    offset_curve(&mut signed, 1.0, 90.0, 0.5).unwrap();
    for (before, after) in original.keyframes.iter().zip(&signed.keyframes) {
        assert_eq!(after.in_speed, before.in_speed);
        assert_eq!(after.out_speed, before.out_speed);
        assert_eq!(after.in_influence, before.in_influence);
        assert_eq!(after.out_influence, before.out_influence);
        assert_eq!(after.time_secs, before.time_secs + 0.5);
        assert_eq!(after.values[0], before.values[0] + 90.0);
    }
}

#[test]
fn signed_reference_with_constant_counterrotates_the_sibling_curve() {
    let (owner, composition) = fixture_with_expression(
        true,
        "-thisComp.layer(\"Angle_Box_01\").transform.rotation - 90",
    );
    let source = rotation(&composition.layers[1]);
    let mut lowered = rotation(&owner);
    lower(&owner, &composition, &mut lowered).unwrap().unwrap();

    assert_eq!(source.keyframes.len(), lowered.keyframes.len());
    for (raw, mapped) in source.keyframes.iter().zip(&lowered.keyframes) {
        assert_eq!(mapped.values, vec![-raw.values[0] - 90.0]);
        assert_eq!(
            mapped.in_speed,
            raw.in_speed.iter().map(|speed| -speed).collect::<Vec<_>>()
        );
        assert_eq!(
            mapped.out_speed,
            raw.out_speed.iter().map(|speed| -speed).collect::<Vec<_>>()
        );
        assert_eq!(mapped.in_influence, raw.in_influence);
        assert_eq!(mapped.out_influence, raw.out_influence);
    }

    let mut nonzero_speeds = source.clone();
    for key in &mut nonzero_speeds.keyframes {
        key.in_speed = vec![-12.0];
        key.out_speed = vec![24.0];
    }
    let original = nonzero_speeds.clone();
    offset_curve(&mut nonzero_speeds, -1.0, 90.0, 0.0).unwrap();
    for (raw, mapped) in original.keyframes.iter().zip(&nonzero_speeds.keyframes) {
        assert_eq!(mapped.values, vec![-raw.values[0] + 90.0]);
        assert_eq!(mapped.in_speed, vec![12.0]);
        assert_eq!(mapped.out_speed, vec![-24.0]);
        assert_eq!(mapped.in_influence, raw.in_influence);
        assert_eq!(mapped.out_influence, raw.out_influence);
    }
    assert!(!lowered.expression_enabled);
    assert!(!lowered.expression_present);
}

#[test]
fn lower_uses_native_zero_when_sibling_omits_rotation() {
    let (owner, mut composition) = fixture(false);
    composition.layers[1].content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(b"tdgp", Vec::new()),
        ],
    )];
    let mut lowered = rotation(&owner);
    lower(&owner, &composition, &mut lowered).unwrap().unwrap();
    assert_eq!(lowered.values, vec![90.0]);
    assert!(!lowered.animated);
    assert!(lowered.keyframes.is_empty());
    assert!(!lowered.expression_enabled);
}

#[test]
fn malformed_sibling_transform_group_is_not_a_missing_rotation_default() {
    let (owner, mut composition) = fixture(false);
    composition.layers[1].content =
        vec![list(b"tdgp", vec![data(b"tdmn", b"ADBE Transform Group")])];
    let mut lowered = rotation(&owner);
    assert!(lower(&owner, &composition, &mut lowered).unwrap().is_err());
    assert!(lowered.expression_enabled);
}

#[test]
fn lower_rebases_equal_positive_clocks_without_changing_curve_duration() {
    let (mut owner, composition) = fixture(true);
    owner.record = with_start_time(&owner.record, 11, 24);
    let source = rotation(&composition.layers[1]);
    let mut lowered = rotation(&owner);
    lower(&owner, &composition, &mut lowered).unwrap().unwrap();
    assert_eq!(source.keyframes.len(), lowered.keyframes.len());
    for (raw, mapped) in source.keyframes.iter().zip(&lowered.keyframes) {
        let source_time = composition.layers[1].record.start_time().unwrap()
            + raw.time_secs * composition.layers[1].record.stretch().unwrap();
        let owner_time =
            owner.record.start_time().unwrap() + mapped.time_secs * owner.record.stretch().unwrap();
        assert!((source_time - owner_time).abs() < 1e-10);
        assert_eq!(mapped.values[0], raw.values[0] + 90.0);
        assert_eq!(mapped.in_speed, raw.in_speed);
        assert_eq!(mapped.out_speed, raw.out_speed);
        assert_eq!(mapped.in_influence, raw.in_influence);
        assert_eq!(mapped.out_influence, raw.out_influence);
        assert_eq!(mapped.in_interpolation, raw.in_interpolation);
        assert_eq!(mapped.out_interpolation, raw.out_interpolation);
    }
    let raw_duration =
        source.keyframes.last().unwrap().time_secs - source.keyframes.first().unwrap().time_secs;
    let mapped_duration =
        lowered.keyframes.last().unwrap().time_secs - lowered.keyframes.first().unwrap().time_secs;
    assert_eq!(mapped_duration, raw_duration);
}

#[test]
fn lower_rejects_ambiguous_recursive_expression_and_unsafe_clocks_atomically() {
    let (owner, mut composition) = fixture(true);
    let original = rotation(&owner);

    composition.layers.push(composition.layers[1].clone());
    let mut property = original.clone();
    assert!(lower(&owner, &composition, &mut property).unwrap().is_err());
    assert_eq!(property, original);
    composition.layers.pop();

    let mut recursive_owner = owner.clone();
    recursive_owner.name = "Angle_Box_01".into();
    let mut recursive_composition = composition.clone();
    recursive_composition.layers = vec![recursive_owner.clone()];
    let mut property = rotation(&recursive_owner);
    assert!(
        lower(&recursive_owner, &recursive_composition, &mut property)
            .unwrap()
            .is_err()
    );

    let mut expression_source = composition.clone();
    expression_source.layers[1].content = owner.content.clone();
    expression_source.layers[1].name = "Angle_Box_01".into();
    let mut property = original.clone();
    assert!(
        lower(&owner, &expression_source, &mut property)
            .unwrap()
            .is_err()
    );
    assert_eq!(property, original);

    let mut reversed = composition.clone();
    reversed.layers[1].record = with_stretch(&reversed.layers[1].record, -1, 1);
    let mut property = original.clone();
    assert!(lower(&owner, &reversed, &mut property).unwrap().is_err());
    assert_eq!(property, original);

    let mut unequal = composition;
    unequal.layers[1].record = with_stretch(&unequal.layers[1].record, 2, 1);
    let mut property = original.clone();
    assert!(lower(&owner, &unequal, &mut property).unwrap().is_err());
    assert_eq!(property, original);
}

#[test]
fn recognized_sibling_rotation_failure_keeps_its_actionable_warning() {
    let (owner, mut composition) = fixture(true);
    composition.layers.push(composition.layers[1].clone());

    let (_, warnings) = super::super::read_layer_transform(&owner, &composition).unwrap();
    let warning = warnings
        .iter()
        .find(|warning| {
            warning.starts_with("ADBE Rotate Z: sibling Rotation offset link not lowered")
        })
        .expect("recognized sibling Rotation warning");
    assert!(warning.contains("ambiguous sibling Rotation layer"));
    assert!(!warning.contains("not a direct scalar offset binding"));
}

#[test]
fn raw_rotation_validation_rejects_malformed_key_metadata() {
    let (_, composition) = fixture(true);
    let source = rotation(&composition.layers[1]);
    let mut malformed = source.clone();
    malformed.keyframes[0].values[0] = f64::NAN;
    assert!(validate_raw_rotation(&malformed).is_err());
    malformed = source.clone();
    malformed.keyframes[0].in_speed[0] = f64::INFINITY;
    assert!(validate_raw_rotation(&malformed).is_err());
    malformed = source.clone();
    malformed.keyframes[0].out_influence[0] = 100.1;
    assert!(validate_raw_rotation(&malformed).is_err());
    malformed = source.clone();
    malformed.keyframes[0].in_influence[0] = -0.1;
    assert!(validate_raw_rotation(&malformed).is_err());
    malformed = source.clone();
    malformed.keyframes[0].out_interpolation = 4;
    assert!(validate_raw_rotation(&malformed).is_err());
    malformed = source.clone();
    // Incoming Hold is still unsupported when the outgoing segment is curved.
    malformed.keyframes[0].out_interpolation = 2;
    malformed.keyframes[1].in_interpolation = 3;
    assert!(validate_raw_rotation(&malformed).is_err());
    malformed = source.clone();
    malformed.keyframes[0].spatial_in = vec![0.0];
    assert!(validate_raw_rotation(&malformed).is_err());
    malformed = source;
    malformed.keyframes[1].time_secs = malformed.keyframes[0].time_secs;
    assert!(validate_raw_rotation(&malformed).is_err());
}
