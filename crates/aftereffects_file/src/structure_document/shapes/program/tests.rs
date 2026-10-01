use super::*;
use crate::structure_document::animation_budget::AnimationBudget;

fn entry(name: &str, children: Vec<Chunk>) -> Vec<Chunk> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    vec![
        Chunk::data(*b"tdmn", bytes).unwrap(),
        Chunk::list(*b"tdgp", children),
    ]
}

fn entries(values: Vec<Vec<Chunk>>) -> Vec<Chunk> {
    values.into_iter().flatten().collect()
}

fn rectangle() -> Vec<Chunk> {
    entry("ADBE Vector Shape - Rect", vec![])
}
fn fill() -> Vec<Chunk> {
    entry("ADBE Vector Graphic - Fill", vec![])
}
fn stroke() -> Vec<Chunk> {
    entry("ADBE Vector Graphic - Stroke", vec![])
}
fn group(children: Vec<Chunk>) -> Vec<Chunk> {
    entry(
        "ADBE Vector Group",
        entries(vec![
            entry("ADBE Vectors Group", children),
            entry("ADBE Vector Transform Group", vec![]),
        ]),
    )
}

fn disabled(mut operation: Vec<Chunk>) -> Vec<Chunk> {
    operation[1]
        .children_mut()
        .unwrap()
        .insert(0, Chunk::data(*b"tdsb", vec![0, 0, 0, 2]).unwrap());
    operation
}

#[test]
fn disabled_operations_do_not_affect_live_siblings_or_export_group_geometry() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![
        rectangle(),
        disabled(fill()),
        stroke(),
        disabled(entry(
            "ADBE Vector Filter - RC",
            number("ADBE Vector RoundCorner Radius", &[12.0]),
        )),
        disabled(rectangle()),
        disabled(group(entries(vec![rectangle(), fill()]))),
        fill(),
    ]);
    let program = Program::parse(&chunks, 8);
    assert_eq!(
        program.scopes[0].geometry.len(),
        1,
        "disabled sources/groups must not feed parent paints"
    );
    assert!(
        program
            .geometry
            .iter()
            .all(|geometry| geometry.modifiers.is_empty())
    );
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
    let layers = collector.collect_contents(
        &chunks,
        "toggle",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let paints: Vec<_> = layers
        .iter()
        .filter_map(|layer| match layer {
            fx_schema::LayerData::Shape(shape)
                if !shape.shape.fills.is_empty() || !shape.shape.strokes.is_empty() =>
            {
                Some(shape)
            }
            _ => None,
        })
        .collect();
    assert_eq!(paints.len(), 3);
    assert!(paints[0].is_hidden);
    assert!(!paints[1].is_hidden && !paints[2].is_hidden);
    let nested = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Group(group) if !group.layers.is_empty() => Some(group),
            _ => None,
        })
        .unwrap();
    assert!(nested.is_hidden);
    assert!(
        !nested.layers.is_empty(),
        "disabled group content remains editable"
    );
    validate(layers, collector.animations);
}

