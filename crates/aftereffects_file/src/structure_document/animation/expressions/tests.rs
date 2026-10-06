use super::*;
use fx_schema::{LayerData, PropType};

use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, Layer, read_project},
    structure_document::to_structural_fx_document_with_budget,
};

fn samples(values: Vec<Vec<f64>>) -> EvaluatedProperty {
    EvaluatedProperty {
        composition_id: 1,
        layer_id: 2,
        property: PropertyIdentity::Transform {
            match_name: "ADBE Position".into(),
        },
        start_ms: 2_000,
        sample_times_seconds: Vec::new(),
        frame_sampled: false,
        values,
    }
}

#[test]
fn converter_frame_samples_reuse_fitting_without_claiming_adobe_provenance() {
    let mut observations = samples(vec![vec![0.0], vec![50.0], vec![100.0]]);
    observations.start_ms = 0;
    observations.frame_sampled = true;
    observations.sample_times_seconds = vec![0.0, 0.5, 1.0];
    assert_eq!(fitting_grid(&observations).unwrap(), (0, 1001));
    let (entries, warnings) = evaluated_numeric_entries(
        "test",
        &observations,
        &[target(0, PropType::PositionX)],
        &[],
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 1, "{warnings:?}");
    let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].layer_time().as_millis(), 0);
    assert_eq!(keys[1].layer_time().as_millis(), 1000);
    assert!(matches!(keys[1].value(),PropertyValue::Float(value) if *value==100.0));
    assert!(warnings[0].contains("Adobe oracle equality pending"));
}

#[test]
fn expression_override_invalidation_is_composition_local() {
    use std::collections::{HashMap, HashSet};

    use crate::{
        essential::{Override, OverrideValue},
        expression_samples::ExpressionEvaluationError,
        structure_document::{
            AssetNamespace, Converter, MediaAssetRequest, MediaResolution, Progress, shapes,
        },
    };
    const SOURCE: &[u8] = include_bytes!(
        "../../../../tests/fixtures/expression_samples/sampled_position_expression.aep"
    );
    const ORACLE: &[u8] = include_bytes!(
        "../../../../tests/fixtures/expression_samples/sampled_position_expression.v2.json"
    );
    let project = read_project(SOURCE).unwrap();
    let item = project.item(1).unwrap();
    let oracle = ExpressionSamples::from_json_for_source(ORACLE, SOURCE).unwrap();
    let observed = &oracle.properties()[0];
    let mut captured_error = oracle.clone();
    captured_error.properties.clear();
    captured_error.errors.push(ExpressionEvaluationError {
        composition_id: observed.composition_id(),
        layer_id: observed.layer_id(),
        property: observed.property().clone(),
        message: "native capture failed for this property".into(),
    });
    for captures in [&oracle, &captured_error] {
        let convert = |override_comp: Option<u32>| {
            let mut resolver = |_: &MediaAssetRequest| MediaResolution::Unavailable;
            let mut converter = Converter {
                expression_samples: captures,
                items: project.items.iter().map(|item| (item.id, item)).collect(),
                camera_normalizations: HashMap::new(),
                diagnostics: Vec::new(),
                next_id: 1,
                linked: false,
                asset_namespace: AssetNamespace::STANDALONE,
                stack: Vec::new(),
                visited_compositions: HashSet::new(),
                animations: Vec::new(),
                animation_budget: AnimationBudget::default(),
                committed_inline_remap_bytes: 0,
                unavailable_cutouts: 0,
                overrides: override_comp
                    .into_iter()
                    .map(|source_comp_id| Override {
                        source_comp_id,
                        source_layer_id: u32::MAX,
                        value: OverrideValue::Media {
                            source_id: u32::MAX,
                        },
                    })
                    .collect(),
                media_resolver: &mut resolver,
                assets: Vec::new(),
                shape_budget: shapes::OutputBudget::default(),
                mapped_shape_expressions: Default::default(),
                root_progress: Progress::default().phase(
                    "Expression override scope test",
                    "layers",
                    0,
                ),
            };
            let layers = converter
                .composition_layers(item, fx_schema::LayerId::new(50_000), 0)
                .unwrap();
            (
                serde_json::to_value(layers).unwrap(),
                serde_json::to_value(converter.animations).unwrap(),
                format!("{:?}", converter.diagnostics),
            )
        };
        let unchanged = convert(None);
        assert_eq!(
            convert(Some(u32::MAX)),
            unchanged,
            "unrelated override invalidated source-keyed captures"
        );
        let overridden = convert(Some(item.id));
        assert!(
            overridden
                .2
                .contains("converter-evaluated expression lowered"),
            "same-composition override must evaluate fresh: {overridden:?}"
        );
        if !captures.errors().is_empty() {
            assert_ne!(
                overridden.1, unchanged.1,
                "same-composition override must invalidate captured errors"
            );
        }
    }
}

