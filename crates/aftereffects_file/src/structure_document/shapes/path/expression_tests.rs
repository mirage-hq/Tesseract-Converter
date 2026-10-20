use super::*;
use fx_schema::{LayerId, PropType};

fn owner() -> Layer {
    let project = crate::structure::read_project(include_bytes!(
        "../../../../tests/fixtures/expression_samples/expression_paths.aep"
    ))
    .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("native composition missing")
    };
    comp.layers
        .iter()
        .find(|layer| layer.name.as_ref() == "CreatedClosed")
        .unwrap()
        .clone()
}

fn value(x: f64) -> NativePathValue {
    NativePathValue {
        vertices: vec![[x, 0.0], [20.0 + x, 15.0], [40.0 + x, 0.0]],
        in_tangents: vec![[0.0, 0.0], [-5.0, 0.0], [-5.0, 0.0]],
        out_tangents: vec![[5.0, 0.0], [5.0, 0.0], [0.0, 0.0]],
        closed: true,
    }
}
fn sample(times: Vec<f64>, values: Vec<NativePathValue>) -> EvaluatedPath {
    EvaluatedPath {
        composition_id: 1,
        layer_id: 15,
        property: crate::expression_samples::PropertyIdentity::Shape { path: Vec::new() },
        sample_times_seconds: times,
        values,
    }
}
fn target() -> PropertyTarget {
    PropertyTarget::layer(LayerId::new(999), PropType::ShapePath)
}

#[test]
fn path_expression_linear_and_constant_observations_reduce_to_sparse_keys() {
    let times: Vec<_> = (0..=30).map(|frame| f64::from(frame) / 30.0).collect();
    let linear = sample(
        times.clone(),
        times.iter().map(|time| value(10.0 * time)).collect(),
    );
    let mut budget = AnimationBudget::default();
    let (entries, warnings) = entries(
        &linear,
        &owner(),
        NumericAnimationClock::source_local(),
        target(),
        &mut budget,
    );
    assert_eq!(entries.len(), 1, "{warnings:?}");
    let track = entries[0].animator.keyframe_track().unwrap();
    assert_eq!(track.keyframes().len(), 2);
    assert_eq!(track.keyframes()[1].layer_time().as_millis(), 1000);
    let constant = sample(times, vec![value(3.0); 31]);
    let (entries, warnings) = super::entries(
        &constant,
        &owner(),
        NumericAnimationClock::source_local(),
        target(),
        &mut budget,
    );
    assert_eq!(entries.len(), 1, "{warnings:?}");
    assert_eq!(
        entries[0]
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()
            .len(),
        1
    );
}

#[test]
fn path_expression_never_emits_an_unreduced_frame_table() {
    let samples = sample(
        vec![0.0, 0.3, 0.6, 1.0],
        vec![value(0.0), value(10.0), value(-10.0), value(20.0)],
    );
    let mut budget = AnimationBudget::default();
    let (entries, warnings) = entries(
        &samples,
        &owner(),
        NumericAnimationClock::source_local(),
        target(),
        &mut budget,
    );
    assert!(entries.is_empty());
    assert_eq!(budget.used(), 0);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("per-frame key output is prohibited"))
    );
}

#[test]
fn path_expression_clock_collisions_and_topology_fail_atomically() {
    let mut samples = sample(
        vec![0.0, 0.0001, 1.0],
        vec![value(0.0), value(1.0), value(2.0)],
    );
    let mut budget = AnimationBudget::default();
    let (entries, warnings) = entries(
        &samples,
        &owner(),
        NumericAnimationClock::source_local(),
        target(),
        &mut budget,
    );
    assert!(entries.is_empty());
    assert_eq!(budget.used(), 0);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("collapse or reorder"))
    );
    samples.sample_times_seconds = vec![0.0, 0.5, 1.0];
    samples.values[1].closed = false;
    let (entries, warnings) = super::entries(
        &samples,
        &owner(),
        NumericAnimationClock::source_local(),
        target(),
        &mut budget,
    );
    assert!(entries.is_empty());
    assert_eq!(budget.used(), 0);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("changing Path expression topology"))
    );
}

#[test]
fn path_expression_tiny_budget_preserves_accounting() {
    let samples = sample(vec![0.0, 1.0], vec![value(0.0), value(10.0)]);
    let mut budget = AnimationBudget::with_limit(1);
    let (entries, _) = entries(
        &samples,
        &owner(),
        NumericAnimationClock::source_local(),
        target(),
        &mut budget,
    );
    assert!(entries.is_empty());
    assert_eq!(budget.used(), 0);
}
