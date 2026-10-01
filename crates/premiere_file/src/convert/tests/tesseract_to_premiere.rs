use super::*;
use crate::{
    audio_media::SourceSound,
    format::{FrameRate, PremiereProjectXml},
    schema::{
        records::MediaPathField, AudioChannels, PrAudioStream, VideoCodec, TICKS,
        TICKS_PER_MILLISECOND,
    },
    test_support::editable_document,
    tests::support::{current_blur_export, exported_blur, first_clip, left_crop},
    OmissionKind,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const FRAME_30: i64 = FrameRate::Fps30.ticks_per_frame();

/// Length of the 30 fps source that `convert` inspects and `document` claims.
const SOURCE_MILLIS: i64 = 10_000;

/// Inspected facts of `premiere-video-1`, the one source in these documents.
fn source(frame_rate: FrameRate, duration_ticks: i64) -> BTreeMap<String, MediaFacts> {
    BTreeMap::from([(
        "premiere-video-1".to_owned(),
        MediaFacts::Video(crate::media::VideoMedia {
            orientation: crate::schema::VideoOrientation::Identity,
            codec: VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: 1920,
            height: 1080,
            timing: crate::media::VideoTiming::for_test(frame_rate, duration_ticks),
        }),
    )])
}

/// Inspected stereo sound of `premiere-video-1` and the audio-only `music`.
fn sound() -> BTreeMap<String, SourceSound> {
    let stream = SourceSound::Supported(PrAudioStream {
        intrinsic_ticks: SOURCE_MILLIS * TICKS_PER_MILLISECOND,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    });
    BTreeMap::from([
        ("premiere-video-1".to_owned(), stream.clone()),
        ("music".to_owned(), stream),
    ])
}

/// Converts with the document's packaged font registry (`metadata.json` `fonts`).
fn convert_with_fonts(
    wire: Value,
    fonts: &BTreeMap<String, FontAssetProperties>,
) -> Result<(PrProjectFile, Vec<crate::Omission>)> {
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let media = source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND);
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &media,
        &sound(),
        fonts,
        FrameRate::Fps30,
        &mut omissions,
    )?;
    Ok((project, omissions))
}

fn convert_with_omissions(wire: Value) -> Result<(PrProjectFile, Vec<crate::Omission>)> {
    convert_with_fonts(wire, &BTreeMap::new())
}

fn convert(wire: Value) -> Result<PrProjectFile> {
    convert_with_omissions(wire).map(|(project, _)| project)
}

/// The shared one-clip document, claiming the length of the source that
/// `convert` inspects.
fn document() -> Value {
    let mut document = editable_document();
    document["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(SOURCE_MILLIS);
    document
}

fn write_and_load_with_crate_reader(mut project: PrProjectFile) -> PrProjectFile {
    for media in project.media.values_mut() {
        media.name = "source.mp4".into();
        media.relative_path = Some("./media/source.mp4".into());
        media.relative_paths = vec!["./media/source.mp4".into()];
        media.absolute_paths = vec![(MediaPathField::FilePath, "/media/source.mp4".into())];
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (project, omissions) = PrProjectFile::load(&path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    project
}

fn two_clips() -> Value {
    let mut document = document();
    document["duration"] = json!(5);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 0, "duration": 2000}),
        json!({"start": 1000, "duration": 2000}),
    );
    layers[0]["sourceRange"] = json!({"start": 1000, "duration": 2000});
    layers[1]["activeRange"]["duration"] = json!(5000);
    let mut second = layers[0].clone();
    second["id"] = json!(3);
    second["playback"] = crate::test_support::linear_playback(
        json!({"start": 3000, "duration": 2000}),
        json!({"start": 6000, "duration": 2000}),
    );
    second["sourceRange"] = json!({"start": 6000, "duration": 2000});
    layers.insert(1, second);
    document
}

#[test]
fn a_rotated_source_is_rejected_on_export() {
    // Import admits a quarter-turn source, but export writes no orientation.
    // A half turn keeps the sourceRect frame, so only this check catches it.
    use crate::schema::VideoOrientation;
    let document = EditableFxCompositionDocument::from_json_value(document()).unwrap();
    for orientation in [
        VideoOrientation::Clockwise,
        VideoOrientation::HalfTurn,
        VideoOrientation::CounterClockwise,
    ] {
        let mut media = source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND);
        let Some(MediaFacts::Video(video)) = media.get_mut("premiere-video-1") else {
            panic!("the one source is a video");
        };
        video.orientation = orientation;
        let error = tesseract_to_premiere(
            &document,
            &media,
            &sound(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("rotated source video cannot be exported"),
            "{orientation:?}: {error}"
        );
    }
}

#[test]
fn frame_blending_modes_export_without_loss_diagnostics() {
    for (wire, expected) in [
        (json!(true), fx_schema::FrameBlendingMode::Simple),
        (
            json!("opticalFlow"),
            fx_schema::FrameBlendingMode::OpticalFlow,
        ),
    ] {
        let mut source = document();
        source["composition"]["layers"][0]["frameBlending"] = wire;
        let (project, omissions) = convert_with_omissions(source).unwrap();
        let occurrence = project.sequences[0].video_occurrences().next().unwrap();
        assert_eq!(occurrence.frame_blending, Some(expected));
        assert!(!omissions
            .iter()
            .any(|item| item.reason.contains("frame blending")));
    }
}

#[test]
fn edited_fx_rotation_keys_export_on_the_source_clock() {
    use crate::{
        schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe},
        tests::support::{project_document, video_sequence},
    };
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.animations.push(PrPropertyAnimation::Rotation(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 2 * TICKS,
            value: 90.0,
            easing: PrKeyframeEasing::Hold,
        },
        PrScalarKeyframe {
            source_ticks: 7 * TICKS,
            value: 180.0,
            easing: PrKeyframeEasing::Linear,
        },
    ]));
    let mut wire = project_document(&sequence);
    wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1]["value"]["value"] =
        json!(45.0);
    wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][2]["easing"] =
        json!({"type":"cubicBezier","x1":0.2,"y1":0.1,"x2":0.8,"y2":0.9});
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = &project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .animations[0];
    assert_eq!(
        exported
            .keys()
            .iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        [(0, 0.0), (2 * TICKS, 45.0), (7 * TICKS, 180.0)]
    );
    assert_eq!(exported.keys()[1].easing, PrKeyframeEasing::Hold);
    assert_eq!(
        exported.keys()[2].easing,
        PrKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 0.8,
            y2: 0.9,
        }
    );
}

#[test]
fn edited_fx_opacity_keys_export_on_the_source_clock() {
    use crate::{
        schema::{PrAnimatedProperty, PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe},
        tests::support::{project_document, video_sequence},
    };
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.opacity = 80.0;
    clip.animations.push(PrPropertyAnimation::Opacity(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 80.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 2 * TICKS,
            value: 50.0,
            easing: PrKeyframeEasing::Hold,
        },
        PrScalarKeyframe {
            source_ticks: 7 * TICKS,
            value: 20.0,
            easing: PrKeyframeEasing::Linear,
        },
    ]));
    let mut wire = project_document(&sequence);
    wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1]["value"]["value"] =
        json!(40.0);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.opacity, 80.0);
    let exported = clip
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Opacity)
        .unwrap();
    assert_eq!(
        exported
            .keys()
            .iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        [(0, 80.0), (2 * TICKS, 40.0), (7 * TICKS, 20.0)]
    );
    assert_eq!(exported.keys()[1].easing, PrKeyframeEasing::Hold);
}

#[test]
fn edited_fx_position_paths_export_on_the_source_clock() {
    use crate::{
        schema::{PrAnimatedProperty, PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation},
        tests::support::{project_document, video_sequence},
    };
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.animations.push(PrPropertyAnimation::Position(vec![
        PrPointKeyframe {
            source_ticks: 0,
            value: [0.25, 0.5],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: Some([0.1, -0.05]),
        },
        PrPointKeyframe {
            source_ticks: 2 * TICKS,
            value: [0.75, 0.75],
            easing: PrKeyframeEasing::CubicBezier {
                x1: 0.2,
                y1: 0.1,
                x2: 0.8,
                y2: 0.9,
            },
            spatial_in_tangent: Some([-0.1, 0.05]),
            spatial_out_tangent: None,
        },
    ]));
    let wire = project_document(&sequence);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Position)
        .unwrap()
        .point_keys()
        .unwrap();
    assert_eq!(exported[0].source_ticks, 0);
    assert_eq!(exported[1].source_ticks, 2 * TICKS);
    assert_eq!(exported[0].value, [0.25, 0.5]);
    assert_eq!(exported[1].value, [0.75, 0.75]);
    assert_eq!(exported[0].spatial_out_tangent, Some([0.1, -0.05]));
    assert_eq!(exported[1].spatial_in_tangent, Some([-0.1, 0.05]));
    assert_eq!(
        exported[1].easing,
        PrKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 0.8,
            y2: 0.9,
        }
    );
}

#[test]
fn first_keys_after_the_source_in_export_and_reimport_unchanged() {
    use crate::{
        schema::{PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe},
        tests::support::{project_document, project_document_with_media, video_sequence},
    };

    // Each first key comes after the source In and differs from the static
    // value, which Premiere never shows before it.
    let delayed = TICKS + 500 * TICKS_PER_MILLISECOND;
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
    let animations = vec![
        PrPropertyAnimation::Position(vec![
            point(delayed, [0.25, 0.5]),
            point(2 * TICKS, [0.75, 0.75]),
        ]),
        PrPropertyAnimation::Rotation(vec![scalar(delayed, 30.0), scalar(2 * TICKS, 90.0)]),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.transform.position = [0.5, 0.5];
    clip.transform.rotation = 0.0;
    clip.animations.clone_from(&animations);
    // The FX layer tracks by property: `(layerTime, value)` per key.
    let tracks = |wire: &Value| -> BTreeMap<String, Vec<(Value, Value)>> {
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
                    .map(|key| (key["layerTime"].clone(), key["value"].clone()))
                    .collect();
                let property = entry["target"]["propertyType"].as_str().unwrap();
                (property.to_owned(), keys)
            })
            .collect()
    };

    let imported = project_document(&sequence);
    let (project, omissions) = convert_with_omissions(imported.clone()).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let reread = write_and_load_with_crate_reader(project);
    for animation in &animations {
        let exported = first_clip(&reread)
            .animations
            .iter()
            .find(|exported| exported.property() == animation.property());
        assert_eq!(exported, Some(animation));
    }
    let reimported = project_document_with_media(reread.single_sequence().unwrap(), &reread.media);
    let imported_tracks = tracks(&imported);
    assert_eq!(
        imported_tracks
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["positionX", "positionY", "rotation"]
    );
    assert_eq!(tracks(&reimported), imported_tracks);
}

#[test]
fn stationary_cubic_position_is_omitted() {
    use crate::{
        schema::{PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation},
        tests::support::{project_document, video_sequence},
    };

    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.animations.push(PrPropertyAnimation::Position(vec![
        PrPointKeyframe {
            source_ticks: TICKS,
            value: [0.5, 0.5],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        },
        PrPointKeyframe {
            source_ticks: 2 * TICKS,
            value: [0.5, 0.5],
            easing: PrKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: 0.4,
                x2: 0.75,
                y2: 0.6,
            },
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        },
    ]));

    let (project, omissions) = convert_with_omissions(project_document(&sequence)).unwrap();
    assert!(project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .animations
        .is_empty());
    assert!(
        omissions.iter().any(|item| item
            .reason
            .contains("stationary spatial segment cannot preserve Premiere velocity")),
        "{omissions:?}"
    );
}

#[test]
fn dropped_animation_does_not_claim_its_cubic_easing_was_approximated() {
    use crate::{
        schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe},
        tests::support::{project_document, video_sequence},
    };
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
                source_ticks: TICKS,
                value: 90.0,
                easing: PrKeyframeEasing::Linear,
            },
        ]));
    let mut wire = project_document(&sequence);
    // A cubic curve between equal values drops the animation.
    let key = &mut wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1];
    key["value"]["value"] = json!(0.0);
    key["easing"] = json!({"type":"cubicBezier","x1":0.2,"y1":0.1,"x2":0.8,"y2":0.9});
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .animations
        .is_empty());
    assert!(
        omissions.iter().any(|item| item.reason.contains(
            "Rotation animation was not exported: unsupported conversion: Rotation cubic easing between equal values"
        )),
        "{omissions:?}"
    );
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.contains("cubic easing approximated")),
        "{omissions:?}"
    );
}

#[test]
fn a_bezier_curve_into_a_key_that_starts_a_hold_is_reported_and_the_static_value_exports() {
    use crate::{
        schema::{
            PrAnimatedProperty, PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation,
            PrScalarKeyframe,
        },
        tests::support::{project_document, video_sequence},
    };
    use PrKeyframeEasing::{CubicBezier, Hold, Linear};
    // Rotation 0 at 0 s, 90 at 2 s and 180 at 4 s, where the key at 2 s
    // starts a Hold and `arrival` eases into it. Premiere ignores that key's
    // in-handle, so only a zero-length arrival, a straight curve or Linear
    // is drawn as written; any other curve reports the keys and exports the
    // static Rotation (30).
    let exported = |arrival| {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.transform.rotation = 30.0;
        let key = |seconds: i64, value, easing| PrScalarKeyframe {
            source_ticks: seconds * TICKS,
            value,
            easing,
        };
        clip.animations.push(PrPropertyAnimation::Rotation(vec![
            key(0, 0.0, Linear),
            key(2, 90.0, arrival),
            key(4, 180.0, Hold),
        ]));
        let (project, omissions) = convert_with_omissions(project_document(&sequence)).unwrap();
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap()
            .clone();
        (clip, omissions)
    };
    let (clip, omissions) = exported(CubicBezier {
        x1: 0.2,
        y1: 0.1,
        x2: 0.8,
        y2: 0.9,
    });
    assert!(clip.animations.is_empty(), "{:?}", clip.animations);
    assert_eq!(clip.transform.rotation, 30.0);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.scope, omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            crate::OmissionScope::Feature,
            "Rotation animation was not exported: unsupported conversion: Rotation cubic easing into a key that starts a Hold cannot keep its in-handle, which Premiere ignores"
        )]
    );
    for arrival in [
        CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 1.0,
            y2: 1.0,
        },
        CubicBezier {
            x1: 0.3,
            y1: 0.3,
            x2: 0.7,
            y2: 0.7,
        },
        Linear,
    ] {
        let (clip, omissions) = exported(arrival);
        assert!(omissions.is_empty(), "{arrival:?}: {omissions:?}");
        let rotation = clip
            .animations
            .iter()
            .find(|animation| animation.property() == PrAnimatedProperty::Rotation)
            .unwrap_or_else(|| panic!("{arrival:?}"));
        assert_eq!(
            rotation
                .keys()
                .iter()
                .map(|key| key.easing)
                .collect::<Vec<_>>(),
            [Linear, arrival, Hold]
        );
    }

    // Point (Position) keys are unchanged: a Point Bezier into a Hold key is
    // unprobed, so the same curve into one still exports with its in-handle.
    let mut sequence = video_sequence();
    let point = |seconds: i64, value, easing| PrPointKeyframe {
        source_ticks: seconds * TICKS,
        value,
        easing,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    let arrival = CubicBezier {
        x1: 0.2,
        y1: 0.1,
        x2: 0.8,
        y2: 0.9,
    };
    let keys = vec![
        point(0, [0.25, 0.5], Linear),
        point(2, [0.75, 0.75], arrival),
        point(4, [0.5, 0.5], Hold),
    ];
    sequence.video_tracks[0]
        .clip_mut(0)
        .animations
        .push(PrPropertyAnimation::Position(keys.clone()));
    let (project, omissions) = convert_with_omissions(project_document(&sequence)).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Position)
        .unwrap()
        .point_keys()
        .unwrap()
        .to_vec();
    assert_eq!(exported, keys);
}

#[test]
fn a_linear_wipe_curve_into_a_key_that_starts_a_hold_stops_the_export_like_its_other_unholdable_keys(
) {
    use crate::{
        schema::{PrKeyframeEasing, PrLinearWipe, PrScalarKeyframe},
        tests::support::{project_document, video_sequence},
    };
    let key = |seconds: i64, value, easing| PrScalarKeyframe {
        source_ticks: seconds * TICKS,
        value,
        easing,
    };
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).linear_wipe = Some(PrLinearWipe {
        initial_completion: 100.0,
        completion: vec![
            key(0, 100.0, PrKeyframeEasing::Linear),
            key(
                1,
                50.0,
                PrKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: 0.1,
                    x2: 0.8,
                    y2: 0.9,
                },
            ),
            key(2, 0.0, PrKeyframeEasing::Hold),
        ],
        angle_degrees: 270,
        feather: 5.0,
    });
    let error = convert_with_omissions(project_document(&sequence)).unwrap_err();
    assert!(
        error.to_string().ends_with("unsupported conversion: Linear Wipe completion cubic easing into a key that starts a Hold cannot keep its in-handle, which Premiere ignores"),
        "{error}"
    );
}

/// A clip's effect stack: a static blur, then a blur whose Blurriness is 0,
/// 20 and 40 at 0, 1 and 2 s, where the key at 1 s starts a Hold and
/// `arrival` eases into it, then another static blur.
fn blur_stack_with_an_arrival_into_a_hold(
    arrival: crate::schema::PrKeyframeEasing,
) -> Vec<crate::schema::PrEffect> {
    use crate::schema::{
        PrEffect, PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, PrGaussianBlur,
        PrKeyframeEasing::{Hold, Linear},
        PrScalarKeyframe, GAUSSIAN_BLUR_BLURRINESS,
    };
    let blur = |blurriness, repeat_edge_pixels, animations| PrEffect {
        enabled: true,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels,
        }),
        animations,
    };
    let key = |seconds: i64, value, easing| PrScalarKeyframe {
        source_ticks: seconds * TICKS,
        value,
        easing,
    };
    let keyed_blurriness = PrEffectParamAnimation {
        param: &GAUSSIAN_BLUR_BLURRINESS,
        keys: PrEffectParamKeys::Scalar(vec![
            key(0, 0.0, Linear),
            key(1, 20.0, arrival),
            key(2, 40.0, Hold),
        ]),
    };
    // A keyed parameter's static value is its first key's.
    vec![
        blur(25.0, true, Vec::new()),
        blur(0.0, false, vec![keyed_blurriness]),
        blur(10.0, false, Vec::new()),
    ]
}

/// The exported occurrence of `video_sequence`'s clip with `effects`, and the
/// export's omissions.
fn exported_with_effects(
    effects: Vec<crate::schema::PrEffect>,
) -> (crate::schema::PrVideoOccurrence, Vec<crate::Omission>) {
    use crate::tests::support::{project_document, video_sequence};
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).effects = effects;
    let (project, omissions) = convert_with_omissions(project_document(&sequence)).unwrap();
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .clone();
    (clip, omissions)
}

#[test]
fn a_blurriness_curve_into_a_key_that_starts_a_hold_omits_only_its_effect() {
    use crate::schema::PrKeyframeEasing::CubicBezier;
    let expected_omissions = [crate::Omission {
        scope: crate::OmissionScope::Feature,
        kind: crate::OmissionKind::Omitted,
        record: "layer 1 (\"Premiere video 1\")".to_owned(),
        reason: "effects: gaussianBlur effect 2 was not exported: unsupported conversion: blurriness cubic easing into a key that starts a Hold cannot keep its in-handle, which Premiere ignores".to_owned(),
    }];
    // Premiere ignores the in-handle of the key at 1 s. A curve that bends
    // into the key is reported, and so are curves that only nearly arrive at
    // (1, 1) or nearly lie on the straight line, because the comparison is
    // exact.
    for arrival in [
        CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 0.8,
            y2: 0.9,
        },
        CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 1.0 - 1e-12,
            y2: 1.0,
        },
        CubicBezier {
            x1: 0.3,
            y1: 0.3,
            x2: 0.7,
            y2: 0.7 + 1e-12,
        },
    ] {
        let stack = blur_stack_with_an_arrival_into_a_hold(arrival);
        let (clip, omissions) = exported_with_effects(stack.clone());
        assert_eq!(omissions, expected_omissions, "{arrival:?}");
        // The whole keyed blur is omitted. The clip keeps its range and the
        // blurs around it, in stack order.
        assert_eq!(
            clip.effects,
            [
                current_blur_export(stack[0].clone()),
                current_blur_export(stack[2].clone())
            ],
            "{arrival:?}"
        );
        assert_eq!(
            [
                clip.start_ticks,
                clip.end_ticks,
                clip.in_ticks,
                clip.out_ticks
            ],
            [0, 5 * TICKS, 0, 5 * TICKS],
            "{arrival:?}"
        );
    }
}

#[test]
fn blurriness_keys_arriving_at_a_hold_key_as_premiere_draws_them_export_unchanged() {
    use crate::schema::PrKeyframeEasing::{CubicBezier, Hold, Linear};
    // A curve that already arrives at (1, 1), a straight one, Linear and Hold
    // are drawn as written into the key at 1 s that starts a Hold.
    for arrival in [
        CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 1.0,
            y2: 1.0,
        },
        CubicBezier {
            x1: 0.3,
            y1: 0.3,
            x2: 0.7,
            y2: 0.7,
        },
        Linear,
        Hold,
    ] {
        let stack = blur_stack_with_an_arrival_into_a_hold(arrival);
        let (clip, omissions) = exported_with_effects(stack.clone());
        assert!(omissions.is_empty(), "{arrival:?}: {omissions:?}");
        let expected: Vec<_> = stack.into_iter().map(current_blur_export).collect();
        assert_eq!(clip.effects, expected, "{arrival:?}");
    }
}

#[test]
fn linear_wipe_keys_arriving_at_a_hold_key_read_back_natively_or_stop_the_export() {
    use crate::{
        schema::{
            PrKeyframeEasing::{self, CubicBezier, Hold, Linear},
            PrLinearWipe, PrScalarKeyframe,
        },
        tests::support::{project_document, video_sequence},
    };
    // Completion 100 at 0 s, 50 at 1 s and 0 at 2 s, where the key at 1 s
    // starts a Hold and `arrival` eases into it.
    let completion = |arrival: PrKeyframeEasing| {
        let key = |seconds: i64, value, easing| PrScalarKeyframe {
            source_ticks: seconds * TICKS,
            value,
            easing,
        };
        vec![
            key(0, 100.0, Linear),
            key(1, 50.0, arrival),
            key(2, 0.0, Hold),
        ]
    };
    let exported = |arrival| {
        let mut sequence = video_sequence();
        sequence.video_tracks[0].clip_mut(0).linear_wipe = Some(PrLinearWipe {
            initial_completion: 100.0,
            completion: completion(arrival),
            angle_degrees: 270,
            feather: 5.0,
        });
        convert_with_omissions(project_document(&sequence))
    };
    let zero_length = CubicBezier {
        x1: 0.2,
        y1: 0.1,
        x2: 1.0,
        y2: 1.0,
    };
    // Each arrival that Premiere draws as written exports, and the written
    // project reads back with the wipe's times, values and easing. The reader
    // ignores the in-handle of a key that starts a Hold, as Premiere does, so
    // the straight curve reads back as the same line arriving at (1, 1).
    for (arrival, read_back) in [
        (zero_length, zero_length),
        (
            CubicBezier {
                x1: 0.3,
                y1: 0.3,
                x2: 0.7,
                y2: 0.7,
            },
            CubicBezier {
                x1: 0.3,
                y1: 0.3,
                x2: 1.0,
                y2: 1.0,
            },
        ),
        (Linear, Linear),
        (Hold, Hold),
    ] {
        let (project, omissions) = exported(arrival).unwrap();
        assert!(omissions.is_empty(), "{arrival:?}: {omissions:?}");
        let project = write_and_load_with_crate_reader(project);
        let wipe = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap()
            .linear_wipe
            .clone()
            .unwrap_or_else(|| panic!("{arrival:?}: the wipe did not read back"));
        assert_eq!(
            (wipe.initial_completion, wipe.angle_degrees, wipe.feather),
            (100.0, 270, 5.0),
            "{arrival:?}"
        );
        assert_eq!(wipe.completion, completion(read_back), "{arrival:?}");
    }
    // A curve that only nearly arrives at (1, 1) or nearly lies on the line
    // stops the export, as the wipe's other keys that Premiere cannot hold do,
    // so no clip exports without its wipe.
    for arrival in [
        CubicBezier {
            x1: 0.2,
            y1: 0.1,
            x2: 1.0 - 1e-12,
            y2: 1.0,
        },
        CubicBezier {
            x1: 0.3,
            y1: 0.3,
            x2: 0.7,
            y2: 0.7 + 1e-12,
        },
    ] {
        let Err(error) = exported(arrival) else {
            panic!("{arrival:?}: the export did not stop");
        };
        assert!(
            error.to_string().ends_with("unsupported conversion: Linear Wipe completion cubic easing into a key that starts a Hold cannot keep its in-handle, which Premiere ignores"),
            "{arrival:?}: {error}"
        );
    }
}

#[test]
fn edited_fx_scale_keys_export_on_the_source_clock() {
    use crate::{
        schema::{PrAnimatedProperty, PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe},
        tests::support::{project_document, video_sequence},
    };
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.animations.push(PrPropertyAnimation::UniformScale(vec![
        PrScalarKeyframe {
            source_ticks: 0,
            value: 100.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 2 * TICKS,
            value: 150.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 7 * TICKS,
            value: 200.0,
            easing: PrKeyframeEasing::Linear,
        },
    ]));
    let mut wire = project_document(&sequence);
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        assert!(matches!(
            entry["target"]["propertyType"].as_str(),
            Some("scaleX" | "scaleY")
        ));
        let keys = entry["animator"]["keyframes"].as_array_mut().unwrap();
        assert_eq!(keys[0]["layerTime"], -1000);
        assert_eq!(keys[1]["layerTime"], 1000);
        assert_eq!(keys[2]["layerTime"], 6000);
        keys[1]["value"]["value"] = json!(175.0);
        keys[1]["easing"] = json!({"type":"hold"});
        keys[2]["easing"] = json!({"type":"cubicBezier","x1":0.2,"y1":0.1,"x2":0.8,"y2":0.9});
    }
    let converted = convert(wire).unwrap();
    let clip = converted
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.animations.len(), 1);
    let scale = &clip.animations[0];
    assert_eq!(scale.property(), PrAnimatedProperty::UniformScale);
    let expected = [
        (0, 100.0, PrKeyframeEasing::Linear),
        (2 * TICKS, 175.0, PrKeyframeEasing::Hold),
        (
            7 * TICKS,
            200.0,
            PrKeyframeEasing::CubicBezier {
                x1: 0.2,
                y1: 0.1,
                x2: 0.8,
                y2: 0.9,
            },
        ),
    ];
    assert_eq!(
        scale
            .keys()
            .iter()
            .map(|key| (key.source_ticks, key.value, key.easing))
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn duplicated_videos_can_share_one_canonical_linear_wipe_guide() {
    use crate::{
        schema::{PrKeyframeEasing, PrLinearWipe, PrScalarKeyframe, TICKS},
        tests::support::{project_document, video_sequence},
    };

    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).linear_wipe = Some(PrLinearWipe {
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
    let mut wire = project_document(&sequence);
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    let video_index = layers
        .iter()
        .position(|layer| layer["type"] == "Video")
        .unwrap();
    let mut duplicate = layers[video_index].clone();
    fn replace_item_ids(value: &mut Value, next_id: &mut u64) {
        match value {
            Value::Object(fields) => {
                if let Some(id) = fields.get_mut("id") {
                    *id = match id {
                        Value::String(_) => Value::String(format!("duplicate-{next_id}")),
                        _ => Value::from(*next_id),
                    };
                    *next_id += 1;
                }
                for child in fields.values_mut() {
                    replace_item_ids(child, next_id);
                }
            }
            Value::Array(items) => {
                for item in items {
                    replace_item_ids(item, next_id);
                }
            }
            _ => {}
        }
    }
    replace_item_ids(&mut duplicate, &mut 100);
    layers.insert(video_index + 1, duplicate);

    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let wipes: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .map(|occurrence| occurrence.linear_wipe.as_ref().unwrap())
        .collect();
    assert_eq!(wipes.len(), 2);
    assert!(wipes.iter().all(|wipe| {
        wipe.angle_degrees == 270
            && wipe.feather == 5.0
            && wipe.completion.len() == 2
            && wipe.completion[0].value == 100.0
            && wipe.completion[1].value == 0.0
    }));
}

/// [`two_clips`] whose first video (layer 1, 0 to 2 s) has `mask` with the
/// guide layer 10: a canonical Crop guide, or a canonical 270-degree Linear
/// Wipe guide with its completion keys. The second video is the sibling.
fn two_clips_with_mask(wipe: bool, edit: impl FnOnce(&mut Value)) -> Value {
    let mut wire = two_clips();
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    let video = &mut layers[0];
    video["masks"] = json!([{"id": 11, "mode": "add", "layer": 10, "feather": [0.0, 0.0]}]);
    let mut guide = json!({
        "type": "Rect",
        "id": 10,
        "name": "Premiere Crop guide 1",
        "activeRange": (*crate::test_support::layer_range(video)).clone(),
        "transform": video["transform"].clone(),
        "rect": {"position": [0.0, 162.0], "size": [1920.0, 918.0], "fillColor": [0, 0, 0, 1]},
    });
    if wipe {
        guide["name"] = json!("Premiere Linear Wipe guide 1");
        guide["rect"] = json!({"size": [1920.0, 1080.0], "fillColor": [0, 0, 0, 1]});
        guide["transform"]["scale"] = json!([0.0, 100.0]);
        wire["composition"]["dynamics"] = json!({"entries": [{
            "target": {"kind": "layer", "layerId": 10, "propertyType": "scaleX"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "start", "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "linear"}},
                {"id": "end", "layerTime": 1000, "value": {"type": "float", "value": 100.0}, "easing": {"type": "linear"}}
            ]}
        }]});
    }
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, guide);
    edit(&mut wire);
    wire
}

