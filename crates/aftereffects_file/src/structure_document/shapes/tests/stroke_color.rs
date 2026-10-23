use super::*;

fn collected_stroke(expression: &str, source_expression: &str) -> (ShapeStrokeStyle, Vec<String>) {
    let (composition, owner) = color_alias_probe(expression, source_expression);
    let run = vec![test_list(
        b"tdgp",
        vec![
            test_match_name("ADBE Vector Stroke Color"),
            test_color([255.0, 255.0, 0.0, 0.0], expression),
            test_match_name("ADBE Vector Stroke Width"),
            test_numeric(&[12.0], ""),
        ],
    )];
    let mut id = 100;
    let mut budget = AnimationBudget::default();
    let mut collector = Collector {
        source_items: None,
        includes_occurrence_pipeline: true,
        next_id: &mut id,
        animation_budget: &mut budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
        evaluated_shapes: Default::default(),
        mapped_expressions: Vec::new(),
    };
    let decorations = collector.decorations(
        &[("ADBE Vector Graphic - Stroke", &run)],
        Some((&owner, &composition)),
    );
    assert_eq!(decorations.strokes.len(), 1);
    (
        decorations.strokes.into_iter().next().unwrap(),
        collector.warnings,
    )
}

#[test]
fn evaluated_shape_paint_keeps_editable_rgba_without_native_keys() {
    use crate::expression_samples::{EvaluatedProperty, ShapePathSegment};
    use fx_schema::PropertyKeyframeEasing;

    for (name, property) in [
        ("ADBE Vector Fill Color", PropType::FillColor),
        ("ADBE Vector Stroke Color", PropType::StrokeColor),
    ] {
        for supported in [true, false] {
            let run = vec![test_color(
                [255.0, 255.0, 0.0, 0.0],
                "[1-time, time, 0, 0.25+time/2]",
            )];
            let leaves = [(name, run.as_slice())];
            let mut warnings = Vec::new();
            assert!(
                numeric_leaf(&leaves, name, &mut warnings)
                    .unwrap()
                    .keyframes
                    .is_empty()
            );
            let identity = PropertyIdentity::Shape {
                path: vec![ShapePathSegment {
                    index: 1,
                    match_name: name.into(),
                }],
            };
            let samples = EvaluatedProperty {
                composition_id: 1,
                layer_id: 2,
                property: identity.clone(),
                start_ms: 0,
                sample_times_seconds: vec![0.0, 0.5, 1.0],
                values: vec![
                    vec![1.0, 0.0, 0.0, 0.25],
                    vec![0.5, 0.5, 0.0, 0.5],
                    vec![0.0, 1.0, 0.0, 0.75],
                ],
                frame_sampled: true,
            };
            let mut id = 100;
            let mut budget = AnimationBudget::default();
            let mut collector = Collector {
                source_items: None,
                includes_occurrence_pipeline: true,
                next_id: &mut id,
                animation_budget: &mut budget,
                animations: Vec::new(),
                warnings: Vec::new(),
                frame_fade_lowered: false,
                evaluated_shapes: HashMap::from([(
                    run.as_ptr() as usize,
                    EvaluatedShape::Numeric(samples),
                )]),
                mapped_expressions: Vec::new(),
            };
            let targets = [LayerId::new(10), LayerId::new(11)];
            collector.add_paint_entry(&leaves, name, property, &targets, supported);
            if !supported {
                assert!(collector.animations.is_empty());
                assert!(collector.mapped_expressions.is_empty());
                assert!(
                    collector
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("non-solid or non-primary"))
                );
                continue;
            }
            assert_eq!(collector.animations.len(), 2, "{:?}", collector.warnings);
            for (entry, target_id) in collector.animations.iter().zip(targets) {
                assert_eq!(entry.target, PropertyTarget::layer(target_id, property));
                assert!(!entry.animator.is_js_script());
                let keys = entry.animator.keyframe_track().unwrap().keyframes();
                assert_eq!(keys.len(), 2);
                assert_eq!(keys[0].layer_time().as_millis(), 0);
                assert_eq!(keys[1].layer_time().as_millis(), 1000);
                assert_eq!(
                    keys[0].value(),
                    &PropertyValue::Color([1.0, 0.0, 0.0, 0.25])
                );
                assert_eq!(
                    keys[1].value(),
                    &PropertyValue::Color([0.0, 1.0, 0.0, 0.75])
                );
                assert_eq!(keys[1].easing(), PropertyKeyframeEasing::Linear);
            }
            assert!(
                collector
                    .mapped_expressions
                    .iter()
                    .all(|(mapped, _)| mapped == &identity)
            );
            assert_eq!(collector.mapped_expressions.len(), 2);
            assert!(
                collector
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("Adobe oracle equality unverified"))
            );
            fx_schema::AnimationGraph::from_entries(collector.animations).unwrap();
        }
    }
}

#[test]
fn static_stroke_color_alias_uses_same_color_control_resolver_as_fill() {
    let (stroke, warnings) = collected_stroke(
        "thisComp.layer(\"Color Controller\").effect(\"Background\")(\"Color\")",
        "",
    );
    assert_eq!(
        stroke.paint,
        ShapePaint::Solid {
            color: [0.0, 0.0, 0.0, 1.0].into()
        }
    );
    assert_eq!(stroke.width.value(), 12.0);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("live controller linkage is lost"))
    );
}

#[test]
fn plain_stroke_color_retains_authored_style_without_alias_diagnostic() {
    let (stroke, warnings) = collected_stroke("", "");
    assert_eq!(
        stroke.paint,
        ShapePaint::Solid {
            color: [1.0, 0.0, 0.0, 1.0].into()
        }
    );
    assert_eq!(stroke.width.value(), 12.0);
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("ADBE Vector Stroke Color"))
    );
}

#[test]
fn expression_backed_stroke_controller_is_not_misrepresented_as_static() {
    let (stroke, warnings) = collected_stroke(
        "thisComp.layer(\"Color Controller\").effect(\"Background\")(\"Color\")",
        "value",
    );
    assert_eq!(
        stroke.paint,
        ShapePaint::Solid {
            color: [1.0, 0.0, 0.0, 1.0].into()
        }
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("ADBE Vector Stroke Color: unsupported expression"))
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("live controller linkage is lost"))
    );
}