#[test]
fn converter_gives_each_paint_an_independent_editable_target() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![rectangle(), fill(), stroke()]);
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
    let layers = collector.collect_contents(
        &chunks,
        "shape",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let painted: Vec<_> = layers
        .iter()
        .filter_map(|layer| match layer {
            fx_schema::LayerData::Shape(shape)
                if !shape.shape.fills.is_empty() || !shape.shape.strokes.is_empty() =>
            {
                Some(shape)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        painted.len(),
        2,
        "fill and stroke must not share a broadcast target"
    );
    assert_ne!(painted[0].id, painted[1].id);
    assert!(
        painted
            .iter()
            .all(|shape| shape.shape.fills.len() + shape.shape.strokes.len() == 1)
    );
    let consumers: Vec<_> = painted
        .iter()
        .map(|shape| {
            collector
                .animations
                .iter()
                .find(|entry| {
                    entry.target
                        == fx_schema::PropertyTarget::layer(
                            shape.id,
                            fx_schema::PropType::ShapePath,
                        )
                })
                .unwrap()
        })
        .collect();
    assert!(
        consumers
            .iter()
            .all(|entry| entry.dependencies.is_empty() && !entry.animator.is_js_script())
    );
    assert!(matches!(
        (consumers[0].animator.data(), consumers[1].animator.data()),
        (fx_schema::animator::AnimatorData::Constant { value: fx_schema::PropertyValue::Path(a) },
         fx_schema::animator::AnimatorData::Constant { value: fx_schema::PropertyValue::Path(b) }) if a == b
    ));
    assert!(
        collector
            .warnings
            .iter()
            .any(|message| message.contains("independently editable paint"))
    );
    validate(layers, collector.animations);
}

fn validate(
    layers: Vec<fx_schema::LayerData>,
    animations: Vec<fx_schema::animator::AnimationGraphEntry>,
) {
    let mut root = crate::structure_document::group(
        fx_schema::LayerId::new(1),
        "root".into(),
        None,
        super::super::full_active_range(),
    );
    root.layers = crate::structure_document::stored_layers(layers).unwrap();
    fx_schema::FXComposition::try_from_parts(
        fx_schema::CompositionId::new("paint-test"),
        "Paint test",
        fx_schema::AnimationGraph::from_entries(animations).unwrap(),
        crate::structure_document::stored_layers(vec![fx_schema::LayerData::Group(root)]).unwrap(),
    )
    .unwrap();
}

fn number(name: &str, values: &[f64]) -> Vec<Chunk> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = values.len() as u8;
    vec![
        Chunk::data(*b"tdmn", bytes).unwrap(),
        Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", meta).unwrap(),
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
fn group_opacity_composites_once_and_paint_opacities_have_separate_targets() {
    use super::super::{Collector, Decorations};
    let children = entries(vec![
        rectangle(),
        entry(
            "ADBE Vector Graphic - Fill",
            number("ADBE Vector Fill Opacity", &[25.0]),
        ),
        entry(
            "ADBE Vector Graphic - Stroke",
            number("ADBE Vector Stroke Opacity", &[75.0]),
        ),
    ]);
    let chunks = entry(
        "ADBE Vector Group",
        entries(vec![
            entry("ADBE Vectors Group", children),
            entry(
                "ADBE Vector Transform Group",
                number("ADBE Vector Group Opacity", &[50.0]),
            ),
        ]),
    );
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
    let layers = collector.collect_contents(
        &chunks,
        "group",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let group = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Group(group) if !group.is_hidden => Some(group),
            _ => None,
        })
        .unwrap();
    assert_eq!(group.transform.opacity.value(), 50.0);
    let paints: Vec<_> = group
        .layers
        .iter()
        .filter_map(|layer| match layer.data() {
            fx_schema::LayerData::Shape(shape) => Some(shape),
            _ => None,
        })
        .collect();
    assert_eq!(paints.len(), 2);
    assert_eq!(paints[0].transform.opacity.value(), 25.0);
    assert_eq!(paints[1].transform.opacity.value(), 75.0);
    assert_eq!(paints[0].shape.fills[0].opacity, 1.0);
    assert_eq!(paints[1].shape.strokes[0].opacity, 1.0);
    validate(layers, collector.animations);
}

#[test]
fn offset_and_trim_controls_are_shared_between_paints() {
    use super::super::{Collector, Decorations};
    for (operator, property, target) in [
        (
            "ADBE Vector Filter - Offset",
            "ADBE Vector Offset Amount",
            fx_schema::PropType::OffsetPathsAmount,
        ),
        (
            "ADBE Vector Filter - Trim",
            "ADBE Vector Trim Start",
            fx_schema::PropType::TrimStart,
        ),
    ] {
        let chunks = entries(vec![
            rectangle(),
            fill(),
            stroke(),
            entry(operator, number(property, &[12.0])),
        ]);
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
        let layers = collector.collect_contents(
            &chunks,
            "modified",
            fx_schema::LayerId::new(1),
            8,
            &Decorations::default(),
        );
        let bindings: Vec<_> = layers
            .iter()
            .filter_map(|layer| match layer {
                fx_schema::LayerData::Shape(shape) if !shape.is_hidden => Some(
                    collector
                        .animations
                        .iter()
                        .find(|entry| {
                            entry.target == fx_schema::PropertyTarget::layer(shape.id, target)
                        })
                        .unwrap(),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(bindings.len(), 2);
        assert!(
            bindings
                .iter()
                .all(|entry| entry.dependencies.is_empty() && !entry.animator.is_js_script())
        );
        validate(layers, collector.animations);
    }
}

#[test]
fn shape_budget_rejects_large_paint_without_dangling_entries_and_keeps_smaller_sibling() {
    use super::super::{Collector, OutputBudget};
    fn cost(chunks: &[Chunk]) -> usize {
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
        let mut budget = OutputBudget::with_limit(1_000_000);
        let layers = collector
            .collect_contents_with_budget(
                chunks,
                "budget",
                fx_schema::LayerId::new(1),
                8,
                &mut budget,
            )
            .unwrap();
        assert!(!layers.is_empty());
        assert!(
            !collector
                .warnings
                .iter()
                .any(|warning| warning.contains(super::super::budget::EXHAUSTED))
        );
        1_000_000 - budget.remaining()
    }
    let sources = entries((0..100).map(|_| rectangle()).collect());
    let small = entries(vec![rectangle(), fill()]);
    let mut budget = OutputBudget::with_limit(cost(&sources) + cost(&small) + 1024);
    let mut large = sources;
    large.extend(fill());
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
    let mut layers = collector
        .collect_contents_with_budget(&large, "budget", fx_schema::LayerId::new(1), 8, &mut budget)
        .unwrap();
    assert!(
        layers
            .iter()
            .all(|layer| matches!(layer,fx_schema::LayerData::Shape(shape) if shape.is_hidden))
    );
    assert!(
        collector
            .warnings
            .iter()
            .any(|warning| warning.contains(super::super::budget::EXHAUSTED))
    );
    let following = collector
        .collect_contents_with_budget(&small, "budget", fx_schema::LayerId::new(1), 8, &mut budget)
        .unwrap();
    assert!(
        following
            .iter()
            .any(|layer| matches!(layer, fx_schema::LayerData::Rect(rect) if !rect.is_hidden))
    );
    layers.extend(following);
    validate(layers, collector.animations);
}

#[test]
fn review_shapes_boolean_missing_operand_is_atomic_and_keeps_independent_sibling() {
    use super::super::{Collector, Decorations};

    let malformed_operand = entry(
        "ADBE Vector Shape - Rect",
        entry(
            "ADBE Vector Rect Size",
            vec![Chunk::data(*b"tdb4", vec![0]).unwrap()],
        ),
    );
    let chunks = entries(vec![
        malformed_operand,
        rectangle(),
        entry(
            "ADBE Vector Filter - Merge",
            number("ADBE Vector Merge Type", &[3.0]),
        ),
        fill(),
        group(entries(vec![rectangle(), stroke()])),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "atomic Boolean",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );

    assert!(
        layers
            .iter()
            .all(|layer| !matches!(layer, fx_schema::LayerData::BooleanOperation(_))),
        "a Boolean with a missing operand must be omitted atomically"
    );
    assert!(
        layers.iter().any(|layer| {
            matches!(
                layer,
                fx_schema::LayerData::Group(group)
                    if !group.is_hidden
                        && group.layers.iter().any(|child| {
                            matches!(
                                child.data(),
                                fx_schema::LayerData::Shape(shape)
                                    if !shape.is_hidden && !shape.shape.strokes.is_empty()
                            )
                        })
            )
        }),
        "the independent following stroke group must survive"
    );
    validate(layers, collector.animations);
}

#[test]
fn review_shapes_hidden_helper_is_charged_once() {
    use super::super::{Collector, OutputBudget};

    let chunks = rectangle();
    let limit = 1_000_000;
    let mut budget = OutputBudget::with_limit(limit);
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
    let layers = collector
        .collect_contents_with_budget(
            &chunks,
            "one helper",
            fx_schema::LayerId::new(1),
            8,
            &mut budget,
        )
        .unwrap();

    assert_eq!(layers.len(), 1);
    assert!(matches!(
        &layers[0],
        fx_schema::LayerData::Shape(shape) if shape.is_hidden
    ));
    let expected = serde_json::to_vec(&(layers.first().unwrap(), collector.animations.as_slice()))
        .unwrap()
        .len();
    assert_eq!(
        limit - budget.remaining(),
        expected,
        "the hidden helper and its animation suffix must each be serialized exactly once"
    );
    validate(layers, collector.animations);
}

#[test]
fn review_shapes_failed_group_budget_transaction_keeps_following_sibling() {
    use super::super::{Collector, OutputBudget};

    let failed_group = group(entries(vec![rectangle(), rectangle(), fill()]));
    let mut baseline_budget = OutputBudget::with_limit(1_000_000);
    let mut baseline_next_id = 100;
    let mut baseline_animation_budget = AnimationBudget::default();
    let mut baseline = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut baseline_next_id,
        animation_budget: &mut baseline_animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let baseline_layers = baseline
        .collect_contents_with_budget(
            &failed_group,
            "failed group",
            fx_schema::LayerId::new(1),
            8,
            &mut baseline_budget,
        )
        .unwrap();
    let entries_for = |id| {
        baseline
            .animations
            .iter()
            .filter(|entry| entry.target.layer_id() == Some(id))
            .collect::<Vec<_>>()
    };
    let helper_cost: usize = baseline_layers
        .iter()
        .map(|layer| match layer {
            fx_schema::LayerData::Group(group) if group.is_hidden => {
                serde_json::to_vec(&(group, entries_for(group.id)))
                    .unwrap()
                    .len()
            }
            fx_schema::LayerData::Shape(shape) if shape.is_hidden => {
                serde_json::to_vec(&(layer, entries_for(shape.id)))
                    .unwrap()
                    .len()
            }
            _ => 0,
        })
        .sum();
    let failed_wrapper = baseline_layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Group(group) if !group.is_hidden => Some(group),
            _ => None,
        })
        .expect("compound fixture lowers to a visible group with a child");
    let failed_child = failed_wrapper
        .layers
        .first()
        .expect("compound fixture has one painted child")
        .data();
    let failed_child_cost = match failed_child {
        fx_schema::LayerData::Shape(shape) => {
            serde_json::to_vec(&(failed_child, entries_for(shape.id)))
                .unwrap()
                .len()
        }
        _ => panic!("compound fixture must lower to a Shape child"),
    };

    let following = entries(vec![rectangle(), fill()]);
    let mut following_budget = OutputBudget::with_limit(1_000_000);
    let mut following_next_id = 500;
    let mut following_animation_budget = AnimationBudget::default();
    let mut following_baseline = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut following_next_id,
        animation_budget: &mut following_animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let following_layers = following_baseline
        .collect_contents_with_budget(
            &following,
            "following sibling",
            fx_schema::LayerId::new(1),
            8,
            &mut following_budget,
        )
        .unwrap();
    let following_cost = 1_000_000 - following_budget.remaining();
    assert_eq!(following_layers.len(), 1);
    assert!(
        failed_child_cost >= following_cost,
        "fixture must leave enough rolled-back child capacity for the sibling"
    );

    let mut budget = OutputBudget::with_limit(helper_cost + failed_child_cost);
    let mut next_id = 100;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let mut layers = collector
        .collect_contents_with_budget(
            &failed_group,
            "failed group",
            fx_schema::LayerId::new(1),
            8,
            &mut budget,
        )
        .unwrap();
    assert!(
        layers.iter().all(|layer| {
            !matches!(layer, fx_schema::LayerData::Group(group) if !group.is_hidden)
        })
    );
    let following_layers = collector
        .collect_contents_with_budget(
            &following,
            "following sibling",
            fx_schema::LayerId::new(1),
            8,
            &mut budget,
        )
        .unwrap();
    assert!(
        following_layers
            .iter()
            .any(|layer| { matches!(layer, fx_schema::LayerData::Rect(rect) if !rect.is_hidden) }),
        "a failed compound group must restore capacity for a smaller independent sibling"
    );
    layers.extend(following_layers);
    validate(layers, collector.animations);
}

fn has_visible_paint(layer: &fx_schema::LayerData) -> bool {
    match layer {
        fx_schema::LayerData::Rect(rect) => {
            !rect.is_hidden && (rect.rect.fill_enabled || rect.rect.stroke_enabled)
        }
        fx_schema::LayerData::Shape(shape) => {
            !shape.is_hidden && (!shape.shape.fills.is_empty() || !shape.shape.strokes.is_empty())
        }
        fx_schema::LayerData::BooleanOperation(boolean) => {
            !boolean.is_hidden && (!boolean.fills.is_empty() || !boolean.strokes.is_empty())
        }
        fx_schema::LayerData::Group(group) => {
            !group.is_hidden
                && group
                    .layers
                    .iter()
                    .any(|child| has_visible_paint(child.data()))
        }
        _ => false,
    }
}

#[test]
fn review_shapes_invalid_static_enums_diagnose_and_keep_paints() {
    use super::super::{Collector, Decorations};
    let cases = [
        (
            "Fill Rule",
            "ADBE Vector Fill Rule",
            entries(vec![
                rectangle(),
                entry(
                    "ADBE Vector Graphic - Fill",
                    number("ADBE Vector Fill Rule", &[1.5]),
                ),
            ]),
        ),
        (
            "Stroke Cap",
            "ADBE Vector Stroke Line Cap",
            entries(vec![
                rectangle(),
                entry(
                    "ADBE Vector Graphic - Stroke",
                    number("ADBE Vector Stroke Line Cap", &[4.0]),
                ),
            ]),
        ),
        (
            "Stroke Join",
            "ADBE Vector Stroke Line Join",
            entries(vec![
                rectangle(),
                entry(
                    "ADBE Vector Graphic - Stroke",
                    number("ADBE Vector Stroke Line Join", &[2.5]),
                ),
            ]),
        ),
        (
            "Offset Join",
            "ADBE Vector Offset Line Join",
            entries(vec![
                rectangle(),
                entry(
                    "ADBE Vector Filter - Offset",
                    number("ADBE Vector Offset Line Join", &[0.0]),
                ),
                fill(),
            ]),
        ),
        (
            "Trim Type",
            "ADBE Vector Trim Type",
            entries(vec![
                rectangle(),
                entry(
                    "ADBE Vector Filter - Trim",
                    number("ADBE Vector Trim Type", &[3.0]),
                ),
                fill(),
            ]),
        ),
    ];

    for (case, property, chunks) in cases {
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
        let layers = collector.collect_contents(
            &chunks,
            case,
            fx_schema::LayerId::new(1),
            8,
            &Decorations::default(),
        );
        assert!(
            layers.iter().any(has_visible_paint),
            "{case} must keep the affected paint: {:?}",
            collector.warnings
        );
        assert!(
            collector
                .warnings
                .iter()
                .any(|warning| warning.contains(property)),
            "{case} must diagnose invalid {property}: {:?}",
            collector.warnings
        );
        validate(layers, collector.animations);
    }
}

#[test]
fn compound_commands_above_former_quota_keep_a_paint_consumer() {
    use super::super::{Collector, Decorations};
    let mut chunks = entries((0..2001).map(|_| rectangle()).collect());
    chunks.extend(fill());
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
    let layers = collector.collect_contents(
        &chunks,
        "many editable contours",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    assert!(
        layers.iter().any(has_visible_paint),
        "{:?}",
        collector.warnings
    );
    assert!(
        !collector
            .warnings
            .iter()
            .any(|warning| warning.contains("before contour duplication"))
    );
    assert!(
        collector
            .animations
            .iter()
            .all(|entry| !entry.animator.is_js_script())
    );
    validate(layers, collector.animations);
}

#[test]
fn native_direction_reverses_primitive_winding_and_rectangle_trim_start() {
    use super::super::Collector;
    for direction in [1.0, 2.0, 3.0] {
        for name in [
            "ADBE Vector Shape - Rect",
            "ADBE Vector Shape - Ellipse",
            "ADBE Vector Shape - Star",
        ] {
            let run = entry(
                name,
                entries(vec![
                    number("ADBE Vector Shape Direction", &[direction]),
                    number("ADBE Vector Rect Size", &[100.0, 40.0]),
                ]),
            );
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
            let layer = collector
                .source_layer(
                    name,
                    &run,
                    "direction",
                    fx_schema::LayerId::new(1),
                    None,
                    None,
                )
                .unwrap()
                .unwrap();
            let fx_schema::LayerData::Shape(shape) = &layer else {
                panic!("shape source");
            };
            if let Some(ellipse) = &shape.shape.ellipse {
                assert_eq!(ellipse.reversed, direction == 3.0);
            } else if let Some(star) = &shape.shape.poly_star {
                assert_eq!(star.reversed, direction == 3.0);
            } else {
                assert_eq!(shape.shape.path.commands[0].endpoint(), Some((50.0, -20.0)));
                assert_eq!(
                    shape.shape.path.commands[1].endpoint(),
                    Some(if direction == 3.0 {
                        (-50.0, -20.0)
                    } else {
                        (50.0, 20.0)
                    })
                );
            }
            validate(vec![layer], collector.animations);
        }
    }
}

fn native_keyed_scalar() -> Vec<Chunk> {
    fn find(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
        for (_, run) in crate::properties::runs(chunks).ok()? {
            if crate::properties::unique_list(run, *b"tdbs")
                .and_then(crate::properties::read_numeric)
                .is_ok_and(|value| {
                    value
                        .keyframes
                        .first()
                        .is_some_and(|key| key.values.len() == 1)
                })
            {
                return Some(run.to_vec());
            }
        }
        chunks.iter().filter_map(Chunk::children).find_map(find)
    }
    let project = crate::structure::read_project(include_bytes!(
        "../../../../tests/fixtures/properties/property_rotation.aep"
    ))
    .unwrap();
    project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            crate::structure::ItemKind::Composition(comp) => {
                comp.layers.iter().find_map(|layer| find(&layer.content))
            }
            _ => None,
        })
        .expect("native scalar key records")
}

fn relabel_scalar(name: &str, run: Vec<Chunk>) -> Vec<Chunk> {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    let mut result = vec![Chunk::data(*b"tdmn", bytes).unwrap()];
    result.extend(run);
    result
}

#[test]
fn shared_native_keys_keep_unique_ids_across_geometry_groups_and_modifiers() {
    use super::super::{Collector, Decorations};
    // Supplemental combination/relabeling of native rotation records, not an
    // independently authored Adobe shared-geometry animation fixture.
    let contents = entries(vec![
        entry(
            "ADBE Vector Shape - Star",
            relabel_scalar("ADBE Vector Star Rotation", native_keyed_scalar()),
        ),
        entry(
            "ADBE Vector Filter - Offset",
            relabel_scalar("ADBE Vector Offset Amount", native_keyed_scalar()),
        ),
        fill(),
        stroke(),
    ]);
    let chunks = entries(vec![
        entry(
            "ADBE Vector Group",
            entries(vec![
                entry("ADBE Vectors Group", contents),
                entry(
                    "ADBE Vector Transform Group",
                    relabel_scalar("ADBE Vector Rotation", native_keyed_scalar()),
                ),
            ]),
        ),
        fill(),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "shared native keys",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    for property in [
        fx_schema::PropType::PolyStarRotation,
        fx_schema::PropType::Rotation,
        fx_schema::PropType::OffsetPathsAmount,
    ] {
        let tracks: Vec<_> = collector
            .animations
            .iter()
            .filter(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|target| target.property_type() == property)
            })
            .filter_map(|entry| entry.animator.keyframe_track())
            .collect();
        assert!(tracks.len() >= 3, "{property:?}: {:?}", collector.warnings);
        for copy in tracks.iter().skip(1) {
            assert_eq!(copy.keyframes().len(), tracks[0].keyframes().len());
            for (source, copied) in tracks[0].keyframes().iter().zip(copy.keyframes()) {
                assert_ne!(source.id(), copied.id());
                assert_eq!(source.layer_time(), copied.layer_time());
                assert_eq!(source.value(), copied.value());
                assert_eq!(source.easing(), copied.easing());
            }
        }
    }
    assert!(
        collector
            .animations
            .iter()
            .all(|entry| !entry.animator.is_js_script())
    );
    validate(layers, collector.animations);
}

#[test]
fn native_star_roundess_tracks_use_shared_source_targets() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![
        entry(
            "ADBE Vector Shape - Star",
            entries(vec![
                relabel_scalar("ADBE Vector Star Outer Roundess", native_keyed_scalar()),
                relabel_scalar("ADBE Vector Star Inner Roundess", native_keyed_scalar()),
                relabel_scalar("ADBE Vector Star Points", native_keyed_scalar()),
            ]),
        ),
        fill(),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "keyed star",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    for property in [
        fx_schema::PropType::PolyStarOuterRoundness,
        fx_schema::PropType::PolyStarInnerRoundness,
        fx_schema::PropType::PolyStarPoints,
    ] {
        assert!(collector.animations.iter().any(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|target| target.property_type() == property)
                && matches!(
                    entry.animator.data(),
                    fx_schema::animator::AnimatorData::Keyframes { .. }
                )
        }));
    }
    assert!(
        collector
            .warnings
            .iter()
            .any(|warning| warning.contains("point counts are floored"))
    );
    validate(layers, collector.animations);
}

#[test]
fn gradient_stroke_miter_limit_keeps_numeric_animation() {
    use super::super::{Collector, Decorations};
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
    let layers = collector.collect_contents(
        &entries(vec![rectangle(), stroke()]),
        "stroke",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let layer = layers
        .iter()
        .find(|layer| matches!(layer, fx_schema::LayerData::Rect(rect) if rect.rect.stroke_enabled))
        .unwrap();
    let native = entry(
        "ADBE Vector Graphic - G-Stroke",
        relabel_scalar("ADBE Vector Stroke Miter Limit", native_keyed_scalar()),
    );
    collector.add_scope_entries(
        &crate::properties::runs(&native).unwrap(),
        &Decorations::default(),
        std::slice::from_ref(layer),
        None,
    );
    assert!(collector.animations.iter().any(|entry| {
        entry
            .target
            .as_property()
            .is_some_and(|target| target.property_type() == fx_schema::PropType::StrokeMiterLimit)
    }));
    validate(layers, collector.animations);
}

#[test]
fn native_star_roundess_names_keep_shared_editable_controls() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![
        entry(
            "ADBE Vector Shape - Star",
            entries(vec![
                number("ADBE Vector Star Outer Roundess", &[45.0]),
                number("ADBE Vector Star Inner Roundess", &[30.0]),
                number("ADBE Vector Star Points", &[4.5]),
            ]),
        ),
        fill(),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "star",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let source = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Shape(shape) if shape.shape.poly_star.is_some() => Some(shape),
            _ => None,
        })
        .unwrap();
    let star = source.shape.poly_star.as_ref().unwrap();
    assert_eq!((star.outer_roundness, star.inner_roundness), (45.0, 30.0));
    // Keep authored parametric controls editable; fractional Star rendering is excluded.
    assert_eq!(star.points, 4.5);
    assert!(
        collector
            .warnings
            .iter()
            .any(|warning| warning.contains("point counts are floored"))
    );
    for (property, expected) in [
        (fx_schema::PropType::PolyStarOuterRoundness, 45.0),
        (fx_schema::PropType::PolyStarInnerRoundness, 30.0),
        (fx_schema::PropType::PolyStarPoints, 4.5),
    ] {
        assert!(collector.animations.iter().all(|entry| {
            entry.target != fx_schema::PropertyTarget::layer(source.id, property)
                || !entry.animator.is_js_script()
        }));
        let actual = match property {
            fx_schema::PropType::PolyStarOuterRoundness => star.outer_roundness,
            fx_schema::PropType::PolyStarInnerRoundness => star.inner_roundness,
            fx_schema::PropType::PolyStarPoints => star.points,
            _ => unreachable!(),
        };
        assert_eq!(actual, expected);
    }
    validate(layers, collector.animations);
}

