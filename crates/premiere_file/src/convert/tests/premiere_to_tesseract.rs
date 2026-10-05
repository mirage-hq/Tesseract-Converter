use crate::{
    convert::{premiere_to_tesseract, tesseract_to_premiere},
    error::BuildError,
    format::{inspect_project_with_media, FrameRate, MediaId, PrMedia},
    media::{MediaFacts, VideoMedia},
    schema::{
        PrCornerPin, PrEffect, PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams,
        PrGaussianBlur, PrKeyframeEasing, PrLinearWipe, PrPointKeyframe, PrPropertyAnimation,
        PrScalarKeyframe, PrSequence, PrStaticCrop, PrTransform, PrVideoItem, PrVideoTrack,
        PrVideoTransition, PrVideoTransitionKind, TICKS, TICKS_PER_MILLISECOND, TRANSFORM_OPACITY,
        TRANSFORM_POSITION, TRANSFORM_ROTATION, TRANSFORM_SCALE_HEIGHT, TRANSFORM_SCALE_WIDTH,
        TRANSFORM_SHUTTER_ANGLE,
    },
    tests::support::{
        clip_of, keyed_opacity_mask, left_crop, nested_sequence, opacity_mask, project_document,
        sequence_of, text_graphic, transform_effect, video_media, video_sequence,
        DEFAULT_PR_TRANSFORM,
    },
};
use fx_schema::{
    animator::{PropertyKeyframe, PropertyKeyframeTrack},
    AssetId,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read};

const THIRTY_FPS_TICKS: i64 = FrameRate::Fps30.ticks_per_frame();

// The strict fixture retains the Adobe-authored point values and resolved
// handles, with its source clock relocated to 0.5–3.5 seconds.
fn native_straight_position_keys() -> Vec<PrPointKeyframe> {
    let (sequence, _) = native_fixture(
        "feature_motion_position_path_strict.prproj",
        "c8acf9c1-34b2-4086-9f55-d528950a7059",
    );
    sequence.video_tracks[0].clip(0).animations[0]
        .point_keys()
        .unwrap()
        .to_vec()
}

// Match fx_composition::animator::keyframes: spatial handles select a
// parametric cubic, not arc-length traversal. These samples have Linear timing.
fn sample_linear_fx_point_segment(
    left: &PropertyKeyframe,
    right: &PropertyKeyframe,
    layer_ms: i64,
) -> f64 {
    assert_eq!(right.easing(), fx_schema::PropertyKeyframeEasing::Linear);
    let fx_schema::PropertyValue::Float(from) = left.value() else {
        panic!("expected a float point axis");
    };
    let fx_schema::PropertyValue::Float(to) = right.value() else {
        panic!("expected a float point axis");
    };
    let progress = (layer_ms - left.layer_time().as_millis()) as f64
        / (right.layer_time().as_millis() - left.layer_time().as_millis()) as f64;
    let outgoing = left.spatial_out_tangent();
    let incoming = right.spatial_in_tangent();
    if outgoing.is_none() && incoming.is_none() {
        return from + (to - from) * progress;
    }
    let first = from + outgoing.unwrap_or((to - from) / 3.0);
    let second = to + incoming.unwrap_or((from - to) / 3.0);
    let inverse = 1.0 - progress;
    inverse.powi(3) * from
        + 3.0 * inverse * inverse * progress * first
        + 3.0 * inverse * progress * progress * second
        + progress.powi(3) * to
}

#[test]
fn straight_position_native_handles_follow_temporal_traversal() {
    let keys = native_straight_position_keys();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].source_ticks, TICKS / 2);
    assert_eq!(keys[1].source_ticks, 7 * TICKS / 2);
    assert_eq!(keys[0].easing, PrKeyframeEasing::Linear);
    assert_eq!(keys[1].easing, PrKeyframeEasing::Linear);
    assert_eq!(
        keys[0].spatial_out_tangent,
        Some([0.06640624987582365, -7.064254126110115e-9])
    );
    assert_eq!(
        keys[1].spatial_in_tangent,
        Some([-0.06640624987582365, 7.064254126110115e-9])
    );
    let dimensions = [1920, 1080];
    let (x, y) = super::position_tracks(
        &PrPropertyAnimation::Position(keys.clone()),
        TICKS,
        fx_schema::LayerId::new(1),
        dimensions,
    )
    .unwrap();
    for (axis, track) in [x, y].iter().enumerate() {
        let converted = track.keyframes();
        assert_eq!(converted[0].layer_time().as_millis(), -500);
        assert_eq!(converted[1].layer_time().as_millis(), 2500);
        for (native, fx) in keys.iter().zip(converted) {
            assert_eq!(
                fx.value(),
                &fx_schema::PropertyValue::Float(native.value[axis] * f64::from(dimensions[axis]))
            );
            assert_eq!(
                fx.easing(),
                crate::convert::keyframes::fx_easing(native.easing)
            );
        }
        let expected = (keys[0].value[axis] + (keys[1].value[axis] - keys[0].value[axis]) / 4.0)
            * f64::from(dimensions[axis]);
        let actual = sample_linear_fx_point_segment(&converted[0], &converted[1], 250);
        assert!(
            (actual - expected).abs() < 1e-6,
            "axis {axis}: {actual} != {expected}"
        );
        assert!(!track.has_spatial_tangents());
    }
}

#[test]
fn straight_position_normalization_keeps_curved_neighbor_sides() {
    // Supplemental edits to the parsed native straight pair: curved paths on
    // either side, and non-Linear arrival easing on the straight segment.
    let mut keys = native_straight_position_keys();
    let mut before = keys[0].clone();
    before.source_ticks -= TICKS;
    before.value[0] -= 0.2;
    before.spatial_out_tangent = Some([0.04, 0.08]);
    keys[0].spatial_in_tangent = Some([-0.03, 0.01]);
    keys[0].easing = PrKeyframeEasing::Hold;
    keys[1].easing = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    keys[1].spatial_out_tangent = Some([0.04, 0.08]);
    let mut after = keys[1].clone();
    after.source_ticks += TICKS;
    after.value[0] += 0.2;
    after.spatial_in_tangent = Some([-0.03, -0.01]);
    after.spatial_out_tangent = None;
    keys.insert(0, before);
    keys.push(after);
    let dimensions = [1920, 1080];
    let (x, y) = super::position_tracks(
        &PrPropertyAnimation::Position(keys.clone()),
        TICKS,
        fx_schema::LayerId::new(1),
        dimensions,
    )
    .unwrap();
    for (axis, track) in [x, y].iter().enumerate() {
        let converted = track.keyframes();
        assert_eq!(converted[0].spatial_in_tangent(), None);
        assert_eq!(converted[1].spatial_out_tangent(), None);
        assert_eq!(converted[2].spatial_in_tangent(), None);
        assert_eq!(converted[3].spatial_out_tangent(), None);
        for index in [0, 2] {
            assert_eq!(
                converted[index].spatial_out_tangent(),
                keys[index]
                    .spatial_out_tangent
                    .map(|t| t[axis] * f64::from(dimensions[axis]))
            );
            assert_eq!(
                converted[index + 1].spatial_in_tangent(),
                keys[index + 1]
                    .spatial_in_tangent
                    .map(|t| t[axis] * f64::from(dimensions[axis]))
            );
        }
        for (native, fx) in keys.iter().zip(converted) {
            assert_eq!(
                fx.layer_time().as_millis(),
                (native.source_ticks - TICKS) / TICKS_PER_MILLISECOND
            );
            assert_eq!(
                fx.value(),
                &fx_schema::PropertyValue::Float(native.value[axis] * f64::from(dimensions[axis]))
            );
            assert_eq!(
                fx.easing(),
                crate::convert::keyframes::fx_easing(native.easing)
            );
        }
    }
}

#[test]
fn straight_position_edited_export_keeps_linear_traversal() {
    let keys = native_straight_position_keys();
    let dimensions = [1920, 1080];
    let (x, y) = super::position_tracks(
        &PrPropertyAnimation::Position(keys.clone()),
        TICKS,
        fx_schema::LayerId::new(1),
        dimensions,
    )
    .unwrap();
    let edited: Vec<_> = [x, y]
        .into_iter()
        .zip([120.0, 60.0])
        .map(|(track, shift)| {
            let keys = track
                .keyframes()
                .iter()
                .enumerate()
                .map(|(index, key)| {
                    let fx_schema::PropertyValue::Float(value) = key.value() else {
                        panic!("expected float axis");
                    };
                    PropertyKeyframe::new(
                        key.id().clone(),
                        key.layer_time(),
                        fx_schema::PropertyValue::Float(
                            value + if index == 1 { shift } else { 0.0 },
                        ),
                        key.easing(),
                    )
                    .with_spatial_tangents(key.spatial_in_tangent(), key.spatial_out_tangent())
                })
                .collect();
            PropertyKeyframeTrack::new(keys).unwrap()
        })
        .collect();
    let native =
        tesseract_to_premiere::export_position_keys(&edited[0], &edited[1], TICKS, dimensions)
            .unwrap();
    assert_eq!(native[0].value, keys[0].value);
    assert_eq!(
        native[1].value,
        [
            keys[1].value[0] + 120.0 / 1920.0,
            keys[1].value[1] + 60.0 / 1080.0
        ]
    );
    assert!(native
        .iter()
        .all(|key| key.spatial_in_tangent.is_none() && key.spatial_out_tangent.is_none()));
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).animations =
        vec![PrPropertyAnimation::Position(native.clone())];
    let mut media = video_media();
    let source = media.values_mut().next().unwrap();
    source.relative_path = Some("./media/source.mp4".into());
    source.absolute_paths = vec![(
        crate::schema::records::MediaPathField::FilePath,
        "/media/source.mp4".into(),
    )];
    source.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Video {
        codec: Some(crate::schema::VideoCodec::H264),
        hdr_profile: None,
    };
    let project = crate::format::PrProjectFile::from_sequences(vec![sequence], media);
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("edited.prproj");
    crate::format::PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&output)
        .unwrap();
    let (readback, omissions) = crate::format::PrProjectFile::load(&output).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let readback = readback.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .animations[0]
        .point_keys()
        .unwrap();
    assert_eq!(readback, native);
    let (x, y) = super::position_tracks(
        &PrPropertyAnimation::Position(readback.to_vec()),
        TICKS,
        fx_schema::LayerId::new(1),
        dimensions,
    )
    .unwrap();
    for (axis, track) in [x, y].iter().enumerate() {
        assert!(!track.has_spatial_tangents());
        let expected = (native[0].value[axis]
            + (native[1].value[axis] - native[0].value[axis]) / 4.0)
            * f64::from(dimensions[axis]);
        let actual =
            sample_linear_fx_point_segment(&track.keyframes()[0], &track.keyframes()[1], 250);
        assert!(
            (actual - expected).abs() < 1e-6,
            "axis {axis}: {actual} != {expected}"
        );
    }
}

#[test]
fn defaults_and_explicit_silent_audio_policy() {
    let doc = project_document(&video_sequence());
    assert_eq!(doc["duration"], 5.0);
    assert_eq!(doc["dimensions"], json!({"width": 1920, "height": 1080}));
    assert_eq!(doc["composition"]["name"], "Main");
    let layer = &doc["composition"]["layers"][0];
    assert_eq!(layer["volume"].as_f64(), Some(0.0));
    assert_eq!(layer["transform"]["opacity"].as_f64(), Some(100.0));
    assert_eq!(layer["sourceRange"]["start"], 0.0);
    assert_eq!(
        (*crate::test_support::layer_range(layer))["duration"],
        5000.0
    );
    let canvas = &doc["composition"]["layers"][1];
    assert_eq!(canvas["type"], "Rect");
    assert_eq!(canvas["rect"]["fillColor"], json!([0.0, 0.0, 0.0, 1.0]));
}

#[test]
fn frame_interpolation_remains_editable_on_import() {
    for (mode, wire) in [
        (fx_schema::FrameBlendingMode::Simple, json!(true)),
        (
            fx_schema::FrameBlendingMode::OpticalFlow,
            json!("opticalFlow"),
        ),
    ] {
        let mut sequence = video_sequence();
        sequence.video_tracks[0].clip_mut(0).frame_blending = Some(mode);
        let document = project_document(&sequence);
        assert_eq!(document["composition"]["layers"][0]["frameBlending"], wire);
    }
}

fn two_sided_dissolve_sequence() -> PrSequence {
    let mut outgoing = clip_of("source", 0..2 * TICKS, 0);
    outgoing.id = Some("outgoing".into());
    outgoing.opacity = 80.0;
    let mut incoming = clip_of("source", 2 * TICKS..5 * TICKS, 3 * TICKS);
    incoming.id = Some("incoming".into());
    incoming.opacity = 65.0;
    let mut sequence = sequence_of("Dissolve", vec![PrVideoTrack::media([outgoing, incoming])]);
    let start = 3 * TICKS / 2 - 1892;
    let end = 5 * TICKS / 2 - 1892;
    sequence.video_tracks[0]
        .transitions
        .push(PrVideoTransition {
            id: "two-sided".into(),
            kind: PrVideoTransitionKind::CrossDissolve,
            start_ticks: start,
            cut_ticks: 2 * TICKS,
            end_ticks: end,
            outgoing_clip: Some("outgoing".into()),
            incoming_clip: Some("incoming".into()),
        });
    sequence
}

#[test]
fn two_sided_cross_dissolve_keeps_handles_and_weighted_picture_opacity() {
    let sequence = two_sided_dissolve_sequence();
    let start = sequence.video_tracks[0].transitions[0].start_ticks;
    let end = sequence.video_tracks[0].transitions[0].end_ticks;
    let media = video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions).unwrap();
    assert!(
        !omissions.iter().any(|item| {
            item.record == "two-sided" && item.kind == crate::OmissionKind::Omitted
        }),
        "{omissions:?}"
    );
    let value = document.to_json_value().unwrap();
    let composite = value["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(composite["blendMode"], "normal");
    assert!(!composite["masks"].as_array().unwrap().is_empty());
    let pictures: Vec<_> = composite["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .collect();
    assert_eq!(pictures.len(), 2);
    let progress_at_cut = (2 * TICKS - start) as f64 / (end - start) as f64;
    for (index, picture) in pictures.into_iter().enumerate() {
        assert_eq!(picture["blendMode"], "add");
        let video = picture["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        let (window, source, opacity, weights) = if index == 0 {
            (
                (0, 2500),
                (0, 2500),
                80.0,
                [100.0, 100.0 * (1.0 - progress_at_cut), 0.0],
            )
        } else {
            (
                (1500, 3500),
                (2500, 3500),
                65.0,
                [0.0, 100.0 * progress_at_cut, 100.0],
            )
        };
        assert_eq!(
            crate::test_support::layer_range(video),
            &json!({"start": window.0, "duration": window.1})
        );
        assert_eq!(
            video["sourceRange"],
            json!({"start": source.0, "duration": source.1})
        );
        assert_eq!(video["transform"]["opacity"].as_f64(), Some(opacity));
        let animation = value["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == picture["id"]
                    && entry["target"]["propertyType"] == "opacity"
            })
            .unwrap();
        let keys = animation["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 3);
        for ((key, time), weight) in keys.iter().zip([1500, 2000, 2500]).zip(weights) {
            assert_eq!(key["layerTime"], time);
            assert!((key["value"]["value"].as_f64().unwrap() - weight).abs() < 1e-10);
        }
    }
}

#[test]
fn two_sided_cross_dissolve_exports_current_ramps_and_native_links() {
    let sequence = two_sided_dissolve_sequence();
    let media = video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut Vec::new()).unwrap();
    let facts = BTreeMap::from([(
        ids.values().next().unwrap().as_str().to_owned(),
        MediaFacts::Video(VideoMedia {
            pixel_aspect: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            codec: crate::schema::VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: 1920,
            height: 1080,
            timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
        }),
    )]);
    for (edited, without_canvas) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut value = document.to_json_value().unwrap();
        if without_canvas {
            let layers = value["composition"]["layers"].as_array_mut().unwrap();
            assert_eq!(
                layers
                    .iter()
                    .filter(|layer| layer["type"] == "Rect")
                    .count(),
                1
            );
            layers.retain(|layer| layer["type"] != "Rect");
            value["duration"] = json!(6);
        }
        if edited {
            for entry in value["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
            {
                if entry["target"]["propertyType"] == "opacity" {
                    let keys = entry["animator"]["keyframes"].as_array_mut().unwrap();
                    keys[0]["layerTime"] = json!(1600);
                    keys[1]["value"]["value"] = json!(50.0);
                    keys[2]["layerTime"] = json!(2400);
                }
            }
        }
        let mut omissions = Vec::new();
        let edited_document =
            fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let lowered = super::super::tesseract_to_premiere::lower_document(
            &edited_document,
            &facts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        assert!(lowered.packing.is_complete());
        let mut exported = lowered.project.unwrap();
        if without_canvas {
            // Keep the existing last-occurrence duration diagnostic, not a gap rejection.
            assert_eq!(
                omissions,
                [crate::Omission {
                    scope: crate::OmissionScope::Feature,
                    kind: crate::OmissionKind::Omitted,
                    record: "document.duration".into(),
                    reason: format!(
                        "duration differs from the last occurrence; exported duration is {} ticks",
                        5 * TICKS
                    ),
                }]
            );
        } else {
            assert!(
                !omissions
                    .iter()
                    .any(|o| o.kind == crate::OmissionKind::Omitted),
                "{omissions:?}"
            );
        }
        let native = exported.single_sequence().unwrap();
        assert_eq!(
            native.end_ticks(),
            if without_canvas { 6 * TICKS } else { 5 * TICKS }
        );
        assert_eq!(
            native.gaps(&exported.media),
            if without_canvas {
                Vec::from_iter(Some(5 * TICKS..6 * TICKS))
            } else {
                Vec::new()
            }
        );
        assert_eq!(
            native
                .video_occurrences()
                .map(|clip| (
                    clip.start_ticks,
                    clip.end_ticks,
                    clip.in_ticks,
                    clip.out_ticks,
                    clip.opacity,
                ))
                .collect::<Vec<_>>(),
            [
                (0, 2 * TICKS, 0, 2 * TICKS, 80.0),
                (2 * TICKS, 5 * TICKS, 3 * TICKS, 6 * TICKS, 65.0)
            ]
        );
        let transition = native
            .video_tracks
            .iter()
            .flat_map(|track| &track.transitions)
            .next()
            .unwrap();
        let expected = if edited {
            (8 * TICKS / 5, 12 * TICKS / 5)
        } else {
            (3 * TICKS / 2 - 1892, 5 * TICKS / 2 - 1892)
        };
        assert_eq!(
            (
                transition.start_ticks,
                transition.cut_ticks,
                transition.end_ticks
            ),
            (expected.0, 2 * TICKS, expected.1)
        );
        for (index, source) in exported.media.values_mut().enumerate() {
            source.name = format!("source-{index}.mp4");
            source.relative_path = Some(format!("./media/{}", source.name));
            source.absolute_paths = vec![(
                crate::schema::records::MediaPathField::FilePath,
                format!("/media/{}", source.name).into(),
            )];
        }
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("dissolve.prproj");
        crate::format::PremiereProjectXml::new(&exported)
            .unwrap()
            .write_new(&output)
            .unwrap();
        let (readback, omissions) = crate::format::PrProjectFile::load(&output).unwrap();
        assert!(
            !omissions
                .iter()
                .any(|o| o.kind == crate::OmissionKind::Omitted),
            "{omissions:?}"
        );
        let readback = readback.single_sequence().unwrap();
        let track = readback
            .video_tracks
            .iter()
            .find(|track| !track.transitions.is_empty())
            .unwrap();
        assert_eq!(
            track.transitions[0].kind,
            PrVideoTransitionKind::CrossDissolve
        );
        assert_eq!(
            (
                track.transitions[0].start_ticks,
                track.transitions[0].cut_ticks,
                track.transitions[0].end_ticks
            ),
            (expected.0, 2 * TICKS, expected.1)
        );
        assert_eq!(
            (track.clip(0).end_ticks, track.clip(1).start_ticks),
            (2 * TICKS, 2 * TICKS)
        );
        assert_eq!((track.clip(0).opacity, track.clip(1).opacity), (80.0, 65.0));
    }
}

#[test]
fn one_sided_cross_dissolve_exports_edited_head_and_tail_opacity() {
    for tail in [false, true] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.id = Some("picture".into());
        clip.in_ticks = TICKS;
        clip.out_ticks = 6 * TICKS;
        clip.opacity = 80.0;
        sequence.video_tracks[0]
            .transitions
            .push(PrVideoTransition {
                id: "edge".into(),
                kind: PrVideoTransitionKind::CrossDissolve,
                start_ticks: if tail { 4 * TICKS } else { 0 },
                cut_ticks: if tail { 5 * TICKS } else { 0 },
                end_ticks: if tail { 5 * TICKS } else { TICKS },
                outgoing_clip: tail.then(|| "picture".into()),
                incoming_clip: (!tail).then(|| "picture".into()),
            });
        let media = video_media();
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions).unwrap();
        assert!(
            !omissions
                .iter()
                .any(|o| o.kind == crate::OmissionKind::Omitted),
            "{omissions:?}"
        );
        let mut value = document.to_json_value().unwrap();
        let entry = value["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["target"]["propertyType"] == "opacity")
            .unwrap();
        entry["animator"]["keyframes"][usize::from(!tail)]["value"]["value"] = json!(65.0);
        let facts = BTreeMap::from([(
            ids.values().next().unwrap().as_str().to_owned(),
            MediaFacts::Video(VideoMedia {
                pixel_aspect: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                codec: crate::schema::VideoCodec::H264,
                bit_depth: 8,
                colour: None,
                width: 1920,
                height: 1080,
                timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
            }),
        )]);
        omissions.clear();
        let edited_document =
            fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let exported = tesseract_to_premiere(
            &edited_document,
            &facts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        assert!(
            !omissions
                .iter()
                .any(|o| o.kind == crate::OmissionKind::Omitted),
            "{omissions:?}"
        );
        let track = exported
            .single_sequence()
            .unwrap()
            .video_tracks
            .iter()
            .find(|track| !track.transitions.is_empty())
            .unwrap();
        assert_eq!(track.clip(0).opacity, 65.0);
        assert_eq!(track.clip(0).in_ticks, TICKS);
        assert!(track.clip(0).animations.is_empty());
        let actual = &track.transitions[0];
        let expected = &sequence.video_tracks[0].transitions[0];
        assert_eq!(
            (actual.start_ticks, actual.cut_ticks, actual.end_ticks),
            (expected.start_ticks, expected.cut_ticks, expected.end_ticks)
        );
        assert_eq!(actual.outgoing_clip.is_some(), tail);
        assert_eq!(actual.incoming_clip.is_some(), !tail);
    }
}

#[test]
fn cross_dissolve_noncanonical_edits_do_not_restore_pictures_or_abort_sibling() {
    fn counts(
        sequence: &PrSequence,
        media: &BTreeMap<MediaId, PrMedia>,
        source: &MediaId,
    ) -> (usize, usize) {
        let mut total = (
            sequence
                .video_occurrences()
                .filter(|clip| {
                    &clip.media == source
                        || media
                            .get(&clip.media)
                            .and_then(|media| media.video.as_ref())
                            .is_some_and(|video| {
                                matches!(&video.kind,
                                crate::schema::PrMediaKind::ColorMatte(matte)
                                    if matte.fill_color() == [1.0, 0.0, 0.0, 1.0]
                                        || matte.fill_color() == [0.0, 1.0, 0.0, 1.0])
                            })
                })
                .count(),
            sequence
                .video_tracks
                .iter()
                .map(|track| track.transitions.len())
                .sum(),
        );
        for nest in sequence.nest_occurrences() {
            let child = counts(&nest.sequence, media, source);
            total.0 += child.0;
            total.1 += child.1;
        }
        total
    }
    let sequence = two_sided_dissolve_sequence();
    let media = video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut Vec::new()).unwrap();
    let asset = ids.values().next().unwrap().as_str().to_owned();
    let facts = BTreeMap::from([(
        asset.clone(),
        MediaFacts::Video(VideoMedia {
            pixel_aspect: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            codec: crate::schema::VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: 1920,
            height: 1080,
            timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
        }),
    )]);
    for edit in [
        "trim-out",
        "trim-in",
        "screen",
        "matte-normal",
        "matte-screen",
        "matte-binding",
        "speed",
    ] {
        let mut value = document.to_json_value().unwrap();
        let layers = value["composition"]["layers"].as_array_mut().unwrap();
        let matte_template = layers
            .iter()
            .find(|layer| layer["type"] == "Rect")
            .unwrap()
            .clone();
        let composite = layers
            .iter_mut()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        let mut sibling = composite["layers"][0]["layers"][0].clone();
        sibling["id"] = json!(5000);
        sibling.as_object_mut().unwrap().remove("parent");
        if matches!(edit, "matte-normal" | "matte-screen" | "matte-binding") {
            for (side, color) in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]]
                .into_iter()
                .enumerate()
            {
                let picture = &mut composite["layers"][side]["layers"][0];
                let mut matte = matte_template.clone();
                matte["id"] = picture["id"].clone();
                assert!(picture["parent"].is_number());
                matte["parent"] = picture["parent"].clone();
                matte["activeRange"] = picture["playback"]["inputRange"].clone();
                matte["transform"]["opacity"] = json!(100.0);
                matte["rect"]["fillColor"] = json!(color);
                *picture = matte;
            }
        }
        let side = usize::from(edit == "trim-in");
        let picture = &mut composite["layers"][side]["layers"][0];
        if edit == "screen" || edit == "matte-screen" {
            picture["blendMode"] = json!("screen");
        } else if edit == "matte-binding" {
            // A flat dissolve picture cannot borrow a matte outside its fade group.
            picture["trackMatte"] = json!({"mode": "alpha", "layer": matte_template["id"]});
        } else if edit != "matte-normal" {
            let (start, duration, source_start, source_duration) = match edit {
                "trim-out" => (0, 2300, 0, 2300),
                "trim-in" => (1700, 3300, 2700, 3300),
                "speed" => (0, 2500, 0, 5000),
                _ => unreachable!(),
            };
            let window = json!({"start": start, "duration": duration});
            let source = json!({"start": source_start, "duration": source_duration});
            picture["playback"] = crate::test_support::linear_playback(window, source.clone());
            picture["sourceRange"] = source;
        }
        // The Normal/Screen matte controls differ only in the outgoing blend.
        // Screen must not escape isolation and start sampling this blue backing.
        if edit == "screen" || edit == "matte-normal" || edit == "matte-screen" {
            let backing = layers
                .iter_mut()
                .find(|layer| layer["type"] == "Rect")
                .unwrap();
            backing["rect"]["fillColor"] = json!([0.0, 0.0, 1.0, 1.0]);
        }
        layers.push(sibling);
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
        let mut omissions = Vec::new();
        let exported = tesseract_to_premiere(
            &document,
            &facts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        let sequence = exported.single_sequence().unwrap();
        let (pictures, transitions) = counts(sequence, &exported.media, &MediaId(asset.clone()));
        if edit == "matte-normal" {
            assert_eq!((pictures, transitions), (3, 1), "{edit}: {omissions:?}");
            assert!(
                !omissions
                    .iter()
                    .any(|o| o.kind == crate::OmissionKind::Omitted),
                "{omissions:?}"
            );
        } else {
            assert_eq!(transitions, 0, "{edit}: {omissions:?}");
        }
        let sibling = sequence
            .video_occurrences()
            .find(|clip| clip.media.as_str() == asset)
            .unwrap();
        assert_eq!(
            (
                sibling.start_ticks,
                sibling.end_ticks,
                sibling.in_ticks,
                sibling.out_ticks,
                sibling.opacity
            ),
            (0, 5 * TICKS / 2, 0, 5 * TICKS / 2, 80.0),
            "{edit}"
        );
        // Generic nest export can reject a trailing transparent window. That
        // must remain a scoped occurrence loss, not silent handle restoration
        // or failure to publish the independent picture above.
        if pictures < 3 {
            assert!(
                omissions
                    .iter()
                    .any(|o| o.scope == crate::OmissionScope::Occurrence
                        && o.kind == crate::OmissionKind::Omitted),
                "{edit}: {omissions:?}"
            );
        }
    }
}

