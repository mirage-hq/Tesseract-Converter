use crate::{
    format::{inspect_project, inspect_project_with_omissions},
    schema::{PrAnimatedProperty, PrBlendMode, TICKS},
};

#[path = "../../../tests/support/animation.rs"]
pub(super) mod animation_fixture;
use animation_fixture::{animated_xml, SOURCE};

#[test]
fn position_path_preserves_coordinates_timing_easing_and_paired_tangents() {
    let keys = format!(
        "0,0.25:0.5,5,0,0,0.2,0.375,0.4,5,4,0,0,0.1,-0.05;{},0.75:0.75,0,0,0.5,0.2,0,0,5,4,-0.1,0.05,0,0;",
        TICKS
    );
    let xml = animated_xml("").replace(
        "<Name>Position</Name>",
        &format!("<Name>Position</Name><Keyframes>{keys}</Keyframes>"),
    );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let sequence = parsed.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    let position = clip
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Position)
        .unwrap();
    let keys = position.point_keys().unwrap();
    assert_eq!(keys[0].source_ticks, 0);
    assert_eq!(keys[1].source_ticks, TICKS);
    assert_eq!(keys[0].value, [0.25, 0.5]);
    assert_eq!(keys[1].value, [0.75, 0.75]);
    assert_eq!(keys[0].spatial_out_tangent, Some([0.1, -0.05]));
    assert_eq!(keys[1].spatial_in_tangent, Some([-0.1, 0.05]));
    assert_eq!(
        keys[1].easing,
        crate::schema::PrKeyframeEasing::CubicBezier {
            x1: 0.4,
            y1: 0.26015378689598456,
            x2: 0.8,
            y2: 0.8265641420693436,
        }
    );

    let document = crate::tests::support::project_document_with_media(sequence, &parsed.media);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (property, dimension, values, tangents) in [
        (
            "positionX",
            sequence.width as f64,
            [0.25, 0.75],
            [0.1, -0.1],
        ),
        (
            "positionY",
            sequence.height as f64,
            [0.5, 0.75],
            [-0.05, 0.05],
        ),
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"].as_str() == Some(property))
            .unwrap_or_else(|| panic!("missing {property}: {entries:#?}"));
        assert_eq!(
            entry["animator"]["keyframes"][0]["value"]["value"],
            values[0] * dimension
        );
        assert_eq!(
            entry["animator"]["keyframes"][1]["value"]["value"],
            values[1] * dimension
        );
        assert_eq!(
            entry["animator"]["keyframes"][0]["spatialOutTangent"],
            tangents[0] * dimension
        );
        assert_eq!(
            entry["animator"]["keyframes"][1]["spatialInTangent"],
            tangents[1] * dimension
        );
    }
}

#[test]
fn linear_spatial_flags_two_keep_editable_position_values_easing_and_clip_clock() {
    let keys = format!(
        "{},0.25:0.5,5,0,0,0.2,0,0.4,0,2,0,0,0,0;{},0.75:0.75,5,0,0,0.2,0,0.4,0,2,0,0,0,0;",
        TICKS / 2,
        2 * TICKS
    );
    let xml = animated_xml("")
        .replace(
            "<Name>Position</Name>",
            &format!("<Name>Position</Name><Keyframes>{keys}</Keyframes>"),
        )
        .replace(
            "<InPoint>0</InPoint>",
            &format!("<InPoint>{TICKS}</InPoint>"),
        )
        .replace(
            "<OutPoint>1270080000000</OutPoint>",
            &format!("<OutPoint>{}</OutPoint>", 6 * TICKS),
        );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let sequence = parsed.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    let keys = clip.animations[0].point_keys().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].source_ticks, TICKS / 2);
    assert_eq!(keys[1].source_ticks, 2 * TICKS);
    assert_eq!(keys[0].value, [0.25, 0.5]);
    assert_eq!(keys[1].value, [0.75, 0.75]);
    assert_eq!(
        keys[1].easing,
        crate::schema::PrKeyframeEasing::CubicBezier {
            x1: 0.4,
            y1: 0.0,
            x2: 0.8,
            y2: 1.0
        }
    );
    let document = crate::tests::support::project_document_with_media(sequence, &parsed.media);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    for (property, values) in [
        ("positionX", [480.0, 1440.0]),
        ("positionY", [540.0, 810.0]),
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"].as_str() == Some(property))
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        for (key, value) in keys.iter().zip(values) {
            assert_eq!(key["value"]["value"], value);
            assert!(key.get("spatialInTangent").is_none());
            assert!(key.get("spatialOutTangent").is_none());
        }
        assert_eq!(keys[1]["easing"]["type"], "cubicBezier");
    }
    let editable = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
    for entry in editable.composition().dynamics().entries() {
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
        else {
            panic!("expected editable Position keys");
        };
        assert_eq!(track.keyframes()[0].layer_time().as_millis(), -500);
        assert_eq!(track.keyframes()[1].layer_time().as_millis(), 1000);
        assert!(!track.has_spatial_tangents());
    }
}

#[test]
fn rotation_keys_preserve_source_times_outside_the_clip_trim() {
    let xml = animated_xml(&format!(
        "0,0.,0,0,0,0,0,0;{},90.,0,0,0,0,0,0;{},180.,0,0,0,0,0,0;",
        2 * TICKS,
        7 * TICKS
    ))
    .replace(
        "<InPoint>0</InPoint>",
        &format!("<InPoint>{}</InPoint>", TICKS),
    )
    .replace(
        "<OutPoint>1270080000000</OutPoint>",
        &format!("<OutPoint>{}</OutPoint>", 6 * TICKS),
    );
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let clip = project.video_occurrences().next().unwrap();
    assert_eq!(clip.in_ticks, TICKS);
    assert_eq!(clip.animations.len(), 1);
    assert_eq!(clip.animations[0].property(), PrAnimatedProperty::Rotation);
    assert_eq!(
        clip.animations[0]
            .keys()
            .iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        [(0, 0.0), (2 * TICKS, 90.0), (7 * TICKS, 180.0)]
    );
}

#[test]
fn trimmed_rotation_uses_static_start_value_before_its_first_key() {
    let keys = format!(
        "{},12.,0,0,0,0,0,0;{},90.,0,0,0,0,0,0;",
        2 * TICKS,
        4 * TICKS
    );
    let xml = animated_xml(&keys)
        .replace(
            "<Name>Rotation</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.,",
            "<Name>Rotation</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,12.,",
        )
        .replace("<InPoint>0</InPoint>", &format!("<InPoint>{}</InPoint>", TICKS))
        .replace("<OutPoint>1270080000000</OutPoint>", &format!("<OutPoint>{}</OutPoint>", 6 * TICKS));
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let clip = project.video_occurrences().next().unwrap();
    assert_eq!(clip.transform.rotation, 12.0);
    assert_eq!(clip.animations[0].keys()[0].value, 12.0);
}

#[test]
fn a_time_varying_motion_parameter_without_keys_keeps_its_start_keyframe() {
    // A time-varying parameter saved without keys keeps its StartKeyframe, as
    // an Opacity saved so renders it (`read_video_compositing`).
    let xml = animated_xml("")
        .replace(
            "<Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,",
            "<Name>Position</Name><ParameterID>1</ParameterID><IsTimeVarying>true</IsTimeVarying><StartKeyframe>-91445760000000000,0.25:0.5,",
        )
        .replace(
            "<Name>Rotation</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.,",
            "<Name>Rotation</Name><ParameterID>5</ParameterID><IsTimeVarying>true</IsTimeVarying><StartKeyframe>-91445760000000000,30.,",
        );
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let clip = project.video_occurrences().next().unwrap();
    assert_eq!(clip.transform.position, [0.25, 0.5]);
    assert_eq!(clip.transform.rotation, 30.0);
    assert!(clip.animations.is_empty(), "{:?}", clip.animations);
    // A malformed StartKeyframe still rejects, keys or none.
    let malformed = xml.replace("0.25:0.5,", "0.25,");
    assert_ne!(malformed, xml);
    assert!(inspect_project(&malformed, Some("sequence-1")).is_err());
}