#[test]
fn shape_group_and_each_paint_own_their_blend_modes() {
    use super::super::{Collector, Decorations};
    let child = entries(vec![
        rectangle(),
        entry(
            "ADBE Vector Graphic - Fill",
            number("ADBE Vector Blend Mode", &[4.0]),
        ),
        entry(
            "ADBE Vector Graphic - Stroke",
            number("ADBE Vector Blend Mode", &[10.0]),
        ),
    ]);
    let chunks = entries(vec![
        entry(
            "ADBE Vector Group",
            entries(vec![
                number("ADBE Vector Blend Mode", &[15.0]),
                entry("ADBE Vectors Group", child),
            ]),
        ),
        entry(
            "ADBE Vector Graphic - Fill",
            number("ADBE Vector Blend Mode", &[7.0]),
        ),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "blend ownership",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let group = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Group(group) if !group.is_hidden => Some(group),
            _ => None,
        })
        .unwrap();
    assert_eq!(group.blend_mode, fx_schema::BlendMode::Overlay);
    let modes: Vec<_> = group
        .layers
        .iter()
        .filter_map(|layer| match layer.data() {
            fx_schema::LayerData::Shape(shape) => Some(shape.blend_mode),
            _ => None,
        })
        .collect();
    assert_eq!(
        modes,
        vec![fx_schema::BlendMode::Multiply, fx_schema::BlendMode::Screen]
    );
    let parent_paint = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Shape(shape) if !shape.is_hidden => Some(shape),
            _ => None,
        })
        .unwrap();
    assert_eq!(parent_paint.blend_mode, fx_schema::BlendMode::DarkerColor);
    validate(layers, collector.animations);
}

