use super::*;
use crate::{
    essential::{OverrideValue, SourcePropertyRef},
    properties,
    rifx::Chunk,
    schema::layer_records::LayerRecord,
    structure::{ItemKind, StructuralProject, read_project},
    structure_document::to_structural_fx_document,
};
use fx_schema::{PropType, PropertyValue};

const SOURCE: &[u8] =
    include_bytes!("../../../tests/fixtures/expression_samples/sampled_position_expression.aep");

fn composition(project: &StructuralProject, id: u32) -> &Composition {
    let ItemKind::Composition(comp) = &project.item(id).unwrap().kind else {
        panic!("composition {id} missing");
    };
    comp
}

fn position_storage(comp: &Composition) -> Vec<Chunk> {
    let roots = properties::root_runs(&comp.layers[0].content).unwrap();
    let (_, transform) = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Transform Group")
        .unwrap();
    let group = properties::unique_list(transform, *b"tdgp").unwrap();
    properties::runs(group)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Position")
        .map(|(_, storage)| storage.to_vec())
        .unwrap_or_else(|| position_storage(composition(&read_project(SOURCE).unwrap(), 1)))
}

fn position_override(comp: &Composition, code: &str) -> Override {
    let mut storage = position_storage(comp);
    let leaf = storage
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .unwrap()
        .children_mut()
        .unwrap();
    leaf.retain(|chunk| !matches!(&chunk.id(), b"Utf8" | b"expr"));
    leaf.push(Chunk::data(*b"Utf8", code.as_bytes()).unwrap());
    Override {
        source_comp_id: 1,
        source_layer_id: comp.layers[0].record.id(),
        value: OverrideValue::Property {
            path: ["ADBE Transform Group", "ADBE Position"]
                .into_iter()
                .map(|name| SourcePropertyRef {
                    match_name: name.into(),
                    child_index: None,
                })
                .collect(),
            chunks: storage,
        },
    }
}

#[test]
fn occurrence_expression_reuse_distinguishes_exact_overrides_and_repeats() {
    let project = read_project(SOURCE).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let captured = ExpressionSamples::default();
    let comp = composition(&project, 1);
    let mut cache = ExpressionEvaluations::default();
    let first = cache.evaluate(&items, 1, comp, &captured, &[]);
    assert!(first.samples.errors().is_empty());
    assert!(!first.samples.properties().is_empty());
    for _ in 0..24 {
        let repeated = cache.evaluate(&items, 1, comp, &captured, &[]);
        assert_eq!(repeated.samples, first.samples);
    }
    assert_eq!(cache.evaluations, 1, "repeat factor must not resample");

    for code in ["[time * 20, 12, 0]", "[time * 30, 24, 0]"] {
        let override_value = position_override(comp, code);
        let mut changed = comp.clone();
        crate::essential::apply(&mut changed.layers[0], &override_value).unwrap();
        let evaluated = cache.evaluate(
            &items,
            1,
            &changed,
            &captured,
            std::slice::from_ref(&override_value),
        );
        assert!(evaluated.samples.errors().is_empty());
        assert_ne!(evaluated.samples, first.samples);
        let mut notes = Vec::new();
        let fresh = crate::expression_eval::evaluate_occurrence_with_diagnostics(
            &items, 1, &changed, &captured, true, &mut notes,
        );
        assert_eq!(evaluated.samples, fresh, "override samples changed");
        let repeated = cache.evaluate(&items, 1, &changed, &captured, &[override_value]);
        assert_eq!(repeated.samples, fresh);
    }
    assert_eq!(cache.evaluations, 3, "different bindings must evaluate");
}

