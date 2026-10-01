use super::super::tests::{data, list, numeric};
use super::*;
use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
    structure_document::control_links,
};

const EXPRESSION: &str = r#"
s = [];
ps = parent.transform.scale.value;
for (i = 0; i < ps.length; i++) {
    s[i] = value[i] * 100 / ps[i];
}
s
"#;
const FIRST_PARENT_PERCENT: f64 = 0.001;
const FIRST_PARENT_FRACTION: f64 = FIRST_PARENT_PERCENT / 100.0;

fn scalar_property(keys: &[(f64, f64)]) -> NumericProperty {
    NumericProperty {
        values: Vec::new(),
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: keys
            .iter()
            .map(|(time_secs, value)| NumericKeyframe {
                time_secs: *time_secs,
                values: vec![*value],
                in_interpolation: 1,
                out_interpolation: 1,
                in_speed: Vec::new(),
                in_influence: Vec::new(),
                out_speed: Vec::new(),
                out_influence: Vec::new(),
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            })
            .collect(),
        value_kind: NumericValueKind::Continuous,
    }
}

fn set_segment_easing(property: &mut NumericProperty, from: usize, [x1, x2, y1, y2]: [f64; 4]) {
    let (before, after) = property.keyframes.split_at_mut(from + 1);
    let previous = &mut before[from];
    let current = &mut after[0];
    let duration = current.time_secs - previous.time_secs;
    let delta = current.values[0] - previous.values[0];
    previous.out_interpolation = 2;
    current.in_interpolation = 2;
    previous.out_influence = vec![x1 * 100.0];
    current.in_influence = vec![(1.0 - x2) * 100.0];
    previous.out_speed = vec![y1 * delta / (duration * x1)];
    current.in_speed = vec![(1.0 - y2) * delta / (duration * (1.0 - x2))];
}

fn intro_parent_curve() -> NumericProperty {
    let mut property = scalar_property(&[
        (2.0, FIRST_PARENT_FRACTION),
        (2.042, 0.65),
        (2.125, 0.893_535_533_879_408_8),
        (2.917, 1.000_001),
    ]);
    set_segment_easing(
        &mut property,
        0,
        [
            0.069_766_441_589_932_3,
            0.043_445_010_705_511_655,
            0.374_709_098_664_761_1,
            0.713_193_724_023_591_2,
        ],
    );
    set_segment_easing(
        &mut property,
        1,
        [
            0.216_247_590_253_400_48,
            0.532_959_438_856_692,
            0.346_102_167_571_835,
            0.861_451_594_411_009_4,
        ],
    );
    set_segment_easing(
        &mut property,
        2,
        [
            0.097_457_330_247_592_18,
            0.180_338_263_930_755_7,
            0.628_258_865_611_148_7,
            1.0,
        ],
    );
    property
}

fn axis_clock(duration: u64, active_start: f64, owner_start: f64, parent_start: f64) -> AxisClock {
    AxisClock {
        duration,
        active_start,
        owner_start,
        owner_stretch: 1.0,
        parent_start,
        parent_stretch: 1.0,
    }
}

fn first_fixture_layer() -> (Layer, Composition) {
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/properties/property_2D_position.aep"
    ))
    .unwrap();
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