#[test]
fn hold_and_asymmetric_bezier_are_preserved() {
    let xml = animated_xml(&format!(
        "0,0.,4,0,0,0,0,0;{},90.,5,0,1,0.2,3,0.4;{},180.,0,0,0,0,0,0;",
        TICKS,
        2 * TICKS
    ));
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let keys = project.video_occurrences().next().unwrap().animations[0].keys();
    assert_eq!(
        keys.iter().map(|key| key.value).collect::<Vec<_>>(),
        [0.0, 90.0, 180.0]
    );
    assert_eq!(keys[1].easing, crate::schema::PrKeyframeEasing::Hold);
    assert_eq!(
        keys[2].easing,
        crate::schema::PrKeyframeEasing::CubicBezier {
            x1: 0.4,
            y1: 0.013333333333333336,
            x2: 1.0,
            y2: 1.0,
        }
    );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let doc = crate::tests::support::project_document_with_media(
        parsed.single_sequence().unwrap(),
        &parsed.media,
    );
    let animator = &doc["composition"]["dynamics"]["entries"][0]["animator"];
    assert_eq!(animator["keyframes"][1]["easing"]["type"], "hold");
    assert_eq!(animator["keyframes"][2]["easing"]["type"], "cubicBezier");
    let editable = fx_schema::EditableFxCompositionDocument::from_json_value(doc).unwrap();
    let entry = &editable.composition().dynamics().entries()[0];
    let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
        panic!("expected editable keyframe animator");
    };
    assert_eq!(track.keyframes().len(), 3);
    // Numerical samples remain covered by fx_composition/tests/premiere_keyframes.rs;
    // portable conversion checks must not import the execution engine.
}

#[test]
fn native_bezier_uses_destination_incoming_handle_and_accepts_extreme_ticks() {
    let xml = animated_xml(&format!("0,0.,5,0,0,0,3,0.4;{},90.,0,0,2,0.2,0,0;", TICKS));
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let keys = project.video_occurrences().next().unwrap().animations[0].keys();
    let crate::schema::PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = keys[1].easing else {
        panic!("expected cubic segment");
    };
    assert_eq!((x1, x2), (0.4, 0.8));
    assert!((y1 - 0.4 * 3.0 / 90.0).abs() < 1e-12);
    assert!((y2 - (1.0 - 0.2 * 2.0 / 90.0)).abs() < 1e-12);

    let extreme = animated_xml(&format!(
        "{},0.,5,0,0,0,3,0.4;{},90.,0,0,2,0.2,0,0;",
        i64::MIN,
        i64::MAX
    ));
    let parsed = inspect_project(&extreme, Some("sequence-1")).unwrap();
    let keys = parsed.video_occurrences().next().unwrap().animations[0].keys();
    assert_eq!(keys[0].source_ticks, i64::MIN);
    assert_eq!(keys[1].source_ticks, i64::MAX);
    assert!(matches!(
        keys[1].easing,
        crate::schema::PrKeyframeEasing::CubicBezier { .. }
    ));
}

/// The progress of a native cubic Bezier easing at normalized time `x`.
pub(super) fn bezier_progress(easing: crate::schema::PrKeyframeEasing, x: f64) -> f64 {
    let crate::schema::PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = easing else {
        panic!("expected a cubic Bezier segment, got {easing:?}");
    };
    let curve = |a: f64, b: f64, t: f64| {
        3.0 * (1.0 - t).powi(2) * t * a + 3.0 * (1.0 - t) * t * t * b + t.powi(3)
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..100 {
        let t = (low + high) / 2.0;
        if curve(x1, x2, t) < x {
            low = t;
        } else {
            high = t;
        }
    }
    curve(y1, y2, (low + high) / 2.0)
}

/// Clip Motion Scale keys as Premiere 26.5.1 saved them in the Bezier-end
/// probe: 100 at 1 s (Linear), 150 at 2 s (Bezier, out 0/s over 1/6), 100 at
/// 4 s (Hold, in -25/s over 1/6) and 150 at 5 s, on a clip with its In at 0.
const HOLD_END_PROBE_SCALE: &str = "254016000000,100.,0,0,0,0.16666666666666666,50,0.16666666666666666;508032000000,150.,5,0,12.5,0.59999999999999998,0,0.16666666666666666;1016064000000,100.,4,0,-25,0.16666666666666666,0,0.33333333333333331;1270080000000,150.,5,0,12.5,0.59999999999999998,0,0.16666666666666666;";

#[test]
fn a_bezier_segment_into_a_hold_key_arrives_with_a_zero_length_handle_as_premiere_reads_it() {
    use crate::schema::PrKeyframeEasing;
    // Premiere read 128.379578 at 3.0 s, halfway into the Hold key at 4 s: it
    // follows the out-handle of the key at 2 s and ignores the stored in-handle
    // of the key that starts the Hold, which would give 128.125.
    let xml = animated_xml("").replace(
        "<Name>Scale</Name>",
        &format!("<Name>Scale</Name><Keyframes>{HOLD_END_PROBE_SCALE}</Keyframes>"),
    );
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let clip = project.video_occurrences().next().unwrap();
    assert_eq!(
        clip.animations[0].property(),
        PrAnimatedProperty::UniformScale
    );
    let keys = clip.animations[0].keys();
    assert_eq!(
        keys[2].easing,
        PrKeyframeEasing::CubicBezier {
            x1: 0.16666666666666666,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        }
    );
    let at_3_s = 150.0 - 50.0 * bezier_progress(keys[2].easing, 0.5);
    assert!((at_3_s - 128.379578).abs() < 1e-5, "{at_3_s}");
    // The Hold that the key at 4 s starts still holds.
    assert_eq!(keys[3].easing, PrKeyframeEasing::Hold);
}

#[test]
fn a_point_bezier_segment_into_a_hold_key_keeps_its_stored_in_handle() {
    // Premiere's zero-length arrival at a Hold key is measured on scalar keys
    // only, so a Point (Position) key that starts a Hold reads its stored
    // in-handle as before: the same curve as without the Hold.
    let keys = format!(
        "0,0.25:0.5,5,0,0,0.2,0.375,0.4,5,4,0,0,0.1,-0.05;{},0.75:0.75,4,0,0.5,0.2,0,0,5,4,-0.1,0.05,0,0;{},0.5:0.5,0,0,0,0,0,0,0,0,0,0,0,0;",
        TICKS,
        2 * TICKS
    );
    let xml = animated_xml("").replace(
        "<Name>Position</Name>",
        &format!("<Name>Position</Name><Keyframes>{keys}</Keyframes>"),
    );
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let position = project
        .video_occurrences()
        .next()
        .unwrap()
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Position)
        .unwrap();
    let keys = position.point_keys().unwrap();
    assert_eq!(
        keys[1].easing,
        crate::schema::PrKeyframeEasing::CubicBezier {
            x1: 0.4,
            y1: 0.26015378689598456,
            x2: 0.8,
            y2: 0.8265641420693436,
        }
    );
    assert_eq!(keys[2].easing, crate::schema::PrKeyframeEasing::Hold);
}

#[test]
fn keyframe_flags_do_not_change_linear_easing() {
    let xml = animated_xml(&format!("0,0.,0,4,0,0,0,0;{},90.,0,0,0,0,0,0;", TICKS));
    let project = inspect_project(&xml, Some("sequence-1")).unwrap();
    let keys = project.video_occurrences().next().unwrap().animations[0].keys();
    assert_eq!(keys[1].source_ticks, TICKS);
    assert_eq!(keys[1].value, 90.0);
    assert_eq!(keys[1].easing, crate::schema::PrKeyframeEasing::Linear);
}

#[test]
fn unsupported_interpolation_mode_is_reported_instead_of_flattened() {
    let xml = animated_xml(&format!("0,0.,6,0,0,0,0,0;{},90.,0,0,0,0,0,0;", TICKS));
    let error = inspect_project(&xml, Some("sequence-1")).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported interpolation mode 6"),
        "{error}"
    );
}