#[test]
fn cross_dissolve_does_not_overwrite_an_existing_film_impact_opacity_owner() {
    for reverse in [false, true] {
        let mut sequence = video_sequence();
        sequence.video_tracks[0].clip_mut(0).id = Some("picture".into());
        sequence.video_tracks[0].transitions = vec![
            PrVideoTransition {
                id: "cross".into(),
                kind: PrVideoTransitionKind::CrossDissolve,
                start_ticks: 0,
                cut_ticks: 0,
                end_ticks: TICKS,
                outgoing_clip: None,
                incoming_clip: Some("picture".into()),
            },
            PrVideoTransition {
                id: "impact".into(),
                kind: PrVideoTransitionKind::FilmImpactDissolve,
                start_ticks: 4 * TICKS,
                cut_ticks: 5 * TICKS,
                end_ticks: 5 * TICKS,
                outgoing_clip: Some("picture".into()),
                incoming_clip: None,
            },
        ];
        if reverse {
            sequence.video_tracks[0].transitions.reverse();
        }
        let media = video_media();
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions).unwrap();
        assert!(
            omissions
                .iter()
                .any(|o| o.record == "cross" && o.kind == crate::OmissionKind::Omitted),
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|o| o.record == "impact" && o.kind == crate::OmissionKind::Approximated),
            "{omissions:?}"
        );
        let value = document.to_json_value().unwrap();
        let entry = &value["composition"]["dynamics"]["entries"][0];
        assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 4000);
        assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 5000);
        assert_eq!(
            entry["animator"]["keyframes"][1]["easing"]["type"],
            "cubicBezier"
        );
    }
}

#[test]
fn detected_cross_dissolve_is_reported_without_faking_an_editable_graph() {
    let mut sequence = video_sequence();
    sequence.video_tracks[0]
        .transitions
        .push(PrVideoTransition {
            id: "VideoTransitionTrackItem:ObjectID:16".into(),
            kind: PrVideoTransitionKind::CrossDissolve,
            start_ticks: 2 * TICKS,
            cut_ticks: 5 * TICKS / 2,
            end_ticks: 3 * TICKS,
            outgoing_clip: Some("3".into()),
            incoming_clip: Some("9".into()),
        });
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions).unwrap();

    assert_eq!(
        document.to_json_value().unwrap()["composition"]["layers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        omissions.iter().any(|item| {
            item.record == "VideoTransitionTrackItem:ObjectID:16"
                && item.reason.contains("Cross Dissolve detected")
                && item.reason.contains("not converted")
        }),
        "{omissions:?}"
    );
}

#[test]
fn multiple_linear_wipes_receive_globally_unique_guide_and_mask_ids() {
    fn wipe(source_in: i64) -> PrLinearWipe {
        PrLinearWipe {
            initial_completion: 100.0,
            completion: vec![
                PrScalarKeyframe {
                    source_ticks: source_in,
                    value: 100.0,
                    easing: PrKeyframeEasing::Linear,
                },
                PrScalarKeyframe {
                    source_ticks: source_in + TICKS,
                    value: 0.0,
                    easing: PrKeyframeEasing::Linear,
                },
            ],
            angle_degrees: 270,
            feather: 5.0,
        }
    }

    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).linear_wipe = Some(wipe(0));
    let mut second = sequence.video_tracks[0].clip(0).clone();
    second.start_ticks = 5 * TICKS;
    second.end_ticks = 10 * TICKS;
    second.in_ticks = 5 * TICKS;
    second.out_ticks = 10 * TICKS;
    second.linear_wipe = Some(wipe(5 * TICKS));
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(second));

    let document = project_document(&sequence);
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 5);
    let mut item_ids = std::collections::BTreeSet::new();
    for layer in layers {
        assert!(item_ids.insert(layer["id"].as_u64().unwrap()));
        for mask in layer["masks"].as_array().into_iter().flatten() {
            assert!(item_ids.insert(mask["id"].as_u64().unwrap()));
        }
    }
}

#[test]
fn combined_crop_and_linear_wipe_are_explicitly_omitted_before_mask_mutations() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.id = Some("combined-effects".into());
    clip.crop = PrStaticCrop {
        left: 7.0,
        ..PrStaticCrop::default()
    };
    clip.linear_wipe = Some(PrLinearWipe {
        initial_completion: 100.0,
        completion: vec![
            PrScalarKeyframe {
                source_ticks: 0,
                value: 100.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: TICKS,
                value: 0.0,
                easing: PrKeyframeEasing::Linear,
            },
        ],
        angle_degrees: 270,
        feather: 5.0,
    });
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
    let mut omissions = Vec::new();

    let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions).unwrap();
    let wire = document.to_json_value().unwrap();
    let layers = wire["composition"]["layers"].as_array().unwrap();

    // Dropping both masks would show what they hide: the whole clip is omitted.
    assert_eq!(layers.len(), 1, "only the black canvas remains");
    assert_eq!(layers[0]["name"], "Premiere black canvas");
    assert_eq!(
        omissions,
        [crate::Omission {
            scope: crate::OmissionScope::Occurrence,
            kind: crate::OmissionKind::Omitted,
            record: "combined-effects".into(),
            reason: format!(
                "track 0, range 0..{} ticks: Crop and Linear Wipe on one clip are not converted; occurrence omitted",
                5 * TICKS
            ),
        }]
    );
}

#[test]
fn static_motion_imports_as_editable_source_and_canvas_coordinates() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.transform.position = [0.75, 0.25];
    clip.transform.anchor_point = [0.125, 0.75];
    clip.transform.scale = [80.0, 125.0];
    clip.transform.rotation = -15.0;

    let document = project_document(&sequence);
    let transform = &document["composition"]["layers"][0]["transform"];
    assert_eq!(transform["position"], json!([1440.0, 270.0]));
    assert_eq!(transform["anchorPoint"], json!([240.0, 810.0]));
    assert_eq!(transform["scale"], json!([80.0, 125.0]));
    assert_eq!(transform["rotation"], json!(-15.0));
}

#[test]
fn static_crop_uses_source_dimensions_and_follows_motion_across_aspect_ratios() {
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).crop = PrStaticCrop {
        // Exact standalone Crop value observed in the pinned Adobe-authored
        // `corporate_slideshow` project; this is not an inferred UI setting.
        left: 7.0,
        ..PrStaticCrop::default()
    };
    let mut media = video_media();
    let source = media.values_mut().next().unwrap().video.as_mut().unwrap();
    // Exact natural dimensions observed for the portrait CTA clip in the pinned
    // Adobe-authored `2128_aiedit_en_na_multicreators` project.
    source.width = 1080;
    source.height = 1920;

    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut Vec::new()).unwrap();
    let wire = document.to_json_value().unwrap();
    let video = &wire["composition"]["layers"][0];
    let guide = &wire["composition"]["layers"][1];

    assert_eq!(
        video["source"]["sourceRect"],
        json!({"x": 0.0, "y": 0.0, "width": 1080.0, "height": 1920.0})
    );
    assert_eq!(guide["rect"]["fillEnabled"], false);
    assert_eq!(guide["rect"]["strokeEnabled"], false);
    assert_eq!(guide["rect"]["position"], json!([75.6, 0.0]));
    assert_eq!(guide["rect"]["size"], json!([1004.4, 1920.0]));
    assert_eq!(guide["transform"], video["transform"]);
}

#[test]
fn crop_guide_repeats_the_keyed_motion_of_its_video() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.crop = PrStaticCrop {
        left: 7.0,
        ..PrStaticCrop::default()
    };
    let key = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    let point = |source_ticks, value| PrPointKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    clip.animations = vec![
        PrPropertyAnimation::Position(vec![point(0, [0.5, 0.5]), point(TICKS, [0.25, 0.5])]),
        PrPropertyAnimation::Rotation(vec![key(0, 0.0), key(TICKS, 20.0)]),
        PrPropertyAnimation::UniformScale(vec![key(0, 100.0), key(TICKS, 80.0)]),
        PrPropertyAnimation::Opacity(vec![key(0, 100.0), key(TICKS, 50.0)]),
    ];
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let wire = document.to_json_value().unwrap();
    let layers = wire["composition"]["layers"].as_array().unwrap();
    // One flat layer: no effect applies before the Crop.
    assert_eq!(
        layers
            .iter()
            .map(|layer| &layer["name"])
            .collect::<Vec<_>>(),
        [
            "Premiere video 1",
            "Premiere Crop guide 1",
            "Premiere black canvas"
        ]
    );
    let (video, guide) = (&layers[0], &layers[1]);
    assert_eq!(video["masks"][0]["layer"], guide["id"]);
    assert_eq!(guide["transform"], video["transform"]);
    // The guide repeats every Motion track of the video, not its Opacity.
    let tracks = |layer: &Value| {
        wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["target"]["layerId"] == layer["id"])
            .map(|entry| {
                let keys: Vec<_> = entry["animator"]["keyframes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|key| (key["layerTime"].clone(), key["value"].clone()))
                    .collect();
                (entry["target"]["propertyType"].clone(), keys)
            })
            .collect::<Vec<_>>()
    };
    let video_tracks = tracks(video);
    assert_eq!(video_tracks.len(), 6);
    assert_eq!(
        tracks(guide),
        video_tracks
            .into_iter()
            .filter(|(property, _)| property != "opacity")
            .collect::<Vec<_>>()
    );
}

/// An enabled static Gaussian Blur.
fn blur(blurriness: f64) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels: false,
        }),
        animations: Vec::new(),
    }
}

/// The imported document of `sequence` whose one source has `frame`, and its
/// omissions.
fn import_with_frame(sequence: &PrSequence, frame: [u32; 2]) -> (Value, Vec<crate::Omission>) {
    let mut media = video_media();
    let source = media.values_mut().next().unwrap().video.as_mut().unwrap();
    [source.width, source.height] = frame;
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(sequence, &media, &ids, &mut omissions).unwrap();
    (document.to_json_value().unwrap(), omissions)
}

#[test]
fn crop_below_an_effect_stages_the_clip_with_its_flat_geometry() {
    // A blur that applies before a Crop on a moved clip, on landscape and on
    // portrait media; the portrait clip is disabled.
    for frame in [[1920, 1080], [1080, 1920]] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.crop = PrStaticCrop {
            left: 20.0,
            top: 15.0,
            bottom: 10.0,
            ..PrStaticCrop::default()
        };
        clip.transform.scale = [80.0; 2];
        clip.transform.rotation = 15.0;
        clip.transform.position = [0.45, 0.55];
        clip.opacity = 50.0;
        clip.enabled = frame[0] > frame[1];
        clip.effects = vec![blur(40.0)];
        // With the Crop applied before the blur the clip is flat; after it, it
        // stages.
        let (flat, _) = import_with_frame(&sequence, frame);
        sequence.video_tracks[0].clip_mut(0).effects_above_mask = 1;
        let (staged, omissions) = import_with_frame(&sequence, frame);
        assert!(omissions.is_empty(), "{omissions:?}");
        let [flat_video, flat_guide, flat_canvas] =
            flat["composition"]["layers"].as_array().unwrap().as_slice()
        else {
            panic!("expected a flat video, its guide and the canvas");
        };
        let [group, canvas] = staged["composition"]["layers"]
            .as_array()
            .unwrap()
            .as_slice()
        else {
            panic!("expected one stage group and the canvas");
        };
        // Guide 2 and mask 3 as when flat, then the group; later ids shift by one.
        assert_eq!(group["type"], "Group");
        assert_eq!(group["name"], "Premiere stage 1");
        assert_eq!([&flat_canvas["id"], &group["id"], &canvas["id"]], [4, 4, 5]);
        // The group takes the clip's range, visibility, Motion, Opacity and mask.
        for field in ["activeRange", "isHidden", "transform", "masks"] {
            assert_eq!(group[field], flat_video[field], "{field}");
        }
        let [video, guide] = group["layers"].as_array().unwrap().as_slice() else {
            panic!("expected the video and its Crop guide");
        };
        assert_eq!(
            [&video["id"], &guide["id"]],
            [&flat_video["id"], &flat_guide["id"]]
        );
        // Under it, both are in the source frame on the group clock.
        for layer in [video, guide] {
            assert_eq!(layer["parent"], group["id"]);
            assert_eq!(
                (*crate::test_support::layer_range(layer)),
                json!({"start": 0, "duration": 5000})
            );
            assert_eq!(layer["transform"], canvas["transform"], "identity");
        }
        assert!(video.get("isHidden").is_none() && video.get("masks").is_none());
        for field in ["sourceRange", "source", "effects"] {
            assert_eq!(video[field], flat_video[field], "{field}");
        }
        assert_eq!(guide["rect"], flat_guide["rect"]);
    }
}

/// The keys of each animated layer property, by layer id and property:
/// `(layer time, value, easing)` per key.
type LayerPropertyTracks = BTreeMap<(u64, String), Vec<(u64, f64, String)>>;

