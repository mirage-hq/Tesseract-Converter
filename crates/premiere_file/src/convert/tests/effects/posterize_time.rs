use super::*;

const NATIVE: &str = include_str!("../../../../tests/fixtures/posterize_time/native-controls.xml");

fn native_project(xml: &str) -> (PrProjectFile, Vec<Omission>) {
    crate::format::inspect_project_with_omissions(xml, Some("sequence-1")).unwrap()
}

fn import_native(xml: &str) -> (Value, Vec<Omission>) {
    let (project, mut omissions) = native_project(xml);
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    (document, omissions)
}

#[test]
fn posterize_time_native_initial_rate_imports_without_replaying_animation_or_cache() {
    // Supplementary native bypass mutation, not Adobe bypass acceptance.
    let bypass = NATIVE.replace(
        "<DisplayName>Posterize Time</DisplayName>",
        "<DisplayName>Posterize Time</DisplayName><Bypass>true</Bypass>",
    );
    let (disabled, _) = import_native(&bypass);
    assert_eq!(
        disabled["composition"]["layers"][0]["effects"][0]["enabled"],
        false
    );
    assert_eq!(
        disabled["composition"]["layers"][0]["effects"][0]["effect"]["frameRate"],
        6.0
    );
    let (wire, omissions) = import_native(NATIVE);
    let layer = &wire["composition"]["layers"][0];
    assert_eq!(layer["effects"].as_array().unwrap().len(), 1);
    assert_eq!(layer["effects"][0]["enabled"], true);
    assert_eq!(
        layer["effects"][0]["effect"],
        json!({"type": "posterizeTime", "frameRate": 6.0})
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    assert!(document.composition().dynamics().entries().is_empty());
    assert!(
        omissions
            .iter()
            .any(|note| note.reason.contains("Frame Rate animation")),
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .any(|note| note.kind == OmissionKind::Approximated
                && note.reason.contains("layer's in-point")),
        "{omissions:?}"
    );

    let (unsupported, notes) = native_project(&NATIVE.replace(
        "<DiscontinuousInterpolate>true</DiscontinuousInterpolate>",
        "<DiscontinuousInterpolate>false</DiscontinuousInterpolate>",
    ));
    assert!(unsupported.sequences[0].video_tracks[0]
        .clip(0)
        .effects
        .is_empty());
    assert!(notes
        .iter()
        .any(|note| note.reason.contains("Frame Rate layout")));

    // Supplementary trim: sample the parsed Linear keys, never CurrentValue.
    let trimmed = NATIVE
        .replace("<InPoint>0</InPoint>", "<InPoint>254016000000</InPoint>")
        .replace(
            "<OutPoint>1270080000000</OutPoint>",
            "<OutPoint>1524096000000</OutPoint>",
        )
        .replace(
            "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
            "<FrameRect>0,0,1280,720</FrameRect></VideoStream>",
        );
    let (wire, notes) = import_native(&trimmed);
    assert_eq!(wire["dimensions"]["width"], 1920);
    assert_eq!(
        wire["composition"]["layers"][0]["source"]["sourceRect"]["width"].as_f64(),
        Some(1280.0)
    );
    assert_eq!(
        wire["composition"]["layers"][0]["sourceRange"]["start"],
        1000
    );
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"]["frameRate"],
        6.0 + 6.0 * 254016000000.0 / 672387794653.0
    );
    let (later, _) = import_native(
        &NATIVE
            .replace("<InPoint>0</InPoint>", "<InPoint>762048000000</InPoint>")
            .replace(
                "<OutPoint>1270080000000</OutPoint>",
                "<OutPoint>2032128000000</OutPoint>",
            ),
    );
    assert_eq!(
        later["composition"]["layers"][0]["effects"][0]["effect"]["frameRate"],
        12.0
    );
    assert!(notes
        .iter()
        .any(|note| note.kind == OmissionKind::Approximated));
}

