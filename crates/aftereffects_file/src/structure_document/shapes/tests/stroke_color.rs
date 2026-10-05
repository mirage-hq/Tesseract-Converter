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
fn static_stroke_color_alias_uses_same_color_control_resolver_as_fill() {
    let (stroke, warnings) = collected_stroke(
        "thisComp.layer(\"Color Controller\").effect(\"Background\")(\"Color\")",
        "",
    );
    assert_eq!(
        stroke.paint,
        ShapePaint::Solid {
            color: [0.0, 0.0, 0.0, 1.0]
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
            color: [1.0, 0.0, 0.0, 1.0]
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
            color: [1.0, 0.0, 0.0, 1.0]
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