#[test]
fn occurrence_expression_reuse_keeps_errors_uncached_and_random_source_clock() {
    let mut project = read_project(SOURCE).unwrap();
    let original = composition(&project, 1).clone();
    let random = position_override(
        &original,
        "seedRandom(7); [random(100) + time, random(200), 0]",
    );
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!("composition missing");
    };
    crate::essential::apply(&mut comp.layers[0], &random).unwrap();
    let mut other = project.item(1).unwrap().clone();
    other.id = 1001;
    other.name = "Other random identity and grid".into();
    let ItemKind::Composition(comp) = &mut other.kind else {
        unreachable!()
    };
    comp.duration_secs = 1.0;
    comp.frame_rate = 12.0;
    project.items.push(other);
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let captured = ExpressionSamples::default();
    let mut cache = ExpressionEvaluations::default();
    let mut successful = Vec::new();
    for id in [1, 1001] {
        let comp = composition(&project, id);
        let first = cache.evaluate(&items, id, comp, &captured, &[]);
        let repeated = cache.evaluate(&items, id, comp, &captured, &[]);
        assert_eq!(first.samples, repeated.samples);
        assert!(first.samples.errors().is_empty());
        let mut notes = Vec::new();
        let fresh = crate::expression_eval::evaluate_occurrence_with_diagnostics(
            &items, id, comp, &captured, false, &mut notes,
        );
        assert_eq!(first.samples, fresh);
        assert!(!first.approximations.is_empty());
        successful.push(first);
    }
    assert_ne!(
        successful[0].samples.properties()[0].values(),
        successful[1].samples.properties()[0].values()
    );
    assert_eq!(
        successful[1].samples.properties()[0]
            .sample_times_seconds()
            .len(),
        13
    );
    assert_eq!(cache.evaluations, 2);

    let bad = position_override(
        composition(&project, 1),
        "thisComp.layer('missing').transform.position",
    );
    let mut comp = composition(&project, 1).clone();
    crate::essential::apply(&mut comp.layers[0], &bad).unwrap();
    for _ in 0..2 {
        let failed = cache.evaluate(&items, 1, &comp, &captured, std::slice::from_ref(&bad));
        assert!(!failed.samples.errors().is_empty());
    }
    assert_eq!(
        cache.evaluations, 4,
        "errors must be evaluated per occurrence"
    );
}

#[test]
fn occurrence_expression_reuse_bounded_cpu_comparison() {
    use std::time::Instant;
    let project = read_project(SOURCE).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let comp = composition(&project, 1);
    let captured = ExpressionSamples::default();
    let start = Instant::now();
    let mut cache = ExpressionEvaluations::default();
    let first = cache.evaluate(&items, 1, comp, &captured, &[]);
    for _ in 1..12 {
        assert_eq!(
            cache.evaluate(&items, 1, comp, &captured, &[]).samples,
            first.samples
        );
    }
    let reused = start.elapsed();
    let start = Instant::now();
    let mut fresh_evaluations = 0;
    for _ in 0..12 {
        let mut fresh = ExpressionEvaluations::default();
        assert_eq!(
            fresh.evaluate(&items, 1, comp, &captured, &[]).samples,
            first.samples
        );
        fresh_evaluations += fresh.evaluations;
    }
    let fresh = start.elapsed();
    assert_eq!((cache.evaluations, fresh_evaluations), (1, 12));
    eprintln!(
        "occurrence_expression_reuse: identical 12-occurrence samples; reused={reused:?} fresh={fresh:?}; evaluations=1/12"
    );
}