#[test]
fn long_append_chains_and_boolean_depth_are_bounded() {
    use super::super::{Collector, Decorations};
    for (mode, count) in [(1.0, 2000), (2.0, 40)] {
        let mut chunks = rectangle();
        for _ in 0..count {
            chunks.extend(entry(
                "ADBE Vector Filter - Merge",
                number("ADBE Vector Merge Type", &[mode]),
            ));
        }
        chunks.extend(fill());
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
        let layers = collector.collect_contents(
            &chunks,
            "bounded",
            fx_schema::LayerId::new(1),
            2,
            &Decorations::default(),
        );
        assert!(!layers.is_empty());
        if mode == 2.0 {
            assert!(
                collector
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("remaining Boolean depth"))
            );
        }
        validate(layers, collector.animations);
    }
}

#[test]
fn bounded_boolean_keeps_native_rectangle_operands_and_typed_keys() {
    use super::super::{Collector, Decorations};
    // Supplemental construction from native numeric/key records. This is not
    // independent Adobe render or editable-structure proof.
    let first = entry(
        "ADBE Vector Shape - Rect",
        entries(vec![
            number("ADBE Vector Rect Size", &[120.0, 60.0]),
            number("ADBE Vector Rect Position", &[25.0, 30.0]),
            relabel_scalar("ADBE Vector Rect Roundness", native_keyed_scalar()),
        ]),
    );
    let second = entry(
        "ADBE Vector Shape - Rect",
        entries(vec![
            number("ADBE Vector Rect Size", &[80.0, 40.0]),
            number("ADBE Vector Rect Position", &[-10.0, 5.0]),
            number("ADBE Vector Rect Roundness", &[4.0]),
        ]),
    );
    let chunks = entries(vec![
        first,
        second,
        entry(
            "ADBE Vector Filter - Merge",
            number("ADBE Vector Merge Type", &[2.0]),
        ),
        fill(),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "typed Rectangle Boolean",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let boolean = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::BooleanOperation(layer) => Some(layer),
            _ => None,
        })
        .expect("bounded Merge Paths lowers to Boolean");
    let operands: Vec<_> = boolean.layers.iter().map(|layer| layer.data()).collect();
    assert_eq!(operands.len(), 2);
    let first = match operands[0] {
        fx_schema::LayerData::Rect(rect) => rect,
        _ => panic!("first Boolean operand must remain a typed Rect"),
    };
    assert_eq!(first.parent, Some(boolean.id));
    assert_eq!(first.rect.size, [120.0, 60.0]);
    assert_eq!(first.transform.anchor_point, [60.0, 30.0]);
    assert_eq!(
        first.transform.position,
        fx_schema::Position::TwoD([25.0, 30.0])
    );
    assert!(!first.rect.fill_enabled && !first.rect.stroke_enabled);
    assert!(collector.animations.iter().any(|entry| {
        entry.target
            == fx_schema::PropertyTarget::layer(first.id, fx_schema::PropType::RectRoundness)
            && !entry.animator.is_js_script()
    }));
    validate(layers, collector.animations);
}

