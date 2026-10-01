use super::*;
use crate::rifx::Chunk;
use crate::structure_document::animation_budget::AnimationBudget;

fn group(name: &str, children: Vec<Chunk>) -> Vec<Chunk> {
    let mut marker = name.as_bytes().to_vec();
    marker.resize(40, 0);
    vec![
        Chunk::data(*b"tdmn", marker).unwrap(),
        Chunk::list(*b"tdgp", children),
    ]
}

fn numeric(name: &str, values: &[f64]) -> Vec<Chunk> {
    let mut marker = name.as_bytes().to_vec();
    marker.resize(40, 0);
    let mut metadata = vec![0; 124];
    metadata[..2].copy_from_slice(&[0xdb, 0x99]);
    metadata[3] = values.len() as u8;
    vec![
        Chunk::data(*b"tdmn", marker).unwrap(),
        Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", metadata).unwrap(),
                Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                Chunk::data(
                    *b"cdat",
                    values
                        .iter()
                        .flat_map(|value| value.to_be_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap(),
            ],
        ),
    ]
}

#[test]
fn disabled_groups_cannot_select_the_dual_paint_shape_fast_path() {
    // Synthetic selector regression, not independent Adobe render evidence.
    for geometry in [
        "ADBE Vector Shape - Ellipse",
        "ADBE Vector Shape - Star",
        "ADBE Vector Shape - Group",
    ] {
        let chunks: Vec<_> = [
            group(geometry, vec![]),
            group("ADBE Vector Graphic - Stroke", vec![]),
            group("ADBE Vector Graphic - Fill", vec![]),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut program = Program::parse(&chunks, 24);
        let draws = std::mem::take(&mut program.scopes[0].draws);
        program.scopes[0].draws = vec![Draw::Group(ScopeId(1))];
        program.scopes.push(super::super::program::Scope {
            parent: Some(ScopeId(0)),
            enabled: true,
            operation: None,
            transform: None,
            draws,
            geometry: vec![super::super::program::GeometryId(0)],
        });
        program.geometry[0].owner = ScopeId(1);
        for paint in &mut program.paints {
            paint.owner = ScopeId(1);
        }
        assert!(
            dual_source(&program, &mut Vec::new()).is_some(),
            "{geometry}"
        );
        program.scopes[1].enabled = false;
        assert!(
            dual_source(&program, &mut Vec::new()).is_none(),
            "{geometry}"
        );
    }
}

#[test]
fn ellipse_with_fill_and_stroke_keeps_independent_static_paint_opacity() {
    // Synthetic native records supplement, but cannot replace, an AE-authored
    // two-paint Ellipse source and independent render comparison.
    let chunks: Vec<_> = [
        group("ADBE Vector Shape - Ellipse", vec![]),
        group(
            "ADBE Vector Graphic - Stroke",
            [
                numeric("ADBE Vector Stroke Color", &[0.0, 0.0, 1.0]),
                numeric("ADBE Vector Stroke Width", &[4.0]),
                numeric("ADBE Vector Stroke Opacity", &[50.0]),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
        group(
            "ADBE Vector Graphic - Fill",
            [
                numeric("ADBE Vector Fill Color", &[1.0, 0.0, 0.0]),
                numeric("ADBE Vector Fill Opacity", &[80.0]),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut program = Program::parse(&chunks, 24);
    let mut next_id = 2;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    // Reversing the native stack must not select FX's fixed fill-then-stroke
    // order; the generic ordered-paint lowering still owns this case.
    program.scopes[0].draws.reverse();
    assert!(
        lower(
            &mut collector,
            &program,
            "fill above stroke",
            LayerId::new(1),
            &mut OutputBudget::default(),
        )
        .unwrap()
        .is_none()
    );
    assert!(collector.animations.is_empty());
    program.scopes[0].draws.reverse();
    let layers = lower(
        &mut collector,
        &program,
        "two-paint ellipse",
        LayerId::new(1),
        &mut OutputBudget::default(),
    )
    .unwrap()
    .expect("existing FX Shape supports two ordered paints");
    let FxLayer::Shape(shape) = &layers[0] else {
        panic!("native editable Shape")
    };
    assert!(shape.shape.ellipse.is_some());
    assert_eq!(shape.shape.fills.len(), 1);
    assert_eq!(shape.shape.strokes.len(), 1);
    assert_eq!(shape.shape.fills[0].opacity, 0.8);
    assert_eq!(shape.shape.strokes[0].opacity, 0.5);
    assert_eq!(shape.transform.opacity.value(), 100.0);
    assert!(
        collector
            .animations
            .iter()
            .all(|entry| !entry.animator.is_js_script())
    );
    let mut root =
        crate::structure_document::group(LayerId::new(1), "root".into(), None, full_active_range());
    root.layers = crate::structure_document::stored_layers(layers).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("two-paint-ellipse"),
        "Two-paint Ellipse",
        fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
        crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
    )
    .unwrap();
}
