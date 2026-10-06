use super::super::tests::{data, list, name, numeric};
use super::*;
use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
    structure_document::control_links,
};

const EXPRESSION: &str = r#"
var p = thisComp.layer("Master").transform.position; // source controller
var fps = thisComp.frameDuration;
var s = timeRemap;
var d = thisComp.layer("Master").effect("delay")("Slider") * fps;
var amnt = easeOut(time, s.key(1).time, s.key(2).time, 1, 0);
p.valueAtTime(time - amnt * d);
"#;

fn fixture_layer(file: &[u8]) -> (Layer, Composition) {
    let project = read_project(file).unwrap();
    let composition = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some(composition.clone()),
            _ => None,
        })
        .unwrap();
    (composition.layers[0].clone(), *composition)
}

fn numeric_children(layer: &Layer, property: &str) -> Vec<crate::rifx::Chunk> {
    let root = properties::root_runs(&layer.content).unwrap();
    let transform = unique_run(&root, "ADBE Transform Group").unwrap();
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").unwrap()).unwrap();
    properties::unique_list(unique_run(&leaves, property).unwrap(), *b"tdbs")
        .unwrap()
        .to_vec()
}

fn record_with_id(record: &LayerRecord, id: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[..4].copy_from_slice(&id.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn delayed_fixture() -> (Layer, Composition) {
    let (position_layer, mut composition) = fixture_layer(include_bytes!(
        "../../../../tests/fixtures/properties/property_2D_position.aep"
    ));
    let (opacity_layer, _) = fixture_layer(include_bytes!(
        "../../../../tests/fixtures/properties/property_1D_opacity.aep"
    ));
    let remap = numeric_children(&opacity_layer, "ADBE Opacity");
    let position = numeric_children(&position_layer, "ADBE Position");

    let mut owner = position_layer.clone();
    owner.name = "Circle".into();
    owner.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Time Remapping"),
            list(b"tdbs", remap),
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Position"),
                    numeric(&[368.0, 216.0, 0.0], Some(EXPRESSION)),
                ],
            ),
        ],
    )];

    let mut controller = position_layer;
    controller.name = "Master".into();
    controller.record = record_with_id(&controller.record, 99);
    controller.content = vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Effect Parade"),
            list(
                b"tdgp",
                vec![
                    data(b"tdmn", b"ADBE Slider Control"),
                    list(
                        b"sspc",
                        vec![list(
                            b"tdgp",
                            vec![
                                name("delay"),
                                data(b"tdmn", b"ADBE Slider Control-0001"),
                                numeric(&[5.0], None),
                            ],
                        )],
                    ),
                ],
            ),
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![data(b"tdmn", b"ADBE Position"), list(b"tdbs", position)],
            ),
        ],
    )];
    composition.layers = vec![owner.clone(), controller];
    (owner, composition)
}

#[test]
fn hold_curve_selects_new_value_at_exact_key_time() {
    let (_, composition) = delayed_fixture();
    let mut source = controller_position(&composition.layers[1]).unwrap();
    source.keyframes.truncate(2);
    source.keyframes[0].time_secs = 0.0;
    source.keyframes[1].time_secs = 1.0;
    source.keyframes[0].out_interpolation = 3;
    assert_eq!(
        evaluate_position(&source, 0.999, 0.0, 1.0).unwrap(),
        source.keyframes[0].values
    );
    assert_eq!(
        evaluate_position(&source, 1.0, 0.0, 1.0).unwrap(),
        source.keyframes[1].values
    );
    let curve =
        fit_scalar_curve::<PropertyError>(1000, 0.01, |t| Ok(if t < 1000 { 1.0 } else { 10.0 }))
            .unwrap();
    assert_eq!(fitted_value(&curve, 1000, &mut 0), 10.0);
}

#[test]
fn duration_and_output_key_counts_above_legacy_caps_are_accepted() {
    let long = fit_curve(60_001, |time| Ok(time as f64)).unwrap();
    assert_eq!(long.keys.last().unwrap().offset_ms, 60_001);

    let dense = fit_curve(256, |time| Ok((time % 2) as f64)).unwrap();
    assert!(dense.keys.len() > 128);
}