#[test]
fn occurrence_expression_reuse_public_import_separates_overridden_instances() {
    const OCCURRENCES: &[u8] =
        include_bytes!("../../../tests/fixtures/essential/import_occurrence_overrides.aep");
    for reverse in [false, true] {
        let mut project = read_project(OCCURRENCES).unwrap();
        let mut value = position_override(
            composition(&project, 32),
            "[value[0] + time * 100, value[1], 0]",
        );
        value.source_comp_id = 32;
        let ItemKind::Composition(comp) = &mut project
            .items
            .iter_mut()
            .find(|item| item.id == 32)
            .unwrap()
            .kind
        else {
            unreachable!()
        };
        crate::essential::apply(&mut comp.layers[0], &value).unwrap();
        let ItemKind::Composition(comp) = &mut project
            .items
            .iter_mut()
            .find(|item| item.id == 46)
            .unwrap()
            .kind
        else {
            unreachable!()
        };
        // Both native instances save explicit values, even the default one.
        // Make one supplementary CPU occurrence genuinely unoverridden.
        comp.layers
            .iter_mut()
            .find(|layer| layer.name.as_ref() == "unchanged_instance")
            .unwrap()
            .content
            .clear();
        if reverse {
            comp.layers.reverse();
        }
        let imported = to_structural_fx_document(&project, Some(46)).unwrap();
        let tracks = imported
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .filter(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|p| p.property_type() == PropType::PositionX)
            })
            .count();
        assert_eq!(
            tracks, 1,
            "only the unoverridden instance retains the source expression: {:?}",
            imported.diagnostics
        );
        assert_eq!(
            imported
                .diagnostics
                .iter()
                .filter(|d| d.message.contains("converter-evaluated expression lowered"))
                .count(),
            1
        );
        fx_schema::EditableFxCompositionDocument::from_json_slice(
            &imported.document.to_json_vec().unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn occurrence_expression_reuse_public_import_keeps_independent_editable_tracks() {
    for (mut project, mixed) in [
        (read_project(SOURCE).unwrap(), false),
        (mixed_batch_project(), true),
    ] {
        let mut parent = project.item(1).unwrap().clone();
        parent.id = 1001;
        parent.name = "Repeated editable precomposition".into();
        let ItemKind::Composition(comp) = &mut parent.kind else {
            unreachable!()
        };
        let template = comp.layers[0].clone();
        comp.layers = (0..12)
            .map(|index| {
                let mut layer = template.clone();
                let mut record = layer.record.raw_bytes().to_vec();
                record[0..4].copy_from_slice(&(10_000_u32 + index).to_be_bytes());
                record[40..44].copy_from_slice(&1_u32.to_be_bytes());
                layer.record = LayerRecord::decode(&record).unwrap();
                layer.content.clear();
                layer
            })
            .collect();
        project.items.push(parent);
        let imported = to_structural_fx_document(&project, Some(1001)).unwrap();
        let entries: Vec<_> = imported
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .filter(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|p| p.property_type() == PropType::PositionX)
            })
            .collect();
        assert_eq!(
            entries.len(),
            12,
            "every occurrence needs editable Position"
        );
        let targets: std::collections::HashSet<_> = entries
            .iter()
            .map(|entry| entry.target.layer_id())
            .collect();
        assert_eq!(targets.len(), 12, "FX IDs must not be shared");
        let tracks: Vec<_> = entries
            .iter()
            .map(|entry| entry.animator.keyframe_track().unwrap())
            .collect();
        let keys = tracks[0].keyframes();
        assert!(keys.len() >= 2);
        assert_ne!(keys.first().unwrap().value(), keys.last().unwrap().value());
        assert!(matches!(keys[0].value(), PropertyValue::Float(_)));
        for track in &tracks[1..] {
            let mut actual = serde_json::to_value(track).unwrap();
            let mut expected = serde_json::to_value(tracks[0]).unwrap();
            for value in [&mut actual, &mut expected] {
                for key in value["keyframes"].as_array_mut().unwrap() {
                    key.as_object_mut().unwrap().remove("id");
                }
            }
            assert_eq!(
                actual, expected,
                "samples/clocks/easing must stay identical"
            );
        }
        let messages = imported
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic
                    .message
                    .contains("converter-evaluated expression lowered")
            })
            .count();
        assert_eq!(messages, 12, "lowering diagnostics remain occurrence-local");
        if mixed {
            assert_eq!(
                imported
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic
                        .message
                        .contains("converter expression evaluation unsupported"))
                    .count(),
                12,
                "failed sibling diagnostics remain occurrence-local"
            );
        }
        fx_schema::EditableFxCompositionDocument::from_json_slice(
            &imported.document.to_json_vec().unwrap(),
        )
        .unwrap();
    }
}

fn mixed_batch_project() -> StructuralProject {
    let mut project = read_project(SOURCE).unwrap();
    let source = composition(&project, 1).clone();
    let random = position_override(&source, "seedRandom(7); [random(100) + time * 20, 12, 0]");
    let bad = position_override(&source, "thisComp.layer('missing').transform.position");
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    let mut failed = comp.layers[0].clone();
    let mut record = failed.record.raw_bytes().to_vec();
    record[0..4].copy_from_slice(&100_u32.to_be_bytes());
    failed.record = LayerRecord::decode(&record).unwrap();
    failed.name = "failed expression sibling".into();
    crate::essential::apply(&mut failed, &bad).unwrap();
    crate::essential::apply(&mut comp.layers[0], &random).unwrap();
    comp.layers.push(failed);
    project
}