#[test]
fn mask_guides_export_as_the_crop_or_linear_wipe_that_their_shape_draws() {
    let rename = |name: &'static str| {
        move |wire: &mut Value| wire["composition"]["layers"][1]["name"] = json!(name)
    };
    let crop = |left, top, edge_feather| PrStaticCrop {
        left,
        top,
        right: 0.0,
        bottom: 0.0,
        edge_feather,
    };
    // The feathered Crop of this sequence-sized source's whole frame: its guide
    // has a Linear Wipe guide's rectangle and one Scale track, but repeats its
    // video's (which export does not write).
    let feathered = two_clips_with_mask(false, |wire| {
        let layers = &mut wire["composition"]["layers"];
        layers[0]["masks"][0]["feather"] = json!([5.0, 5.0]);
        layers[1]["rect"] = json!({"size": [1920.0, 1080.0], "fillColor": [0, 0, 0, 1]});
        let linear = json!({"type": "linear"});
        wire["composition"]["dynamics"] = json!({"entries": [
            two_layer_keys(1, "scaleX", [100.0, 50.0], linear.clone()),
            two_layer_keys(10, "scaleX", [100.0, 50.0], linear),
        ]});
    });
    // Guide and video only translate: the rectangle at (380, 62) of the guide
    // is at (480, 162) of the video's frame.
    let translated = two_clips_with_mask(false, |wire| {
        let layers = &mut wire["composition"]["layers"];
        layers[0]["transform"]["anchorPoint"] = json!([960.0, 540.0]);
        layers[0]["transform"]["position"] = json!([1060.0, 590.0]);
        layers[1]["transform"]["anchorPoint"] = json!([10.0, 20.0]);
        layers[1]["transform"]["position"] = json!([210.0, 170.0]);
        layers[1]["rect"]["position"] = json!([380.0, 62.0]);
        layers[1]["rect"]["size"] = json!([1440.0, 918.0]);
    });
    // Completion 100 to 0 over the first second from source In 1 s.
    let wipe = Some((270, 100.0, vec![(TICKS, 100.0), (2 * TICKS, 0.0)]));
    for (wire, expected_crop, expected_wipe) in [
        (
            two_clips_with_mask(true, rename("Wipe")),
            PrStaticCrop::default(),
            wipe,
        ),
        (
            two_clips_with_mask(false, rename("Premiere Linear Wipe guide 1")),
            crop(0.0, 15.0, 0.0),
            None,
        ),
        (feathered, crop(0.0, 0.0, 5.0), None),
        (translated, crop(25.0, 15.0, 0.0), None),
    ] {
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let clip = first_clip(&project);
        let wipe = clip.linear_wipe.as_ref().map(|wipe| {
            let keys = wipe
                .completion
                .iter()
                .map(|key| (key.source_ticks, key.value));
            (wipe.angle_degrees, wipe.initial_completion, keys.collect())
        });
        assert_eq!(
            (clip.start_ticks, clip.crop, wipe),
            (0, expected_crop, expected_wipe),
            "{omissions:?}"
        );
    }
}

#[test]
fn a_crop_guide_without_its_videos_motion_keys_omits_the_clip() {
    // The video keys Rotation and its guide does not: the Crop would move
    // against the video. The video's own keys get the generic report.
    let rotation = json!({
        "target": {"kind": "layer", "layerId": 1, "propertyType": "rotation"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "a", "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "linear"}},
            {"id": "b", "layerTime": 1000, "value": {"type": "float", "value": 20.0}, "easing": {"type": "linear"}}
        ]}
    });
    let wire = two_clips_with_mask(false, |wire| {
        wire["composition"]["dynamics"] = json!({"entries": [rotation]});
    });
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert_eq!(
        omissions,
        [
            crate::Omission {
                scope: crate::OmissionScope::Occurrence,
                kind: crate::OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".into(),
                reason: "masks cannot be exported: the Crop guide's transform or Motion keys differ from its video's; occurrence omitted".into(),
            },
            crate::Omission {
                scope: crate::OmissionScope::Feature,
                kind: crate::OmissionKind::Omitted,
                record: "layer 1".into(),
                reason: "animation on an omitted or unsupported layer was not exported".into(),
            },
        ]
    );
    assert_eq!(first_clip(&project).start_ticks, 3 * TICKS);
}

/// A static Gaussian Blur model.
fn blur_effect(blurriness: f64) -> crate::schema::PrEffect {
    crate::schema::PrEffect {
        enabled: true,
        params: crate::schema::PrEffectParams::GaussianBlur(crate::schema::PrGaussianBlur {
            blurriness,
            repeat_edge_pixels: false,
        }),
        animations: Vec::new(),
    }
}

/// A sequence whose first clip (0 to 2 s) stages: a blur (40) that applies
/// before a Crop (Top 15) on a moved, half-transparent clip with Rotation keys.
/// A plain clip of the same source follows at 3 to 5 s.
fn staged_sequence() -> crate::format::PrSequence {
    use crate::{
        schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe, PrVideoItem},
        tests::support::video_sequence,
    };
    let mut sequence = video_sequence();
    let mut plain = sequence.video_tracks[0].clip(0).clone();
    (plain.start_ticks, plain.end_ticks) = (3 * TICKS, 5 * TICKS);
    (plain.in_ticks, plain.out_ticks) = (3 * TICKS, 5 * TICKS);
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.end_ticks, clip.out_ticks) = (2 * TICKS, 2 * TICKS);
    clip.crop.top = 15.0;
    clip.transform.scale = [80.0; 2];
    clip.transform.rotation = 0.0;
    clip.transform.position = [0.25, 0.75];
    clip.opacity = 50.0;
    clip.animations = vec![PrPropertyAnimation::Rotation(
        [(0, 0.0), (TICKS, 20.0)]
            .map(|(source_ticks, value)| PrScalarKeyframe {
                source_ticks,
                value,
                easing: PrKeyframeEasing::Linear,
            })
            .into(),
    )];
    clip.effects = vec![blur_effect(40.0)];
    clip.effects_above_mask = 1;
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(plain));
    sequence
}

/// [`staged_sequence`] with a 270-degree Linear Wipe (Transition Completion
/// 100 to 0 over the first second, Feather 5) in place of its Crop.
fn staged_wipe_sequence() -> crate::format::PrSequence {
    use crate::schema::{PrKeyframeEasing, PrLinearWipe, PrScalarKeyframe};
    let mut sequence = staged_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.crop = Default::default();
    clip.linear_wipe = Some(PrLinearWipe {
        initial_completion: 100.0,
        completion: [(0, 100.0), (TICKS, 0.0)]
            .map(|(source_ticks, value)| PrScalarKeyframe {
                source_ticks,
                value,
                easing: PrKeyframeEasing::Linear,
            })
            .into(),
        angle_degrees: 270,
        feather: 5.0,
    });
    sequence
}

#[test]
fn edited_stage_group_exports_as_one_clip_and_rereads_in_order() {
    let source = staged_sequence();
    let unedited = crate::tests::support::project_document(&source);
    // Edits: Blurriness 40 to 60 on the video, the Crop's top edge 15 to 25%.
    let mut edited = unedited.clone();
    let stage = &mut edited["composition"]["layers"][0];
    assert_eq!(stage["type"], "Group");
    stage["layers"][0]["effects"][0]["effect"]["blurriness"] = json!(60.0);
    stage["layers"][1]["rect"]["position"] = json!([0.0, 270.0]);
    stage["layers"][1]["rect"]["size"] = json!([1920.0, 810.0]);
    for (wire, blurriness, top) in [(unedited, 40.0, 15.0), (edited, 60.0, 25.0)] {
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        // The group adds no native level.
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.nest_occurrences().count(), 0);
        assert_eq!(sequence.video_occurrences().count(), 2);
        let clip = first_clip(&project);
        let expected = source.video_tracks[0].clip(0);
        assert_eq!(
            (
                clip.start_ticks,
                clip.end_ticks,
                clip.in_ticks,
                clip.out_ticks
            ),
            (0, 2 * TICKS, 0, 2 * TICKS)
        );
        assert_eq!(
            (clip.transform, clip.opacity, clip.enabled),
            (expected.transform, 50.0, true)
        );
        // The native chain applies the blur, then the Crop; it reads back so.
        let reread = write_and_load_with_crate_reader(project);
        let clip = first_clip(&reread);
        assert_eq!(clip.effects, [exported_blur(true, blurriness, false)]);
        assert_eq!(clip.effects_above_mask, 1);
        assert_eq!(clip.crop.top, top);
        let keys: Vec<_> = clip.animations[0]
            .keys()
            .iter()
            .map(|key| (key.source_ticks, key.value))
            .collect();
        assert_eq!(keys, [(0, 0.0), (TICKS, 20.0)]);
    }
}

#[test]
fn stage_group_keeps_constant_speed_reverse_and_enable_on_export() {
    for (playback_rate, enabled) in [(2.0, true), (-1.0, false)] {
        let mut sequence = staged_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.animations.clear();
        clip.playback_rate = playback_rate;
        // Native reverse bounds count back from the 10 s media end.
        (clip.in_ticks, clip.out_ticks) = if playback_rate > 0.0 {
            (TICKS, 5 * TICKS)
        } else {
            (TICKS, 3 * TICKS)
        };
        clip.enabled = enabled;
        let expected = sequence.video_tracks[0].clip(0).clone();
        let wire = crate::tests::support::project_document(&sequence);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let clip = first_clip(&project);
        assert_eq!(
            (
                clip.playback_rate,
                clip.in_ticks,
                clip.out_ticks,
                clip.enabled
            ),
            (
                playback_rate,
                expected.in_ticks,
                expected.out_ticks,
                enabled
            )
        );
        assert_eq!(clip.effects_above_mask, 1);
    }
}

#[test]
fn stage_group_inside_a_nest_exports_as_one_clip_of_the_inner_sequence() {
    use crate::tests::support::{nested_sequence, project_document_with_media};
    let (mut outer, media) = nested_sequence();
    for nest in &mut outer.video_tracks[1].nests {
        let clip = nest.sequence.video_tracks[0].clip_mut(0);
        clip.crop.top = 15.0;
        clip.effects = vec![blur_effect(40.0)];
        clip.effects_above_mask = 1;
    }
    let document =
        EditableFxCompositionDocument::from_json_value(project_document_with_media(&outer, &media))
            .unwrap();
    let facts = ["premiere-video-1", "premiere-video-2"]
        .into_iter()
        .flat_map(|asset| {
            source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND)
                .into_values()
                .map(move |facts| (asset.to_owned(), facts))
        })
        .collect();
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
    assert!(omissions.is_empty(), "{omissions:?}");
    // Each nest's inner sequence holds the staged clip as one clip.
    let nests: Vec<_> = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .collect();
    assert_eq!(nests.len(), 2);
    for nest in nests {
        let staged = nest.sequence.video_occurrences().next().unwrap();
        assert_eq!(staged.crop.top, 15.0);
        assert_eq!(staged.effects, [exported_blur(true, 40.0, false)]);
        assert_eq!(staged.effects_above_mask, 1);
        assert_eq!(nest.sequence.nest_occurrences().count(), 0);
    }
}

#[test]
fn stage_groups_that_one_clip_cannot_carry_export_as_nests() {
    let base = crate::tests::support::project_document(&staged_sequence());
    // Layer 5 is the stage group, 1 its video and 3 its Crop guide.
    let field = |pointer: &'static str, value: Value| {
        move |wire: &mut Value| *wire.pointer_mut(pointer).unwrap() = value.clone()
    };
    let video_key = |wire: &mut Value| {
        wire["composition"]["dynamics"]["entries"][0]["target"]["layerId"] = json!(1);
    };
    let video = "/composition/layers/0/layers/0";
    let edits = [
        (
            Box::new(video_key) as Box<dyn Fn(&mut Value)>,
            "the video has keys",
        ),
        (
            Box::new(field(
                "/composition/layers/0/layers/0/transform/opacity",
                json!(50.0),
            )),
            "the video's transform or opacity is not the identity",
        ),
        (
            Box::new(move |wire: &mut Value| {
                wire.pointer_mut(video).unwrap()["blendMode"] = json!("multiply");
            }),
            "the video has a blend mode",
        ),
        (
            Box::new(move |wire: &mut Value| {
                wire.pointer_mut(video).unwrap()["cornerRadius"] = json!(10.0);
            }),
            "the video has a corner radius",
        ),
        (
            Box::new(move |wire: &mut Value| {
                wire.pointer_mut(video).unwrap()["motionBlur"] = json!(true);
            }),
            "the video has motion blur",
        ),
        (
            Box::new(move |wire: &mut Value| {
                wire.pointer_mut(video).unwrap()["isHidden"] = json!(true);
            }),
            "the video is hidden",
        ),
        (
            Box::new(move |wire: &mut Value| {
                wire.pointer_mut(video).unwrap()["masks"] =
                    json!([{"id": 20, "mode": "add", "layer": 3}]);
            }),
            "the video has masks",
        ),
        (
            Box::new(|wire: &mut Value| {
                wire["composition"]["layers"][0]["effects"] = json!([{"id": 9, "effect": {"type": "mosaic", "horizontalBlocks": 10.0, "verticalBlocks": 20.0}}]);
            }),
            "the group has effects",
        ),
        // The last five stay omitted.
        (
            Box::new(field(
                "/composition/layers/0/layers/0/playback/inputRange/duration",
                json!(1000),
            )),
            "stage group was not exported as one clip: the video's range is not the group's",
        ),
        // A track matte beside the group's Crop; its source is no sibling, so
        // the other clip stays visible content.
        (
            Box::new(|wire: &mut Value| {
                wire["composition"]["layers"][0]["trackMatte"] =
                    json!({"mode": "alpha", "layer": 99});
            }),
            "group was not exported as a nested sequence: a track matte with masks on one clip is not converted; FX's intersection of the two is unverified against Premiere",
        ),
        // Without its guide child the group has no stage shape.
        (
            Box::new(|wire: &mut Value| {
                wire["composition"]["layers"][0]["layers"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }),
            "group was not exported as a nested sequence: the mask guide is not a rectangle or shape beside the group",
        ),
        // A flip has no Motion form on the clip or on the nest's placement.
        (
            Box::new(field(
                "/composition/layers/0/transform/scale",
                json!([-80.0, 80.0]),
            )),
            "group was not exported as a nested sequence: flip (negative scale) was not exported",
        ),
        // A ramp has no native form on the clip or on the nest's inner clip.
        (
            Box::new(move |wire: &mut Value| {
                wire.pointer_mut(video).unwrap()["playback"] = crate::test_support::remapped_playback(crate::test_support::layer_range(wire.pointer_mut(video).unwrap()).clone(), json!({"keyframes": [
                    {"id": "a", "time": 0, "value": 0, "easing": {"type": "linear"}},
                    {"id": "b", "time": 1000, "value": 500, "easing": {"type": "linear"}},
                    {"id": "c", "time": 2000, "value": 2000, "easing": {"type": "linear"}}
                ], "before": "inactive", "after": "inactive"}));
            }),
            "group with no exportable video was not exported",
        ),
    ];
    for (index, (edit, reason)) in edits.into_iter().enumerate() {
        let mut wire = base.clone();
        edit(&mut wire);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let sequence = project.single_sequence().unwrap();
        if index < 8 {
            // The nest's placement carries the group's Opacity and Crop, and
            // its sequence the video.
            assert!(
                omissions
                    .iter()
                    .all(|omission| omission.scope == crate::OmissionScope::Feature),
                "{reason}: {omissions:?}"
            );
            let nest = sequence.nest_occurrences().next().unwrap();
            assert_eq!(
                (
                    nest.timeline_ticks(),
                    nest.opacity,
                    nest.crop.top,
                    nest.sequence.video_occurrences().count()
                ),
                (0..2 * TICKS, 50.0, 15.0, 1),
                "{reason}"
            );
        } else {
            assert!(
                omissions.contains(&crate::Omission {
                    scope: crate::OmissionScope::Occurrence,
                    kind: crate::OmissionKind::Omitted,
                    record: "layer 5 (\"Premiere stage 1\")".into(),
                    reason: reason.into(),
                }),
                "{reason}: {omissions:?}"
            );
            assert_eq!(sequence.nest_occurrences().count(), 0, "{reason}");
        }
        assert_eq!(first_clip(&project).start_ticks, 3 * TICKS, "{reason}");
    }
}

/// A sequence whose one clip (0 to 2 s, from source In 0.5 s) carries clip
/// D's keyed Transform of the run E11 fixture with Position keys added and a
/// non-centred anchor, under Motion Scale 50 and Rotation 30, with a blur
/// applied before it.
fn transform_stage_sequence() -> crate::format::PrSequence {
    use crate::{
        schema::{
            PrEffectParamAnimation, PrEffectParamKeys, PrKeyframeEasing, PrPointKeyframe,
            PrScalarKeyframe, PrTransform, TRANSFORM_POSITION, TRANSFORM_ROTATION,
            TRANSFORM_SCALE_HEIGHT,
        },
        tests::support::{transform_effect, video_sequence, DEFAULT_PR_TRANSFORM},
    };
    let key = |source_ticks, value, easing| PrScalarKeyframe {
        source_ticks,
        value,
        easing,
    };
    use PrKeyframeEasing::{Hold, Linear};
    let mut sequence = video_sequence();
    sequence.timeline_end_ticks = 2 * TICKS;
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.end_ticks, clip.in_ticks, clip.out_ticks) = (2 * TICKS, TICKS / 2, 5 * TICKS / 2);
    clip.transform.scale = [50.0; 2];
    clip.transform.rotation = 30.0;
    clip.effects = vec![
        blur_effect(40.0),
        transform_effect(
            PrTransform {
                anchor_point: [0.75, 0.5],
                position: [0.25, 0.5],
                uniform_scale: true,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![
                PrEffectParamAnimation {
                    param: &TRANSFORM_POSITION,
                    keys: PrEffectParamKeys::Point(
                        [(TICKS / 2, 0.25), (3 * TICKS / 2, 0.75)]
                            .map(|(source_ticks, x)| PrPointKeyframe {
                                source_ticks,
                                value: [x, 0.5],
                                easing: Linear,
                                spatial_in_tangent: None,
                                spatial_out_tangent: None,
                            })
                            .into(),
                    ),
                },
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
            ],
        ),
    ];
    sequence
}

#[test]
fn transform_stage_groups_export_as_one_clip_ending_in_a_transform() {
    use crate::{
        schema::{PrEffectParams, PrTransform},
        tests::support::{current_blur_export, DEFAULT_PR_TRANSFORM},
    };
    let source = transform_stage_sequence();
    let wire = crate::tests::support::project_document(&source);
    let stage = &wire["composition"]["layers"][0];
    assert!(
        stage["type"] == "Group" && stage.get("masks").is_none(),
        "{stage}"
    );
    let (project, omissions) = convert_with_omissions(wire.clone()).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 0);
    let expected = source.video_tracks[0].clip(0);
    let clip = first_clip(&project);
    // The clip takes the group's Motion and range; its chain is the blur (as
    // the current Gaussian Blur), then the Transform, whose values and keys
    // are the video's transform and tracks: Scale Height on both axes written
    // as Uniform Scale, Position back in source fractions.
    assert_eq!(
        (clip.transform, clip.in_ticks, clip.effects_above_mask),
        (expected.transform, TICKS / 2, 0)
    );
    let exported: Vec<_> = expected
        .effects
        .iter()
        .cloned()
        .map(current_blur_export)
        .collect();
    assert_eq!(clip.effects, exported);
    let reread = write_and_load_with_crate_reader(project);
    assert_eq!(first_clip(&reread).effects, exported);
    // Static videos: unequal axes write Uniform Scale off, equal axes write
    // it on whichever the source had; a skew at rotation 0 writes Skew with
    // Skew Axis 90° more than the skew axis.
    for (scale, [skew, skew_axis], uniform_scale, [scale_height, scale_width], native_axis) in [
        ([70.0, 100.0], [0.0, 0.0], false, [100.0, 70.0], 0.0),
        ([100.0, 100.0], [0.0, 0.0], true, [100.0, 100.0], 0.0),
        ([100.0, 100.0], [30.0, -45.0], true, [100.0, 100.0], 45.0),
    ] {
        let mut wire = wire.clone();
        let video = &mut wire["composition"]["layers"][0]["layers"][0];
        video["transform"]["scale"] = json!(scale);
        video["transform"]["skew"] = json!(skew);
        video["transform"]["skewAxis"] = json!(skew_axis);
        wire["composition"]["dynamics"]["entries"] = json!([]);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let PrEffectParams::Transform(transform) = first_clip(&project).effects[1].params else {
            panic!("expected a Transform");
        };
        assert_eq!(
            transform,
            PrTransform {
                anchor_point: [0.75, 0.5],
                position: [0.25, 0.5],
                uniform_scale,
                scale_height,
                scale_width,
                skew,
                skew_axis: native_axis,
                ..DEFAULT_PR_TRANSFORM
            }
        );
    }
}

#[test]
fn transform_skew_axis_round_trips_through_the_fx_axis_90_less() {
    use crate::{
        schema::{PrEffectParams, PrTransform},
        tests::support::{project_document, transform_effect, DEFAULT_PR_TRANSFORM},
    };
    // The gate's measured vector (Skew 30 at Skew Axis −30), E11's control
    // (Skew Axis 45, which fits both conventions), the formula's unmeasured
    // Skew Axis 0 under a skew, and the unskewed default.
    for (skew, skew_axis, fx_skew_axis) in [
        (30.0, -30.0, -120.0),
        (30.0, 45.0, -45.0),
        (30.0, 0.0, -90.0),
        (0.0, 0.0, 0.0),
    ] {
        let mut sequence = transform_stage_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.effects = vec![transform_effect(
            PrTransform {
                skew,
                skew_axis,
                ..DEFAULT_PR_TRANSFORM
            },
            vec![],
        )];
        let wire = project_document(&sequence);
        assert_eq!(
            wire["composition"]["layers"][0]["layers"][0]["transform"]["skewAxis"],
            json!(fx_skew_axis),
            "Skew Axis {skew_axis}"
        );
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let reread = write_and_load_with_crate_reader(project);
        let PrEffectParams::Transform(transform) = first_clip(&reread).effects[0].params else {
            panic!("expected a Transform");
        };
        assert_eq!(
            (transform.skew, transform.skew_axis),
            (skew, skew_axis),
            "Skew Axis {skew_axis}"
        );
    }
}

#[test]
fn transform_skew_axes_beyond_premiere_bounds_round_trip_as_the_same_shear() {
    use crate::{schema::PrEffectParams, tests::support::project_document_with_media};
    // A skewed video's `skewAxis` whose Skew Axis, 90° more, would leave
    // Premiere's −32768 to 32767 writes its equivalent from 0° to 180° (the
    // shear repeats every 180°), which reimports a whole number of 180°
    // periods from the edited axis.
    let base = crate::tests::support::project_document(&transform_stage_sequence());
    for (skew_axis, native_axis, reimported) in [(32700.0, 30.0, -60.0), (-32900.0, 130.0, 40.0)] {
        let mut wire = base.clone();
        let video = &mut wire["composition"]["layers"][0]["layers"][0];
        video["transform"]["skew"] = json!(30.0);
        video["transform"]["skewAxis"] = json!(skew_axis);
        wire["composition"]["dynamics"]["entries"] = json!([]);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let reread = write_and_load_with_crate_reader(project);
        let PrEffectParams::Transform(transform) = first_clip(&reread).effects[1].params else {
            panic!("expected a Transform");
        };
        assert_eq!(transform.skew_axis, native_axis, "skewAxis {skew_axis}");
        let back = project_document_with_media(reread.single_sequence().unwrap(), &reread.media);
        let axis = &back["composition"]["layers"][0]["layers"][0]["transform"]["skewAxis"];
        assert_eq!(axis, &json!(reimported), "skewAxis {skew_axis}");
        assert_eq!(
            (skew_axis - reimported) % 180.0,
            0.0,
            "skewAxis {skew_axis}"
        );
    }
}

