use super::review_regressions::{review_text_layer, review_text_path_guide};
use super::*;

fn with_lowerer(document: &EditableFxCompositionDocument, check: impl FnOnce(&mut Lowerer<'_>)) {
    let dynamics = AnimationIndex::new(document.composition().dynamics().entries());
    let resolved_media = BTreeMap::new();
    let rate = crate::timing::FrameRate::new(30.0).unwrap();
    let (_, duration) = rate.duration(document.duration().as_millis()).unwrap();
    let roots = document.composition().layers();
    let precompositions = source_variant_precompositions(document, roots, &resolved_media).unwrap();
    let preflight = source_variants::preflight_layers(document, roots, &precompositions).unwrap();
    let mut lowerer = Lowerer {
        rate,
        duration,
        end: Time::ZERO.saturating_add(document.duration()),
        dynamics: &dynamics,
        dimensions: document.dimensions(),
        logical_dimensions: document.dimensions(),
        mosaic_root_adjustments: roots
            .iter()
            .filter(|layer| {
                document
                    .background_color()
                    .is_none_or(|color| color[3] == 0.0)
                    && matches!(layer.data(), LayerData::Adjustment(_))
            })
            .map(Layer::id)
            .collect(),
        mosaic_cross_layer_inputs: super::super::mosaic_domain::has_cross_layer_inputs(roots),
        resolved_media: &resolved_media,
        fonts: None,
        composition_options: composition_options::from_motion_blur(
            document.composition().motion_blur(),
        )
        .unwrap(),
        occupied_ids: id_reservations::IdReservations::new(preflight.occupied_ids),
        source_variant_eligibility: preflight.eligibility,
        source_variants: preflight.decisions,
        consumed_guides: id_reservations::IdReservations::new(BTreeSet::new()),
        inside_precomposition: false,
        layers: Vec::new(),
        diagnostics: Vec::new(),
        omitted_layer_ids: BTreeSet::new(),
    };
    check(&mut lowerer);
}

#[test]
fn guide_free_nested_inline_masks_do_not_require_native_probe() {
    let mut value = imported();
    let mut child = rect(&value, 6_500);
    child["masks"] = json!([{
        "id": 16_500, "mode": "add", "inverted": false,
        "path": {"commands": [
            {"type": "moveTo", "x": 0.0, "y": 0.0},
            {"type": "lineTo", "x": 40.0, "y": 0.0},
            {"type": "lineTo", "x": 40.0, "y": 40.0},
            {"type": "close"}
        ]},
        "feather": [0.0, 0.0], "expansion": 0.0, "opacity": 1.0
    }]);
    let group_id = value["composition"]["layers"][0]["id"].clone();
    child["parent"] = group_id;
    value["composition"]["layers"][0]["layers"] = json!([child]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let roots = document.composition().layers();
    assert!(!roots.iter().any(has_source_guide_reference));
    with_lowerer(&document, |lowerer| {
        let expected = lowerer.probe_consumed_guides(roots);
        assert!(expected.is_empty());
        assert_eq!(lowerer.collect_consumed_guides(roots), expected);
    });
}

#[test]
fn nested_mask_references_keep_native_guide_probe() {
    let mut value = imported();
    let mut child = rect(&value, 6_500);
    child["parent"] = value["composition"]["layers"][0]["id"].clone();
    // Even a missing guide must retain the original best-effort probe path.
    child["masks"] = json!([{
        "id": 16_500, "layer": 6_501, "mode": "add", "inverted": false,
        "feather": [0.0, 0.0], "expansion": 0.0, "opacity": 1.0
    }]);
    value["composition"]["layers"][0]["layers"] = json!([child]);
    value["composition"]["dynamics"] = json!({"entries": []});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let roots = document.composition().layers();
    assert!(roots.iter().any(has_source_guide_reference));
    with_lowerer(&document, |lowerer| {
        assert_eq!(
            lowerer.collect_consumed_guides(roots),
            lowerer.probe_consumed_guides(roots)
        );
    });
}

#[test]
fn text_paths_keep_native_guide_probe_and_exact_consumption() {
    for hidden in [false, true] {
        let mut value = imported();
        value["composition"]["layers"] = json!([
            review_text_path_guide(&value, 6_500),
            review_text_layer(&value, 6_501, "Guide probe owner", hidden, Some(6_500))
        ]);
        value["composition"]["dynamics"] = json!({"entries": []});
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        let roots = document.composition().layers();
        assert!(roots.iter().any(has_source_guide_reference));
        with_lowerer(&document, |lowerer| {
            let expected = lowerer.probe_consumed_guides(roots);
            assert_eq!(expected.contains(&LayerId::new(6_500)), !hidden);
            assert_eq!(lowerer.collect_consumed_guides(roots), expected);
        });
    }
}

#[test]
fn failed_text_owner_restores_guide_without_erasing_successful_consumption() {
    let mut value = imported();
    let first_guide = review_text_path_guide(&value, 6_300);
    let first_owner = review_text_layer(&value, 6_301, "Successful owner", false, Some(6_300));
    let failed_guide = review_text_path_guide(&value, 6_400);
    let mut failed_owner = review_text_layer(&value, 6_401, "Failed owner", false, Some(6_400));
    failed_owner["sourceText"]["fontSize"] = json!(f64::MAX);
    failed_owner["sourceText"]["leading"] = Value::Null;
    value["composition"]["layers"] = json!([first_guide, first_owner, failed_guide, failed_owner]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    with_lowerer(&document, |lowerer| {
        let roots = document.composition().layers();
        let demand = hierarchy::root_demand(document.dimensions(), document.duration().as_millis());
        lowerer.layer(&roots[1], None, None, roots, 0, true, &demand);
        assert_eq!(
            lowerer.consumed_guides.as_set(),
            &BTreeSet::from([LayerId::new(6_300)])
        );
        // Prove that the exact failed input reaches guide consumption before
        // Source Text validation, rather than rejecting its mask up front.
        let checkpoint = lowerer.consumed_guides.checkpoint();
        lowerer
            .prepare_layer_options(&roots[3], None, None, roots, lowerer.dynamics, true)
            .unwrap();
        assert!(lowerer.consumed_guides.contains(&LayerId::new(6_400)));
        lowerer.consumed_guides.rollback(checkpoint);
        let prior_layers = lowerer.layers.len();
        lowerer.layer(&roots[3], None, None, roots, 0, true, &demand);
        assert_eq!(lowerer.layers.len(), prior_layers);
        assert!(
            lowerer
                .diagnostics
                .last()
                .unwrap()
                .message
                .contains("invalid Source Text style value")
        );
        assert_eq!(
            lowerer.consumed_guides.as_set(),
            &BTreeSet::from([LayerId::new(6_300)])
        );
        lowerer.layer(&roots[2], None, None, roots, 0, true, &demand);
        assert!(
            lowerer.layers.len() > prior_layers,
            "failed owner's guide remains paintable"
        );
        let after_guide = lowerer.layers.len();
        lowerer.layer(&roots[0], None, None, roots, 0, true, &demand);
        assert_eq!(
            lowerer.layers.len(),
            after_guide,
            "successful guide remains consumed"
        );
        assert_eq!(
            lowerer.omitted_layer_ids,
            BTreeSet::from([LayerId::new(6_401)])
        );
    });
}

#[test]
fn mixed_text_fallback_reuses_skew_helper_reserved_by_failed_attempt() {
    let mut value = mixed_scene_with_text_branch();
    value["composition"]["layers"][0]["transform"]["skew"] = json!(15.0);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    with_lowerer(&document, |lowerer| {
        let roots = document.composition().layers();
        let LayerData::Group(group) = roots[0].data() else {
            panic!("mixed scene Group")
        };
        let demand = hierarchy::root_demand(document.dimensions(), document.duration().as_millis());
        let occupied_checkpoint = lowerer.occupied_ids.checkpoint();
        let diagnostics_before = lowerer.diagnostics.len();
        let options = native_layer_options(&roots[0]).unwrap();
        assert_eq!(
            lowerer.lower_group_original(group, options, roots, 0, true, &demand, false),
            Err("Text/font glyph bounds are not known from the FX text box")
        );
        assert!(lowerer.occupied_ids.contains(&LayerId::new(60_001)));
        assert!(lowerer.layers.is_empty());
        lowerer.occupied_ids.rollback(occupied_checkpoint);
        lowerer.diagnostics.truncate(diagnostics_before);
        let before = lowerer.occupied_ids.as_set().clone();
        lowerer.layer(&roots[0], None, None, roots, 0, true, &demand);
        assert!(!lowerer.layers.is_empty(), "{:?}", lowerer.diagnostics);
        let added: BTreeSet<_> = lowerer
            .occupied_ids
            .as_set()
            .difference(&before)
            .copied()
            .collect();
        assert_eq!(added, BTreeSet::from([LayerId::new(60_001)]));
        assert!(lowerer.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(60_060))
                && diagnostic.message.contains("Text-only branch omitted")
        }));
        assert!(lowerer.omitted_layer_ids.contains(&LayerId::new(60_061)));
        assert!(!lowerer.omitted_layer_ids.contains(&LayerId::new(60_000)));
    });
}

#[test]
fn takeover_ids_skip_collisions_and_fail_without_partial_reservations_at_overflow() {
    let value = imported();
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    with_lowerer(&document, |lowerer| {
        lowerer
            .occupied_ids
            .extend([LayerId::new(101), LayerId::new(103)]);
        let before = lowerer.occupied_ids.as_set().clone();
        let ids = lowerer.next_takeover_ids(LayerId::new(100)).unwrap();
        assert_eq!(ids.inner_footage, LayerId::new(102));
        assert_eq!(ids.slide_parent, LayerId::new(104));
        assert_eq!(lowerer.occupied_ids.as_set(), &before);
        // One candidate is available, but allocating the second must overflow.
        assert!(matches!(
            lowerer.next_takeover_ids(LayerId::new(u64::MAX - 1)),
            Err("Takeover synthetic identity space is exhausted")
        ));
        assert!(matches!(
            lowerer.next_takeover_ids(LayerId::new(u64::MAX)),
            Err("Takeover synthetic identity space is exhausted")
        ));
        assert_eq!(lowerer.occupied_ids.as_set(), &before);
    });
}