fn layer_property_tracks(wire: &Value) -> LayerPropertyTracks {
    wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["kind"] == "layer")
        .map(|entry| {
            let keys = entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| {
                    (
                        key["layerTime"].as_u64().unwrap(),
                        key["value"]["value"].as_f64().unwrap(),
                        key["easing"]["type"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            (
                (
                    entry["target"]["layerId"].as_u64().unwrap(),
                    entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                ),
                keys,
            )
        })
        .collect()
}

/// Clip D's Transform keys of the run E11 fixture: Scale Height Linear 100 at
/// source 1 s to 200 at 1.5 s, Hold to 50 at 2.5 s; Rotation Linear 0 to 90.
fn transform_d_animations() -> Vec<PrEffectParamAnimation> {
    let key = |source_ticks, value, easing| PrScalarKeyframe {
        source_ticks,
        value,
        easing,
    };
    use PrKeyframeEasing::{Hold, Linear};
    vec![
        PrEffectParamAnimation {
            param: &TRANSFORM_SCALE_HEIGHT,
            keys: PrEffectParamKeys::Scalar(vec![
                key(TICKS, 100.0, Linear),
                key(3 * TICKS / 2, 200.0, Linear),
                key(5 * TICKS / 2, 50.0, Hold),
            ]),
        },
        PrEffectParamAnimation {
            param: &TRANSFORM_ROTATION,
            keys: PrEffectParamKeys::Scalar(vec![
                key(TICKS, 0.0, Linear),
                key(5 * TICKS / 2, 90.0, Linear),
            ]),
        },
    ]
}

#[test]
fn a_transform_stages_the_clip_as_its_videos_transform_and_tracks() {
    // Clip D's keyed Transform with Position keys added, under clip E's
    // Motion (Scale 50, Rotation 30), with a blur applied before it and a
    // blur applied after it; the clip starts at 6 s from source In 0.5 s. No
    // skew: the reader rejects one with a keyed Rotation, and clip
    // C's axis negation is the fixture import test's.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.id = Some("transform".into());
    (clip.start_ticks, clip.end_ticks) = (6 * TICKS, 17 * TICKS / 2);
    (clip.in_ticks, clip.out_ticks) = (TICKS / 2, 3 * TICKS);
    clip.transform.scale = [50.0; 2];
    clip.transform.rotation = 30.0;
    let mut animations = transform_d_animations();
    animations.insert(
        0,
        PrEffectParamAnimation {
            param: &TRANSFORM_POSITION,
            keys: PrEffectParamKeys::Point(vec![
                PrPointKeyframe {
                    source_ticks: TICKS / 2,
                    value: [0.25, 0.5],
                    easing: PrKeyframeEasing::Linear,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
                PrPointKeyframe {
                    source_ticks: 3 * TICKS / 2,
                    value: [0.75, 0.5],
                    easing: PrKeyframeEasing::Linear,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
            ]),
        },
    );
    clip.effects = vec![
        blur(25.0),
        transform_effect(
            PrTransform {
                anchor_point: [0.75, 0.5],
                position: [0.25, 0.5],
                uniform_scale: true,
                ..DEFAULT_PR_TRANSFORM
            },
            animations,
        ),
        blur(60.0),
    ];
    let (wire, omissions) = import_with_frame(&sequence, [1920, 1080]);
    assert_eq!(
        omissions,
        [crate::Omission {
            scope: crate::OmissionScope::Feature,
            kind: crate::OmissionKind::Omitted,
            record: "transform".into(),
            reason: "Gaussian Blur effect at stack position 3 was not imported: it applies after the Transform, which the stage group's video carries as its transform, and that stage carries no effects".into(),
        }]
    );
    let [group, canvas] = wire["composition"]["layers"].as_array().unwrap().as_slice() else {
        panic!("expected one stage group and the canvas");
    };
    // The group takes the clip's place and Motion, without a mask; the
    // video, on the group clock, carries the Transform: both points in
    // source pixels, Scale Height on both axes, the first keys' values as
    // statics.
    assert_eq!(
        (
            &group["type"],
            &group["name"],
            crate::test_support::layer_range(group)
        ),
        (
            &json!("Group"),
            &json!("Premiere stage 1"),
            &json!({"start": 6000, "duration": 2500})
        )
    );
    assert!(group.get("masks").is_none(), "{group}");
    assert_eq!(group["transform"]["scale"], json!([50.0, 50.0]));
    assert_eq!(group["transform"]["rotation"], json!(30.0));
    let [video] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the video alone");
    };
    assert_eq!(video["parent"], group["id"]);
    assert_eq!(
        (*crate::test_support::layer_range(video)),
        json!({"start": 0, "duration": 2500})
    );
    assert_eq!(
        video["transform"],
        json!({
            "anchorPoint": [1440.0, 540.0],
            "position": [480.0, 540.0],
            "scale": [100.0, 100.0],
            "rotation": 0.0,
            "skew": 0.0,
            "skewAxis": 0.0,
            "rotationX": 0.0,
            "rotationY": 0.0,
            "orientation": [0.0, 0.0, 0.0],
            "opacity": 100.0,
        })
    );
    assert_eq!(video["effects"][0]["effect"]["blurriness"], json!(25.0));
    assert_eq!(video["effects"].as_array().unwrap().len(), 1);
    assert_eq!(canvas["name"], "Premiere black canvas");
    // The Transform's keys are the video's tracks on the group clock from
    // the source In (500 ms before the first key), in source pixels.
    let video_id = video["id"].as_u64().unwrap();
    let scale = vec![
        (500, 100.0, "linear".to_owned()),
        (1000, 200.0, "linear".to_owned()),
        (2000, 50.0, "hold".to_owned()),
    ];
    assert_eq!(
        layer_property_tracks(&wire),
        BTreeMap::from([
            (
                (video_id, "positionX".to_owned()),
                vec![
                    (0, 480.0, "linear".to_owned()),
                    (1000, 1440.0, "linear".to_owned())
                ]
            ),
            (
                (video_id, "positionY".to_owned()),
                vec![
                    (0, 540.0, "linear".to_owned()),
                    (1000, 540.0, "linear".to_owned())
                ]
            ),
            (
                (video_id, "rotation".to_owned()),
                vec![
                    (500, 0.0, "linear".to_owned()),
                    (2000, 90.0, "linear".to_owned())
                ]
            ),
            ((video_id, "scaleX".to_owned()), scale.clone()),
            ((video_id, "scaleY".to_owned()), scale),
        ])
    );
}

#[test]
fn transforms_that_stage_nothing_keep_the_clip_flat_with_a_reason() {
    let key = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    let width_keys = || {
        vec![PrEffectParamAnimation {
            param: &TRANSFORM_SCALE_WIDTH,
            keys: PrEffectParamKeys::Scalar(vec![key(0, 100.0), key(TICKS, 70.0)]),
        }]
    };
    let transform = || transform_effect(DEFAULT_PR_TRANSFORM, Vec::new());
    type Edit<'a> = Box<dyn Fn(&mut crate::schema::PrVideoOccurrence) + 'a>;
    /// (case, edit, source frame, the omissions, whether the clip stages)
    type Case<'a> = (&'a str, Edit<'a>, [u32; 2], Vec<&'a str>, bool);
    let cases: [Case<'_>; 8] = [
        // The reader counted a second active Transform that it did not
        // convert (`transforms_beside_a_rejected_one_stage_nothing` reads both).
        (
            "a Transform beside a rejected one",
            Box::new(|clip| {
                clip.effects = vec![transform()];
                clip.active_transforms = 2;
            }),
            [1920, 1080],
            vec![
                "Transform effect at stack position 1 was not imported: another active Transform on the same clip is not converted with this one: native measurements cover one Transform per clip, and Premiere's composition of two is unmeasured",
            ],
            false,
        ),
        (
            "a Transform with a Crop",
            Box::new(|clip| {
                clip.effects = vec![transform()];
                clip.crop = left_crop();
            }),
            [1920, 1080],
            vec![
                "Transform effect at stack position 1 was not imported: a Transform with a Crop, Linear Wipe or Opacity mask on one clip is not converted: the mask keeps its stage group, which carries one shape",
            ],
            false,
        ),
        // The reader counts the Transform as applied before the Opacity mask,
        // which applies after every effect: the mask stages the clip.
        (
            "a Transform with an Opacity mask",
            Box::new(|clip| {
                let mut mask = opacity_mask();
                mask.feather = 0.0;
                clip.effects = vec![transform()];
                clip.opacity_mask = Some(mask);
                clip.effects_above_mask = 1;
            }),
            [1920, 1080],
            vec![
                "Transform effect at stack position 1 was not imported: a Transform with a Crop, Linear Wipe or Opacity mask on one clip is not converted: the mask keeps its stage group, which carries one shape",
            ],
            true,
        ),
        (
            "a Transform on portrait media",
            Box::new(|clip| clip.effects = vec![transform()]),
            [1080, 1920],
            vec![
                "Transform effect at stack position 1 was not imported: a Transform on media that is not sequence-sized is not converted: the frame that Premiere normalizes its Position to is unmeasured there (native measurements cover 1920 x 1080 media on a 1920 x 1080 sequence)",
            ],
            false,
        ),
        // Non-uniform: Scale Width keys move the video's x scale only.
        (
            "Scale Width keys without Uniform Scale",
            Box::new(|clip| {
                clip.effects = vec![transform_effect(DEFAULT_PR_TRANSFORM, width_keys())];
            }),
            [1920, 1080],
            vec![],
            true,
        ),
        // A retimed clip's keys are not converted, like Motion keys.
        (
            "a keyed Transform on a retimed clip",
            Box::new(|clip| {
                clip.effects = vec![transform_effect(DEFAULT_PR_TRANSFORM, width_keys())];
                clip.playback_rate = 2.0;
                clip.out_ticks = 10 * TICKS;
            }),
            [1920, 1080],
            vec![
                "Transform animation was not imported: keys on a retimed, reversed or time-remapped clip are not converted; static values were kept",
            ],
            true,
        ),
        // A disabled clip's stage group is hidden, as a mask stage is.
        (
            "a Transform on a disabled clip",
            Box::new(|clip| {
                clip.effects = vec![transform()];
                clip.enabled = false;
            }),
            [1920, 1080],
            vec![],
            true,
        ),
        // Skew 0 shears nothing at any axis: the video keeps skew axis 0 and
        // the native Skew Axis is reported.
        (
            "a Skew Axis without Skew",
            Box::new(|clip| {
                clip.effects = vec![transform_effect(
                    PrTransform {
                        skew_axis: 30.0,
                        ..DEFAULT_PR_TRANSFORM
                    },
                    Vec::new(),
                )];
            }),
            [1920, 1080],
            vec!["Skew Axis 30 without Skew is not retained (no render effect)"],
            true,
        ),
    ];
    for (case, edit, frame, expected, staged) in cases {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.id = Some("transform".into());
        edit(clip);
        let (wire, omissions) = import_with_frame(&sequence, frame);
        let reasons: Vec<_> = omissions
            .iter()
            .map(|omission| {
                assert_eq!(omission.record, "transform", "{case}");
                omission.reason.as_str()
            })
            .collect();
        assert_eq!(reasons, expected, "{case}");
        let first = &wire["composition"]["layers"][0];
        if staged {
            assert_eq!(first["type"], "Group", "{case}");
            assert_eq!(
                first["isHidden"].as_bool().unwrap_or(false),
                !sequence.video_tracks[0].clip(0).enabled,
                "{case}"
            );
            let video = &first["layers"][0];
            assert!(video.get("effects").is_none(), "{case}: {video}");
            assert_eq!(video["transform"]["skewAxis"], json!(0.0), "{case}");
            let tracks = layer_property_tracks(&wire);
            let keyed: Vec<_> = tracks
                .keys()
                .map(|(_, property)| property.as_str())
                .collect();
            let expected_keys: &[&str] = match case {
                "Scale Width keys without Uniform Scale" => &["scaleX"],
                _ => &[],
            };
            assert_eq!(keyed, expected_keys, "{case}");
        } else {
            assert_eq!(first["type"], "Video", "{case}");
            assert!(first.get("effects").is_none(), "{case}: {first}");
        }
    }
}

#[test]
fn approximated_transform_parameters_convert_with_one_warning_each() {
    use crate::OmissionKind::{Approximated, Omitted};
    let keys = |param, values: [f64; 2]| PrEffectParamAnimation {
        param,
        keys: PrEffectParamKeys::Scalar(
            values
                .into_iter()
                .zip([0, TICKS])
                .map(|(value, source_ticks)| PrScalarKeyframe {
                    source_ticks,
                    value,
                    easing: PrKeyframeEasing::Linear,
                })
                .collect(),
        ),
    };
    let shutter = |angle| PrTransform {
        composition_shutter_angle: false,
        shutter_angle: angle,
        ..DEFAULT_PR_TRANSFORM
    };
    let blur = "Transform motion blur (Shutter Angle 180) approximated by FX motion blur";
    // (Transform, its keys, the video's opacity, rotation, skew and skew
    // axis, its keyed properties, the composition shutter, the warnings'
    // kinds and starts)
    type Case = (
        PrTransform,
        Vec<PrEffectParamAnimation>,
        [f64; 4],
        Vec<&'static str>,
        Option<f64>,
        Vec<(crate::OmissionKind, &'static str)>,
    );
    #[rustfmt::skip]
    let cases: [Case; 7] = [
        // Clip A: Opacity 50 over the clip's Opacity 50 (the group's).
        (PrTransform { opacity: 50.0, ..DEFAULT_PR_TRANSFORM }, vec![], [50.0, 0.0, 0.0, 0.0], vec![], None, vec![(Approximated, "Transform Opacity 50 blends in linear light in Premiere; converted as sRGB opacity")]),
        (DEFAULT_PR_TRANSFORM, vec![keys(&TRANSFORM_OPACITY, [100.0, 50.0])], [100.0, 0.0, 0.0, 0.0], vec!["opacity"], None, vec![(Approximated, "keyed Transform Opacity blends in linear light in Premiere; converted as sRGB opacity")]),
        // Clip F: the shutter checkbox off at 180; a keyed angle at its first key.
        (shutter(180.0), vec![], [100.0, 0.0, 0.0, 0.0], vec![], Some(180.0), vec![(Approximated, blur)]),
        (shutter(180.0), vec![keys(&TRANSFORM_SHUTTER_ANGLE, [180.0, 90.0])], [100.0, 0.0, 0.0, 0.0], vec![], Some(180.0), vec![(Approximated, blur), (Approximated, "keyed Transform Shutter Angle converts as its first key's value 180")]),
        (PrTransform { bicubic_sampling: true, ..DEFAULT_PR_TRANSFORM }, vec![], [100.0, 0.0, 0.0, 0.0], vec![], None, vec![(Approximated, "Transform Sampling 1 (bicubic) has no FX equivalent; bilinear used")]),
        (PrTransform { skew: 30.0, rotation: 30.0, ..DEFAULT_PR_TRANSFORM }, vec![], [100.0, 30.0, 30.0, -90.0], vec![], None, vec![(Approximated, "Transform Skew 30 with a Rotation converts with FX's composition of skew, rotation and scale")]),
        // Scale Width keys under Uniform Scale do not render: none import.
        (PrTransform { uniform_scale: true, ..DEFAULT_PR_TRANSFORM }, vec![keys(&TRANSFORM_SCALE_WIDTH, [100.0, 70.0])], [100.0, 0.0, 0.0, 0.0], vec![], None, vec![(Omitted, "Transform Scale Width keys under Uniform Scale were not imported")]),
    ];
    for (index, (transform, animations, video_values, keyed, composition_shutter, warnings)) in
        cases.into_iter().enumerate()
    {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.id = Some("transform".into());
        clip.opacity = 50.0;
        clip.effects = vec![transform_effect(transform, animations)];
        let (wire, omissions) = import_with_frame(&sequence, [1920, 1080]);
        assert_eq!(
            omissions.len(),
            warnings.len(),
            "case {index}: {omissions:?}"
        );
        for (omission, (kind, start)) in omissions.iter().zip(&warnings) {
            assert_eq!(omission.record, "transform", "case {index}");
            assert_eq!(omission.kind, *kind, "case {index}");
            assert!(
                omission.reason.starts_with(start),
                "case {index}: {}",
                omission.reason
            );
        }
        let group = &wire["composition"]["layers"][0];
        assert_eq!(group["transform"]["opacity"], json!(50.0), "case {index}");
        let video = &group["layers"][0];
        let t = &video["transform"];
        assert_eq!(
            [&t["opacity"], &t["rotation"], &t["skew"], &t["skewAxis"]]
                .map(|value| value.as_f64().unwrap()),
            video_values,
            "case {index}"
        );
        let tracks: Vec<_> = layer_property_tracks(&wire)
            .into_keys()
            .map(|(_, property)| property)
            .collect();
        assert_eq!(tracks, keyed, "case {index}");
        // The video blurs at the composition's one shutter, phase 0.
        assert_eq!(
            video["motionBlur"].as_bool().unwrap_or(false),
            composition_shutter.is_some(),
            "case {index}"
        );
        let settings = &wire["composition"]["motionBlur"];
        match composition_shutter {
            Some(angle) => assert_eq!(
                (
                    &settings["enabled"],
                    &settings["shutterAngle"],
                    &settings["shutterPhase"]
                ),
                (&json!(true), &json!(angle), &json!(0.0)),
                "case {index}"
            ),
            None => assert!(
                settings
                    .get("enabled")
                    .is_none_or(|enabled| enabled == false),
                "case {index}: {settings}"
            ),
        }
    }
    // FX has one shutter per composition: the upper track's clip sets it, and
    // a lower clip that asks for 90 blurs at 180 and is reported, also inside
    // a nest (the first nest of `nested_sequence()`, under a clip on a new top
    // track), whose clips share the composition's shutter.
    let blurred = |clip: &mut crate::schema::PrVideoOccurrence, id: &str, angle| {
        clip.id = Some(id.into());
        clip.effects = vec![transform_effect(shutter(angle), vec![])];
    };
    let mut tracks = video_sequence();
    tracks.video_tracks.push(tracks.video_tracks[0].clone());
    blurred(tracks.video_tracks[0].clip_mut(0), "lower", 90.0);
    blurred(tracks.video_tracks[1].clip_mut(0), "upper", 180.0);
    let (mut nest, nest_media) = nested_sequence();
    nest.video_tracks
        .push(PrVideoTrack::media([clip_of("red", 0..10 * TICKS, 0)]));
    blurred(nest.video_tracks[2].clip_mut(0), "upper", 180.0);
    let inner = &mut nest.video_tracks[1].nests[0].sequence;
    blurred(inner.video_tracks[0].clip_mut(0), "lower", 90.0);
    for (sequence, media) in [(tracks, video_media()), (nest, nest_media)] {
        let (wire, omissions) = import(&sequence, &media);
        let reasons = [
            ("upper", blur),
            (
                "lower",
                "Transform motion blur (Shutter Angle 90) approximated by FX motion blur",
            ),
            (
                "lower",
                "composition shutter set to 180° by clip upper; clip lower requested 90°",
            ),
        ];
        let name = &sequence.name;
        assert_eq!(omissions.len(), reasons.len(), "{name}: {omissions:?}");
        for (omission, (record, start)) in omissions.iter().zip(reasons) {
            assert_eq!(omission.record, record, "{name}");
            assert_eq!(omission.kind, Approximated, "{name}");
            assert!(
                omission.reason.starts_with(start),
                "{name}: {}",
                omission.reason
            );
        }
        assert_eq!(
            wire["composition"]["motionBlur"]["shutterAngle"],
            json!(180.0),
            "{name}"
        );
    }
}

#[test]
fn moved_linear_wipe_stages_and_omits_the_effects_below_it() {
    let key = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    let static_motion = |clip: &mut crate::schema::PrVideoOccurrence| {
        clip.transform.scale = [80.0; 2];
        clip.transform.position = [0.45, 0.55];
    };
    let keyed_rotation = |clip: &mut crate::schema::PrVideoOccurrence| {
        clip.animations = vec![PrPropertyAnimation::Rotation(vec![
            key(0, 0.0),
            key(TICKS, 15.0),
        ])];
    };
    // A Corner Pin is a converted effect, omitted the same way.
    let pin = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::CornerPin(PrCornerPin {
            corners: [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]],
        }),
        animations: Vec::new(),
    };
    for (motion, effect, group_keys) in [
        (&static_motion as &dyn Fn(&mut _), blur(25.0), vec![]),
        (
            &keyed_rotation,
            blur(25.0),
            vec![json!("premiere-rotation-4-0")],
        ),
        (&static_motion, pin, vec![]),
    ] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.id = Some("moved-wipe".into());
        clip.linear_wipe = Some(wipe(&[(0, 100.0), (TICKS, 0.0)]));
        let name = effect.spec().display_name;
        clip.effects = vec![effect];
        motion(clip);
        let (wire, omissions) = import_with_frame(&sequence, [1920, 1080]);
        assert_eq!(
            omissions,
            [crate::Omission {
                scope: crate::OmissionScope::Feature,
                kind: crate::OmissionKind::Omitted,
                record: "moved-wipe".into(),
                reason: format!("{name} effect at stack position 1 was not imported: it applies after the Crop or Linear Wipe, which a group carries with the clip's Motion, and that group carries no effects"),
            }]
        );
        let group = &wire["composition"]["layers"][0];
        assert_eq!(group["type"], "Group");
        let [video, guide] = group["layers"].as_array().unwrap().as_slice() else {
            panic!("expected the video and its wipe guide");
        };
        assert!(video.get("effects").is_none());
        assert_eq!(guide["name"], "Premiere Linear Wipe guide 1");
        assert_eq!(group["masks"][0]["layer"], guide["id"]);
        // The wipe keys stay on the guide; the Motion keys are the group's.
        let entries = wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let first_key = |layer: &Value| {
            entries
                .iter()
                .filter(|entry| entry["target"]["layerId"] == layer["id"])
                .map(|entry| entry["animator"]["keyframes"][0]["id"].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(first_key(guide), [json!("premiere-linear-wipe-2-0")]);
        assert_eq!(first_key(group), group_keys);
        assert!(first_key(video).is_empty());
    }
}

#[test]
fn staged_clip_keeps_its_playback_on_the_group_clock() {
    let remap = crate::schema::PrTimeRemap {
        keys: [(0, 0), (2 * TICKS, 4 * TICKS)]
            .map(
                |(timeline_ticks, source_ticks)| crate::schema::PrTimeRemapKeyframe {
                    timeline_ticks,
                    source_ticks,
                    easing: PrKeyframeEasing::Linear,
                },
            )
            .into(),
    };
    // Constant 2x speed, reverse and a time remap, each on a clip at 1 to 3 s.
    for (playback_rate, source_ticks, time_remap) in [
        (2.0, 4 * TICKS, None),
        (-1.0, 2 * TICKS, None),
        (1.0, 2 * TICKS, Some(remap)),
    ] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        (clip.start_ticks, clip.end_ticks) = (TICKS, 3 * TICKS);
        (clip.in_ticks, clip.out_ticks) = (0, source_ticks);
        clip.playback_rate = playback_rate;
        clip.time_remap = time_remap;
        clip.crop.left = 20.0;
        clip.effects = vec![blur(40.0)];
        let (flat, _) = import_with_frame(&sequence, [1920, 1080]);
        sequence.video_tracks[0].clip_mut(0).effects_above_mask = 1;
        let (staged, omissions) = import_with_frame(&sequence, [1920, 1080]);
        assert!(omissions.is_empty(), "{omissions:?}");
        let keys = |layer: &Value, origin: u64| {
            crate::tests::support::playback_keys(layer)
                .iter()
                .map(|key| (key["time"].as_u64().unwrap() - origin, key["value"].clone()))
                .collect::<Vec<_>>()
        };
        let flat_video = &flat["composition"]["layers"][0];
        let video = &staged["composition"]["layers"][0]["layers"][0];
        assert_eq!(keys(video, 0), keys(flat_video, 1000), "{playback_rate}");
        assert_eq!(video["sourceRange"], flat_video["sourceRange"]);
    }
}

#[test]
fn staged_clip_from_a_trimmed_in_moves_its_remap_offset_to_the_group_clock() {
    // A clip at 1 to 3 s plays a curve from In 0.4 s at 0.8x; its keys, at
    // input 0 and 2 s, are reached 0.5 s before and 2 s after the clip start.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.start_ticks, clip.end_ticks) = (TICKS, 3 * TICKS);
    (clip.in_ticks, clip.out_ticks) = (2 * TICKS / 5, 2 * TICKS);
    clip.playback_rate = 0.8;
    clip.time_remap = Some(crate::schema::PrTimeRemap {
        keys: [(-2 * TICKS / 5, 0), (8 * TICKS / 5, 4 * TICKS)]
            .map(
                |(timeline_ticks, source_ticks)| crate::schema::PrTimeRemapKeyframe {
                    timeline_ticks,
                    source_ticks,
                    easing: PrKeyframeEasing::Linear,
                },
            )
            .into(),
    });
    clip.crop.left = 20.0;
    clip.effects = vec![blur(40.0)];
    let (flat, _) = import_with_frame(&sequence, [1920, 1080]);
    sequence.video_tracks[0].clip_mut(0).effects_above_mask = 1;
    let (staged, omissions) = import_with_frame(&sequence, [1920, 1080]);
    assert!(omissions.is_empty(), "{omissions:?}");
    let group = &staged["composition"]["layers"][0];
    assert_eq!(
        *crate::test_support::layer_range(group),
        json!({"start": 1000, "duration": 2000})
    );
    // On the sequence clock the keys are at 0.5 and 3 s. The group clock
    // starts at the clip, so there the first precedes zero and the keys and
    // input move 500 ms later together; both show source 1.6 s at sequence
    // time 1.5 s.
    let placements: [(&Value, u64, u64, [u64; 2]); 2] = [
        (&flat["composition"]["layers"][0], 1000, 0, [500, 3000]),
        (&group["layers"][0], 0, 500, [0, 2500]),
    ];
    for (video, start, offset, times) in placements {
        let playback = &video["playback"];
        assert_eq!(
            playback["inputRange"],
            json!({"start": start, "duration": 2000})
        );
        assert_eq!(playback["inputOffsetMs"], offset);
        let keys = playback["mapping"]["property"]["keyframes"]
            .as_array()
            .unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| (
                    key["time"].as_u64().unwrap(),
                    key["value"].as_u64().unwrap()
                ))
                .collect::<Vec<_>>(),
            times.into_iter().zip([0, 4000]).collect::<Vec<_>>()
        );
        assert!(keys.iter().all(|key| key["easing"]["type"] == "linear"));
        let input = 500 + start + offset;
        assert_eq!((input - times[0]) * 4000 / (times[1] - times[0]), 1600);
    }
}

#[test]
fn time_remap_parent_times_round_once_to_the_millisecond() {
    use crate::schema::{PrTimeRemap, PrTimeRemapKeyframe};
    // In, speed and two key times in input ticks after In; then the input
    // offset and key times in milliseconds, or `None` when the curve fails.
    type Case = (i64, f64, [i64; 2], Option<(i64, [u64; 2])>);
    let ms = TICKS_PER_MILLISECOND;
    let cases: [Case; 5] = [
        // A key one tick before zero moves the curve and its input 1 ms
        // later, where 1.5 ms then rounds forward.
        (ms, 1.0, [-1, ms / 2], Some((1, [1, 2]))),
        // At 2.5x the clip reaches input 63821519999 ticks 100.4999999984 ms
        // after its start: 100 ms, where rounding to a tick first gives 101.
        (0, 2.5, [0, 63_821_519_999], Some((0, [0, 100]))),
        // Untrimmed at unit speed, as before, a key less than 0.5 ms before
        // the start rounds to it and one 0.5 ms before fails, without offset.
        (0, 1.0, [1 - ms / 2, ms], Some((0, [0, 1]))),
        (0, 1.0, [-ms / 2, ms], None),
        // Times that round to one millisecond fail; they do not merge.
        (ms, 1.0, [0, ms / 4], None),
    ];
    for (in_ticks, playback_rate, times, expected) in cases {
        // The clip plays the curve, as an imported occurrence does.
        let mut clip = clip_of("source", 0..TICKS, in_ticks);
        clip.playback_rate = playback_rate;
        clip.time_remap = Some(PrTimeRemap {
            keys: times
                .into_iter()
                .zip([0, TICKS])
                .map(|(timeline_ticks, source_ticks)| PrTimeRemapKeyframe {
                    timeline_ticks,
                    source_ticks,
                    easing: PrKeyframeEasing::Linear,
                })
                .collect(),
        });
        let remap = clip.time_remap.as_ref().unwrap();
        let converted = super::time_remap_property(
            remap,
            0,
            clip.playback_rate,
            clip.remaps_from_in_or_speed(),
        )
        .ok();
        let actual = converted.map(|(property, offset)| {
            let keys = property.keyframes().iter();
            (offset, keys.map(|key| key.time.as_millis()).collect())
        });
        let expected = expected.map(|(offset, times)| (offset, times.to_vec()));
        assert_eq!(actual, expected, "{times:?}");
    }
}

#[test]
fn slow_time_remap_past_its_covering_keys_once_rounded_omits_only_its_clip() {
    use crate::schema::{PrTimeRemap, PrTimeRemapKeyframe};
    // A 2 s clip plays input 3 to 4 ticks at speed 2^-38, a span that In to
    // Out matches within the tolerance; its keys, at input 0, 1, 2, 4 and 5
    // ticks, are 1082.1 ms apart on the parent clock. Moved 3247 ms later, it
    // plays 3247 to 5247 ms, past the key at 4329 ms: into the segment before
    // a last key at the media end, or after an ordinary last key.
    let media = video_media();
    let facts = &media[&MediaId("source".into())];
    // Source seconds of each key, with and without the media-end key.
    let curves: [&[i64]; 2] = [&[0, 1, 3, 5, 10], &[0, 1, 3, 5]];
    for seconds in curves {
        let mut sequence = video_sequence();
        let mut sibling = sequence.video_tracks[0].clip(0).clone();
        (sibling.start_ticks, sibling.end_ticks) = (5 * TICKS, 7 * TICKS);
        (sibling.in_ticks, sibling.out_ticks) = (0, 2 * TICKS);
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.end_ticks = 2 * TICKS;
        (clip.in_ticks, clip.out_ticks) = (3, 4);
        clip.playback_rate = 1.0 / (1_u64 << 38) as f64;
        clip.time_remap = Some(PrTimeRemap {
            keys: [-3, -2, -1, 1, 2]
                .into_iter()
                .zip(seconds)
                .map(|(timeline_ticks, source)| PrTimeRemapKeyframe {
                    timeline_ticks,
                    source_ticks: source * TICKS,
                    easing: PrKeyframeEasing::Linear,
                })
                .collect(),
        });
        // Native validation, which checks the saved In and Out, admits it.
        clip.validate(FrameRate::Fps30, facts).unwrap();
        sequence.video_tracks[0]
            .items
            .push(PrVideoItem::Media(sibling));
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        let reason = "clip was not imported: unsupported conversion: TimeRemapping from a source In or at another speed plays input 3247 to 5247 ms, outside its covering keys at 1 to 4329 ms";
        let occurrences: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.scope == crate::OmissionScope::Occurrence)
            .map(|omission| omission.reason.as_str())
            .collect();
        assert_eq!(occurrences, [reason], "{omissions:?}");
        // The sibling still converts.
        let videos: Vec<_> = wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .collect();
        assert_eq!(videos.len(), 1, "{seconds:?}");
        assert_eq!(videos[0]["playback"]["inputRange"]["start"], 5000);
    }
}

#[test]
fn staged_clips_inside_each_nest_copy_get_unique_ids() {
    let (mut outer, media) = nested_sequence();
    for nest in &mut outer.video_tracks[1].nests {
        let clip = nest.sequence.video_tracks[0].clip_mut(0);
        clip.crop.left = 20.0;
        clip.effects = vec![blur(40.0)];
        clip.effects_above_mask = 1;
    }
    let ids = crate::tesseract_output::asset_ids_in_order(&outer, &media);
    let mut omissions = Vec::new();
    let wire = premiere_to_tesseract(&outer, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let mut item_ids = std::collections::BTreeSet::new();
    let mut stages = Vec::new();
    fn walk<'a>(
        layers: &'a Value,
        item_ids: &mut std::collections::BTreeSet<u64>,
        stages: &mut Vec<(&'a Value, &'a Value)>,
    ) {
        for layer in layers.as_array().into_iter().flatten() {
            assert!(item_ids.insert(layer["id"].as_u64().unwrap()), "{layer}");
            for mask in layer["masks"].as_array().into_iter().flatten() {
                assert!(item_ids.insert(mask["id"].as_u64().unwrap()), "{mask}");
            }
            for child in layer["layers"].as_array().into_iter().flatten() {
                if child["type"] == "Group" {
                    stages.push((layer, child));
                }
            }
            walk(&layer["layers"], item_ids, stages);
        }
    }
    walk(&wire["composition"]["layers"], &mut item_ids, &mut stages);
    // Each placement's copy of the staged clip is a stage group in its nest.
    assert_eq!(stages.len(), 2);
    for (nest, stage) in stages {
        assert_eq!(nest["name"], "Inner");
        assert_eq!(stage["parent"], nest["id"]);
        for child in stage["layers"].as_array().unwrap() {
            assert_eq!(child["parent"], stage["id"]);
        }
    }
}

#[test]
fn crop_feather_approximations_are_reported_without_rejecting_negative_native_values() {
    for feather in [-12.0, 12.0] {
        let mut sequence = video_sequence();
        sequence.video_tracks[0].clip_mut(0).crop = PrStaticCrop {
            left: 7.0,
            edge_feather: feather,
            ..PrStaticCrop::default()
        };
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions)
            .expect("valid native Crop feather should not abort import");
        let wire = document.to_json_value().unwrap();
        assert_eq!(
            wire["composition"]["layers"][0]["masks"][0]["feather"],
            json!([feather.max(0.0), feather.max(0.0)])
        );
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(omissions[0].scope, crate::OmissionScope::Feature);
        assert_eq!(omissions[0].kind, crate::OmissionKind::Approximated);
        assert!(omissions[0].reason.contains("Edge Feather"));
    }
}

/// The `(propertyType, keys)` tracks of `layer` in `wire`.
fn layer_tracks(wire: &Value, layer: &Value) -> Vec<(Value, Vec<(Value, Value)>)> {
    wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == layer["id"])
        .map(|entry| {
            let keys: Vec<_> = entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| (key["layerTime"].clone(), key["value"].clone()))
                .collect();
            (entry["target"]["propertyType"].clone(), keys)
        })
        .collect()
}

