use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_layer_blend_modes_import_as_exact_editable_modes() {
    // Oracle: /tmp/jerboa-aep-import-blend-modes-20260926.jsx + readback status.
    let targets = [
        (1, "NORMAL", "normal"),
        (18, "OVERLAY", "overlay"),
        (34, "SOFT_LIGHT", "softLight"),
        (50, "HARD_LIGHT", "hardLight"),
        (66, "DARKEN", "darken"),
        (82, "LIGHTEN", "lighten"),
        (98, "CLASSIC_DIFFERENCE", "classicDifference"),
        (114, "HUE", "hue"),
        (130, "SATURATION", "saturation"),
        (146, "COLOR", "color"),
        (162, "LUMINOSITY", "luminosity"),
        (178, "CLASSIC_COLOR_DODGE", "classicColorDodge"),
        (194, "CLASSIC_COLOR_BURN", "classicColorBurn"),
        (210, "EXCLUSION", "exclusion"),
        (226, "DIFFERENCE", "difference"),
        (242, "COLOR_DODGE", "colorDodge"),
        (258, "COLOR_BURN", "colorBurn"),
        (274, "LINEAR_BURN", "linearBurn"),
        (290, "LINEAR_LIGHT", "linearLight"),
        (306, "VIVID_LIGHT", "vividLight"),
        (322, "PIN_LIGHT", "pinLight"),
        (338, "HARD_MIX", "hardMix"),
        (354, "LIGHTER_COLOR", "lighterColor"),
        (370, "DARKER_COLOR", "darkerColor"),
        (386, "SUBTRACT", "subtract"),
        (402, "DIVIDE", "divide"),
    ];
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, suffix, expected) in targets {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/compositing/import_remaining_blend_modes.aep",
            id,
            || {
                let project = pinned_project(&BLEND_MODES);
                let converted = fresh_import(&project, id, &format!("IMPORT_BLEND_{suffix}"));
                let foreground = named_group(root(&converted), "blend_foreground");
                assert_eq!(
                    serde_json::to_value(foreground.blend_mode).unwrap(),
                    expected
                );
                let rect = editable_rect(foreground);
                assert_eq!(rect.rect.size, [480.0, 270.0]);
                assert_eq!(rect.rect.fill_color, [0.85, 0.25, 0.15, 1.0]);
                assert_eq!(
                    named_group(root(&converted), "gray_blue_backdrop").blend_mode,
                    fx_schema::BlendMode::Normal
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_track_mattes_keep_mode_reference_and_editable_sources() {
    // Oracle: /tmp/jerboa-aep-import-matte-20260926.jsx + readback status.
    let targets = [
        (1, "ALPHA", "alpha"),
        (20, "ALPHA_INVERTED", "alphaInverted"),
        (38, "LUMA", "luma"),
        (56, "LUMA_INVERTED", "lumaInverted"),
    ];
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, suffix, mode) in targets {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/compositing/import_track_matte_cases.aep",
            id,
            || {
                let project = pinned_project(&TRACK_MATTES);
                let converted = fresh_import(&project, id, &format!("IMPORT_MATTE_{suffix}"));
                let target = named_group(root(&converted), "matted_foreground");
                let matte = target
                    .track_matte
                    .as_ref()
                    .expect("native matte relationship");
                assert_eq!(serde_json::to_value(matte.mode).unwrap(), mode);
                let provider = all_groups(root(&converted))
                    .into_iter()
                    .find(|group| group.id == matte.layer)
                    .unwrap();
                assert_eq!(
                    editable_rect(target).rect.fill_color,
                    [0.75, 0.125, 0.5, 1.0]
                );
                assert_eq!(editable_rect(provider).rect.size, [240.0, 160.0]);
                assert_eq!(
                    provider.transform.position,
                    fx_composition::Position::TwoD([900.0, 530.0])
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_motion_blur_keeps_master_layer_shutter_and_position_keys() {
    // Oracle: /tmp/jerboa-aep-import-motion-blur-20260926.jsx + readback status.
    let targets = [
        (1, "COMP_OFF", false, true, 180.0, 0.0),
        (18, "LAYER_OFF", true, false, 180.0, 0.0),
        (34, "BOTH_ON", true, true, 180.0, 0.0),
        (50, "CUSTOM_SHUTTER", true, true, 270.0, -90.0),
    ];
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, suffix, master, layer, angle, phase) in targets {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/compositing/import_motion_blur_cases.aep",
            id,
            || {
                let project = pinned_project(&MOTION_BLUR);
                let converted = fresh_import(&project, id, &format!("IMPORT_BLUR_{suffix}"));
                let settings = converted.document.composition().motion_blur();
                assert_eq!(settings.enabled, master);
                assert_eq!(settings.shutter_angle.value(), angle);
                assert_eq!(settings.shutter_phase, phase);
                let foreground = named_group(root(&converted), "moving_foreground");
                assert_eq!(foreground.motion_blur, layer);
                assert_eq!(editable_rect(foreground).rect.size, [240.0, 120.0]);
                let json = document_json(&converted);
                let entries = dynamic_entries(&json);
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry["target"]["propertyType"] == "positionX")
                );
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry["target"]["propertyType"] == "positionY")
                );
            },
        );
    }
    cases.finish();
}