fn export_xml(mut project: PrProjectFile) -> String {
    for media in project.media.values_mut() {
        // Supplementary writer-only cases supply the same H.264 fact as the
        // export test harness; the fragment itself has no inspected codec.
        if let Some(video) = &mut media.video {
            if let crate::schema::PrMediaKind::Video { codec, .. } = &mut video.kind {
                *codec = Some(crate::schema::VideoCodec::H264);
            }
        }

        media.name = "source.mp4".to_owned();
        media.relative_path = Some("./media/source.mp4".to_owned());
        media.relative_paths = vec!["./media/source.mp4".to_owned()];
        media.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            "/tmp/source.mp4".into(),
        )];
    }
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("edited.prproj");
    crate::format::PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    crate::format::read_xml(&path).unwrap()
}

#[test]
fn posterize_time_current_edited_export_writes_native_rate_and_bypass_records() {
    let (mut wire, _) = import_native(NATIVE);
    let layer = &mut wire["composition"]["layers"][0];
    layer["effects"][0]["effect"]["frameRate"] = json!(9.5);
    layer["effects"][0]["enabled"] = json!(false);
    let (project, notes) = export_at(wire, FrameRate::Fps24);
    assert!(
        notes
            .iter()
            .all(|note| !note.reason.contains("was not exported")),
        "{notes:?}"
    );
    let xml = export_xml(project);
    // Inspect wire records without the converter's own native reader.
    let document = roxmltree::Document::parse(&xml).unwrap();
    let component = document
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoFilterComponent")
                && node.children().any(|child| {
                    child.has_tag_name("MatchName")
                        && child.text() == Some("AE.ADBE Posterize Time")
                })
        })
        .expect("edited native Posterize Time component");
    let text = |tag| {
        component
            .descendants()
            .find(|node| node.has_tag_name(tag))
            .and_then(|node| node.text())
    };
    assert_eq!(component.attribute("Version"), Some("9"));
    assert_eq!(text("DisplayName"), Some("Posterize Time"));
    assert_eq!(text("Bypass"), Some("true"));
    let reference = component
        .descendants()
        .find(|node| node.has_tag_name("Param"))
        .unwrap()
        .attribute("ObjectRef")
        .unwrap();
    let parameter = document
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some(reference))
        .unwrap();
    let text = |tag| {
        parameter
            .children()
            .find(|node| node.has_tag_name(tag))
            .and_then(|node| node.text())
    };
    assert_eq!(
        parameter.attribute("ClassID"),
        Some("fe47129e-6c94-4fc0-95d5-c056a517aaf3")
    );
    assert_eq!(parameter.attribute("Version"), Some("10"));
    assert_eq!(text("Name"), Some("Frame Rate"));
    assert_eq!(text("ParameterID"), Some("1"));
    assert_eq!(
        text("StartKeyframe"),
        Some("-91445760000000000,9.5,0,0,0,0,0,0")
    );
    assert_eq!(text("LowerBound"), Some("0.0099945068359375"));
    assert_eq!(text("UpperBound"), Some("99"));
    assert_eq!(text("LowerUIBound"), Some("1"));
    assert_eq!(text("UpperUIBound"), Some("64"));
    assert_eq!(text("DiscontinuousInterpolate"), Some("true"));
    assert_eq!(text("Keyframes"), None);
    assert_eq!(text("CurrentValue"), None);
    assert_eq!(text("ParameterControlType"), None);
    // Supplementary strict-reader reimport; independent XML assertions above
    // remain the export-layout evidence.
    let (read, notes) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    let effects = &read.sequences[0].video_tracks[0].clip(0).effects;
    assert_eq!(effects.len(), 1, "{notes:?}");
    assert!(!effects[0].enabled);
    assert_eq!(
        effects[0].params,
        PrEffectParams::PosterizeTime { frame_rate: 9.5 }
    );
    assert!(effects[0].animations.is_empty());
}