#[test]
fn expression_lowered_tracks_remove_only_replaced_fallback_warnings() {
    for frame_sampled in [false, true] {
        let name = "Owner: Position";
        let mut observations = samples(vec![vec![0.0], vec![50.0], vec![100.0]]);
        observations.start_ms = 0;
        observations.frame_sampled = frame_sampled;
        observations.sample_times_seconds = if frame_sampled {
            vec![0.0, 0.5, 1.0]
        } else {
            vec![0.0, 0.001, 0.002]
        };
        let (entries, evaluated) = evaluated_numeric_entries(
            name,
            &observations,
            &[target(0, PropType::PositionX)],
            &[],
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries.len(), 1, "{evaluated:?}");
        let unrelated = "Other: enabled AE expression; authored fallback retained".to_owned();
        let failure = format!("{name}: lowering failed; static authored/default value retained");
        let mut warnings = vec![
            format!("{name}: enabled AE expression; authored fallback retained"),
            unrelated.clone(),
            failure.clone(),
        ];
        crate::structure_document::suppress_replaced_expression_warnings(&mut warnings, &evaluated);
        assert_eq!(warnings, vec![unrelated, failure], "{evaluated:?}");
    }
}

#[test]
fn expression_failed_lowering_preserves_fallback_warnings() {
    let name = "Position";
    let observations = samples(vec![vec![0.0], vec![100.0]]);
    let (entries, evaluated) = evaluated_numeric_entries(
        name,
        &observations,
        &[target(1, PropType::PositionY)],
        &[],
        &mut AnimationBudget::default(),
    );
    assert!(entries.is_empty(), "{evaluated:?}");
    assert!(evaluated[0].contains("static authored/default value retained"));
    let fallback = format!("{name}: enabled AE expression; authored fallback retained");
    let mut warnings = vec![fallback.clone()];
    crate::structure_document::suppress_replaced_expression_warnings(&mut warnings, &evaluated);
    assert_eq!(warnings, vec![fallback.clone()]);

    let failed_with_quoted_provenance = vec![format!(
        "{name}: rejected diagnostic text: converter-evaluated expression lowered into 2 editable scalar keys"
    )];
    crate::structure_document::suppress_replaced_expression_warnings(
        &mut warnings,
        &failed_with_quoted_provenance,
    );
    assert_eq!(warnings, vec![fallback]);
}

#[test]
fn sparse_converter_expression_frames_do_not_expand_to_an_unbounded_fitting_grid() {
    let mut observations = samples(vec![vec![0.0], vec![1.0]]);
    observations.frame_sampled = true;
    observations.sample_times_seconds = vec![0.0, 1_000_000.0];
    assert!(fitting_grid(&observations).unwrap_err().contains("exceeds"));
}

fn target(component: usize, property: PropType) -> NumericAnimationTarget {
    NumericAnimationTarget::float(
        PropertyTarget::layer(LayerId::new(3), property),
        component,
        1.0,
    )
}

#[test]
fn unrelated_evaluated_functions_become_sparse_native_keys_without_source_code() {
    for values in [
        (0..=2_000).map(|ms| vec![f64::from(ms) * 0.03]).collect(),
        (0..=2_000)
            .map(|ms| vec![(f64::from(ms) / 400.0).sin() * 12.0])
            .collect(),
        (0..=2_000)
            .map(|ms| vec![(f64::from(ms) / 1_000.0).powi(3)])
            .collect(),
        (0..=2_000)
            .map(|ms| vec![if ms < 1_000 { 0.0 } else { 20.0 }])
            .collect(),
    ] {
        let input = samples(values);
        let (entries, warnings) = evaluated_numeric_entries(
            "arbitrary AE expression",
            &input,
            &[target(0, PropType::PositionX)],
            &[],
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries.len(), 1, "{warnings:?}");
        let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
        assert!(keys.len() < 60);
        assert_eq!(keys[0].layer_time().as_millis(), 2_000);
        assert!(!entries[0].animator.is_js_script());
        fx_schema::AnimationGraph::from_entries(entries).unwrap();
    }
}