#[test]
fn disabled_or_invalid_motion_keyframe_state_rejects_without_activating_keys() {
    let keys = format!("0,0.,0,0,0,0,0,0;{},90.,0,0,0,0,0,0;", TICKS);
    let animated = animated_xml(&keys);
    let marker = "<Name>Rotation</Name>";
    for state in ["false", "unexpected"] {
        let xml = animated.replacen(
            marker,
            &format!("{marker}<IsTimeVarying>{state}</IsTimeVarying>"),
            1,
        );
        let error = inspect_project(&xml, Some("sequence-1"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("IsTimeVarying"), "{error}");
    }
    let enabled = animated.replacen(
        marker,
        &format!("{marker}<IsTimeVarying>true</IsTimeVarying>"),
        1,
    );
    assert_eq!(
        inspect_project(&enabled, Some("sequence-1"))
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap()
            .animations[0]
            .keys()
            .len(),
        2
    );
    let static_xml = animated_xml("");
    let disabled = static_xml.replacen(
        marker,
        &format!("{marker}<IsTimeVarying>false</IsTimeVarying>"),
        1,
    );
    assert!(inspect_project(&disabled, Some("sequence-1")).is_ok());
    let invalid = static_xml.replacen(
        marker,
        &format!("{marker}<IsTimeVarying>unexpected</IsTimeVarying>"),
        1,
    );
    assert!(inspect_project(&invalid, Some("sequence-1"))
        .unwrap_err()
        .to_string()
        .contains("IsTimeVarying"));
}

#[test]
fn shared_motion_record_rejects_missing_required_fields_with_native_identity() {
    let xml = animated_xml("");
    for (field, replacement) in [
        ("Name", "<Name>Rotation</Name>"),
        ("ParameterID", "<ParameterID>5</ParameterID>"),
        (
            "StartKeyframe",
            "<StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>",
        ),
    ] {
        let invalid = xml.replacen(replacement, "", 1);
        assert_ne!(invalid, xml);
        let error = inspect_project(&invalid, Some("sequence-1"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("VideoComponentParam:15"), "{field}: {error}");
        assert!(error.contains(field), "{field}: {error}");
    }
}

#[test]
fn unsupported_animated_scale_width_rejects() {
    let xml = animated_xml(&format!("0,0.,0,0,0,0,0,0;{},90.,0,0,0,0,0,0;", TICKS));
    let width = xml.replace(
        "<Name>Scale Width</Name>",
        "<Name>Scale Width</Name><Keyframes>0,100.,0,0,0,0,0,0;</Keyframes>",
    );
    assert!(inspect_project(&width, Some("sequence-1"))
        .unwrap_err()
        .to_string()
        .contains("animated Scale Width is unsupported"));
}

#[test]
fn duplicate_scale_width_under_uniform_scale_imports_once() {
    // The Big Sale discriminator's duplicate Linear form, using neutral test values.
    // Keep the native 14-frame span; no licensed project records are embedded here.
    let end = TICKS * 14 / 30;
    let keys = format!("0,0.,0,0,0,0,0,0;{end},75.,0,0,0,0,0,0;");
    let xml = animated_xml("")
        .replace(
            "<Name>Scale</Name>",
            &format!("<Name>Scale</Name><Keyframes>{keys}</Keyframes>"),
        )
        .replace(
            "<Name>Scale Width</Name>",
            &format!("<Name>Scale Width</Name><Keyframes>{keys}</Keyframes>"),
        );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let sequence = parsed.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.animations.len(), 1);
    assert_eq!(
        clip.animations[0].property(),
        PrAnimatedProperty::UniformScale
    );
    assert_eq!(clip.animations[0].keys()[0].source_ticks, 0);
    assert_eq!(clip.animations[0].keys()[0].value, 0.0);
    assert_eq!(clip.animations[0].keys()[1].source_ticks, end);
    assert_eq!(clip.animations[0].keys()[1].value, 75.0);
    let document = crate::tests::support::project_document_with_media(sequence, &parsed.media);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    for property in ["scaleX", "scaleY"] {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["layerTime"], 0);
        assert_eq!(keys[0]["value"]["value"], 0.0);
        assert_eq!(keys[1]["layerTime"], 467);
        assert_eq!(keys[1]["value"]["value"], 75.0);
        assert_eq!(keys[1]["easing"]["type"], "linear");
    }
}

#[test]
fn duplicate_scale_width_admission_preserves_curve_rejections() {
    let scale = format!("0,0.,0,0,0,0,0,0;{TICKS},75.,0,0,0,0,0,0;");
    let mismatch = "animated Scale Width is unsupported under Uniform Scale";
    for (width, expected) in [
        (scale.replace("75.", "50."), mismatch),
        (
            scale.replace(&TICKS.to_string(), &(TICKS / 2).to_string()),
            mismatch,
        ),
        (format!("{TICKS},75.,0,0,0,0,0,0;"), mismatch),
        (
            scale.replace("0,0.,0,", "0,0.,4,"),
            "only Linear Scale Width keys convert",
        ),
        (scale.replace("75.", "invalid"), "invalid"),
    ] {
        let xml = animated_xml("")
            .replace(
                "<Name>Scale</Name>",
                &format!("<Name>Scale</Name><Keyframes>{scale}</Keyframes>"),
            )
            .replace(
                "<Name>Scale Width</Name>",
                &format!("<Name>Scale Width</Name><Keyframes>{width}</Keyframes>"),
            );
        let error = inspect_project(&xml, Some("sequence-1"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn animated_uniform_scale_imports_as_distinct_editable_axis_tracks() {
    let xml = animated_xml("").replace(
        "<Name>Scale</Name>",
        &format!(
            "<Name>Scale</Name><Keyframes>0,100.,4,0,0,0,0,0;{},150.,0,0,0,0,0,0;</Keyframes>",
            TICKS
        ),
    );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let clip = parsed
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.animations.len(), 1);
    assert_eq!(
        clip.animations[0].property(),
        PrAnimatedProperty::UniformScale
    );
    assert_eq!(clip.animations[0].keys()[1].value, 150.0);
    assert_eq!(
        clip.animations[0].keys()[1].easing,
        crate::schema::PrKeyframeEasing::Hold
    );
    let doc = crate::tests::support::project_document_with_media(
        parsed.single_sequence().unwrap(),
        &parsed.media,
    );
    let entries = doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    let mut properties: Vec<_> = entries
        .iter()
        .map(|entry| entry["target"]["propertyType"].as_str().unwrap_or(""))
        .collect();
    properties.sort_unstable();
    assert_eq!(properties, ["scaleX", "scaleY"]);
    assert_eq!(
        entries[0]["animator"]["keyframes"][1]["easing"]["type"],
        "hold"
    );
}

#[test]
fn uniform_scale_hold_and_bezier_preserve_both_axis_tracks() {
    let xml = animated_xml("").replace(
        "<Name>Scale</Name>",
        &format!(
            "<Name>Scale</Name><Keyframes>0,100.,4,0,0,0,0,0;{},150.,5,0,1,0.2,3,0.4;{},200.,0,0,0,0,0,0;</Keyframes>",
            TICKS,
            2 * TICKS
        ),
    );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let document = crate::tests::support::project_document_with_media(
        parsed.single_sequence().unwrap(),
        &parsed.media,
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    let editable =
        fx_schema::EditableFxCompositionDocument::from_json_value(document.clone()).unwrap();
    for (property_type, property) in [
        (fx_schema::PropType::ScaleX, "scaleX"),
        (fx_schema::PropType::ScaleY, "scaleY"),
    ] {
        let entry_json = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"].as_str() == Some(property))
            .unwrap();
        assert_eq!(
            entry_json["animator"]["keyframes"][1]["easing"]["type"],
            "hold"
        );
        assert_eq!(
            entry_json["animator"]["keyframes"][2]["easing"]["type"],
            "cubicBezier"
        );
        let entry = editable
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|entry| {
                matches!(
                    &entry.target,
                    fx_schema::PropertyTarget::LayerProperty(target)
                        if target.property_type() == property_type
                )
            })
            .unwrap();
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
        else {
            panic!("expected editable Scale keyframes");
        };
        assert_eq!(track.keyframes().len(), 3);
        // Runtime values at every former sample are asserted in
        // fx_composition/tests/premiere_keyframes.rs, using public file import.
    }
}

#[test]
fn disabled_uniform_scale_keys_reject_instead_of_becoming_visible() {
    let xml = animated_xml("").replace(
        "<Name>Scale</Name>",
        &format!(
            "<Name>Scale</Name><IsTimeVarying>false</IsTimeVarying><Keyframes>0,100.,4,0,0,0,0,0;{},150.,0,0,0,0,0,0;</Keyframes>",
            TICKS
        ),
    );
    let error = inspect_project(&xml, Some("sequence-1"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("disabled IsTimeVarying"), "{error}");
}

#[test]
fn uniform_scale_retains_keys_outside_trim_and_signed_times() {
    let xml = animated_xml("")
        .replace(
            "<Name>Scale</Name>",
            &format!(
                "<Name>Scale</Name><Keyframes>0,100.,0,0,0,0,0,0;{},175.,0,0,0,0,0,0;</Keyframes>",
                7 * TICKS
            ),
        )
        .replace(
            "<InPoint>0</InPoint>",
            &format!("<InPoint>{}</InPoint>", TICKS),
        )
        .replace(
            "<OutPoint>1270080000000</OutPoint>",
            &format!("<OutPoint>{}</OutPoint>", 6 * TICKS),
        );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let clip = parsed
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(
        clip.animations[0]
            .keys()
            .iter()
            .map(|key| key.source_ticks)
            .collect::<Vec<_>>(),
        [0, 7 * TICKS]
    );
    let doc = crate::tests::support::project_document_with_media(
        parsed.single_sequence().unwrap(),
        &parsed.media,
    );
    for entry in doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
    {
        assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], -1000);
        assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 6000);
    }
}

#[test]
fn keyed_motion_and_opacity_hold_their_first_key_before_the_consumed_source_interval() {
    // Premiere holds a keyed property's first key before that key, also over
    // a trim that starts earlier, whatever its static StartKeyframe is. Each
    // first key below lies one second after the In and differs from its
    // static value. The FX animator holds it there too, so import keeps the
    // two real keys and adds no trim key.
    let keyed = |xml: String, name: &str, keys: String| {
        xml.replace(
            &format!("<Name>{name}</Name>"),
            &format!("<Name>{name}</Name><Keyframes>{keys}</Keyframes>"),
        )
    };
    let scalar = |first: &str, last: &str, from: i64| {
        format!(
            "{from},{first},0,0,0,0,0,0;{},{last},0,0,0,0,0,0;",
            from + TICKS
        )
    };
    let position = format!(
        "{TICKS},0.25:0.75,0,0,0,0,0,0,0,0,0,0,0,0;{},0.75:0.25,0,0,0,0,0,0,0,0,0,0,0,0;",
        2 * TICKS
    );
    // Rotation keys from 2 s on a placement trimmed to start at 1 s.
    let trimmed_rotation = animated_xml(&scalar("13.", "90.", 2 * TICKS))
        .replace(
            "<InPoint>0</InPoint>",
            &format!("<InPoint>{TICKS}</InPoint>"),
        )
        .replace(
            "<OutPoint>1270080000000</OutPoint>",
            &format!("<OutPoint>{}</OutPoint>", 6 * TICKS),
        );
    // The 26.3 Opacity (static 50) with time-varying keys.
    let opacity = premiere_26_3_opacity_xml().replace(
        "<IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType>",
        "<ParameterControlType>2</ParameterControlType>",
    );
    let cases = [
        (
            keyed(animated_xml(""), "Scale", scalar("89.", "94.", TICKS)),
            PrAnimatedProperty::UniformScale,
            TICKS,
            &[("scaleX", 89.0), ("scaleY", 89.0)][..],
        ),
        (
            keyed(animated_xml(""), "Position", position),
            PrAnimatedProperty::Position,
            TICKS,
            &[("positionX", 0.25 * 1920.0), ("positionY", 0.75 * 1080.0)],
        ),
        (
            trimmed_rotation,
            PrAnimatedProperty::Rotation,
            2 * TICKS,
            &[("rotation", 13.0)],
        ),
        (
            keyed(opacity, "Opacity", scalar("80.", "20.", TICKS)),
            PrAnimatedProperty::Opacity,
            TICKS,
            &[("opacity", 80.0)],
        ),
    ];
    for (xml, property, first_ticks, tracks) in cases {
        let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1"))
            .unwrap_or_else(|error| panic!("{property:?}: {error}"));
        let sequence = parsed.single_sequence().unwrap();
        let clip = sequence.video_occurrences().next().unwrap();
        let animation = clip
            .animations
            .iter()
            .find(|animation| animation.property() == property)
            .unwrap_or_else(|| panic!("{property:?} keys were not read"));
        let native_times: Vec<_> = match animation.point_keys() {
            Some(keys) => keys.iter().map(|key| key.source_ticks).collect(),
            None => animation
                .keys()
                .iter()
                .map(|key| key.source_ticks)
                .collect(),
        };
        assert_eq!(
            native_times,
            [first_ticks, first_ticks + TICKS],
            "{property:?}"
        );
        let document = crate::tests::support::project_document_with_media(sequence, &parsed.media);
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        for &(fx_property, first_value) in tracks {
            let entry = entries
                .iter()
                .find(|entry| entry["target"]["propertyType"] == fx_property)
                .unwrap_or_else(|| panic!("no {fx_property} track"));
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2, "{fx_property}");
            assert_eq!(keys[0]["layerTime"], 1000, "{fx_property}");
            assert_eq!(keys[0]["value"]["value"], first_value, "{fx_property}");
            assert_eq!(keys[1]["layerTime"], 2000, "{fx_property}");
        }
    }
}