#[test]
fn posterize_time_runtime_default_exports_and_unsupported_native_rate_keeps_siblings() {
    let (project, notes) = export(document_with_effects(json!([
        {"id": 1, "effect": {"type": "posterizeTime"}}
    ])));
    assert!(
        notes
            .iter()
            .all(|note| !note.reason.contains("was not exported")),
        "{notes:?}"
    );
    assert!(export_xml(project).contains("-91445760000000000,8.,0,0,0,0,0,0"));

    // Native 99 fps is outside FX's runtime 1–60 fps contract. Keep the clip
    // and a static blur instead of silently rendering the rate at 60 fps.
    let (mut project, _) = native_project(NATIVE);
    let clip = project.sequences[0].video_tracks[0].clip_mut(0);
    clip.effects.insert(0, blur(true, 20.0, false));
    let original = clip.effects[1].clone();
    // Remove keys to exercise a genuinely static out-of-runtime-range rate.
    let static_native = NATIVE.replace(
        "<IsTimeVarying>true</IsTimeVarying>",
        "<IsTimeVarying>false</IsTimeVarying>",
    );
    let start = static_native.find("<Keyframes>").unwrap();
    let end = static_native.find("</Keyframes>").unwrap() + "</Keyframes>".len();
    let static_native = format!("{}{}", &static_native[..start], &static_native[end..])
        .replace(
            ",6.,0,0,0,0,0,0</StartKeyframe>",
            ",99.,0,0,0,0,0,0</StartKeyframe>",
        )
        .replace(
            "<CurrentValue>12</CurrentValue>",
            "<CurrentValue>99</CurrentValue>",
        );
    let (unsupported, _) = native_project(&static_native);
    let unsupported_rate = unsupported.sequences[0].video_tracks[0]
        .clip(0)
        .effects
        .clone();
    assert_eq!(unsupported_rate.len(), 1);
    assert_ne!(unsupported_rate[0], original);
    project.sequences[0].video_tracks[0].clip_mut(0).effects[1] = unsupported_rate[0].clone();
    let media = &project.media;
    let ids = crate::tesseract_output::asset_ids_in_order(&project.sequences[0], media);
    let mut notes = Vec::new();
    let wire =
        crate::convert::premiere_to_tesseract(&project.sequences[0], media, &ids, &mut notes)
            .unwrap()
            .to_json_value()
            .unwrap();
    assert_eq!(
        wire["composition"]["layers"][0]["effects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"]["type"],
        "gaussianBlur"
    );
    assert!(
        notes.iter().any(|note| note.reason.contains("1 to 60")),
        "{notes:?}"
    );
}

#[test]
fn posterize_time_with_motion_keeps_motion_and_sibling_effects_in_both_directions() {
    let (mut native, _) = native_project(NATIVE);
    let sequence = &mut native.sequences[0];
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.effects.insert(0, blur(true, 20.0, false));
    clip.animations
        .push(crate::schema::PrPropertyAnimation::Rotation(vec![
            key(0, 10.0, PrKeyframeEasing::Linear),
            key(TICKS, 30.0, PrKeyframeEasing::Linear),
        ]));
    let media = &native.media;
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, media);
    let mut notes = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(sequence, media, &ids, &mut notes)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert_eq!(
        wire["composition"]["layers"][0]["effects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    assert_eq!(document.composition().dynamics().entries().len(), 1);
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("other layer or effect animation")),
        "{notes:?}"
    );

    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 20.0}},
        {"id": 2, "effect": {"type": "posterizeTime", "frameRate": 6.0}}
    ]));
    wire["composition"]["dynamics"] = json!({"entries": [layer_track("rotation")]});
    let (project, notes) = export(wire.clone());
    assert_eq!(exported_effects(&project).len(), 1);
    assert_eq!(
        project.sequences[0].video_tracks[0]
            .clip(0)
            .animations
            .len(),
        1
    );
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("other layer or effect animation")),
        "{notes:?}"
    );
    wire["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 1, "paramName": "blurriness"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            fx_key("blur-0", 0, 20.0, json!({"type": "linear"})),
            fx_key("blur-1", 500, 30.0, json!({"type": "linear"})),
        ]}
    }]});
    let (project, notes) = export(wire.clone());
    assert_eq!(exported_effects(&project).len(), 1);
    assert_eq!(exported_effects(&project)[0].animations.len(), 1);
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("other layer or effect animation")),
        "{notes:?}"
    );
    // Bypass removes the clock conflict without discarding either control.
    wire["composition"]["layers"][0]["effects"][1]["enabled"] = json!(false);
    let (project, notes) = export(wire);
    assert_eq!(exported_effects(&project).len(), 2);
    assert!(!exported_effects(&project)[1].enabled);
    assert!(
        notes
            .iter()
            .all(|note| !note.reason.contains("was not exported")),
        "{notes:?}"
    );
}

