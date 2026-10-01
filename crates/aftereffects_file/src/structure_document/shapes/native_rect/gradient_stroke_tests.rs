use super::*;
use crate::structure::{ItemKind, read_project};
use crate::structure_document::animation_budget::AnimationBudget;
use crate::structure_document::shapes::OutputBudget;

fn named(name: &str, storage: Vec<Chunk>) -> Vec<Chunk> {
    let mut marker = name.as_bytes().to_vec();
    marker.resize(40, 0);
    let mut chunks = vec![Chunk::data(*b"tdmn", marker).unwrap()];
    chunks.extend(storage);
    chunks
}

fn scalar(name: &str, value: f64) -> Vec<Chunk> {
    let mut metadata = vec![0; 124];
    metadata[..2].copy_from_slice(&[0xdb, 0x99]);
    metadata[3] = 1;
    named(
        name,
        vec![Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", metadata).unwrap(),
                Chunk::data(*b"tdsb", vec![0, 0, 0, 1]).unwrap(),
                Chunk::data(*b"cdat", value.to_be_bytes().to_vec()).unwrap(),
            ],
        )],
    )
}

fn find_run(chunks: &[Chunk], name: &str) -> Option<Vec<Chunk>> {
    if let Ok(runs) = crate::properties::runs(chunks)
        && let Some((_, run)) = runs.into_iter().find(|(candidate, _)| *candidate == name)
    {
        return Some(run.to_vec());
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| find_run(children, name))
}

fn source_run(bytes: &[u8], composition: u32, layer: u32, name: &str) -> Vec<Chunk> {
    let project = read_project(bytes).unwrap();
    let ItemKind::Composition(comp) = &project.item(composition).unwrap().kind else {
        panic!("pinned composition");
    };
    let source = comp
        .layers
        .iter()
        .find(|item| item.record.id() == layer)
        .unwrap();
    find_run(&source.content, name).expect("pinned source property")
}

