use super::*;

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_pixel_motion_keeps_optical_flow_and_contextual_approximation_diagnostic() {
    let project = pinned_project(&AUDIO_MEDIA);
    let converted = fresh_import_with_assets(&project, 139, "VIDEO_FRAME_BLEND_2");
    let root = root(&converted);
    assert_eq!(root.playback.input_range().start, Time::ZERO);
    assert_eq!(
        root.playback.input_range().duration,
        Duration::from_secs(3.0)
    );

    let occurrence = named_group(root, "feature_rate_24_blue.mp4");
    assert_eq!(occurrence.playback.input_range().start, Time::ZERO);
    assert_eq!(
        occurrence.playback.input_range().duration,
        Duration::from_secs(3.0)
    );
    let source_clock = named_group(occurrence, "Source content clock");
    assert_eq!(source_clock.playback.input_range().start, Time::ZERO);
    assert_eq!(
        source_clock.playback.input_range().duration,
        Duration::from_secs(7.5)
    );
    let playback = source_clock
        .playback
        .time_remap()
        .expect("Pixel Motion source clock must use editable keyframes");
    let keys = playback.keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(
        (keys[0].time.as_secs(), keys[0].value.as_secs()),
        (0.0, 0.0)
    );
    assert_eq!(
        (keys[1].time.as_secs(), keys[1].value.as_secs()),
        (7.5, 5.0)
    );

    let FxLayer::Video(video) = source_clock.layers[0].data() else {
        panic!("Pixel Motion source must remain an editable Video layer")
    };
    assert_eq!(video.source.asset_id.as_str(), "aep-local-item-108");
    assert_eq!(video.playback.input_range().start, Time::ZERO);
    assert_eq!(
        video.playback.input_range().duration,
        Duration::from_secs(5.0)
    );
    assert_eq!(video.source_range.start, Time::ZERO);
    assert_eq!(video.source_range.duration, Duration::from_secs(5.0));
    assert_eq!(video.source_intrinsic_duration, Duration::from_secs(5.0));
    assert_eq!(
        video.frame_blending,
        Some(fx_schema::layer::FrameBlendingData::Mode(
            fx_schema::layer::FrameBlendingMode::OpticalFlow
        ))
    );

    assert_eq!(converted.assets.len(), 1);
    let asset = &converted.assets[0];
    assert_eq!(asset.logical_id.as_str(), "aep-local-item-108");
    assert!(asset.authored_path.ends_with("feature_rate_24_blue.mp4"));
    assert_eq!(asset.kind, MediaAssetKind::Video);
    assert!(diagnostic_contains(&converted, "AE Pixel Motion"));
    assert!(diagnostic_contains(
        &converted,
        "represented by FX optical-flow frame blending"
    ));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_media_cases_keep_assets_gain_visibility_and_frame_policy() {
    // Oracles: /tmp/aep-author-batch3.jsx, image-gradient-clocks.jsx, final-eleven.jsx.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (id, name, gain) in [
        (2, "AUDIO_UNITY", 1.0),
        (18, "AUDIO_LEFT_GAIN", 0.251188643150958),
        (33, "AUDIO_RIGHT_GAIN", 0.251188643150958),
        (48, "AUDIO_STEREO_GAIN", 0.35481338923357547),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/media/import_audio_media_controls.aep",
            id,
            || {
                let audio = pinned_project(&AUDIO_MEDIA);
                let converted = fresh_import_with_assets(&audio, id, name);
                let json = document_json(&converted).to_string();
                assert!(json.contains("audio-stereo.wav"));
                assert!(json.contains(&gain.to_string()[..6]));
            },
        );
    }
    cases.run(
        "crates/aftereffects_file/tests/fixtures/media/import_audio_media_controls.aep",
        63,
        || {
            let audio = pinned_project(&AUDIO_MEDIA);
            let keyed = fresh_import_with_assets(&audio, 63, "AUDIO_GAIN_KEYED");
            super::super::adobe_feature_additions::assert_audio_gain_and_switch(&keyed, 63);
            assert!(
                dynamic_entries(&document_json(&keyed))
                    .iter()
                    .any(|entry| entry["target"]["propertyType"] == "audioVolume")
            );
        },
    );
    for (id, name) in [(78, "AUDIO_AUDIO_OFF"), (93, "AUDIO_VISUAL_OFF")] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/media/import_audio_media_controls.aep",
            id,
            || {
                let audio = pinned_project(&AUDIO_MEDIA);
                let converted = fresh_import_with_assets(&audio, id, name);
                super::super::adobe_feature_additions::assert_audio_gain_and_switch(&converted, id);
                assert!(document_json(&converted).to_string().contains("isHidden"));
            },
        );
    }
    for (id, name, mode) in [
        (109, "VIDEO_FRAME_BLEND_0", None),
        (124, "VIDEO_FRAME_BLEND_1", Some("simple")),
        (139, "VIDEO_FRAME_BLEND_2", Some("opticalFlow")),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/media/import_audio_media_controls.aep",
            id,
            || {
                let audio = pinned_project(&AUDIO_MEDIA);
                let converted = fresh_import_with_assets(&audio, id, name);
                let json = document_json(&converted).to_string();
                assert_eq!(json.contains("frameBlending"), mode.is_some());
                if let Some(mode) = mode {
                    assert!(json.contains(mode));
                }
                assert_eq!(
                    diagnostic_contains(&converted, "AE Pixel Motion"),
                    id == 139,
                    "{name} approximation diagnostic"
                );
            },
        );
    }
    cases.run(
        "crates/aftereffects_file/tests/fixtures/media/import_frame_blend_master_off.aep",
        2,
        || {
            let master = pinned_project(&FRAME_BLEND_MASTER_OFF);
            let converted = fresh_import_with_assets(&master, 2, "FRAME_BLEND_MASTER_OFF");
            assert!(
                !document_json(&converted)
                    .to_string()
                    .contains("frameBlending")
            );
        },
    );
    cases.finish();

    let images = pinned_project(&IMAGE_SOURCES);
    for (id, name, assets) in [
        (3, "STILL_IMAGE", 1),
        (16, "SHARED_IMAGE_INSTANCES", 1),
        (30, "DISTINCT_IMAGES", 2),
    ] {
        let converted = fresh_import_with_assets(&images, id, name);
        assert_eq!(converted.assets.len(), assets);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_essential_overrides_are_occurrence_local_and_editable() {
    // Oracles: /tmp/aep-author-batch3.jsx, essential-text-flags.jsx, gradient-nested.jsx.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (source_id, source_name, override_id, override_name) in [
        (1, "SOURCE_OPACITY", 16, "OVERRIDE_OPACITY"),
        (32, "SOURCE_POSITION", 46, "OVERRIDE_POSITION"),
        (62, "SOURCE_ROTATION", 76, "OVERRIDE_ROTATION"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/essential/import_occurrence_overrides.aep",
            source_id,
            || {
                let transform = pinned_project(&ESSENTIAL_TRANSFORM);
                let source = fresh_import(&transform, source_id, source_name);
                assert_eq!(root(&source).layers.len(), 1);
            },
        );
        cases.run(
            "crates/aftereffects_file/tests/fixtures/essential/import_occurrence_overrides.aep",
            override_id,
            || {
                let transform = pinned_project(&ESSENTIAL_TRANSFORM);
                let changed = fresh_import(&transform, override_id, override_name);
                let changed_group = named_group(root(&changed), "changed_instance");
                let unchanged = named_group(root(&changed), "unchanged_instance");
                match override_id {
                    16 => {
                        assert_eq!(changed_group.transform.opacity.value(), 45.0);
                        assert_eq!(unchanged.transform.opacity.value(), 100.0);
                    }
                    46 => {
                        assert_eq!(
                            changed_group.transform.position,
                            fx_composition::Position::TwoD([800.0, 400.0])
                        );
                        assert_ne!(
                            changed_group.transform.position,
                            unchanged.transform.position
                        );
                    }
                    76 => {
                        assert_eq!(changed_group.transform.rotation, 35.0);
                        assert_eq!(unchanged.transform.rotation, 0.0);
                    }
                    _ => unreachable!(),
                }
            },
        );
    }
    for (id, name) in [
        (1, "COLOR_SOURCE"),
        (14, "COLOR_OVERRIDE"),
        (28, "MULTIPLE_SOURCE"),
        (43, "MULTIPLE_OVERRIDE"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/essential/import_color_nested_overrides.aep",
            id,
            || {
                let colors = pinned_project(&ESSENTIAL_COLOR);
                let converted = fresh_import(&colors, id, name);
                assert!(!root(&converted).layers.is_empty());
                if id == 14 {
                    assert_eq!(
                        editable_rect(named_group(root(&converted), "changed"))
                            .rect
                            .fill_color,
                        [0.1, 0.8, 0.3, 1.0]
                    );
                }
                if id == 43 {
                    assert_eq!(
                        named_group(root(&converted), "changed")
                            .transform
                            .opacity
                            .value(),
                        55.0
                    );
                }
            },
        );
    }
    cases.finish();

    let nested = pinned_project(&ESSENTIAL_NESTED);
    for (id, name) in [
        (1, "NESTED_SOURCE"),
        (16, "MIDDLE_OVERRIDE_55"),
        (29, "OUTER_OVERRIDE_25"),
    ] {
        let converted = fresh_import(&nested, id, name);
        assert!(!root(&converted).layers.is_empty());
        if id == 29 {
            assert!(
                all_groups(root(&converted))
                    .iter()
                    .any(|group| group.transform.opacity.value() == 25.0)
            );
        }
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn grouped_native_media_replacement_changes_only_selected_source_identity() {
    // Oracle: /tmp/aep-author-essential-text-flags.jsx. The published source comp remains blue;
    // it is not a visual pass for the red replacement occurrence.
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/media-replacement/import_media_replacement_cases.aep",
        3,
        || {
            let project = pinned_project(&MEDIA_REPLACEMENT);
            let source = fresh_import_with_assets(&project, 3, "MEDIA_SOURCE");
            assert!(
                document_json(&source)
                    .to_string()
                    .contains("feature_rate_24_blue.mp4")
            );
        },
    );
    cases.run(
        "crates/aftereffects_file/tests/fixtures/media-replacement/import_media_replacement_cases.aep",
        16,
        || {
            let project = pinned_project(&MEDIA_REPLACEMENT);
            let replaced =
                fresh_import_with_assets(&project, 16, "MEDIA_REPLACED_INSTANCE");
            let json = document_json(&replaced).to_string();
            assert!(json.contains("feature_rate_24_red.mp4"));
            assert!(json.contains("feature_rate_24_blue.mp4"));
        },
    );
    cases.run(
        "crates/aftereffects_file/tests/fixtures/media-replacement/import_media_replacement_cases.aep",
        30,
        || {
            let project = pinned_project(&MEDIA_REPLACEMENT);
            let alternate = fresh_import_with_assets(
                &project,
                30,
                "feature_rate_24_blue.mp4_feature_rate_24_red",
            );
            assert!(!root(&alternate).layers.is_empty());
        },
    );
    cases.finish();
}