#[test]
fn posterize_time_unsupported_owner_and_rate_animation_export_keep_convertible_content() {
    let (native, _) = native_project(NATIVE);
    let mut clip = native.sequences[0].video_tracks[0].clip(0).clone();
    clip.effects.push(blur(true, 20.0, false));
    let mut ids = super::super::EffectIdAllocator::default();
    let mut notes = Vec::new();
    let (effects, _) = super::super::import_effects(
        &clip,
        fx_schema::LayerId::new(1),
        false,
        crate::schema::MaskBoundary::Flat,
        false,
        false,
        crate::schema::PrMediaKind::Adjustment,
        [1920, 1080],
        [1920, 1080],
        [1920, 1080],
        &mut ids,
        &mut notes,
    );
    assert_eq!(effects.len(), 1);
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("adjustment effects do not hold time")),
        "{notes:?}"
    );

    let mut wire = document_with_posterize(
        json!({}),
        &[(
            "frameRate",
            json!([
                fx_key("rate-0", 0, 6.0, json!({"type": "linear"})),
                fx_key("rate-1", 500, 12.0, json!({"type": "linear"})),
            ]),
        )],
    );
    wire["composition"]["layers"][0]["effects"][1]["effect"] =
        json!({"type": "posterizeTime", "frameRate": 7.5});
    let (project, notes) = export(wire);
    assert_eq!(exported_effects(&project).len(), 2);
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("frameRate animation was not exported")),
        "{notes:?}"
    );
    let xml = export_xml(project);
    assert!(xml.contains("-91445760000000000,7.5,0,0,0,0,0,0"));
    assert!(!xml.contains("<Keyframes>"));

    // An FX Adjustment never consumes a Posterize Time hold. Export must not
    // enable a new native clock warp on the lower sibling stack.
    let mut wire = document_with_effects(json!([]));
    let transform = wire["composition"]["layers"][0]["transform"].clone();
    wire["composition"]["layers"].as_array_mut().unwrap().push(json!({
        "type": "Adjustment", "id": 2, "name": "Adjustment",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": transform,
        "effects": [
            {"id": 1, "effect": {"type": "posterizeTime", "frameRate": 6.0}},
            {"id": 2, "effect": {"type": "brightnessContrast", "brightness": 10.0, "contrast": 0.0}}
        ]
    }));
    let (project, notes) = export(wire);
    assert!(project.sequences[0].video_occurrences().any(|clip| {
        clip.effects
            .iter()
            .any(|effect| matches!(effect.params, PrEffectParams::BrightnessContrast(_)))
    }));
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("adjustment effects do not hold time")),
        "{notes:?}"
    );
}

// The mutations below are supplementary safety evidence, not native proof.
fn import_project(project: &PrProjectFile) -> (Value, Vec<Omission>) {
    let sequence = &project.sequences[0];
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut notes = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut notes)
        .unwrap()
        .to_json_value()
        .unwrap();
    (wire, notes)
}

#[test]
fn posterize_time_source_in_hold_and_unsupported_interior_keep_siblings() {
    for easing in [
        PrKeyframeEasing::Hold,
        PrKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.0,
            x2: 0.8,
            y2: 1.0,
        },
    ] {
        let (mut project, _) = native_project(NATIVE);
        let clip = project.sequences[0].video_tracks[0].clip_mut(0);
        clip.in_ticks = TICKS;
        clip.out_ticks += TICKS;
        let PrEffectParamKeys::Scalar(keys) = &mut clip.effects[0].animations[0].keys else {
            panic!("scalar rate");
        };
        keys[1].easing = easing;
        clip.effects.push(blur(true, 20.0, false));
        let (wire, notes) = import_project(&project);
        let effects = wire["composition"]["layers"][0]["effects"]
            .as_array()
            .unwrap();
        if easing == PrKeyframeEasing::Hold {
            assert_eq!(effects.len(), 2);
            assert_eq!(effects[0]["effect"]["frameRate"], 6.0);
        } else {
            assert_eq!(effects.len(), 1);
            assert_eq!(effects[0]["effect"]["type"], "gaussianBlur");
            assert!(
                notes
                    .iter()
                    .any(|note| note.reason.contains("inside unsupported easing")),
                "{notes:?}"
            );
        }
    }
}

