use super::*;
use crate::expression_samples::{EvaluatedProperty, PropertyIdentity};
use crate::structure::{ItemKind, read_project};
use crate::structure_document::animation_budget::AnimationBudget;
use crate::structure_document::shapes::{EvaluatedShape, OutputBudget, program};

#[test]
fn evaluated_rectangle_controls_reject_invalid_sample_values() {
    let mut sample = EvaluatedProperty {
        composition_id: 1,
        layer_id: 1,
        property: PropertyIdentity::Shape { path: Vec::new() },
        start_ms: 0,
        sample_times_seconds: vec![0.0, 1.0],
        values: vec![vec![0.0, 50.0], vec![100.0, 50.0]],
        frame_sampled: true,
    };
    assert!(valid_evaluated_control(&sample, 2, "ADBE Vector Rect Size"));
    for invalid in [
        vec![-1.0, 50.0],
        vec![f64::NAN, 50.0],
        vec![f64::INFINITY, 50.0],
        vec![100.0],
        vec![100.0, 50.0, 0.0],
    ] {
        sample.values[1] = invalid;
        assert!(!valid_evaluated_control(
            &sample,
            2,
            "ADBE Vector Rect Size"
        ));
    }
    sample.values = vec![vec![0.0, 50.0]; 2];
    assert!(!valid_evaluated_control(
        &sample,
        2,
        "ADBE Vector Rect Size"
    ));
    sample.values = vec![vec![-100.0, 50.0]; 2];
    assert!(valid_evaluated_control(
        &sample,
        2,
        "ADBE Vector Rect Position"
    ));
    sample.values = vec![vec![-1.0]; 2];
    assert!(!valid_evaluated_control(
        &sample,
        1,
        "ADBE Vector Rect Roundness"
    ));
}

#[test]
fn evaluated_rectangle_size_keeps_zero_start_and_editable_centered_keys() {
    let project = read_project(include_bytes!(
        "../../../../tests/fixtures/geometry/geometry_probe.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(14).unwrap().kind else {
        panic!("geometry composition")
    };
    let layer = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 40)
        .unwrap();
    let root = crate::properties::root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Root Vectors Group")
        .unwrap()
        .1;
    let mut program =
        program::Program::parse(crate::properties::unique_list(root, *b"tdgp").unwrap(), 24);
    let GeometryKind::Source(source) = &program.geometry[0].kind else {
        panic!("rectangle")
    };
    let mut run = source.chunks.to_vec();
    let children = run[0].children_mut().unwrap();
    let offset = children
        .iter()
        .position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .unwrap()
                    .starts_with(b"ADBE Vector Rect Size")
        })
        .unwrap();
    let data = children[offset + 1].children_mut().unwrap();
    data.push(Chunk::data(*b"Utf8", b"[time * 100, 50]".to_vec()).unwrap());
    program.geometry[0].kind = GeometryKind::Source(program::NativeRun {
        name: "ADBE Vector Shape - Rect",
        chunks: &run,
    });
    let leaves = property_group(&run, "rectangle").unwrap();
    let size_run = leaves
        .iter()
        .find(|(name, _)| *name == "ADBE Vector Rect Size")
        .unwrap()
        .1;
    assert!(
        numeric_leaf(&leaves, "ADBE Vector Rect Size", &mut Vec::new())
            .unwrap()
            .expression_enabled
    );
    let sample = EvaluatedProperty {
        composition_id: 14,
        layer_id: 40,
        property: PropertyIdentity::Shape { path: Vec::new() },
        start_ms: 0,
        sample_times_seconds: vec![0.0, 0.5, 1.0],
        values: vec![vec![0.0, 50.0], vec![50.0, 50.0], vec![100.0, 50.0]],
        frame_sampled: true,
    };
    let mut next_id = 500;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = Collector {
        source_items: None,
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
        evaluated_shapes: [(size_run.as_ptr() as usize, EvaluatedShape::Numeric(sample))].into(),
        mapped_expressions: Vec::new(),
    };
    let layers = lower(
        &mut collector,
        &program,
        "evaluated rectangle",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .expect("evaluated Size must not fall back to a cached outline");
    let FxLayer::Rect(rect) = &layers[0] else {
        panic!("editable Rect")
    };
    assert_eq!(rect.rect.size, [0.0, 50.0]);
    assert_eq!(rect.transform.anchor_point, [0.0, 25.0]);
    for property in [
        PropType::RectSize,
        PropType::AnchorPointX,
        PropType::AnchorPointY,
    ] {
        let entry = collector
            .animations
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(rect.id, property))
            .unwrap();
        assert!(!entry.animator.is_js_script());
        let keys = entry.animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.first().unwrap().layer_time().as_millis(), 0);
        assert_eq!(
            keys.last().unwrap().layer_time().as_millis(),
            if property == PropType::AnchorPointY {
                0
            } else {
                1000
            }
        );
        let (first, last) = match property {
            PropType::RectSize => (
                fx_schema::PropertyValue::Vector2([0.0, 50.0]),
                fx_schema::PropertyValue::Vector2([100.0, 50.0]),
            ),
            PropType::AnchorPointX => (
                fx_schema::PropertyValue::Float(0.0),
                fx_schema::PropertyValue::Float(50.0),
            ),
            _ => (
                fx_schema::PropertyValue::Float(25.0),
                fx_schema::PropertyValue::Float(25.0),
            ),
        };
        assert_eq!(keys.first().unwrap().value(), &first);
        assert_eq!(keys.last().unwrap().value(), &last);
    }
    assert_eq!(collector.mapped_expressions.len(), 1);
    collector.evaluated_shapes.clear();
    assert!(
        lower(
            &mut collector,
            &program,
            "missing evaluation",
            LayerId::new(1),
            &mut OutputBudget::default()
        )
        .unwrap()
        .is_none()
    );
    let mut root =
        crate::structure_document::group(LayerId::new(1), "root".into(), None, full_active_range());
    root.layers = crate::structure_document::stored_layers(layers).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("sampled-rectangle"),
        "Sampled Rectangle",
        fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
        crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
    )
    .unwrap();
}
