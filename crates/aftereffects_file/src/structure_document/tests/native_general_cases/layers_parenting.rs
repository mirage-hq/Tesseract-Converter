use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coverage_contract_native_adjustment_retains_editable_layer_kind() {
    use sha2::Digest;

    // This historical flag fixture proves only the native switch/identity, not
    // cross-layer effect pixels. A new discriminating render fixture is needed.
    let bytes = include_bytes!("../../../../tests/fixtures/layers/avlayer_flags.aep");
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(bytes)),
        "5710aa1799366be346efa66b42533e64a476d8015c3f46ff40630be2960a775d"
    );
    let project = crate::structure::read_project(bytes).expect("pinned native flags");
    let converted = fresh_import(&project, 1, "adjustmentLayer_true");
    fn adjustment(layers: &[fx_schema::Layer]) -> Option<&fx_schema::AdjustmentLayer> {
        layers.iter().find_map(|layer| match layer.data() {
            FxLayer::Adjustment(layer) => Some(layer),
            FxLayer::Group(group) => adjustment(&group.layers),
            _ => None,
        })
    }
    let layer = adjustment(converted.document.composition().layers()).unwrap_or_else(|| {
        panic!(
            "native adjustment became a plain carrier: {:?}",
            converted.diagnostics
        )
    });
    assert_eq!(layer.name, "TestLayer");
    assert!(!layer.is_hidden);
    assert_eq!(layer.active_range.start.as_millis(), 0);
    assert_eq!(layer.active_range.duration.as_millis(), 10_000);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_precomp_structure_preserves_instances_nesting_and_stack_order() {
    // Oracle: /tmp/aep-author-structure-blend-keyed.jsx.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/layers/import_precomp_structure.aep",
        1,
        || {
            let project = pinned_project(&PRECOMP_STRUCTURE);
            let source = fresh_import(&project, 1, "SHARED_SOURCE");
            assert_eq!(
                editable_rect(named_group(root(&source), "source_red"))
                    .rect
                    .fill_color,
                [0.9, 0.2, 0.1, 1.0]
            );
        },
    );
    cases.run(
        "crates/aftereffects_file/tests/fixtures/layers/import_precomp_structure.aep",
        17,
        || {
            let project = pinned_project(&PRECOMP_STRUCTURE);
            let two = fresh_import(&project, 17, "TWO_SOURCE_INSTANCES");
            let occurrences: Vec<_> = all_groups(root(&two))
                .into_iter()
                .filter(|group| group.name == "SHARED_SOURCE")
                .collect();
            assert_eq!(occurrences.len(), 2);
            assert_ne!(occurrences[0].id, occurrences[1].id);
            assert_eq!(
                occurrences
                    .iter()
                    .map(|group| group.transform.position)
                    .collect::<Vec<_>>(),
                [
                    fx_composition::Position::TwoD([1250.0, 650.0]),
                    fx_composition::Position::TwoD([650.0, 450.0])
                ]
            );
        },
    );
    cases.run(
        "crates/aftereffects_file/tests/fixtures/layers/import_precomp_structure.aep",
        31,
        || {
            let project = pinned_project(&PRECOMP_STRUCTURE);
            let nested = fresh_import(&project, 31, "NESTED_PRECOMP");
            assert_eq!(
                all_groups(root(&nested))
                    .iter()
                    .filter(|group| group.name == "SHARED_SOURCE")
                    .count(),
                2
            );
        },
    );
    cases.run(
        "crates/aftereffects_file/tests/fixtures/layers/import_precomp_structure.aep",
        44,
        || {
            let project = pinned_project(&PRECOMP_STRUCTURE);
            let stacked = fresh_import(&project, 44, "SIBLING_STACKING");
            assert_eq!(
                root(&stacked)
                    .layers
                    .iter()
                    .map(|layer| as_group(layer).name.as_str())
                    .collect::<Vec<_>>(),
                ["top_green", "middle_red", "bottom_blue"]
            );
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_switches_keep_editable_content_and_independent_visibility() {
    // Oracle: /tmp/jerboa-aep-import-switches-20260926.jsx + readback status.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, hidden) in [
        (1, "IMPORT_SWITCH_EYE_OFF", true),
        (18, "IMPORT_SWITCH_SOLO_BACKGROUND", true),
        (34, "IMPORT_SWITCH_GUIDE_FOREGROUND", true),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/layers/import_layer_switches.aep",
            id,
            || {
                let project = pinned_project(&LAYER_SWITCHES);
                let converted = fresh_import(&project, id, name);
                let foreground = named_group(root(&converted), "colored_foreground");
                assert_eq!(foreground.is_hidden, hidden);
                assert_eq!(
                    editable_rect(foreground).rect.fill_color,
                    [0.75, 0.125, 0.25, 1.0]
                );
            },
        );
    }
    for (id, name, enabled) in [
        (51, "IMPORT_SWITCH_AUDIO_ON", true),
        (64, "IMPORT_SWITCH_AUDIO_OFF", false),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/layers/import_layer_switches.aep",
            id,
            || {
                let project = pinned_project(&LAYER_SWITCHES);
                let converted = fresh_import_with_assets(&project, id, name);
                let json = document_json(&converted);
                assert!(json.to_string().contains("audio-mono.wav"));
                assert_eq!(json.to_string().contains("\"isHidden\":true"), !enabled);
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_timing_controls_keep_authored_ranges_and_clock_tracks() {
    // Oracle: /tmp/aep-author-batch2.jsx/status: start/in/out/stretch/reverse values.
    let project = pinned_project(&TIMING_CONTROLS);
    for (id, name) in [
        (1, "TIMING_START"),
        (18, "TIMING_IN"),
        (34, "TIMING_OUT"),
        (50, "TIMING_STRETCH"),
        (66, "TIMING_REVERSE"),
    ] {
        let converted = fresh_import(&project, id, name);
        let target = named_group(root(&converted), "timed_target");
        assert_eq!(editable_rect(target).rect.size, [480.0, 270.0]);
        assert!(!dynamic_entries(&document_json(&converted)).is_empty());
        if id == 66 {
            assert!(target.playback.time_remap().is_some());
        }
    }
    // Reverse source clocks can expose black reference intervals; no visual pass is claimed.
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_parenting_keeps_transform_ancestors_and_child_pixels() {
    // Oracle: /tmp/jerboa-aep-import-parenting-20260926.jsx + readback status.
    let targets = [
        (
            1,
            "IMPORT_PARENT_POSITION",
            "parent_position",
            [870.0, 520.0],
            [100.0, 100.0],
            0.0,
        ),
        (
            20,
            "IMPORT_PARENT_SCALE",
            "parent_scale",
            [960.0, 540.0],
            [150.0, 75.0],
            0.0,
        ),
        (
            38,
            "IMPORT_PARENT_ROTATION",
            "parent_rotation",
            [960.0, 540.0],
            [100.0, 100.0],
            22.0,
        ),
        (
            56,
            "IMPORT_PARENT_CHAIN",
            "parent_chain",
            [60.0, 0.0],
            [100.0, 100.0],
            0.0,
        ),
        (
            76,
            "IMPORT_PARENT_SOLID",
            "parent_solid",
            [960.0, 540.0],
            [100.0, 100.0],
            0.0,
        ),
    ];
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, comp, parent_name, position, scale, rotation) in targets {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/parenting/import_parenting_cases.aep",
            id,
            || {
                let project = pinned_project(&PARENTING);
                let converted = fresh_import(&project, id, comp);
                let parent = named_group(root(&converted), parent_name);
                assert_eq!(
                    parent.transform.position,
                    fx_composition::Position::TwoD(position)
                );
                assert_eq!(parent.transform.scale, scale);
                assert_eq!(parent.transform.rotation, rotation);
                let child = all_groups(parent)
                    .into_iter()
                    .find(|group| group.name.starts_with("child_"))
                    .unwrap();
                assert_eq!(editable_rect(child).rect.size, [240.0, 120.0]);
                assert_eq!(
                    editable_rect(child).rect.fill_color,
                    [0.75, 0.125, 0.25, 1.0]
                );
            },
        );
    }
    cases.finish();
}
