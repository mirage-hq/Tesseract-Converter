use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_static_transform_components_keep_exact_editable_values() {
    // Oracle: /tmp/jerboa-aep-import-transform-components-20260926.jsx + status JSON.
    let targets = [
        (
            1,
            "ANCHOR_X",
            [90.0, 60.0],
            [960.0, 540.0],
            [100.0, 100.0],
            0.0,
            100.0,
        ),
        (
            18,
            "ANCHOR_Y",
            [120.0, 45.0],
            [960.0, 540.0],
            [100.0, 100.0],
            0.0,
            100.0,
        ),
        (
            34,
            "POSITION_X",
            [120.0, 60.0],
            [830.0, 540.0],
            [100.0, 100.0],
            0.0,
            100.0,
        ),
        (
            50,
            "POSITION_Y",
            [120.0, 60.0],
            [960.0, 425.0],
            [100.0, 100.0],
            0.0,
            100.0,
        ),
        (
            66,
            "SCALE_X",
            [120.0, 60.0],
            [960.0, 540.0],
            [135.0, 100.0],
            0.0,
            100.0,
        ),
        (
            82,
            "SCALE_Y",
            [120.0, 60.0],
            [960.0, 540.0],
            [100.0, 70.0],
            0.0,
            100.0,
        ),
        (
            98,
            "ROTATION_Z",
            [120.0, 60.0],
            [960.0, 540.0],
            [100.0, 100.0],
            35.0,
            100.0,
        ),
        (
            114,
            "OPACITY",
            [120.0, 60.0],
            [960.0, 540.0],
            [100.0, 100.0],
            0.0,
            60.0,
        ),
        (
            130,
            "SEPARATED_POSITION_X",
            [120.0, 60.0],
            [850.0, 540.0],
            [100.0, 100.0],
            0.0,
            100.0,
        ),
        (
            146,
            "SEPARATED_POSITION_Y",
            [120.0, 60.0],
            [960.0, 470.0],
            [100.0, 100.0],
            0.0,
            100.0,
        ),
    ];
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, suffix, anchor, position, scale, rotation, opacity) in targets {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/properties/import_transform_components.aep",
            id,
            || {
                let project = pinned_project(&TRANSFORM_COMPONENTS);
                let converted = fresh_import(&project, id, &format!("IMPORT_TRANSFORM_{suffix}"));
                let target = named_group(root(&converted), "single_transform_target");
                assert_eq!(target.transform.anchor_point, anchor);
                assert_eq!(
                    target.transform.position,
                    fx_composition::Position::TwoD(position)
                );
                assert_eq!(target.transform.scale, scale);
                assert_eq!(target.transform.rotation, rotation);
                assert_eq!(target.transform.opacity.value(), opacity);
                assert_eq!(editable_rect(target).rect.size, [240.0, 120.0]);
            },
        );
    }

    let targets_3d = [
        (
            162,
            "3D_POSITION_Z",
            [960.0, 540.0, 160.0],
            0.0,
            0.0,
            [0.0, 0.0, 0.0],
        ),
        (
            178,
            "3D_ROTATION_X",
            [960.0, 540.0, 0.0],
            25.0,
            0.0,
            [0.0, 0.0, 0.0],
        ),
        (
            194,
            "3D_ROTATION_Y",
            [960.0, 540.0, 0.0],
            0.0,
            -20.0,
            [0.0, 0.0, 0.0],
        ),
        (
            210,
            "3D_ORIENTATION_X",
            [960.0, 540.0, 0.0],
            0.0,
            0.0,
            [20.0, 0.0, 0.0],
        ),
        (
            226,
            "3D_ORIENTATION_Y",
            [960.0, 540.0, 0.0],
            0.0,
            0.0,
            [0.0, 15.0, 0.0],
        ),
        (
            242,
            "3D_ORIENTATION_Z",
            [960.0, 540.0, 0.0],
            0.0,
            0.0,
            [0.0, 0.0, 22.0],
        ),
    ];
    for (id, suffix, position, rx, ry, orientation) in targets_3d {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/properties/import_transform_components.aep",
            id,
            || {
                let project = pinned_project(&TRANSFORM_COMPONENTS);
                let converted = fresh_import(&project, id, &format!("IMPORT_TRANSFORM_{suffix}"));
                let transform = named_group(root(&converted), "single_transform_target").transform;
                assert_eq!(
                    transform.position,
                    fx_composition::Position::ThreeD(position)
                );
                assert_eq!(transform.rotation_x, rx);
                assert_eq!(transform.rotation_y, ry);
                assert_eq!(transform.orientation, orientation);
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_numeric_animation_cases_have_exact_targeted_two_key_tracks() {
    // Oracle: /tmp/aep-author-remaining-batch.jsx; values are authored at 0.5s and 2s.
    let targets = [
        (1, "OPACITY_LINEAR", "opacity"),
        (18, "OPACITY_HOLD", "opacity"),
        (34, "OPACITY_BEZIER", "opacity"),
        (50, "KEYED_ANCHOR", "anchorPointX"),
        (66, "KEYED_POSITION", "positionX"),
        (82, "KEYED_SCALE", "scaleX"),
        (98, "KEYED_ROTATION_Z", "rotation"),
        (114, "KEYED_ROTATION_X", "rotationX"),
        (130, "KEYED_ROTATION_Y", "rotationY"),
        (146, "KEYED_ORIENTATION_X", "orientationX"),
        (162, "KEYED_ORIENTATION_Y", "orientationY"),
        (178, "KEYED_ORIENTATION_Z", "orientationZ"),
        (194, "KEYED_POSITION_Z", "positionZ"),
        (210, "SEPARATED_KEYED_X", "positionX"),
        (226, "SEPARATED_KEYED_Y", "positionY"),
        (242, "SEPARATED_KEYED_Z", "positionZ"),
        (258, "SPATIAL_BEZIER_POSITION", "positionX"),
    ];
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, property) in targets {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/properties/import_numeric_animation_cases.aep",
            id,
            || {
                let project = pinned_project(&NUMERIC_ANIMATION);
                let converted = fresh_import(&project, id, name);
                let json = document_json(&converted);
                let entry = entry_for(dynamic_entries(&json), property);
                let keys = entry["animator"]["keyframes"].as_array().unwrap();
                assert_eq!(keys.len(), 2, "{name} {property}");
                assert_eq!(keys[0]["layerTime"], 500, "{name}");
                assert_eq!(keys[1]["layerTime"], 2000, "{name}");
                assert_eq!(
                    editable_rect(named_group(root(&converted), "target"))
                        .rect
                        .size,
                    [480.0, 270.0]
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_temporal_clocks_rebase_tracks_without_claiming_reverse_visuals() {
    // Oracle: /tmp/aep-author-image-gradient-clocks.jsx.
    let project = pinned_project(&TEMPORAL_CLOCKS);
    for (id, name) in [
        (1, "CLOCK_IDENTITY"),
        (16, "CLOCK_START_STRETCH"),
        (30, "CLOCK_REVERSE_BEZIER"),
        (44, "CLOCK_SHAPE_START_STRETCH"),
        (57, "CLOCK_MASK_START_STRETCH"),
    ] {
        let converted = fresh_import(&project, id, name);
        assert!(
            !dynamic_entries(&document_json(&converted)).is_empty(),
            "{name}"
        );
        if id == 30 {
            assert!(
                diagnostic_contains(&converted, "reverse")
                    || diagnostic_contains(&converted, "negative")
            );
        }
    }
    // The reverse-clock Adobe reference contains black intervals; this is structural clock proof,
    // deliberately not a visual-pass assertion.
}