#[test]
fn transform_stages_that_one_transform_cannot_carry_export_as_nests_like_renamed_ones() {
    let base = crate::tests::support::project_document(&transform_stage_sequence());
    let video = "/composition/layers/0/layers/0";
    let field = |pointer: &'static str, value: Value| {
        move |wire: &mut Value| *wire.pointer_mut(pointer).unwrap() = value.clone()
    };
    // Retarget the video's Rotation track to `property`.
    let retarget = |property: &'static str| {
        move |wire: &mut Value| {
            for entry in wire["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
            {
                if entry["target"]["propertyType"] == "rotation" {
                    entry["target"]["propertyType"] = json!(property);
                }
            }
        }
    };
    // Drop the video's Rotation track and move its second `scaleX` key from
    // 1000 ms to 1500 ms: both scale tracks still run 100 to 200 to 50.
    let scale_x_apart = |wire: &mut Value| {
        let entries = wire["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap();
        entries.retain(|entry| entry["target"]["propertyType"] != "rotation");
        let scale_x = entries
            .iter_mut()
            .find(|entry| entry["target"]["propertyType"] == "scaleX")
            .unwrap();
        let second = &mut scale_x["animator"]["keyframes"][1]["layerTime"];
        assert_eq!(*second, json!(1000));
        *second = json!(1500);
    };
    /// (edit, the nest's own report of what the inner clip loses, or "")
    type Edit<'a> = (Box<dyn Fn(&mut Value) + 'a>, &'a str);
    let edits: [Edit<'_>; 5] = [
        // A renamed group is a nest of one moved clip, as a Premiere nest
        // imports.
        (
            Box::new(field("/composition/layers/0/name", json!("Inner"))),
            "",
        ),
        (Box::new(retarget("anchorPointX")), ""),
        (
            Box::new(move |wire: &mut Value| {
                field("/composition/layers/0/layers/0/transform/skew", json!(20.0))(wire);
                retarget("skew")(wire);
            }),
            "skew was not exported",
        ),
        (
            Box::new(field(
                "/composition/layers/0/layers/0/transform/rotationX",
                json!(20.0),
            )),
            "3D rotation was not exported",
        ),
        // The identity without keys is no Transform stage.
        (
            Box::new(move |wire: &mut Value| {
                let video = wire.pointer_mut(video).unwrap();
                video["transform"] = json!({
                    "anchorPoint": [0.0, 0.0], "position": [0.0, 0.0], "scale": [100.0, 100.0],
                    "rotation": 0.0, "opacity": 100.0,
                });
                wire["composition"]["dynamics"]["entries"] = json!([]);
            }),
            "",
        ),
    ];
    for (index, (edit, lost)) in edits.into_iter().enumerate() {
        let mut wire = base.clone();
        edit(&mut wire);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let sequence = project.single_sequence().unwrap();
        // The nest's placement carries the group's Motion and its sequence
        // the video as one clip with the video's transform as Motion; the
        // nest reports what that Motion cannot carry.
        let nest = sequence
            .nest_occurrences()
            .next()
            .unwrap_or_else(|| panic!("edit {index}: {omissions:?}"));
        assert_eq!(
            (
                nest.timeline_ticks(),
                nest.transform.scale,
                nest.transform.rotation
            ),
            (0..2 * TICKS, [50.0; 2], 30.0),
            "edit {index}"
        );
        let inner = nest.sequence.video_occurrences().next().unwrap();
        assert!(
            inner.effects.iter().all(|effect| !matches!(
                effect.params,
                crate::schema::PrEffectParams::Transform(_)
            )),
            "edit {index}: {:?}",
            inner.effects
        );
        let reasons: Vec<_> = omissions
            .iter()
            .map(|omission| omission.reason.as_str())
            .collect();
        assert!(
            omissions
                .iter()
                .all(|omission| omission.scope == crate::OmissionScope::Feature),
            "edit {index}: {omissions:?}"
        );
        if lost.is_empty() {
            assert!(
                !reasons
                    .iter()
                    .any(|reason| reason.contains("skew") || reason.contains("3D")),
                "edit {index}: {reasons:?}"
            );
        } else {
            assert!(reasons.contains(&lost), "edit {index}: {reasons:?}");
        }
    }
    // Two scale tracks of one range at different key times export as a
    // Transform with Uniform Scale off and both axes keyed.
    let mut wire = base.clone();
    scale_x_apart(&mut wire);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = first_clip(&project);
    let crate::schema::PrEffectParams::Transform(transform) = clip.effects[1].params else {
        panic!("expected a Transform: {:?}", clip.effects);
    };
    assert!(!transform.uniform_scale, "{transform:?}");
    let key_times: Vec<Vec<i64>> = clip.effects[1]
        .animations
        .iter()
        .filter_map(|animation| animation.keys.scalar())
        .map(|keys| keys.iter().map(|key| key.source_ticks).collect())
        .collect();
    // Params order: Scale Height, then Scale Width, on the source clock.
    assert_eq!(
        key_times,
        [
            vec![TICKS, 3 * TICKS / 2, 5 * TICKS / 2],
            vec![TICKS, 2 * TICKS, 5 * TICKS / 2]
        ]
    );
}

#[test]
fn transform_stages_export_approximated_values_with_the_import_warnings() {
    use crate::{
        schema::{PrEffectParams, PrTransform, TRANSFORM_OPACITY},
        tests::support::{
            nested_sequence, project_document_with_media, transform_effect, DEFAULT_PR_TRANSFORM,
        },
        OmissionKind::{Approximated, Omitted},
    };
    let base = crate::tests::support::project_document(&transform_stage_sequence());
    let video = "/composition/layers/0/layers/0";
    let set = |pointer: &'static str, value: Value| {
        move |wire: &mut Value| *wire.pointer_mut(pointer).unwrap() = value.clone()
    };
    let skew = set("/composition/layers/0/layers/0/transform/skew", json!(20.0));
    // The video's Rotation track (0 to 90) becomes an Opacity track, or its
    // `scaleX` track's second key moves from 1000 to 1500 ms (one range, other
    // times: unequal axes).
    let entries = |wire: &mut Value| {
        std::mem::take(
            wire["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap(),
        )
    };
    let opacity_keys = move |wire: &mut Value| {
        let mut tracks = entries(wire);
        for entry in &mut tracks {
            if entry["target"]["propertyType"] == "rotation" {
                entry["target"]["propertyType"] = json!("opacity");
            }
        }
        wire["composition"]["dynamics"]["entries"] = json!(tracks);
    };
    let scale_x_apart = move |wire: &mut Value| {
        let mut tracks = entries(wire);
        tracks.retain(|entry| entry["target"]["propertyType"] != "rotation");
        for entry in &mut tracks {
            if entry["target"]["propertyType"] == "scaleX" {
                entry["animator"]["keyframes"][1]["layerTime"] = json!(1500);
            }
        }
        wire["composition"]["dynamics"]["entries"] = json!(tracks);
    };
    let motion_blur = move |angle: f64, phase: f64| {
        move |wire: &mut Value| {
            wire.pointer_mut(video).unwrap()["motionBlur"] = json!(true);
            wire["composition"]["motionBlur"] =
                json!({"enabled": true, "shutterAngle": angle, "shutterPhase": phase});
        }
    };
    let opacity_error = "blends in linear light in Premiere; converted as sRGB opacity (Oracle run E11 clip A, Transform Opacity 50 with clip Opacity 50: mean error \u{2248} 22 levels, p99 \u{2248} 80)";
    let unmeasured = "converts with FX's composition of skew, rotation and scale; skew with rotation or non-uniform scale is unmeasured (Oracle run E11 measured the shear at Rotation 0 and Scale 100/100 only, T5)";
    let blur = "Transform motion blur (Shutter Angle 180) approximated by FX motion blur";
    let lost = "motion blur was not exported";
    /// (edit, the Transform's Opacity, keyed or not, its shutter checkbox
    /// and Shutter Angle, and the kind and start of every warning)
    type Case<'a> = (
        Box<dyn Fn(&mut Value) + 'a>,
        (f64, bool),
        (bool, f64),
        Vec<(OmissionKind, String)>,
    );
    let cases: [Case<'_>; 8] = [
        (
            Box::new(set(
                "/composition/layers/0/layers/0/transform/opacity",
                json!(50.0),
            )),
            (50.0, false),
            (true, 0.0),
            vec![(Approximated, format!("Transform Opacity 50 {opacity_error}"))],
        ),
        (
            Box::new(opacity_keys),
            (0.0, true),
            (true, 0.0),
            vec![(Approximated, format!("keyed Transform Opacity {opacity_error}"))],
        ),
        // The keyed Rotation, or unequal axes, under a skew.
        (
            Box::new(skew.clone()),
            (100.0, false),
            (true, 0.0),
            vec![(Approximated, format!("Transform Skew 20 with a Rotation {unmeasured}"))],
        ),
        (
            Box::new(move |wire: &mut Value| {
                skew(wire);
                scale_x_apart(wire);
            }),
            (100.0, false),
            (true, 0.0),
            vec![(
                Approximated,
                format!("Transform Skew 20 with unequal Scale Width and Scale Height {unmeasured}"),
            )],
        ),
        // The composition's shutter, which Premiere's Transform has no phase
        // for; beyond the Shutter Angle bounds or on a composition without
        // motion blur the video's motion blur is reported.
        (
            Box::new(motion_blur(180.0, 0.0)),
            (100.0, false),
            (false, 180.0),
            vec![(Approximated, blur.to_owned())],
        ),
        (
            Box::new(motion_blur(180.0, -90.0)),
            (100.0, false),
            (false, 180.0),
            vec![
                (Approximated, blur.to_owned()),
                (
                    Omitted,
                    "motion blur shutter phase -90\u{b0} was not exported: a Transform's own shutter has no phase"
                        .to_owned(),
                ),
            ],
        ),
        (
            Box::new(motion_blur(540.0, 0.0)),
            (100.0, false),
            (true, 0.0),
            vec![(Omitted, lost.to_owned()), (Omitted, lost.to_owned())],
        ),
        (
            Box::new(move |wire: &mut Value| {
                wire.pointer_mut(video).unwrap()["motionBlur"] = json!(true);
            }),
            (100.0, false),
            (true, 0.0),
            vec![(Omitted, lost.to_owned())],
        ),
    ];
    for (index, (edit, (opacity, keyed), shutter, warnings)) in cases.into_iter().enumerate() {
        let mut wire = base.clone();
        edit(&mut wire);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let clip = first_clip(&project);
        let transform = clip.effects.last().unwrap();
        let PrEffectParams::Transform(values) = transform.params else {
            panic!(
                "edit {index}: expected a Transform stage: {:?}",
                clip.effects
            );
        };
        assert_eq!(
            (
                values.opacity,
                transform.keys(&TRANSFORM_OPACITY).is_some(),
                (values.composition_shutter_angle, values.shutter_angle),
            ),
            (opacity, keyed, shutter),
            "edit {index}"
        );
        assert_eq!(
            omissions.len(),
            warnings.len(),
            "edit {index}: {omissions:?}"
        );
        for (omission, (kind, start)) in omissions.iter().zip(&warnings) {
            assert_eq!(omission.kind, *kind, "edit {index}: {}", omission.reason);
            assert!(
                omission.reason.starts_with(start.as_str()),
                "edit {index}: {}",
                omission.reason
            );
        }
    }
    // A stage inside a nest group writes the composition's shutter too, and
    // the composition's motion blur is not reported: the nest's clips share
    // it and whether a stage wrote it. Here the
    // inner clip of `nested_sequence()`'s first nest moves and blurs at 180.
    let (mut outer, media) = nested_sequence();
    let inner = &mut outer.video_tracks[1].nests[0].sequence;
    inner.video_tracks[0].clip_mut(0).effects = vec![transform_effect(
        PrTransform {
            position: [0.25, 0.5],
            composition_shutter_angle: false,
            shutter_angle: 180.0,
            ..DEFAULT_PR_TRANSFORM
        },
        vec![],
    )];
    let document =
        EditableFxCompositionDocument::from_json_value(project_document_with_media(&outer, &media))
            .unwrap();
    let facts = ["premiere-video-1", "premiere-video-2"]
        .into_iter()
        .flat_map(|asset| {
            source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND)
                .into_values()
                .map(move |facts| (asset.to_owned(), facts))
        })
        .collect();
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
    let sequence = project.single_sequence().unwrap();
    let nest = sequence.nest_occurrences().next().unwrap();
    let staged = nest.sequence.video_occurrences().next().unwrap();
    let PrEffectParams::Transform(values) = staged.effects.last().unwrap().params else {
        panic!("expected a Transform stage: {:?}", staged.effects);
    };
    assert_eq!(
        (values.composition_shutter_angle, values.shutter_angle),
        (false, 180.0)
    );
    let reasons: Vec<_> = omissions.iter().map(|omission| &omission.reason).collect();
    assert!(
        matches!(&reasons[..], [reason] if reason.starts_with(blur)),
        "{reasons:?}"
    );
}

#[test]
fn staged_linear_wipes_export_as_one_clip_and_reread() {
    use crate::schema::PrScalarKeyframe;
    // The first clip of `staged_wipe_sequence` stages for its blur, which
    // applies before the wipe (the case "an effect above the wipe"), its static
    // Motion and its Rotation keys. Each case keeps one of them.
    type Keep = fn(&mut PrVideoOccurrence);
    let cases: [(&str, Keep); 3] = [
        ("an effect above the wipe", |clip| {
            clip.transform = Default::default();
            clip.animations.clear();
        }),
        ("static Motion", |clip| {
            (clip.effects, clip.effects_above_mask) = (Vec::new(), 0);
            clip.animations.clear();
        }),
        ("Motion keys", |clip| {
            (clip.effects, clip.effects_above_mask) = (Vec::new(), 0);
            clip.transform = Default::default();
        }),
    ];
    let scalar_keys = |keys: &[PrScalarKeyframe]| -> Vec<(i64, f64)> {
        keys.iter()
            .map(|key| (key.source_ticks, key.value))
            .collect()
    };
    let wipe_fields = |clip: &PrVideoOccurrence| {
        let wipe = clip.linear_wipe.as_ref().unwrap();
        (
            wipe.initial_completion,
            scalar_keys(&wipe.completion),
            wipe.angle_degrees,
            wipe.feather,
        )
    };
    let motion_keys = |clip: &PrVideoOccurrence| -> Vec<_> {
        clip.animations
            .iter()
            .map(|animation| (animation.property(), scalar_keys(animation.keys())))
            .collect()
    };
    for (staging, keep) in cases {
        let mut sequence = staged_wipe_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        keep(clip);
        let expected = clip.clone();
        let wire = crate::tests::support::project_document(&sequence);
        assert_eq!(
            wire["composition"]["layers"][0]["type"], "Group",
            "{staging}"
        );
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{staging}: {omissions:?}");
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 2, "{staging}");
        assert_eq!(sequence.nest_occurrences().count(), 0, "{staging}");
        // The written chain puts the video's effects, if any, at higher
        // `Index`es than the wipe, so they apply before it.
        let reread = write_and_load_with_crate_reader(project);
        let clip = first_clip(&reread);
        assert_eq!(wipe_fields(clip), wipe_fields(&expected), "{staging}");
        assert_eq!(
            (
                clip.transform,
                motion_keys(clip),
                &clip.effects,
                clip.effects_above_mask
            ),
            (
                expected.transform,
                motion_keys(&expected),
                &expected
                    .effects
                    .iter()
                    .cloned()
                    .map(current_blur_export)
                    .collect::<Vec<_>>(),
                expected.effects_above_mask
            ),
            "{staging}"
        );
    }
}

/// Adds keys 0 to 10 at 0 and 1 s on `target`, with key ids from `name`.
fn add_keys(wire: &mut Value, target: Value, name: &str) {
    let entries = &mut wire["composition"]["dynamics"]["entries"];
    if entries.is_null() {
        *entries = json!([]);
    }
    entries.as_array_mut().unwrap().push(json!({
        "target": target,
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": format!("{name}-a"), "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "linear"}},
            {"id": format!("{name}-b"), "layerTime": 1000, "value": {"type": "float", "value": 10.0}, "easing": {"type": "linear"}}
        ]}
    }));
}