#[test]
fn uniform_scale_sets_both_axes_and_ignores_scale_width_in_both_layouts() {
    // Premiere leaves Scale Width at 100 when Uniform Scale changes Scale; the AME
    // render of clip S2 of the 26.5 fixture (Scale 60) shows 60% on both axes.
    let keys = format!("<Keyframes>0,60.,0,0,0,0,0,0;{TICKS},80.,0,0,0,0,0,0;</Keyframes>");
    let layouts = [
        (
            animated_xml(""),
            "<Name>Scale</Name><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe>",
            "<Name>Scale</Name><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,60.,0,0,0,0,0,0</StartKeyframe>",
        ),
        (
            premiere_26_5_xml(),
            "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe>",
            "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,60.,0,0,0,0,0,0</StartKeyframe>",
        ),
    ];
    for (xml, default_scale, scale_60) in layouts {
        assert!(xml.contains(default_scale));
        for keyed in [false, true] {
            let scale = if keyed {
                format!("{scale_60}{keys}")
            } else {
                scale_60.to_owned()
            };
            let parsed = crate::format::inspect_project_with_media(
                &xml.replace(default_scale, &scale),
                Some("sequence-1"),
            )
            .unwrap();
            let sequence = parsed.single_sequence().unwrap();
            let clip = sequence.video_occurrences().next().unwrap();
            assert_eq!(clip.transform.scale, [60.0, 60.0]);
            let document =
                crate::tests::support::project_document_with_media(sequence, &parsed.media);
            let mut tracks: Vec<_> = document["composition"]["dynamics"]["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|entry| {
                    let keys: Vec<_> = entry["animator"]["keyframes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|key| {
                            (
                                key["layerTime"].as_i64().unwrap(),
                                key["value"]["value"].as_f64().unwrap(),
                            )
                        })
                        .collect();
                    (entry["target"]["propertyType"].as_str().unwrap(), keys)
                })
                .collect();
            tracks.sort_by_key(|(property, _)| *property);
            let expected = if keyed {
                let keys = vec![(0, 60.0), (1000, 80.0)];
                vec![("scaleX", keys.clone()), ("scaleY", keys)]
            } else {
                Vec::new()
            };
            assert_eq!(tracks, expected, "keyed: {keyed}");
        }
    }
}