fn contents(fill_opacity: f64, stroke_opacity: f64, dashed: bool) -> Vec<Chunk> {
    // Unchanged pinned sources supply individual records, not a combined native
    // feature oracle. Paint pairing and opacity variants below are synthetic.
    let rect = source_run(
        include_bytes!("../../../../tests/fixtures/geometry/geometry_probe.aep"),
        14,
        40,
        "ADBE Vector Shape - Rect",
    );
    let gradient = source_run(
        include_bytes!("../../../../tests/fixtures/shapes/gradient.aep"),
        1,
        13,
        "ADBE Vector Graphic - G-Fill",
    );
    let leaves = crate::properties::unique_list(&gradient, *b"tdgp").unwrap();
    let mut fill: Vec<_> = crate::properties::runs(leaves)
        .unwrap()
        .into_iter()
        .filter(|(name, _)| *name != "ADBE Vector Fill Opacity")
        .flat_map(|(name, run)| named(name, run.to_vec()))
        .collect();
    fill.extend(scalar("ADBE Vector Fill Opacity", fill_opacity));
    let mut stroke = scalar("ADBE Vector Stroke Opacity", stroke_opacity);
    stroke.extend(scalar("ADBE Vector Stroke Width", 4.0));
    if dashed {
        stroke.extend(named(
            "ADBE Vector Stroke Dashes",
            vec![Chunk::list(
                *b"tdgp",
                [
                    scalar("ADBE Vector Stroke Dash 1", 4.0),
                    scalar("ADBE Vector Stroke Gap 1", 2.0),
                ]
                .into_iter()
                .flatten()
                .collect(),
            )],
        ));
    }
    [
        named("ADBE Vector Shape - Rect", rect),
        named(
            "ADBE Vector Graphic - Stroke",
            vec![Chunk::list(*b"tdgp", stroke)],
        ),
        named(
            "ADBE Vector Graphic - G-Fill",
            vec![Chunk::list(*b"tdgp", fill)],
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[test]
fn gradient_fill_and_solid_stroke_retain_parametric_rect_and_group_ownership() {
    for grouped in [false, true] {
        let mut chunks = contents(100.0, 100.0, false);
        if grouped {
            let transform: Vec<_> = [
                scalar("ADBE Vector Rotation", 30.0),
                scalar("ADBE Vector Group Opacity", 50.0),
            ]
            .into_iter()
            .flatten()
            .collect();
            let entries = [
                named("ADBE Vectors Group", vec![Chunk::list(*b"tdgp", chunks)]),
                named(
                    "ADBE Vector Transform Group",
                    vec![Chunk::list(*b"tdgp", transform)],
                ),
            ]
            .into_iter()
            .flatten()
            .collect();
            chunks = named("ADBE Vector Group", vec![Chunk::list(*b"tdgp", entries)]);
        }
        let mut program = Program::parse(&chunks, 24);
        let mut next_id = 500;
        let mut animation_budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut animation_budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
        };
        let layers = lower(
            &mut collector,
            &program,
            "gradient and stroke",
            LayerId::new(1),
            &mut OutputBudget::default(),
        )
        .unwrap()
        .expect("compatible gradient plus solid Stroke uses native Rect");
        let rect = if grouped {
            let FxLayer::Group(group) = &layers[0] else {
                panic!("owned Group");
            };
            assert_eq!(group.transform.rotation, 30.0);
            assert_eq!(group.transform.opacity.value(), 50.0);
            let FxLayer::Rect(rect) = group.layers[0].data() else {
                panic!("native Rect");
            };
            assert_eq!(rect.parent, Some(group.id));
            rect
        } else {
            let FxLayer::Rect(rect) = &layers[0] else {
                panic!("native Rect");
            };
            assert_eq!(rect.parent, Some(LayerId::new(1)));
            rect
        };
        assert_eq!(rect.rect.size, [100.0, 50.0]);
        assert!(rect.rect.fill_enabled && rect.rect.stroke_enabled);
        assert_eq!(rect.rect.stroke_width.value(), 4.0);
        assert_eq!(rect.rect.stroke_color, Some([1.0; 4]));
        assert_eq!(rect.transform.opacity.value(), 100.0);
        let Some(ShapePaint::Gradient {
            start, end, stops, ..
        }) = &rect.rect.fill_paint
        else {
            panic!("editable gradient");
        };
        assert_eq!(*start, [50.0, 25.0]);
        assert_eq!(*end, [150.0, 25.0]);
        assert!(stops.len() >= 2);
        let owner = usize::from(grouped);
        program.scopes[owner].draws.reverse();
        assert!(
            dual_source(&program, &mut Vec::new()).is_none(),
            "paint order"
        );
        program.scopes[owner].draws.reverse();
        program.scopes[owner].enabled = false;
        assert!(
            dual_source(&program, &mut Vec::new()).is_none(),
            "disabled owner"
        );
        let mut root = crate::structure_document::group(
            LayerId::new(1),
            "root".into(),
            None,
            full_active_range(),
        );
        root.layers = crate::structure_document::stored_layers(layers).unwrap();
        assert!(
            collector
                .animations
                .iter()
                .all(|entry| !entry.animator.is_js_script())
        );
        fx_schema::FXComposition::try_from_parts(
            fx_schema::CompositionId::new("gradient-stroke-rect"),
            "Gradient Stroke Rect",
            fx_schema::AnimationGraph::from_entries(collector.animations).unwrap(),
            crate::structure_document::stored_layers(vec![FxLayer::Group(root)]).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn gradient_pair_does_not_freeze_axes_during_rect_geometry_motion() {
    for property in ["ADBE Vector Rect Size", "ADBE Vector Rect Position"] {
        let chunks = contents(100.0, 100.0, false);
        // Relabelled native Scale keys supplement the guard, not an AE-native
        // animated Rectangle/gradient combination or a fidelity assertion.
        let mut program = Program::parse(&chunks, 24);
        let GeometryKind::Source(source) = &program.geometry[0].kind else {
            panic!("native Rectangle source");
        };
        let leaves = crate::properties::unique_list(source.chunks, *b"tdgp").unwrap();
        let mut modified: Vec<_> = crate::properties::runs(leaves)
            .unwrap()
            .into_iter()
            .filter(|(name, _)| *name != property)
            .flat_map(|(name, run)| named(name, run.to_vec()))
            .collect();
        let animation = super::tests::animated_scale_run();
        let numeric = super::super::numeric_from_run(&animation).unwrap();
        assert!(!numeric.keyframes.is_empty());
        assert!(numeric.keyframes.iter().all(|key| {
            key.values.len() >= 2
                && key.values[..2]
                    .iter()
                    .all(|value| value.is_finite() && *value > 0.0)
        }));
        modified.extend(named(property, animation));
        let geometry = vec![Chunk::list(*b"tdgp", modified)];
        program.geometry[0].kind = GeometryKind::Source(super::super::program::NativeRun {
            name: "ADBE Vector Shape - Rect",
            chunks: &geometry,
        });
        let mut next_id = 500;
        let mut animation_budget = AnimationBudget::default();
        let mut collector = Collector {
            includes_occurrence_pipeline: true,
            next_id: &mut next_id,
            animation_budget: &mut animation_budget,
            animations: Vec::new(),
            warnings: Vec::new(),
            frame_fade_lowered: false,
        };
        assert!(
            lower(
                &mut collector,
                &program,
                "moving gradient Rect",
                LayerId::new(1),
                &mut OutputBudget::default(),
            )
            .unwrap()
            .is_none()
        );
        assert!(collector.animations.is_empty());
        // The same complete animated geometry is eligible with solid Fill:
        // rejection above must not be caused by missing Rectangle properties.
        let solid = vec![Chunk::list(*b"tdgp", Vec::new())];
        let fill = program
            .paints
            .iter_mut()
            .find(|paint| paint.operation.name == "ADBE Vector Graphic - G-Fill")
            .unwrap();
        fill.operation = super::super::program::NativeRun {
            name: "ADBE Vector Graphic - Fill",
            chunks: &solid,
        };
        let layers = lower(
            &mut collector,
            &program,
            "moving solid Rect",
            LayerId::new(1),
            &mut OutputBudget::default(),
        )
        .unwrap()
        .expect("identical geometry is eligible without gradient axes");
        assert!(!layers.is_empty());
    }
}

#[test]
fn gradient_pair_keeps_opacity_and_dash_fallback_boundaries() {
    for (fill, stroke, dashed) in [
        (50.0, 100.0, false),
        (100.0, 50.0, false),
        (100.0, 100.0, true),
    ] {
        let chunks = contents(fill, stroke, dashed);
        let program = Program::parse(&chunks, 24);
        assert!(dual_source(&program, &mut Vec::new()).is_none());
    }
}