#[test]
fn masks_that_one_crop_or_wipe_cannot_carry_omit_their_clip_or_stage() {
    let layer =
        |id: u64, property: &str| json!({"kind": "layer", "layerId": id, "propertyType": property});
    let mask = |id: u64| json!({"kind": "fxItemProperty", "itemId": id, "propertyName": "opacity"});
    let keyed =
        |wipe, target: Value| two_clips_with_mask(wipe, |wire| add_keys(wire, target, "authored"));
    // A disabled animation still moves the video in FX, at its disabled
    // value, but it exports as no Premiere key.
    let disabled = |wipe| {
        two_clips_with_mask(wipe, |wire| {
            add_keys(wire, layer(1, "rotation"), "disabled");
            let entries = wire["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap();
            let animator = &mut entries.last_mut().unwrap()["animator"];
            animator["enabled"] = json!(false);
            animator["disabledValue"] = json!({"type": "float", "value": 5.0});
        })
    };
    let second_guide_track = |wire: &mut Value| {
        let mut opacity = wire["composition"]["dynamics"]["entries"][0].clone();
        opacity["target"]["propertyType"] = json!("opacity");
        for key in opacity["animator"]["keyframes"].as_array_mut().unwrap() {
            key["id"] = json!(format!("opacity-{}", key["id"].as_str().unwrap()));
        }
        wire["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(opacity);
    };
    // FX draws the mask with the guide's roundness, and a wipe edge where the
    // guide is; the native Crop is square, and the native wipe edge is fixed
    // at the frame edge that its angle names.
    let rounded: fn(&mut Value) = |guide| guide["rect"]["roundness"] = json!(12.0);
    let moved: fn(&mut Value) = |guide| guide["transform"]["position"] = json!([100.0, 0.0]);
    let (crop_keys, frame) = (
        "the Crop guide's transform or Motion keys differ from its video's",
        "the video does not map its frame onto the canvas unchanged, so its Linear Wipe guide is not its frame",
    );
    // Guide 10 moved by (100, 50), with its rectangle moved back, draws the
    // unmoved guide's Crop only while neither it nor its video scales, rotates
    // or has keys, even equal ones.
    let translated = |edit: &dyn Fn(&mut Value)| {
        two_clips_with_mask(false, |wire| {
            let guide = &mut wire["composition"]["layers"][1];
            guide["transform"]["position"] = json!([100.0, 50.0]);
            guide["rect"]["position"] = json!([-100.0, 112.0]);
            edit(wire);
        })
    };
    let both = |wire: &mut Value, field: &str, value: Value| {
        for layer in [0, 1] {
            wire["composition"]["layers"][layer]["transform"][field] = value.clone();
        }
    };
    // Beside video 1: guide 10 and mask 11. `true`: the omission is the only
    // report, as the guide and its keys are consumed.
    let flat = [
        (
            two_clips_with_mask(false, |wire| {
                wire["composition"]["layers"][0]["masks"][0]["inverted"] = json!(true)
            }),
            "the mask is inverted",
            true,
        ),
        // A second track makes the wipe guide no Linear Wipe guide's shape, so
        // the Crop checks decide.
        (
            two_clips_with_mask(true, second_guide_track),
            crop_keys,
            true,
        ),
        (
            two_clips_with_mask(true, |wire| {
                wire["composition"]["layers"][0]["transform"]["scale"] = json!([80.0, 80.0])
            }),
            frame,
            true,
        ),
        (
            two_clips_with_mask(false, |wire| rounded(&mut wire["composition"]["layers"][1])),
            "the Crop guide has rounded corners",
            true,
        ),
        (
            two_clips_with_mask(true, |wire| moved(&mut wire["composition"]["layers"][1])),
            "the Linear Wipe guide is not a cardinal wipe",
            true,
        ),
        // Keys on the guide's outline, on a video transform property that its
        // guide does not repeat, or on the mask change what FX masks over time.
        (keyed(false, layer(10, "rectRoundness")), crop_keys, false),
        (keyed(false, layer(1, "anchorPointX")), crop_keys, false),
        (keyed(true, layer(10, "rectRoundness")), crop_keys, false),
        (keyed(true, layer(1, "anchorPointX")), frame, false),
        (keyed(false, mask(11)), "the mask has keys", false),
        (disabled(false), crop_keys, false),
        (disabled(true), frame, false),
        (
            translated(&|wire| both(wire, "scale", json!([80.0, 80.0]))),
            crop_keys,
            true,
        ),
        (
            translated(&|wire| both(wire, "rotation", json!(10.0))),
            crop_keys,
            true,
        ),
        (
            translated(&|wire| {
                add_keys(wire, layer(1, "positionX"), "video");
                add_keys(wire, layer(10, "positionX"), "guide");
            }),
            crop_keys,
            false,
        ),
        // Off the video's frame, the rectangle is no Crop.
        (
            translated(&|wire| {
                wire["composition"]["layers"][1]["rect"]["position"] = json!([0.0, 162.0])
            }),
            "invalid Premiere project: Crop edge percentages must be finite and within 0..=100",
            true,
        ),
    ]
    .map(|(wire, reason, only)| {
        (
            wire,
            "layer 1 (\"Source\")",
            format!("masks cannot be exported: {reason}; occurrence omitted"),
            only,
        )
    });
    // Stage group 5 over video 1, guide 3 and mask 4; the guide is its second
    // child. One clip cannot carry any of these, and neither can the nest that
    // the group then exports as, which checks the same mask.
    let staged = |sequence: crate::format::PrSequence, edit: &dyn Fn(&mut Value)| {
        let mut wire = crate::tests::support::project_document(&sequence);
        edit(&mut wire);
        wire
    };
    let guide = |edit: fn(&mut Value)| {
        move |wire: &mut Value| edit(&mut wire["composition"]["layers"][0]["layers"][1])
    };
    let keys = |target: Value| move |wire: &mut Value| add_keys(wire, target.clone(), "authored");
    let stage = [
        (
            staged(staged_sequence(), &guide(rounded)),
            "the Crop guide has rounded corners",
        ),
        (
            staged(staged_wipe_sequence(), &guide(moved)),
            "the Linear Wipe guide is not a cardinal wipe",
        ),
        (
            staged(staged_sequence(), &keys(layer(3, "rectRoundness"))),
            "the Crop guide under the group has keys",
        ),
        (
            staged(staged_wipe_sequence(), &keys(layer(3, "rectRoundness"))),
            "the Crop guide under the group is not at the identity",
        ),
        // A nest writes no background, so its keys still omit the group.
        (
            staged(staged_sequence(), &keys(layer(5, "cornerRadiusTopLeft"))),
            "group backgrounds are not supported",
        ),
        (
            staged(staged_sequence(), &keys(mask(4))),
            "the mask has keys",
        ),
    ]
    .map(|(wire, reason)| {
        (
            wire,
            "layer 5 (\"Premiere stage 1\")",
            format!("group was not exported as a nested sequence: {reason}"),
            false,
        )
    });
    for (wire, record, reason, only) in flat.into_iter().chain(stage) {
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let omission = crate::Omission {
            scope: crate::OmissionScope::Occurrence,
            kind: crate::OmissionKind::Omitted,
            record: record.into(),
            reason,
        };
        if only {
            assert_eq!(omissions, std::slice::from_ref(&omission));
        } else {
            assert!(omissions.contains(&omission), "{omission}: {omissions:?}");
        }
        // Only the sibling at 3 s exports.
        let starts: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .map(|clip| clip.start_ticks)
            .collect();
        assert_eq!(starts, [3 * TICKS], "{omission}");
    }
}

/// [`staged_sequence`] with the half-opaque, 12 px feathered Opacity mask
/// of `tests::support::opacity_mask` in place of its Crop; the
/// blur applies before the mask, as every effect does.
fn staged_opacity_mask_sequence() -> crate::format::PrSequence {
    let mut sequence = staged_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.crop = Default::default();
    clip.opacity_mask = Some(crate::tests::support::opacity_mask());
    sequence
}

/// [`staged_opacity_mask_sequence`] without its blur: one flat video with the
/// mask, whose guide repeats its Rotation keys.
fn flat_opacity_mask_sequence() -> crate::format::PrSequence {
    let mut sequence = staged_opacity_mask_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.effects.clear();
    clip.effects_above_mask = 0;
    sequence
}

#[test]
fn edited_opacity_masks_export_as_the_native_mask_and_reread() {
    use crate::schema::text::{PrPathVertex, PrShapePath};
    // Edits on the imported form: the guide's third vertex moves, the mask's
    // feather changes and it inverts at full Mask Opacity; the video's own
    // Opacity and keys stay on the clip.
    let mut edited = crate::tests::support::opacity_mask();
    edited.path.vertices[2] = PrPathVertex {
        smooth: false,
        point: [0.8, 0.7],
        in_tangent: [0.8, 0.7],
        out_tangent: [0.8, 0.7],
    };
    edited.feather = 40.0;
    edited.opacity = 100.0;
    edited.inverted = true;
    for (case, sequence, path) in [
        (
            "flat",
            flat_opacity_mask_sequence(),
            &["composition", "layers", "0", "masks", "0"][..],
        ),
        (
            "staged",
            staged_opacity_mask_sequence(),
            &["composition", "layers", "0", "masks", "0"][..],
        ),
    ] {
        let mut wire = crate::tests::support::project_document(&sequence);
        // The guide is the video's sibling (flat) or the group's second child.
        let guide = match case {
            "flat" => &mut wire["composition"]["layers"][1],
            _ => &mut wire["composition"]["layers"][0]["layers"][1],
        };
        assert_eq!(guide["type"], "Shape", "{case}");
        guide["shape"]["path"]["commands"][2] =
            json!({"type": "lineTo", "x": 0.8 * 1920.0, "y": 0.7 * 1080.0});
        let mut mask = &mut wire;
        for step in path {
            mask = match step.parse::<usize>() {
                Ok(index) => &mut mask[index],
                Err(_) => &mut mask[*step],
            };
        }
        mask["feather"] = json!([40.0, 40.0]);
        mask["opacity"] = json!(1.0);
        mask["inverted"] = json!(true);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert_eq!(
            omissions
                .iter()
                .map(|omission| (omission.scope, omission.kind, omission.reason.as_str()))
                .collect::<Vec<_>>(),
            [(
                crate::OmissionScope::Feature,
                OmissionKind::Approximated,
                crate::schema::MASK_FEATHER_APPROXIMATION
            )],
            "{case}"
        );
        let clip = first_clip(&project);
        assert_eq!(clip.opacity_mask, Some(edited.clone()), "{case}");
        assert_eq!(clip.opacity, 50.0, "{case}");
        assert_eq!(clip.animations.len(), 1, "{case}");
        assert_eq!(clip.effects.len(), usize::from(case == "staged"), "{case}");
        assert_eq!(clip.effects_above_mask, clip.effects.len(), "{case}");
        let reread = write_and_load_with_crate_reader(project);
        let clip = first_clip(&reread);
        assert_eq!(clip.opacity_mask, Some(edited.clone()), "{case}");
        assert_eq!(clip.effects_above_mask, clip.effects.len(), "{case}");
        let _: &PrShapePath = &edited.path;
    }
}

#[test]
fn opacity_masks_that_one_mask_record_cannot_carry_omit_their_clip_or_stage() {
    let layer =
        |id: u64, property: &str| json!({"kind": "layer", "layerId": id, "propertyType": property});
    let flat = |edit: &dyn Fn(&mut Value)| {
        let mut wire = crate::tests::support::project_document(&flat_opacity_mask_sequence());
        edit(&mut wire);
        wire
    };
    let staged = |edit: &dyn Fn(&mut Value)| {
        let mut wire = crate::tests::support::project_document(&staged_opacity_mask_sequence());
        edit(&mut wire);
        wire
    };
    let omitted = |reason: &str| format!("masks cannot be exported: {reason}; occurrence omitted");
    let nested = |reason: &str| format!("group was not exported as a nested sequence: {reason}");
    // Beside video 1: guide 2 and mask 3 when flat; with the plain clip at 2,
    // the staged clip's guide is 3, its mask 4 and its stage group 5.
    for (wire, record, reason) in [
        (
            flat(&|wire| {
                wire["composition"]["layers"][1]["shape"]["path"]["commands"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }),
            "layer 1 (\"Premiere video 1\")",
            omitted("invalid Premiere project: a mask path must be a closed outline of at least three vertices"),
        ),
        (
            flat(&|wire| {
                wire["composition"]["layers"][1]["shape"]["path"]["commands"]
                    .as_array_mut()
                    .unwrap()
                    .insert(2, json!({"type": "moveTo", "x": 0.0, "y": 0.0}));
            }),
            "layer 1 (\"Premiere video 1\")",
            omitted("the Opacity mask guide path cannot be exported: unsupported conversion: a Premiere shape path holds one contour; holes need Mask with Shape (JRB-2083)"),
        ),
        (
            flat(&|wire| {
                wire["composition"]["layers"][1]["shape"]["trim"] = json!({"start": 0.0, "end": 50.0, "offset": 0.0})
            }),
            "layer 1 (\"Premiere video 1\")",
            omitted("the Opacity mask guide has a path modifier or primitive"),
        ),
        // FX rounds an anchor with a corner radius; a mask vertex has none.
        (
            flat(&|wire| {
                wire["composition"]["layers"][1]["shape"]["path"]["commands"][1]["cornerRadius"] = json!(10.0)
            }),
            "layer 1 (\"Premiere video 1\")",
            omitted("the Opacity mask guide path cannot be exported: unsupported conversion: rounded shape path corners are unsupported"),
        ),
        (
            staged(&|wire| {
                wire["composition"]["layers"][0]["layers"][1]["shape"]["path"]["commands"][1]["cornerRadius"] = json!(10.0)
            }),
            "layer 5 (\"Premiere stage 1\")",
            nested("an Opacity mask on a nested sequence is not converted"),
        ),
        (
            flat(&|wire| wire["composition"]["layers"][1]["transform"]["scale"] = json!([100.0, 100.0])),
            "layer 1 (\"Premiere video 1\")",
            omitted("the Opacity mask guide's transform or Motion keys differ from its video's"),
        ),
        // The staged form of this clip exports the blur before the mask
        // (`edited_opacity_masks_export_as_the_native_mask_and_reread`).
        (
            flat(&|wire| {
                wire["composition"]["layers"][0]["effects"] =
                    json!([{"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}}])
            }),
            "layer 1 (\"Premiere video 1\")",
            omitted("the video has effects, which FX applies after its mask and Premiere before an Opacity mask; the mask needs a stage group"),
        ),
        (
            flat(&|wire| wire["composition"]["layers"][0]["masks"][0]["feather"] = json!([1500.0, 1500.0])),
            "layer 1 (\"Premiere video 1\")",
            omitted("invalid Premiere project: Mask Feather must be finite and within 0..=1000, the written record's bound"),
        ),
        // Fixture clip D (G3b): Premiere renders 0.5 x (1 - coverage), FX
        // 1 - 0.5 x coverage.
        (
            flat(&|wire| wire["composition"]["layers"][0]["masks"][0]["inverted"] = json!(true)),
            "layer 1 (\"Premiere video 1\")",
            omitted("invalid Premiere project: an inverted mask with Mask Opacity below 100 is not converted: Premiere renders Mask Opacity times the inverted coverage, FX inverts the Mask Opacity-weighted coverage"),
        ),
        // A staged guide with keys is no stage, and no nest carries the mask.
        (
            staged(&|wire| add_keys(wire, layer(3, "rotation"), "authored")),
            "layer 5 (\"Premiere stage 1\")",
            nested("an Opacity mask on a nested sequence is not converted"),
        ),
    ] {
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let omission = crate::Omission {
            scope: crate::OmissionScope::Occurrence,
            kind: crate::OmissionKind::Omitted,
            record: record.into(),
            reason,
        };
        assert!(omissions.contains(&omission), "{omission}: {omissions:?}");
        // Only the plain clip at 3 s exports, and the guide is never a graphic.
        let sequence = project.single_sequence().unwrap();
        let starts: Vec<_> = sequence
            .video_occurrences()
            .map(|clip| clip.start_ticks)
            .collect();
        assert_eq!(starts, [3 * TICKS], "{omission}");
        assert_eq!(sequence.video_items().filter_map(PrVideoItem::graphic).count(), 0, "{omission}");
    }
}

#[test]
fn stage_group_exports_the_effects_it_converts_before_its_mask() {
    let mut wire = crate::tests::support::project_document(&staged_sequence());
    wire["composition"]["layers"][0]["layers"][0]["effects"] = json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 7, "effect": {"type": "dropShadow", "color": [0, 0, 0, 1], "offset": [0, 10], "blurRadius": 4, "spreadRadius": 0}},
        {"id": 8, "effect": {"type": "gaussianBlur", "blurriness": 20.0}},
    ]);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert_eq!(
        omissions,
        [crate::Omission {
            scope: crate::OmissionScope::Feature,
            kind: crate::OmissionKind::Omitted,
            record: "layer 5 (\"Premiere stage 1\")".into(),
            reason:
                "effects: dropShadow effect 7 was not exported: it has no Premiere effect mapping"
                    .into(),
        }]
    );
    let clip = first_clip(&project);
    assert_eq!(
        clip.effects,
        [
            exported_blur(true, 10.0, false),
            exported_blur(true, 20.0, false)
        ]
    );
    // Application order [blur, blur, Crop]: the mask applies after every
    // exported effect.
    assert_eq!(clip.effects_above_mask, 2);
}

#[test]
fn keyed_effects_before_or_after_a_crop_or_keyed_wipe_import_export_and_reread() {
    use crate::schema::{
        PrCornerPin, PrEffect, PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams,
        PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe, CORNER_PIN,
        GAUSSIAN_BLUR_BLURRINESS,
    };
    // Keys on the source clock of a clip whose source In is 1 s.
    let at = |millis: i64| TICKS + millis * TICKS_PER_MILLISECOND;
    let keys = |pairs: &[(i64, f64)]| -> Vec<PrScalarKeyframe> {
        pairs
            .iter()
            .map(|&(millis, value)| PrScalarKeyframe {
                source_ticks: at(millis),
                value,
                easing: PrKeyframeEasing::Linear,
            })
            .collect()
    };
    let mut blur = blur_effect(10.0);
    blur.animations = vec![PrEffectParamAnimation {
        param: &GAUSSIAN_BLUR_BLURRINESS,
        keys: PrEffectParamKeys::Scalar(keys(&[(500, 10.0), (1000, 30.0), (1500, 0.0)])),
    }];
    let blur_tracks = || {
        BTreeMap::from([(
            "blurriness".to_owned(),
            vec![(500, 10.0), (1000, 30.0), (1500, 0.0)],
        )])
    };
    // A skew whose Upper Left moves on a straight path: an x and a y track.
    let corner_key = |millis, value| PrPointKeyframe {
        source_ticks: at(millis),
        value,
        easing: PrKeyframeEasing::Linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    let pin = PrEffect {
        enabled: true,
        params: PrEffectParams::CornerPin(PrCornerPin {
            corners: [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]],
        }),
        animations: vec![PrEffectParamAnimation {
            param: &CORNER_PIN.params[0],
            keys: PrEffectParamKeys::Point(vec![
                corner_key(500, [0.1, 0.05]),
                corner_key(1500, [0.2, 0.1]),
            ]),
        }],
    };
    let pin_tracks = BTreeMap::from([
        ("upperLeftX".to_owned(), vec![(500, 0.1), (1500, 0.2)]),
        ("upperLeftY".to_owned(), vec![(500, 0.05), (1500, 0.1)]),
    ]);
    // An effect that applies before the mask stages the clip (k = 1); one that
    // applies after it stays on the one video layer (k = 0). A flat wipe needs
    // the default static Motion without keys.
    for (effect, tracks, wipe, before) in [
        (&blur, blur_tracks(), false, true),
        (&blur, blur_tracks(), false, false),
        (&blur, blur_tracks(), true, true),
        (&blur, blur_tracks(), true, false),
        (&pin, pin_tracks.clone(), false, true),
        (&pin, pin_tracks.clone(), false, false),
    ] {
        let case = format!(
            "{:?}, wipe {wipe}, before the mask {before}",
            effect.spec().display_name
        );
        let mut sequence = if wipe {
            staged_wipe_sequence()
        } else {
            staged_sequence()
        };
        let clip = sequence.video_tracks[0].clip_mut(0);
        (clip.in_ticks, clip.out_ticks) = (TICKS, 3 * TICKS);
        clip.animations = if wipe && !before {
            clip.transform = Default::default();
            Vec::new()
        } else {
            vec![PrPropertyAnimation::Rotation(keys(&[
                (0, 0.0),
                (1000, 20.0),
            ]))]
        };
        if let Some(wipe) = &mut clip.linear_wipe {
            wipe.completion = keys(&[(0, 100.0), (1000, 0.0)]);
        }
        (clip.effects, clip.effects_above_mask) = (vec![effect.clone()], usize::from(before));
        let expected = clip.clone();
        let media = crate::tests::support::video_media();
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        // Import: the keys are tracks on the effect of the video, the group's
        // child when staged, at layer times from the source In.
        let top = &wire["composition"]["layers"][0];
        assert_eq!(
            top["type"],
            if before { "Group" } else { "Video" },
            "{case}"
        );
        let video = if before { &top["layers"][0] } else { top };
        let imported: BTreeMap<String, Vec<(i64, f64)>> = wire["composition"]["dynamics"]
            ["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["target"]["kind"] == "effectProperty")
            .map(|entry| {
                assert_eq!(
                    entry["target"]["effectId"], video["effects"][0]["id"],
                    "{case}"
                );
                let keys = entry["animator"]["keyframes"]
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
                (
                    entry["target"]["paramName"].as_str().unwrap().to_owned(),
                    keys,
                )
            })
            .collect();
        assert_eq!(imported, tracks, "{case}");
        // Export writes one clip whose chain rereads with the same order, keys,
        // mask and Motion, and reports nothing.
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        let exported = project.single_sequence().unwrap();
        assert_eq!(exported.video_occurrences().count(), 2, "{case}");
        assert_eq!(exported.nest_occurrences().count(), 0, "{case}");
        let reread = write_and_load_with_crate_reader(project);
        let clip = first_clip(&reread);
        let reading = |clip: &PrVideoOccurrence| {
            let motion_keys: Vec<_> = clip
                .animations
                .iter()
                .flat_map(|animation| animation.keys())
                .map(|key| (key.source_ticks, key.value))
                .collect();
            let wipe = clip.linear_wipe.as_ref().map(|wipe| {
                let keys: Vec<_> = wipe
                    .completion
                    .iter()
                    .map(|key| (key.source_ticks, key.value))
                    .collect();
                (keys, wipe.angle_degrees, wipe.feather)
            });
            (
                clip.effects.clone(),
                clip.effects_above_mask,
                clip.crop,
                clip.transform,
                (clip.in_ticks, clip.out_ticks),
                motion_keys,
                wipe,
            )
        };
        let mut expected = expected;
        expected.effects = expected
            .effects
            .into_iter()
            .map(current_blur_export)
            .collect();
        assert_eq!(reading(clip), reading(&expected), "{case}");
    }
}

#[test]
fn a_video_z_position_is_reported_as_not_exported() {
    // A clip has no depth, where FX's camera scales a layer at depth z: export
    // writes x and y as before and reports a nonzero z or a PositionZ track on
    // the layer whose transform is the clip's Motion.
    let z = |layer: &mut Value, z: f64| {
        let position = layer["transform"]["position"].as_array().unwrap().clone();
        layer["transform"]["position"] = json!([position[0], position[1], z]);
    };
    let position_z = |id: u64| json!({"kind": "layer", "layerId": id, "propertyType": "positionZ"});
    let exported = |document: Value| {
        let (project, omissions) = convert_with_omissions(document).unwrap();
        (format!("{:?}", first_clip(&project)), omissions)
    };
    let reported = |record: &str| crate::Omission {
        scope: crate::OmissionScope::Feature,
        kind: crate::OmissionKind::Omitted,
        record: record.into(),
        reason: "3D position was not exported".into(),
    };
    // The one-clip document's video 1, and stage group 5 of a staged clip.
    let staged = crate::tests::support::project_document(&staged_sequence());
    for (document, record) in [
        (document(), "layer 1 (\"Source\")"),
        (staged.clone(), "layer 5 (\"Premiere stage 1\")"),
    ] {
        let (clip, omissions) = exported(document.clone());
        assert!(omissions.is_empty(), "{omissions:?}");
        let mut static_z = document;
        z(&mut static_z["composition"]["layers"][0], 50.0);
        assert_eq!(exported(static_z), (clip, vec![reported(record)]));
    }
    // A PositionZ track also reports that its keys cannot be exported.
    let mut keyed = document();
    z(&mut keyed["composition"]["layers"][0], 0.0);
    add_keys(&mut keyed, position_z(1), "z");
    let (_, omissions) = exported(keyed);
    assert_eq!(
        omissions,
        [
            crate::Omission {
                scope: crate::OmissionScope::Feature,
                kind: crate::OmissionKind::Omitted,
                record: "layer 1".into(),
                reason: "only independent Opacity, paired Position, Rotation, uniform Scale, audio volume and text Source Text keyframes can be exported; animation was omitted".into(),
            },
            reported("layer 1 (\"Source\")"),
        ]
    );
    // On a stage group it omits the group, as before, with no new report.
    let mut keyed = staged;
    z(&mut keyed["composition"]["layers"][0], 0.0);
    add_keys(&mut keyed, position_z(5), "z");
    let (_, omissions) = convert_with_omissions(keyed).unwrap();
    assert!(
        omissions.iter().any(|omission| omission.reason
            == "group was not exported as a nested sequence: group PositionZ animation is not supported"),
        "{omissions:?}"
    );
    assert!(
        !omissions.contains(&reported("layer 5 (\"Premiere stage 1\")")),
        "{omissions:?}"
    );
}

#[test]
fn mismatched_or_unpaired_scale_axes_are_omitted_with_diagnostics() {
    use crate::{
        schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe},
        tests::support::{project_document, video_sequence},
    };
    let mut sequence = video_sequence();
    sequence.video_tracks[0]
        .clip_mut(0)
        .animations
        .push(PrPropertyAnimation::UniformScale(vec![
            PrScalarKeyframe {
                source_ticks: 0,
                value: 100.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: TICKS,
                value: 150.0,
                easing: PrKeyframeEasing::Hold,
            },
        ]));
    let wire = project_document(&sequence);
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 2);
    let unpaired = "nonuniform or unpaired Scale";
    for axis in 0..2 {
        let mut changed = wire.clone();
        changed["composition"]["dynamics"]["entries"][axis]["animator"]["keyframes"][1]["value"]
            ["value"] = json!(175.0);
        assert_scale_omitted(changed, unpaired);
        let mut moved = wire.clone();
        moved["composition"]["dynamics"]["entries"][axis]["animator"]["keyframes"][1]
            ["layerTime"] = json!(1001);
        assert_scale_omitted(moved, unpaired);
        let mut re_eased = wire.clone();
        re_eased["composition"]["dynamics"]["entries"][axis]["animator"]["keyframes"][1]
            ["easing"] = json!({"type":"linear"});
        assert_scale_omitted(re_eased, unpaired);
        let mut shortened = wire.clone();
        assert!(
            shortened["composition"]["dynamics"]["entries"][axis]["animator"]["keyframes"]
                .as_array_mut()
                .unwrap()
                .pop()
                .is_some()
        );
        assert_scale_omitted(shortened, unpaired);
        let mut missing = wire.clone();
        let entries = missing["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap();
        entries.remove(axis);
        // An X axis keyed alone is Scale Width, whose Hold key is unmeasured;
        // a Y axis alone is no Premiere form.
        let reason = if entries[0]["target"]["propertyType"] == "scaleX" {
            "Scale Width animation was not exported: unsupported conversion: only Linear Scale Width keys convert"
        } else {
            unpaired
        };
        assert_scale_omitted(missing, reason);
    }
}

#[test]
fn anchor_point_and_scale_width_tracks_export_on_the_source_frame() {
    use crate::schema::PrAnimatedProperty;
    // An explicit document: a 1280 x 720 source on the 1920 x 1080 canvas at
    // equal static axes, whose X axis alone grows, so Uniform Scale must turn
    // off. The anchor moves in source pixels, the position in canvas pixels.
    let linear = json!({"type": "linear"});
    let mut wire = document();
    let video = &mut wire["composition"]["layers"][0];
    video["source"]["sourceRect"] = json!({"x": 0, "y": 0, "width": 1280, "height": 720});
    video["transform"] = json!({
        "anchorPoint": [640, 360], "position": [960, 540], "scale": [50, 50],
        "rotation": 0, "opacity": 100,
    });
    wire["composition"]["dynamics"] = json!({"entries": [
        two_layer_keys(1, "positionX", [960.0, 480.0], linear.clone()),
        two_layer_keys(1, "positionY", [540.0, 810.0], linear.clone()),
        two_layer_keys(1, "anchorPointX", [640.0, 320.0], linear.clone()),
        two_layer_keys(1, "anchorPointY", [360.0, 540.0], linear.clone()),
        two_layer_keys(1, "scaleX", [50.0, 100.0], linear),
    ]});
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut media = source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND);
    let Some(MediaFacts::Video(facts)) = media.get_mut("premiere-video-1") else {
        panic!("the source is a video");
    };
    (facts.width, facts.height) = (1280, 720);
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &media,
        &sound(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
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
    let expected = [
        PrPropertyAnimation::Position(vec![point(0, [0.5, 0.5]), point(TICKS, [0.25, 0.75])]),
        PrPropertyAnimation::AnchorPoint(vec![point(0, [0.5, 0.5]), point(TICKS, [0.25, 0.75])]),
        PrPropertyAnimation::ScaleWidth(vec![scalar(0, 50.0), scalar(TICKS, 100.0)]),
    ];
    let by_property = |clip: &PrVideoOccurrence| {
        let mut animations = clip.animations.clone();
        animations.sort_by_key(PrPropertyAnimation::property);
        animations
    };
    let clip = first_clip(&project);
    assert_eq!(clip.transform.anchor_point, [0.5, 0.5]);
    assert_eq!(clip.transform.scale, [50.0, 50.0]);
    assert_eq!(by_property(clip), expected);
    // This crate reads Scale Width keys only without Uniform Scale, so the
    // reread keys show that the writer turned it off.
    let reread = write_and_load_with_crate_reader(project);
    let clip = first_clip(&reread);
    assert_eq!(clip.transform.scale, [50.0, 50.0]);
    assert_eq!(by_property(clip), expected);
    assert!(clip
        .animations
        .iter()
        .all(|animation| animation.property() != PrAnimatedProperty::UniformScale));
}

#[test]
fn anchor_point_and_scale_width_tracks_outside_the_measured_form_keep_static_motion() {
    let linear = json!({"type": "linear"});
    let hold = json!({"type": "hold"});
    let mut early_y = two_layer_keys(1, "anchorPointY", [0.0, 540.0], linear.clone());
    early_y["animator"]["keyframes"][1]["layerTime"] = json!(500);
    let not_exported = "animation was not exported: unsupported conversion: only Linear";
    for (entries, reason) in [
        (
            vec![two_layer_keys(1, "anchorPointX", [0.0, 960.0], linear.clone())],
            "unpaired Anchor Point keyframes were not exported".to_owned(),
        ),
        (
            vec![
                two_layer_keys(1, "anchorPointX", [0.0, 960.0], linear.clone()),
                early_y,
            ],
            "Anchor Point animation was not exported: unsupported conversion: Anchor Point X/Y keys must have identical times and temporal easing".to_owned(),
        ),
        (
            vec![
                two_layer_keys(1, "anchorPointX", [0.0, 960.0], hold.clone()),
                two_layer_keys(1, "anchorPointY", [0.0, 540.0], hold.clone()),
            ],
            format!("Anchor Point {not_exported} Anchor Point keys without spatial tangents convert; Premiere's other Anchor Point keys are unmeasured"),
        ),
        (
            vec![two_layer_keys(1, "scaleX", [100.0, 150.0], hold)],
            format!("Scale Width {not_exported} Scale Width keys convert; Premiere's other Scale Width keys are unmeasured"),
        ),
        // Scale Height keys without Uniform Scale are unmeasured.
        (
            vec![two_layer_keys(1, "scaleY", [100.0, 150.0], linear)],
            "nonuniform or unpaired Scale keyframes were not exported".to_owned(),
        ),
    ] {
        let mut wire = document();
        wire["composition"]["dynamics"] = json!({ "entries": entries });
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let clip = first_clip(&project);
        assert!(clip.animations.is_empty(), "{reason}");
        assert_eq!(
            clip.transform,
            PrStaticTransform {
                position: [0.0, 0.0],
                anchor_point: [0.0, 0.0],
                ..PrStaticTransform::default()
            },
            "{reason}"
        );
        let reasons: Vec<_> = omissions
            .iter()
            .map(|omission| (omission.scope, omission.reason.as_str()))
            .collect();
        assert_eq!(reasons, [(OmissionScope::Feature, reason.as_str())]);
    }
}

#[test]
fn a_crop_guide_repeating_anchor_point_keys_exports_with_its_clip() {
    use crate::tests::support::{project_document, video_sequence};
    // Import gives the Crop guide its video's Anchor Point keys; export
    // writes them once, on the clip, and reports nothing lost.
    let point = |source_ticks, value| PrPointKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    let anchor =
        PrPropertyAnimation::AnchorPoint(vec![point(0, [0.5, 0.5]), point(TICKS, [0.25, 0.75])]);
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.crop = left_crop();
    clip.animations = vec![anchor.clone()];
    let wire = project_document(&sequence);
    let guide = &wire["composition"]["layers"][1];
    assert_eq!(guide["name"], "Premiere Crop guide 1");
    let guide_tracks = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == guide["id"])
        .count();
    assert_eq!(guide_tracks, 2);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = first_clip(&project);
    assert!((clip.crop.left - 7.0).abs() < 1e-9, "{:?}", clip.crop);
    assert_eq!(clip.animations, [anchor]);
}

fn assert_scale_omitted(wire: Value, reason: &str) {
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .animations
        .is_empty());
    assert!(
        omissions.iter().any(|item| item.reason.contains(reason)),
        "{reason}: {omissions:?}"
    );
}

#[test]
fn exports_slow_fast_and_reverse_constant_playback() {
    for (active_duration, source_start, source_duration, selection, reverse, expected_rate) in [
        (2000, 1000, 1000, [1000, 1000], false, 0.5),
        (1000, 1000, 2000, [1000, 2000], false, 2.0),
        (2000, 4000, 1000, [4000, 1000], true, -0.5),
        // Worlds selects these intervals inside a larger sourceRange.
        (6000, 0, 6000, [0, 6042], false, 1.0),
        (1400, 2310, 2860, [0, 10000], false, 2860.0 / 1400.0),
        (2000, 4000, 1000, [1000, 8000], true, -0.5),
        (
            2021,
            0,
            2000,
            [0, 4042],
            false,
            (2000 * TICKS_PER_MILLISECOND) as f64 / (61 * FRAME_30) as f64,
        ),
    ] {
        let mut wire = document();
        wire["duration"] = json!(active_duration as f64 / 1000.0);
        let video = &mut wire["composition"]["layers"][0];
        video["playback"] = crate::test_support::linear_playback(
            json!({"start": 0, "duration": active_duration}),
            json!({"start": source_start, "duration": source_duration}),
        );
        video["sourceRange"] = json!({"start": selection[0], "duration": selection[1]});
        video["sourceIntrinsicDuration"] = json!(SOURCE_MILLIS);
        let (first_value, last_value) = if reverse {
            (source_start + source_duration, source_start)
        } else {
            (source_start, source_start + source_duration)
        };
        video["playback"] = crate::test_support::remapped_playback(
            crate::test_support::layer_range(video).clone(),
            json!({
                "keyframes": [
                    {"id":"start", "time":0, "value":first_value, "easing":{"type":"linear"}},
                    {"id":"end", "time":active_duration, "value":last_value, "easing":{"type":"linear"}}
                ],
                "before":"inactive",
                "after":"inactive"
            }),
        );
        wire["composition"]["layers"][1]["activeRange"]["duration"] = json!(active_duration);

        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(
            omissions.is_empty(),
            "constant playback should be lossless: {omissions:?}"
        );
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!(clip.playback_rate, expected_rate);
        assert_eq!(
            (clip.start_ticks, clip.end_ticks),
            (
                0,
                frame_ticks_from_time(
                    Time::from_millis(active_duration),
                    FrameRate::Fps30,
                    "test active end"
                )
                .unwrap()
            )
        );
        assert_eq!(
            (clip.in_ticks, clip.out_ticks),
            (
                (if reverse {
                    SOURCE_MILLIS - source_start - source_duration
                } else {
                    source_start
                }) * TICKS
                    / 1000,
                (if reverse {
                    SOURCE_MILLIS - source_start
                } else {
                    source_start + source_duration
                }) * TICKS
                    / 1000
            )
        );
    }
}

#[test]
fn bounded_constant_playback_respects_signed_input_offsets() {
    for input_offset in [-700, 700] {
        let mut wire = document();
        wire["duration"] = json!(3.0);
        let video = &mut wire["composition"]["layers"][0];
        video["sourceRange"] = json!({"start": 0, "duration": SOURCE_MILLIS});
        video["playback"] = crate::test_support::remapped_playback(
            json!({"start": 1000, "duration": 2000}),
            json!({"keyframes": [
                {"id": "a", "time": 1000 + input_offset, "value": 2000, "easing": {"type": "linear"}},
                {"id": "b", "time": 3000 + input_offset, "value": 3000, "easing": {"type": "linear"}}
            ], "before": "inactive", "after": "inactive"}),
        );
        video["playback"]["inputOffsetMs"] = json!(input_offset);
        wire["composition"]["layers"][1]["activeRange"]["duration"] = json!(3000);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{input_offset}: {omissions:?}");
        let clip = first_clip(&project);
        assert_eq!(
            (
                clip.start_ticks,
                clip.end_ticks,
                clip.in_ticks,
                clip.out_ticks,
                clip.playback_rate
            ),
            (TICKS, 3 * TICKS, 2 * TICKS, 3 * TICKS, 0.5)
        );
    }
}

#[test]
fn bounded_constant_playback_rejects_unsupported_curves_and_windows() {
    for (path, invalid) in [
        ("/mapping/property/before", json!("hold")),
        ("/mapping/property/after", json!("continue")),
        ("/mapping/property/keyframes/0/time", json!(1)),
        ("/mapping/property/keyframes/1/time", json!(1999)),
        ("/mapping/property/keyframes/0/value", json!(999)),
        ("/mapping/property/keyframes/1/value", json!(8001)),
        ("/mapping/property/keyframes/1/value", json!(2000)),
        (
            "/mapping/property/keyframes/1/easing",
            json!({"type": "hold"}),
        ),
        (
            "/mapping/property/keyframes/1/easing",
            json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.25, "y2": 1.0}),
        ),
        ("/inputOffsetMs", json!(1)),
    ] {
        let mut wire = two_clips();
        let video = &mut wire["composition"]["layers"][0];
        video["sourceRange"] = json!({"start": 1000, "duration": 7000});
        video["playback"] = crate::test_support::remapped_playback(
            json!({"start": 0, "duration": 2000}),
            json!({"keyframes": [
                {"id": "a", "time": 0, "value": 2000, "easing": {"type": "linear"}},
                {"id": "b", "time": 2000, "value": 3000, "easing": {"type": "linear"}}
            ], "before": "inactive", "after": "inactive"}),
        );
        *video["playback"].pointer_mut(path).unwrap() = invalid;
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let clips: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .collect();
        assert_eq!(clips.len(), 1, "{path}: {omissions:?}");
        assert_eq!(clips[0].start_ticks, 3 * TICKS);
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == OmissionScope::Occurrence
                    && item.reason.contains("time remapping was not exported")),
            "{path}: {omissions:?}"
        );
    }
}

#[test]
fn bounded_constant_playback_keeps_media_duration_checks() {
    for (source_end, intrinsic_duration, reverse, accepted) in [
        (10000, SOURCE_MILLIS, false, true),
        (10000, SOURCE_MILLIS, true, true),
        (11000, SOURCE_MILLIS, false, false),
        (11000, SOURCE_MILLIS, true, false),
        (10000, SOURCE_MILLIS - 1, false, false),
    ] {
        let mut wire = document();
        let video = &mut wire["composition"]["layers"][0];
        video["sourceRange"] = json!({"start": 0, "duration": 12000});
        video["sourceIntrinsicDuration"] = json!(intrinsic_duration);
        let (first, last) = if reverse {
            (source_end, 9000)
        } else {
            (9000, source_end)
        };
        video["playback"] = crate::test_support::remapped_playback(
            json!({"start": 0, "duration": 1000}),
            json!({"keyframes": [
                {"id": "a", "time": 0, "value": first, "easing": {"type": "linear"}},
                {"id": "b", "time": 1000, "value": last, "easing": {"type": "linear"}}
            ], "before": "inactive", "after": "inactive"}),
        );
        let result = convert_with_omissions(wire);
        if accepted {
            let (project, omissions) = result.unwrap();
            assert!(omissions.is_empty(), "{omissions:?}");
            let clip = first_clip(&project);
            let expected = if reverse {
                (0, TICKS, -1.0)
            } else {
                (9 * TICKS, 10 * TICKS, 1.0)
            };
            assert_eq!(
                (clip.in_ticks, clip.out_ticks, clip.playback_rate),
                expected
            );
        } else {
            let error = result.unwrap_err().to_string();
            let reason = if intrinsic_duration != SOURCE_MILLIS {
                "sourceIntrinsicDuration"
            } else if reverse {
                "invalid timeline/source ranges"
            } else {
                "source range requires a frame past the media end"
            };
            assert!(error.contains(reason), "{reason}: {error}");
        }
    }
}