#[test]
fn posterize_time_disabled_animated_sibling_survives_import_and_export() {
    let (mut project, _) = native_project(NATIVE);
    project.sequences[0].video_tracks[0]
        .clip_mut(0)
        .effects
        .push(keyed(
            blur(false, 20.0, false),
            vec![
                key(0, 20.0, PrKeyframeEasing::Linear),
                key(TICKS, 30.0, PrKeyframeEasing::Linear),
            ],
        ));
    let (wire, notes) = import_project(&project);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "{notes:?}"
    );
    let (exported, notes) = export(wire);
    assert_eq!(exported_effects(&exported).len(), 2, "{notes:?}");
    assert!(!exported_effects(&exported)[1].enabled);
    assert_eq!(exported_effects(&exported)[1].animations.len(), 1);
}

#[test]
fn posterize_time_keeps_dissolve_and_pop_tracks_instead_of_holding_them() {
    use crate::schema::{PrVideoTransition, PrVideoTransitionKind};
    for kind in [
        PrVideoTransitionKind::FilmImpactDissolve,
        PrVideoTransitionKind::FilmImpactPop,
    ] {
        let (mut project, _) = native_project(NATIVE);
        let track = &mut project.sequences[0].video_tracks[0];
        track.clip_mut(0).id = Some("incoming".into());
        track.transitions.push(PrVideoTransition {
            id: "head".into(),
            kind,
            start_ticks: 0,
            cut_ticks: 0,
            end_ticks: TICKS,
            outgoing_clip: None,
            incoming_clip: Some("incoming".into()),
        });
        let (wire, notes) = import_project(&project);
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        assert!(
            !document.composition().dynamics().entries().is_empty(),
            "{notes:?}"
        );
        assert!(
            document.composition().layers()[0].effects().is_empty(),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|note| note.reason.contains("other layer or effect animation")),
            "{notes:?}"
        );
    }
}

#[test]
fn posterize_time_across_crop_keeps_blur_enabled_or_bypassed() {
    for enabled in [true, false] {
        for before in [true, false] {
            let (mut project, _) = native_project(NATIVE);
            let clip = project.sequences[0].video_tracks[0].clip_mut(0);
            clip.effects[0].enabled = enabled;
            clip.effects[0].animations.clear();
            clip.crop.top = 15.0;
            clip.effects
                .insert(usize::from(before), blur(true, 20.0, false));
            clip.effects_above_mask = 1;
            // Writer-only assembly gives a native chain straddling Crop. The
            // reader must drop the clock before its mask-side accounting.
            let xml = export_xml(project);
            let (read, notes) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
            let effects = &read.sequences[0].video_tracks[0].clip(0).effects;
            assert_eq!(effects.len(), 1, "{notes:?}");
            assert!(matches!(effects[0].params, PrEffectParams::GaussianBlur(_)));
            assert!(
                notes
                    .iter()
                    .any(|note| note.reason.contains("Posterize Time on a masked host")),
                "{notes:?}"
            );
            let (wire, _) = import_project(&read);
            assert!(wire.to_string().contains("gaussianBlur"));
        }
    }
}

#[test]
fn posterize_time_nested_child_is_omitted_independent_of_nest_motion() {
    for moved in [false, true] {
        let mut wire = document_with_effects(json!([
            {"id": 1, "effect": {"type": "posterizeTime", "frameRate": 6.0}},
            {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 20.0}}
        ]));
        let layers = wire["composition"]["layers"].as_array_mut().unwrap();
        let mut video = layers.remove(0);
        video["parent"] = json!(10);
        let mut transform = video["transform"].clone();
        if moved {
            transform["scale"] = json!([0.5, 0.5]);
        }
        layers.insert(0, json!({
            "type": "Group", "id": 10, "name": "Nested", "blendMode": "normal",
            "playback": crate::test_support::linear_playback(json!(*crate::test_support::layer_range(&video)), json!({"start": 0, "duration": (*crate::test_support::layer_range(&video))["duration"]})),
            "transform": transform, "layers": [video]
        }));
        let (mut project, notes) = export(wire);
        assert!(
            notes.iter().any(|note| note.reason.contains("nested")),
            "{notes:?}"
        );
        let nest = &mut project.sequences[0].video_tracks[0].nests[0];
        assert_eq!(nest.sequence.video_tracks[0].clip(0).effects.len(), 1);
        // Supplementary native-model nest: import must enforce the same clock
        // boundary even when the placement has no Motion.
        let (native, _) = native_project(NATIVE);
        nest.sequence.video_tracks[0]
            .clip_mut(0)
            .effects
            .push(native.sequences[0].video_tracks[0].clip(0).effects[0].clone());
        let (imported, notes) = import_project(&project);
        assert!(!imported.to_string().contains("posterizeTime"), "{notes:?}");
        assert!(imported.to_string().contains("gaussianBlur"));
        assert!(
            notes.iter().any(|note| note.reason.contains("nested")),
            "{notes:?}"
        );
    }
}