#[test]
fn exact_delayed_master_family_restores_source_position_with_sparse_editable_keys() {
    let (owner, composition) = delayed_fixture();
    let raw = properties::read_transform(&owner.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .unwrap();
    assert_eq!(raw.values, [368.0, 216.0, 0.0]);
    assert!(raw.expression_enabled);

    let (properties, warnings) = control_links::read_layer_transform(&owner, &composition).unwrap();
    let lowered = properties
        .into_iter()
        .find(|property| property.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .unwrap();
    assert!(!lowered.expression_enabled);
    assert!(lowered.animated);
    assert!(
        lowered.keyframes.len() < 30,
        "one key per frame was generated"
    );
    assert_eq!(lowered.keyframes[0].values, [0.0, 0.0, 0.0]);
    assert_eq!(
        lowered.keyframes.last().unwrap().values,
        [100.0, 100.0, 0.0]
    );
    assert!(warnings.iter().any(|warning| {
        warning.contains("delayed Master rig approximated") && warning.contains("0.01 pixel")
    }));
}

#[test]
fn recognizer_rejects_executable_suffix_and_ambiguous_controller() {
    assert_eq!(
        normalized_expression(EXPRESSION).as_deref(),
        Some(CANONICAL_EXPRESSION)
    );
    assert_ne!(
        normalized_expression(&format!("{EXPRESSION}value;")).as_deref(),
        Some(CANONICAL_EXPRESSION)
    );

    let (owner, mut composition) = delayed_fixture();
    composition.layers.push(composition.layers[1].clone());
    let (properties, warnings) = control_links::read_layer_transform(&owner, &composition).unwrap();
    let position = properties
        .into_iter()
        .find(|property| property.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .unwrap();
    assert!(position.expression_enabled);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("ambiguous delayed Position controller"))
    );
}

#[test]
fn native_controller_keys_use_controller_start_and_stretch_and_keep_constant_x() {
    let source = NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![
            NumericKeyframe {
                time_secs: 0.0,
                values: vec![1920.0, 800.0],
                in_interpolation: 1,
                out_interpolation: 1,
                in_speed: Vec::new(),
                in_influence: Vec::new(),
                out_speed: Vec::new(),
                out_influence: Vec::new(),
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            },
            NumericKeyframe {
                time_secs: 1.0,
                values: vec![1920.0, 1000.0],
                in_interpolation: 1,
                out_interpolation: 1,
                in_speed: Vec::new(),
                in_influence: Vec::new(),
                out_speed: Vec::new(),
                out_influence: Vec::new(),
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            },
        ],
        value_kind: NumericValueKind::Continuous,
    };
    assert_eq!(
        evaluate_position(&source, 3.0, 2.0, 2.0).unwrap(),
        [1920.0, 900.0]
    );
}

#[test]
fn descending_spatial_segment_uses_unsigned_path_speed() {
    let from = NumericKeyframe {
        time_secs: 0.0,
        values: vec![1920.0, 1100.0],
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![0.0],
        in_influence: vec![33.333_333],
        out_speed: vec![0.0],
        out_influence: vec![33.333_333],
        spatial_in: vec![0.0, 0.0],
        spatial_out: vec![0.0, 0.0],
    };
    let mut to = from.clone();
    to.time_secs = 1.0;
    to.values = vec![1920.0, 800.0];
    to.in_speed = vec![600.0];

    let progress = native_progress(&from, &to, 1, 1.0, 0.5).unwrap();
    assert!((0.0..=1.0).contains(&progress));
    assert_eq!(native_progress(&from, &to, 0, 1.0, 0.5).unwrap(), progress);
}

#[test]
fn nonzero_spatial_tangents_and_equal_endpoint_speed_are_rejected() {
    let mut source = NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![
            NumericKeyframe {
                time_secs: 0.0,
                values: vec![1920.0, 800.0],
                in_interpolation: 2,
                out_interpolation: 2,
                in_speed: vec![1.0],
                in_influence: vec![33.0],
                out_speed: vec![1.0],
                out_influence: vec![33.0],
                spatial_in: vec![0.0, 0.0],
                spatial_out: vec![0.0, 0.0],
            },
            NumericKeyframe {
                time_secs: 1.0,
                values: vec![1920.0, 800.0],
                in_interpolation: 2,
                out_interpolation: 2,
                in_speed: vec![1.0],
                in_influence: vec![33.0],
                out_speed: vec![1.0],
                out_influence: vec![33.0],
                spatial_in: vec![0.0, 0.0],
                spatial_out: vec![0.0, 0.0],
            },
        ],
        value_kind: NumericValueKind::Continuous,
    };
    assert!(native_progress(&source.keyframes[0], &source.keyframes[1], 1, 1.0, 0.5).is_err());
    source.keyframes[0].spatial_out[0] = 1.0;
    assert!(validate_source(&source, 2).is_err());
}

#[test]
fn ease_out_follows_ae_hermite_measured_values() {
    // AE 26.5 `easeOut(t, 0, 1, 0, 1)` readback; the previous Bezier
    // (0.167,0.167,0.667,1) approximation gave 0.637 at 0.5.
    for (progress, expected) in [
        (0.1, 0.109),
        (0.2, 0.232),
        (0.3, 0.363),
        (0.4, 0.496),
        (0.5, 0.625),
    ] {
        let actual = super::ease_out(progress, 0.0, 1.0, 0.0, 1.0);
        assert!(
            (actual - expected).abs() < 5e-4,
            "{progress}: {actual} != {expected}"
        );
    }
    assert_eq!(super::ease_out(-1.0, 0.0, 1.0, 2.0, 4.0), 2.0);
    assert_eq!(super::ease_out(2.0, 0.0, 1.0, 2.0, 4.0), 4.0);
}