#[test]
fn intrinsic_duration_accepts_only_floor_or_nearest_of_the_exact_source_clock() {
    for (ticks, authored, accepted) in [
        (121 * FrameRate::Fps24.ticks_per_frame(), 5041, true),
        (121 * FrameRate::Fps24.ticks_per_frame(), 5042, true),
        (121 * FrameRate::Fps24.ticks_per_frame(), 5040, false),
        (121 * FrameRate::Fps24.ticks_per_frame(), 5043, false),
        (120 * FrameRate::Fps24.ticks_per_frame(), 5000, true),
        (120 * FrameRate::Fps24.ticks_per_frame(), 4999, false),
        (120 * FrameRate::Fps24.ticks_per_frame(), 5001, false),
    ] {
        let mut wire = document();
        wire["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(authored);
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let mut collector = crate::export_loss::LossCollector::default();
        let result = lower_document(
            &document,
            &source(FrameRate::Fps24, ticks),
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut collector,
        );
        let report = collector.finish(result.is_ok());
        let omissions = &report.diagnostics;
        if !accepted {
            let Err(error) = result else {
                panic!("invalid sourceIntrinsicDuration {authored} was accepted");
            };
            let error = error.to_string();
            assert!(
                error.contains("sourceIntrinsicDuration"),
                "{authored}: {error}"
            );
            continue;
        }
        let project = result.unwrap().project.unwrap();
        assert_eq!(
            project.media[&MediaId("premiere-video-1".into())]
                .video
                .as_ref()
                .unwrap()
                .intrinsic_ticks,
            ticks
        );
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!(
            (clip.in_ticks, clip.out_ticks, clip.playback_rate),
            (0, TICKS, 1.0)
        );
        let truncated = authored == 5041;
        assert_eq!(
            omissions.len(),
            usize::from(truncated),
            "{authored}: {omissions:?}"
        );
        if truncated {
            assert!(omissions[0]
                .reason
                .contains("floor of the packaged MP4 duration"));
            assert_eq!(report.losses.len(), 1);
            assert_eq!(report.losses[0].domain, ExportLossDomain::Metadata);
            assert_eq!(
                report.losses[0].source,
                ExportLossSource::Layer(LayerId::new(1))
            );
        }
    }
}

#[test]
fn playback_that_native_speed_cannot_carry_omits_its_clip() {
    // Native constant speed and reverse carry only a two-key linear playback
    // within the source range. (the first clip's active duration and source
    // range, its playback or `source.timeRemap`, and the reason that omits it,
    // or "" when it exports)
    let remapped = "time remapping was not exported: Premiere export writes constant speed only";
    let ramp = json!({"keyframes": [
        {"id": "a", "time": 0, "value": 0, "easing": {"type": "linear"}},
        {"id": "b", "time": 1000, "value": 1000, "easing": {"type": "linear"}},
        {"id": "c", "time": 3000, "value": 6000, "easing": {"type": "linear"}}
    ], "before": "inactive", "after": "inactive"});
    for (active, [start, duration], playback, time_remap, reason) in [
        (3000, [0, 6000], Some(ramp), None, remapped),
        (2000, [4000, 1000], None, Some(1.0), remapped),
        // FX does not render the remap: it plays the source range at unit speed.
        (2000, [4000, 2000], None, Some(1.0), ""),
    ] {
        let mut wire = two_clips();
        let video = &mut wire["composition"]["layers"][0];
        video["playback"] = crate::test_support::linear_playback(
            json!({"start": 0, "duration": active}),
            json!({"start": start, "duration": duration}),
        );
        video["sourceRange"] = json!({"start": start, "duration": duration});
        if let Some(playback) = playback {
            video["playback"] = crate::test_support::remapped_playback(
                crate::test_support::layer_range(video).clone(),
                playback,
            );
        }
        if let Some(remap) = time_remap {
            video["source"]["timeRemap"] = json!(remap);
        }
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let clips: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .map(|clip| {
                (
                    clip.start_ticks,
                    clip.in_ticks,
                    clip.out_ticks,
                    clip.playback_rate,
                )
            })
            .collect();
        let sibling = (3 * TICKS, 6 * TICKS, 8 * TICKS, 1.0);
        let (expected_clips, scope, expected_reason) = if reason.is_empty() {
            let clip = (0, 4 * TICKS, 6 * TICKS, 1.0);
            let report = "time remap (using source range at constant speed) was not exported";
            (vec![clip, sibling], crate::OmissionScope::Feature, report)
        } else {
            (vec![sibling], crate::OmissionScope::Occurrence, reason)
        };
        assert_eq!(clips, expected_clips, "{reason}");
        assert_eq!(
            omissions,
            [crate::Omission {
                scope,
                kind: crate::OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".into(),
                reason: expected_reason.into(),
            }],
            "{reason}"
        );
    }
}

#[test]
fn maps_all_cuts_trims_and_gaps_without_an_archive() {
    let (project, omissions) = convert_with_omissions(two_clips()).unwrap();
    assert!(
        omissions.is_empty(),
        "lossless cuts should not warn: {omissions:?}"
    );
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.name, "Fresh exact 30");
    assert_eq!((sequence.width, sequence.height), (1920, 1080));
    let ranges: Vec<_> = sequence
        .video_occurrences()
        .map(|o| (o.start_ticks, o.end_ticks, o.in_ticks, o.out_ticks))
        .collect();
    assert_eq!(
        ranges,
        [
            (0, 2 * TICKS, TICKS, 3 * TICKS),
            (3 * TICKS, 5 * TICKS, 6 * TICKS, 8 * TICKS),
        ]
    );
    assert!(sequence
        .video_occurrences()
        .all(|o| o.media.as_str() == "premiere-video-1"));
    let media = &project.media[&MediaId("premiere-video-1".into())];
    assert_eq!(media.video.as_ref().unwrap().intrinsic_ticks, 10 * TICKS);
}

#[test]
fn shared_asset_rejects_conflicting_intrinsic_durations() {
    let mut wire = two_clips();
    wire["composition"]["layers"][1]["sourceIntrinsicDuration"] = json!(9000);
    // The inspected file, not the other layer, decides which claim is wrong.
    let error = convert(wire).unwrap_err().to_string();
    assert!(
        error.starts_with("layer 3 ")
            && error.contains(
                "sourceIntrinsicDuration 9000 ms differs from the packaged MP4 duration 10000 ms"
            ),
        "{error}"
    );
}

#[test]
fn current_layers_control_order_trims_and_deletion() {
    let mut wire = two_clips();
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    let project = convert(wire.clone()).unwrap();
    let starts: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .map(|o| o.start_ticks)
        .collect();
    assert_eq!(starts, [0, 3 * TICKS]);

    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    let project = convert(wire).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(
        (
            clip.start_ticks,
            clip.end_ticks,
            clip.in_ticks,
            clip.out_ticks
        ),
        (3 * TICKS, 5 * TICKS, 6 * TICKS, 8 * TICKS)
    );
}

#[test]
fn absolute_boundaries_snap_but_source_in_stays_off_the_sequence_grid() {
    for (start, frame) in [(10, 0), (17, 1), (33, 1), (40, 1), (50, 2), (100, 3)] {
        let mut wire = document();
        wire["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
            json!({"start": start, "duration": 1000 - start}),
            json!({"start": 7, "duration": 1000 - start}),
        );
        wire["composition"]["layers"][0]["sourceRange"] =
            json!({"start": 7, "duration": 1000 - start});
        let project = convert(wire).unwrap();
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!(
            (clip.start_ticks, clip.end_ticks),
            (frame * FRAME_30, TICKS)
        );
        assert_eq!(clip.in_ticks, 7 * TICKS_PER_MILLISECOND);
        assert_eq!(clip.out_ticks, clip.in_ticks + TICKS - clip.start_ticks);
    }
}

#[test]
fn generated_mixed_rate_media_end_project_loads_with_crate_reader() {
    // A 23.976 fps source rounds to a 42 ms source-in. The 30 fps export's
    // final sample is 125 us past the exact media end, within the 1.5 ms limit.
    let frame_rate = FrameRate::Fps24000Over1001;
    let media_frame = frame_rate.ticks_per_frame();
    let mut wire = document();
    wire["duration"] = json!(0.25);
    wire["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
        json!({"start":83,"duration":167}),
        json!({"start":42,"duration":167}),
    );
    wire["composition"]["layers"][0]["sourceRange"] = json!({"start":42,"duration":167});
    wire["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(209);
    wire["composition"]["layers"][1]["activeRange"]["duration"] = json!(250);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let facts = source(frame_rate, 5 * media_frame);

    let project = tesseract_to_premiere(
        &document,
        &facts,
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut Vec::new(),
    )
    .unwrap();
    let expected_timeline = 2 * FRAME_30..8 * FRAME_30;
    let expected_source = 42 * TICKS_PER_MILLISECOND..42 * TICKS_PER_MILLISECOND + 6 * FRAME_30;
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    let video = project.media(clip).unwrap().video.as_ref().unwrap();
    assert_eq!(video.frame_rate, frame_rate.into());
    assert_eq!(clip.timeline_ticks(), expected_timeline);
    assert_eq!(clip.source_ticks(), expected_source);
    assert_eq!(clip.out_ticks - FRAME_30 - 5 * media_frame, TICKS / 8_000);

    let loaded = write_and_load_with_crate_reader(project);
    let clip = loaded
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.timeline_ticks(), 2 * FRAME_30..8 * FRAME_30);
    assert_eq!(
        clip.source_ticks(),
        42 * TICKS_PER_MILLISECOND..42 * TICKS_PER_MILLISECOND + 6 * FRAME_30
    );
}

#[test]
fn snapping_keeps_adjacent_layers_adjacent() {
    let mut wire = two_clips();
    wire["duration"] = json!(0.1);
    for (index, (start, duration)) in [(0, (0, 50)), (1, (50, 50))] {
        wire["composition"]["layers"][index]["playback"] = crate::test_support::linear_playback(
            json!({"start":start, "duration":duration}),
            json!({"start":0, "duration":duration}),
        );
        wire["composition"]["layers"][index]["sourceRange"] =
            json!({"start":0, "duration":duration});
    }
    let project = convert(wire).unwrap();
    let clips: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    // 50 ms is halfway between frames 1 and 2; the shared cut goes forward.
    assert_eq!(clips[0].end_ticks, 2 * FRAME_30);
    assert_eq!(clips[0].end_ticks, clips[1].start_ticks);
    assert_eq!(clips[1].end_ticks, 3 * FRAME_30);
}

#[test]
fn snapping_rejects_layers_that_collapse_to_zero_frames() {
    for (start, end) in [(17, 33), (20, 40)] {
        let mut wire = document();
        wire["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
            json!({"start": start, "duration":end-start}),
            json!({"start":0,"duration":end-start}),
        );
        wire["composition"]["layers"][0]["sourceRange"] = json!({"start":0,"duration":end-start});
        let error = convert(wire).unwrap_err().to_string();
        assert!(
            error.contains("layer 1") && error.contains("collapses"),
            "{error}"
        );
    }
}

#[test]
fn mapped_video_window_outside_its_authored_selection_is_rejected() {
    let mut wire = document();
    // Keep the authored 1 s mapping while shortening only its source selection.
    wire["composition"]["layers"][0]["sourceRange"]["duration"] = json!(900);
    let error = convert(wire).unwrap_err().to_string();
    assert!(
        error.contains("layer 1")
            && error.contains("playback window extends beyond the authored source selection"),
        "{error}"
    );
}

#[test]
fn timeline_overflow_is_rejected_with_layer_context() {
    let mut wire = document();
    wire["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 40_000_000_000_u64, "duration": 1000}),
        json!({"start": 0, "duration": 1000}),
    );
    let error = convert(wire).unwrap_err().to_string();
    assert!(
        error.contains("layer 1") && error.contains("tick range"),
        "{error}"
    );
}

#[test]
fn supported_opacity_keeps_both_clips_and_only_reports_unsupported_effects() {
    let mut wire = two_clips();
    wire["composition"]["layers"][1]["transform"]["opacity"] = json!(50);
    wire["composition"]["layers"][1]["effects"] = json!([
        {"id": 1, "effect": {"type": "mosaic", "horizontalBlocks": 10.0, "verticalBlocks": 20.0}}
    ]);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let occurrences = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect::<Vec<_>>();
    assert_eq!(occurrences.len(), 2);
    assert!(occurrences.iter().any(|clip| clip.opacity == 50.0));
    assert!(omissions
        .iter()
        .all(|item| !item.reason.contains("opacity")));
    assert!(
        omissions
            .iter()
            .any(|item| item.reason.contains("effects") && item.record.contains("layer 3")),
        "{omissions:?}"
    );
}

#[test]
fn preserved_unknown_fields_are_reported_at_every_document_level() {
    for pointer in [
        "",
        "/composition",
        "/composition/layers/0",
        "/composition/layers/1",
    ] {
        let mut wire = document();
        wire.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("futureBehavior".into(), json!({"enabled": true}));
        let (_, omissions) = convert_with_omissions(wire).unwrap();
        assert!(
            omissions
                .iter()
                .any(|item| item.reason.contains("futureBehavior")),
            "{pointer}: {omissions:?}"
        );
    }
}

#[test]
fn edited_static_motion_exports_as_normalized_native_values() {
    let mut wire = document();
    let transform = &mut wire["composition"]["layers"][0]["transform"];
    transform["anchorPoint"] = json!([320, 270]);
    transform["position"] = json!([1440, 270]);
    transform["scale"] = json!([80, 125]);
    transform["rotation"] = json!(-15);

    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.transform.position, [0.75, 0.25]);
    assert_eq!(clip.transform.anchor_point, [1.0 / 6.0, 0.25]);
    assert_eq!(clip.transform.scale, [80.0, 125.0]);
    assert_eq!(clip.transform.rotation, -15.0);
    assert!(
        !omissions.iter().any(|item| {
            item.reason.contains("position/anchor")
                || item.reason.contains("static scale")
                || item.reason.contains("static rotation")
        }),
        "{omissions:?}"
    );
}

#[test]
fn a_clip_whose_scale_or_rotation_motion_cannot_show_is_omitted() {
    // Premiere's Motion holds Scale 0 to 10000 and Rotation -32768 to 32767.
    // (fields of the first clip's static transform, its tracks from their
    // first value at the source In to `key` at 500 ms, which they reach with
    // `easing`, the reason that omits the clip, or "" when it exports
    // unchanged)
    let flip = "flip (negative scale) was not exported";
    let scale: &[(&str, f64)] = &[("scaleX", 100.0), ("scaleY", 100.0)];
    let rotation: &[(&str, f64)] = &[("rotation", 0.0)];
    let linear = json!({"type": "linear"});
    // Between two keys inside the bounds, `dip` takes Scale from 100 to 200
    // through -55.1, and `back` passes the second key by 9.8 % of the step:
    // Scale from 100 to 9500 peaks at 10419 (to 9000, at 9870) and Rotation
    // from 0 to 30000 at 32934 (to 29000, at 31836).
    let dip = json!({"type": "cubicBezier", "x1": 0.25, "y1": -4.0, "x2": 0.75, "y2": 1.0});
    let back = json!({"type": "cubicBezier", "x1": 0.34, "y1": 1.56, "x2": 0.64, "y2": 1.0});
    // `vast_dip`, a finite handle that FX accepts, takes Scale from 100 to
    // 200 through about -4.4e156, and `quadratic_back`, whose timing curve
    // is quadratic up to rounding, passes the second key by 12.5 % of the
    // step: Scale from 100 to 9000 peaks at 10112.5.
    let vast_dip = json!({"type": "cubicBezier", "x1": 0.25, "y1": -1e155, "x2": 0.75, "y2": 1.0});
    let quadratic_back =
        json!({"type": "cubicBezier", "x1": 0.5, "y1": 1.0, "x2": 0.5, "y2": 4.0 / 3.0});
    #[rustfmt::skip]
    let rows = [
        (json!({"scale": [-50.0, 50.0]}), &[][..], 0.0, &linear, flip),
        (json!({}), scale, -100.0, &linear, flip),
        (json!({}), scale, 200.0, &dip, flip),
        (json!({}), scale, 200.0, &vast_dip, flip),
        (json!({"scale": [20000.0, 20000.0]}), &[][..], 0.0, &linear, "static scale exceeds Premiere's supported range"),
        (json!({}), scale, 20000.0, &linear, "Scale animation exceeds Premiere's supported range"),
        (json!({}), scale, 9500.0, &back, "Scale animation exceeds Premiere's supported range"),
        (json!({}), scale, 9000.0, &quadratic_back, "Scale animation exceeds Premiere's supported range"),
        (json!({"rotation": 40000.0}), &[][..], 0.0, &linear, "static rotation exceeds Premiere's supported range"),
        (json!({}), rotation, 40000.0, &linear, "Rotation animation exceeds Premiere's supported range"),
        (json!({}), rotation, 30000.0, &back, "Rotation animation exceeds Premiere's supported range"),
        (json!({"scale": [0.0, 10000.0], "rotation": -32768.0}), &[][..], 0.0, &linear, ""),
        (json!({"rotation": 32767.0}), scale, 10000.0, &linear, ""),
        (json!({}), scale, 9000.0, &back, ""),
        (json!({}), rotation, 29000.0, &back, ""),
    ];
    for (fields, tracks, key, easing, reason) in rows {
        let mut wire = two_clips();
        let transform = &mut wire["composition"]["layers"][0]["transform"];
        for (field, value) in fields.as_object().unwrap() {
            transform[field] = value.clone();
        }
        let transform = transform.clone();
        let entries: Vec<_> = tracks
            .iter()
            .map(|(property, first)| {
                let keys = [(0, first, &linear), (500, &key, easing)].map(|(time, value, easing)| {
                    json!({"id": format!("{property}-{time}"), "layerTime": time, "value": {"type": "float", "value": value}, "easing": easing})
                });
                json!({
                    "target": {"kind": "layer", "layerId": 1, "propertyType": property},
                    "animator": {"type": "keyframes", "enabled": true, "keyframes": keys}
                })
            })
            .collect();
        wire["composition"]["dynamics"] = json!({ "entries": entries });
        let case = format!("{fields} {tracks:?} {key} {easing}");
        // Media inspection skips the clip that export omits.
        let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
        let composition = document.composition();
        let inspected: Vec<_> = crate::convert::exported_video_layers(
            composition.layers(),
            composition.dynamics(),
            [1920, 1080],
        )
        .into_iter()
        .map(Layer::id)
        .collect();
        let kept = if reason.is_empty() { &[1, 3][..] } else { &[3] };
        assert_eq!(
            inspected,
            kept.iter().map(|id| LayerId::new(*id)).collect::<Vec<_>>(),
            "{case}"
        );
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let clips: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .collect();
        if reason.is_empty() {
            // Premiere's bounds themselves, and curves inside them, export
            // unchanged.
            assert!(omissions.is_empty(), "{case}: {omissions:?}");
            let fx_scale: Vec<f64> = serde_json::from_value(transform["scale"].clone()).unwrap();
            assert_eq!(clips[0].transform.scale.to_vec(), fx_scale, "{case}");
            assert_eq!(
                Some(clips[0].transform.rotation),
                transform["rotation"].as_f64()
            );
            let keys: Vec<_> = clips[0]
                .animations
                .iter()
                .flat_map(|animation| animation.keys())
                .map(|native| native.value)
                .collect();
            let fx_keys = tracks
                .first()
                .map_or(vec![], |(_, first)| vec![*first, key]);
            assert_eq!(keys, fx_keys, "{case}");
            continue;
        }
        // The clip is omitted whole, its keys with it; its sibling exports.
        let starts: Vec<_> = clips.iter().map(|clip| clip.start_ticks).collect();
        assert_eq!(starts, [3 * TICKS], "{case}");
        let mut expected = vec![crate::Omission {
            scope: crate::OmissionScope::Occurrence,
            kind: crate::OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".into(),
            reason: reason.into(),
        }];
        if !tracks.is_empty() {
            expected.push(crate::Omission {
                scope: crate::OmissionScope::Feature,
                kind: crate::OmissionKind::Omitted,
                record: "layer 1".into(),
                reason: "animation on an omitted or unsupported layer was not exported".into(),
            });
        }
        assert_eq!(omissions, expected, "{case}");
    }
}

#[test]
fn edited_static_motion_survives_export_alongside_rotation_keys() {
    use crate::{
        schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe},
        tests::support::{project_document, video_sequence},
    };
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
                source_ticks: TICKS,
                value: 90.0,
                easing: PrKeyframeEasing::Linear,
            },
        ]));
    let mut wire = project_document(&sequence);
    let transform = &mut wire["composition"]["layers"][0]["transform"];
    transform["anchorPoint"] = json!([240, 810]);
    transform["position"] = json!([480, 270]);
    transform["scale"] = json!([75, 75]);
    transform["rotation"] = json!(12);

    let project = convert(wire).unwrap();
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.transform.anchor_point, [0.125, 0.75]);
    assert_eq!(clip.transform.position, [0.25, 0.25]);
    assert_eq!(clip.transform.scale, [75.0, 75.0]);
    assert_eq!(clip.transform.rotation, 12.0);
    assert_eq!(clip.animations.len(), 1);
}

#[test]
fn unsupported_video_properties_and_transforms_are_reported() {
    for (field, value, expected) in [
        (
            "preserveAudioPitch",
            json!(true),
            "audio pitch preservation",
        ),
        (
            "effects",
            json!([
                {"id": 1, "effect": {"type": "mosaic", "horizontalBlocks": 10.0, "verticalBlocks": 20.0}}
            ]),
            "effects",
        ),
    ] {
        let mut wire = document();
        wire["composition"]["layers"][0][field] = value;
        let (_, omissions) = convert_with_omissions(wire).unwrap();
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == crate::OmissionScope::Feature
                    && item.record.contains("layer 1")
                    && item.reason.contains(expected)),
            "{field}: {omissions:?}"
        );
    }
    for (field, value, expected) in [
        ("skew", json!(10), "skew"),
        ("rotationX", json!(15), "3D rotation"),
    ] {
        let mut wire = document();
        wire["composition"]["layers"][0]["transform"][field] = value;
        let (_, omissions) = convert_with_omissions(wire).unwrap();
        assert!(
            omissions.iter().any(|item| item.reason.contains(expected)),
            "{field}: {omissions:?}"
        );
    }

    let mut wire = document();
    wire["composition"]["layers"][0]["transform"]["opacity"] = json!(90);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions
        .iter()
        .all(|item| !item.reason.contains("opacity")));
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap()
            .opacity,
        90.0
    );
}

#[test]
fn every_fx_blend_mode_exports_as_its_premiere_mode_or_the_nearest() {
    use crate::schema::PrBlendMode;
    for (wire_mode, expected, report) in [
        ("normal", PrBlendMode::Normal, None),
        ("screen", PrBlendMode::Screen, None),
        ("multiply", PrBlendMode::Multiply, None),
        (
            "lighterColor",
            PrBlendMode::LighterColor,
            Some((OmissionKind::Approximated, "Lighter Color picks")),
        ),
        // FX's classic After Effects modes export as the nearest mode.
        (
            "classicColorBurn",
            PrBlendMode::ColorBurn,
            Some((OmissionKind::Approximated, "exports as the nearest")),
        ),
    ] {
        let mut wire = document();
        wire["composition"]["layers"][0]["blendMode"] = json!(wire_mode);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let actual = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap()
            .blend_mode;
        assert_eq!(actual, expected, "{wire_mode}");
        let reports: Vec<_> = omissions
            .iter()
            .filter(|item| item.reason.to_lowercase().contains("blend mode"))
            .map(|item| (item.kind, item.reason.as_str()))
            .collect();
        match report {
            None => assert!(reports.is_empty(), "{wire_mode}: {reports:?}"),
            Some((kind, text)) => assert!(
                matches!(reports.as_slice(), [(actual, reason)] if *actual == kind && reason.contains(text)),
                "{wire_mode}: {reports:?}"
            ),
        }
    }
}

#[test]
fn a_rounded_nonblack_bottom_rectangle_is_a_graphic_below_the_video() {
    let mut wire = document();
    wire["composition"]["layers"][1]["rect"]["fillColor"] = json!([1.0, 0.0, 0.0, 1.0]);
    // Rounded, so it is no Color Matte but a graphic Shape
    // (`convert::color_matte` covers its outline).
    wire["composition"]["layers"][1]["rect"]["roundness"] = json!(12);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let graphic_tracks: Vec<Vec<bool>> = project
        .single_sequence()
        .unwrap()
        .video_tracks()
        .map(|track| track.iter().map(|item| item.graphic().is_some()).collect())
        .collect();
    assert_eq!(graphic_tracks, [vec![true], vec![false]]);
}

#[test]
fn black_canvas_is_semantically_validated_and_required_for_gaps() {
    let mut wire = document();
    wire["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 500, "duration": 500}),
        json!({"start": 0, "duration": 500}),
    );
    wire["composition"]["layers"][0]["sourceRange"]["duration"] = json!(500);
    wire["composition"]["layers"][1]["isHidden"] = json!(false);
    convert(wire.clone()).unwrap();
    // A bottom rectangle that is not the canvas converts as an authored Color
    // Matte or graphic Shape (`convert::color_matte` covers both). These are
    // no canvas, so they leave the gap uncovered; a short canvas is still a
    // canvas and fails coverage.
    for (pointer, value) in [
        ("/isHidden", json!(true)),
        ("/transform/position", json!([1, 0])),
        ("/activeRange/duration", json!(400)),
        ("/transform/opacity", json!(50)),
        ("/rect/size", json!([1919, 1080])),
    ] {
        let mut bad = wire.clone();
        *bad["composition"]["layers"][1]
            .pointer_mut(pointer)
            .unwrap() = value;
        let error = convert(bad).unwrap_err().to_string();
        assert!(
            error.contains("gaps require an explicit"),
            "{pointer}: {error}"
        );
    }
    let mut missing = wire.clone();
    missing["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(convert(missing)
        .unwrap_err()
        .to_string()
        .contains("gaps require an explicit"));
    // Above other layers, the same black rectangle is an authored black matte.
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let project = convert(wire).unwrap();
    let top = project
        .single_sequence()
        .unwrap()
        .video_tracks()
        .last()
        .unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(
        project
            .media(top[0].media().unwrap())
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .kind,
        PrMediaKind::ColorMatte(crate::schema::PrColorMatte { rgb: [0, 0, 0] })
    );
}

#[test]
fn black_canvas_only_needs_to_cover_the_actual_gap() {
    for (canvas_duration, accepted) in [(400, false), (500, true)] {
        let mut wire = document();
        wire["backgroundColor"] = json!([0, 0, 0, 1]);
        wire["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
            json!({"start": 500, "duration": 500}),
            json!({"start": 0, "duration": 500}),
        );
        wire["composition"]["layers"][0]["sourceRange"]["duration"] = json!(500);
        wire["composition"]["layers"][1]["activeRange"]["duration"] = json!(canvas_duration);
        let result = convert(wire);
        assert_eq!(
            result.is_ok(),
            accepted,
            "canvas duration {canvas_duration}: {result:?}"
        );
    }
}

#[test]
fn text_past_the_last_video_needs_the_canvas_under_its_tail() {
    // Text is transparent, so time that only text covers is still a gap.
    for (canvas_duration, accepted) in [(1000, false), (1500, true)] {
        let mut wire = with_text(json!({"activeRange": {"start": 500, "duration": 1000}}));
        wire["duration"] = json!(1.5);
        wire["composition"]["layers"][2]["activeRange"]["duration"] = json!(canvas_duration);
        match convert_with_fonts(wire, &inter_bold()).map(|(project, _)| project) {
            Ok(project) => {
                assert!(accepted, "canvas duration {canvas_duration}");
                assert_eq!(
                    project.single_sequence().unwrap().end_ticks(),
                    3 * TICKS / 2
                );
            }
            Err(error) => {
                assert!(!accepted, "canvas duration {canvas_duration}: {error}");
                assert!(
                    error.to_string().contains("gaps require an explicit"),
                    "{error}"
                );
            }
        }
    }
}