#[test]
fn opacity_mask_imports_as_a_shape_guide_in_source_pixels_beside_its_moved_video() {
    // The half-opaque, feathered rectangle over the middle half of the
    // source frame, on a moved, keyed, half-opaque portrait clip.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.opacity_mask = Some(opacity_mask());
    clip.transform.scale = [80.0; 2];
    clip.transform.position = [0.45, 0.55];
    clip.opacity = 60.0;
    let key = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    clip.animations = vec![
        PrPropertyAnimation::Rotation(vec![key(0, 0.0), key(TICKS, 20.0)]),
        PrPropertyAnimation::Opacity(vec![key(0, 60.0), key(TICKS, 100.0)]),
    ];
    let (wire, omissions) = import_with_frame(&sequence, [1080, 1920]);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.scope, omission.kind, omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            crate::OmissionScope::Feature,
            crate::OmissionKind::Approximated,
            crate::schema::MASK_FEATHER_APPROXIMATION
        )]
    );
    let [video, guide, _canvas] = wire["composition"]["layers"].as_array().unwrap().as_slice()
    else {
        panic!("expected a flat video, its guide and the canvas");
    };
    assert_eq!(guide["type"], "Shape");
    assert_eq!(guide["name"], "Premiere Opacity mask 1");
    // The unit-frame outline in source pixels, closed, without paint.
    assert_eq!(
        guide["shape"],
        json!({"path": {"commands": [
            {"type": "moveTo", "x": 270.0, "y": 480.0},
            {"type": "lineTo", "x": 810.0, "y": 480.0},
            {"type": "lineTo", "x": 810.0, "y": 1440.0},
            {"type": "lineTo", "x": 270.0, "y": 1440.0},
            {"type": "close"}
        ]}})
    );
    assert_eq!(
        video["masks"],
        json!([{"id": 3, "mode": "add", "inverted": false, "layer": 2, "feather": [12.0, 12.0], "expansion": 0.0, "opacity": 0.5}])
    );
    // The guide shares the video's transform and Motion keys, not its Opacity.
    assert_eq!(guide["transform"], video["transform"]);
    assert_eq!(video["transform"]["opacity"], 60.0);
    let video_tracks = layer_tracks(&wire, video);
    assert_eq!(video_tracks.len(), 2);
    assert_eq!(
        layer_tracks(&wire, guide),
        video_tracks
            .into_iter()
            .filter(|(property, _)| property != "opacity")
            .collect::<Vec<_>>()
    );
}

#[test]
fn opacity_mask_over_effects_stages_the_disabled_clip() {
    // Every effect applies before the Opacity mask, so one converted effect
    // stages the clip: the group takes the mask, the video keeps the effect.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    let mut mask = opacity_mask();
    mask.feather = 0.0;
    clip.opacity_mask = Some(mask);
    clip.effects = vec![blur(40.0)];
    clip.effects_above_mask = 1;
    clip.enabled = false;
    let (wire, omissions) = import_with_frame(&sequence, [1920, 1080]);
    assert!(omissions.is_empty(), "{omissions:?}");
    let [group, canvas] = wire["composition"]["layers"].as_array().unwrap().as_slice() else {
        panic!("expected one stage group and the canvas");
    };
    assert_eq!(group["type"], "Group");
    assert_eq!(group["isHidden"], true);
    assert_eq!(group["masks"][0]["layer"], 2);
    assert_eq!(group["masks"][0]["opacity"], 0.5);
    let [video, guide] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the video and its mask guide");
    };
    assert_eq!(video["effects"][0]["effect"]["blurriness"], 40.0);
    assert!(video.get("masks").is_none());
    assert_eq!(guide["type"], "Shape");
    assert_eq!(guide["transform"], canvas["transform"], "identity");
    assert_eq!(
        guide["shape"]["path"]["commands"][2],
        json!({"type": "lineTo", "x": 1440.0, "y": 810.0})
    );
}

#[test]
fn mask_path_keys_import_on_the_trimmed_owner_clock_at_unit_speed() {
    // The rectangle in the source frame's pixels, `left` px from its edge.
    let outline = |left: f64| {
        json!({"type": "path", "value": {"commands": [
            {"type": "moveTo", "x": left, "y": 270.0},
            {"type": "lineTo", "x": left + 960.0, "y": 270.0},
            {"type": "lineTo", "x": left + 960.0, "y": 810.0},
            {"type": "lineTo", "x": left, "y": 810.0},
            {"type": "close"}
        ]}})
    };
    let outline_keys = (
        json!("shapePath"),
        vec![(json!(-500), outline(480.0)), (json!(500), outline(960.0))],
    );
    let rotation = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    // Flat, the guide repeats the clip's Rotation keys beside its outline
    // keys, all on the clip clock; staged, it holds the outline keys alone.
    for staged in [false, true] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
        clip.opacity_mask = Some(keyed_opacity_mask());
        clip.animations = vec![PrPropertyAnimation::Rotation(vec![
            rotation(TICKS, 0.0),
            rotation(2 * TICKS, 20.0),
        ])];
        if staged {
            clip.effects = vec![blur(40.0)];
            clip.effects_above_mask = 1;
        }
        let (wire, omissions) = import_with_frame(&sequence, [1920, 1080]);
        assert!(
            omissions
                .iter()
                .all(|omission| omission.reason == crate::schema::MASK_FEATHER_APPROXIMATION),
            "{omissions:?}"
        );
        let root = &wire["composition"]["layers"][0];
        let (owner, layers) = if staged {
            (root, &root["layers"])
        } else {
            (root, &wire["composition"]["layers"])
        };
        let guide = layers
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == owner["masks"][0]["layer"])
            .unwrap();
        assert_eq!(guide["shape"]["path"], outline(480.0)["value"], "{staged}");
        let mut guide_tracks = layer_tracks(&wire, guide);
        let position = guide_tracks
            .iter()
            .position(|track| *track == outline_keys)
            .unwrap_or_else(|| panic!("{staged}: {guide_tracks:?}"));
        guide_tracks.remove(position);
        let repeated = if staged {
            Vec::new()
        } else {
            layer_tracks(&wire, &layers[0])
        };
        assert_eq!(guide_tracks, repeated, "{staged}");
    }
    // The typed mapper also keeps still masks static; it must not freeze
    // a keyed outline when called without the native reader's host check.
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).opacity_mask = Some(keyed_opacity_mask());
    let mut media = video_media();
    media
        .values_mut()
        .next()
        .unwrap()
        .video
        .as_mut()
        .unwrap()
        .kind = crate::schema::PrMediaKind::Still { alpha: false };
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert!(wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|layer| layer["type"] == "Rect"));
    assert!(omissions.iter().any(|omission| omission.reason
        == "Mask Path keys on a still are not converted; only a video clip's Opacity mask converts keyed"));
    // A reverse or a hold needs a non-unit mapping from the source clock.
    // Neither can use the signed source-In translation tested above.
    type Clip = crate::format::PrVideoOccurrence;
    type Case = (&'static str, fn(&mut Clip), &'static str);
    let retimed = "they are on the source clock, which a retimed, reversed, held or time-remapped clip does not play at unit speed from its start";
    let cases: [Case; 2] = [
        (
            "reverse",
            |clip: &mut Clip| clip.playback_rate = -1.0,
            retimed,
        ),
        (
            "Frame Hold",
            |clip: &mut Clip| {
                clip.time_remap = Some(crate::schema::PrTimeRemap::frame_hold(TICKS, 5 * TICKS));
            },
            retimed,
        ),
    ];
    for (case, edit, reason) in cases {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.opacity_mask = Some(keyed_opacity_mask());
        edit(clip);
        let (wire, omissions) = import_with_frame(&sequence, [1920, 1080]);
        let types: Vec<_> = wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["Rect"], "{case}");
        assert!(
            omissions.iter().any(
                |omission| omission.scope == crate::OmissionScope::Occurrence
                    && omission.reason.contains(&format!(
                        "Mask Path keys were not imported: {reason}; occurrence omitted"
                    ))
            ),
            "{case}: {omissions:?}"
        );
    }
}

#[test]
fn numeric_opacity_masks_do_not_freeze_on_static_still_import() {
    for control in ["Feather", "Opacity", "Expansion", "static Expansion"] {
        let mut sequence = video_sequence();
        let mut mask = opacity_mask();
        let keys = vec![PrScalarKeyframe {
            source_ticks: 0,
            value: 20.0,
            easing: PrKeyframeEasing::Linear,
        }];
        match control {
            "Feather" => mask.feather_keys = keys,
            "Opacity" => mask.opacity_keys = keys,
            "Expansion" => mask.expansion_keys = keys,
            _ => mask.expansion = 20.0,
        }
        sequence.video_tracks[0].clip_mut(0).opacity_mask = Some(mask);
        let mut media = video_media();
        let stream = media.values_mut().next().unwrap().video.as_mut().unwrap();
        stream.kind = crate::schema::PrMediaKind::Still { alpha: false };
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        assert!(wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|layer| layer["type"] == "Rect"));
        let reason = if control == "static Expansion" {
            "Mask Expansion on a still is not converted"
        } else {
            "numeric Opacity mask keys on a still are not converted; this host has no admitted numeric mask clock"
        };
        assert!(
            omissions.iter().any(|omission| omission.reason == reason),
            "{control}: {omissions:?}"
        );
    }
}

#[test]
fn a_clips_blend_converts_on_its_root_layer_both_ways() {
    use crate::{schema::PrBlendMode, OmissionKind};
    let dissolve = PrBlendMode::Unmeasured {
        primary: 6,
        legacy: 1,
    };
    // (blend, whether the clip stages, its root layer type and FX blend, the
    // exported blend).
    for (blend, staged, root, fx, exported) in [
        // At half Opacity, over the black canvas.
        (
            PrBlendMode::Screen,
            false,
            "Video",
            "screen",
            PrBlendMode::Screen,
        ),
        // A disabled clip whose Opacity mask applies after a blur stages it:
        // the group blends the masked picture and its video stays Normal.
        (
            PrBlendMode::DarkerColor,
            true,
            "Group",
            "darkerColor",
            PrBlendMode::DarkerColor,
        ),
        // An unmeasured code keeps the clip, Normal.
        (dissolve, true, "Group", "normal", PrBlendMode::Normal),
    ] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        (clip.blend_mode, clip.opacity) = (blend, 50.0);
        if staged {
            let mut mask = opacity_mask();
            mask.feather = 0.0;
            clip.opacity_mask = Some(mask);
            clip.effects = vec![blur(40.0)];
            clip.effects_above_mask = 1;
            clip.enabled = false;
        }
        let (wire, omissions) = import_with_frame(&sequence, [1920, 1080]);
        let layer = &wire["composition"]["layers"][0];
        assert_eq!(
            (
                &layer["type"],
                &layer["blendMode"],
                &layer["transform"]["opacity"]
            ),
            (&json!(root), &json!(fx), &json!(50.0)),
            "{blend:?}"
        );
        if staged {
            assert_eq!(layer["isHidden"], true);
            assert_eq!(layer["masks"][0]["opacity"], 0.5);
            assert_eq!(layer["layers"][0]["blendMode"], "normal");
        }
        // One report for an approximated blend, in each direction that
        // converts it; Screen converts as its measured formula.
        let reports = |omissions: &[crate::Omission]| -> Vec<(OmissionKind, String)> {
            omissions
                .iter()
                .filter(|omission| omission.reason.contains("Blend Mode"))
                .map(|omission| (omission.kind, omission.reason.clone()))
                .collect()
        };
        let report = |mode: PrBlendMode| -> Vec<(OmissionKind, String)> {
            mode.approximation()
                .map(|warning| (OmissionKind::Approximated, warning))
                .into_iter()
                .collect()
        };
        assert_eq!(reports(&omissions), report(blend), "{blend:?}");

        let document = fx_schema::EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let facts = BTreeMap::from([(
            "premiere-video-1".to_owned(),
            MediaFacts::Video(VideoMedia {
                pixel_aspect: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                codec: crate::schema::VideoCodec::H264,
                bit_depth: 8,
                colour: None,
                width: 1920,
                height: 1080,
                timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
            }),
        )]);
        let mut omissions = Vec::new();
        let project = tesseract_to_premiere(
            &document,
            &facts,
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!(
            (clip.blend_mode, clip.opacity),
            (exported, 50.0),
            "{blend:?}"
        );
        assert_eq!(clip.opacity_mask.is_some(), staged, "{blend:?}");
        assert_eq!(reports(&omissions), report(exported), "{blend:?}");
    }
}

#[test]
fn cuts_preserve_distinct_ranges_at_persisted_millisecond_precision() {
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).end_ticks = 60 * THIRTY_FPS_TICKS;
    sequence.video_tracks[0].clip_mut(0).out_ticks = 60 * THIRTY_FPS_TICKS;
    let mut second = sequence.video_tracks[0].clip(0).clone();
    second.start_ticks = 60 * THIRTY_FPS_TICKS;
    second.end_ticks = 151 * THIRTY_FPS_TICKS;
    second.in_ticks = 90 * THIRTY_FPS_TICKS;
    second.out_ticks = 181 * THIRTY_FPS_TICKS;
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(second));
    sequence.timeline_end_ticks = 151 * THIRTY_FPS_TICKS;
    let document = project_document(&sequence);
    assert_eq!(document["duration"], 5.033);
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3);
    for (layer, active, source) in [
        (
            &layers[0],
            json!({"start": 0, "duration": 2000}),
            json!({"start": 0, "duration": 2000}),
        ),
        (
            &layers[1],
            json!({"start": 2000, "duration": 3033}),
            json!({"start": 3000, "duration": 3033}),
        ),
    ] {
        assert_eq!((*crate::test_support::layer_range(layer)), active);
        assert_eq!(layer["sourceRange"], source);
    }
}

#[test]
fn constant_speed_and_reverse_import_as_editable_time_remap() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.end_ticks = 2 * TICKS;
    clip.in_ticks = 4 * TICKS;
    clip.out_ticks = 5 * TICKS;
    clip.playback_rate = -0.5;

    let document = project_document(&sequence);
    let layer = &document["composition"]["layers"][0];
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        layer["sourceRange"],
        json!({"start": 5000, "duration": 1000})
    );
    assert_eq!(
        layer["volume"], 0.0,
        "the picture layer stays silent; sound imports as a separate audio layer"
    );
    assert_eq!(
        layer["playback"]["mapping"]["property"],
        json!({
            "keyframes": [
                {"id": "premiere-playback-start", "time": 0, "value": 6000, "easing": {"type": "linear"}},
                {"id": "premiere-playback-end", "time": 2000, "value": 5000, "easing": {"type": "linear"}}
            ],
            "before": "inactive",
            "after": "inactive"
        })
    );
}

#[test]
fn reverse_rounding_uses_the_same_endpoints_for_ranges_and_keys() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks = 2 * THIRTY_FPS_TICKS;
    clip.end_ticks = 4 * THIRTY_FPS_TICKS;
    clip.in_ticks = 2 * THIRTY_FPS_TICKS;
    clip.out_ticks = 4 * THIRTY_FPS_TICKS;
    clip.playback_rate = -1.0;

    let document = project_document(&sequence);
    let layer = &document["composition"]["layers"][0];
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 67, "duration": 66})
    );
    assert_eq!(layer["sourceRange"], json!({"start": 9867, "duration": 66}));
    assert_eq!(crate::tests::support::playback_keys(layer)[0]["time"], 67);
    assert_eq!(crate::tests::support::playback_keys(layer)[1]["time"], 133);
    assert_eq!(
        crate::tests::support::playback_keys(layer)[0]["value"],
        9933
    );
    assert_eq!(
        crate::tests::support::playback_keys(layer)[1]["value"],
        9867
    );
}

#[test]
fn positive_speed_nonintegral_source_endpoints_stay_consistent() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = 1000 * TICKS_PER_MILLISECOND + 3 * TICKS_PER_MILLISECOND / 5;
    clip.out_ticks = 8000 * TICKS_PER_MILLISECOND + 2 * TICKS_PER_MILLISECOND / 5;
    clip.playback_rate = 1.4;

    let document = project_document(&sequence);
    let layer = &document["composition"]["layers"][0];
    assert_eq!(
        layer["sourceRange"],
        json!({"start": 1001, "duration": 6999})
    );
    assert_eq!(
        crate::tests::support::playback_keys(layer)[0]["value"],
        1001
    );
    assert_eq!(
        crate::tests::support::playback_keys(layer)[1]["value"],
        8000
    );
}

#[test]
fn focused_native_reverse_fixture_imports_as_editable_time_remap() {
    // This fixture derives from the independently Adobe-exported minimal
    // nonzero-source-trim project. Only its synchronized names, timeline end,
    // and exact native vhsvertical speed/reverse clip fields differ; the format
    // regression pins that complete decompressed element diff.
    let bytes =
        include_bytes!("../../../tests/fixtures/feature_constant_reverse_0_905_strict.prproj");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut xml)
        .unwrap();
    let project =
        inspect_project_with_media(&xml, Some("c8acf9c1-34b2-4086-9f55-d528950a7059")).unwrap();
    let (mut sequences, media) = project.into_parts();
    assert_eq!(sequences.len(), 1);
    let sequence = sequences.remove(0);
    let clips: Vec<_> = sequence.video_occurrences().collect();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].timeline_ticks(), 0..1_879_718_400_000);
    assert_eq!(clips[0].source_ticks(), 762_048_000_000..2_463_193_152_000);
    assert_eq!(clips[0].playback_rate, -0.905);
    assert_eq!(
        media[&clips[0].media].name(),
        "feature_timecoded_source.mp4"
    );

    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut Vec::new()).unwrap();
    let wire = document.to_json_value().unwrap();
    assert_eq!(wire["duration"], 7.4);
    assert_eq!(wire["dimensions"], json!({"width": 1920, "height": 1080}));
    let layer = &wire["composition"]["layers"][0];
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 0, "duration": 7400})
    );
    assert_eq!(
        layer["sourceRange"],
        json!({"start": 303, "duration": 6697})
    );
    assert_eq!(
        layer["playback"]["mapping"]["property"],
        json!({
            "keyframes": [
                {"id": "premiere-playback-start", "time": 0, "value": 7000, "easing": {"type": "linear"}},
                {"id": "premiere-playback-end", "time": 7400, "value": 303, "easing": {"type": "linear"}}
            ],
            "before": "inactive",
            "after": "inactive"
        })
    );
    assert_eq!(
        layer["volume"], 0.0,
        "the picture layer stays silent; sound imports as a separate audio layer"
    );

    let source = media[&clips[0].media].video.as_ref().unwrap();
    let facts = BTreeMap::from([(
        ids[&clips[0].media].as_str().to_owned(),
        MediaFacts::Video(VideoMedia {
            pixel_aspect: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            codec: crate::schema::VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: source.width,
            height: source.height,
            timing: crate::media::VideoTiming::for_test(
                source.frame_rate.supported().unwrap(),
                source.intrinsic_ticks,
            ),
        }),
    )]);
    let mut omissions = Vec::new();
    let roundtrip = tesseract_to_premiere(
        &document,
        &facts,
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.contains("playback was not exported")),
        "native reverse playback was approximated: {omissions:?}"
    );
    let roundtrip_clip = roundtrip
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(roundtrip_clip.playback_rate, -0.905);
    // Export preserves the edited FX frame convention, not the former native
    // control tuple. The selected last frame needs its own native Frame Hold.
    let frame = crate::format::FrameRate::Fps30.ticks_per_frame();
    let boundary = frame - 1;
    assert_eq!(
        roundtrip_clip.source_ticks(),
        (clips[0].in_ticks - boundary)..(clips[0].out_ticks - boundary - frame * 905 / 1000)
    );
    assert_eq!(roundtrip_clip.end_ticks, clips[0].end_ticks - frame);
    let exported: Vec<_> = roundtrip
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    assert_eq!(exported.len(), 2);
    assert_eq!(exported[1].start_ticks, roundtrip_clip.end_ticks);
    assert_eq!(exported[1].end_ticks, clips[0].end_ticks);
    assert_eq!(
        exported[1]
            .time_remap
            .as_ref()
            .unwrap()
            .held_source_ticks(frame),
        Some(9 * frame)
    );
}

#[test]
fn remapped_rotation_keeps_unmeasured_properties_and_hosts_closed() {
    let (sequence, media) = native_fixture(
        "feature_time_remap_rotation_26_5_strict.prproj",
        "9a10a3b7-a83b-47d9-a68c-91d06d937738",
    );
    let original = sequence.video_tracks[0].clip(0);
    assert!(original.has_media_clock_rotation());
    let PrPropertyAnimation::Rotation(keys) = &original.animations[0] else {
        panic!("native fixture must contain Rotation keys");
    };
    let mut opacity = original.clone();
    opacity.animations = vec![PrPropertyAnimation::Opacity(keys.clone())];
    let mut held_key = original.clone();
    let PrPropertyAnimation::Rotation(keys) = &mut held_key.animations[0] else {
        unreachable!();
    };
    keys[0].easing = PrKeyframeEasing::Hold;
    let mut held_media = original.clone();
    held_media.time_remap = Some(crate::schema::PrTimeRemap::frame_hold(0, 2 * TICKS));
    let mut masked = original.clone();
    masked.crop = left_crop();
    for clip in [opacity, held_key, held_media, masked] {
        assert!(!clip.has_media_clock_rotation());
        assert!(clip
            .validate(FrameRate::Fps30, &media[&clip.media])
            .is_err());
    }
    // A Transform moves Motion to a stage group without the video's remap.
    // Keep that Rotation static rather than silently use the stage clock.
    let mut staged = sequence;
    staged.video_tracks[0].clip_mut(0).effects =
        vec![transform_effect(DEFAULT_PR_TRANSFORM, Vec::new())];
    let (wire, omissions) = import(&staged, &media);
    assert!(animated_properties(&wire).is_empty());
    assert!(
        omissions.iter().any(|omission| {
            omission
                .reason
                .contains("Rotation animation was not imported")
        }),
        "{omissions:?}"
    );
}

#[test]
fn imported_rotation_is_editable_and_retains_signed_off_trim_keys() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.animations = vec![PrPropertyAnimation::Rotation(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 2 * TICKS,
            value: 90.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 7 * TICKS,
            value: 180.0,
            easing: PrKeyframeEasing::Linear,
        },
    ])];
    let document = project_document(&sequence);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let animator = &entries[0]["animator"];
    assert_eq!(animator["type"], "keyframes");
    assert_eq!(animator["keyframes"].as_array().unwrap().len(), 3);
    assert_eq!(animator["keyframes"][0]["layerTime"], -1000);
    assert_eq!(animator["keyframes"][1]["layerTime"], 1000);
    assert_eq!(animator["keyframes"][2]["layerTime"], 6000);
    assert_eq!(
        animator["keyframes"][1]["value"],
        json!({"type":"float","value":90.0})
    );
    assert_eq!(animator["keyframes"][1]["easing"]["type"], "linear");

    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
    let typed = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut Vec::new()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let asset = dir.path().join("source.mp4");
    std::fs::write(&asset, b"archive persistence uses original source bytes").unwrap();
    let output = dir.path().join("rotation.tsrct");
    tesseract_file::TesseractFileBuilder::try_new(typed)
        .unwrap()
        .add_asset("premiere-video-1", &asset, tesseract_file::AssetKind::Video)
        .unwrap()
        .write(&output)
        .unwrap();
    let reopened = tesseract_file::TesseractFile::open(&output).unwrap();
    assert_eq!(
        reopened.project_json().unwrap()["composition"]["dynamics"]["entries"][0]["animator"],
        *animator
    );
}

#[test]
fn animated_source_uses_source_dimensions_for_anchor_and_canvas_for_position() {
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).animations = vec![PrPropertyAnimation::Rotation(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: TICKS,
            value: 60.0,
            easing: PrKeyframeEasing::Linear,
        },
    ])];
    let mut media = video_media();
    let video = media.values_mut().next().unwrap().video.as_mut().unwrap();
    video.width = 1280;
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions).unwrap();
    let wire = document.to_json_value().unwrap();
    let layer = &wire["composition"]["layers"][0];
    assert_eq!(layer["transform"]["anchorPoint"], json!([640.0, 540.0]));
    assert_eq!(layer["transform"]["position"], json!([960.0, 540.0]));
    assert_eq!(
        layer["source"]["sourceRect"],
        json!({"x": 0.0, "y": 0.0, "width": 1280.0, "height": 1080.0})
    );
    assert_eq!(
        wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[test]
fn anchor_point_and_scale_width_keys_import_on_their_own_frames_and_key_ids() {
    // A 1280 x 720 source on the 1920 x 1080 canvas: Anchor Point keys are
    // fractions of the source and Position keys of the canvas, so taking one
    // frame for the other moves both. Scale Width keys move the X axis alone,
    // and each layer track's key ids name its own property.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.transform.scale = [50.0, 50.0];
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
    clip.animations = vec![
        PrPropertyAnimation::Position(vec![point(0, [0.5, 0.5]), point(TICKS, [0.25, 0.75])]),
        PrPropertyAnimation::AnchorPoint(vec![point(0, [0.5, 0.5]), point(TICKS, [0.25, 0.75])]),
        PrPropertyAnimation::ScaleWidth(vec![scalar(0, 50.0), scalar(TICKS, 100.0)]),
    ];
    let mut media = video_media();
    let video = media.values_mut().next().unwrap().video.as_mut().unwrap();
    (video.width, video.height) = (1280, 720);
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let wire = document.to_json_value().unwrap();
    let layer = &wire["composition"]["layers"][0];
    assert_eq!(layer["transform"]["anchorPoint"], json!([640.0, 360.0]));
    assert_eq!(layer["transform"]["position"], json!([960.0, 540.0]));
    assert_eq!(layer["transform"]["scale"], json!([50.0, 50.0]));
    let tracks: serde_json::Map<String, Value> = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            assert_eq!(entry["target"]["layerId"], layer["id"]);
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            let keys = keys
                .iter()
                .map(|key| json!([key["id"], key["layerTime"], key["value"]["value"]]))
                .collect();
            let property = entry["target"]["propertyType"].as_str().unwrap();
            (property.to_owned(), Value::Array(keys))
        })
        .collect();
    assert_eq!(
        Value::Object(tracks),
        json!({
            "positionX": [["premiere-position-x-1-0", 0, 960.0], ["premiere-position-x-1-1", 1000, 480.0]],
            "positionY": [["premiere-position-y-1-0", 0, 540.0], ["premiere-position-y-1-1", 1000, 810.0]],
            "anchorPointX": [["premiere-anchor-point-x-1-0", 0, 640.0], ["premiere-anchor-point-x-1-1", 1000, 320.0]],
            "anchorPointY": [["premiere-anchor-point-y-1-0", 0, 360.0], ["premiere-anchor-point-y-1-1", 1000, 540.0]],
            "scaleX": [["premiere-scale-x-1-0", 0, 50.0], ["premiere-scale-x-1-1", 1000, 100.0]],
        })
    );
}