#[test]
fn nested_boolean_crosses_only_static_identity_vector_scopes() {
    use super::super::{Collector, Decorations};

    for (name, transform, expect_outer) in [
        ("identity", Vec::new(), true),
        (
            "nonidentity",
            number("ADBE Vector Rotation", &[15.0]),
            false,
        ),
        (
            "animated",
            relabel_scalar("ADBE Vector Rotation", native_keyed_scalar()),
            false,
        ),
    ] {
        let nested = entry(
            "ADBE Vector Group",
            entries(vec![
                entry(
                    "ADBE Vectors Group",
                    entries(vec![
                        rectangle(),
                        rectangle(),
                        entry(
                            "ADBE Vector Filter - Merge",
                            number("ADBE Vector Merge Type", &[2.0]),
                        ),
                    ]),
                ),
                entry("ADBE Vector Transform Group", transform),
            ]),
        );
        let chunks = entries(vec![
            nested,
            rectangle(),
            entry(
                "ADBE Vector Filter - Merge",
                number("ADBE Vector Merge Type", &[2.0]),
            ),
            fill(),
        ]);
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
        let layers = collector.collect_contents(
            &chunks,
            name,
            fx_schema::LayerId::new(1),
            8,
            &Decorations::default(),
        );
        let outer = layers.iter().find_map(|layer| match layer {
            fx_schema::LayerData::BooleanOperation(boolean) => Some(boolean),
            _ => None,
        });
        assert_eq!(outer.is_some(), expect_outer, "{name}");
        if let Some(outer) = outer {
            assert_eq!(outer.layers.len(), 2, "{name}");
            assert!(matches!(
                outer.layers[0].data(),
                fx_schema::LayerData::BooleanOperation(_)
            ));
            assert!(matches!(
                outer.layers[1].data(),
                fx_schema::LayerData::Rect(_)
            ));
        } else {
            assert!(collector.warnings.iter().any(|warning| {
                warning.contains("native Rectangle producer cannot cross a vector-group")
                    && warning.contains("static Shape fallback required")
            }));
        }
        validate(layers, collector.animations);
    }
}