#[test]
fn occurrence_expression_reuse_mixed_batch_keeps_source_text_and_note_order() {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/expression_samples/expression_apis2.aep"
    ))
    .unwrap();
    let original = composition(&project, 1);
    let bad = position_override(original, "thisComp.layer('missing').transform.position");
    let mut failed = original
        .layers
        .iter()
        .find(|layer| layer.record.id() == 13)
        .unwrap()
        .clone();
    let mut record = failed.record.raw_bytes().to_vec();
    record[0..4].copy_from_slice(&100_u32.to_be_bytes());
    failed.record = LayerRecord::decode(&record).unwrap();
    failed.name = "failed Position with live Source Text".into();
    crate::essential::apply(&mut failed, &bad).unwrap();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    comp.layers.push(failed);
    let comp = composition(&project, 1);
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let captured = ExpressionSamples::default();
    let mut notes = Vec::new();
    let fresh = crate::expression_eval::evaluate_occurrence_with_diagnostics(
        &items, 1, comp, &captured, false, &mut notes,
    );
    assert_eq!(fresh.errors().len(), 1);
    assert!(fresh.texts.len() >= 2);
    assert!(
        fresh
            .texts
            .iter()
            .any(|text| text.texts.first() != text.texts.last())
    );
    assert!(!fresh.properties().is_empty());
    let expected_notes: Vec<_> = notes
        .iter()
        .map(|note| (&note.property, &note.apis, &note.key_notes))
        .collect();
    let mut cache = ExpressionEvaluations::default();
    for _ in 0..3 {
        let result = cache.evaluate(&items, 1, comp, &captured, &[]);
        assert_eq!(
            result.samples, fresh,
            "Source Text/numeric grids/errors changed"
        );
        let actual_notes: Vec<_> = result
            .approximations
            .iter()
            .map(|note| (&note.property, &note.apis, &note.key_notes))
            .collect();
        assert_eq!(actual_notes, expected_notes, "diagnostic order changed");
        assert!(
            cache.successful[&1].evaluation.samples.texts.is_empty(),
            "mixed-batch Source Text must remain freshly evaluated"
        );
    }
    assert_eq!(cache.evaluations, 3);
    assert_eq!(cache.reused_properties, fresh.properties().len() * 2);
}

#[test]
fn occurrence_expression_reuse_mixed_batch_retains_complete_tracks_not_errors() {
    use std::time::Instant;
    let project = mixed_batch_project();
    let comp = composition(&project, 1);
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let captured = ExpressionSamples::default();
    let mut notes = Vec::new();
    let fresh = crate::expression_eval::evaluate_occurrence_with_diagnostics(
        &items, 1, comp, &captured, false, &mut notes,
    );
    assert_eq!(fresh.properties().len(), 1);
    assert_eq!(fresh.errors().len(), 1);
    assert_eq!(fresh.errors()[0].layer_id(), 100);
    assert!(!notes.is_empty());
    let expected_notes: Vec<_> = notes
        .iter()
        .map(|note| (&note.property, &note.apis, &note.key_notes))
        .collect();
    let mut cache = ExpressionEvaluations::default();
    let start = Instant::now();
    for _ in 0..12 {
        let result = cache.evaluate(&items, 1, comp, &captured, &[]);
        assert_eq!(result.samples, fresh, "grids/values/fresh failures changed");
        let actual_notes: Vec<_> = result
            .approximations
            .iter()
            .map(|note| (&note.property, &note.apis, &note.key_notes))
            .collect();
        assert_eq!(actual_notes, expected_notes, "random diagnostics changed");
        let retained = &cache.successful[&1];
        assert!(!retained.complete);
        assert!(
            retained.evaluation.samples.errors().is_empty(),
            "errors must never enter the reuse entry"
        );
    }
    let reused = start.elapsed();
    assert_eq!(
        cache.evaluations, 12,
        "failed properties must evaluate each time"
    );
    assert_eq!(
        cache.reused_properties, 11,
        "complete numeric tracks must not resample"
    );
    assert!(captured.properties().is_empty() && captured.errors().is_empty());
    let start = Instant::now();
    for _ in 0..12 {
        assert_eq!(
            crate::expression_eval::evaluate_occurrence_with_diagnostics(
                &items,
                1,
                comp,
                &captured,
                false,
                &mut Vec::new(),
            ),
            fresh
        );
    }
    eprintln!(
        "occurrence_expression_reuse_mixed_batch: identical 12-occurrence samples/errors; reused={reused:?} fresh={:?}; complete numeric tracks supplied=11; failures evaluated=12",
        start.elapsed()
    );
}