#[test]
fn appending_an_opaque_video_does_not_require_editing_an_invisible_canvas() {
    let mut wire = document();
    wire["duration"] = json!(2);
    let mut second = wire["composition"]["layers"][0].clone();
    second["id"] = json!(3);
    second["playback"]["inputRange"]["start"] = json!(1000);
    second["playback"]["mapping"]["input"]["start"] = json!(1000);
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, second);
    convert(wire).unwrap();
}

#[test]
fn existing_opaque_black_background_field_is_supported() {
    let mut wire = document();
    wire["backgroundColor"] = json!([0.0, 0.0, 0.0, 1.0]);
    convert(wire).unwrap();
}

#[test]
fn placement_preserves_precedence_and_reuses_adjacent_tracks() {
    for (starts, duration, expected) in [
        (&[2000, 1000, 0][..], 2000, vec![vec![4], vec![2], vec![0]]),
        (&[2000, 1000, 0][..], 1000, vec![vec![4, 2, 0]]),
        (&[0, 0, 0][..], 2000, vec![vec![4], vec![2], vec![0]]),
        (&[0][..], 2000, vec![vec![0]]),
    ] {
        let mut wire = two_clips();
        let layers: Vec<_> = starts
            .iter()
            .enumerate()
            .map(|(index, start)| {
                let mut layer = wire["composition"]["layers"][0].clone();
                layer["id"] = json!(index + 10);
                layer["playback"] = crate::test_support::linear_playback(
                    json!({"start":start,"duration":duration}),
                    json!({"start":index*2000,"duration":duration}),
                );
                layer["sourceRange"] = json!({"start":index*2000,"duration":duration});
                layer
            })
            .collect();
        wire["duration"] = json!((starts.iter().max().unwrap() + duration) as f64 / 1000.0);
        wire["composition"]["layers"] = json!(layers);
        let project = convert(wire).unwrap();
        let actual: Vec<Vec<_>> = project
            .single_sequence()
            .unwrap()
            .video_tracks()
            .map(|track| {
                track
                    .iter()
                    .map(|item| item.media().unwrap().in_ticks / TICKS)
                    .collect()
            })
            .collect();
        assert_eq!(actual, expected, "{starts:?}, {duration}");
    }
}

#[test]
fn canvas_covers_union_gaps_and_duration_uses_the_latest_video_end() {
    for (start, duration, canvas_start, canvas_duration, accepted) in [
        (1000, 4000, 0, 0, true),
        (3000, 2000, 0, 0, false),
        (3000, 2000, 2000, 1000, true),
        (3000, 2000, 2000, 500, false),
    ] {
        let mut wire = two_clips();
        let layers = wire["composition"]["layers"].as_array_mut().unwrap();
        layers[1]["playback"] = crate::test_support::linear_playback(
            json!({"start":start,"duration":duration}),
            json!({"start":0,"duration":duration}),
        );
        layers[1]["sourceRange"] = json!({"start":0,"duration":duration});
        if canvas_duration == 0 {
            layers.pop();
        } else {
            layers.last_mut().unwrap()["activeRange"] =
                json!({"start":canvas_start,"duration":canvas_duration});
        }
        let result = convert(wire);
        assert_eq!(result.is_ok(), accepted, "{result:?}");
        if !accepted {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("gaps require an explicit"));
        }
    }
    // A document that ends before its last occurrence exports that occurrence
    // end and reports the difference; one that ends after it has a trailing
    // gap, which needs the canvas like any other gap.
    let mut wire = two_clips();
    wire["duration"] = json!(4.0);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert_eq!(project.single_sequence().unwrap().end_ticks(), 5 * TICKS);
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "document.duration"),
        "{omissions:?}"
    );
    let mut wire = two_clips();
    wire["duration"] = json!(6.0);
    let error = convert(wire).unwrap_err().to_string();
    assert!(error.contains("gaps require an explicit"), "{error}");
}

#[test]
fn trailing_empty_time_exports_as_the_work_area_without_touching_occurrences() {
    // Occurrences end at 5 s; the document ends at 8 s.
    let mut wire = two_clips();
    wire["duration"] = json!(8);
    wire["composition"]["layers"][2]["activeRange"]["duration"] = json!(8_000);
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .filter(|item| item.record == "document.duration")
        .map(|item| item.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        [format!(
            "duration differs from the last occurrence; exported duration is {} ticks",
            5 * TICKS
        )],
        "Premiere renders only to the last occurrence, so the tail is reported"
    );
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.occurrence_end_ticks(), 5 * TICKS);
    assert_eq!(sequence.end_ticks(), 8 * TICKS);
    assert_eq!(
        sequence.gaps(&project.media),
        vec![2 * TICKS..3 * TICKS, 5 * TICKS..8 * TICKS]
    );
    let ranges: Vec<_> = sequence
        .video_occurrences()
        .map(PrVideoOccurrence::timeline_ticks)
        .collect();
    assert_eq!(ranges, [0..2 * TICKS, 3 * TICKS..5 * TICKS]);
}

#[test]
fn no_convertible_video_does_not_claim_success() {
    let mut wire = document();
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let error = convert_with_omissions(wire).unwrap_err();
    assert!(
        error.to_string().contains("no convertible video"),
        "{error}"
    );
}

#[test]
fn unsupported_upper_layer_types_are_omitted_with_source_context() {
    let mut wire = two_clips();
    let layer = &mut wire["composition"]["layers"][0];
    layer["name"] = json!("Unsupported upper");
    // Root shape layers export as graphics; PAG layers have no clip.
    layer["activeRange"] = layer["playback"]["inputRange"].clone();
    layer.as_object_mut().unwrap().remove("playback");
    layer["type"] = json!("Pag");
    layer["items"] = json!([{"assetId": "sticker"}]);
    for key in [
        "source",
        "sourceRange",
        "sourceIntrinsicDuration",
        "volume",
        "transform",
    ] {
        layer.as_object_mut().unwrap().remove(key);
    }
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.record.contains("Unsupported upper")
                && item.reason.contains("layer type")),
        "{omissions:?}"
    );
}

fn audio_layer(id: u64, asset_id: &str, start: u64, volume: f64) -> Value {
    json!({
        "type": "Audio",
        "id": id,
        "name": format!("Audio {id}"),
        "playback": crate::test_support::linear_playback(json!({"start": start, "duration": 700}), json!({"start": 250, "duration": 700})),
        "sourceRange": {"start": 250, "duration": 700},
        "sourceIntrinsicDuration": SOURCE_MILLIS,
        "volume": volume,
        "source": {"assetId": asset_id},
    })
}

#[test]
fn off_grid_audio_end_extends_the_document_and_survives_native_export() {
    for millis in [6001, 6020] {
        let mut wire = two_clips();
        wire["duration"] = json!(f64::from(millis) / 1000.0);
        let layers = wire["composition"]["layers"].as_array_mut().unwrap();
        // Canvas coverage still snaps to video frames, so cover the next frame.
        layers[2]["activeRange"]["duration"] = json!(6034);
        let mut audio = audio_layer(4, "music", 0, 1.0);
        audio["playback"] = crate::test_support::linear_playback(
            json!({"start": audio["playback"]["inputRange"]["start"], "duration": millis}),
            json!({"start": audio["sourceRange"]["start"], "duration": millis}),
        );
        audio["sourceRange"]["duration"] = json!(millis);
        layers.insert(0, audio);
        let (mut project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let end = i64::from(millis) * TICKS_PER_MILLISECOND;
        assert_eq!(project.sequences[0].end_ticks(), end);
        for (id, media) in &mut project.media {
            let name = if id.as_str() == "music" {
                "music.wav"
            } else {
                "source.mp4"
            };
            media.name = name.into();
            media.relative_path = Some(format!("./media/{name}"));
            media.relative_paths = vec![format!("./media/{name}")];
            media.absolute_paths =
                vec![(MediaPathField::FilePath, format!("/media/{name}").into())];
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("project.prproj");
        PremiereProjectXml::new(&project)
            .unwrap()
            .write_new(&path)
            .unwrap();
        let text = crate::format::read_xml(&path).unwrap();
        let xml = roxmltree::Document::parse(&text).unwrap();
        for tag in ["MZ.WorkOutPoint", "OriginalDuration"] {
            let count = xml
                .descendants()
                .filter(|node| {
                    node.has_tag_name(tag) && node.text() == Some(end.to_string().as_str())
                })
                .count();
            assert_eq!(count, if tag == "MZ.WorkOutPoint" { 1 } else { 2 });
        }
        let (loaded, omissions) = PrProjectFile::load(&path).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(loaded.sequences[0].timeline_end_ticks, 5 * TICKS);
        assert_eq!(loaded.sequences[0].end_ticks(), end);
        let document =
            crate::tests::support::project_document_with_media(&loaded.sequences[0], &loaded.media);
        assert_eq!(document["duration"], f64::from(millis) / 1000.0);
        assert_eq!(
            (*crate::test_support::layer_range(&document["composition"]["layers"][3]))["duration"],
            millis
        );
    }
}

#[test]
fn audio_layers_and_audible_video_become_separate_sound_placements() {
    let mut wire = document();
    wire["duration"] = json!(1.2);
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    // 1001 ms is off the 30 fps grid: picture snaps, sound keeps the millisecond.
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": layers[0]["playback"]["inputRange"]["start"], "duration": 1001}),
        json!({"start": layers[0]["sourceRange"]["start"], "duration": 1001}),
    );
    layers[0]["sourceRange"]["duration"] = json!(1001);
    layers[0]["volume"] = json!(0.5);
    layers[1]["activeRange"]["duration"] = json!(1200);
    layers.insert(0, audio_layer(3, "music", 500, 2.0));
    layers.insert(0, audio_layer(4, "premiere-video-1", 0, 1.0));
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let picture = sequence.video_occurrences().next().unwrap();
    assert_eq!(picture.timeline_ticks(), 0..30 * FRAME_30);
    // Bottom to top: the video layer's own sound, then the two explicit layers.
    let audio: Vec<_> = sequence
        .audio
        .iter()
        .map(|clip| {
            (
                clip.media.as_str(),
                clip.start_ticks..clip.end_ticks,
                clip.in_ticks..clip.out_ticks,
                clip.volume.as_f64(),
            )
        })
        .collect();
    let ms = TICKS_PER_MILLISECOND;
    assert_eq!(
        audio,
        [
            ("premiere-video-1", 0..1001 * ms, 0..1001 * ms, 0.5),
            ("music", 500 * ms..1200 * ms, 250 * ms..950 * ms, 2.0),
            ("premiere-video-1", 0..700 * ms, 250 * ms..950 * ms, 1.0),
        ]
    );
    let linked = &project.media[&MediaId("premiere-video-1".into())];
    assert!(
        linked.video.is_some()
            && linked.audio.clone().map(SourceSound::Supported)
                == sound().remove("premiere-video-1")
    );
    let music = &project.media[&MediaId("music".into())];
    assert!(music.video.is_none() && music.audio.is_some());
}

/// An `AudioVolume` key track of one layer.
fn volume_entry(layer_id: u64, keys: &[(i64, f64, Value)]) -> Value {
    json!({
        "target": {"kind": "layer", "layerId": layer_id, "propertyType": "volume"},
        "animator": {
            "type": "keyframes",
            "enabled": true,
            "keyframes": keys
                .iter()
                .enumerate()
                .map(|(index, (time, value, easing))| json!({
                    "id": format!("volume-{layer_id}-{index}"),
                    "layerTime": time,
                    "value": {"type": "float", "value": value},
                    "easing": easing,
                }))
                .collect::<Vec<_>>(),
        },
        "dependencies": [],
        "layerRefs": {},
    })
}

#[test]
fn volume_keys_export_as_clip_volume_keys_on_the_source_clock() {
    use crate::schema::PrKeyframeEasing::{Hold, Linear};
    let mut wire = document();
    wire["duration"] = json!(1.2);
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    layers[1]["activeRange"]["duration"] = json!(1200);
    // A picture whose sound plays only through its keys.
    layers[0].as_object_mut().unwrap().remove("volume");
    layers.insert(0, audio_layer(3, "music", 500, 1.0));
    let cubic = json!({"type": "cubicBezier", "x1": 0.2, "y1": 0.1, "x2": 0.8, "y2": 0.9});
    wire["composition"]["dynamics"] = json!({"entries": [
        volume_entry(3, &[
            (-100, 1.0, json!({"type": "linear"})),
            (200, 0.1, cubic),
            (500, 2.0, json!({"type": "hold"})),
        ]),
        volume_entry(1, &[(0, 0.0, json!({"type": "linear"})), (500, 1.0, json!({"type": "linear"}))]),
    ]});
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    // Neither curve is Premiere's, so Linear pieces follow it; the fade from
    // silence changes too fast for its first milliseconds.
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(
        (omissions[0].scope, omissions[0].record.as_str()),
        (OmissionScope::Feature, "layer 1 (\"Source\")")
    );
    assert!(omissions[0]
        .reason
        .starts_with("volume curve approximated, not removed: "));
    let audio = &project.single_sequence().unwrap().audio;
    let ms = TICKS_PER_MILLISECOND;
    // Bottom to top: each FX key on the source clock (In 0 and 250 ms), with
    // Linear keys on the millisecond grid inside each segment that is not Hold.
    for (clip, media, volume, fx_keys) in [
        (
            &audio[0],
            "premiere-video-1",
            0.0,
            &[(0, 0.0, Linear), (500 * ms, 1.0, Linear)][..],
        ),
        (
            &audio[1],
            "music",
            1.0,
            &[
                (150 * ms, 1.0, Linear),
                (450 * ms, 0.1, Linear),
                (750 * ms, 2.0, Hold),
            ][..],
        ),
    ] {
        assert_eq!((clip.media.as_str(), clip.volume.as_f64()), (media, volume));
        let keys = clip.volume_keys.as_ref().unwrap();
        assert_eq!(keys.gain, 1.0);
        let written: Vec<_> = keys
            .keys
            .iter()
            .map(|key| (key.source_ticks, key.value, key.easing))
            .collect();
        assert!(written.len() > fx_keys.len());
        assert!(written.iter().all(|key| (key.0 - clip.in_ticks) % ms == 0));
        for pair in fx_keys.windows(2) {
            let inner: Vec<_> = written
                .iter()
                .filter(|key| key.0 > pair[0].0 && key.0 < pair[1].0)
                .collect();
            assert_eq!(inner.is_empty(), pair[1].2 == Hold, "{written:?}");
            assert!(inner.iter().all(|key| key.2 == Linear));
        }
        for key in fx_keys {
            assert!(written.contains(key), "{key:?} in {written:?}");
        }
    }
}

#[test]
fn volume_keys_beyond_the_former_limit_preserve_audio_and_siblings() {
    let holds = |count: usize| {
        (0..count)
            .map(|index| (index as i64, [1.0, 0.5][index % 2], json!({"type": "hold"})))
            .collect::<Vec<_>>()
    };
    // The picture (layer 1) with its own sound at `volume`, and a music layer
    // (3) beside it at 0.5; `keys` animate the volume of layer `layer_id`.
    let convert_keys = |layer_id: u64, keys: &[(i64, f64, Value)], volume: Option<f64>| {
        let mut wire = document();
        let layers = wire["composition"]["layers"].as_array_mut().unwrap();
        match volume {
            Some(volume) => layers[0]["volume"] = json!(volume),
            None => drop(layers[0].as_object_mut().unwrap().remove("volume")),
        }
        layers.insert(0, audio_layer(3, "music", 0, 0.5));
        wire["composition"]["dynamics"] = json!({"entries": [volume_entry(layer_id, keys)]});
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1);
        let sounds: Vec<_> = sequence
            .audio
            .iter()
            .map(|clip| {
                let keys = clip.volume_keys.as_ref().map(|keys| keys.keys.len());
                (clip.media.as_str().to_owned(), clip.volume.as_f64(), keys)
            })
            .collect();
        (project, sounds, omissions)
    };
    let count = 4097;
    for layer in [1, 3] {
        {
            let volume = Some(1.0);
            let (mut project, sounds, omissions) = convert_keys(layer, &holds(count), volume);
            assert!(omissions.is_empty(), "{omissions:?}");
            let keyed = if layer == 1 {
                "premiere-video-1"
            } else {
                "music"
            };
            assert_eq!(sounds.len(), 2);
            assert_eq!(
                sounds.iter().find(|(id, _, _)| id == keyed).unwrap().2,
                Some(count)
            );
            // The picture's single source can be written and read back.
            if layer == 1 {
                project.sequences[0]
                    .audio
                    .retain(|sound| sound.media.as_str() == keyed);
                project.media.retain(|id, _| id.as_str() == keyed);
                let loaded = write_and_load_with_crate_reader(project);
                assert_eq!(
                    loaded.single_sequence().unwrap().audio[0]
                        .volume_keys
                        .as_ref()
                        .unwrap()
                        .keys
                        .len(),
                    count
                );
            }
        }
    }

    // The Linear fade is subdivided as needed, beyond the old key count.
    let mut fade = holds(4090);
    fade.last_mut().unwrap().1 = 0.0;
    fade.push((5089, 1.0, json!({"type": "linear"})));
    let (_, sounds, omissions) = convert_keys(1, &fade, Some(1.0));
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(omissions[0]
        .reason
        .starts_with("volume curve approximated, not removed:"));
    assert!(sounds[0].2.unwrap() > 4096);
}

#[test]
fn a_group_sound_exports_into_its_nest_with_its_volume_keys() {
    let mut wire = document();
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    let mut video = layers.remove(0);
    video["parent"] = json!(10);
    video.as_object_mut().unwrap().remove("volume");
    let mut sound = audio_layer(11, "music", 0, 1.0);
    sound["parent"] = json!(10);
    layers.insert(
        0,
        json!({
            "type": "Group", "id": 10, "name": "Inner", "blendMode": "normal",
            "playback": crate::test_support::linear_playback(json!(*crate::test_support::layer_range(&video)), json!({"start": 0, "duration": (*crate::test_support::layer_range(&video))["duration"]})), "transform": layers[0]["transform"],
            "layers": [video, sound]
        }),
    );
    let linear = json!({"type": "linear"});
    wire["composition"]["dynamics"] = json!({"entries": [
        volume_entry(1, &[(0, 0.0, linear.clone()), (500, 1.0, linear.clone())]),
        volume_entry(11, &[(0, 1.0, linear.clone()), (500, 0.5, linear)]),
    ]});
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|item| (item.record.as_str(), item.reason.as_str()))
        .collect();
    // The group's sound and its keys export into the nest; the embedded sound
    // of the video inside it does not (D-IN2-3).
    assert_eq!(
        reasons,
        [(
            "group 10, layer 1 (\"Source\")",
            "embedded clip sound inside a nested sequence is not exported"
        )]
    );
    let outer = project.single_sequence().unwrap();
    assert!(outer.audio.is_empty());
    let inner = &outer.nest_occurrences().next().unwrap().sequence;
    assert_eq!(inner.audio.len(), 1);
    assert!(inner.audio[0].volume_keys.is_some());
}

#[test]
fn audio_layer_animation_beside_its_volume_keys_is_reported() {
    // The sound exports its volume keys; the Opacity keys of the same audio
    // layer are reported, not dropped with them.
    let mut wire = document();
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    layers.insert(0, audio_layer(3, "music", 0, 1.0));
    wire["composition"]["dynamics"] = json!({"entries": [
        volume_entry(3, &[
            (0, 1.0, json!({"type": "linear"})),
            (500, 0.5, json!({"type": "hold"})),
        ]),
        {
            "target": {"kind": "layer", "layerId": 3, "propertyType": "opacity"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "opacity-start", "layerTime": 0, "value": {"type": "float", "value": 1.0}, "easing": {"type": "linear"}},
                {"id": "opacity-end", "layerTime": 500, "value": {"type": "float", "value": 0.5}, "easing": {"type": "linear"}}
            ]}
        },
    ]});
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let reasons: Vec<_> = omissions
        .iter()
        .map(|item| (item.scope, item.record.as_str(), item.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        [(
            OmissionScope::Feature,
            "layer 3",
            "animation on an omitted or unsupported layer was not exported"
        )]
    );
    let sounds: Vec<_> = project
        .single_sequence()
        .unwrap()
        .audio
        .iter()
        .map(|clip| {
            (
                clip.media.as_str(),
                clip.volume_keys.as_ref().map(|keys| keys.keys.len()),
            )
        })
        .collect();
    assert_eq!(sounds, [("music", Some(2))]);
}

#[test]
fn hidden_audible_video_exports_its_disabled_picture_without_sound() {
    let mut wire = document();
    let video = &mut wire["composition"]["layers"][0];
    video["isHidden"] = json!(true);
    video["volume"] = json!(1.0);

    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let sequence = project.single_sequence().unwrap();
    let picture = sequence.video_occurrences().next().unwrap();
    assert!(!picture.enabled);
    assert_eq!(picture.timeline_ticks(), 0..TICKS);
    assert_eq!(picture.source_ticks(), 0..TICKS);
    assert!(sequence.audio.is_empty());
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Feature);
    assert_eq!(omissions[0].record, "layer 1 (\"Source\")");
    assert_eq!(
        omissions[0].reason,
        "hidden video's embedded audio is not exported; whether the FX renderer mutes hidden layers is unverified"
    );
}

#[test]
fn retimed_audible_video_exports_its_picture_without_sound() {
    let mut wire = document();
    wire["duration"] = json!(2);
    let video = &mut wire["composition"]["layers"][0];
    video["volume"] = json!(1.0);
    video["playback"] = crate::test_support::linear_playback(
        json!({"start": 0, "duration": 2000}),
        json!({"start": 4000, "duration": 1000}),
    );
    video["sourceRange"] = json!({"start": 4000, "duration": 1000});
    video["sourceIntrinsicDuration"] = json!(SOURCE_MILLIS);
    video["playback"] = crate::test_support::remapped_playback(
        crate::test_support::layer_range(video).clone(),
        json!({
            "keyframes": [
                {"id":"start", "time":0, "value":5000, "easing":{"type":"linear"}},
                {"id":"end", "time":2000, "value":4000, "easing":{"type":"linear"}}
            ],
            "before":"inactive",
            "after":"inactive"
        }),
    );
    wire["composition"]["layers"][1]["activeRange"]["duration"] = json!(2000);

    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        sequence.video_occurrences().next().unwrap().playback_rate,
        -0.5
    );
    assert!(sequence.audio.is_empty());
    assert!(
        omissions
            .iter()
            .any(|item| item.reason == "embedded audio of a retimed video layer was not exported"),
        "{omissions:?}"
    );
}

#[test]
fn audible_video_without_sound_exports_only_the_picture() {
    let mut wire = document();
    wire["composition"]["layers"][0]["volume"] = json!(1.0);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let media = source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND);
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &media,
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert!(project.single_sequence().unwrap().audio.is_empty());
}

#[test]
fn unsupported_audio_properties_are_omitted_and_invalid_ranges_reject() {
    let with_audio = |key: &str, value: Value| {
        let mut wire = document();
        let mut layer = audio_layer(3, "music", 0, 1.0);
        layer[key] = value;
        wire["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .insert(0, layer);
        wire
    };
    for (key, value, reason) in [
        ("isHidden", json!(true), "hidden audio"),
        (
            "preserveAudioPitch",
            json!(true),
            "audio pitch preservation",
        ),
    ] {
        let (_, omissions) = convert_with_omissions(with_audio(key, value)).unwrap();
        assert!(
            omissions
                .iter()
                .any(|item| item.record.contains("Audio 3") && item.reason.contains(reason)),
            "{key}: {omissions:?}"
        );
    }
    for (key, value, reason) in [
        (
            "sourceRange",
            json!({"start": 9500, "duration": 700}),
            "audio playback window extends beyond the authored source selection",
        ),
        (
            "sourceRange",
            json!({"start": 250, "duration": 600}),
            "audio playback window extends beyond the authored source selection",
        ),
        (
            "sourceIntrinsicDuration",
            json!(9000),
            "sourceIntrinsicDuration",
        ),
    ] {
        let error = convert(with_audio(key, value)).unwrap_err().to_string();
        assert!(
            error.contains("Audio 3") && error.contains(reason),
            "{key}: {error}"
        );
    }
}

#[test]
fn audio_playback_keeping_its_duration_exports_while_a_mismatched_retime_rejects() {
    // An audio layer whose `playback` plays `source_millis` of source in 700 ms.
    let with_playback = |source_millis: i64, intrinsic_millis: i64| {
        let mut wire = document();
        let mut sound = audio_layer(3, "music", 0, 1.0);
        sound["sourceRange"]["duration"] = json!(source_millis);
        sound["sourceIntrinsicDuration"] = json!(intrinsic_millis);
        sound["playback"] = crate::test_support::remapped_playback(
            crate::test_support::layer_range(&sound).clone(),
            json!({"keyframes": [
            {"id": "start", "time": 0, "value": 250, "easing": {"type": "linear"}},
            {"id": "end", "time": 700, "value": 250 + source_millis, "easing": {"type": "linear"}}
        ], "before": "inactive", "after": "inactive"}),
        );
        wire["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .insert(0, sound);
        wire
    };
    // An authored TimeRemap is diagnosed and omitted, even at unit speed.
    let (project, omissions) = convert_with_omissions(with_playback(700, SOURCE_MILLIS)).unwrap();
    assert_eq!(project.single_sequence().unwrap().audio.len(), 0);
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert_eq!(
        omissions,
        [crate::Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record: "layer 3 (\"Audio 3\")".to_owned(),
            reason: "retimed audio layer was not exported".to_owned(),
        }]
    );
    // The same endpoints in a canonical unit Linear clock retain exact timing.
    let mut unit = with_playback(700, SOURCE_MILLIS);
    unit["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
        json!({"start":0,"duration":700}),
        json!({"start":250,"duration":700}),
    );
    let (project, omissions) = convert_with_omissions(unit).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let [sound] = project.single_sequence().unwrap().audio.as_slice() else {
        panic!("one unit sound");
    };
    assert_eq!(
        (
            sound.start_ticks,
            sound.end_ticks,
            sound.in_ticks,
            sound.out_ticks
        ),
        (
            0,
            700 * TICKS_PER_MILLISECOND,
            250 * TICKS_PER_MILLISECOND,
            950 * TICKS_PER_MILLISECOND
        )
    );
    // A retimed layer is omitted only after its packaged duration matches.
    let error = convert(with_playback(1400, 9000)).unwrap_err().to_string();
    assert!(
        error.contains("Audio 3") && error.contains("sourceIntrinsicDuration"),
        "{error}"
    );
}

/// A Studio-authored Inter Bold text layer over the fixture's video.
/// [`inter_bold`] packages its face.
fn with_text(edit: Value) -> Value {
    fn merge(target: &mut Value, edit: Value) {
        match (target, edit) {
            (Value::Object(target), Value::Object(edit)) => {
                for (key, value) in edit {
                    merge(target.entry(key).or_insert(Value::Null), value);
                }
            }
            (target, edit) => *target = edit,
        }
    }
    let mut text = json!({
        "type": "Text",
        "id": 9,
        "name": "Title",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"anchorPoint": [10, 20], "position": [960, 540], "scale": [50, 50], "rotation": 30, "opacity": 60},
        "sourceText": {
            "text": "Line one\r\nLine two",
            "fontFamily": "Inter",
            "fontStyle": "Bold",
            "fontSize": 80,
            "fillColor": [1, 0, 0, 1],
            "applyStroke": true,
            "strokeColor": [0, 0, 1, 1],
            "strokeWidth": 10,
            "justification": "center",
            "tracking": -20,
            "leading": 146,
            "boxText": true,
            "boxSize": [800, 400],
            "boxPosition": [-400, -200],
            "verticalAlign": "center",
            "allCaps": true
        }
    });
    merge(&mut text, edit);
    let mut wire = document();
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, text);
    wire
}