#[test]
fn groups_and_append_are_not_reinterpreted_as_boolean_rectangle_operands() {
    use super::super::{Collector, Decorations};
    let grouped_boolean = entries(vec![
        group(entries(vec![rectangle()])),
        rectangle(),
        entry(
            "ADBE Vector Filter - Merge",
            number("ADBE Vector Merge Type", &[3.0]),
        ),
        fill(),
    ]);
    let append = entries(vec![
        rectangle(),
        rectangle(),
        entry(
            "ADBE Vector Filter - Merge",
            number("ADBE Vector Merge Type", &[1.0]),
        ),
        fill(),
    ]);
    for (name, chunks, expect_boolean) in [
        ("cross-scope group", grouped_boolean, true),
        ("Append", append, false),
    ] {
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
        let layers = collector.collect_contents(
            &chunks,
            name,
            fx_schema::LayerId::new(1),
            8,
            &Decorations::default(),
        );
        let boolean = layers.iter().find_map(|layer| match layer {
            fx_schema::LayerData::BooleanOperation(layer) => Some(layer),
            _ => None,
        });
        assert_eq!(boolean.is_some(), expect_boolean);
        if let Some(boolean) = boolean {
            assert!(
                boolean
                    .layers
                    .iter()
                    .all(|layer| !matches!(layer.data(), fx_schema::LayerData::Group(_)))
            );
            assert_eq!(
                boolean
                    .layers
                    .iter()
                    .filter(|layer| matches!(layer.data(), fx_schema::LayerData::Rect(_)))
                    .count(),
                1,
                "only the same-scope Rectangle may use the typed producer"
            );
            assert!(collector.warnings.iter().any(|warning| {
                warning.contains("crosses a vector scope or an intermediate modifier")
            }));
        }
        validate(layers, collector.animations);
    }
}

#[test]
fn reversed_and_intermediate_modified_rectangles_keep_diagnosed_shape_fallbacks() {
    use super::super::{Collector, Decorations};
    let reversed = entry(
        "ADBE Vector Shape - Rect",
        entries(vec![
            number("ADBE Vector Rect Size", &[100.0, 50.0]),
            number("ADBE Vector Shape Direction", &[3.0]),
        ]),
    );
    let modified = entries(vec![
        entry(
            "ADBE Vector Shape - Rect",
            number("ADBE Vector Rect Size", &[100.0, 50.0]),
        ),
        entry(
            "ADBE Vector Filter - RC",
            number("ADBE Vector RoundCorner Radius", &[8.0]),
        ),
    ]);
    for (name, first) in [
        ("reversed Rectangle", reversed),
        ("modified Rectangle", modified),
    ] {
        let chunks = entries(vec![
            first,
            rectangle(),
            entry(
                "ADBE Vector Filter - Merge",
                number("ADBE Vector Merge Type", &[2.0]),
            ),
            fill(),
        ]);
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
        let layers = collector.collect_contents(
            &chunks,
            name,
            fx_schema::LayerId::new(1),
            8,
            &Decorations::default(),
        );
        let boolean = layers
            .iter()
            .find_map(|layer| match layer {
                fx_schema::LayerData::BooleanOperation(layer) => Some(layer),
                _ => None,
            })
            .expect("bounded Boolean retained");
        assert_eq!(
            boolean
                .layers
                .iter()
                .filter(|layer| matches!(layer.data(), fx_schema::LayerData::Rect(_)))
                .count(),
            1,
            "unsupported producer must fall back without changing its sibling"
        );
        assert!(
            collector
                .animations
                .iter()
                .all(|entry| !entry.animator.is_js_script())
        );
        assert!(collector.warnings.iter().any(|warning| {
            warning.contains("typed Rect Boolean producer")
                || warning.contains("crosses a vector scope or an intermediate modifier")
        }));
        validate(layers, collector.animations);
    }
}

#[test]
fn typed_rectangle_boolean_budget_failure_rolls_back_owned_tracks() {
    use super::super::{Collector, OutputBudget};
    let chunks = entries(vec![
        entry(
            "ADBE Vector Shape - Rect",
            entries(vec![
                number("ADBE Vector Rect Size", &[100.0, 50.0]),
                relabel_scalar("ADBE Vector Rect Roundness", native_keyed_scalar()),
            ]),
        ),
        rectangle(),
        entry(
            "ADBE Vector Filter - Merge",
            number("ADBE Vector Merge Type", &[2.0]),
        ),
        fill(),
    ]);
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
    let layers = collector
        .collect_contents_with_budget(
            &chunks,
            "budgeted typed Rectangle",
            fx_schema::LayerId::new(1),
            8,
            &mut OutputBudget::with_limit(0),
        )
        .unwrap();
    assert!(collector.animations.is_empty());
    assert!(
        collector
            .warnings
            .iter()
            .any(|warning| warning.contains(super::super::budget::EXHAUSTED))
    );
    assert!(
        layers
            .iter()
            .all(|layer| !matches!(layer, fx_schema::LayerData::BooleanOperation(_)))
    );
}