fn record_with_identity(record: &LayerRecord, id: u32, parent_id: u32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[..4].copy_from_slice(&id.to_be_bytes());
    bytes[132..136].copy_from_slice(&parent_id.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn record_with_stretch(record: &LayerRecord, numerator: i32) -> LayerRecord {
    let mut bytes = record.encode();
    bytes[8..12].copy_from_slice(&numerator.to_be_bytes());
    LayerRecord::decode(&bytes).unwrap()
}

fn scale_content(values: &[f64], expression: Option<&str>) -> Vec<crate::rifx::Chunk> {
    vec![list(
        b"tdgp",
        vec![
            data(b"tdmn", b"ADBE Transform Group"),
            list(
                b"tdgp",
                vec![data(b"tdmn", b"ADBE Scale"), numeric(values, expression)],
            ),
        ],
    )]
}

fn static_fixture() -> (Layer, Composition) {
    let (template, mut composition) = first_fixture_layer();
    let mut owner = template.clone();
    owner.name = "Guide".into();
    owner.record = record_with_identity(&owner.record, 20, 36);
    owner.content = scale_content(&[1.0, 0.8, 1.0], Some(EXPRESSION));

    let mut parent = template;
    parent.name = "Scaler".into();
    parent.record = record_with_identity(&parent.record, 36, 0);
    parent.content = scale_content(&[0.5, 0.25, 1.0], None);
    composition.layers = vec![owner.clone(), parent];
    (owner, composition)
}

#[test]
fn exact_parent_scale_grammar_accepts_only_the_complete_stock_loop() {
    assert!(parse(EXPRESSION));
    assert!(parse(
        "s=[]\r\nps=parent.transform.scale.value\r\nfor(i=0;i<ps.length;i++){s[i]=value[i]*100/ps[i]}\r\ns;"
    ));
    for rejected in [
        "s=[];ps=thisLayer.transform.scale.value;for(i=0;i<ps.length;i++){s[i]=value[i]*100/ps[i];}s",
        "s=[];ps=parent.transform.scale.value;for(j=0;j<ps.length;j++){s[j]=value[j]*100/ps[j];}s",
        "s=[];ps=parent.transform.scale.value;for(i=0;i<ps.length;i++){s[i]=ps[i]*100/value[i];}s",
        "s=[];ps=parent.transform.scale.value;for(i=0;i<ps.length;i++){s[i]=value[i]*100/ps[i];}s;evil()",
        "var s=[];ps=parent.transform.scale.value;for(i=0;i<ps.length;i++){s[i]=value[i]*100/ps[i];}s",
    ] {
        assert!(!parse(rejected), "unexpectedly accepted: {rejected}");
    }
}

#[test]
fn durations_source_keys_partitions_and_output_keys_above_legacy_caps_are_accepted() {
    assert_eq!(bounded_duration(0.0, 60.001).unwrap(), 60_001);

    let keys = (0..=4_096)
        .map(|index| (f64::from(index) / 1_000.0, 1.0))
        .collect::<Vec<_>>();
    let mut source = scalar_property(&keys);
    for key in &mut source.keyframes {
        key.out_interpolation = 3;
    }
    validate_nonsingular_curve(&source).unwrap();

    let parent_samples = (0..=600)
        .map(|index| if index % 2 == 0 { 1.0 } else { 3.0 })
        .collect::<Vec<_>>();
    let boundaries = fitting_boundaries(&parent_samples).unwrap();
    assert!(boundaries.len() - 1 > 256);
    let curve = fit_weighted_reciprocal(&parent_samples, &|offset| {
        let index = usize::try_from(offset)
            .map_err(|_| PropertyError::Layout("test sample offset overflow"))?;
        reciprocal(1.0, parent_samples[index])
    })
    .unwrap();
    assert!(curve.keys.len() > 256);
}

#[test]
fn static_parent_cancellation_publishes_independent_axes_and_keeps_parent() {
    let (owner, composition) = static_fixture();
    let (properties, warnings) = control_links::read_layer_transform(&owner, &composition).unwrap();
    let combined = properties
        .iter()
        .find(|property| property.match_name == "ADBE Scale")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(combined.values, vec![1.0, 1.0]);
    assert!(!combined.expression_enabled && !combined.expression_present);
    let x = properties
        .iter()
        .find(|property| property.match_name == control_links::SCALE_X)
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    let y = properties
        .iter()
        .find(|property| property.match_name == control_links::SCALE_Y)
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert_eq!(x.values, vec![2.0]);
    assert_eq!(y.values, vec![3.2]);
    assert_eq!(composition.layers[1].record.id(), 36);
    assert!(warnings.iter().any(|warning| {
        warning.contains("parent-Scale cancellation")
            && warning.contains("parent transform remains intact")
            && warning.contains("0.01 percentage points")
    }));
}

#[test]
fn near_singular_native_ramp_is_sparse_and_effectively_exact_on_every_millisecond() {
    let parent = intro_parent_curve();
    validate_nonsingular_curve(&parent).unwrap();
    let duration = 11_042;
    let clock = axis_clock(duration, 0.0, 0.0, 0.0);
    let x = lower_axis(&parent, 1.0, clock).unwrap();
    let y = lower_axis(&parent, 0.8, clock).unwrap();
    assert!(x.keyframes.len() < usize::try_from(duration / 10).unwrap());
    assert!(y.keyframes.len() < usize::try_from(duration / 10).unwrap());

    // Native `0.001` is a percentage-point value. Numeric Scale storage uses
    // fractions, so the denominator is 0.00001 and the editable child is
    // 100,000× internally / 10,000,000% at the FX ScaleX/ScaleY boundary.
    assert_eq!(FIRST_PARENT_FRACTION, 0.000_01);
    let child_fraction = x.keyframes[0].values[0];
    assert!((child_fraction - 100_000.0).abs() < 1.0e-6);
    let fx_percent = child_fraction * 100.0;
    assert!((fx_percent - 10_000_000.0).abs() < 0.1);
    assert!(fx_percent.is_finite() && (fx_percent as f32).is_finite());
    for property in [fx_schema::PropType::ScaleX, fx_schema::PropType::ScaleY] {
        let bounds = property
            .static_write_bounds()
            .expect("Scale axes have the nonzero static-write contract");
        assert_eq!(bounds.max, None, "FX Scale has no magnitude clamp");
    }
    // FX transform evaluation stores percentage in f64, divides by 100 and
    // casts each affine component to f32. Both extreme child and tiny parent
    // remain finite and compose back to identity without a clamp.
    let child_affine_scale = (fx_percent / 100.0) as f32;
    let parent_affine_scale = (FIRST_PARENT_PERCENT / 100.0) as f32;
    assert!(child_affine_scale.is_finite() && parent_affine_scale.is_finite());
    assert!((child_affine_scale * parent_affine_scale - 1.0).abs() < 1.0e-6);

    for millisecond in 0..=duration {
        let time = millisecond as f64 / 1_000.0;
        let parent_value = evaluate_position(&parent, time, 0.0, 1.0).unwrap()[0];
        let x_value = evaluate_position(&x, time, 0.0, 1.0).unwrap()[0];
        let y_value = evaluate_position(&y, time, 0.0, 1.0).unwrap()[0];
        assert!((parent_value * x_value - 1.0).abs() <= EFFECTIVE_TOLERANCE + 1.0e-9);
        assert!((parent_value * y_value - 0.8).abs() <= EFFECTIVE_TOLERANCE + 1.0e-9);
    }
}

#[test]
fn hold_boundaries_and_different_owner_starts_keep_composition_time_alignment() {
    let mut parent = scalar_property(&[(0.5, 0.5), (0.75, 0.25), (1.0, 1.0)]);
    parent.keyframes[0].out_interpolation = 3;
    let output = lower_axis(&parent, 1.0, axis_clock(1_000, 1.5, 1.0, 1.0)).unwrap();
    for (composition_time, expected_parent) in [(1.749, 0.5), (1.75, 0.25), (2.0, 1.0)] {
        let child = evaluate_position(&output, composition_time, 1.0, 1.0).unwrap()[0];
        assert!((expected_parent * child - 1.0).abs() <= EFFECTIVE_TOLERANCE + 1.0e-9);
    }
}

#[test]
fn singular_overshooting_recursive_and_ambiguous_parents_are_rejected() {
    let mut crossing = scalar_property(&[(0.0, -0.5), (1.0, 0.5)]);
    assert!(validate_nonsingular_curve(&crossing).is_err());

    crossing.keyframes[0].values[0] = 1.0;
    crossing.keyframes[1].values[0] = 2.0;
    crossing.keyframes[0].out_interpolation = 2;
    crossing.keyframes[1].in_interpolation = 2;
    crossing.keyframes[0].out_influence = vec![50.0];
    crossing.keyframes[1].in_influence = vec![50.0];
    crossing.keyframes[0].out_speed = vec![-100.0];
    crossing.keyframes[1].in_speed = vec![0.0];
    assert!(validate_nonsingular_curve(&crossing).is_err());

    let (owner, mut composition) = static_fixture();
    composition.layers[1].content = scale_content(&[1.0, 1.0, 1.0], Some(EXPRESSION));
    let base = properties::read_transform(&owner.content).unwrap()[0]
        .numeric
        .as_ref()
        .unwrap()
        .clone();
    assert!(lower(&owner, &composition, &base).unwrap().is_err());

    let mut duplicate = composition.layers[1].clone();
    duplicate.content = scale_content(&[1.0, 1.0, 1.0], None);
    composition.layers.push(duplicate);
    assert!(lower(&owner, &composition, &base).unwrap().is_err());
}

#[test]
fn unequal_or_reversed_clocks_retain_the_expression_fallback() {
    let (owner, mut composition) = static_fixture();
    composition.layers[1].record = record_with_stretch(&composition.layers[1].record, 2);
    let (properties, warnings) = control_links::read_layer_transform(&owner, &composition).unwrap();
    let scale = properties
        .iter()
        .find(|property| property.match_name == "ADBE Scale")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert!(scale.expression_enabled);
    assert!(!properties.iter().any(|property| {
        matches!(
            property.match_name.as_str(),
            control_links::SCALE_X | control_links::SCALE_Y
        )
    }));
    assert!(warnings.iter().any(|warning| {
        warning.contains("parent Scale cancellation not lowered")
            && warning.contains("equal positive stretches")
    }));

    composition.layers[1].record = record_with_stretch(&composition.layers[1].record, -1);
    assert!(
        control_links::read_layer_transform(&owner, &composition)
            .unwrap()
            .1
            .iter()
            .any(|warning| warning.contains("invalid parent Scale source clock"))
    );
}

#[test]
#[ignore = "requires local licensed AEP_PARENT_SCALE_SOURCE, which cannot be redistributed"]
fn local_external_source_restores_comp3_guide_parent_scale_cancellation() {
    use sha2::{Digest, Sha256};

    let bytes = std::fs::read(
        std::env::var_os("AEP_PARENT_SCALE_SOURCE").expect("local licensed source path"),
    )
    .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = read_project(&bytes).unwrap();
    let ItemKind::Composition(composition) = &project.item(3).unwrap().kind else {
        panic!("composition 3")
    };
    let parent = composition
        .layers
        .iter()
        .find(|layer| layer.record.id() == 36)
        .unwrap();
    let parent_axes = resolved_parent_axes(parent, composition).unwrap();
    let (parent_start, parent_stretch) = source_clock(parent).unwrap();

    for id in [20, 32, 33, 34] {
        let owner = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == id)
            .unwrap();
        assert_eq!(owner.record.parent_id(), 36);
        let raw_scale = properties::read_transform(&owner.content)
            .unwrap()
            .into_iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .unwrap();
        let (properties, warnings) =
            control_links::read_layer_transform(owner, composition).unwrap();
        let combined = properties
            .iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(combined.values, vec![1.0, 1.0], "layer {id}: {warnings:?}");
        assert!(!combined.expression_enabled && !combined.expression_present);
        assert!(
            properties
                .iter()
                .any(|property| property.match_name == "ADBE Position")
        );
        assert!(
            properties
                .iter()
                .any(|property| property.match_name == "ADBE Rotate Z")
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("parent-Scale cancellation"))
        );

        let axes = [control_links::SCALE_X, control_links::SCALE_Y].map(|name| {
            properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
        });
        let (owner_start, owner_stretch) = source_clock(owner).unwrap();
        for local_milliseconds in [2_000_u64, 2_042, 2_125, 2_917] {
            // These are composition times from the reported frames, not the
            // guide's source-local clock (which starts at -22/24 seconds).
            let composition_time = local_milliseconds as f64 / 1_000.0;
            for axis in 0..2 {
                let parent_value = evaluate_position(
                    &parent_axes[axis],
                    composition_time,
                    parent_start,
                    parent_stretch,
                )
                .unwrap()[0];
                let child_value =
                    evaluate_position(axes[axis], composition_time, owner_start, owner_stretch)
                        .unwrap()[0];
                assert!(
                    (parent_value * child_value - raw_scale.values[axis]).abs()
                        <= EFFECTIVE_TOLERANCE + 1.0e-9,
                    "layer {id}, axis {axis}, {local_milliseconds}ms: parent={parent_value} child={child_value} expected={} owner_start={owner_start} parent_start={parent_start} first={:?}",
                    raw_scale.values[axis],
                    axes[axis].keyframes.first()
                );
                if local_milliseconds == 2_000 {
                    assert!((parent_value * 100.0 - FIRST_PARENT_PERCENT).abs() < 1.0e-9);
                    assert!((parent_value - FIRST_PARENT_FRACTION).abs() < 1.0e-12);
                    assert!((child_value * 100.0 - 10_000_000.0).abs() < 0.1);
                }
            }
        }
    }
}