#[test]
fn components_offsets_and_coupled_budget_are_independent_of_expression_program() {
    let input = samples(
        (0..=1_000)
            .map(|ms| vec![f64::from(ms) / 100.0, 8.0])
            .collect(),
    );
    let targets = [
        target(0, PropType::PositionX),
        target(1, PropType::PositionY),
    ];
    let (entries, warnings) = evaluated_numeric_entries(
        "dependent vector expression",
        &input,
        &targets,
        &[10.0, -3.0],
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 2, "{warnings:?}");
    assert_eq!(
        entries[0].animator.keyframe_track().unwrap().keyframes()[0].value(),
        &PropertyValue::Float(10.0)
    );
    let constant = entries[1].animator.keyframe_track().unwrap().keyframes();
    assert_eq!(constant.len(), 1);
    assert_eq!(constant[0].value(), &PropertyValue::Float(5.0));
    let mut tiny = AnimationBudget::with_limit(1);
    let (entries, warnings) = evaluated_numeric_entries("vector", &input, &targets, &[], &mut tiny);
    assert!(entries.is_empty());
    assert!(!warnings.is_empty());
    assert_eq!(tiny.used(), 0);
}

#[test]
fn fitted_linear_and_step_segments_keep_their_interpolation() {
    let linear = samples((0..=100).map(|ms| vec![f64::from(ms)]).collect());
    let (entries, warnings) = evaluated_numeric_entries(
        "linear",
        &linear,
        &[target(0, PropType::PositionX)],
        &[],
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 1, "{warnings:?}");
    let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[1].easing(), PropertyKeyframeEasing::Linear);

    let step = samples(
        (0..=100)
            .map(|ms| vec![if ms < 50 { 0.0 } else { 1.0 }])
            .collect(),
    );
    let (entries, warnings) = evaluated_numeric_entries(
        "step",
        &step,
        &[target(0, PropType::PositionX)],
        &[],
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 1, "{warnings:?}");
    assert!(
        entries[0]
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()
            .iter()
            .any(|key| key.easing() == PropertyKeyframeEasing::Hold)
    );
}

#[test]
fn exact_signed_clock_and_target_clamping_failures_are_atomic() {
    let mut exact = samples(vec![vec![7.0]]);
    // FX timestamps must also fit the exact JSON integer range.
    let earliest_json_time = -((1_i64 << 53) - 1);
    exact.start_ms = earliest_json_time;
    let (entries, warnings) = evaluated_numeric_entries(
        "exact clock",
        &exact,
        &[target(0, PropType::PositionX)],
        &[],
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 1, "{warnings:?}");
    assert_eq!(
        entries[0].animator.keyframe_track().unwrap().keyframes()[0]
            .layer_time()
            .as_millis(),
        earliest_json_time
    );
    exact.start_ms = i64::MIN;
    let mut budget = AnimationBudget::default();
    let (entries, warnings) = evaluated_numeric_entries(
        "unrepresentable clock",
        &exact,
        &[target(0, PropType::PositionX)],
        &[],
        &mut budget,
    );
    assert!(entries.is_empty());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("exact JSON integer range"))
    );
    assert_eq!(budget.used(), 0);

    let invalid_opacity = samples(vec![vec![101.0]]);
    let mut budget = AnimationBudget::default();
    let (entries, warnings) = evaluated_numeric_entries(
        "opacity",
        &invalid_opacity,
        &[target(0, PropType::Opacity)],
        &[],
        &mut budget,
    );
    assert!(entries.is_empty());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("invalid FX track"))
    );
    assert_eq!(budget.used(), 0);
}

#[test]
fn irregular_native_timestamps_are_resampled_and_validated_at_actual_times() {
    let mut input = samples(
        (0..=2_000)
            .map(|ms| {
                let seconds = f64::from(ms) / 1_000.0;
                vec![80.0 + 35.0 * (2.0 * std::f64::consts::PI * seconds).sin() + 15.0 * seconds]
            })
            .collect(),
    );
    input.start_ms = 0;
    input.sample_times_seconds = (0..=2_000)
        .map(|ms| (f64::from(ms) / 1_000.0 * 24_576.0).round() / 24_576.0)
        .collect();
    for (value, time) in input.values.iter_mut().zip(&input.sample_times_seconds) {
        value[0] = 80.0 + 35.0 * (2.0 * std::f64::consts::PI * time).sin() + 15.0 * time;
    }
    let curve = fit_component(&input, 0, 1.0, 0.0).expect("irregular native observations fit");
    assert!(curve.keys.len() < input.values.len());
}