#[test]
fn premiere_frame_ticks_round_to_nearest_signed_millisecond() {
    let animation = PrPropertyAnimation::Rotation(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: THIRTY_FPS_TICKS,
            value: 10.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 2 * THIRTY_FPS_TICKS,
            value: 20.0,
            easing: PrKeyframeEasing::Linear,
        },
    ]);
    let layer_id = fx_schema::LayerId::new(1);
    let keys = super::scalar_keys(
        &animation,
        THIRTY_FPS_TICKS,
        layer_id,
        fx_schema::PropType::Rotation,
    )
    .unwrap();
    assert_eq!(
        keys.keyframes()
            .iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        [-33, 0, 33]
    );
    let keys = super::scalar_keys(&animation, 0, layer_id, fx_schema::PropType::Rotation).unwrap();
    assert_eq!(
        keys.keyframes()
            .iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        [0, 33, 67]
    );
}

#[test]
fn native_key_times_colliding_after_millisecond_rounding_warn_and_keep_clip() {
    let mut sequence = video_sequence();
    sequence.video_tracks[0]
        .clip_mut(0)
        .animations
        .push(PrPropertyAnimation::Rotation(vec![
            PrScalarKeyframe {
                source_ticks: 0,
                value: 0.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: TICKS / 10_000,
                value: 1.0,
                easing: PrKeyframeEasing::Linear,
            },
        ]));
    let ids = std::collections::BTreeMap::from([(
        sequence.video_tracks[0].clip(0).media.clone(),
        AssetId::from_trusted("resolved-video"),
    )]);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions).unwrap();
    assert!(
        document.to_json_value().unwrap()["composition"]["dynamics"]["entries"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.reason.contains("cannot be imported")),
        "{omissions:?}"
    );
}

/// `[target layer, [keyframe ids]]` of each animation graph entry.
fn keyframe_ids(document: &Value) -> Vec<Value> {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            let ids: Vec<_> = keys.iter().map(|key| &key["id"]).collect();
            json!([entry["target"]["layerId"], ids])
        })
        .collect()
}

/// Opacity keys at source 0 s (100%) and 1 s (0%).
fn opacity_fade() -> PrPropertyAnimation {
    PrPropertyAnimation::Opacity(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 100.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: TICKS,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
    ])
}

/// Converts a sequence over the media of `nested_sequence` that loses nothing.
fn nested_document(sequence: &PrSequence) -> Value {
    let (_, media) = nested_sequence();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(sequence, &media, &ids, &mut omissions).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    document.to_json_value().unwrap()
}

#[test]
fn the_same_animated_property_on_two_layers_gets_distinct_keyframe_ids() {
    let scalar = |first: f64, second: f64| {
        [(0, first), (TICKS, second)]
            .map(|(source_ticks, value)| PrScalarKeyframe {
                source_ticks,
                value,
                easing: PrKeyframeEasing::Linear,
            })
            .to_vec()
    };
    let position = [(0, 0.25), (TICKS, 0.75)]
        .map(|(source_ticks, x)| PrPointKeyframe {
            source_ticks,
            value: [x, 0.5],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        })
        .to_vec();
    for (animation, names) in [
        (opacity_fade(), &["opacity"][..]),
        (
            PrPropertyAnimation::Rotation(scalar(0.0, 90.0)),
            &["rotation"],
        ),
        (
            PrPropertyAnimation::UniformScale(scalar(100.0, 150.0)),
            &["scale-x", "scale-y"],
        ),
        (
            PrPropertyAnimation::Position(position),
            &["position-x", "position-y"],
        ),
    ] {
        let keyed = |timeline| {
            let mut clip = clip_of("source", timeline, 0);
            clip.animations = vec![animation.clone()];
            clip
        };
        let sequence = sequence_of(
            "Main",
            vec![PrVideoTrack::media([
                keyed(0..5 * TICKS),
                keyed(5 * TICKS..10 * TICKS),
            ])],
        );
        // Each id names the layer that owns its track.
        let expected: Vec<_> = [1, 2]
            .into_iter()
            .flat_map(|layer| {
                names.iter().map(move |name| {
                    let id = |index| format!("premiere-{name}-{layer}-{index}");
                    json!([layer, [id(0), id(1)]])
                })
            })
            .collect();
        assert_eq!(
            keyframe_ids(&project_document(&sequence)),
            expected,
            "{animation:?}"
        );
    }
}

