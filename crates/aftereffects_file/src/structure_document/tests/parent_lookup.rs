//! Differential CPU regressions: mutated models are supplementary, not Adobe proof.
use std::{hint::black_box, time::Instant};

use super::*;

fn converter<'a>(
    samples: &'a ExpressionSamples,
    resolver: &'a mut dyn FnMut(&MediaAssetRequest) -> MediaResolution,
) -> Converter<'a> {
    Converter {
        expression_samples: samples,
        items: HashMap::new(),
        camera_normalizations: HashMap::new(),
        diagnostics: Vec::new(),
        next_id: 1,
        linked: false,
        asset_namespace: AssetNamespace::STANDALONE,
        stack: Vec::new(),
        visited_compositions: HashSet::new(),
        animations: Vec::new(),
        animation_budget: animation_budget::AnimationBudget::default(),
        committed_inline_remap_bytes: 0,
        unavailable_cutouts: 0,
        overrides: Vec::new(),
        media_resolver: resolver,
        assets: Vec::new(),
        shape_budget: shapes::OutputBudget::default(),
        mapped_shape_expressions: Default::default(),
        root_progress: Progress::default().phase("parent lookup probe", "layers", 0),
    }
}

// Frozen pre-index implementation, intentionally independent of the actual lookup.
fn linear_ancestors<'a>(
    converter: &mut Converter<'_>,
    context: &LayerContext<'a>,
    layer: &Layer,
) -> Vec<&'a Layer> {
    let mut ancestors = Vec::new();
    let mut parent_id = layer.record.parent_id();
    let mut seen = HashSet::from([layer.record.id()]);
    while parent_id != 0 {
        if !seen.insert(parent_id) || ancestors.len() + context.depth + 2 >= MAX_GROUP_DEPTH {
            converter.warn(
                Limitation::Parenting,
                Some(context.comp_id),
                Some(layer.record.id()),
                "cyclic or over-depth transform-parent chain omitted".into(),
            );
            return Vec::new();
        }
        let Some(parent) = context
            .comp
            .layers
            .iter()
            .find(|parent| parent.record.id() == parent_id)
        else {
            converter.warn(
                Limitation::Parenting,
                Some(context.comp_id),
                Some(layer.record.id()),
                format!("missing transform parent {parent_id}; known part of chain retained"),
            );
            break;
        };
        ancestors.push(parent);
        parent_id = parent.record.parent_id();
    }
    ancestors
}

fn layers(comp: &mut Composition, edges: &[(u32, u32)]) {
    let mut template = comp.layers[0].clone();
    // Ancestry lookup needs only native records; do not multiply property payloads.
    template.content.clear();
    comp.layers = edges
        .iter()
        .map(|&(id, parent)| {
            let mut layer = template.clone();
            patch(&mut layer, 0, &id.to_be_bytes());
            patch(&mut layer, 132, &parent.to_be_bytes());
            layer
        })
        .collect();
}

#[test]
fn parent_lookup_matches_frozen_linear_ancestry_and_ordered_diagnostics() {
    let mut project = fixture("layer_misc.aep");
    let comp = composition_mut(&mut project, 44);
    let cases = [
        (vec![(1, 0)], 0, vec![]),
        (vec![(1, 99)], 0, vec![]),
        (vec![(1, 2), (2, 99)], 0, vec![1]),
        (vec![(1, 1)], 0, vec![]),
        (vec![(1, 2), (2, 3), (3, 1)], 0, vec![]),
        (vec![(1, 2), (2, 3), (2, 0), (3, 0)], 0, vec![1, 3]),
        (vec![(1, 2), (3, 0), (2, 3)], 0, vec![2, 1]),
        (vec![(1, 2), (2, 0)], MAX_GROUP_DEPTH - 3, vec![1]),
        (vec![(1, 2), (2, 0)], MAX_GROUP_DEPTH - 2, vec![]),
        (vec![(1, 2), (2, 3), (3, 0)], MAX_GROUP_DEPTH - 3, vec![]),
    ];
    let samples = ExpressionSamples::default();
    let mut actual_resolver = |_: &MediaAssetRequest| MediaResolution::Unavailable;
    let mut oracle_resolver = |_: &MediaAssetRequest| MediaResolution::Unavailable;
    let mut actual = converter(&samples, &mut actual_resolver);
    let mut oracle = converter(&samples, &mut oracle_resolver);
    for (edges, depth, expected_indices) in cases {
        layers(comp, &edges);
        let layer_indices = index_layers(&comp.layers);
        let context = LayerContext {
            expression_samples: &samples,
            comp_id: 44,
            comp,
            parent: LayerId::new(1),
            depth,
            solo: false,
            layer_indices: &layer_indices,
            camera_normalization: None,
        };
        let found = actual.transform_ancestors(&context, &comp.layers[0]);
        let expected = linear_ancestors(&mut oracle, &context, &comp.layers[0]);
        assert_eq!(
            found.len(),
            expected_indices.len(),
            "{edges:?}, depth={depth}"
        );
        assert_eq!(found.len(), expected.len());
        for ((found, expected), index) in found.iter().zip(expected).zip(expected_indices) {
            assert!(std::ptr::eq(*found, expected));
            assert!(
                std::ptr::eq(*found, &comp.layers[index]),
                "original first-match reference"
            );
        }
        // Compare the entire accumulated ordered stream, including every context/message.
        assert_eq!(actual.diagnostics, oracle.diagnostics);
    }
    assert_eq!(actual.diagnostics.len(), 6);
}