/// Supplementary legacy Motion graph with the inert trailing controls saved by
/// the Creative/Podcast sources; no licensed project records are embedded here.
fn legacy_motion_with_passive_crop_xml() -> String {
    let references = (8..=11)
        .map(|id| format!("<Param Index=\"{}\" ObjectRef=\"{}\"/>", id - 1, id + 10))
        .collect::<String>();
    let mut records = String::new();
    for (id, name) in [
        (8, "Crop Left"),
        (9, "Crop Top"),
        (10, "Crop Right"),
        (11, "Crop Bottom"),
    ] {
        records.push_str(&format!(
            "<VideoComponentParam ObjectID=\"{}\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\" Version=\"9\"><Name>{name}</Name><ParameterID>{id}</ParameterID><ParameterControlType>2</ParameterControlType><IsTimeVarying>false</IsTimeVarying><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-3.4028234663852886e+38</LowerBound><UpperBound>3.4028234663852886e+38</UpperBound></VideoComponentParam>",
            id + 10
        ));
    }
    animated_xml("")
        .replace("</Params>", &format!("{references}</Params>"))
        .replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

#[test]
fn legacy_motion_passive_crop_preserves_native_uniform_scale() {
    // Creative 1154/Motion 2334 saves percentage Crop bounds; Podcast 2974/
    // Motion 5047 saves full scalar bounds. Their inactive Width must not
    // override static Uniform Scale, even with eleven controls.
    for (scale, width, lower, upper) in [
        (119.0, 100.0, "0", "100"),
        (
            100.0,
            50.0,
            "-3.4028234663852886e+38",
            "3.4028234663852886e+38",
        ),
    ] {
        let xml = legacy_motion_with_passive_crop_xml()
            .replace(
                "<Name>Scale</Name><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,100.,",
                &format!("<Name>Scale</Name><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,{scale},"),
            )
            .replace(
                "<Name>Scale Width</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,100.,",
                &format!("<Name>Scale Width</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,{width},"),
            )
            .replace(
                "<LowerBound>-3.4028234663852886e+38</LowerBound>",
                &format!("<LowerBound>{lower}</LowerBound>"),
            )
            .replace(
                "<UpperBound>3.4028234663852886e+38</UpperBound>",
                &format!("<UpperBound>{upper}</UpperBound>"),
            );
        let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
        let sequence = parsed.single_sequence().unwrap();
        let clip = sequence.video_occurrences().next().unwrap();
        assert_eq!(clip.transform.scale, [scale, scale]);
        assert_eq!(clip.transform.position, [0.5, 0.5]);
        assert_eq!(clip.transform.anchor_point, [0.5, 0.5]);
        assert_eq!(clip.transform.rotation, 0.0);
        assert!(clip.crop.is_default());
        assert!(clip.animations.is_empty());
        assert_eq!((clip.start_ticks, clip.end_ticks), (0, 5 * TICKS));
        assert_eq!((clip.in_ticks, clip.out_ticks), (0, 5 * TICKS));
        let document = crate::tests::support::project_document_with_media(sequence, &parsed.media);
        let video = &document["composition"]["layers"][0];
        assert_eq!(video["type"], "Video");
        assert_eq!(
            video["transform"]["scale"],
            serde_json::json!([scale, scale])
        );
        assert_eq!(
            video["transform"]["position"],
            serde_json::json!([960.0, 540.0])
        );
        assert!(document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn motion_constant_uniform_flag_keeps_scale_and_numeric_animation() {
    // Creative Motion 2746/parameter 3250 stores this singleton true key
    // beside the same true StartKeyframe, with no IsTimeVarying field.
    // The public native-record scaffold keeps its ordinary numeric keys.
    let rotation = format!("0,0.,0,0,0,0,0,0;{TICKS},90.,0,0,0,0,0,0;");
    for (flag, expected_scale) in [(true, [50.0, 50.0]), (false, [70.0, 50.0])] {
        for varying in ["", "<IsTimeVarying>true</IsTimeVarying>"] {
            let xml = legacy_motion_with_passive_crop_xml()
                .replace("<Keyframes></Keyframes>", &format!("<Keyframes>{rotation}</Keyframes>"))
                .replace(
                    "<Name>Scale</Name><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,100.,",
                    "<Name>Scale</Name><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,50.,",
                )
                .replace(
                    "<Name>Scale Width</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,100.,",
                    "<Name>Scale Width</Name><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,70.,",
                )
                .replace(
                    "<ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>",
                    &format!("<ParameterID>4</ParameterID>{varying}<StartKeyframe>-91445760000000000,{flag},0,0,0,0,0,0</StartKeyframe><Keyframes>0,{flag},0,0,0,0,0,0;</Keyframes>"),
                );
            let parsed =
                crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
            let sequence = parsed.single_sequence().unwrap();
            let clip = sequence.video_occurrences().next().unwrap();
            assert_eq!(clip.transform.scale, expected_scale);
            assert_eq!(clip.animations.len(), 1);
            assert_eq!(clip.animations[0].property(), PrAnimatedProperty::Rotation);
            assert_eq!(
                (
                    clip.start_ticks,
                    clip.end_ticks,
                    clip.in_ticks,
                    clip.out_ticks
                ),
                (0, 5 * TICKS, 0, 5 * TICKS)
            );
            let document =
                crate::tests::support::project_document_with_media(sequence, &parsed.media);
            assert_eq!(
                document["composition"]["layers"][0]["transform"]["scale"],
                serde_json::json!(expected_scale)
            );
            let entries = document["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0]["target"]["propertyType"], "rotation");
            assert_eq!(
                entries[0]["animator"]["keyframes"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }
    }
}

#[test]
fn motion_constant_uniform_flag_rejects_changes_and_malformed_keys() {
    let xml = legacy_motion_with_passive_crop_xml();
    for (wire, varying) in [
        ("0,false,0,0,0,0,0,0;", ""),
        ("0,true,0,0,0,0,0,0;1,false,0,0,0,0,0,0;", ""),
        (
            "0,true,0,0,0,0,0,0;",
            "<IsTimeVarying>false</IsTimeVarying>",
        ),
        ("0,true,0,0,0,0,0,0", ""),
        ("invalid,true,0,0,0,0,0,0;", ""),
        ("0,unknown,0,0,0,0,0,0;", ""),
        ("0,true,0,0,0,0,0;", ""),
        ("0,true,0,0,0,0,0,1;", ""),
    ] {
        let input = xml.replace(
            "<ParameterID>4</ParameterID>",
            &format!("<ParameterID>4</ParameterID>{varying}<Keyframes>{wire}</Keyframes>"),
        );
        let error = inspect_project_with_omissions(&input, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("VideoComponentParam:14"), "{wire}: {error}");
    }
}

#[test]
fn legacy_motion_passive_crop_preserves_duplicate_uniform_scale_keys() {
    // Podcast Motion 5626's exact first/last Linear key times and values;
    // the full eleven-key native record is checked in supplementary CLI evidence.
    let keys = "770515200000,100.,0,0,0,0,0,0;872121600000,120.,0,0,0,0,0,0;";
    let xml = legacy_motion_with_passive_crop_xml()
        .replace(
            "<Name>Scale</Name>",
            &format!("<Name>Scale</Name><Keyframes>{keys}</Keyframes>"),
        )
        .replace(
            "<Name>Scale Width</Name>",
            &format!("<Name>Scale Width</Name><Keyframes>{keys}</Keyframes>"),
        );
    let parsed = crate::format::inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let sequence = parsed.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.transform.scale, [100.0, 100.0]);
    assert!(clip.crop.is_default());
    assert_eq!(clip.animations.len(), 1);
    assert_eq!(
        clip.animations[0].property(),
        PrAnimatedProperty::UniformScale
    );
    assert_eq!(
        clip.animations[0]
            .keys()
            .iter()
            .map(|key| (key.source_ticks, key.value, key.easing))
            .collect::<Vec<_>>(),
        [
            (
                770_515_200_000,
                100.0,
                crate::schema::PrKeyframeEasing::Linear
            ),
            (
                872_121_600_000,
                120.0,
                crate::schema::PrKeyframeEasing::Linear
            ),
        ]
    );
    let document = crate::tests::support::project_document_with_media(sequence, &parsed.media);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    for property in ["scaleX", "scaleY"] {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["layerTime"], 3033);
        assert_eq!(keys[0]["value"]["value"], 100.0);
        assert_eq!(keys[1]["layerTime"], 3433);
        assert_eq!(keys[1]["value"]["value"], 120.0);
    }
}

#[test]
fn legacy_motion_passive_crop_rejects_active_or_malformed_controls() {
    let xml = legacy_motion_with_passive_crop_xml();
    let missing = xml.replace("<Param Index=\"10\" ObjectRef=\"21\"/>", "");
    assert!(inspect_project(&missing, Some("sequence-1"))
        .unwrap_err()
        .to_string()
        .contains("unsupported Motion parameter layout"));
    let start = xml.find("<VideoComponentParam ObjectID=\"18\"").unwrap();
    let end = start
        + xml[start..].find("</VideoComponentParam>").unwrap()
        + "</VideoComponentParam>".len();
    let crop_left = &xml[start..end];
    for (from, to, diagnostic) in [
        (
            "<Name>Crop Left</Name><ParameterID>8</ParameterID>",
            "<Name>Crop Top</Name><ParameterID>9</ParameterID>",
            "duplicate Motion ParameterID",
        ),
        (
            "<Name>Crop Left</Name>",
            "<Name>Not Crop</Name>",
            "unexpected Motion parameter",
        ),
        (
            "VideoComponentParam",
            "PointComponentParam",
            "unexpected Motion parameter",
        ),
        (
            "<ParameterControlType>2</ParameterControlType>",
            "<ParameterControlType>3</ParameterControlType>",
            "unexpected Motion parameter layout",
        ),
        (
            "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
            "invalid-class",
            "unexpected Motion parameter layout",
        ),
        (
            "<LowerBound>-3.4028234663852886e+38</LowerBound>",
            "<LowerBound>0</LowerBound>",
            "unexpected Motion parameter layout",
        ),
        (
            ",0.,0,0,0,0,0,0",
            ",NaN,0,0,0,0,0,0",
            "nonfinite initial value",
        ),
        (
            ",0.,0,0,0,0,0,0",
            ",1.,0,0,0,0,0,0",
            "nonzero legacy Motion Crop Left",
        ),
        (
            "<IsTimeVarying>false</IsTimeVarying>",
            "<Keyframes>0,0.,0,0,0,0,0,0;</Keyframes>",
            "animated Motion Crop Left",
        ),
        (
            "<IsTimeVarying>false</IsTimeVarying>",
            "<IsTimeVarying>true</IsTimeVarying>",
            "animated Motion Crop Left",
        ),
    ] {
        let altered = crop_left.replace(from, to);
        assert_ne!(altered, crop_left);
        let error = inspect_project(&xml.replace(crop_left, &altered), Some("sequence-1"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(diagnostic), "{diagnostic}: {error}");
    }
}

#[test]
fn uniform_scale_off_keeps_independent_axes_and_rejects_animated_scale() {
    // S6 of the 26.5 fixture: Uniform Scale off, Scale (height) 50, Scale Width 80.
    let xml = premiere_26_5_edit(&[
        (
            "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,",
            "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,50.,",
        ),
        (
            "<Name>Scale Width</Name><ParameterID>3</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,",
            "<Name>Scale Width</Name><ParameterID>3</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,80.,",
        ),
        (
            "<Name> </Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,true,",
            "<Name> </Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,false,",
        ),
    ]);
    let sequence = inspect_project(&xml, Some("sequence-1")).unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.transform.scale, [80.0, 50.0]);
    let height = "<StartKeyframe>-91445760000000000,50.,0,0,0,0,0,0</StartKeyframe>";
    assert!(xml.contains(height));
    let keyed = xml.replace(
        height,
        &format!("{height}<Keyframes>0,50.,0,0,0,0,0,0;{TICKS},70.,0,0,0,0,0,0;</Keyframes>"),
    );
    let error = inspect_project(&keyed, Some("sequence-1"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "VideoFilterComponent:10: animated Scale Height without Uniform Scale is unsupported"
        ),
        "{error}"
    );
}

#[test]
fn premiere_26_5_layout_reads_only_linear_anchor_point_and_scale_width_keys() {
    use crate::schema::{PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe};
    let width = "<Name>Scale Width</Name><ParameterID>3</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe>";
    let anchor = "<Name>Anchor Point</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>";
    let uniform =
        "<Name> </Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,true,";
    let keyed = |width_keys: &str, anchor_keys: &str, uniform_value: &str| {
        premiere_26_5_edit(&[
            (
                width,
                &format!("{width}<Keyframes>{width_keys}</Keyframes>"),
            ),
            (
                anchor,
                &format!("{anchor}<Keyframes>{anchor_keys}</Keyframes>"),
            ),
            (uniform, &uniform.replace("true", uniform_value)),
        ])
    };
    // The probe's key forms: Linear (mode 0) with Premiere's 1/6 influences,
    // and Anchor Point keys without spatial tangents (mode 0).
    let linear_width = format!("0,100.,0,0,0,0,0,0;{TICKS},50.,0,0,0,0,0,0;");
    let linear_anchor = format!(
        "0,0.5:0.5,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;{TICKS},0.25:0.75,0,0,0,0.16666666666666666,0,0.16666666666666666,0,0,0,0,0,0;"
    );
    let sequence = inspect_project(
        &keyed(&linear_width, &linear_anchor, "false"),
        Some("sequence-1"),
    )
    .unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.transform.scale, [100.0, 100.0]);
    assert_eq!(clip.transform.anchor_point, [0.5, 0.5]);
    let point = |source_ticks, value| PrPointKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    let scalar = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    assert_eq!(
        clip.animations,
        [
            PrPropertyAnimation::ScaleWidth(vec![scalar(0, 100.0), scalar(TICKS, 50.0)]),
            PrPropertyAnimation::AnchorPoint(vec![
                point(0, [0.5, 0.5]),
                point(TICKS, [0.25, 0.75])
            ]),
        ]
    );
    // Hold, Bezier and spatial forms stay unsupported, as do Scale Width
    // keys without a matching Scale track under Uniform Scale.
    let hold_width = format!("0,100.,4,0,0,0,0,0;{TICKS},50.,0,0,0,0,0,0;");
    let bezier_anchor = format!(
        "0,0.5:0.5,5,0,0,0.16666666666666666,0.3,0.33333333333333331,0,0,0,0,0,0;{TICKS},0.25:0.75,0,0,0.3,0.33333333333333331,0,0.16666666666666666,0,0,0,0,0,0;"
    );
    let spatial_anchor = format!(
        "0,0.5:0.5,0,0,0,0,0,0,5,0,0,0,0.1,0;{TICKS},0.25:0.75,0,0,0,0,0,0,5,0,-0.1,0,0,0;"
    );
    let width_rule = "VideoComponentParam:32: only Linear Scale Width keys convert";
    let anchor_rule =
        "PointComponentParam:35: only Linear Anchor Point keys without spatial tangents convert";
    for (width_keys, anchor_keys, uniform_value, expected) in [
        (&hold_width, &linear_anchor, "false", width_rule),
        (&linear_width, &bezier_anchor, "false", anchor_rule),
        (&linear_width, &spatial_anchor, "false", anchor_rule),
        (
            &linear_width,
            &linear_anchor,
            "true",
            "VideoFilterComponent:10: animated Scale Width is unsupported under Uniform Scale",
        ),
    ] {
        let error = inspect_project(
            &keyed(width_keys, anchor_keys, uniform_value),
            Some("sequence-1"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn no_animation_is_not_inferred_from_premiere_ui_keyframe_fields() {
    let project = inspect_project(SOURCE, Some("sequence-1")).unwrap();
    assert!(project
        .video_occurrences()
        .next()
        .unwrap()
        .animations
        .is_empty());
}

/// Opacity and Motion records as Premiere 26.5.1 saved them in
/// `feature_motion_opacity_26_5_strict.prproj` (new ObjectIDs, default values).
const PREMIERE_26_5_COMPONENTS: &str = r#"<VideoFilterComponent ObjectID="9" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="20"/><Param Index="1" ObjectRef="21"/><Param Index="2" ObjectRef="22"/></Params><ID>2</ID><Intrinsic>true</Intrinsic><DisplayName>Opacity</DisplayName></Component><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Opacity</MatchName></VideoFilterComponent>
<VideoComponentParam ObjectID="20" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Opacity</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="21" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10"><Name>Blend Mode</Name><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>10</ParameterControlType><ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,18,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>27</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="22" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="10"><Name>Blend Mode</Name><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>31</UpperBound></VideoComponentParam>
<VideoFilterComponent ObjectID="10" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="9"><Component Version="7"><Params Version="1"><Param Index="0" ObjectRef="30"/><Param Index="1" ObjectRef="31"/><Param Index="2" ObjectRef="32"/><Param Index="3" ObjectRef="33"/><Param Index="4" ObjectRef="34"/><Param Index="5" ObjectRef="35"/><Param Index="6" ObjectRef="36"/><Param Index="7" ObjectRef="37"/><Param Index="8" ObjectRef="38"/><Param Index="9" ObjectRef="39"/><Param Index="10" ObjectRef="40"/></Params><ID>1</ID><Intrinsic>true</Intrinsic><DisplayName>Motion</DisplayName></Component><PremiereFilterPrivateData Encoding="base64" BinaryHash="912f5b46-c9ac-9035-33ce-63970000000e">AWI=</PremiereFilterPrivateData><VideoFilterType>2</VideoFilterType><MatchName>AE.ADBE Motion</MatchName></VideoFilterComponent>
<PointComponentParam ObjectID="30" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>
<VideoComponentParam ObjectID="31" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>10000</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="32" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Scale Width</Name><ParameterID>3</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>10000</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="33" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="10"><Name> </Name><ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe></VideoComponentParam>
<VideoComponentParam ObjectID="34" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Rotation</Name><ParameterControlType>3</ParameterControlType><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound></VideoComponentParam>
<PointComponentParam ObjectID="35" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="4"><Name>Anchor Point</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe></PointComponentParam>
<VideoComponentParam ObjectID="36" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="10"><Name>Anti-flicker Filter</Name><ParameterID>7</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="37" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Crop Left</Name><ParameterID>8</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="38" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Crop Top</Name><ParameterID>9</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="39" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Crop Right</Name><ParameterID>10</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
<VideoComponentParam ObjectID="40" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="10"><Name>Crop Bottom</Name><ParameterID>11</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound></VideoComponentParam>
"#;

/// The one-clip source with Premiere 26.5 intrinsic Opacity and Motion.
fn premiere_26_5_xml() -> String {
    let defaults = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>";
    let chain = "<VideoComponentChain ObjectID=\"4\"><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"9\"/><Component Index=\"1\" ObjectRef=\"10\"/></Components></ComponentChain></VideoComponentChain>";
    let xml = SOURCE.replace(defaults, chain);
    assert_ne!(xml, SOURCE);
    xml.replace(
        "</PremiereData>",
        &format!("{PREMIERE_26_5_COMPONENTS}</PremiereData>"),
    )
}

/// Replaces one exact fragment of the 26.5 source, which must occur in it.
fn premiere_26_5_edit(edits: &[(&str, &str)]) -> String {
    edits.iter().fold(premiere_26_5_xml(), |xml, (from, to)| {
        assert!(xml.contains(from), "{from}");
        xml.replace(from, to)
    })
}

fn premiere_26_5_error(edits: &[(&str, &str)]) -> String {
    inspect_project(&premiere_26_5_edit(edits), Some("sequence-1"))
        .unwrap_err()
        .to_string()
}

#[test]
fn premiere_26_5_layout_reads_motion_opacity_keys_and_its_normal_pair() {
    // S1 Position, S5 Opacity and the K clip's Rotation keys (Hold, then Linear).
    let xml = premiere_26_5_edit(&[
        (
            "<Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,",
            "<Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.625:0.65,",
        ),
        (
            "<Name>Opacity</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,100.,",
            "<Name>Opacity</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,50.,",
        ),
        (
            "<Name>Rotation</Name><ParameterControlType>3</ParameterControlType><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>",
            "<Name>Rotation</Name><ParameterControlType>3</ParameterControlType><IsTimeVarying>true</IsTimeVarying><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><Keyframes>127008000000,0.,4,0,0,0.16666666666666666,0,0.33333333333333331;254016000000,20.,0,0,40,0.16666666666666666,-6,0.16666666666666666;</Keyframes>",
        ),
    ]);
    let sequence = inspect_project(&xml, Some("sequence-1")).unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.transform.position, [0.625, 0.65]);
    assert_eq!(clip.transform.scale, [100.0, 100.0]);
    assert_eq!(clip.opacity, 50.0);
    assert_eq!(clip.blend_mode, crate::schema::PrBlendMode::Normal);
    assert_eq!(clip.animations.len(), 1);
    assert_eq!(clip.animations[0].property(), PrAnimatedProperty::Rotation);
    assert_eq!(
        clip.animations[0]
            .keys()
            .iter()
            .map(|key| (key.source_ticks, key.value, key.easing))
            .collect::<Vec<_>>(),
        [
            (TICKS / 2, 0.0, crate::schema::PrKeyframeEasing::Linear),
            (TICKS, 20.0, crate::schema::PrKeyframeEasing::Hold),
        ]
    );
}

/// An edit for [`premiere_26_5_edit`] that sets the static value of Motion
/// Crop `name` (Left, Top, Right or Bottom; parameter `id`) to `value`.
fn motion_crop_edit(name: &str, id: u32, value: &str) -> (String, String) {
    let record = format!("<Name>Crop {name}</Name><ParameterID>{id}</ParameterID>");
    (
        format!("{record}<StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>"),
        format!("{record}<StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>"),
    )
}

#[test]
fn premiere_26_5_static_motion_crop_imports_as_a_crop_guide_in_the_clip_frame() {
    // A moved clip whose Motion crops Left 20.25, Top 10 and Right 24.5 of
    // its 1920x1080 frame. A nonzero edge is saved with a `CurrentValue`.
    let (left_from, left_to) = motion_crop_edit("Left", 8, "20.25");
    let (top_from, top_to) = motion_crop_edit("Top", 9, "10.");
    let (right_from, right_to) = motion_crop_edit("Right", 10, "24.5");
    let left_to = format!("{left_to}<CurrentValue>20.25</CurrentValue>");
    let right_to = format!("{right_to}<CurrentValue>24.5</CurrentValue>");
    let xml = premiere_26_5_edit(&[
        (
            "<Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,",
            "<Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.25:0.5,",
        ),
        (
            "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,100.,",
            "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound><StartKeyframe>-91445760000000000,50.,",
        ),
        (&left_from, &left_to),
        (&top_from, &top_to),
        (&right_from, &right_to),
    ]);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(
        clip.crop,
        crate::schema::PrStaticCrop {
            left: 20.25,
            top: 10.0,
            right: 24.5,
            bottom: 0.0,
            edge_feather: 0.0,
        }
    );
    assert_eq!(clip.transform.scale, [50.0, 50.0]);
    assert_eq!(clip.effects_above_mask, 0);

    // The existing Crop lowering: the video, its guide in source pixels with
    // the video's transform, and the black canvas.
    let document = crate::tests::support::project_document_with_media(sequence, &project.media);
    let layers = document["composition"]["layers"].as_array().unwrap();
    let names: Vec<_> = layers.iter().map(|layer| &layer["name"]).collect();
    assert_eq!(
        names,
        [
            "Premiere video 1",
            "Premiere Crop guide 1",
            "Premiere black canvas"
        ]
    );
    let (video, guide) = (&layers[0], &layers[1]);
    assert_eq!(
        video["transform"]["position"],
        serde_json::json!([480.0, 540.0])
    );
    assert_eq!(video["transform"]["scale"], serde_json::json!([50.0, 50.0]));
    assert_eq!(guide["transform"], video["transform"]);
    assert_eq!(guide["rect"]["position"], serde_json::json!([388.8, 108.0]));
    assert_eq!(guide["rect"]["size"], serde_json::json!([1060.8, 972.0]));
    let masks = video["masks"].as_array().unwrap();
    assert_eq!(masks.len(), 1);
    assert_eq!(masks[0]["layer"], guide["id"]);
    assert_eq!(masks[0]["feather"], serde_json::json!([0.0, 0.0]));
}

#[test]
fn premiere_26_5_keyed_or_invalid_motion_crop_omits_the_clip() {
    // The keyed Crop Top of the Premiere 26.5.1 probe project.
    let top = "<Name>Crop Top</Name><ParameterID>9</ParameterID><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>";
    let keyed = format!("{top}<Keyframes>127008000000,0.,0,0,0,0.16666666666666666,10,0.16666666666666666;381024000000,10.,0,0,10,0.16666666666666666,0,0.16666666666666666;</Keyframes>")
        .replace("<Name>Crop Top</Name>", "<Name>Crop Top</Name><IsTimeVarying>true</IsTimeVarying>");
    let time_varying = top.replace(
        "<Name>Crop Top</Name>",
        "<Name>Crop Top</Name><IsTimeVarying>true</IsTimeVarying>",
    );
    for (edits, expected) in [
        (
            vec![(top.to_owned(), keyed)],
            "VideoComponentParam:38: animated Motion Crop Top is unsupported",
        ),
        (
            vec![(top.to_owned(), time_varying)],
            "VideoComponentParam:38: animated Motion Crop Top is unsupported",
        ),
        (
            vec![motion_crop_edit("Left", 8, "100.5")],
            "VideoFilterComponent:10: Motion Crop: invalid Premiere project: Crop edge percentages must be finite and within 0..=100",
        ),
        (
            vec![motion_crop_edit("Bottom", 11, "-1.")],
            "VideoFilterComponent:10: Motion Crop: invalid Premiere project: Crop edge percentages must be finite and within 0..=100",
        ),
        (
            vec![motion_crop_edit("Right", 10, "nan")],
            "VideoComponentParam:39: nonfinite initial value",
        ),
        (
            vec![motion_crop_edit("Right", 10, "a")],
            "VideoComponentParam:39: invalid initial value",
        ),
        (
            vec![
                motion_crop_edit("Left", 8, "60."),
                motion_crop_edit("Right", 10, "40."),
            ],
            "VideoFilterComponent:10: Motion Crop: invalid Premiere project: opposing Crop edges must leave a positive visible area",
        ),
        (
            vec![motion_crop_edit("Top", 9, "10.,0")],
            "VideoComponentParam:38: unexpected Premiere keyframe shape",
        ),
    ] {
        let edits: Vec<_> = edits
            .iter()
            .map(|(from, to)| (from.as_str(), to.as_str()))
            .collect();
        let error = premiere_26_5_error(&edits);
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

/// The 26.3-layout Opacity records of `feature_opacity_screen_strict.prproj`
/// (50% Opacity, Blend Mode pair (18, 0)), which AME renders as a mix of the layers.
const PREMIERE_26_3_OPACITY: &str = r#"<VideoFilterComponent ObjectID="200" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="7"><Component Version="5"><Params Version="1"><Param Index="0" ObjectRef="201" /><Param Index="1" ObjectRef="202" /><Param Index="2" ObjectRef="203" /></Params><ID>2</ID><DisplayName>Opacity</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Opacity</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>
<VideoComponentParam ObjectID="201" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="9"><Name>Opacity</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,50.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="202" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="9"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>10</ParameterControlType><StartKeyframe>-91445760000000000,18,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>26</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="203" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="9"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>31</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>
"#;

/// The one-clip source whose occurrence has default Motion and the 26.3 Opacity.
fn premiere_26_3_opacity_xml() -> String {
    let defaults = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>";
    let chain = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"200\"/></Components></ComponentChain></VideoComponentChain>";
    let xml = SOURCE.replace(defaults, chain);
    assert_ne!(xml, SOURCE);
    xml.replace(
        "</PremiereData>",
        &format!("{PREMIERE_26_3_OPACITY}</PremiereData>"),
    )
}

/// `xml` with a second, plain occurrence (`VideoClipTrackItem:90`) of the same
/// source at 5 to 10 s, which converts whatever the first occurrence's Opacity is.
fn with_plain_second_clip(xml: &str) -> String {
    let track_items = "<TrackItem ObjectRef=\"3\"/>";
    assert!(xml.contains(track_items));
    xml.replace(track_items, "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"90\"/>")
        .replace(
            "</PremiereData>",
            "<VideoClipTrackItem ObjectID=\"90\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"91\"/></ComponentOwner><TrackItem><Start>1270080000000</Start><End>2540160000000</End></TrackItem><SubClip ObjectRef=\"92\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>\
<VideoComponentChain ObjectID=\"91\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>\
<SubClip ObjectID=\"92\"><Clip ObjectRef=\"93\"/><Name>Second</Name></SubClip>\
<VideoClip ObjectID=\"93\"><Clip><Source ObjectRef=\"7\"/><InPoint>1270080000000</InPoint><OutPoint>2540160000000</OutPoint></Clip></VideoClip></PremiereData>",
        )
}

#[test]
fn each_blend_pair_reads_as_its_mode_in_both_layouts() {
    // Each layout with the fragments before its primary and legacy Blend Mode values.
    for (layout, xml, primary, legacy) in [
        (
            "26.3",
            premiere_26_3_opacity_xml(),
            "<ParameterControlType>10</ParameterControlType><StartKeyframe>-91445760000000000,",
            "<ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,",
        ),
        (
            "26.5",
            premiere_26_5_xml(),
            "<ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,",
            "<ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,",
        ),
    ] {
        let xml = with_plain_second_clip(&xml);
        // Parameter 2 selects the mode: a scripted `set_blend_mode "Normal"`
        // writes (1, 0), which renders as Color Burn. Dissolve (6) is unmeasured.
        for (pair, expected) in [
            ((18, 0), PrBlendMode::Normal),
            ((22, 10), PrBlendMode::Screen),
            ((1, 0), PrBlendMode::ColorBurn),
            (
                (6, 1),
                PrBlendMode::Unmeasured {
                    primary: 6,
                    legacy: 1,
                },
            ),
        ] {
            let edited = xml
                .replace(&format!("{primary}18,"), &format!("{primary}{},", pair.0))
                .replace(&format!("{legacy}0,"), &format!("{legacy}{},", pair.1));
            assert!(
                edited.contains(&format!("{primary}{},", pair.0))
                    && edited.contains(&format!("{legacy}{},", pair.1)),
                "{layout}"
            );
            // The reader keeps every pair; the converter reports its approximation.
            let (project, omissions) =
                inspect_project_with_omissions(&edited, Some("sequence-1")).unwrap();
            assert!(omissions.is_empty(), "{layout} {pair:?}: {omissions:?}");
            let clips: Vec<_> = project.sequences[0].video_occurrences().collect();
            assert_eq!(clips.len(), 2, "{layout} {pair:?}");
            assert_eq!(clips[0].blend_mode, expected, "{layout} {pair:?}");
            assert_eq!(
                clips[1].blend_mode,
                PrBlendMode::Normal,
                "{layout} {pair:?}"
            );
        }
    }
}

#[test]
fn premiere_26_5_layout_with_a_missing_or_extra_parameter_rejects() {
    let crop_references = (7..=10)
        .map(|index| format!("<Param Index=\"{index}\" ObjectRef=\"{}\"/>", 30 + index))
        .collect::<String>();
    for edit in [
        // Without Crop Bottom, or without all of Motion Crop: the 26.3 list without `Bypass`.
        ("<Param Index=\"10\" ObjectRef=\"40\"/>", ""),
        (crop_references.as_str(), ""),
        (
            "<Param Index=\"10\" ObjectRef=\"40\"/>",
            "<Param Index=\"10\" ObjectRef=\"40\"/><Param Index=\"11\" ObjectRef=\"40\"/>",
        ),
    ] {
        let error = premiere_26_5_error(&[edit]);
        assert!(
            error.contains("VideoFilterComponent:10: unsupported Motion parameter layout"),
            "{error}"
        );
    }
    let error = premiere_26_5_error(&[("<Param Index=\"2\" ObjectRef=\"22\"/>", "")]);
    assert!(
        error.contains("VideoFilterComponent:9: unsupported Opacity parameter layout"),
        "{error}"
    );
}

#[test]
fn premiere_26_5_motion_parameters_are_checked_against_their_table() {
    let rotation = "<Name>Rotation</Name><ParameterControlType>3</ParameterControlType>";
    let scale = "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>200</UpperUIBound>";
    for (from, to, record) in [
        // A wrong or missing control type, and the 26.3 control type where 26.5 has none.
        (
            rotation,
            "<Name>Rotation</Name><ParameterControlType>2</ParameterControlType>",
            "VideoComponentParam:34",
        ),
        (rotation, "<Name>Rotation</Name>", "VideoComponentParam:34"),
        (
            "<Name>Position</Name><ParameterID>1</ParameterID>",
            "<Name>Position</Name><ParameterControlType>6</ParameterControlType><ParameterID>1</ParameterID>",
            "PointComponentParam:30",
        ),
        // Wrong bounds: another range, the Scale UI bound of Premiere 9-14, an added
        // lower UI bound, and the 26.3 bounds of the uniform-scale flag.
        (
            "<LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound>",
            "<LowerBound>-360</LowerBound><UpperBound>360</UpperBound>",
            "VideoComponentParam:34",
        ),
        (
            scale,
            "<Name>Scale</Name><ParameterID>2</ParameterID><UpperUIBound>100</UpperUIBound>",
            "VideoComponentParam:31",
        ),
        (
            scale,
            "<Name>Scale</Name><ParameterID>2</ParameterID><LowerUIBound>0</LowerUIBound><UpperUIBound>200</UpperUIBound>",
            "VideoComponentParam:31",
        ),
        (
            "<Name> </Name><ParameterID>4</ParameterID>",
            "<Name> </Name><ParameterID>4</ParameterID><LowerBound>false</LowerBound><UpperBound>true</UpperBound>",
            "VideoComponentParam:33",
        ),
        // The uniform-scale flag's ClassID on Rotation.
        (
            "<VideoComponentParam ObjectID=\"34\" ClassID=\"fe47129e-6c94-4fc0-95d5-c056a517aaf3\"",
            "<VideoComponentParam ObjectID=\"34\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\"",
            "VideoComponentParam:34",
        ),
    ] {
        let error = premiere_26_5_error(&[(from, to)]);
        assert!(
            error.contains(&format!("{record}: unexpected Motion parameter layout")),
            "{to}: {error}"
        );
    }
}

#[test]
fn premiere_26_3_motion_keeps_accepting_the_fields_that_older_saves_vary() {
    // Premiere 10.3 wrote Scale with an UpperUIBound of 100, and the synthetic
    // records carry no ClassID, control type or bounds; the 26.5 check would reject both.
    let scale = "<Name>Scale</Name><ParameterID>2</ParameterID>";
    let xml = animated_xml("");
    assert!(xml.contains(scale));
    let xml = xml.replace(
        scale,
        "<Name>Scale</Name><ParameterControlType>2</ParameterControlType><ParameterID>2</ParameterID><UpperUIBound>100</UpperUIBound>",
    );
    let sequence = inspect_project(&xml, Some("sequence-1")).unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.transform.scale, [100.0, 100.0]);
}

#[test]
fn intrinsic_parameter_bypass_omits_the_clip_in_both_layouts() {
    // No Premiere save writes `Bypass` on a Motion or Opacity parameter.
    let rotation = "<Name>Rotation</Name><ParameterControlType>3</ParameterControlType>";
    let opacity = "<Name>Opacity</Name><ParameterID>1</ParameterID>";
    for value in ["true", "false"] {
        let error =
            premiere_26_5_error(&[(rotation, &format!("{rotation}<Bypass>{value}</Bypass>"))]);
        assert!(
            error.contains("VideoComponentParam:34: unsupported Motion parameter Bypass"),
            "{value}: {error}"
        );
        let error =
            premiere_26_5_error(&[(opacity, &format!("{opacity}<Bypass>{value}</Bypass>"))]);
        assert!(
            error.contains("VideoComponentParam:20: unsupported Opacity parameter Bypass"),
            "{value}: {error}"
        );
    }
    let xml = animated_xml("");
    assert!(xml.contains("<Name>Rotation</Name>"));
    let bypassed = xml.replace(
        "<Name>Rotation</Name>",
        "<Name>Rotation</Name><Bypass>true</Bypass>",
    );
    let error = inspect_project(&bypassed, Some("sequence-1"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("VideoComponentParam:15: unsupported Motion parameter Bypass"),
        "{error}"
    );
    let xml = premiere_26_3_opacity_xml();
    let opacity = "<Name>Opacity</Name><IsTimeVarying>false</IsTimeVarying>";
    assert!(xml.contains(opacity));
    let bypassed = xml.replace(opacity, &format!("{opacity}<Bypass>true</Bypass>"));
    let error = inspect_project(&bypassed, Some("sequence-1"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("VideoComponentParam:201: unsupported Opacity parameter Bypass"),
        "{error}"
    );
}
