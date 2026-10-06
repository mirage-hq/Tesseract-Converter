use super::*;
use crate::writer::{
    NativeTransform3d, NumericKeyframe, NumericTrack, Transform3dAnimations,
    footage::{
        FootageClock, FootageKind, FootageSpec, NativeFrameBlending, NativeFrameRate, NativeSource,
        NativeSourceFormat, RelativeMediaPath, SourceGeometry,
    },
    keyframes::{Easing, PropertyClock},
    source_clock::SourceClockPlan,
};

fn layer(occurrence: u64, source: u64, times: &[i64]) -> LayerSpec {
    let plan = SourceClockPlan::affine(
        fx_schema::TimeRangeProperty::new(
            fx_schema::Time::ZERO,
            fx_schema::Duration::from_millis(occurrence),
        ),
        fx_schema::Time::ZERO,
        fx_schema::Time::from_millis(source),
        source,
    )
    .unwrap();
    let footage = FootageSpec {
        name: "Planar Scale control".into(),
        kind: FootageKind::Video,
        source: NativeSource {
            path: RelativeMediaPath::new("media/control.mp4").unwrap(),
            format: NativeSourceFormat::QuickTime,
            dimensions: [320, 180],
            duration_millis: source,
            duration_native_ticks: None,
            frame_rate: NativeFrameRate::integer(30),
            audio_sample_rate: 0.0,
            wave_metadata: None,
            native_duration: None,
        },
        source_geometry: SourceGeometry::default(),
        transform: SolidLayerSpec {
            name: "Planar Scale control".into(),
            width: 320,
            height: 180,
            color: [0.0; 3],
            transform: SolidTransform {
                anchor: [0.0; 2],
                position: [0.0; 2],
                scale: [100.0; 2],
                rotation: 0.0,
                opacity: 100.0,
            },
        },
        clock: FootageClock::Source(plan),
        static_source_time_secs: None,
        time_remap_requires_source_owned_transform: false,
        frame_blending: NativeFrameBlending::Disabled,
        audio_enabled: false,
        audio_levels_db: [-192.0; 2],
        audio_levels_animation: None,
    };
    let track = NumericTrack {
        keys: times
            .iter()
            .enumerate()
            .map(|(index, time)| NumericKeyframe {
                time_millis: *time,
                values: vec![0.1 + index as f64 * 0.11, 0.2 + index as f64 * 0.07, 1.0],
                easing: vec![Easing::Linear; 3],
                spatial_in: vec![],
                spatial_out: vec![],
            })
            .collect(),
    };
    let follower = |offset| NumericTrack {
        keys: track
            .keys
            .iter()
            .enumerate()
            .map(|(index, key)| NumericKeyframe {
                time_millis: key.time_millis,
                values: vec![offset + index as f64 * 10.0],
                easing: vec![Easing::Linear],
                spatial_in: vec![],
                spatial_out: vec![],
            })
            .collect(),
    };
    let position_separated = Some([Some(follower(240.0)), Some(follower(100.0)), None]);
    LayerSpec::Options(
        Box::new(LayerSpec::Footage(footage, TransformAnimations::default())),
        NativeLayerOptions {
            fx_id: 10.into(),
            parent: None,
            matte: None,
            enabled: true,
            adjustment_layer: false,
            motion_blur: false,
            blend_mode: 0,
            masks: vec![],
            effects: vec![],
            styles: vec![],
            source_clock: None,
            transform_3d: Some((
                NativeTransform3d {
                    is_three_d: false,
                    anchor: [0.0; 3],
                    position: [0.0; 3],
                    scale: [1.0; 3],
                    orientation: [0.0; 3],
                    rotation_x: 0.0,
                    rotation_y: 0.0,
                    rotation_z: 0.0,
                    opacity: 1.0,
                },
                Transform3dAnimations {
                    scale: Some(track),
                    position_separated,
                    ..Default::default()
                },
            )),
        },
    )
}

#[test]
fn p024_planar_video_scale_keeps_unproved_profiles_rejected() {
    for profile in 0..9 {
        let mut input = layer(3209, 3208, &[0, 2008, 3209]);
        let LayerSpec::Options(base, options) = &mut input else {
            unreachable!()
        };
        let (transform, animations) = options.transform_3d.as_mut().unwrap();
        let track = animations.scale.as_mut().unwrap();
        let LayerSpec::Footage(footage, _) = base.as_mut() else {
            unreachable!()
        };
        match profile {
            0 => transform.is_three_d = true,
            1 => track.keys[1].easing = vec![Easing::Hold, Easing::Linear, Easing::Hold],
            2 => {
                track.keys[1].easing = vec![
                    Easing::CubicBezier {
                        x1: 0.2,
                        y1: 0.1,
                        x2: 0.8,
                        y2: 0.9
                    };
                    3
                ]
            }
            3 => track.keys[1].spatial_in = vec![0.0; 3],
            4 => footage.kind = FootageKind::Audio,
            5 => footage.static_source_time_secs = Some(0.0),
            6 => footage.time_remap_requires_source_owned_transform = true,
            7 => {
                animations.position_separated.as_mut().unwrap()[0]
                    .as_mut()
                    .unwrap()
                    .keys[1]
                    .easing = vec![Easing::CubicBezier {
                    x1: 0.2,
                    y1: 0.1,
                    x2: 0.8,
                    y2: 0.9,
                }]
            }
            _ => {
                let followers = animations.position_separated.as_mut().unwrap();
                followers[2] = followers[0].clone();
            }
        }
        assert!(
            build_timeline_at_rate(
                std::slice::from_ref(&input),
                Duration24::from_frames(168).unwrap(),
                [320, 180],
                crate::timing::FrameRate::new(30.0).unwrap()
            )
            .is_err(),
            "profile {profile}"
        );
    }
}