#[test]
fn parent_lookup_bounded_wide_cpu_timings() {
    let mut project = fixture("layer_misc.aep");
    let comp = composition_mut(&mut project, 44);
    let samples = ExpressionSamples::default();
    let mut resolver = |_: &MediaAssetRequest| MediaResolution::Unavailable;
    let mut converter = converter(&samples, &mut resolver);
    for count in [1024_u32, 4096] {
        for parented in [true, false] {
            let edges: Vec<_> = (1..=count)
                .map(|id| (id, if parented && id != count { count } else { 0 }))
                .collect();
            layers(comp, &edges);
            let membership_start = Instant::now();
            let layer_indices = index_layers(&comp.layers);
            let membership_elapsed = membership_start.elapsed();
            let context = LayerContext {
                expression_samples: &samples,
                comp_id: 44,
                comp,
                parent: LayerId::new(1),
                depth: 0,
                solo: false,
                layer_indices: &layer_indices,
                camera_normalization: None,
            };
            for repetition in 1..=3 {
                let start = Instant::now();
                let mut ancestor_count = 0;
                for layer in &comp.layers {
                    ancestor_count += black_box(
                        converter.transform_ancestors(black_box(&context), black_box(layer)),
                    )
                    .len();
                }
                let elapsed = start.elapsed();
                assert_eq!(
                    ancestor_count,
                    if parented {
                        usize::try_from(count - 1).unwrap()
                    } else {
                        0
                    }
                );
                println!(
                    "parent_lookup layers={count} parented={parented} repetition={repetition} membership_build={membership_elapsed:?} traversal={elapsed:?}"
                );
            }
        }
    }
    assert!(converter.diagnostics.is_empty());
}

#[test]
fn parent_lookup_native_fresh_conversion_fingerprints() {
    let diagnostic_bytes = |diagnostics: &[ImportDiagnostic]| {
        let records: Vec<_> = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.limitation.code(),
                    diagnostic.composition_id,
                    diagnostic.layer_id,
                    &diagnostic.message,
                )
            })
            .collect();
        serde_json::to_vec(&records).unwrap()
    };
    // Feature-specific editable parenting assertions for these exact targets remain in
    // native_general_cases::layers_parenting::grouped_native_parenting_keeps_transform_ancestors_and_child_pixels.
    // Essential override behavior is covered by essential_overrides_are_applied_during_fresh_occurrence_conversion.
    for (file, targets) in [
        ("parenting/import_parenting_cases.aep", &[1_u32, 56][..]),
        ("essential/multiple_controllers.aep", &[16_u32][..]),
    ] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file);
        let bytes = fs::read(&path).unwrap();
        let source_hash = format!("{:x}", Sha256::digest(&bytes));
        for &target in targets {
            let project = read_project(&bytes).unwrap();
            let converted = to_structural_fx_document(&project, Some(target)).unwrap();
            assert_imported_canvas_matches_source(composition(&project, target), &converted, file);
            assert!(
                !root(&converted).layers.is_empty(),
                "{file} target={target}"
            );
            let document = serde_json::to_vec(&converted.document).unwrap();
            let diagnostics = diagnostic_bytes(&converted.diagnostics);
            let fresh =
                to_structural_fx_document(&read_project(&bytes).unwrap(), Some(target)).unwrap();
            assert_eq!(document, serde_json::to_vec(&fresh.document).unwrap());
            assert_eq!(diagnostics, diagnostic_bytes(&fresh.diagnostics));
            println!(
                "parent_lookup native={file} target={target} source_sha256={source_hash} document_sha256={:x} ordered_diagnostics_sha256={:x}",
                Sha256::digest(&document),
                Sha256::digest(&diagnostics)
            );
        }
    }
}