#[test]
fn posterize_time_bypassed_matte_sibling_keeps_transform_without_weakening_a4() {
    use crate::schema::{PrMatteChannel, PrTrackMatte, TRANSFORM_POSITION};
    for enabled in [false, true] {
        let (mut project, _) = native_project(NATIVE);
        let clip = project.sequences[0].video_tracks[0].clip_mut(0);
        clip.effects[0].enabled = enabled;
        clip.effects[0].animations.clear();
        clip.effects.push(transform_effect(
            DEFAULT_PR_TRANSFORM,
            vec![PrEffectParamAnimation {
                param: &TRANSFORM_POSITION,
                keys: PrEffectParamKeys::Point(vec![
                    PrPointKeyframe {
                        source_ticks: 0,
                        value: [0.5, 0.5],
                        easing: PrKeyframeEasing::Linear,
                        spatial_in_tangent: None,
                        spatial_out_tangent: None,
                    },
                    PrPointKeyframe {
                        source_ticks: TICKS,
                        value: [0.6, 0.5],
                        easing: PrKeyframeEasing::Linear,
                        spatial_in_tangent: None,
                        spatial_out_tangent: None,
                    },
                ]),
            }],
        ));
        clip.active_transforms = 1;
        clip.track_matte = Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        });
        let mut matte = project.sequences[0].video_tracks[0].clone();
        matte.clip_mut(0).track_matte = None;
        matte.clip_mut(0).effects.clear();
        matte.clip_mut(0).active_transforms = 0;
        project.sequences[0].video_tracks.push(matte);
        let xml = export_xml(project);
        let (read, notes) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        let clip = read.sequences[0].video_tracks[0].clip(0);
        if enabled {
            assert!(clip.effects.is_empty(), "{notes:?}");
            assert!(
                notes
                    .iter()
                    .any(|note| note.reason.contains("outside measured A4")),
                "{notes:?}"
            );
        } else {
            assert_eq!(clip.effects.len(), 1, "{notes:?}");
            assert!(matches!(
                clip.effects[0].params,
                PrEffectParams::Transform(_)
            ));
            assert!(clip
                .transform_stage([1920, 1080], [1920, 1080])
                .unwrap()
                .is_some());
        }
    }
}

#[test]
fn posterize_time_static_transform_stage_keeps_transform_and_blur() {
    let (mut project, _) = native_project(NATIVE);
    let clip = project.sequences[0].video_tracks[0].clip_mut(0);
    clip.effects[0].animations.clear();
    clip.effects.push(blur(true, 10.0, false));
    clip.effects.push(transform_effect(
        crate::schema::PrTransform {
            position: [0.75, 0.5],
            ..DEFAULT_PR_TRANSFORM
        },
        Vec::new(),
    ));
    clip.active_transforms = 1;
    let record = clip.record().to_owned();
    assert!(clip
        .transform_stage([1920, 1080], [1920, 1080])
        .unwrap()
        .is_some());
    let (wire, notes) = import_project(&project);
    let group = &wire["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    let video = &group["layers"][0];
    assert_eq!(video["type"], "Video");
    assert_eq!(video["source"]["assetId"], "premiere-video-1");
    assert_eq!(video["transform"]["position"][0], 1440.0);
    let effects = video["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(
        effects[0]["effect"],
        json!({"type": "gaussianBlur", "blurriness": 10.0})
    );
    assert!(
        notes.iter().any(|note| note.scope == OmissionScope::Feature
            && note.record == record
            && note
                .reason
                .contains("Posterize Time effect at stack position 1 was not imported")
            && note.reason.contains("staged")),
        "{notes:?}"
    );
}