#[test]
fn p024_planar_video_scale_preserves_two_stage_source_ticks() {
    for (occurrence, source, times, expected) in [
        (
            3209,
            3208,
            vec![0, 2008, 2042, 2500, 3209],
            vec![0, 61667, 62710, 76776, 98549],
        ),
        (
            6417,
            6416,
            vec![0, 900, 1317, 2600, 6417],
            vec![0, 27644, 40452, 79860, 197099],
        ),
    ] {
        let mut input = layer(occurrence, source, &times);
        if occurrence == 3209 {
            let LayerSpec::Options(_, options) = &mut input else {
                unreachable!()
            };
            let animations = &mut options.transform_3d.as_mut().unwrap().1;
            animations.scale.as_mut().unwrap().keys[1].easing = vec![Easing::Hold; 3];
            for track in animations
                .position_separated
                .as_mut()
                .unwrap()
                .iter_mut()
                .flatten()
            {
                track.keys[1].easing = vec![Easing::Hold];
            }
        }
        let original = input.envelope().unwrap().2.unwrap().clone();
        let timeline = build_timeline_at_rate(
            std::slice::from_ref(&input),
            Duration24::from_frames(168).unwrap(),
            [320, 180],
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let output = timeline
            .layers
            .iter()
            .find(|c| c.list_kind() == Some(*b"Layr"))
            .unwrap();
        let roots = crate::properties::root_runs(output.children().unwrap()).unwrap();
        let transform = roots
            .iter()
            .find(|(name, _)| *name == "ADBE Transform Group")
            .unwrap()
            .1;
        let properties =
            crate::properties::runs(crate::properties::unique_list(transform, *b"tdgp").unwrap())
                .unwrap();
        for name in ["ADBE Position_0", "ADBE Position_1"] {
            let follower = properties
                .iter()
                .find(|(match_name, _)| *match_name == name)
                .unwrap()
                .1;
            let chunks = crate::properties::unique_list(follower, *b"tdbs").unwrap();
            assert_eq!(
                crate::properties::data(chunks, *b"tdsb").unwrap(),
                &1_u32.to_be_bytes()
            );
            let data = crate::properties::data(
                crate::properties::unique_list(chunks, *b"list").unwrap(),
                *b"ldat",
            )
            .unwrap();
            let units: Vec<_> = data
                .chunks_exact(48)
                .map(|item| i32::from_be_bytes(item[..4].try_into().unwrap()))
                .collect();
            assert_eq!(units, expected);
            if occurrence == 3209 {
                assert_eq!(data[5], 3);
                assert_eq!(data[48 + 4], 3);
            }
        }
        let scale = properties
            .iter()
            .find(|(name, _)| *name == "ADBE Scale")
            .unwrap()
            .1;
        let chunks = crate::properties::unique_list(scale, *b"tdbs").unwrap();
        let descriptor = crate::properties::data(chunks, *b"tdb4").unwrap();
        assert_eq!(&descriptor[12..16], &30720_u32.to_be_bytes());
        let list = crate::properties::unique_list(chunks, *b"list").unwrap();
        let data = crate::properties::data(list, *b"ldat").unwrap();
        let actual: Vec<_> = data
            .chunks_exact(128)
            .map(|item| i32::from_be_bytes(item[..4].try_into().unwrap()))
            .collect();
        assert_eq!(actual, expected);
        if occurrence == 3209 {
            assert_eq!(data[5], 3, "previous Scale key's outgoing Hold is retained");
            assert_eq!(
                data[128 + 4],
                3,
                "arriving Scale key's incoming Hold is retained"
            );
            assert_eq!(data[128 + 5], 1, "following segment remains Linear");
        }
        let numeric = crate::properties::read_numeric(chunks).unwrap();
        assert_eq!(numeric.keyframes.len(), times.len());
        for (index, key) in numeric.keyframes.iter().enumerate() {
            assert_eq!(
                key.values,
                [0.1 + index as f64 * 0.11, 0.2 + index as f64 * 0.07, 1.0]
            );
        }
        assert_eq!(
            input.envelope().unwrap().2.unwrap(),
            &original,
            "authored occurrence keys are not rebased to rounded source ms"
        );
        let LayerSpec::Options(base, _) = &input else {
            unreachable!()
        };
        let LayerSpec::Footage(spec, _) = base.as_ref() else {
            unreachable!()
        };
        let FootageClock::Source(clock) = &spec.clock else {
            unreachable!()
        };
        assert!(
            clock.source_time_millis(times[1]).is_err(),
            "generic integral-ms guard remains"
        );
        assert_eq!(
            PropertyClock::for_rate(crate::timing::FrameRate::new(30.0).unwrap())
                .unwrap()
                .ticks(),
            30720
        );
    }
}