#[test]
fn quantized_endpoints_keep_native_samples_inside_the_integer_key_domain() {
    for (start_ms, ticks) in [(1, [25.0, 49.0, 74.0]), (2, [49.0, 74.0, 98.0])] {
        let times = ticks.map(|tick| tick / 24_576.0);
        let mut input = samples(times.iter().map(|time| vec![time * 1_000.0]).collect());
        input.start_ms = start_ms;
        input.sample_times_seconds = times.to_vec();
        let (entries, warnings) = evaluated_numeric_entries(
            "quantized endpoint clock",
            &input,
            &[target(0, PropType::PositionX)],
            &[],
            &mut AnimationBudget::default(),
        );
        assert_eq!(entries.len(), 1, "{warnings:?}");
        let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].layer_time().as_millis(), 1);
        assert_eq!(keys[1].layer_time().as_millis(), 4);
        let curve = fit_component(&input, 0, 1.0, 0.0).unwrap();
        for time in times {
            let actual = fitted_value_at(&curve, time * 1_000.0 - 1.0, &mut 0);
            assert!((actual - time * 1_000.0).abs() <= FIT_TOLERANCE);
        }
    }
}

#[test]
fn narrow_unobserved_pulses_are_never_silently_published_as_constant() {
    let mut values = vec![vec![0.0]; 1_001];
    values[3][0] = 100.0;
    let input = samples(values);
    match fit_component(&input, 0, 1.0, 0.0) {
        Ok(curve) => {
            let mut segment = 0;
            assert_eq!(fitted_value(&curve, 3, &mut segment), 100.0);
        }
        Err(message) => assert!(message.contains("tolerance"), "{message}"),
    }
}

#[test]
fn unsupported_dimensions_are_atomic_and_high_key_counts_are_preserved() {
    let input = samples(vec![vec![1.0]]);
    let (entries, warnings) = evaluated_numeric_entries(
        "wrong dimensions",
        &input,
        &[
            target(0, PropType::PositionX),
            target(1, PropType::PositionY),
        ],
        &[],
        &mut AnimationBudget::default(),
    );
    assert!(entries.is_empty());
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("dimensions"))
    );
    let input = samples((0..=256).map(|ms| vec![f64::from(ms % 2)]).collect());
    let (entries, warnings) = evaluated_numeric_entries(
        "high frequency",
        &input,
        &[target(0, PropType::PositionX)],
        &[],
        &mut AnimationBudget::default(),
    );
    assert_eq!(entries.len(), 1, "{warnings:?}");
    assert!(
        entries[0]
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()
            .len()
            > 128
    );
}

fn patch_layer_record(layer: &mut Layer, offset: usize, bytes: &[u8]) {
    let mut raw = layer.record.encode();
    raw[offset..offset + bytes.len()].copy_from_slice(bytes);
    layer.record = LayerRecord::decode(&raw).expect("mutated test layer record");
}