#[test]
fn a_rectangle_that_only_a_text_or_group_mask_references_is_no_content() {
    // FX never paints a mask guide, whichever layer's mask references it. This
    // static, plain, full-frame guide above the video would otherwise export
    // as a visible Color Matte.
    let identity = document()["composition"]["layers"][0]["transform"].clone();
    let guide = json!({
        "type": "Rect",
        "id": 11,
        "name": "Guide",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": identity,
        "rect": {"size": [1920.0, 1080.0], "fillColor": [1, 1, 1, 1]},
    });
    let mask = json!([{"id": 50, "mode": "add", "layer": 11}]);
    let text_mask = with_text(json!({"masks": mask}));
    let mut group_mask = document();
    let mut child = group_mask["composition"]["layers"][0].clone();
    (child["id"], child["parent"]) = (json!(31), json!(30));
    let group = json!({
        "type": "Group",
        "id": 30,
        "name": "Nest",
        "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
        "transform": identity,
        "masks": mask,
        "layers": [child],
    });
    group_mask["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, group);
    for (consumer, mut wire) in [("text", text_mask), ("group", group_mask)] {
        wire["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .insert(1, guide.clone());
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let media: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .map(|clip| clip.media.as_str())
            .collect();
        assert_eq!(media, ["premiere-video-1"], "{consumer}: the sibling alone");
        assert!(
            omissions.contains(&crate::Omission {
                scope: crate::OmissionScope::Occurrence,
                kind: crate::OmissionKind::Omitted,
                record: "layer 11 (\"Guide\")".into(),
                reason: "unsupported layer type was not exported".into(),
            }),
            "{consumer}: {omissions:?}"
        );
    }
}

#[test]
fn an_image_with_a_mask_is_omitted_whole_and_an_unmasked_image_exports() {
    // A still exports no Crop or Linear Wipe, so an image never exports
    // without its mask, even a mask that a video would export as a Crop. Its
    // guide goes with it; an unmasked image beside it exports.
    let identity = document()["composition"]["layers"][0]["transform"].clone();
    let image = |id: u64, name: &str, asset: &str| {
        json!({
            "type": "Image",
            "id": id,
            "name": name,
            "activeRange": {"start": 0, "duration": 1000},
            "transform": identity,
            "source": {
                "assetId": asset,
                "fit": "contain",
                "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080},
            },
        })
    };
    let guide = json!({
        "type": "Rect",
        "id": 31,
        "name": "Premiere Crop guide 1",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": identity,
        "rect": {"position": [0.0, 162.0], "size": [1920.0, 918.0], "fillColor": [0, 0, 0, 1]},
    });
    let still = crate::image_media::ValidatedImage {
        format: crate::image_media::ImageFormat::Png,
        width: 1920,
        height: 1080,
        alpha: false,
        icc_profile: false,
    };
    let mut media = source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND);
    for asset in ["premiere-image-2", "premiere-image-3"] {
        media.insert(asset.to_owned(), MediaFacts::Still(still));
    }
    for (case, mask) in [
        ("crop", json!({"id": 32, "mode": "add", "layer": 31})),
        (
            "inverted",
            json!({"id": 32, "mode": "add", "layer": 31, "inverted": true}),
        ),
    ] {
        let mut wire = document();
        let mut masked = image(30, "Masked", "premiere-image-2");
        masked["masks"] = json!([mask]);
        let layers = wire["composition"]["layers"].as_array_mut().unwrap();
        // Top first: the masked image, its guide, the unmasked image.
        for layer in [
            image(33, "Unmasked", "premiere-image-3"),
            guide.clone(),
            masked,
        ] {
            layers.insert(0, layer);
        }
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let mut omissions = Vec::new();
        let project = tesseract_to_premiere(
            &document,
            &media,
            &sound(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        let mut exported: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .map(|clip| clip.media.as_str().to_owned())
            .collect();
        exported.sort_unstable();
        assert_eq!(
            (exported, omissions),
            (
                vec!["premiere-image-3".to_owned(), "premiere-video-1".to_owned()],
                vec![crate::Omission {
                    scope: crate::OmissionScope::Occurrence,
                    kind: crate::OmissionKind::Omitted,
                    record: "layer 30 (\"Masked\")".into(),
                    reason: "masks cannot be exported: a still image exports no Crop, Linear Wipe or Track Matte Key; occurrence omitted".into(),
                }],
            ),
            "{case}"
        );
    }
}

/// A font registry in the `metadata.json` `fonts` wire shape, validated as
/// `TesseractFile::open` validates it.
fn packaged_fonts(fonts: Value) -> BTreeMap<String, FontAssetProperties> {
    let fonts: BTreeMap<String, FontAssetProperties> = serde_json::from_value(fonts).unwrap();
    fx_schema::validate_embedded_font_registry(
        fonts
            .iter()
            .map(|(key, properties)| (key.as_str(), properties)),
    )
    .unwrap();
    fonts
}

/// One static face; its family/style names form its only selection name.
fn static_face(family: &str, style: &str, postscript: &str) -> Value {
    json!({
        "postscriptName": postscript,
        "fullName": format!("{family} {style}"),
        "familyName": family,
        "styleName": style,
        "weight": 700,
        "width": 5,
        "selectionNames": [format!("{family}/{style}")]
    })
}

/// The packaged face of the Inter Bold that [`with_text`] names.
fn inter_bold() -> BTreeMap<String, FontAssetProperties> {
    packaged_fonts(json!({
        "inter-bold": {"faces": [static_face("Inter", "Bold", "Inter-Bold")]}
    }))
}

fn graphics(project: &PrProjectFile) -> Vec<&crate::schema::PrGraphic> {
    let sequence = project.single_sequence().unwrap();
    sequence
        .video_items()
        .filter_map(crate::schema::PrVideoItem::graphic)
        .collect()
}

#[test]
fn text_layers_export_their_current_document() {
    use crate::schema::text::{
        PrJustification, PrRgb, PrTextDocument, PrTextFrame, PrTextStroke, PrTextTransform,
        PrVerticalAlign,
    };
    let (project, omissions) = convert_with_fonts(with_text(json!({})), &inter_bold()).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let [graphic] = graphics(&project)[..] else {
        panic!("expected one graphic");
    };
    assert_eq!(graphic.timeline_ticks(), 0..TICKS);
    assert_eq!(graphic.text().name, "Title");
    assert_eq!(
        graphic.text().document,
        PrTextDocument {
            text: "Line one\nLine two".into(),
            // The packaged face names the PostScript name that Premiere loads.
            font: "Inter-Bold".into(),
            size: 80.0,
            fill: Some(PrRgb([255, 0, 0])),
            stroke: Some(PrTextStroke {
                color: PrRgb([0, 0, 255]),
                width: 5.0,
            }),
            shadow: None,
            all_caps: true,
            tracking: -20.0,
            leading: 50.0,
            justification: PrJustification::Center,
            frame: PrTextFrame::Box {
                width: 800.0,
                height: 400.0,
                vertical: PrVerticalAlign::Center,
            },
            background: None,
        }
    );
    // Premiere's box starts at the layer origin, so the box offset moves into the anchor.
    assert_eq!(
        graphic.text().transform,
        PrTextTransform {
            position: [960.0, 540.0],
            anchor: [410.0, 220.0],
            scale: 50.0,
            rotation: 30.0,
            opacity: 60.0,
        }
    );
    // The text graphic and the video overlap, so the text takes the upper track.
    assert_eq!(sequence.video_tracks().len(), 2);
}

#[test]
fn packaged_faces_name_text_by_family_typographic_or_selection_name() {
    let fonts = packaged_fonts(json!({
        "brand-bold": {"faces": [static_face("Brand", "Bold", "Brand-Bold")]},
        "brand-heavy": {"faces": [{
            "postscriptName": "Brand-Heavy",
            "fullName": "Brand Heavy",
            "familyName": "Brand Heavy",
            "styleName": "Regular",
            "typographicFamilyName": "Brand",
            "typographicStyleName": "Heavy",
            "weight": 900,
            "width": 5,
            "selectionNames": ["Brand Heavy/Regular"]
        }]},
        "inter-variable": {"faces": [{
            "postscriptName": "Inter-Regular",
            "fullName": "Inter Regular",
            "familyName": "Inter",
            "styleName": "Regular",
            "weight": 400,
            "width": 5,
            "variationAxes": [{"tag": "wght", "min": 100, "default": 400, "max": 900}],
            "variationInstances": [
                {"name": "Bold", "postscriptName": "Inter-Bold",
                 "coordinates": [{"tag": "wght", "value": 700}], "selectionName": "Inter/Bold"},
                {"name": "SemiBold",
                 "coordinates": [{"tag": "wght", "value": 600}], "selectionName": "Inter/SemiBold"}
            ],
            "selectionNames": ["Inter/Regular", "Inter/Bold", "Inter/SemiBold"]
        }]}
    }));
    for (family, style, postscript) in [
        // Family and style names.
        ("Brand", "Bold", "Brand-Bold"),
        // Typographic names of a per-weight legacy family.
        ("Brand", "Heavy", "Brand-Heavy"),
        // The selection name of a named instance with its own PostScript name.
        ("Inter", "Bold", "Inter-Bold"),
    ] {
        let wire = with_text(json!({"sourceText": {"fontFamily": family, "fontStyle": style}}));
        let (project, omissions) = convert_with_fonts(wire, &fonts).unwrap();
        assert!(omissions.is_empty(), "{family} {style}: {omissions:?}");
        let [graphic] = graphics(&project)[..] else {
            panic!("{family} {style}: expected one graphic");
        };
        assert_eq!(graphic.text().document.font, postscript, "{family} {style}");
    }
    // A named instance without its own PostScript name gives Premiere no name to load.
    let wire = with_text(json!({"sourceText": {"fontFamily": "Inter", "fontStyle": "SemiBold"}}));
    let (project, omissions) = convert_with_fonts(wire, &fonts).unwrap();
    assert!(graphics(&project).is_empty());
    assert_eq!(
        omissions,
        [crate::Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record: r#"layer 9 ("Title")"#.to_owned(),
            reason: "font \"Inter SemiBold\" selects the named instance \"SemiBold\" of the \
                     packaged face \"Inter-Regular\", which has no PostScript name of its own, so \
                     Premiere cannot select it."
                .to_owned(),
        }]
    );
}

#[test]
fn empty_style_text_writes_its_family_as_the_postscript_name() {
    // Converted Premiere text stores the PostScript name with an empty style.
    let wire = with_text(json!({"sourceText": {"fontFamily": "OpenSans-Bold", "fontStyle": ""}}));
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let [graphic] = graphics(&project)[..] else {
        panic!("expected one graphic");
    };
    assert_eq!(graphic.text().document.font.as_bytes(), b"OpenSans-Bold");
}

#[test]
fn unpackaged_family_and_style_omit_only_the_text_layer() {
    // Another packaged font does not serve Inter Bold, so tsrct would refuse to render it.
    let fonts = packaged_fonts(json!({
        "brand-bold": {"faces": [static_face("Brand", "Bold", "Brand-Bold")]}
    }));
    let (project, omissions) = convert_with_fonts(with_text(json!({})), &fonts).unwrap();
    assert!(graphics(&project).is_empty());
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert_eq!(
        omissions,
        [crate::Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record: r#"layer 9 ("Title")"#.to_owned(),
            reason: "font \"Inter Bold\" is not packaged in this document; import it with tsrct \
                     project import-font before export."
                .to_owned(),
        }]
    );
}

#[test]
fn unsupported_text_layer_values_are_omitted_with_layer_context() {
    for (edit, expected) in [
        (json!({"transform": {"scale": [50, 60]}}), "uniform"),
        (
            json!({"sourceText": {"strokeOverFill": true}}),
            "stroke over fill",
        ),
        (json!({"sourceText": {"underline": true}}), "underline"),
        (
            json!({"sourceText": {"boxFirstBaseline": 71.5, "verticalAlign": "top"}}),
            "authored first baseline",
        ),
        (json!({"sourceText": {"applyFill": false}}), "outline-only"),
        (json!({"sourceText": {"leading": 60}}), "below 0.8 em"),
        (
            json!({"sourceText": {"verticalAlign": null}}),
            "explicit vertical alignment",
        ),
        (
            json!({"sourceText": {"fillColor": [1, 0, 0, 0.5]}}),
            "opaque color",
        ),
        (
            json!({"pathOptions": {"id": 51, "pathLayer": 2}}),
            "path options",
        ),
        (
            json!({"sourceText": {"fontFamily": "OpenSans/Bold", "fontStyle": ""}}),
            "cannot form a family/style key",
        ),
    ] {
        let (project, omissions) = convert_with_fonts(with_text(edit), &inter_bold()).unwrap();
        // The video below the text still exports; only the text layer is omitted.
        assert_eq!(project.single_sequence().unwrap().video_tracks().len(), 1);
        assert!(
            omissions.iter().any(|item| {
                item.scope == OmissionScope::Occurrence
                    && item.record == "layer 9 (\"Title\")"
                    && item.reason.contains("text layer was not exported")
                    && item.reason.contains(expected)
            }),
            "{expected}: {omissions:?}"
        );
    }
    // Layer properties shared with video layers follow the video rules.
    let (project, omissions) = convert_with_fonts(
        with_text(json!({"description": "note", "transform": {"skew": 5}})),
        &inter_bold(),
    )
    .unwrap();
    assert_eq!(project.single_sequence().unwrap().video_tracks().len(), 2);
    let reasons: Vec<_> = omissions
        .iter()
        .filter(|item| item.record == "layer 9 (\"Title\")")
        .map(|item| (item.scope, item.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        [
            (OmissionScope::Feature, "description was not exported"),
            (OmissionScope::Feature, "skew was not exported"),
        ]
    );
}

#[test]
fn hidden_text_exports_as_a_disabled_graphic() {
    let (project, omissions) =
        convert_with_fonts(with_text(json!({"isHidden": true})), &inter_bold()).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let [graphic] = graphics(&project)[..] else {
        panic!("expected one graphic");
    };
    assert!(!graphic.enabled);
    assert_eq!(graphic.timeline_ticks(), 0..TICKS);
    assert_eq!(graphic.text().name, "Title");
}

#[test]
fn shapes_and_graphic_groups_that_another_layer_uses_do_not_export() {
    let with = |mut layer: Value, fields: Value| {
        for (key, value) in fields.as_object().unwrap() {
            layer[key] = value.clone();
        }
        layer
    };
    let transform = json!({"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100});
    let range = json!({"start": 0, "duration": 1000});
    let shape = json!({
        "type": "Shape",
        "id": 30,
        "name": "Guide",
        "activeRange": range,
        "transform": transform,
        "shape": {
            "path": {"commands": [
                {"type": "moveTo", "x": -100, "y": -90},
                {"type": "lineTo", "x": 100, "y": -90},
                {"type": "lineTo", "x": 100, "y": 90},
                {"type": "close"}
            ]},
            "fills": [{"paint": {"type": "solid", "color": [1, 1, 1, 1]}}]
        }
    });
    let text = json!({
        "type": "Text",
        "id": 41,
        "name": "Title",
        "activeRange": range,
        "transform": transform,
        "sourceText": {"text": "Title", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]}
    });
    // Group 40 holds text 41 and shape 30.
    let group = json!({
        "type": "Group",
        "id": 40,
        "name": "Graphic",
        "playback": crate::test_support::linear_playback(json!(range), json!({"start": 0, "duration": (range)["duration"]})),
        "transform": transform,
        "layers": [
            with(text.clone(), json!({"parent": 40})),
            with(shape.clone(), json!({"parent": 40}))
        ]
    });
    let mask = |layer| json!({"masks": [{"id": 50, "mode": "add", "layer": layer}]});
    let matte = |layer| json!({"trackMatte": {"mode": "alpha", "layer": layer}});
    let shape_omitted = (
        "layer 30 (\"Guide\")",
        "graphic was not exported: its shape layer is another layer's track matte, mask or text path",
    );
    // A track matte source exports only through the clips that it keys; a
    // graphic source keys none (`canonical_track_matte`).
    let shape_source_omitted = (
        "layer 30 (\"Guide\")",
        "track matte source of no exported clip was not exported; FX draws it only through the clips that it keys",
    );
    let group_omitted = (
        "layer 40 (\"Graphic\")",
        "graphic group was not exported: the graphic or one of its layers is another layer's track matte, mask or text path",
    );
    let group_source_omitted = ("layer 40 (\"Graphic\")", shape_source_omitted.1);
    // A shape that one mask alone references is that mask's guide: the masked
    // layer carries the reason (the guide's range is not the video's, and a
    // text exports no mask) and the shape is not reported. A shape with any
    // other use is still reported.
    let video_omitted = (
        "layer 1 (\"Source\")",
        "masks cannot be exported: the guide's range differs from the video's; occurrence omitted",
    );
    let text_omitted = (
        "layer 41 (\"Title\")",
        "graphic was not exported: its text layer must have no masks",
    );
    // `video` joins video layer 1's JSON; `layers` go above it.
    for (video, layers, (record, reason)) in [
        (mask(30), vec![shape.clone()], video_omitted),
        (
            json!({}),
            vec![with(text.clone(), mask(30)), shape.clone()],
            text_omitted,
        ),
        (
            mask(30),
            vec![with(text.clone(), mask(30)), shape.clone()],
            shape_omitted,
        ),
        (
            with(mask(30), matte(30)),
            vec![shape.clone()],
            shape_source_omitted,
        ),
        (matte(30), vec![shape.clone()], shape_source_omitted),
        (matte(30), vec![group.clone()], group_omitted),
        (matte(40), vec![group.clone()], group_source_omitted),
    ] {
        // The second clip exports whether or not the first one's mask does.
        let mut wire = two_clips();
        let composition = wire["composition"]["layers"].as_array_mut().unwrap();
        composition[0] = with(composition[0].clone(), video);
        composition.splice(0..0, layers);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        let graphics = project
            .single_sequence()
            .unwrap()
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .count();
        assert_eq!(graphics, 0, "{reason}: {omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.record == record
                    && omission.reason == reason),
            "{reason}: {omissions:?}"
        );
        assert_eq!(
            omissions
                .iter()
                .any(|omission| omission.record == shape_omitted.0),
            record == shape_omitted.0,
            "{reason}: {omissions:?}"
        );
    }
}

#[test]
fn video_input_transform_and_3d_position_are_reported() {
    let input_transform = json!({"semanticVersion": 1, "transform": {
        "type": "lut3d",
        "assetId": "input-lut",
        "inputEncoding": "camera:apple-log:v1",
        "outputEncoding": "jerboa:sdr-rec709-display:v1",
        "interpolation": "trilinear"
    }});
    // Each loss has its own report; neither stands in for the other.
    for (object, field, value, reason) in [
        (
            "source",
            "inputTransform",
            input_transform,
            "input transform was not exported",
        ),
        (
            "transform",
            "position",
            json!([960, 540, 25]),
            "3D position was not exported",
        ),
    ] {
        let mut wire = document();
        wire["composition"]["layers"][0][object][field] = value;
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .count(),
            1
        );
        let reported: Vec<_> = omissions
            .iter()
            .map(|item| (item.scope, item.record.as_str(), item.reason.as_str()))
            .collect();
        assert_eq!(
            reported,
            [(
                crate::OmissionScope::Feature,
                "layer 1 (\"Source\")",
                reason
            )],
            "{field}"
        );
    }
}

/// A Linear Wipe keyed from 100 to 0 over its first source second.
fn keyed_wipe() -> crate::schema::PrLinearWipe {
    use crate::schema::{PrKeyframeEasing, PrLinearWipe, PrScalarKeyframe};
    let key = |source_ticks, value| PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    PrLinearWipe {
        initial_completion: 100.0,
        completion: vec![key(0, 100.0), key(TICKS, 0.0)],
        angle_degrees: 270,
        feather: 0.0,
    }
}

/// Keys of `property` on `layer` at layer times 0 and 1 s; the second key has `easing`.
fn two_layer_keys(layer: u64, property: &str, values: [f64; 2], easing: Value) -> Value {
    json!({
        "target": {"kind": "layer", "layerId": layer, "propertyType": property},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": format!("{property}-{layer}-0"), "layerTime": 0, "value": {"type": "float", "value": values[0]}, "easing": {"type": "linear"}},
            {"id": format!("{property}-{layer}-1"), "layerTime": 1000, "value": {"type": "float", "value": values[1]}, "easing": easing}
        ]}
    })
}

#[test]
fn retimed_clip_keeps_its_static_crop_when_its_position_keys_are_omitted() {
    use crate::tests::support::{project_document, video_sequence};
    // 4 s of source over 2 s in reverse writes a -2x clip, which omits the
    // Position keys. The written Motion is static, and the static Crop follows
    // it exactly.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.end_ticks = 2 * TICKS;
    clip.in_ticks = 2 * TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.playback_rate = -2.0;
    clip.crop = left_crop();
    sequence.timeline_end_ticks = 2 * TICKS;
    let mut wire = project_document(&sequence);
    let keys = |layer| {
        [
            two_layer_keys(
                layer,
                "positionX",
                [960.0, 480.0],
                json!({"type": "linear"}),
            ),
            two_layer_keys(
                layer,
                "positionY",
                [540.0, 540.0],
                json!({"type": "linear"}),
            ),
        ]
    };
    // A guide that does not repeat the video's keys is not its Crop (JRB-2077).
    wire["composition"]["dynamics"] = json!({ "entries": keys(1) });
    let error = convert(wire.clone()).unwrap_err().to_string();
    assert!(
        error.contains("masks cannot be exported: the Crop guide's transform or Motion keys differ from its video's; occurrence omitted"),
        "{error}"
    );
    let guide = wire["composition"]["layers"][1]["id"].as_u64().unwrap();
    let entries: Vec<_> = keys(1).into_iter().chain(keys(guide)).collect();
    wire["composition"]["dynamics"] = json!({ "entries": entries });
    let (project, omissions) = convert_with_omissions(wire).unwrap();
    let clip = first_clip(&project);
    assert_eq!(clip.playback_rate, -2.0);
    assert!(clip.animations.is_empty());
    assert!((clip.crop.left - 7.0).abs() < 1e-9, "{:?}", clip.crop);
    assert!(clip.crop.right.abs() < 1e-9, "{:?}", clip.crop);
    for axis in ["PositionX", "PositionY"] {
        let reason = format!("{axis} animation was not exported: keys on a retimed");
        assert!(
            omissions.iter().any(
                |item| item.scope == OmissionScope::Feature && item.reason.starts_with(&reason)
            ),
            "{axis}: {omissions:?}"
        );
    }
    assert!(
        !omissions
            .iter()
            .any(|item| item.reason.contains("Crop cannot follow")
                || item.reason.contains("unsupported layer type")),
        "{omissions:?}"
    );
}

#[test]
fn unwritten_motion_keys_leave_a_static_crop_and_omit_a_flat_wiped_clip() {
    use crate::tests::support::{project_document, video_sequence};
    let linear = json!({"type": "linear"});
    let cubic = json!({"type": "cubicBezier", "x1": 0.2, "y1": 0.1, "x2": 0.8, "y2": 0.9});
    // Unit-speed Motion keys that export does not write leave static Motion,
    // which a canonical Crop, whose guide repeats the keys, follows exactly. A
    // flat Linear Wipe guide is the clip frame only without Motion keys.
    let unwritten = [
        (
            vec![two_layer_keys(
                1,
                "positionX",
                [960.0, 480.0],
                linear.clone(),
            )],
            "unpaired Position keyframes were not exported",
        ),
        (
            vec![two_layer_keys(1, "scaleY", [100.0, 150.0], linear.clone())],
            "nonuniform or unpaired Scale keyframes were not exported",
        ),
        (
            vec![two_layer_keys(1, "scaleX", [100.0, 150.0], cubic.clone())],
            "Scale Width animation was not exported: unsupported conversion: only Linear Scale Width keys convert",
        ),
        (
            vec![
                two_layer_keys(1, "positionX", [960.0, 960.0], cubic.clone()),
                two_layer_keys(1, "positionY", [540.0, 540.0], cubic.clone()),
            ],
            "Position cubic easing on a stationary spatial segment",
        ),
        (
            vec![two_layer_keys(1, "rotation", [0.0, 0.0], cubic)],
            "Rotation cubic easing between equal values",
        ),
    ];
    let mut cropped = video_sequence();
    cropped.video_tracks[0].clip_mut(0).crop = left_crop();
    let cropped = project_document(&cropped);
    let mut wiped = video_sequence();
    wiped.video_tracks[0].clip_mut(0).linear_wipe = Some(keyed_wipe());
    let wiped = project_document(&wiped);
    let export = |wire: Value| {
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        (first_clip(&project).clone(), omissions)
    };
    // Only one clip: omitting it leaves nothing to publish.
    let omitted = |wire: Value, reason: &str| {
        let error = convert(wire).unwrap_err().to_string();
        assert!(
            error.contains(&format!(
                "occurrence layer 1 (\"Premiere video 1\"): masks cannot be exported: {reason}; occurrence omitted"
            )),
            "{error}"
        );
    };
    let guide = cropped["composition"]["layers"][1]["id"].as_u64().unwrap();
    for (entries, reason) in unwritten {
        let mut wire = cropped.clone();
        let guide_entries = entries.iter().map(|entry| {
            let mut entry = entry.clone();
            entry["target"]["layerId"] = json!(guide);
            for key in entry["animator"]["keyframes"].as_array_mut().unwrap() {
                key["id"] = json!(format!("guide-{}", key["id"].as_str().unwrap()));
            }
            entry
        });
        let all: Vec<_> = entries.iter().cloned().chain(guide_entries).collect();
        wire["composition"]["dynamics"] = json!({ "entries": all });
        let (clip, omissions) = export(wire);
        assert!(clip.animations.is_empty(), "{reason}");
        assert!(
            (clip.crop.left - 7.0).abs() < 1e-9,
            "{reason}: {omissions:?}"
        );
        assert!(
            omissions.iter().any(|item| item.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
        assert!(
            omissions
                .iter()
                .all(|item| item.scope != OmissionScope::Occurrence),
            "{reason}: {omissions:?}"
        );

        let mut wire = wiped.clone();
        wire["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .extend(entries);
        omitted(
            wire,
            "the video does not map its frame onto the canvas unchanged, so its Linear Wipe guide is not its frame",
        );
    }

    // Written Position keys that the guide does not repeat would move the clip
    // against its Crop: the clip is omitted whole (JRB-2077).
    let mut wire = cropped;
    wire["composition"]["dynamics"] = json!({"entries": [
        two_layer_keys(1, "positionX", [960.0, 480.0], linear.clone()),
        two_layer_keys(1, "positionY", [540.0, 540.0], linear)
    ]});
    omitted(
        wire,
        "the Crop guide's transform or Motion keys differ from its video's",
    );
}

#[test]
fn a_linear_wipe_that_cannot_follow_its_clip_frame_omits_the_clip_or_stops_the_export() {
    use crate::tests::support::{project_document, video_sequence};
    let mut wiped = video_sequence();
    wiped.video_tracks[0].clip_mut(0).linear_wipe = Some(keyed_wipe());
    let wiped = project_document(&wiped);
    let mut scaled = wiped.clone();
    scaled["composition"]["layers"][0]["transform"]["scale"] = json!([50.0, 50.0]);
    let mut keyed = wiped.clone();
    keyed["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(two_layer_keys(
            1,
            "rotation",
            [0.0, 90.0],
            json!({"type": "linear"}),
        ));
    // 10 s of source over 5 s writes a 2x clip, whose key clock is unproven.
    let mut retimed = wiped;
    let video = &mut retimed["composition"]["layers"][0];
    video["sourceRange"] = json!({"start": 0, "duration": 10_000});
    video["playback"] = crate::test_support::remapped_playback(
        crate::test_support::layer_range(video).clone(),
        json!({"keyframes": [
        {"id": "start", "time": 0, "value": 0, "easing": {"type": "linear"}},
        {"id": "end", "time": 5000, "value": 10_000, "easing": {"type": "linear"}}
    ], "before": "inactive", "after": "inactive"}),
    );
    // Moved by static or keyed Motion, the guide is not the clip's frame: the
    // clip is omitted whole, which leaves nothing to publish (JRB-2077).
    for wire in [scaled, keyed] {
        let error = convert(wire).unwrap_err().to_string();
        assert!(
            error.contains("occurrence layer 1 (\"Premiere video 1\"): masks cannot be exported: the video does not map its frame onto the canvas unchanged, so its Linear Wipe guide is not its frame; occurrence omitted"),
            "{error}"
        );
    }
    let error = convert(retimed).unwrap_err().to_string();
    assert!(
        error.ends_with(
            "unsupported conversion: Linear Wipe cannot be exported: keys on a retimed or reversed clip are not converted"
        ),
        "{error}"
    );
}

/// The imported document of a 30 fps sequence: the 0-5 s source on track 0
/// keyed by `channel` from the same source on track 1, a plain 5-10 s clip
/// after it on track 0, and `edit` on the keyed clip.
fn keyed_document(
    channel: crate::schema::PrMatteChannel,
    edit: impl FnOnce(&mut PrVideoOccurrence),
) -> Value {
    use crate::{
        schema::{PrTrackMatte, PrVideoTrack},
        tests::support::{clip_of, sequence_of},
    };
    let mut fill = clip_of("source", 0..5 * TICKS, 0);
    fill.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel,
    });
    edit(&mut fill);
    let mut matte = clip_of("source", 0..5 * TICKS, 0);
    matte.transform.rotation = 15.0;
    crate::tests::support::project_document(&sequence_of(
        "Main",
        vec![
            PrVideoTrack::media([fill, clip_of("source", 5 * TICKS..10 * TICKS, 5 * TICKS)]),
            PrVideoTrack::media([matte]),
        ],
    ))
}