#[test]
fn budget_failed_boolean_helper_copy_rolls_back_and_keeps_later_paint() {
    use super::super::{Collector, OutputBudget};

    let failed_boolean = entries(vec![
        entry(
            "ADBE Vector Shape - Rect",
            entries(vec![
                number("ADBE Vector Rect Size", &[100.0, 50.0]),
                relabel_scalar("ADBE Vector Rect Roundness", native_keyed_scalar()),
            ]),
        ),
        rectangle(),
        entry(
            "ADBE Vector Filter - Merge",
            number("ADBE Vector Merge Type", &[2.0]),
        ),
        fill(),
    ]);
    let later_paint = group(entries(vec![rectangle(), stroke()]));

    let mut baseline_next_id = 2;
    let mut baseline_animation_budget = AnimationBudget::default();
    let mut baseline = Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut baseline_next_id,
        animation_budget: &mut baseline_animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let baseline_layers = baseline
        .collect_contents_with_budget(
            &failed_boolean,
            "baseline Boolean helper copy",
            fx_schema::LayerId::new(1),
            8,
            &mut OutputBudget::default(),
        )
        .unwrap();
    assert!(
        baseline_layers
            .iter()
            .any(|layer| { matches!(layer, fx_schema::LayerData::BooleanOperation(_)) })
    );
    assert!(baseline.animation_budget.used() > 0);
    let serialized_bytes: usize = baseline
        .animations
        .iter()
        .map(crate::structure_document::animation_budget::committed_entry_serialized_bytes)
        .sum::<Result<_, _>>()
        .unwrap();
    assert!(serialized_bytes <= baseline.animation_budget.used());

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

    let failed_layers = collector
        .collect_contents_with_budget(
            &failed_boolean,
            "failed Boolean helper copy",
            fx_schema::LayerId::new(1),
            8,
            &mut OutputBudget::with_limit(0),
        )
        .unwrap();
    assert!(collector.animations.is_empty());
    assert_eq!(collector.animation_budget.used(), 0);
    assert!(
        failed_layers
            .iter()
            .all(|layer| { !matches!(layer, fx_schema::LayerData::BooleanOperation(_)) })
    );

    let later_layers = collector
        .collect_contents_with_budget(
            &later_paint,
            "later paint",
            fx_schema::LayerId::new(1),
            8,
            &mut OutputBudget::default(),
        )
        .unwrap();
    assert!(later_layers.iter().any(|layer| {
        matches!(
            layer,
            fx_schema::LayerData::Rect(rect)
                if !rect.is_hidden && rect.rect.stroke_enabled
        )
    }));
    validate(later_layers, collector.animations);
}

#[test]
fn malformed_shared_operation_is_reported_once_per_native_operation() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![
        rectangle(),
        rectangle(),
        entry(
            "ADBE Vector Filter - RC",
            number("ADBE Vector RoundCorner Radius", &[f64::NAN]),
        ),
        fill(),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "shared omission",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    assert_eq!(
        collector
            .warnings
            .iter()
            .filter(|warning| warning.contains("has no canonical modifier representation"))
            .count(),
        1
    );
    validate(layers, collector.animations);
}

#[test]
fn append_does_not_reorder_trim_before_round_corners() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![
        rectangle(),
        entry(
            "ADBE Vector Filter - Trim",
            number("ADBE Vector Trim Start", &[12.0]),
        ),
        entry(
            "ADBE Vector Filter - Merge",
            number("ADBE Vector Merge Type", &[1.0]),
        ),
        entry(
            "ADBE Vector Filter - RC",
            number("ADBE Vector RoundCorner Radius", &[12.0]),
        ),
        fill(),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "ordered stages",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let paint = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Shape(shape) if !shape.is_hidden => Some(shape),
            _ => None,
        })
        .unwrap();
    assert!(paint.shape.trim.is_some());
    assert!(
        paint.shape.round_corners.is_none(),
        "Round after Trim must not silently become Round before Trim"
    );
    assert!(
        collector
            .warnings
            .iter()
            .any(|warning| warning.contains("resolved-outline stages"))
    );
    validate(layers, collector.animations);
}

#[test]
fn partial_modifier_does_not_leak_to_later_geometry() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![
        rectangle(),
        entry(
            "ADBE Vector Filter - RC",
            number("ADBE Vector RoundCorner Radius", &[12.0]),
        ),
        rectangle(),
        fill(),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "partial",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let paint = layers
        .iter()
        .find_map(|layer| match layer {
            fx_schema::LayerData::Shape(shape) if !shape.is_hidden => Some(shape),
            _ => None,
        })
        .unwrap();
    assert!(paint.shape.round_corners.is_none());
    let entry = collector
        .animations
        .iter()
        .find(|entry| {
            entry.target
                == fx_schema::PropertyTarget::layer(paint.id, fx_schema::PropType::ShapePath)
        })
        .unwrap();
    assert!(entry.dependencies.is_empty() && !entry.animator.is_js_script());
    assert!(
        paint
            .shape
            .path
            .commands
            .iter()
            .any(|command| command.corner_radius().is_some())
    );
    assert!(
        paint
            .shape
            .path
            .commands
            .iter()
            .any(|command| command.corner_radius().is_none())
    );
    assert!(
        collector
            .warnings
            .iter()
            .any(|warning| warning.contains("static per-point geometry"))
    );
    assert!(
        !collector
            .warnings
            .iter()
            .any(|warning| warning.contains("partially covered"))
    );
    validate(layers, collector.animations);
}

#[test]
fn round_corners_remains_shared_by_independent_paints() {
    use super::super::{Collector, Decorations};
    let chunks = entries(vec![
        rectangle(),
        fill(),
        stroke(),
        entry(
            "ADBE Vector Filter - RC",
            number("ADBE Vector RoundCorner Radius", &[12.0]),
        ),
    ]);
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
    let layers = collector.collect_contents(
        &chunks,
        "rounded",
        fx_schema::LayerId::new(1),
        8,
        &Decorations::default(),
    );
    let painted: Vec<_> = layers
        .iter()
        .filter_map(|layer| match layer {
            fx_schema::LayerData::Shape(shape) if !shape.is_hidden => Some(shape),
            _ => None,
        })
        .collect();
    assert_eq!(painted.len(), 2);
    for paint in &painted {
        assert_eq!(
            paint
                .shape
                .round_corners
                .as_ref()
                .map(|value| value.radius.value()),
            Some(12.0)
        );
        assert!(
            paint
                .shape
                .path
                .commands
                .iter()
                .all(|command| command.corner_radius().is_none()),
            "zero rectangle roundness must not disable the modifier"
        );
    }
    let bindings: Vec<_> = painted
        .iter()
        .map(|paint| {
            collector
                .animations
                .iter()
                .find(|entry| {
                    entry.target
                        == fx_schema::PropertyTarget::layer(
                            paint.id,
                            fx_schema::PropType::RoundCornersRadius,
                        )
                })
                .unwrap()
        })
        .collect();
    assert!(
        bindings
            .iter()
            .all(|entry| entry.dependencies.is_empty() && !entry.animator.is_js_script())
    );
    validate(layers, collector.animations);
}