#[test]
fn a_keyed_clip_in_a_nest_placed_twice_gets_distinct_keyframe_ids_in_each_copy() {
    let (mut outer, _) = nested_sequence();
    // Both placements play the one inner sequence, whose first clip fades out.
    for nest in &mut outer.video_tracks[1].nests {
        nest.sequence.video_tracks[0].clip_mut(0).animations = vec![opacity_fade()];
    }
    let document = nested_document(&outer);
    let layers = &document["composition"]["layers"];
    assert_eq!(layers[0]["layers"][0]["id"], 3);
    assert_eq!(layers[1]["layers"][0]["id"], 5);
    assert_eq!(
        keyframe_ids(&document),
        [
            json!([3, ["premiere-opacity-3-0", "premiere-opacity-3-1"]]),
            json!([5, ["premiere-opacity-5-0", "premiere-opacity-5-1"]]),
        ]
    );
    // Each copy keeps the source-clock keys: the first placement starts at
    // inner 1 s, so its keys sit 1 s earlier on its clip.
    let times = |entry: &Value| {
        entry["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| key["layerTime"].as_i64().unwrap())
            .collect::<Vec<_>>()
    };
    let entries = &document["composition"]["dynamics"]["entries"];
    assert_eq!(times(&entries[0]), [-1000, 0]);
    assert_eq!(times(&entries[1]), [0, 1000]);
}

#[test]
fn a_keyed_group_child_and_a_keyed_root_layer_get_distinct_keyframe_ids() {
    let (mut outer, _) = nested_sequence();
    outer.video_tracks[0].clip_mut(0).animations = vec![opacity_fade()];
    let nests = &mut outer.video_tracks[1].nests;
    nests.truncate(1);
    nests[0].sequence.video_tracks[0].clip_mut(0).animations = vec![opacity_fade()];
    let document = nested_document(&outer);
    let layers = &document["composition"]["layers"];
    assert_eq!(layers[0]["layers"][0]["id"], 3);
    assert_eq!(layers[1]["id"], 1);
    assert_eq!(
        keyframe_ids(&document),
        [
            json!([3, ["premiere-opacity-3-0", "premiere-opacity-3-1"]]),
            json!([1, ["premiere-opacity-1-0", "premiere-opacity-1-1"]]),
        ]
    );
}

#[test]
fn mutation_errors_are_not_silently_skipped() {
    let sequence = video_sequence();
    assert!(premiere_to_tesseract(
        &sequence,
        &video_media(),
        &Default::default(),
        &mut Vec::new()
    )
    .is_err());
    let mut media = video_media();
    media
        .get_mut(&sequence.video_tracks[0].clip(0).media)
        .unwrap()
        .video
        .as_mut()
        .unwrap()
        .intrinsic_ticks = 0;
    let ids = std::collections::BTreeMap::from([(
        sequence.video_tracks[0].clip(0).media.clone(),
        AssetId::from_trusted("resolved-video"),
    )]);
    let error = premiere_to_tesseract(&sequence, &media, &ids, &mut Vec::new()).unwrap_err();
    assert!(matches!(error, BuildError::Mutation(_)));
}

#[test]
fn absolute_boundary_rounding_preserves_adjacency() {
    // The rounding table for every rate is in `convert::timing`. These rows prove
    // that the mapper rounds boundaries, not durations: 33 + 33 would leave a gap.
    for (frame_rate, boundaries) in [
        (FrameRate::Fps30, [0, 33, 67, 100]),
        (FrameRate::Fps60000Over1001, [0, 17, 33, 50]),
    ] {
        let mut sequence = video_sequence();
        sequence.frame_rate = frame_rate;
        sequence.timeline_end_ticks = 3 * frame_rate.ticks_per_frame();
        let template = sequence.video_tracks[0].clip(0).clone();
        sequence.video_tracks[0] = PrVideoTrack::media((0..3).map(|index| {
            let mut clip = template.clone();
            clip.start_ticks = index * frame_rate.ticks_per_frame();
            clip.end_ticks = (index + 1) * frame_rate.ticks_per_frame();
            clip.in_ticks = 7 * TICKS_PER_MILLISECOND;
            clip.out_ticks = clip.in_ticks + frame_rate.ticks_per_frame();
            clip
        }));
        let doc = project_document(&sequence);
        for (index, pair) in boundaries.windows(2).enumerate() {
            let layer = &doc["composition"]["layers"][index];
            assert_eq!(
                (*crate::test_support::layer_range(layer)),
                json!({"start":pair[0], "duration":pair[1]-pair[0]})
            );
            assert_eq!(
                layer["sourceRange"],
                json!({"start":7, "duration":pair[1]-pair[0]})
            );
        }
    }
}

#[test]
fn audio_placements_become_audio_layers_below_the_picture() {
    use crate::{
        format::{MediaId, PrMedia},
        schema::{AudioChannels, PrAudioOccurrence, PrAudioStream},
        tests::support::project_document_with_media,
    };
    let mut sequence = video_sequence();
    let mut media = video_media();
    media.insert(
        MediaId("music".into()),
        PrMedia {
            name: "music.mp3".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: None,
            audio: Some(PrAudioStream {
                prepared_clock: None,
                intrinsic_ticks: 8 * TICKS,
                channels: AudioChannels::Stereo,
                sample_rate: 44_100,
            }),
        },
    );
    // Sound past the last picture extends the document and its canvas.
    sequence.audio.push(PrAudioOccurrence {
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        id: None,
        media: MediaId("music".into()),
        start_ticks: TICKS,
        end_ticks: 6 * TICKS + TICKS / 1000,
        in_ticks: TICKS / 2,
        out_ticks: 5 * TICKS + TICKS / 2 + TICKS / 1000,
        volume: fx_schema::LinearGain::new(0.5).unwrap(),
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    });
    let doc = project_document_with_media(&sequence, &media);
    assert_eq!(doc["duration"], 6.001);
    let layers = doc["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0]["type"], "Video");
    let audio = &layers[1];
    assert_eq!(audio["type"], "Audio");
    assert_eq!(audio["source"]["assetId"], "premiere-video-2");
    assert_eq!(
        (*crate::test_support::layer_range(audio)),
        json!({"start": 1000, "duration": 5001})
    );
    assert_eq!(
        audio["sourceRange"],
        json!({"start": 500, "duration": 5001})
    );
    assert_eq!(audio["sourceIntrinsicDuration"], 8000);
    assert_eq!(audio["volume"], 0.5);
    assert_eq!(layers[2]["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(&layers[2]))["duration"],
        6001
    );
}

#[test]
fn keyed_clip_volume_becomes_editable_volume_keys() {
    use crate::{
        format::{MediaId, PrMedia},
        schema::{AudioChannels, PrAudioOccurrence, PrAudioStream, PrVolumeKeys},
        tests::support::project_document_with_media,
    };
    let mut sequence = video_sequence();
    let mut media = video_media();
    media.insert(
        MediaId("music".into()),
        PrMedia {
            name: "music.wav".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: None,
            audio: Some(PrAudioStream {
                prepared_clock: None,
                intrinsic_ticks: 8 * TICKS,
                channels: AudioChannels::Stereo,
                sample_rate: 48_000,
            }),
        },
    );
    // Level keys 0 dB, +12 dB, then a Hold to -6 dB, under -12 dB of other
    // stages. The source clock starts 0.5 s before the placement's In.
    let key = |ticks: i64, value: f64, easing| PrScalarKeyframe {
        source_ticks: ticks,
        value,
        easing,
    };
    sequence.audio.push(PrAudioOccurrence {
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        id: None,
        media: MediaId("music".into()),
        start_ticks: 0,
        end_ticks: 4 * TICKS,
        in_ticks: TICKS,
        out_ticks: 5 * TICKS,
        volume: fx_schema::LinearGain::new(0.25).unwrap(),
        volume_keys: Some(PrVolumeKeys {
            keys: vec![
                key(TICKS / 2, 1.0, PrKeyframeEasing::Linear),
                key(2 * TICKS, 4.0, PrKeyframeEasing::Linear),
                key(3 * TICKS, 0.5, PrKeyframeEasing::Hold),
            ],
            gain: 0.25,
        }),
        fade_in: None,
        fade_out: None,
    });
    let doc = project_document_with_media(&sequence, &media);
    let layers = doc["composition"]["layers"].as_array().unwrap();
    let audio = &layers[1];
    assert_eq!(
        (&audio["type"], &audio["volume"]),
        (&json!("Audio"), &json!(0.25))
    );
    let entries = doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["target"],
        json!({"kind": "layer", "layerId": audio["id"], "propertyType": "volume"})
    );
    // The Linear segment's curve comes from its own Level values: scaled into
    // -12..0 dB, it would take Premiere's other fader branch.
    let keys = entries[0]["animator"]["keyframes"].as_array().unwrap();
    let at = |millis: i64| keys.iter().find(|key| key["layerTime"] == millis).unwrap();
    for (millis, gain, easing) in [
        (-500, 0.25, "linear"),
        (1000, 1.0, "cubicBezier"),
        (2000, 0.125, "hold"),
    ] {
        assert_eq!(at(millis)["value"], json!({"type": "float", "value": gain}));
        assert_eq!(at(millis)["easing"]["type"], easing);
    }
    // The first subdivision is halfway through the native 0..+12 dB curve,
    // before the other stages scale its gain, not halfway through -12..0 dB.
    let expected = ((1.0 + 4_f64.powf(-0.4475)) * 0.5).powf(-1.0 / 0.4475) * 0.25;
    assert!((at(250)["value"]["value"].as_f64().unwrap() - expected).abs() < 1e-9);
    let id = audio["id"].as_u64().unwrap();
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(key["id"], format!("premiere-volume-{id}-{index}"));
    }
}

#[test]
fn audio_fades_become_eased_volume_keys_around_the_clip_level() {
    use crate::{
        format::{MediaId, PrMedia},
        schema::{
            AudioChannels, PrAudioFade, PrAudioOccurrence, PrAudioStream, PrFadeCurve, PrVolumeKeys,
        },
        tests::support::project_document_with_media,
    };
    let mut sequence = video_sequence();
    let mut media = video_media();
    media.insert(
        MediaId("music".into()),
        PrMedia {
            name: "music.wav".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: None,
            audio: Some(PrAudioStream {
                prepared_clock: None,
                intrinsic_ticks: 8 * TICKS,
                channels: AudioChannels::Stereo,
                sample_rate: 48_000,
            }),
        },
    );
    let fade = |curve, millis: i64| PrAudioFade {
        id: None,
        curve,
        duration_ticks: millis * TICKS_PER_MILLISECOND,
    };
    let key = |seconds: f64, value: f64| PrScalarKeyframe {
        source_ticks: (seconds * TICKS as f64) as i64,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    // A 0.3 s Exponential Fade in before the Level keys (0 dB at 1 s, +6 dB
    // at 1.5 s) and a 2 s Constant Power fade out after them, under -12 dB of
    // other stages.
    sequence.audio.push(PrAudioOccurrence {
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        id: None,
        media: MediaId("music".into()),
        start_ticks: 0,
        end_ticks: 4 * TICKS,
        in_ticks: TICKS,
        out_ticks: 5 * TICKS,
        volume: fx_schema::LinearGain::new(0.25).unwrap(),
        volume_keys: Some(PrVolumeKeys {
            keys: vec![key(2.0, 1.0), key(2.5, 2.0)],
            gain: 0.25,
        }),
        fade_in: Some(fade(PrFadeCurve::ExponentialFade, 300)),
        fade_out: Some(fade(PrFadeCurve::ConstantPower, 2000)),
    });
    // Touching Constant Gain fades share their full-level key.
    sequence.audio.push(PrAudioOccurrence {
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        id: None,
        media: MediaId("music".into()),
        start_ticks: 0,
        end_ticks: 2 * TICKS,
        in_ticks: 0,
        out_ticks: 2 * TICKS,
        volume: fx_schema::LinearGain::new(0.5).unwrap(),
        volume_keys: None,
        fade_in: Some(fade(PrFadeCurve::ConstantGain, 1000)),
        fade_out: Some(fade(PrFadeCurve::ConstantGain, 1000)),
    });
    let doc = project_document_with_media(&sequence, &media);
    let layers = doc["composition"]["layers"].as_array().unwrap();
    let entries = doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    let keys = |layer: &Value| {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["layerId"] == layer["id"])
            .unwrap();
        entry["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, key)| {
                assert_eq!(
                    key["id"],
                    format!("premiere-volume-{}-{index}", layer["id"])
                );
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                    key["easing"]["type"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<Vec<_>>()
    };
    let faded = keys(&layers[1]);
    // Fade-in 0-300 ms at the first key's level (0.25), with its inner key at
    // 0.441 of the span; the Level keys; the fade-out from the last key's
    // level (0.5), with its inner keys at 0.807 and 0.987 of 2 s.
    let times: Vec<_> = faded.iter().map(|key| key.0).collect();
    assert_eq!(times, [0, 132, 300, 1000, 1500, 2000, 3614, 3974, 4000]);
    let gains: Vec<_> = faded.iter().map(|key| key.1).collect();
    assert_eq!(
        [gains[0], gains[2], gains[3], gains[4], gains[5], gains[8]],
        [0.0, 0.25, 0.25, 0.5, 0.5, 0.0]
    );
    let exponential = |progress: f64| (3.5 * progress).exp_m1() / 3.5_f64.exp_m1();
    assert!((gains[1] - 0.25 * exponential(132.0 / 300.0)).abs() < 1e-12);
    let easings: Vec<_> = faded.iter().map(|key| key.2.as_str()).collect();
    assert_eq!(
        easings,
        [
            "linear",
            "cubicBezier",
            "cubicBezier",
            "linear",
            "cubicBezier",
            "linear",
            "cubicBezier",
            "cubicBezier",
            "cubicBezier"
        ]
    );
    assert_eq!(
        keys(&layers[2]),
        [
            (0, 0.0, "linear".to_owned()),
            (1000, 0.5, "linear".to_owned()),
            (2000, 0.0, "linear".to_owned())
        ]
    );
}

#[test]
fn a_level_key_at_a_fade_edge_shares_the_fade_key_off_the_millisecond_grid() {
    use crate::{
        format::{MediaId, PrMedia},
        schema::{
            AudioChannels, PrAudioFade, PrAudioOccurrence, PrAudioStream, PrFadeCurve, PrVolumeKeys,
        },
        tests::support::project_document_with_media,
    };
    let mut sequence = video_sequence();
    let mut media = video_media();
    media.insert(
        MediaId("music".into()),
        PrMedia {
            name: "music.wav".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: None,
            audio: Some(PrAudioStream {
                prepared_clock: None,
                intrinsic_ticks: 8 * TICKS,
                channels: AudioChannels::Stereo,
                sample_rate: 48_000,
            }),
        },
    );
    // A placement one 30 fps frame into the sequence, with a 31-frame
    // Constant Power fade-in. Its end, at 1066.67 ms, rounds to layer 1034 ms
    // as a clip boundary but to 1033 ms relative to the In point, where the
    // Level key at the fade's edge goes; the Level then falls to 0.5.
    let frame = TICKS / 30;
    let edge = 31 * frame;
    let key = |source_ticks: i64, value: f64| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    sequence.audio.push(PrAudioOccurrence {
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        id: None,
        media: MediaId("music".into()),
        start_ticks: frame,
        end_ticks: frame + 2 * TICKS,
        in_ticks: 0,
        out_ticks: 2 * TICKS,
        volume: fx_schema::LinearGain::new(1.0).unwrap(),
        volume_keys: Some(PrVolumeKeys {
            keys: vec![key(edge, 1.0), key(9 * TICKS / 5, 0.5)],
            gain: 1.0,
        }),
        fade_in: Some(PrAudioFade {
            id: None,
            curve: PrFadeCurve::ConstantPower,
            duration_ticks: edge,
        }),
        fade_out: None,
    });
    let doc = project_document_with_media(&sequence, &media);
    let entries = doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let keys: Vec<_> = entries[0]["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
                key["easing"]["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    // The Level key joins the fade's full-level key at 1034 ms, which keeps
    // the fade's own last easing; the Level falls from there.
    let times: Vec<_> = keys.iter().map(|key| key.0).collect();
    assert_eq!(times, [0, 13, 200, 1034, 1800]);
    let power = |progress: f64| {
        (std::f64::consts::FRAC_PI_2 * progress.powf(0.6457))
            .sin()
            .powi(2)
    };
    for index in [1, 2] {
        let progress = keys[index].0 as f64 / 1034.0;
        assert!((keys[index].1 - power(progress)).abs() < 1e-12, "{keys:?}");
    }
    assert_eq!(keys[3], (1034, 1.0, "cubicBezier".to_owned()));
    assert_eq!(keys[4], (1800, 0.5, "cubicBezier".to_owned()));
}

#[test]
fn graphic_text_becomes_an_editable_text_layer_above_lower_tracks() {
    let mut sequence = video_sequence();
    sequence.video_tracks.push(PrVideoTrack {
        items: vec![PrVideoItem::Graphic(text_graphic())],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let doc = project_document(&sequence);
    let layers = doc["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .map(|layer| &layer["type"])
            .collect::<Vec<_>>(),
        ["Text", "Video", "Rect"]
    );
    let text = &layers[0];
    assert_eq!(text["name"], "Title");
    assert_eq!(
        (*crate::test_support::layer_range(text)),
        json!({"start": 1000, "duration": 2000})
    );
    let transform = &text["transform"];
    assert_eq!(transform["position"], json!([480.0, 540.0]));
    assert_eq!(transform["anchorPoint"], json!([96.0, 54.0]));
    assert_eq!(transform["scale"], json!([80.0, 80.0]));
    assert_eq!(
        (
            transform["rotation"].as_f64(),
            transform["opacity"].as_f64()
        ),
        (Some(-15.0), Some(75.0))
    );
    let source = &text["sourceText"];
    for (field, expected) in [
        ("text", json!("Line one\nLine two")),
        // The FX renderer resolves the Premiere PostScript name exactly.
        ("fontFamily", json!("OpenSans-Bold")),
        ("fontStyle", json!("")),
        ("fontSize", json!(80.0)),
        ("applyFill", json!(true)),
        ("fillColor", json!([1.0, 0.0, 0.0, 1.0])),
        ("applyStroke", json!(true)),
        ("strokeColor", json!([0.0, 0.0, 1.0, 1.0])),
        // Premiere's 5 px outside stroke is a 10 px centered stroke under the fill.
        ("strokeWidth", json!(10.0)),
        ("strokeOverFill", json!(false)),
        ("justification", json!("center")),
        ("tracking", json!(-20.0)),
        // Premiere adds leading to 1.2 x 80 px.
        ("leading", json!(146.0)),
        ("boxText", json!(true)),
        ("boxSize", json!([800.0, 400.0])),
        ("boxPosition", json!([0.0, 0.0])),
        ("verticalAlign", json!("center")),
        ("allCaps", json!(true)),
    ] {
        assert_eq!(source[field], expected, "{field}");
    }
    assert!(source.get("boxFirstBaseline").is_none());
}

#[test]
fn every_imported_font_is_reported_once_as_unpackaged() {
    // Graphics that differ only in their font, name and one-second slot.
    let text = |id: &str, name: &str, font: &str, second: i64| {
        let mut graphic = text_graphic();
        graphic.id = Some(id.into());
        (graphic.start_ticks, graphic.end_ticks) = (second * TICKS, (second + 1) * TICKS);
        graphic.text_mut().name = name.into();
        graphic.text_mut().document.font = font.into();
        PrVideoItem::Graphic(graphic)
    };
    let mut sequence = video_sequence();
    sequence.video_tracks.push(PrVideoTrack {
        items: vec![
            text("20", "First", "Arial-BoldMT", 0),
            text("21", "Second", "Georgia-Bold", 1),
            text("22", "Third", "Arial-BoldMT", 2),
        ],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &sequence,
        &video_media(),
        &crate::tesseract_output::asset_ids_in_order(&sequence, &video_media()),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    // Each layer stores Premiere's PostScript name verbatim with an empty style.
    let fonts: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Text")
        .map(|layer| {
            let source = &layer["sourceText"];
            (&layer["name"], &source["fontFamily"], &source["fontStyle"])
        })
        .collect();
    assert_eq!(
        fonts,
        [
            (&json!("First"), &json!("Arial-BoldMT"), &json!("")),
            (&json!("Second"), &json!("Georgia-Bold"), &json!("")),
            (&json!("Third"), &json!("Arial-BoldMT"), &json!("")),
        ]
    );
    // One warning per font, naming the first layer and counting the others.
    let unpackaged = |record: &str, font: &str| crate::Omission {
        scope: crate::OmissionScope::Feature,
        kind: crate::OmissionKind::Omitted,
        record: record.to_owned(),
        reason: format!(
            "font \"{font}\" is not packaged in this document; import it with tsrct project \
             import-font before preview or export."
        ),
    };
    assert_eq!(
        omissions,
        [
            unpackaged(r#"20 ("First") and 1 more text layer"#, "Arial-BoldMT"),
            unpackaged(r#"21 ("Second")"#, "Georgia-Bold"),
        ]
    );
}

/// Parses one sequence of a pinned Adobe-derived fixture into its model.
fn native_fixture(name: &str, sequence: &str) -> (PrSequence, BTreeMap<MediaId, PrMedia>) {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let mut xml = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    let (mut sequences, media) = inspect_project_with_media(&xml, Some(sequence))
        .unwrap()
        .into_parts();
    (sequences.remove(0), media)
}

fn import(
    sequence: &PrSequence,
    media: &BTreeMap<MediaId, PrMedia>,
) -> (Value, Vec<crate::Omission>) {
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(sequence, media, &ids, &mut omissions)
        .expect("unsupported mask or key semantics must not abort the import");
    (document.to_json_value().unwrap(), omissions)
}

fn layer_types(wire: &Value) -> Vec<&str> {
    wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect()
}

fn animated_properties(wire: &Value) -> Vec<&str> {
    wire["composition"]["dynamics"]["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| entry["target"]["propertyType"].as_str().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

/// Linear scalar keys at `(source_ticks, value)`.
fn linear_keys(keys: &[(i64, f64)]) -> Vec<PrScalarKeyframe> {
    keys.iter()
        .map(|&(source_ticks, value)| PrScalarKeyframe {
            source_ticks,
            value,
            easing: PrKeyframeEasing::Linear,
        })
        .collect()
}

/// A 270° unfeathered Linear Wipe from 100 with `completion` keys.
fn wipe(completion: &[(i64, f64)]) -> PrLinearWipe {
    PrLinearWipe {
        initial_completion: 100.0,
        completion: linear_keys(completion),
        angle_degrees: 270,
        feather: 0.0,
    }
}

#[test]
fn crop_guide_follows_the_adobe_position_path_of_its_video() {
    // The pinned Adobe-authored Position path, edited with a standalone Crop.
    let (mut sequence, media) = native_fixture(
        "feature_motion_position_path_strict.prproj",
        "c8acf9c1-34b2-4086-9f55-d528950a7059",
    );
    let clip = sequence.video_tracks[0].clip_mut(0);
    assert_eq!(
        clip.animations[0].property(),
        crate::schema::PrAnimatedProperty::Position
    );
    clip.crop = left_crop();
    let (wire, omissions) = import(&sequence, &media);
    assert!(omissions.is_empty(), "{omissions:?}");
    // A flat Crop: the guide shares the video's transform and Position keys.
    assert_eq!(layer_types(&wire), ["Video", "Rect", "Rect"], "Crop guide");
    let layers = wire["composition"]["layers"].as_array().unwrap();
    let (video, guide) = (&layers[0], &layers[1]);
    assert_eq!(video["masks"][0]["layer"], guide["id"]);
    assert_eq!(guide["transform"], video["transform"]);
    let keys = |layer: &Value| {
        wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["target"]["layerId"] == layer["id"])
            .map(|entry| {
                let keys: Vec<_> = entry["animator"]["keyframes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|key| (key["layerTime"].clone(), key["value"].clone()))
                    .collect();
                (entry["target"]["propertyType"].clone(), keys)
            })
            .collect::<Vec<_>>()
    };
    let video_keys = keys(video);
    assert_eq!(
        video_keys
            .iter()
            .map(|(property, _)| property)
            .collect::<Vec<_>>(),
        ["positionX", "positionY"]
    );
    assert_eq!(keys(guide), video_keys);
}

#[test]
fn an_unconvertible_linear_wipe_omits_its_occurrence() {
    // The pinned Adobe-authored full-frame wipe keeps its guide unedited.
    let fixture = || {
        native_fixture(
            "feature_linear_wipe_strict.prproj",
            "49b892d1-dfc3-4be2-a83a-93789626be7c",
        )
    };
    let (sequence, media) = fixture();
    let (wire, omissions) = import(&sequence, &media);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(layer_types(&wire), ["Video", "Rect", "Video", "Rect"]);

    type Edit = fn(&mut crate::schema::PrVideoOccurrence, &mut BTreeMap<MediaId, PrMedia>);
    let edited = |edit: Edit| {
        let (mut sequence, mut media) = fixture();
        let clip = sequence
            .video_tracks
            .iter_mut()
            .flat_map(|track| track.items.iter_mut())
            .find_map(|item| match item {
                PrVideoItem::Media(clip) if clip.linear_wipe.is_some() => Some(clip),
                _ => None,
            })
            .unwrap();
        edit(clip, &mut media);
        let record = clip
            .id
            .clone()
            .unwrap_or_else(|| clip.media.as_str().into());
        (import(&sequence, &media), record)
    };
    let edits: [(Edit, &str); 3] = [
        (
            |clip, media| {
                media
                    .get_mut(&clip.media)
                    .unwrap()
                    .video
                    .as_mut()
                    .unwrap()
                    .width = 1280;
            },
            "Linear Wipe on media that is not sequence-sized is not converted; occurrence omitted",
        ),
        (
            |clip, _| clip.playback_rate = 2.0,
            "Linear Wipe was not imported: unsupported conversion: keys on a retimed, reversed or time-remapped clip",
        ),
        (
            // Distinct native key times that collide after millisecond rounding.
            |clip, _| {
                let source_in = clip.in_ticks;
                clip.linear_wipe.as_mut().unwrap().completion =
                    linear_keys(&[(source_in, 100.0), (source_in + TICKS / 10_000, 0.0)]);
            },
            "Linear Wipe was not imported: unsupported conversion: Premiere Linear Wipe keyframe times/values cannot be imported",
        ),
    ];
    for (edit, reason) in edits {
        let ((wire, omissions), record) = edited(edit);
        // The wiped clip is not imported opaque; the other clip still is.
        assert_eq!(layer_types(&wire), ["Video", "Rect"], "{reason}");
        assert_eq!(omissions.len(), 1, "{reason}: {omissions:?}");
        assert_eq!(omissions[0].scope, crate::OmissionScope::Occurrence);
        assert_eq!(omissions[0].record, record);
        assert!(omissions[0].reason.contains(reason), "{omissions:?}");
    }

    // Static or keyed Motion that moves the clip frame stages the wipe: the
    // group carries the Motion and the mask; the video and its guide keep the
    // canvas frame.
    let moves: [Edit; 2] = [
        |clip, _| clip.transform.scale = [50.0, 50.0],
        |clip, _| {
            clip.animations
                .push(PrPropertyAnimation::Rotation(linear_keys(&[(
                    clip.in_ticks,
                    0.0,
                )])))
        },
    ];
    for (case, edit) in moves.into_iter().enumerate() {
        let ((wire, omissions), _) = edited(edit);
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        assert_eq!(layer_types(&wire), ["Group", "Video", "Rect"], "{case}");
        let layers = wire["composition"]["layers"].as_array().unwrap();
        let (group, canvas) = (&layers[0], &layers[2]);
        let [video, guide] = group["layers"].as_array().unwrap().as_slice() else {
            panic!("{case}: expected the video and its wipe guide");
        };
        assert_eq!(guide["name"], "Premiere Linear Wipe guide 2", "{case}");
        assert_eq!(group["masks"][0]["layer"], guide["id"], "{case}");
        assert!(video.get("masks").is_none(), "{case}");
        assert_eq!(video["transform"], canvas["transform"], "{case}: identity");
        let group_properties: Vec<_> = wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["target"]["layerId"] == group["id"])
            .map(|entry| entry["target"]["propertyType"].as_str().unwrap())
            .collect();
        match case {
            0 => {
                assert_eq!(group["transform"]["scale"], json!([50.0, 50.0]));
                assert!(group_properties.is_empty());
            }
            _ => assert_eq!(group_properties, ["rotation"]),
        }
    }
}

#[test]
fn nonunit_playback_omits_source_clock_keys_and_keeps_the_clip() {
    // The pinned Adobe-authored reverse clip at -0.905x, edited with keys and
    // a Crop. The keys are omitted on the retimed clock, so the Crop guide
    // follows the static Motion that remains.
    let (mut sequence, media) = native_fixture(
        "feature_constant_reverse_0_905_strict.prproj",
        "c8acf9c1-34b2-4086-9f55-d528950a7059",
    );
    let clip = sequence.video_tracks[0].clip_mut(0);
    let source_in = clip.in_ticks;
    clip.opacity = 80.0;
    clip.crop = left_crop();
    clip.animations = vec![
        PrPropertyAnimation::Opacity(linear_keys(&[(source_in, 80.0), (source_in + TICKS, 0.0)])),
        PrPropertyAnimation::Rotation(linear_keys(&[(source_in, 0.0)])),
    ];
    let (wire, omissions) = import(&sequence, &media);
    assert_eq!(layer_types(&wire), ["Video", "Rect", "Rect"], "Crop guide");
    assert!(animated_properties(&wire).is_empty());
    let layers = wire["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers[0]["transform"]["opacity"], 80.0);
    assert_eq!(
        crate::tests::support::playback_keys(&layers[0])[0]["value"],
        7000
    );
    assert_eq!(layers[0]["masks"][0]["layer"], layers[1]["id"]);
    assert_eq!(layers[1]["transform"], layers[0]["transform"]);
    assert_eq!(omissions.len(), 2, "{omissions:?}");
    for (omission, feature) in omissions.iter().zip(["Opacity", "Rotation"]) {
        assert_eq!(omission.scope, crate::OmissionScope::Feature);
        assert!(
            omission.reason.starts_with(&format!(
                "{feature} animation was not imported: keys on a retimed, reversed or time-remapped clip"
            )),
            "{omissions:?}"
        );
    }

    // The same keys at unit speed still import.
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.playback_rate = 1.0;
    clip.in_ticks = 0;
    clip.out_ticks = clip.end_ticks - clip.start_ticks;
    clip.crop = PrStaticCrop::default();
    for animation in &mut clip.animations {
        match animation {
            PrPropertyAnimation::Opacity(keys) | PrPropertyAnimation::Rotation(keys) => {
                for (index, key) in keys.iter_mut().enumerate() {
                    key.source_ticks = index as i64 * TICKS;
                }
            }
            _ => unreachable!(),
        }
    }
    let (wire, omissions) = import(&sequence, &media);
    assert_eq!(animated_properties(&wire), ["opacity", "rotation"]);
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[test]
fn unconvertible_time_remap_omits_only_its_occurrence() {
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).end_ticks = 2 * TICKS;
    sequence.video_tracks[0].clip_mut(0).out_ticks = 2 * TICKS;
    let mut remapped = sequence.video_tracks[0].clip(0).clone();
    remapped.id = Some("remapped".into());
    remapped.start_ticks = 2 * TICKS;
    remapped.end_ticks = 5 * TICKS;
    remapped.out_ticks = 3 * TICKS;
    // Distinct native keys that collide after millisecond rounding.
    remapped.time_remap = Some(crate::schema::PrTimeRemap {
        keys: [0, TICKS / 10_000, TICKS]
            .into_iter()
            .map(|ticks| crate::schema::PrTimeRemapKeyframe {
                timeline_ticks: ticks,
                source_ticks: ticks,
                easing: PrKeyframeEasing::Linear,
            })
            .collect(),
    });
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(remapped));
    let (wire, omissions) = import(&sequence, &video_media());
    assert_eq!(layer_types(&wire), ["Video", "Rect"]);
    assert_eq!(
        (*crate::test_support::layer_range(&wire["composition"]["layers"][0])),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, crate::OmissionScope::Occurrence);
    assert_eq!(omissions[0].record, "remapped");
    assert!(omissions[0]
        .reason
        .contains("TimeRemapping cannot be imported"));
}

#[test]
fn unimportable_motion_keys_leave_static_crop_and_wipe_guides() {
    // Distinct native key times that collide after millisecond rounding cannot
    // be imported, so the clip keeps static Motion, which both guides follow.
    let times = [0, TICKS / 10_000];
    let scalar = |value| linear_keys(&times.map(|ticks| (ticks, value)));
    let point = times
        .map(|source_ticks| PrPointKeyframe {
            source_ticks,
            value: [0.5, 0.5],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        })
        .to_vec();
    let keyed_wipe = wipe(&[(0, 100.0), (TICKS, 0.0)]);
    for animation in [
        PrPropertyAnimation::Position(point),
        PrPropertyAnimation::Rotation(scalar(0.0)),
        PrPropertyAnimation::UniformScale(scalar(100.0)),
    ] {
        let property = animation.property();
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.animations = vec![animation];
        clip.crop = left_crop();
        let (wire, omissions) = import(&sequence, &video_media());
        assert_eq!(
            layer_types(&wire),
            ["Video", "Rect", "Rect"],
            "{property:?}: {omissions:?}"
        );
        let layers = wire["composition"]["layers"].as_array().unwrap();
        assert_eq!(layers[0]["masks"][0]["layer"], layers[1]["id"]);
        assert!(animated_properties(&wire).is_empty());
        assert!(!omissions.is_empty());
        assert!(
            omissions
                .iter()
                .all(|item| item.reason.contains("animation was not imported")),
            "{property:?}: {omissions:?}"
        );

        // Native Motion keys stage a wipe, whose group keeps static Motion.
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.crop = PrStaticCrop::default();
        clip.linear_wipe = Some(keyed_wipe.clone());
        let (wire, omissions) = import(&sequence, &video_media());
        assert_eq!(
            layer_types(&wire),
            ["Group", "Rect"],
            "{property:?}: {omissions:?}"
        );
        let group = &wire["composition"]["layers"][0];
        let [_, guide] = group["layers"].as_array().unwrap().as_slice() else {
            panic!("{property:?}: expected the video and its wipe guide");
        };
        assert_eq!(group["masks"][0]["layer"], guide["id"], "{property:?}");
        assert_eq!(animated_properties(&wire), ["scaleX"], "{property:?}");
        assert!(
            omissions
                .iter()
                .all(|item| item.reason.contains("animation was not imported")),
            "{property:?}: {omissions:?}"
        );
    }
}

/// A 30 fps sequence whose 0-5 s clip on track 0 is keyed by `channel` from
/// the matte track 1, which holds `matte`; `edit` changes the keyed clip.
fn keyed_sequence(
    channel: crate::schema::PrMatteChannel,
    matte: PrVideoTrack,
    edit: impl FnOnce(&mut crate::schema::PrVideoOccurrence),
) -> PrSequence {
    let mut fill = clip_of("source", 0..5 * TICKS, 0);
    fill.track_matte = Some(crate::schema::PrTrackMatte {
        track_index: 1,
        channel,
    });
    edit(&mut fill);
    sequence_of("Main", vec![PrVideoTrack::media([fill]), matte])
}

#[test]
fn track_matte_keys_import_flat_or_staged_with_the_matte_under_the_stage() {
    use crate::schema::PrMatteChannel;
    let matte_track = || PrVideoTrack::media([clip_of("source", 0..5 * TICKS, 0)]);
    // Flat: the video keeps its matte beside it (`horror_title`: Invert after
    // the key stays on the video, and Opacity keys stay too).
    let sequence = keyed_sequence(PrMatteChannel::Alpha, matte_track(), |clip| {
        clip.effects = vec![blur(40.0)];
        clip.animations = vec![opacity_fade()];
    });
    let (flat, omissions) = import(&sequence, &video_media());
    assert!(omissions.is_empty(), "{omissions:?}");
    let [matte, video, canvas] = flat["composition"]["layers"].as_array().unwrap().as_slice()
    else {
        panic!("expected the matte, the keyed video and the canvas");
    };
    assert_eq!(
        video["trackMatte"],
        json!({"mode": "alpha", "layer": matte["id"]})
    );
    assert_eq!(video["effects"].as_array().unwrap().len(), 1);
    assert!(video.get("masks").is_none() && matte.get("trackMatte").is_none());
    assert_eq!(matte["type"], "Video");
    assert_eq!(
        (*crate::test_support::layer_range(matte)),
        (*crate::test_support::layer_range(video))
    );
    assert_eq!(canvas["type"], "Rect");
    // Reverse with Matte Alpha (fixture G3a) is FX `alphaInverted`.
    let (inverted, omissions) = import(
        &keyed_sequence(PrMatteChannel::AlphaInverted, matte_track(), |_| {}),
        &video_media(),
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        inverted["composition"]["layers"][1]["trackMatte"]["mode"],
        "alphaInverted"
    );
    // Staged: a moved clip's Motion moves the matte with it (fixture G5), as a
    // blur applied before the key does (G7), so the group takes the matte as
    // its child on the group clock; the matte's own Motion stays its own.
    for (case, edit) in [
        (
            "Scale 50",
            Box::new(|clip: &mut crate::schema::PrVideoOccurrence| {
                clip.transform.scale = [50.0; 2];
            }) as Box<dyn Fn(&mut crate::schema::PrVideoOccurrence)>,
        ),
        (
            "blur before the key",
            Box::new(|clip: &mut crate::schema::PrVideoOccurrence| {
                clip.effects = vec![blur(40.0)];
                clip.effects_above_mask = 1;
            }),
        ),
    ] {
        let mut matte_track = matte_track();
        matte_track.clip_mut(0).transform.rotation = 15.0;
        let sequence = keyed_sequence(PrMatteChannel::Luma, matte_track, edit);
        let (staged, omissions) = import(&sequence, &video_media());
        // Matte Luma is reported once as an approximation (fixture G2).
        let reported: Vec<_> = omissions
            .iter()
            .map(|omission| {
                (
                    omission.scope,
                    omission.kind,
                    omission.record.as_str(),
                    omission.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(
            reported,
            [(
                crate::OmissionScope::Feature,
                crate::OmissionKind::Approximated,
                "source",
                "Matte Luma is approximated: Premiere weights the matte's encoded RGB by Rec. 601 (measured), FX by Rec. 709; exact for a neutral matte, up to about 11 levels apart on a saturated colour matte"
            )],
            "{case}"
        );
        let [group, canvas] = staged["composition"]["layers"]
            .as_array()
            .unwrap()
            .as_slice()
        else {
            panic!("{case}: expected the stage group and the canvas");
        };
        assert_eq!(group["name"], "Premiere stage 1", "{case}");
        let [video, matte] = group["layers"].as_array().unwrap().as_slice() else {
            panic!("{case}: expected the video and its matte under the group");
        };
        assert_eq!(
            group["trackMatte"],
            json!({"mode": "luma", "layer": matte["id"]}),
            "{case}"
        );
        assert!(video.get("trackMatte").is_none(), "{case}");
        for layer in [video, matte] {
            assert_eq!(layer["parent"], group["id"], "{case}");
            assert_eq!(
                (*crate::test_support::layer_range(layer)),
                json!({"start": 0, "duration": 5000}),
                "{case}"
            );
        }
        assert_eq!(video["transform"], canvas["transform"], "{case}: identity");
        assert_eq!(matte["transform"]["rotation"], 15.0, "{case}");
        assert_eq!(matte["type"], "Video", "{case}");
    }
    // Two clips keyed by one matte, alpha and luma (`corporate_slideshow`
    // 1175/1176 on 1203), name the same layer.
    let mut pair = keyed_sequence(PrMatteChannel::Alpha, matte_track(), |_| {});
    let mut luma = clip_of("source", 0..5 * TICKS, 0);
    luma.track_matte = Some(crate::schema::PrTrackMatte {
        track_index: 2,
        channel: PrMatteChannel::Luma,
    });
    pair.video_tracks.insert(1, PrVideoTrack::media([luma]));
    pair.video_tracks[0]
        .clip_mut(0)
        .track_matte
        .as_mut()
        .unwrap()
        .track_index = 2;
    let (document, omissions) = import(&pair, &video_media());
    assert_eq!(
        omissions
            .iter()
            .map(|omission| omission.scope)
            .collect::<Vec<_>>(),
        [crate::OmissionScope::Feature],
        "the luma clip's approximation: {omissions:?}"
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers[1]["trackMatte"],
        json!({"mode": "luma", "layer": layers[0]["id"]})
    );
    assert_eq!(
        layers[2]["trackMatte"],
        json!({"mode": "alpha", "layer": layers[0]["id"]})
    );
    // A shared matte stays their sibling: a clip that would stage it is omitted.
    pair.video_tracks[0].clip_mut(0).transform.scale = [50.0; 2];
    let (document, omissions) = import(&pair, &video_media());
    assert_eq!(layer_types(&document), ["Video", "Video", "Rect"]);
    assert!(
        omissions.iter().any(|omission| omission.scope == crate::OmissionScope::Occurrence
            && omission.reason
                == "track 0, range 0..1270080000000 ticks: a Track Matte Key whose matte keys another clip too is not converted on a clip that its Motion or effects stage; occurrence omitted"),
        "{omissions:?}"
    );
}

#[test]
fn source_effects_stage_a_track_matte_key_unless_its_matte_is_shared() {
    use crate::schema::{PrMatteChannel, PrSourceEffects, PrTrackMatte};
    // Supplementary: the master clip of the keyed clip owns a blur.
    let master = "MasterClip:master-1";
    let source = || {
        Some(PrSourceEffects {
            master: master.to_owned(),
            effects: vec![blur(40.0)],
            active_transforms: 0,
        })
    };
    let matte_track = || PrVideoTrack::media([clip_of("source", 0..5 * TICKS, 0)]);
    let reports = |omissions: &[crate::Omission]| -> Vec<(String, String)> {
        omissions
            .iter()
            .map(|omission| (omission.record.clone(), omission.reason.clone()))
            .collect()
    };
    // An unmoved keyed clip without effects keys flat. Its source effects
    // apply before the key, so it stages as for a blur before the key (G7):
    // the group takes the matte under it, and the video the source blur.
    let sequence = keyed_sequence(PrMatteChannel::Alpha, matte_track(), |clip| {
        clip.source_effects = source();
    });
    let (staged, omissions) = import(&sequence, &video_media());
    assert_eq!(layer_types(&staged), ["Group", "Rect"]);
    let group = &staged["composition"]["layers"][0];
    let [video, matte] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the video and its matte under the group");
    };
    assert_eq!(
        group["trackMatte"],
        json!({"mode": "alpha", "layer": matte["id"]})
    );
    assert!(video.get("trackMatte").is_none());
    assert_eq!(video["effects"][0]["effect"]["type"], "gaussianBlur");
    assert_eq!(
        reports(&omissions),
        [(
            master.to_owned(),
            super::effects::LINKED_SOURCE_EDITING_REASON.to_owned()
        )]
    );
    // A matte that another clip keys too stays the sibling of both, where no
    // stage group can take it: the clip converts flat, as without its source
    // effects, which are reported.
    let mut pair = keyed_sequence(PrMatteChannel::Alpha, matte_track(), |clip| {
        clip.source_effects = source();
    });
    let mut other = clip_of("source", 0..5 * TICKS, 0);
    other.track_matte = Some(PrTrackMatte {
        track_index: 2,
        channel: PrMatteChannel::Alpha,
    });
    pair.video_tracks.insert(1, PrVideoTrack::media([other]));
    pair.video_tracks[0]
        .clip_mut(0)
        .track_matte
        .as_mut()
        .unwrap()
        .track_index = 2;
    let (flat, omissions) = import(&pair, &video_media());
    assert_eq!(layer_types(&flat), ["Video", "Video", "Video", "Rect"]);
    let layers = flat["composition"]["layers"].as_array().unwrap();
    for fill in &layers[1..3] {
        assert_eq!(
            fill["trackMatte"],
            json!({"mode": "alpha", "layer": layers[0]["id"]})
        );
        assert!(fill.get("effects").is_none(), "{fill}");
    }
    assert_eq!(
        reports(&omissions),
        [
            (
                "source".to_owned(),
                "source effects of MasterClip:master-1 were not imported: a Track Matte Key whose matte keys another clip too is not converted on a clip that its Motion or effects stage".to_owned()
            ),
            (
                master.to_owned(),
                crate::schema::SOURCE_CHAIN_NOT_CONVERTED.to_owned()
            ),
        ]
    );
}

#[test]
fn source_effects_on_a_clip_keyed_by_a_retimed_matte_convert_without_them() {
    use crate::schema::{PrMatteChannel, PrSourceEffects};
    // Supplementary: the keyed clip's master clip owns a blur, which would
    // stage it; its matte plays at 2x. A stage group's child reads the group
    // clock, where the matte's playback keys, on the sequence clock, would
    // be late, so the clip keys flat without its source effects, as it does
    // without any, and they are reported.
    let mut matte = clip_of("source", 0..5 * TICKS, 0);
    matte.out_ticks = 10 * TICKS;
    matte.playback_rate = 2.0;
    let sequence = keyed_sequence(
        PrMatteChannel::Alpha,
        PrVideoTrack::media([matte]),
        |clip| {
            clip.source_effects = Some(PrSourceEffects {
                master: "MasterClip:master-1".to_owned(),
                effects: vec![blur(40.0)],
                active_transforms: 0,
            });
        },
    );
    let (flat, omissions) = import(&sequence, &video_media());
    assert_eq!(layer_types(&flat), ["Video", "Video", "Rect"]);
    let [matte, video, _] = flat["composition"]["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the matte, the keyed video and the canvas");
    };
    assert!(matte.get("playback").is_some(), "{matte}");
    assert_eq!(
        video["trackMatte"],
        json!({"mode": "alpha", "layer": matte["id"]})
    );
    assert!(video.get("effects").is_none(), "{video}");
    let reports: Vec<_> = omissions
        .iter()
        .map(|omission| (omission.record.as_str(), omission.reason.clone()))
        .collect();
    assert_eq!(
        reports,
        [
            (
                "source",
                format!(
                    "source effects of MasterClip:master-1 were not imported: {}",
                    super::STAGED_RETIMED_MATTE_REASON
                )
            ),
            (
                "MasterClip:master-1",
                crate::schema::SOURCE_CHAIN_NOT_CONVERTED.to_owned()
            ),
        ]
    );
}

#[test]
fn a_retimed_matte_clip_does_not_move_under_a_delayed_stage_group() {
    use crate::schema::{PrMatteChannel, PrTimeRemap, PrTimeRemapKeyframe, PrTrackMatte};
    // The keyed clip at Scale 50 over 5-10 s stages; its matte over the same
    // range moves under the group, whose clock starts at 5 s.
    let staged = |matte: crate::schema::PrVideoOccurrence| {
        let mut fill = clip_of("source", 5 * TICKS..10 * TICKS, 0);
        fill.track_matte = Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        });
        fill.transform.scale = [50.0; 2];
        sequence_of(
            "Main",
            vec![PrVideoTrack::media([fill]), PrVideoTrack::media([matte])],
        )
    };
    // At unit speed the child plays the same source times: source 2 s at
    // sequence 5 s is source 2 s at group time 0.
    let (document, omissions) = import(
        &staged(clip_of("source", 5 * TICKS..10 * TICKS, 2 * TICKS)),
        &video_media(),
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    assert_eq!(
        (*crate::test_support::layer_range(group)),
        json!({"start": 5000, "duration": 5000})
    );
    let matte = &group["layers"][1];
    assert_eq!(group["trackMatte"]["layer"], matte["id"]);
    assert_eq!(
        (*crate::test_support::layer_range(matte)),
        json!({"start": 0, "duration": 5000})
    );
    assert_eq!(
        matte["sourceRange"],
        json!({"start": 2000, "duration": 5000})
    );
    assert_eq!(
        matte["playback"],
        json!({"type":"windowed", "inputRange":{"start":0,"duration":5000}, "mapping":{"type":"linear", "input":{"start":5000,"duration":5000}, "output":{"start":2000,"duration":5000}}, "inputOffsetMs":5000})
    );
    // Playback keys are on the sequence clock (5-10 s here) and would read
    // the group clock (0-5 s) under the group: the clip is omitted and the
    // matte clip stays a root layer.
    let mut forward = clip_of("source", 5 * TICKS..10 * TICKS, 0);
    forward.out_ticks = 10 * TICKS;
    forward.playback_rate = 2.0;
    let mut reverse = clip_of("source", 5 * TICKS..10 * TICKS, 0);
    reverse.playback_rate = -1.0;
    let mut remapped = clip_of("source", 5 * TICKS..10 * TICKS, 0);
    remapped.time_remap = Some(PrTimeRemap {
        keys: vec![
            PrTimeRemapKeyframe {
                timeline_ticks: 0,
                source_ticks: 0,
                easing: PrKeyframeEasing::Linear,
            },
            PrTimeRemapKeyframe {
                timeline_ticks: 5 * TICKS,
                source_ticks: 5 * TICKS / 2,
                easing: PrKeyframeEasing::Linear,
            },
        ],
    });
    for (case, matte) in [
        ("2x forward", forward),
        ("reverse", reverse),
        ("Time Remapping", remapped),
    ] {
        // The omitted clip's matte is not content either (fixture G1b).
        let (document, omissions) = import(&staged(matte), &video_media());
        assert_eq!(layer_types(&document), ["Rect"], "{case}");
        let reasons: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.scope == crate::OmissionScope::Occurrence)
            .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [
                ("source", "track 0, range 1270080000000..2540160000000 ticks: a Track Matte Key whose matte clip plays at another speed or is time-remapped is not converted on a clip that its Motion or effects stage; the matte's playback keys are on the sequence clock and a stage group's child reads the group clock; occurrence omitted"),
                ("source", "matte source of the omitted clip source was not converted: Premiere does not draw a track-matte source"),
            ],
            "{case}"
        );
    }
}

fn animated_nested_matte_sequence() -> (PrSequence, Vec<PrPointKeyframe>) {
    use crate::schema::{PrMatteChannel, PrTrackMatte};
    let keys = native_straight_position_keys();
    let mut inner = video_sequence();
    (inner.width, inner.height) = (960, 540);
    inner.name = "Matte picture".into();
    let mut matte = crate::tests::support::nest_of(inner, TICKS..4 * TICKS, TICKS / 2);
    matte.id = Some("nested-matte".into());
    matte.transform.position = keys[0].value;
    matte.animations = vec![PrPropertyAnimation::Position(keys.clone())];
    let mut consumer = clip_of("source", TICKS..4 * TICKS, 0);
    consumer.id = Some("matte-consumer".into());
    consumer.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    let sequence = sequence_of(
        "Outer",
        vec![
            PrVideoTrack::media([consumer]),
            PrVideoTrack {
                items: Vec::new(),
                nests: vec![matte],
                transitions: Vec::new(),
            },
        ],
    );
    (sequence, keys)
}

#[test]
fn nested_matte_motion_keeps_source_guide_children_and_native_position_keys() {
    let (sequence, native_keys) = animated_nested_matte_sequence();
    sequence.validate_timeline(&video_media()).unwrap();
    let (document, omissions) = import(&sequence, &video_media());
    assert!(
        omissions
            .iter()
            .all(|item| item.scope != crate::OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    let matte = layers
        .iter()
        .find(|layer| layer["name"] == "Matte picture")
        .unwrap();
    let consumer = layers
        .iter()
        .find(|layer| layer["trackMatte"]["layer"] == matte["id"])
        .unwrap();
    assert_eq!(consumer["trackMatte"]["mode"], "alpha");
    let children = matte["layers"].as_array().unwrap();
    assert!(children.iter().any(|layer| layer["type"] == "Video"));
    let guide = children
        .iter()
        .find(|layer| layer["name"] == "Nested sequence frame")
        .unwrap();
    assert_eq!(guide["rect"]["size"], json!([960.0, 540.0]));
    assert_eq!(guide["parent"], matte["id"]);
    assert_eq!(matte["masks"][0]["layer"], guide["id"]);
    assert_eq!(matte["transform"]["anchorPoint"], json!([480.0, 270.0]));
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (axis, property) in ["positionX", "positionY"].iter().enumerate() {
        let entry = entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == matte["id"]
                    && entry["target"]["propertyType"] == *property
            })
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), native_keys.len());
        for (native, key) in native_keys.iter().zip(keys) {
            assert_eq!(
                key["layerTime"],
                (native.source_ticks - TICKS / 2) / TICKS_PER_MILLISECOND
            );
            assert_eq!(
                key["value"]["value"],
                native.value[axis] * [1920.0, 1080.0][axis]
            );
        }
    }
}

#[test]
fn nested_matte_motion_diagnostic_covers_new_opacity_keys_not_unchanged_frames() {
    let (mut sequence, _) = animated_nested_matte_sequence();
    let matte = &mut sequence.video_tracks[1].nests[0];
    matte.transform = crate::schema::PrStaticTransform::default();
    matte.animations.clear();
    let (_, omissions) = import(&sequence, &video_media());
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.contains("nested matte Motion")),
        "{omissions:?}"
    );
    sequence.video_tracks[1].nests[0]
        .animations
        .push(PrPropertyAnimation::Opacity(vec![
            PrScalarKeyframe {
                source_ticks: TICKS / 2,
                value: 100.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: 7 * TICKS / 2,
                value: 50.0,
                easing: PrKeyframeEasing::Linear,
            },
        ]));
    let (document, omissions) = import(&sequence, &video_media());
    assert!(
        omissions
            .iter()
            .all(|item| item.scope != crate::OmissionScope::Occurrence),
        "{omissions:?}"
    );
    assert!(
        omissions.iter().any(|item| item.record == "nested-matte"
            && item.kind == crate::OmissionKind::Approximated
            && item.reason.contains("nested matte Motion/Opacity")),
        "{omissions:?}"
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["target"]["propertyType"], "opacity");
    let keys = entries[0]["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[0]["value"]["value"], 100.0);
    assert_eq!(keys[1]["layerTime"], 3000);
    assert_eq!(keys[1]["value"]["value"], 50.0);
}

#[test]
fn nested_matte_motion_key_collision_omits_coverage_not_the_independent_picture() {
    let (mut sequence, _) = animated_nested_matte_sequence();
    let matte = &mut sequence.video_tracks[1].nests[0];
    let PrPropertyAnimation::Position(keys) = &mut matte.animations[0] else {
        unreachable!()
    };
    keys[1].source_ticks = keys[0].source_ticks + 1;
    let mut sibling = clip_of("source", 5 * TICKS..6 * TICKS, 0);
    sibling.id = Some("independent-picture".into());
    sequence.video_tracks.push(PrVideoTrack::media([sibling]));
    let (document, omissions) = import(&sequence, &video_media());
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert!(!layers.iter().any(|layer| layer["type"] == "Group"));
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "nested-matte" && item.reason.contains("matte Motion keys")),
        "{omissions:?}"
    );
    assert!(
        omissions.iter().any(|item| item.record == "matte-consumer"
            && item
                .reason
                .contains("matte clip on track 1 was not converted")),
        "{omissions:?}"
    );
}

#[test]
fn a_nest_keyed_by_a_still_matte_puts_the_key_on_its_group() {
    use crate::schema::{PrMatteChannel, PrTrackMatte};
    let (inner, mut media) = nested_sequence();
    let inner = inner.nest_occurrences().next().unwrap().sequence.clone();
    let mut nest = crate::tests::support::nest_of(inner, 0..3 * TICKS, 0);
    nest.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    let (still_id, mut still) = crate::tests::support::named_media("still");
    still.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Still { alpha: true };
    media.insert(still_id, still);
    let sequence = sequence_of(
        "Outer",
        vec![
            PrVideoTrack {
                items: Vec::new(),
                nests: vec![nest],
                transitions: Vec::new(),
            },
            PrVideoTrack::media([clip_of("still", 0..3 * TICKS, 0)]),
        ],
    );
    let (document, omissions) = import(&sequence, &media);
    assert!(omissions.is_empty(), "{omissions:?}");
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers[0]["type"], "Image");
    assert_eq!(layers[1]["type"], "Group");
    assert_eq!(
        layers[1]["trackMatte"],
        json!({"mode": "alpha", "layer": layers[0]["id"]})
    );
}

#[test]
fn a_clip_whose_matte_the_converter_omits_is_omitted_with_it() {
    use crate::schema::PrMatteChannel;
    // The matte clip's time remap cannot import, so its layer is missing and
    // the keyed clip must not show whole.
    let mut matte = clip_of("source", 0..5 * TICKS, 0);
    matte.time_remap = Some(crate::schema::PrTimeRemap {
        keys: vec![crate::schema::PrTimeRemapKeyframe {
            timeline_ticks: 0,
            source_ticks: 0,
            easing: PrKeyframeEasing::Linear,
        }],
    });
    let sequence = keyed_sequence(PrMatteChannel::Alpha, PrVideoTrack::media([matte]), |_| {});
    let (document, omissions) = import(&sequence, &video_media());
    assert_eq!(layer_types(&document), ["Rect"]);
    assert!(
        omissions.iter().any(|omission| omission.scope == crate::OmissionScope::Occurrence
            && omission.record == "source"
            && omission.reason
                == "track 0, range 0..1270080000000 ticks: the matte clip on track 1 was not converted; occurrence omitted"),
        "{omissions:?}"
    );
}

#[test]
fn native_film_impact_tail_becomes_two_editable_smoothstep_opacity_keys() {
    let (project, _) = crate::format::inspect_project_with_omissions(
        &crate::tests::support::film_impact_tail_xml(),
        None,
    )
    .unwrap();
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut omissions = Vec::new();
    let doc = premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let entries = doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["target"]["layerId"],
        doc["composition"]["layers"][0]["id"]
    );
    let keys = entries[0]["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["layerTime"], 4000);
    assert_eq!(keys[1]["layerTime"], 5000);
    assert_eq!(keys[0]["value"], json!({"type":"float", "value":100.0}));
    assert_eq!(keys[1]["value"], json!({"type":"float", "value":0.0}));
    assert_eq!(
        keys[1]["easing"],
        json!({"type":"cubicBezier", "x1":1.0/3.0,"y1":0.0,"x2":2.0/3.0,"y2":1.0})
    );
    assert_eq!(
        doc["composition"]["layers"][0]["sourceRange"],
        json!({"start":0,"duration":5000})
    );
    assert_eq!(
        doc["composition"]["layers"][0]["transform"]["opacity"],
        100.0
    );
    assert!(omissions.iter().any(
        |item| item.record == sequence.video_tracks[0].transitions[0].id
            && item.kind == crate::OmissionKind::Approximated
    ));
    let writer_error = crate::format::PremiereProjectXml::new(&project)
        .unwrap_err()
        .to_string();
    assert!(
        writer_error.contains("writer supports only native Cross Dissolve New video transitions"),
        "{writer_error}"
    );
}

fn film_impact_tail(sequence: &mut PrSequence) {
    sequence.video_tracks[0].clip_mut(0).id = Some("clip".into());
    let clip = sequence.video_tracks[0].clip(0);
    let tail = PrVideoTransition {
        id: "tail".into(),
        kind: PrVideoTransitionKind::FilmImpactDissolve,
        start_ticks: clip.end_ticks - TICKS,
        cut_ticks: clip.end_ticks,
        end_ticks: clip.end_ticks,
        outgoing_clip: clip.id.clone(),
        incoming_clip: None,
    };
    sequence.video_tracks[0].transitions.push(tail);
}

#[test]
fn film_impact_tail_uses_rounded_absolute_clock_and_keeps_source_trim() {
    let mut sequence = video_sequence();
    let frame = FrameRate::Fps30000Over1001.ticks_per_frame();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks = frame;
    clip.end_ticks = 14 * frame;
    clip.in_ticks = 3 * TICKS;
    clip.out_ticks = 3 * TICKS + 13 * frame;
    clip.opacity = 72.0;
    film_impact_tail(&mut sequence);
    let transition = &mut sequence.video_tracks[0].transitions[0];
    transition.start_ticks = 3 * frame;
    let document = project_document(&sequence);
    let keys = &document["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"];
    assert_eq!(keys[0]["layerTime"], 67); // round(100.1) - round(33.366...)
    assert_eq!(keys[1]["layerTime"], 434); // round(467.133...) - round(33.366...)
    assert_eq!(keys[0]["value"]["value"], 72.0);
    assert_eq!(
        document["composition"]["layers"][0]["playback"]["inputRange"]["duration"],
        434.0
    );
    assert_eq!(
        document["composition"]["layers"][0]["sourceRange"]["start"],
        3000.0
    );
}

#[test]
fn film_impact_tail_protects_host_opacity_masks_and_playback() {
    let mutations: &[fn(&mut crate::schema::PrVideoOccurrence)] = &[
        |clip| clip.playback_rate = 2.0,
        |clip| clip.crop = left_crop(),
        |clip| clip.opacity_mask = Some(opacity_mask()),
        |clip| clip.active_transforms = 1,
        |clip| clip.animations.push(opacity_fade()),
        |clip| clip.blend_mode = crate::schema::PrBlendMode::Multiply,
    ];
    for mutation in mutations {
        let mut sequence = video_sequence();
        film_impact_tail(&mut sequence);
        mutation(sequence.video_tracks[0].clip_mut(0));
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        assert!(
            omissions
                .iter()
                .any(|item| item.record == "tail" && item.kind == crate::OmissionKind::Omitted),
            "{omissions:?}"
        );
        assert!(!document.to_string().contains("film-impact-dissolve"));
    }
    let mut sequence = video_sequence();
    film_impact_tail(&mut sequence);
    let duplicate = sequence.video_tracks[0].transitions[0].clone();
    sequence.video_tracks[0].transitions.push(duplicate);
    assert!(!project_document(&sequence)
        .to_string()
        .contains("film-impact-dissolve"));
}

#[test]
fn native_film_impact_head_becomes_a_mirrored_editable_fade() {
    let (project, reader_omissions) = crate::format::inspect_project_with_omissions(
        &crate::tests::support::film_impact_head_xml(),
        None,
    )
    .unwrap();
    assert!(reader_omissions.is_empty(), "{reader_omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let keys = &document["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"];
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[1]["layerTime"], 1000);
    assert_eq!(keys[0]["value"]["value"], 0.0);
    assert_eq!(keys[1]["value"]["value"], 100.0);
    assert_eq!(keys[1]["easing"]["type"], "cubicBezier");
    assert!(keys[1]["id"]
        .as_str()
        .unwrap()
        .starts_with("premiere-film-impact-dissolve-"));
    assert_eq!(
        document["composition"]["layers"][0]["playback"]["inputRange"]["duration"],
        5000
    );
    assert!(omissions
        .iter()
        .any(|item| item.kind == crate::OmissionKind::Approximated));
}

#[test]
fn native_film_impact_pop_preserves_center_source_clock_and_dissolve() {
    let (project, read_omissions) = crate::format::inspect_project_with_omissions(
        &crate::tests::support::film_impact_pop_xml(),
        None,
    )
    .unwrap();
    assert!(read_omissions.is_empty(), "{read_omissions:?}");
    let mut sequence = project.single_sequence().unwrap().clone();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.transform.anchor_point = [0.25, 0.75];
    clip.transform.position = [0.3, 0.4];
    clip.transform.scale = [50.0, 80.0];
    clip.transform.rotation = 30.0;
    film_impact_tail(&mut sequence);
    sequence.video_tracks[0].transitions[0].incoming_clip = Some("clip".into());
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &project.media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &project.media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(
        entries.len(),
        5,
        "four geometry tracks coexist with tail opacity"
    );
    let keys = |name: &str| {
        &entries
            .iter()
            .find(|entry| entry["target"]["propertyType"] == name)
            .unwrap()["animator"]["keyframes"]
    };
    let sx = keys("scaleX");
    let sy = keys("scaleY");
    assert_eq!(sx[0]["value"]["value"], 0.0);
    assert!((sx[7]["value"]["value"].as_f64().unwrap() - 56.4).abs() < 1e-10);
    assert_eq!(sx[15]["value"]["value"], 50.0);
    assert_eq!(sx[15]["layerTime"], 1000);
    let px = keys("positionX");
    let py = keys("positionY");
    let static_transform = &document["composition"]["layers"][0]["transform"];
    let a = [
        static_transform["anchorPoint"][0].as_f64().unwrap(),
        static_transform["anchorPoint"][1].as_f64().unwrap(),
    ];
    let source = project
        .media
        .values()
        .next()
        .unwrap()
        .video
        .as_ref()
        .unwrap();
    let (sin, cos) = 30_f64.to_radians().sin_cos();
    let center = [
        px[0]["value"]["value"].as_f64().unwrap(),
        py[0]["value"]["value"].as_f64().unwrap(),
    ];
    for index in 0..16 {
        let ox = (f64::from(source.width) * 0.5 - a[0])
            * sx[index]["value"]["value"].as_f64().unwrap()
            / 100.0;
        let oy = (f64::from(source.height) * 0.5 - a[1])
            * sy[index]["value"]["value"].as_f64().unwrap()
            / 100.0;
        assert!(
            (px[index]["value"]["value"].as_f64().unwrap() + cos * ox - sin * oy - center[0]).abs()
                < 1e-9
        );
        assert!(
            (py[index]["value"]["value"].as_f64().unwrap() + sin * ox + cos * oy - center[1]).abs()
                < 1e-9
        );
    }
    assert_eq!(
        document["composition"]["layers"][0]["sourceRange"],
        serde_json::json!({"start":0,"duration":5000})
    );
    assert_eq!(
        omissions
            .iter()
            .filter(|o| o.kind == crate::OmissionKind::Approximated)
            .count(),
        2
    );
}

#[test]
fn native_film_impact_pop_rejects_retained_motion_dependent_effects() {
    use crate::{
        schema::{PrColour, PrMediaKind, PrRamp},
        tests::support::{current_blur_export, directional_blur},
    };
    let (project, _) = crate::format::inspect_project_with_omissions(
        &crate::tests::support::film_impact_pop_xml(),
        None,
    )
    .unwrap();
    let ramp = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Ramp(PrRamp {
            start: [0.5, 0.0],
            start_colour: PrColour { rgb: [0, 0, 0] },
            end: [0.5, 1.0],
            end_colour: PrColour {
                rgb: [255, 255, 255],
            },
            blend: 0.0,
        }),
        animations: Vec::new(),
    };
    // A still imports its effects as a video clip does, so they hold its Pop
    // back alike.
    for still in [false, true] {
        let mut media = project.media.clone();
        if still {
            for source in media.values_mut() {
                source.video.as_mut().unwrap().kind = PrMediaKind::Still { alpha: true };
            }
        }
        for effect in [
            directional_blur(true, 30.0, 12.0),
            current_blur_export(directional_blur(true, 30.0, 12.0)),
            ramp.clone(),
        ] {
            for enabled in [true, false] {
                let mut sequence = project.single_sequence().unwrap().clone();
                let mut effect = effect.clone();
                effect.enabled = enabled;
                sequence.video_tracks[0].clip_mut(0).effects.push(effect);
                let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
                let mut omissions = Vec::new();
                let doc = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
                    .unwrap()
                    .to_json_value()
                    .unwrap();
                let layer = &doc["composition"]["layers"][0];
                assert_eq!(layer["type"], if still { "Image" } else { "Video" });
                assert_eq!(layer["effects"].as_array().unwrap().len(), 1);
                assert_eq!(layer["effects"][0]["enabled"], enabled);
                let tracks = doc["composition"]["dynamics"]["entries"]
                    .as_array()
                    .map_or(0, Vec::len);
                assert_eq!(tracks, if enabled { 0 } else { 4 }, "{omissions:?}");
                assert_eq!(
                    omissions.iter().any(|omission| {
                        omission.record == sequence.video_tracks[0].transitions[0].id
                            && omission.kind == crate::OmissionKind::Omitted
                            && omission.reason.contains("Pop")
                    }),
                    enabled,
                    "{omissions:?}"
                );
            }
        }
    }
}

#[test]
fn native_film_impact_pop_keeps_color_effects_and_omitted_native_effects() {
    use crate::{
        schema::{PrLevels, PrMediaKind},
        tests::support::directional_blur,
    };
    let (project, _) = crate::format::inspect_project_with_omissions(
        &crate::tests::support::film_impact_pop_xml(),
        None,
    )
    .unwrap();
    let levels = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Levels(PrLevels::Master {
            rgb: [10.0, 245.0, 0.0, 255.0, 1.0],
        }),
        animations: Vec::new(),
    };
    // A still keeps its color effect beside its Pop, as a video does.
    for (still, scale, effect, retained) in [
        (false, [100.0; 2], levels.clone(), true),
        (true, [100.0; 2], levels, true),
        (
            false,
            [50.0, 80.0],
            directional_blur(true, 30.0, 12.0),
            false,
        ),
    ] {
        let mut media = project.media.clone();
        if still {
            for source in media.values_mut() {
                source.video.as_mut().unwrap().kind = PrMediaKind::Still { alpha: true };
            }
        }
        let mut sequence = project.single_sequence().unwrap().clone();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.transform.scale = scale;
        clip.effects.push(effect);
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let doc = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        assert_eq!(
            doc["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            4,
            "{omissions:?}"
        );
        assert_eq!(
            doc["composition"]["layers"][0]["effects"]
                .as_array()
                .is_some(),
            retained,
            "{omissions:?}"
        );
        assert!(
            !omissions.iter().any(|omission| {
                omission.record == sequence.video_tracks[0].transitions[0].id
                    && omission.kind == crate::OmissionKind::Omitted
            }),
            "{omissions:?}"
        );
    }
}

#[test]
fn native_film_impact_pop_rejects_changed_or_private_profile() {
    let original = crate::tests::support::film_impact_pop_xml();
    for source in [
        original.replace("260300.,0,0,0,0,0,0", "260301.,0,0,0,0,0,0"),
        original.replace("<Component Version=\"7\">", "<Component Version=\"7\"><Node Version=\"1\" ObjectRef=\"666\"><Properties Version=\"1\"><ECP.Filter.Expanded>true</ECP.Filter.Expanded></Properties></Node>"),
        original.replace(
            "<ParameterID>8022</ParameterID>",
            "<ParameterID>8022</ParameterID><CurrentValue>private</CurrentValue>",
        ),
        original.replace(
            "<ParameterID>54</ParameterID>",
            "<ParameterID>54</ParameterID><Keyframes><!-- private -->1,2</Keyframes>",
        ),
        original.replace(
            "<ParameterID>26</ParameterID>",
            "<ParameterID>26</ParameterID><Private/>",
        ),
    ] {
        assert_ne!(source, original);
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&source, None).unwrap();
        assert!(project.single_sequence().unwrap().video_tracks[0]
            .transitions
            .is_empty());
        assert!(omissions.iter().any(|o| o.record == "1006"));
    }
}

#[test]
fn native_film_impact_pop_accepts32frame_absolute_clock_and_rejects_conflicts() {
    let (project, _) = crate::format::inspect_project_with_omissions(
        &crate::tests::support::film_impact_pop_xml(),
        None,
    )
    .unwrap();
    let mut sequence = project.single_sequence().unwrap().clone();
    sequence.frame_rate = FrameRate::Fps30000Over1001;
    let frame = sequence.frame_rate.ticks_per_frame();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks = frame;
    clip.end_ticks += frame;
    clip.in_ticks = 3 * TICKS;
    clip.out_ticks = 8 * TICKS;
    let pop = &mut sequence.video_tracks[0].transitions[0];
    pop.start_ticks = frame;
    pop.cut_ticks = frame;
    pop.end_ticks = 33 * frame;
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &project.media);
    let build = |sequence: &PrSequence| {
        let mut omissions = Vec::new();
        let doc = premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        (doc, omissions)
    };
    let (document, _) = build(&sequence);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0]["animator"]["keyframes"][15]["layerTime"], 1068);
    assert_eq!(
        document["composition"]["layers"][0]["sourceRange"]["start"],
        3000
    );
    let mutations: &[fn(&mut PrSequence)] = &[
        |s| s.video_tracks[0].clip_mut(0).playback_rate = 2.0,
        |s| s.video_tracks[0].clip_mut(0).crop = left_crop(),
        |s| s.video_tracks[0].clip_mut(0).opacity_mask = Some(opacity_mask()),
        |s| s.video_tracks[0].clip_mut(0).active_transforms = 1,
        |s| {
            s.video_tracks[0]
                .clip_mut(0)
                .animations
                .push(PrPropertyAnimation::UniformScale(vec![PrScalarKeyframe {
                    source_ticks: 3 * TICKS,
                    value: 50.0,
                    easing: PrKeyframeEasing::Linear,
                }]))
        },
        |s| {
            let pop = &s.video_tracks[0].transitions[0];
            let mut fade = pop.clone();
            fade.kind = PrVideoTransitionKind::FilmImpactDissolve;
            fade.outgoing_clip = fade.incoming_clip.take();
            fade.start_ticks = pop.start_ticks;
            fade.end_ticks = s.video_tracks[0].clip(0).end_ticks;
            fade.cut_ticks = fade.end_ticks;
            s.video_tracks[0].transitions.push(fade);
        },
        |s| s.video_tracks[0].transitions[0].end_ticks -= 1,
        |s| s.video_tracks[0].transitions[0].cut_ticks += 1,
        |s| {
            s.video_tracks[0].transitions[0].outgoing_clip =
                s.video_tracks[0].transitions[0].incoming_clip.clone()
        },
        |s| {
            let duplicate = s.video_tracks[0].transitions[0].clone();
            s.video_tracks[0].transitions.push(duplicate);
        },
    ];
    for mutation in mutations {
        let mut invalid = sequence.clone();
        mutation(&mut invalid);
        let (doc, omissions) = build(&invalid);
        assert!(
            !doc.to_string().contains("film-impact-pop"),
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|o| o.record == sequence.video_tracks[0].transitions[0].id
                    && o.kind == crate::OmissionKind::Omitted),
            "{omissions:?}"
        );
    }
}