/// The keyed clips of `project` as (track, start seconds, Track Matte Key)
/// and the rotation of the clip on each track, bottom first.
fn keyed_clips(project: &PrProjectFile) -> Vec<(usize, i64, Option<crate::schema::PrTrackMatte>)> {
    project
        .single_sequence()
        .unwrap()
        .video_tracks
        .iter()
        .enumerate()
        .flat_map(|(track, items)| {
            items
                .items
                .iter()
                .filter_map(PrVideoItem::media)
                .map(move |clip| (track, clip.start_ticks / TICKS, clip.track_matte))
        })
        .collect()
}

#[test]
fn track_mattes_export_as_the_key_with_the_source_above_every_keyed_clip() {
    use crate::schema::{PrBlendMode, PrMatteChannel, PrTrackMatte};
    let beside = |channel| {
        Some(PrTrackMatte {
            track_index: 1,
            channel,
        })
    };
    // Flat: the keyed video and its matte beside it, whichever is listed
    // first; `alphaInverted` writes Reverse true (fixture G3a).
    let flat = keyed_document(PrMatteChannel::Alpha, |_| {});
    let mut source_last = flat.clone();
    let layers = source_last["composition"]["layers"].as_array_mut().unwrap();
    let matte = layers.remove(0);
    layers.insert(2, matte);
    let inverted = keyed_document(PrMatteChannel::AlphaInverted, |_| {});
    assert_eq!(
        inverted["composition"]["layers"][1]["trackMatte"]["mode"],
        "alphaInverted"
    );
    for (wire, channel) in [
        (flat, PrMatteChannel::Alpha),
        (source_last, PrMatteChannel::Alpha),
        (inverted, PrMatteChannel::AlphaInverted),
    ] {
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(
            keyed_clips(&project),
            [(0, 0, beside(channel)), (0, 5, None), (1, 0, None)]
        );
        let reread = write_and_load_with_crate_reader(project);
        assert_eq!(
            keyed_clips(&reread),
            [(0, 0, beside(channel)), (0, 5, None), (1, 0, None)]
        );
        assert_eq!(
            reread.single_sequence().unwrap().video_tracks[1]
                .clip(0)
                .transform
                .rotation,
            15.0
        );
    }
    // Two clips keyed by one source, one above the other: the source lands
    // above both, and both name its track.
    let mut pair = keyed_document(PrMatteChannel::Alpha, |_| {});
    let layers = pair["composition"]["layers"].as_array_mut().unwrap();
    let mut luma = layers[1].clone();
    luma["id"] = json!(40);
    luma["trackMatte"]["mode"] = json!("luma");
    layers.insert(1, luma);
    let (project, omissions) = convert_with_omissions(pair).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let above_both = |channel| {
        Some(PrTrackMatte {
            track_index: 2,
            channel,
        })
    };
    assert_eq!(
        keyed_clips(&project),
        [
            (0, 0, above_both(PrMatteChannel::Alpha)),
            (0, 5, None),
            (1, 0, above_both(PrMatteChannel::Luma)),
            (2, 0, None),
        ]
    );
    // Staged: the group's clip takes its Motion and key; the matte child
    // exports beside it over the group's range with its own Motion.
    let staged = keyed_document(PrMatteChannel::Luma, |clip| {
        clip.transform.scale = [50.0; 2]
    });
    let group = &staged["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    let document = EditableFxCompositionDocument::from_json_value(staged.clone()).unwrap();
    let inspected: Vec<_> = crate::convert::exported_video_layers(
        document.composition().layers(),
        document.composition().dynamics(),
        [1920, 1080],
    )
    .into_iter()
    .map(|layer| layer.id().value())
    .collect();
    assert_eq!(
        inspected,
        [
            group["layers"][0]["id"].as_u64().unwrap(),
            group["layers"][1]["id"].as_u64().unwrap(),
            staged["composition"]["layers"][1]["id"].as_u64().unwrap(),
        ]
    );
    // A script on the matte child bakes into keys that the child's clip
    // writes, so export reports the baked track as written.
    let mut scripted = staged.clone();
    scripted["composition"]["dynamics"]["entries"] = json!([{
        "target": {"kind": "layer", "layerId": group_matte_id(&document), "propertyType": "rotation"},
        "animator": {"type": "jsScript", "layerTimeJsCode": "return 15 + input.time.seconds;"}
    }]);
    let scripted = EditableFxCompositionDocument::from_json_value(scripted).unwrap();
    let baked = crate::convert::bake_scripts(&scripted, &mut Vec::new()).unwrap();
    let mut omissions = Vec::new();
    let exported = export_document(
        baked.document(),
        &source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND),
        &sound(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    baked.report_discarded(&exported.written, &mut omissions);
    let reasons: Vec<_> = omissions
        .iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        ["1 of 1 baked JS animation tracks were written as native keys"]
    );
    let (project, omissions) = convert_with_omissions(staged).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let luma = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Luma,
    });
    assert_eq!(
        keyed_clips(&project),
        [(0, 0, luma), (0, 5, None), (1, 0, None)]
    );
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_tracks[0].clip(0).transform.scale, [50.0; 2]);
    let matte = sequence.video_tracks[1].clip(0);
    assert_eq!(
        (matte.timeline_ticks(), matte.transform.rotation),
        (0..5 * TICKS, 15.0)
    );
    let reread = write_and_load_with_crate_reader(project);
    assert_eq!(
        keyed_clips(&reread),
        [(0, 0, luma), (0, 5, None), (1, 0, None)]
    );
    // A blended source, flat or staged, writes Normal with one loss and no
    // report of its mode, while the clip that it keys keeps its own blend.
    let multiply = |clip: &mut PrVideoOccurrence| clip.blend_mode = PrBlendMode::Multiply;
    let staged_multiply = |clip: &mut PrVideoOccurrence| {
        clip.transform.scale = [50.0; 2];
        clip.blend_mode = PrBlendMode::Multiply;
    };
    for (mut wire, source, channel) in [
        (
            keyed_document(PrMatteChannel::Alpha, multiply),
            "/composition/layers/0",
            PrMatteChannel::Alpha,
        ),
        (
            keyed_document(PrMatteChannel::Luma, staged_multiply),
            "/composition/layers/0/layers/1",
            PrMatteChannel::Luma,
        ),
    ] {
        let source = wire.pointer_mut(source).unwrap();
        source["blendMode"] = json!("darkerColor");
        let record = format!("layer {} ({})", source["id"], source["name"]);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        assert_eq!(
            omissions,
            [crate::Omission {
                scope: crate::OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record,
                reason: "unsupported blend mode was not exported (using normal)".into(),
            }]
        );
        let key = Some(PrTrackMatte {
            track_index: 1,
            channel,
        });
        let check = |project: &PrProjectFile| {
            assert_eq!(
                keyed_clips(project),
                [(0, 0, key), (0, 5, None), (1, 0, None)]
            );
            let tracks = &project.single_sequence().unwrap().video_tracks;
            assert_eq!(
                [tracks[0].clip(0), tracks[0].clip(1), tracks[1].clip(0)]
                    .map(|clip| clip.blend_mode),
                [
                    PrBlendMode::Multiply,
                    PrBlendMode::Normal,
                    PrBlendMode::Normal
                ]
            );
        };
        check(&project);
        check(&write_and_load_with_crate_reader(project));
    }
    // A still under the stage group is inspected and exported like the video.
    let mut still = keyed_document(PrMatteChannel::Alpha, |clip| {
        clip.transform.scale = [50.0; 2]
    });
    let group = &mut still["composition"]["layers"][0];
    let matte = &mut group["layers"][1];
    assert_eq!(matte["type"], "Video");
    matte["activeRange"] = matte["playback"]["inputRange"].clone();
    matte.as_object_mut().unwrap().remove("playback");
    matte["type"] = json!("Image");
    matte["transform"]["rotation"] = json!(0);
    matte["source"] = json!({"assetId": "premiere-image-2", "fit": "contain", "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}});
    for field in ["sourceRange", "sourceIntrinsicDuration", "volume"] {
        matte.as_object_mut().unwrap().remove(field);
    }
    let document = EditableFxCompositionDocument::from_json_value(still).unwrap();
    let composition = document.composition();
    let images: Vec<_> = crate::convert::exported_image_layers(
        composition.layers(),
        composition.dynamics(),
        [1920, 1080],
    )
    .map(|layer| layer.id().value())
    .collect();
    assert_eq!(images, [group_matte_id(&document)]);
    let mut media = source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND);
    media.insert(
        "premiere-image-2".to_owned(),
        MediaFacts::Still(crate::image_media::ValidatedImage {
            format: crate::image_media::ImageFormat::Png,
            width: 1920,
            height: 1080,
            alpha: true,
            icc_profile: false,
        }),
    );
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &media,
        &sound(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        keyed_clips(&project),
        [
            (0, 0, beside(PrMatteChannel::Alpha)),
            (0, 5, None),
            (1, 0, None)
        ]
    );
    assert!(project.single_sequence().unwrap().video_tracks[1]
        .clip(0)
        .media
        .as_str()
        .starts_with("premiere-image"));
}

/// The id of the track matte source of the first layer, a stage group.
fn group_matte_id(document: &EditableFxCompositionDocument) -> u64 {
    match document.composition().layers()[0].data() {
        LayerData::Group(group) => group.track_matte.as_ref().unwrap().layer.value(),
        layer => panic!("{layer:?}"),
    }
}

#[test]
fn a_delayed_stage_exports_its_child_matte_at_unit_speed_only() {
    use crate::{
        schema::{PrMatteChannel, PrTrackMatte, PrVideoTrack},
        tests::support::{clip_of, project_document, sequence_of},
    };
    // The Scale 50 clip over 5-10 s stages; its matte plays source 2-7 s.
    let mut fill = clip_of("source", 5 * TICKS..10 * TICKS, 0);
    fill.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    fill.transform.scale = [50.0; 2];
    let mut matte = clip_of("source", 5 * TICKS..10 * TICKS, 2 * TICKS);
    matte.transform.rotation = 15.0;
    let delayed = project_document(&sequence_of(
        "Main",
        vec![PrVideoTrack::media([fill]), PrVideoTrack::media([matte])],
    ));
    let group = &delayed["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    assert_eq!(
        group["layers"][1]["sourceRange"],
        json!({"start": 2000, "duration": 5000})
    );
    // The child clone plays the same source times over the group's range.
    let (project, omissions) = convert_with_omissions(delayed.clone()).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let alpha = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    assert_eq!(keyed_clips(&project), [(0, 5, alpha), (1, 5, None)]);
    let matte = project.single_sequence().unwrap().video_tracks[1].clip(0);
    assert_eq!(
        (
            matte.timeline_ticks(),
            matte.in_ticks,
            matte.out_ticks,
            matte.playback_rate,
            matte.transform.rotation
        ),
        (5 * TICKS..10 * TICKS, 2 * TICKS, 7 * TICKS, 1.0, 15.0)
    );
    // Playback keys on the child read the group clock; the clone beside the
    // group's clip would read the sequence clock, so the group is omitted,
    // which leaves nothing to publish (JRB-2077).
    let mut retimed = delayed;
    let child = &mut retimed["composition"]["layers"][0]["layers"][1];
    child["sourceRange"] = json!({"start": 0, "duration": 10_000});
    child["playback"] = crate::test_support::remapped_playback(
        crate::test_support::layer_range(child).clone(),
        json!({"keyframes": [
        {"id": "start", "time": 0, "value": 0, "easing": {"type": "linear"}},
        {"id": "end", "time": 5000, "value": 10_000, "easing": {"type": "linear"}}
    ], "before": "inactive", "after": "inactive"}),
    );
    let error = convert(retimed).unwrap_err().to_string();
    assert!(
        error.contains("occurrence layer 3 (\"Premiere stage 1\"): stage group was not exported as one clip: the stage group's track matte source plays at another speed or is time-remapped; its playback keys are on the group clock, and the source clip beside the group's clip would read the sequence clock"),
        "{error}"
    );
}

#[test]
fn still_track_matte_sources_that_export_at_defaults_omit_their_clip() {
    use crate::schema::PrMatteChannel;
    // The matte source as a canvas-sized opaque still: flat beside the keyed
    // video, or the Scale 50 stage group's child.
    let still_source = |matte: &mut Value| {
        assert_eq!(matte["type"], "Video");
        matte["activeRange"] = matte["playback"]["inputRange"].clone();
        matte.as_object_mut().unwrap().remove("playback");
        matte["type"] = json!("Image");
        matte["transform"]["rotation"] = json!(0);
        matte["source"] = json!({"assetId": "premiere-image-2", "fit": "contain", "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}});
        for field in ["sourceRange", "sourceIntrinsicDuration", "volume"] {
            matte.as_object_mut().unwrap().remove(field);
        }
    };
    let mut flat = keyed_document(PrMatteChannel::Alpha, |_| {});
    still_source(&mut flat["composition"]["layers"][0]);
    let mut staged = keyed_document(PrMatteChannel::Alpha, |clip| {
        clip.transform.scale = [50.0; 2]
    });
    still_source(&mut staged["composition"]["layers"][0]["layers"][1]);
    let mut media = source(FrameRate::Fps30, SOURCE_MILLIS * TICKS_PER_MILLISECOND);
    media.insert(
        "premiere-image-2".to_owned(),
        MediaFacts::Still(crate::image_media::ValidatedImage {
            format: crate::image_media::ImageFormat::Png,
            width: 1920,
            height: 1080,
            alpha: false,
            icc_profile: false,
        }),
    );
    let convert = |wire: Value| {
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let mut omissions = Vec::new();
        let project = tesseract_to_premiere(
            &document,
            &media,
            &sound(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        (project, omissions)
    };
    let alpha = Some(crate::schema::PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    // The still at its defaults is the matte in both forms.
    for (case, wire) in [("flat", flat.clone()), ("staged", staged.clone())] {
        let (project, omissions) = convert(wire);
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        assert_eq!(
            keyed_clips(&project),
            [(0, 0, alpha), (0, 5, None), (1, 0, None)],
            "{case}"
        );
    }
    // A matte still stays at its defaults: a written still carries its
    // Opacity, Motion and keys, but the key over them is unmeasured.
    type Still = fn(&mut Value) -> &mut Value;
    type Edit = fn(&mut Value);
    fn flat_still(wire: &mut Value) -> &mut Value {
        &mut wire["composition"]["layers"][0]
    }
    fn staged_still(wire: &mut Value) -> &mut Value {
        &mut wire["composition"]["layers"][0]["layers"][1]
    }
    let staged_record = format!(
        "layer {} ({:?})",
        staged["composition"]["layers"][0]["id"],
        staged["composition"]["layers"][0]["name"].as_str().unwrap()
    );
    let forms: [(&str, &Value, Still, &str); 2] = [
        ("flat", &flat, flat_still, "layer 1 (\"Premiere video 1\")"),
        ("staged", &staged, staged_still, &staged_record),
    ];
    let edits: [(&str, Edit, &str); 4] = [
        (
            "multiply",
            |still| still["blendMode"] = json!("multiply"),
            "blend mode (using normal)",
        ),
        (
            "Opacity 0",
            |still| still["transform"]["opacity"] = json!(0),
            "opacity (using 100%)",
        ),
        (
            "moved",
            |still| still["transform"]["position"] = json!([760.0, 540.0]),
            "position/anchor (using center)",
        ),
        (
            "Opacity keys",
            |still| still["__keys"] = still["id"].clone(),
            "keys",
        ),
    ];
    for ((form, base, still, record), (edit_name, edit, dropped)) in forms
        .iter()
        .flat_map(|form| edits.iter().map(move |edit| (form, edit)))
    {
        let case = format!("{form}, {edit_name}");
        let mut wire = (*base).clone();
        let source = still(&mut wire);
        edit(source);
        let source_record = format!(
            "layer {} ({:?})",
            source["id"],
            source["name"].as_str().unwrap()
        );
        if let Some(id) = source.as_object_mut().unwrap().remove("__keys") {
            wire["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(two_layer_keys(
                    id.as_u64().unwrap(),
                    "opacity",
                    [0.0, 100.0],
                    json!({"type": "linear"}),
                ));
        }
        let (project, omissions) = convert(wire);
        // The plain clip stays; neither the keyed clip nor the still exports,
        // so the fill that the still hid or shaped is not shown whole.
        assert_eq!(keyed_clips(&project), [(0, 5, None)], "{case}");
        let reason = format!(
            "the track matte source is a still whose {dropped} a matte still does not carry; the exported matte would gate the clip by a different picture"
        );
        // A stage group is omitted whole with its child; a flat source that
        // stays in the document is reported as drawn by no clip.
        let staged = *form == "staged";
        let reason = if staged {
            format!("stage group was not exported as one clip: {reason}")
        } else {
            format!("masks cannot be exported: {reason}; occurrence omitted")
        };
        assert!(
            omissions.iter().any(
                |omission| omission.scope == crate::OmissionScope::Occurrence
                    && omission.record == *record
                    && omission.reason == reason
            ),
            "{case}: {omissions:?}"
        );
        assert_eq!(
            omissions
                .iter()
                .find(|omission| omission.record == source_record
                    && omission.scope == crate::OmissionScope::Occurrence)
                .map(|omission| omission.reason.as_str()),
            (!staged).then_some("track matte source of no exported clip was not exported; FX draws it only through the clips that it keys"),
            "{case}: {omissions:?}"
        );
    }
}

#[test]
fn a_keyed_nest_exports_its_key_on_the_placement() {
    use crate::{
        schema::{PrMatteChannel, PrTrackMatte, PrVideoTrack},
        tests::support::{clip_of, nest_of, nested_sequence, sequence_of},
    };
    let (outer, media) = nested_sequence();
    // FX isolates a matted group's layers as Premiere isolates a nest's, so
    // its Screen clip exports without the pass-through report.
    let mut inner = outer.nest_occurrences().next().unwrap().sequence.clone();
    inner.video_tracks[0].clip_mut(0).blend_mode = crate::schema::PrBlendMode::Screen;
    let mut nest = nest_of(inner, 0..3 * TICKS, 0);
    nest.track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    let sequence = sequence_of(
        "Outer",
        vec![
            PrVideoTrack {
                items: Vec::new(),
                nests: vec![nest],
                transitions: Vec::new(),
            },
            PrVideoTrack::media([clip_of("red", 0..3 * TICKS, 0)]),
        ],
    );
    let wire = crate::tests::support::project_document_with_media(&sequence, &media);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let facts = |_: &str| {
        MediaFacts::Video(crate::media::VideoMedia {
            orientation: crate::schema::VideoOrientation::Identity,
            codec: VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: 1920,
            height: 1080,
            timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
        })
    };
    let media: BTreeMap<_, _> = ["premiere-video-1", "premiere-video-2"]
        .into_iter()
        .map(|asset| (asset.to_owned(), facts(asset)))
        .collect();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &media,
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let nest = sequence.nest_occurrences().next().unwrap();
    assert_eq!(
        nest.sequence
            .video_occurrences()
            .map(|clip| clip.blend_mode)
            .collect::<Vec<_>>(),
        [crate::schema::PrBlendMode::Screen]
    );
    assert_eq!(
        (nest.timeline_ticks(), nest.track_matte),
        (
            0..3 * TICKS,
            Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Alpha,
            })
        )
    );
    assert_eq!(
        sequence.video_tracks[1].clip(0).timeline_ticks(),
        0..3 * TICKS
    );
}

#[test]
fn track_mattes_that_one_key_record_cannot_carry_omit_their_clip_and_hide_the_source() {
    use crate::schema::PrMatteChannel;
    let base = keyed_document(PrMatteChannel::Alpha, |_| {});
    // Layers: 0 the matte source ("Premiere video 3"), 1 the keyed video
    // ("Premiere video 1"), 2 the plain clip, 3 the canvas.
    let source_id = base["composition"]["layers"][0]["id"].clone();
    let rect = json!({
        "type": "Rect",
        "id": 60,
        "name": "Solid",
        "activeRange": {"start": 0, "duration": 5000},
        "transform": base["composition"]["layers"][3]["transform"],
        "rect": {"size": [1920.0, 1080.0], "fillColor": [1, 0, 0, 1]},
    });
    let nest = json!({
        "type": "Group",
        "id": 61,
        "name": "Nest",
        "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 5000}), json!({"start": 0, "duration": 5000})),
        "transform": base["composition"]["layers"][3]["transform"],
        "layers": [],
    });
    let shape = json!({
        "type": "Shape",
        "id": 62,
        "name": "Shape",
        "activeRange": {"start": 0, "duration": 5000},
        "transform": base["composition"]["layers"][3]["transform"],
        "shape": {
            "path": {"commands": [
                {"type": "moveTo", "x": 480, "y": 270},
                {"type": "lineTo", "x": 1440, "y": 270},
                {"type": "lineTo", "x": 1440, "y": 810},
                {"type": "close"}
            ]},
            "fills": [{"paint": {"type": "solid", "color": [1, 1, 1, 1]}}]
        }
    });
    let fill_id = base["composition"]["layers"][1]["id"].as_u64().unwrap();
    // A flat clip whose Motion moves or scales its frame, or whose source is
    // not sequence-sized, keeps its sibling source fixed in FX while Premiere
    // would key the moved picture; the staged Scale 50 clip of
    // `track_mattes_export_as_the_key_with_the_source_above_every_keyed_clip`
    // is the supported form.
    let not_the_frame = "the clip does not map its sequence-sized frame onto the canvas unchanged, so its sibling track matte source is not the frame that Premiere keys; a moved or keyed clip needs a stage group with the source as its child";
    type Edit = Box<dyn Fn(&mut Value)>;
    let cases: [(&str, Edit, &str); 12] = [
        (
            "flat clip at Scale 50",
            Box::new(|wire| {
                wire["composition"]["layers"][1]["transform"]["scale"] = json!([50.0, 50.0])
            }),
            not_the_frame,
        ),
        (
            "flat clip with Rotation keys",
            Box::new(move |wire| {
                wire["composition"]["dynamics"]["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(two_layer_keys(
                        fill_id,
                        "rotation",
                        [0.0, 90.0],
                        json!({"type": "linear"}),
                    ))
            }),
            not_the_frame,
        ),
        (
            "flat clip whose source is not sequence-sized",
            Box::new(|wire| {
                wire["composition"]["layers"][1]["source"]["sourceRect"] =
                    json!({"x": 0, "y": 0, "width": 1280, "height": 720})
            }),
            not_the_frame,
        ),
        (
            "lumaInverted mode",
            Box::new(|wire| wire["composition"]["layers"][1]["trackMatte"]["mode"] = json!("lumaInverted")),
            "the track matte mode is lumaInverted, which Premiere's Reverse with Matte Luma does not render: it gives the matte clip's zero-luma exterior full coverage where FX gives none",
        ),
        (
            "masks beside the matte",
            Box::new(|wire| {
                wire["composition"]["layers"][1]["masks"] =
                    json!([{"id": 50, "mode": "add", "layer": 99}])
            }),
            "a track matte with masks on one clip is not converted; FX's intersection of the two is unverified against Premiere",
        ),
        (
            "hidden source",
            Box::new(|wire| wire["composition"]["layers"][0]["isHidden"] = json!(true)),
            "the track matte source is hidden",
        ),
        (
            "source over another range",
            Box::new(|wire| {
                wire["composition"]["layers"][0]["playback"] = crate::test_support::linear_playback(
                    json!({"start": wire["composition"]["layers"][0]["playback"]["inputRange"]["start"], "duration": 4000}),
                    json!({"start": wire["composition"]["layers"][0]["sourceRange"]["start"], "duration": 4000}),
                );
                wire["composition"]["layers"][0]["sourceRange"]["duration"] = json!(4000);
            }),
            "the track matte source's range differs from the clip's; only a source spanning exactly the clip's range converts",
        ),
        (
            "source with its own matte",
            Box::new(|wire| {
                wire["composition"]["layers"][0]["trackMatte"] =
                    json!({"mode": "alpha", "layer": 99})
            }),
            "the track matte source has its own track matte or masks",
        ),
        (
            "source that is no sibling",
            Box::new(|wire| wire["composition"]["layers"][1]["trackMatte"]["layer"] = json!(99)),
            "the track matte source is not beside the clip",
        ),
        (
            "Color Matte source",
            Box::new(move |wire| {
                wire["composition"]["layers"][0] = rect.clone();
                wire["composition"]["layers"][1]["trackMatte"]["layer"] = json!(60);
            }),
            "the track matte source is a Rect layer; only a video or still image source is exported",
        ),
        (
            "nest source",
            Box::new(move |wire| {
                wire["composition"]["layers"][0] = nest.clone();
                wire["composition"]["layers"][1]["trackMatte"]["layer"] = json!(61);
            }),
            "the track matte source is a Group layer; only a video or still image source is exported",
        ),
        // An unsupported Shape matte must not expose the keyed video unmasked.
        (
            "Shape source",
            Box::new(move |wire| {
                wire["composition"]["layers"][0] = shape.clone();
                wire["composition"]["layers"][1]["trackMatte"]["layer"] = json!(62);
            }),
            "the track matte source is a Shape layer; only a video or still image source is exported",
        ),
    ];
    for (case, edit, reason) in cases {
        let mut wire = base.clone();
        edit(&mut wire);
        let (project, omissions) = convert_with_omissions(wire).unwrap();
        // The plain clip stays; neither the keyed clip nor its source exports.
        // A video that no key names any more is plain content.
        let expected: &[_] = if case == "source that is no sibling" {
            &[(0, 0, None), (0, 5, None)]
        } else {
            &[(0, 5, None)]
        };
        assert_eq!(keyed_clips(&project), expected, "{case}");
        assert!(
            omissions.iter().any(
                |omission| omission.scope == crate::OmissionScope::Occurrence
                    && omission.record == "layer 1 (\"Premiere video 1\")"
                    && omission.reason
                        == format!("masks cannot be exported: {reason}; occurrence omitted")
            ),
            "{case}: {omissions:?}"
        );
        // The source that stays in the document is not drawn as content.
        let source = match case {
            "Color Matte source" => "layer 60 (\"Solid\")".to_owned(),
            "nest source" => "layer 61 (\"Nest\")".to_owned(),
            "Shape source" => "layer 62 (\"Shape\")".to_owned(),
            _ => format!("layer {source_id} (\"Premiere video 3\")"),
        };
        let expected = if case == "source that is no sibling" {
            // Nothing names layer 1, which exports as visible content.
            None
        } else {
            Some("track matte source of no exported clip was not exported; FX draws it only through the clips that it keys")
        };
        assert_eq!(
            omissions
                .iter()
                .find(|omission| omission.record == source
                    && omission.scope == crate::OmissionScope::Occurrence)
                .map(|omission| omission.reason.as_str()),
            expected,
            "{case}: {omissions:?}"
        );
    }
}
