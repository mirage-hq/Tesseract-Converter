use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_masks_keep_exact_modes_controls_and_key_tracks() {
    // Oracle: /tmp/aep-author-remaining-batch.jsx.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    let modes = [
        (1, "MASK_ADD", "add"),
        (18, "MASK_SUBTRACT", "subtract"),
        (34, "MASK_INTERSECT", "intersect"),
        (50, "MASK_DIFFERENCE", "difference"),
        (66, "MASK_LIGHTEN", "lighten"),
        (82, "MASK_DARKEN", "darken"),
        (98, "MASK_NONE", "none"),
    ];
    for (id, name, mode) in modes {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/masks/import_mask_controls.aep",
            id,
            || {
                let project = pinned_project(&MASK_CONTROLS);
                let converted = fresh_import(&project, id, name);
                let target = named_group(root(&converted), "target");
                assert_eq!(target.masks.len(), 1);
                assert_eq!(serde_json::to_value(target.masks[0].mode).unwrap(), mode);
                assert!(target.masks[0].layer.is_some());
                assert_eq!(editable_rect(target).rect.size, [480.0, 270.0]);
            },
        );
    }
    for (id, name) in [
        (114, "MASK_INVERTED"),
        (130, "MASK_FEATHER"),
        (146, "MASK_OPACITY"),
        (162, "MASK_EXPANSION"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/masks/import_mask_controls.aep",
            id,
            || {
                let project = pinned_project(&MASK_CONTROLS);
                let converted = fresh_import(&project, id, name);
                let mask = &named_group(root(&converted), "target").masks[0];
                match id {
                    114 => assert!(mask.inverted),
                    130 => assert_eq!(mask.feather, [24.0, 12.0]),
                    146 => assert_eq!(mask.opacity.value(), 0.55),
                    162 => assert_eq!(mask.expansion, 24.0),
                    _ => unreachable!(),
                }
            },
        );
    }
    for (id, name, target) in [
        (178, "MASK_FEATHER_KEYED", "feather"),
        (194, "MASK_OPACITY_KEYED", "opacity"),
        (210, "MASK_EXPANSION_KEYED", "expansion"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/masks/import_mask_controls.aep",
            id,
            || {
                let project = pinned_project(&MASK_CONTROLS);
                let converted = fresh_import(&project, id, name);
                let json = document_json(&converted);
                let entry = dynamic_entries(&json)
                    .iter()
                    .find(|entry| entry["target"]["propertyName"] == target)
                    .unwrap();
                assert_eq!(entry["animator"]["keyframes"].as_array().unwrap().len(), 2);
                let initial = match id {
                    178 => serde_json::json!([0.0, 0.0]),
                    194 => serde_json::json!(1.0),
                    210 => serde_json::json!(0.0),
                    _ => unreachable!(),
                };
                super::super::adobe_feature_additions::assert_mask_key_owner(
                    &converted, target, initial,
                );
            },
        );
    }
    cases.run(
        "crates/aftereffects_file/tests/fixtures/masks/import_mask_controls.aep",
        226,
        || {
            let project = pinned_project(&MASK_CONTROLS);
            let keyed = fresh_import(&project, 226, "MASK_PATH_KEYED");
            assert_eq!(named_group(root(&keyed), "target").masks.len(), 1);
            let guide = named_group(root(&keyed), "target").masks[0].layer.unwrap();
            assert!(keyed.document.composition().dynamics().entries().iter().any(|entry|
                entry.target == fx_schema::PropertyTarget::layer(guide, fx_schema::PropType::ShapePath)
                    && matches!(entry.animator.data(), fx_schema::animator::AnimatorData::Keyframes { track, .. } if track.keyframes().len() == 2)));

        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_shape_path_keys_retain_editable_motion_without_scripts() {
    // Historical independently authored source; Adobe proof backlog stays explicit.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name) in [(1, "PATH_LINEAR"), (17, "PATH_HOLD"), (32, "PATH_BEZIER")] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/path-animation/import_path_key_cases.aep",
            id,
            || {
                let project = pinned_project(&PATH_KEYS);
                let converted = fresh_import(&project, id, name);
                let shapes = root(&converted)
                    .layers
                    .iter()
                    .filter(|layer| matches!(layer.data(), FxLayer::Shape(_)))
                    .count()
                    + all_groups(root(&converted))
                        .iter()
                        .flat_map(|group| group.layers.iter())
                        .filter(|layer| matches!(layer.data(), FxLayer::Shape(_)))
                        .count();
                assert!(shapes > 0, "{name} keeps an editable initial outline");
                assert!(converted.document.composition().dynamics().entries().iter().any(|entry|
                    entry.target.as_property().is_some_and(|target| target.property_type() == fx_schema::PropType::ShapePath)
                        && matches!(entry.animator.data(), fx_schema::animator::AnimatorData::Keyframes { track, .. } if track.keyframes().len() == 2)));
                assert!(!document_json(&converted).to_string().contains("jsScript"));
            },
        );
    }
    cases.finish();
}