fn film_impact_stroke_xml(size: u32, prescale: u32) -> String {
    let native = include_str!("../../../tests/fixtures/film-impact-stroke-profile.xml")
        .replace("<PremiereData>", "")
        .replace("</PremiereData>", "")
        .replace(
            "-91445760000000000,6.,",
            &format!("-91445760000000000,{size}.,"),
        )
        .replace(
            "<CurrentValue>6</CurrentValue>",
            &format!("<CurrentValue>{size}</CurrentValue>"),
        )
        .replace(
            "-91445760000000000,99.,",
            &format!("-91445760000000000,{prescale}.,"),
        )
        .replace(
            "<CurrentValue>99</CurrentValue>",
            &format!("<CurrentValue>{prescale}</CurrentValue>"),
        );
    include_str!("../../../tests/fixtures/one-clip.xml")
        .replace("<DefaultOpacity>true</DefaultOpacity><ComponentChain/>", "<DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"2952\"/></Components></ComponentChain>")
        .replace("</PremiereData>", &format!("{native}</PremiereData>"))
}

fn stroke_project(size: u32, prescale: u32) -> crate::format::PrProjectFile {
    let (project, omissions) = crate::format::inspect_project_with_omissions(
        &film_impact_stroke_xml(size, prescale),
        None,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert!(project.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .stroke
        .is_some());
    project
}

#[test]
fn native_film_impact_stroke_three_profiles_keep_editable_geometry_and_trim() {
    for (size, prescale) in [(6, 99), (6, 100), (66, 99)] {
        let project = stroke_project(size, prescale);
        let mut sequence = project.single_sequence().unwrap().clone();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.start_ticks = 2 * TICKS;
        clip.end_ticks = 7 * TICKS;
        clip.in_ticks = 3 * TICKS;
        clip.out_ticks = 8 * TICKS;
        clip.transform.scale = [53.0; 2];
        clip.transform.rotation = 23.0;
        clip.transform.anchor_point = [0.25, 0.75];
        clip.opacity = 72.0;
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &project.media);
        let mut omissions = Vec::new();
        let doc = premiere_to_tesseract(&sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        let group = &doc["composition"]["layers"][0];
        assert_eq!(group["type"], "Group");
        // A plain Group clock: the outer window starts its children at zero.
        assert_eq!(
            group["playback"],
            crate::test_support::linear_playback(
                json!({"start":2000,"duration":5000}),
                json!({"start":0,"duration":5000})
            )
        );
        assert_eq!(group["transform"]["opacity"], 72.0);
        assert_eq!(group["transform"]["rotation"], 23.0);
        let video = &group["layers"][0];
        assert_eq!(video["type"], "Video");
        // The relocated window keeps the 1x source trim: local zero is source 3 s.
        assert_eq!(
            video["playback"],
            json!({"type":"windowed", "inputRange":{"start":0,"duration":5000}, "mapping":{"type":"linear", "input":{"start":2000,"duration":5000}, "output":{"start":3000,"duration":5000}}, "inputOffsetMs":2000})
        );
        assert_eq!(video["sourceRange"], json!({"start":3000,"duration":5000}));
        assert_eq!(video["volume"], 0.0);
        assert_eq!(video["transform"]["opacity"], 100.0);
        assert_eq!(
            video["transform"]["scale"],
            json!([f64::from(prescale), f64::from(prescale)])
        );
        assert_eq!(video["parent"], group["id"]);
        if size == 66 {
            assert_eq!(group["layers"].as_array().unwrap().len(), 2);
            let border = &group["layers"][1];
            assert_eq!(border["type"], "Rect");
            assert_eq!(border["rect"]["size"], json!([1920.0, 1080.0]));
            assert_eq!(border["rect"]["fillColor"], json!([1.0, 1.0, 1.0, 1.0]));
            assert_eq!(border["parent"], group["id"]);
        } else {
            assert_eq!(group["layers"].as_array().unwrap().len(), 1);
            assert_eq!(video["effects"][0]["effect"]["type"], "stroke");
            assert!(
                (video["effects"][0]["effect"]["width"].as_f64().unwrap() - 3.18).abs() < 1e-10
            );
            assert_eq!(video["effects"][0]["effect"]["position"], "outside");
        }
        assert!(omissions
            .iter()
            .any(|o| o.kind == crate::OmissionKind::Approximated));
    }
}

#[test]
fn native_film_impact_stroke_outer_owner_keeps_pop_tail_and_local_source_clock() {
    let project = stroke_project(66, 99);
    let mut sequence = project.single_sequence().unwrap().clone();
    let (pop, _) = crate::format::inspect_project_with_omissions(
        &crate::tests::support::film_impact_pop_xml(),
        None,
    )
    .unwrap();
    sequence.video_tracks[0].transitions = pop.single_sequence().unwrap().video_tracks[0]
        .transitions
        .clone();
    film_impact_tail(&mut sequence);
    sequence.video_tracks[0].transitions[0].incoming_clip = Some("clip".into());
    let shift = 2 * TICKS;
    for t in &mut sequence.video_tracks[0].transitions {
        t.start_ticks += shift;
        t.end_ticks += shift;
        t.cut_ticks += shift;
    }
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks += shift;
    clip.end_ticks += shift;
    clip.in_ticks = 3 * TICKS;
    clip.out_ticks = 8 * TICKS;
    clip.transform.scale = [40.0; 2];
    clip.transform.rotation = 30.0;
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &project.media);
    let mut omissions = Vec::new();
    let doc = premiere_to_tesseract(&sequence, &project.media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let group = &doc["composition"]["layers"][0];
    let tracks = doc["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(tracks.len(), 5, "{omissions:?}");
    assert!(tracks.iter().all(|t| t["target"]["layerId"] == group["id"]));
    assert_eq!(
        tracks
            .iter()
            .find(|t| t["target"]["propertyType"] == "scaleX")
            .unwrap()["animator"]["keyframes"][0]["layerTime"],
        0
    );
    assert_eq!(
        group["layers"][0]["sourceRange"],
        json!({"start":3000,"duration":5000})
    );
    assert_eq!(
        group["layers"][0]["playback"]["inputRange"],
        json!({"start":0,"duration":5000})
    );
    assert_eq!(
        omissions
            .iter()
            .filter(|o| o.kind == crate::OmissionKind::Approximated)
            .count(),
        3
    );
}

#[test]
fn native_film_impact_stroke_rejects_nonprofile_or_private_controls() {
    let original = film_impact_stroke_xml(6, 99);
    for source in [
        original.replace("18374966859414961920", "18374966859414961921"),
        film_impact_stroke_xml(66, 100),
        film_impact_stroke_xml(7, 99),
        original.replace(
            "<ParameterID>14</ParameterID>",
            "<ParameterID>14</ParameterID><Keyframes><!-- hidden -->1,2</Keyframes>",
        ),
        original.replace(
            "<ECP.Group.Expanded>true</ECP.Group.Expanded>",
            "<ECP.Group.Expanded>true</ECP.Group.Expanded><Private>true</Private>",
        ),
        original.replace(
            "<CurrentValue>99</CurrentValue>",
            "<CurrentValue>98</CurrentValue>",
        ),
        original.replace(
            "<Name>Hide Source</Name>",
            "<Name>Hide Source</Name><CurrentValue>true</CurrentValue>",
        ),
    ] {
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&source, None).unwrap();
        assert!(project.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .stroke
            .is_none());
        assert!(
            omissions.iter().any(|o| o.record.ends_with(":2952")),
            "{omissions:?}"
        );
    }
}

#[test]
fn native_film_impact_stroke_unsupported_hosts_keep_picture_without_border() {
    let mutations: &[fn(&mut crate::schema::PrVideoOccurrence)] = &[
        |c| c.crop = left_crop(),
        |c| c.opacity_mask = Some(opacity_mask()),
        |c| c.playback_rate = 2.0,
        |c| c.active_transforms = 1,
        |c| c.transform.scale = [40.0, 50.0],
        |c| c.blend_mode = crate::schema::PrBlendMode::Multiply,
        |c| c.frame_blending = Some(fx_schema::FrameBlendingMode::Simple),
    ];
    for mutation in mutations {
        let project = stroke_project(66, 99);
        let mut sequence = project.single_sequence().unwrap().clone();
        mutation(sequence.video_tracks[0].clip_mut(0));
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &project.media);
        let mut omissions = Vec::new();
        let doc = premiere_to_tesseract(&sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        assert!(
            !doc.to_string().contains("Premiere Stroke"),
            "{omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|o| o.reason.contains("Film Impact Stroke")
                    && o.kind == crate::OmissionKind::Omitted),
            "{omissions:?}"
        );
    }
}