#[test]
fn paint_captures_only_preceding_geometry_not_all_scope_descendants() {
    let chunks = entries(vec![rectangle(), fill(), rectangle(), stroke()]);
    let program = Program::parse(&chunks, 8);
    assert!(program.warnings.is_empty(), "{:?}", program.warnings);
    assert_eq!(program.paints[0].geometry, vec![GeometryId(0)]);
    assert_eq!(
        program.paints[1].geometry,
        vec![GeometryId(0), GeometryId(1)]
    );
    assert_eq!(program.paints[0].owner, ScopeId(0));
    assert_eq!(
        program.paints[1].operation.name,
        "ADBE Vector Graphic - Stroke"
    );
    assert!(!program.paints[1].operation.chunks.is_empty());
    assert!(
        matches!(&program.geometry[0].kind, GeometryKind::Source(source) if source.name == "ADBE Vector Shape - Rect")
    );
}

#[test]
fn nested_draws_and_exported_geometry_have_separate_ownership() {
    let chunks = entries(vec![group(entries(vec![rectangle(), fill()])), stroke()]);
    let program = Program::parse(&chunks, 8);
    assert_eq!(program.scopes[1].parent, Some(ScopeId(0)));
    assert!(program.scopes[1].transform.is_some());
    assert_eq!(
        program.scopes[0].draws,
        vec![Draw::Group(ScopeId(1)), Draw::Paint(PaintId(1))]
    );
    assert_eq!(program.scopes[1].draws, vec![Draw::Paint(PaintId(0))]);
    assert_eq!(program.paints[0].owner, ScopeId(1));
    assert_eq!(program.paints[1].owner, ScopeId(0));
    assert_eq!(program.paints[0].geometry, program.paints[1].geometry);
    assert_eq!(program.geometry[0].owner, ScopeId(1));
    // One scope transform/opacity record owns all its paint draws. It is not
    // copied into each paint or inherited by the parent's paint operation.
    assert!(program.scopes[0].transform.is_none());
}

#[test]
fn later_modifiers_update_shared_geometry_without_widening_earlier_paint() {
    let chunks = entries(vec![
        rectangle(),
        fill(),
        rectangle(),
        entry("ADBE Vector Filter - RC", vec![]),
        stroke(),
    ]);
    let program = Program::parse(&chunks, 8);
    assert_eq!(program.paints[0].geometry, vec![GeometryId(0)]);
    for geometry in &program.geometry {
        assert_eq!(geometry.modifiers.len(), 1);
        assert_eq!(geometry.modifiers[0].owner, ScopeId(0));
        assert_eq!(
            geometry.modifiers[0].operation.name,
            "ADBE Vector Filter - RC"
        );
    }
}

#[test]
fn merge_replaces_geometry_and_preceding_visuals_not_just_local_paths() {
    let chunks = entries(vec![
        group(entries(vec![rectangle(), fill()])),
        rectangle(),
        stroke(),
        entry("ADBE Vector Filter - Merge", vec![]),
        fill(),
    ]);
    let program = Program::parse(&chunks, 8);
    assert_eq!(program.scopes[0].draws, vec![Draw::Paint(PaintId(2))]);
    assert_eq!(program.scopes[0].geometry, vec![GeometryId(2)]);
    let GeometryKind::Merge {
        operation,
        operands,
    } = &program.geometry[2].kind
    else {
        panic!("merge")
    };
    assert_eq!(operation.name, "ADBE Vector Filter - Merge");
    assert_eq!(operands, &[GeometryId(0), GeometryId(1)]);
    assert_eq!(program.paints[2].geometry, vec![GeometryId(2)]);
}

#[test]
fn native_composite_order_uses_ae_plugin_ordinals_and_preserves_groups() {
    let chunks = entries(vec![
        rectangle(),
        fill(),
        entry(
            "ADBE Vector Graphic - Stroke",
            number("ADBE Vector Composite Order", &[2.0]),
        ),
        group(entries(vec![rectangle(), fill()])),
        fill(),
    ]);
    let program = Program::parse(&chunks, 8);
    assert_eq!(
        program.scopes[0].paint_order(|_| PaintOrder::BelowPrevious),
        vec![
            Draw::Paint(PaintId(0)),
            Draw::Paint(PaintId(1)),
            Draw::Group(ScopeId(1)),
            Draw::Paint(PaintId(3)),
        ]
    );
    assert_eq!(
        program.scopes[0].paint_order(|id| super::super::blend::composite_order(
            program.paints[id.0].operation.chunks,
            &mut Vec::new()
        )),
        vec![
            Draw::Paint(PaintId(1)),
            Draw::Paint(PaintId(0)),
            Draw::Group(ScopeId(1)),
            Draw::Paint(PaintId(3)),
        ]
    );
    let mut next_id = 2;
    let mut animation_budget = AnimationBudget::default();
    let mut collector = super::super::Collector {
        includes_occurrence_pipeline: true,
        next_id: &mut next_id,
        animation_budget: &mut animation_budget,
        animations: Vec::new(),
        warnings: Vec::new(),
        frame_fade_lowered: false,
    };
    let layers = collector.collect_contents(
        &chunks,
        "order",
        fx_schema::LayerId::new(1),
        8,
        &super::super::Decorations::default(),
    );
    assert!(
        matches!(&layers[0],fx_schema::LayerData::Shape(shape) if !shape.shape.strokes.is_empty()),
        "Above stroke must be topmost"
    );
    validate(layers, collector.animations);
}

#[test]
fn depth_limit_keeps_independent_siblings_without_truncating_references() {
    let chunks = entries(vec![
        group(entries(vec![rectangle(), fill()])),
        rectangle(),
        fill(),
    ]);
    let program = Program::parse(&chunks, 0);
    assert_eq!(program.scopes.len(), 1);
    assert_eq!(program.geometry.len(), 1);
    assert_eq!(program.paints.len(), 1);
    assert!(
        program
            .warnings
            .iter()
            .any(|warning| warning.contains("depth budget"))
    );

    let chunks = entries(
        (0..400)
            .map(|_| rectangle())
            .chain((0..400).map(|_| fill()))
            .collect(),
    );
    let program = Program::parse(&chunks, 2);
    assert_eq!(program.geometry.len(), 400);
    assert_eq!(program.paints.len(), 400);
    assert_eq!(program.paints.last().unwrap().geometry.len(), 400);
    assert!(
        !program
            .warnings
            .iter()
            .any(|warning| warning.contains("budget"))
    );
}

#[test]
fn malformed_group_does_not_discard_following_paint() {
    let mut broken = entry("ADBE Vector Group", vec![]);
    broken.pop();
    let chunks = entries(vec![broken, rectangle(), fill()]);
    let program = Program::parse(&chunks, 8);
    assert_eq!(program.paints.len(), 1);
    assert_eq!(program.geometry.len(), 1);
    assert!(
        program
            .warnings
            .iter()
            .any(|warning| warning.contains("malformed"))
    );
}