#[test]
fn sampled_adjustment_position_does_not_consume_ordinary_sibling_budget() {
    const SOURCE: &[u8] = include_bytes!(
        "../../../../tests/fixtures/expression_samples/sampled_position_expression.aep"
    );
    const SAMPLES: &[u8] = include_bytes!(
        "../../../../tests/fixtures/expression_samples/sampled_position_expression.v2.json"
    );
    const ORDINARY_SOURCE_ID: u32 = 17;
    const ADJUSTMENT_SOURCE_ID: u32 = 117;
    const ORDINARY_NAME: &str = "Ordinary sampled Position";
    const ADJUSTMENT_NAME: &str = "Discarded Adjustment sampled Position";

    // Validate the checked-in Adobe sidecar against the untouched pinned source
    // before deriving this CPU-only structural mutation. This derived sibling is
    // supplemental regression coverage, not additional Adobe-native proof.
    let pinned_samples = ExpressionSamples::from_json_for_source(SAMPLES, SOURCE)
        .expect("pinned expression samples match the original AEP bytes");
    assert_eq!(pinned_samples.properties().len(), 1);
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(SAMPLES).expect("pinned expression sidecar JSON");
    let properties = sidecar["properties"]
        .as_array_mut()
        .expect("expression properties");
    let mut adjustment_property = properties[0].clone();
    adjustment_property["layer_id"] = serde_json::json!(ADJUSTMENT_SOURCE_ID);
    properties.push(adjustment_property);
    let derived_sidecar = serde_json::to_vec(&sidecar).expect("derived expression sidecar JSON");
    let samples = ExpressionSamples::from_json_for_source(&derived_sidecar, SOURCE)
        .expect("derived samples remain bound to the original AEP bytes");

    let mut project = read_project(SOURCE).expect("pinned sampled-Position AEP");
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .expect("ExpressionPosition composition");
    let ItemKind::Composition(comp) = &mut item.kind else {
        panic!("ExpressionPosition item must be a composition")
    };
    let ordinary_index = comp
        .layers
        .iter()
        .position(|layer| layer.record.id() == ORDINARY_SOURCE_ID)
        .expect("sampled Position subject");
    comp.layers[ordinary_index].name = ORDINARY_NAME.into();
    let mut adjustment = comp.layers[ordinary_index].clone();
    adjustment.name = ADJUSTMENT_NAME.into();
    patch_layer_record(&mut adjustment, 0, &ADJUSTMENT_SOURCE_ID.to_be_bytes());
    let adjustment_flags = adjustment.record.raw_bytes()[38] | 2;
    patch_layer_record(&mut adjustment, 38, &[adjustment_flags]);
    // The ordinary layer intentionally runs first and consumes its exact budget.
    // A discarded Adjustment Position must not trigger a later global denial.
    comp.layers.insert(ordinary_index + 1, adjustment);

    let mut ordinary_only = project.clone();
    let ordinary_item = ordinary_only
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .expect("ordinary-only composition");
    let ItemKind::Composition(ordinary_comp) = &mut ordinary_item.kind else {
        panic!("ordinary-only item must be a composition")
    };
    ordinary_comp
        .layers
        .retain(|layer| !layer.record.flags().adjustment_layer);
    let ordinary = to_structural_fx_document_with_budget(
        &ordinary_only,
        Some(1),
        &mut |_| crate::structure_document::MediaResolution::Unavailable,
        AnimationBudget::default(),
        &samples,
        crate::structure_document::Destination::Document,
        fx_conv::Progress::default(),
    )
    .expect("ordinary sampled sibling import");
    let converted = to_structural_fx_document_with_budget(
        &project,
        Some(1),
        &mut |_| crate::structure_document::MediaResolution::Unavailable,
        AnimationBudget::with_limit(ordinary.animation_budget_used),
        &samples,
        crate::structure_document::Destination::Document,
        fx_conv::Progress::default(),
    )
    .expect("bounded Adjustment and ordinary sampled import");

    let LayerData::Group(root) = converted.document.composition().layers()[0].data() else {
        panic!("root must be a Group")
    };
    let ordinary_id = root
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            LayerData::Group(group) if group.name == ORDINARY_NAME => Some(group.id),
            _ => None,
        })
        .expect("ordinary sampled sibling");
    let adjustment_id = root
        .layers
        .iter()
        .find_map(|layer| match layer.data() {
            LayerData::Adjustment(adjustment) if adjustment.name == ADJUSTMENT_NAME => {
                Some(adjustment.id)
            }
            _ => None,
        })
        .expect("sampled Adjustment");
    let entries = converted.document.composition().dynamics().entries();
    let ordinary_positions: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry.target.as_property().is_some_and(|property| {
                property.layer_id() == ordinary_id
                    && matches!(
                        property.property_type(),
                        PropType::PositionX | PropType::PositionY
                    )
            })
        })
        .collect();
    assert_eq!(ordinary_positions.len(), 2, "ordinary X/Y tracks retained");
    assert!(ordinary_positions.iter().all(|entry| {
        entry.animator.keyframe_track().is_some() && !entry.animator.is_js_script()
    }));
    assert!(entries.iter().all(|entry| {
        !entry.target.as_property().is_some_and(|property| {
            property.layer_id() == adjustment_id
                && matches!(
                    property.property_type(),
                    PropType::PositionX | PropType::PositionY
                )
        }) && !entry.animator.is_js_script()
    }));
    assert_eq!(
        converted.animation_budget_used, ordinary.animation_budget_used,
        "discarded Adjustment Position must not consume generated-animation budget"
    );
    assert_eq!(
        converted.committed_animation_bytes, ordinary.committed_animation_bytes,
        "only the ordinary sampled sibling may be committed"
    );
    assert!(
        !converted.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(ADJUSTMENT_SOURCE_ID)
                && diagnostic
                    .message
                    .contains("Transform animation omitted at the generated-animation allowance")
        }),
        "discarded Adjustment Position must not cause a global budget denial: {:?}",
        converted.diagnostics
    );
}