#[test]
fn native_film_impact_stroke_remains_import_only_and_rejects_nonvideo_hosts() {
    let project = stroke_project(66, 99);
    let error = crate::format::PremiereProjectXml::new(&project)
        .unwrap_err()
        .to_string();
    assert!(error.contains("import-only"), "{error}");
    for (kind, layer_type) in [
        (crate::schema::PrMediaKind::Still { alpha: true }, "Image"),
        (crate::schema::PrMediaKind::Adjustment, "Adjustment"),
    ] {
        let mut media = project.media.clone();
        for source in media.values_mut() {
            source.video.as_mut().unwrap().kind = kind;
        }
        let mut sequence = project.single_sequence().unwrap().clone();
        if layer_type == "Adjustment" {
            sequence.video_tracks[0].clip_mut(0).effects.push(PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::GaussianBlur(PrGaussianBlur {
                    blurriness: 12.0,
                    repeat_edge_pixels: true,
                }),
                animations: Vec::new(),
            });
        }
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let doc = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        let layer = &doc["composition"]["layers"][0];
        assert_eq!(layer["type"], layer_type);
        if layer_type == "Adjustment" {
            assert_eq!(layer["effects"].as_array().unwrap().len(), 1);
            assert_eq!(layer["effects"][0]["effect"]["type"], "gaussianBlur");
            assert_eq!(layer["effects"][0]["effect"]["blurriness"], 12.0);
        }
        assert!(
            omissions
                .iter()
                .any(|o| o.scope == crate::OmissionScope::Feature
                    && o.kind == crate::OmissionKind::Omitted
                    && o.reason.contains("Film Impact Stroke")
                    && o.reason.contains("not an opaque physical video")),
            "{layer_type}: {omissions:?}"
        );
    }
}

#[test]
fn native_film_impact_stroke_measured_99_start_95_cache_is_only_frame99() {
    // Exact Alix parameter23300; Bella23380 has the identical saved tree.
    let native = include_str!("../../../tests/fixtures/film-impact-stroke-prescale-cache.xml")
        .replace("ObjectID=\"23300\"", "ObjectID=\"5367\"");
    let original = film_impact_stroke_xml(66, 99);
    let start = original
        .find("<VideoComponentParam ObjectID=\"5367\"")
        .unwrap();
    let end = start
        + original[start..].find("</VideoComponentParam>").unwrap()
        + "</VideoComponentParam>".len();
    let mut saved = original.clone();
    saved.replace_range(start..end, &native);
    let (project, omissions) = crate::format::inspect_project_with_omissions(&saved, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .stroke,
        Some(crate::schema::PrFilmImpactStroke::Frame99)
    );
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let doc = premiere_to_tesseract(sequence, &project.media, &ids, &mut Vec::new())
        .unwrap()
        .to_json_value()
        .unwrap();
    assert_eq!(
        doc["composition"]["layers"][0]["layers"][0]["transform"]["scale"],
        json!([99.0, 99.0])
    );
    for invalid in [
        saved.replace(
            "<CurrentValue>95</CurrentValue>",
            "<CurrentValue>94</CurrentValue>",
        ),
        film_impact_stroke_xml(6, 99).replace(
            "<CurrentValue>99</CurrentValue>",
            "<CurrentValue>95</CurrentValue>",
        ),
        saved.replace("-91445760000000000,99.,", "-91445760000000000,98.,"),
        saved.replace(
            "<ParameterID>8223</ParameterID>",
            "<ParameterID>8223</ParameterID><Keyframes><!-- private -->1,2</Keyframes>",
        ),
    ] {
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&invalid, None).unwrap();
        assert!(project.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .stroke
            .is_none());
        assert!(
            omissions.iter().any(|o| o.record.ends_with(":2952")),
            "{omissions:?}"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn interpretation_staged_origin_and_zero_duration_preserve_sibling() {
    use crate::schema::{SourceFrameRate, SourceInterpretation};
    use std::io::Cursor;
    let bytes = include_bytes!("../../../tests/fixtures/video-120000-over-1001fps.mp4");
    for (zero_duration, safe_origin) in [(false, false), (true, false), (false, true)] {
        let mut facts = crate::media::inspect_video_media(
            Cursor::new(bytes),
            Cursor::new(bytes),
            bytes.len() as u64,
        )
        .unwrap();
        let mut media = video_media();
        let mut interpreted = media[&MediaId("source".into())].clone();
        let source = interpreted.video.as_mut().unwrap();
        source.frame_rate = SourceFrameRate::from_ticks_per_frame(2_118_936_594).unwrap();
        source.intrinsic_ticks = 6 * 2_118_936_594;
        source.interpretation = SourceInterpretation::Rate(FrameRate::Fps30.into());
        if zero_duration {
            facts.timing.clock = crate::media::SampleClock::Constant { sample_duration: 1 };
            source.frame_rate = SourceFrameRate::from_ticks_per_frame(TICKS / 120000).unwrap();
            source.intrinsic_ticks = 6 * source.frame_rate.ticks_per_frame();
        }
        let id = MediaId("interpreted".into());
        let clocks = BTreeMap::from([(
            id.clone(),
            crate::media::InterpretedPictureClock::bind(source, &facts)
                .map(crate::media::PictureClock::Interpreted),
        )]);
        if zero_duration {
            assert!(
                clocks[&id].is_err(),
                "zero-ms physical duration must not bind"
            );
        }
        media.insert(id.clone(), interpreted);
        let mut sequence = video_sequence();
        let mut interpreted = sequence.video_tracks[0].clip(0).clone();
        interpreted.media = id.clone();
        interpreted.start_ticks = if safe_origin {
            3 * THIRTY_FPS_TICKS
        } else {
            THIRTY_FPS_TICKS
        };
        interpreted.end_ticks = interpreted.start_ticks + 3 * THIRTY_FPS_TICKS;
        interpreted.in_ticks = 0;
        interpreted.out_ticks = 3 * THIRTY_FPS_TICKS;
        let mut transform = DEFAULT_PR_TRANSFORM;
        transform.rotation = 20.0;
        interpreted.effects = vec![transform_effect(transform, Vec::new())];
        interpreted.active_transforms = 1;
        sequence.video_tracks.push(PrVideoTrack {
            items: vec![PrVideoItem::Media(interpreted)],
            nests: Vec::new(),
            transitions: Vec::new(),
        });
        let ids = BTreeMap::from([
            (MediaId("source".into()), AssetId::new("ordinary").unwrap()),
            (id, AssetId::new("interpreted").unwrap()),
        ]);
        let mut omissions = Vec::new();
        let document = crate::convert::sequence_document_with_progress(
            &sequence,
            &media,
            &ids,
            &clocks,
            &mut Default::default(),
            &mut omissions,
            Default::default(),
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        let layers = document["composition"]["layers"].as_array().unwrap();
        assert_eq!(
            layers.iter().filter(|l| l["type"] == "Video").count(),
            1,
            "{omissions:?}"
        );
        if safe_origin {
            let stage = layers
                .iter()
                .find(|l| l["type"] == "Group")
                .expect("exact static staging retained");
            assert_eq!(
                stage["layers"][0]["playback"]["mapping"]["output"],
                json!({"start":0,"duration":1001})
            );
            assert_eq!(stage["layers"][0]["playback"]["inputOffsetMs"], 0);
            continue;
        }
        assert!(
            !layers.iter().any(|l| l["type"] == "Group"),
            "unsafe staged picture retained: {layers:?}"
        );
        assert!(
            omissions
                .iter()
                .any(|o| o
                    .reason
                    .contains(if zero_duration { "duration" } else { "origin" })),
            "{omissions:?}"
        );
    }
}

#[test]
fn time_remap_unused_tail_checks_rounded_window_and_preserves_sibling() {
    use crate::schema::{PrTimeRemap, PrTimeRemapKeyframe};
    // A frame-aligned 3 s placement plays an identity source curve at a rate
    // reaching exactly 3000.8 ms of media. Its emitted tail keys land at
    // 1999/3999 ms, mapping the end to 3001 ms: beyond the exact media end,
    // even though intrinsic duration rounds up to 3001. At 3000.6 ms the
    // keys land at 2000/3999 ms and the played endpoint remains in bounds.
    // With only 0/4000 ms keys, the rounded tail ends at 3999 ms and
    // plays through 3000.75... ms: safe for 3000.8, unsafe for 3000.6.
    for (times, fraction, expected_videos) in [
        (vec![0, 1000, 2000, 4000], 6, 2),
        (vec![0, 1000, 2000, 4000], 8, 1),
        (vec![0, 4000], 8, 2),
        (vec![0, 4000], 6, 1),
    ] {
        let ms = TICKS_PER_MILLISECOND;
        let end = 3000 * ms + fraction * ms / 10;
        let mut media = video_media();
        let facts = media.get_mut(&MediaId("source".into())).unwrap();
        facts.video.as_mut().unwrap().intrinsic_ticks = end;
        let mut clip = clip_of("source", 0..3 * TICKS, 0);
        clip.out_ticks = end;
        clip.playback_rate = end as f64 / (3 * TICKS) as f64;
        clip.time_remap = Some(PrTimeRemap {
            keys: times
                .into_iter()
                .map(|time| PrTimeRemapKeyframe {
                    timeline_ticks: time * ms,
                    source_ticks: time * ms,
                    easing: PrKeyframeEasing::Linear,
                })
                .collect(),
        });
        clip.validate(FrameRate::Fps30, facts).unwrap();
        let sibling = clip_of("source", 4 * TICKS..5 * TICKS, 0);
        let sequence = sequence_of("bounded tail", vec![PrVideoTrack::media([clip, sibling])]);
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        let videos: Vec<_> = wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .collect();
        assert_eq!(videos.len(), expected_videos);
        assert!(videos
            .iter()
            .any(|layer| layer["playback"]["inputRange"]["start"] == 4000));
        let occurrences: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.scope == crate::OmissionScope::Occurrence)
            .map(|omission| omission.reason.as_str())
            .collect();
        if expected_videos == 1 {
            assert_eq!(occurrences, ["clip was not imported: unsupported conversion: TimeRemapping rounded playback window reaches outside the media bounds"]);
        } else {
            assert!(occurrences.is_empty(), "{omissions:?}");
        }
    }
}

#[test]
fn delayed_audio_rebases_gain_and_in_flight_fades_without_refitting() {
    use crate::schema::{PrAudioFade, PrAudioOccurrence, PrFadeCurve, PrVolumeKeys};
    let ms = TICKS / 1000;
    let clip = PrAudioOccurrence {
        id: None,
        media: MediaId("sound".into()),
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate: 1.0,
        start_ticks: 1000 * ms,
        end_ticks: 2000 * ms,
        in_ticks: 0,
        out_ticks: 1000 * ms,
        volume: fx_schema::LinearGain::new(0.5).unwrap(),
        volume_keys: Some(PrVolumeKeys {
            keys: vec![
                PrScalarKeyframe {
                    source_ticks: 200 * ms,
                    value: 0.5,
                    easing: PrKeyframeEasing::Linear,
                },
                PrScalarKeyframe {
                    source_ticks: 800 * ms,
                    value: 1.0,
                    easing: PrKeyframeEasing::Linear,
                },
            ],
            gain: 0.5,
        }),
        fade_in: Some(PrAudioFade {
            id: None,
            curve: PrFadeCurve::ConstantPower,
            duration_ticks: 200 * ms,
        }),
        fade_out: Some(PrAudioFade {
            id: None,
            curve: PrFadeCurve::ConstantGain,
            duration_ticks: 200 * ms,
        }),
    };
    let mut omissions = Vec::new();
    let track = super::placement_volume_track(
        &clip,
        fx_schema::LayerId::new(1),
        10 * TICKS,
        &mut omissions,
    )
    .unwrap()
    .unwrap();
    let rebased = super::rebase_audio_track(
        track.clone(),
        super::tick_range(1000 * ms, 2000 * ms).unwrap(),
        super::tick_range(1100 * ms, 1900 * ms).unwrap(),
    )
    .unwrap();
    // Both cuts pass through fades. Keep outside support keys to preserve the
    // exact incoming easing and values, while the active window clips playback.
    assert!(
        rebased
            .keyframes()
            .first()
            .unwrap()
            .layer_time()
            .as_millis()
            <= 0
    );
    assert!(rebased.keyframes().last().unwrap().layer_time().as_millis() >= 800);
    for shifted in rebased.keyframes() {
        let original = track
            .keyframes()
            .iter()
            .find(|key| key.id() == shifted.id())
            .unwrap();
        assert_eq!(
            shifted.layer_time().as_millis(),
            original.layer_time().as_millis() - 100
        );
        assert_eq!(shifted.value(), original.value());
        assert_eq!(shifted.easing(), original.easing());
    }
    assert!(omissions.is_empty(), "{omissions:?}");
}
