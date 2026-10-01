use super::*;
use crate::writer::{
    NumericKeyframe, NumericTrack, VectorGroupAnimations, VectorGroupSpec, VectorGroupTransform,
    VectorPaintAnimations,
};
use fx_schema::layer::{BlendMode, ShapeFillRule};

fn fill(alpha: f64) -> VectorContent {
    VectorContent::Paint(VectorPaintSpec::Fill {
        paint: ShapePaint::Solid {
            color: [0.2, 0.4, 0.6, alpha],
        },
        fill_rule: ShapeFillRule::NonZeroWinding,
        blend_mode: BlendMode::default(),
        opacity: 50.0,
        animations: VectorPaintAnimations::default(),
    })
}

fn group(contents: Vec<VectorContent>) -> VectorGroupSpec {
    VectorGroupSpec {
        name: "nested".into(),
        blend_mode: BlendMode::default(),
        transform: VectorGroupTransform {
            opacity: 35.0,
            ..Default::default()
        },
        contents,
    }
}

fn key(value: f64) -> NumericTrack {
    NumericTrack {
        keys: vec![NumericKeyframe {
            time_millis: 250,
            values: vec![value],
            easing: Vec::new(),
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        }],
    }
}

#[test]
fn single_paint_normalization_preserves_color_and_other_controls() {
    let mut contents = vec![fill(0.4)];
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut contents[0] else {
        unreachable!()
    };
    *opacity = 10000.0;
    assert_eq!(
        normalize_single_paint(&mut contents, 0.999, None),
        Some(99.9)
    );
    let VectorContent::Paint(VectorPaintSpec::Fill {
        paint: ShapePaint::Solid { color },
        opacity,
        animations,
        ..
    }) = &contents[0]
    else {
        unreachable!()
    };
    assert_eq!(*color, [0.2, 0.4, 0.6, 0.4]);
    assert_eq!(*opacity, 100.0);
    assert_eq!(*animations, VectorPaintAnimations::default());
}

#[test]
fn single_paint_normalization_rejects_saturation_and_independent_animation() {
    for kind in [
        "ordinary",
        "saturated",
        "opacity",
        "color",
        "blend",
        "multiple",
    ] {
        let mut paint = fill(0.4);
        let VectorContent::Paint(VectorPaintSpec::Fill {
            opacity,
            blend_mode,
            animations,
            ..
        }) = &mut paint
        else {
            unreachable!()
        };
        *opacity = if kind == "ordinary" { 100.0 } else { 10000.0 };
        match kind {
            "opacity" => animations.opacity = Some(key(0.5)),
            "color" => animations.color = Some(key(0.5)),
            "blend" => *blend_mode = BlendMode::Multiply,
            _ => {}
        }
        let mut contents = if kind == "multiple" {
            vec![paint.clone(), paint]
        } else {
            vec![paint]
        };
        let original = contents.clone();
        let layer_opacity = if kind == "saturated" { 2.0 } else { 0.999 };
        assert_eq!(
            normalize_single_paint(&mut contents, layer_opacity, None),
            None,
            "{kind}"
        );
        assert_eq!(
            contents, original,
            "{kind} must retain its fallback controls"
        );
    }
}

#[test]
fn animated_single_paint_rejects_unsupported_keys_without_partial_mutation() {
    for kind in [
        "saturated",
        "negative",
        "infinite",
        "nan",
        "dimensions",
        "spatial",
        "empty",
    ] {
        let mut contents = vec![fill(1.0)];
        let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut contents[0] else {
            unreachable!()
        };
        *opacity = 10000.0;
        let mut track = key(0.00999);
        track.keys[0].easing = vec![KeyframeEasing::Linear];
        let mut last = track.keys[0].clone();
        last.time_millis = 500;
        match kind {
            "saturated" => last.values[0] = 0.02,
            "negative" => last.values[0] = -0.01,
            "infinite" => last.values[0] = f64::INFINITY,
            "nan" => last.values[0] = f64::NAN,
            "dimensions" => last.values.push(0.0),
            "spatial" => last.spatial_in.push(0.0),
            _ => {}
        }
        track.keys.push(last);
        if kind == "empty" {
            track.keys.clear();
        }
        let original_contents = contents.clone();
        // Debug comparison retains NaN while PartialEq intentionally doesn't.
        let original_track = format!("{track:?}");
        assert_eq!(
            normalize_single_paint(&mut contents, 0.999, Some(&mut track)),
            None,
            "{kind}"
        );
        assert_eq!(contents, original_contents, "{kind}");
        assert_eq!(format!("{track:?}"), original_track, "{kind}");
    }
}

#[test]
fn keyed_opacity_uses_first_key_instead_of_unused_static_base() {
    let mut contents = vec![fill(1.0)];
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut contents[0] else {
        unreachable!()
    };
    *opacity = 10000.0;
    let mut track = key(0.01);
    track.keys[0].easing = vec![KeyframeEasing::Linear];
    assert_eq!(
        normalize_single_paint(&mut contents, 100.0, Some(&mut track)),
        Some(100.0)
    );
    assert_eq!(track.keys[0].values, [1.0]);
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &contents[0] else {
        unreachable!()
    };
    assert_eq!(*opacity, 100.0);
}

#[test]
fn bounded_cubic_keys_scale_without_changing_handles_or_clocks() {
    let mut contents = vec![fill(1.0)];
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut contents[0] else {
        unreachable!()
    };
    *opacity = 10000.0;
    let mut track = key(0.005147528349381107);
    track.keys[0].easing = vec![KeyframeEasing::Linear];
    track.keys.push(NumericKeyframe {
        time_millis: 333,
        values: vec![0.0009536368537540305],
        easing: vec![KeyframeEasing::CubicBezier {
            x1: 1.0 / 3.0,
            y1: 0.56,
            x2: 2.0 / 3.0,
            y2: 0.9,
        }],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    });
    let handles = track.keys[1].easing.clone();
    assert!(
        (normalize_single_paint(&mut contents, 100.0, Some(&mut track)).unwrap()
            - 51.47528349381107)
            .abs()
            < 1.0e-10
    );
    assert!((track.keys[0].values[0] - 0.5147528349381107).abs() < 1.0e-14);
    assert!((track.keys[1].values[0] - 0.09536368537540305).abs() < 1.0e-14);
    assert_eq!(track.keys[1].time_millis, 333);
    assert_eq!(track.keys[1].easing, handles);
}

#[test]
fn cubic_overshoot_and_invalid_metadata_reject_atomically() {
    for easing in [
        KeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: 100.0,
            x2: 0.7,
            y2: 1.0,
        },
        KeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: 1.0,
            x2: 0.7,
            y2: 100.0,
        },
        KeyframeEasing::CubicBezier {
            x1: f64::NAN,
            y1: 0.0,
            x2: 0.7,
            y2: 1.0,
        },
        KeyframeEasing::CubicBezier {
            x1: 0.0,
            y1: 1.0,
            x2: 0.7,
            y2: 1.0,
        },
    ] {
        let mut contents = vec![fill(1.0)];
        let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut contents[0] else {
            unreachable!()
        };
        *opacity = 10000.0;
        let mut track = key(0.005);
        track.keys[0].easing = vec![KeyframeEasing::Linear];
        track.keys.push(NumericKeyframe {
            time_millis: 500,
            values: vec![0.009],
            easing: vec![easing],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        });
        let original_contents = contents.clone();
        let original_track = format!("{track:?}");
        assert_eq!(
            normalize_single_paint(&mut contents, 100.0, Some(&mut track)),
            None
        );
        assert_eq!(contents, original_contents);
        assert_eq!(format!("{track:?}"), original_track);
    }
}

#[test]
fn native_spark_tail_with_negative_interior_is_normalized() {
    // Private native-controls.log k31..32: 1.704004..1.848014s, outgoing
    // speed -0.00893978956 and incoming +0.00042483982, both 33.33%
    // influence. Its value at cubic parameter 0.8 is about -7.66e-6.
    let mut contents = vec![fill(1.0)];
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut contents[0] else {
        unreachable!()
    };
    *opacity = 10000.0;
    let mut track = key(0.0003977429969141072);
    track.keys[0].easing = vec![KeyframeEasing::Linear];
    track.keys.push(NumericKeyframe {
        time_millis: 394,
        values: vec![0.0],
        easing: vec![KeyframeEasing::CubicBezier {
            x1: 1.0 / 3.0,
            y1: 1.078937156893922,
            x2: 2.0 / 3.0,
            y2: 1.0512736311432933,
        }],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    });
    let handles = track.keys[1].easing.clone();
    assert_eq!(
        normalize_single_paint(&mut contents, 100.0, Some(&mut track)),
        Some(3.977429969141072)
    );
    assert_eq!(track.keys[0].values, [0.03977429969141072]);
    assert_eq!(track.keys[1].values, [0.0]);
    assert_eq!(track.keys[1].time_millis, 394);
    assert_eq!(track.keys[1].easing, handles);
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &contents[0] else {
        unreachable!()
    };
    assert_eq!(*opacity, 100.0);
}

#[test]
fn bounded_cubic_admits_negative_control_without_interior_overshoot() {
    let mut contents = vec![fill(1.0)];
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut contents[0] else {
        unreachable!()
    };
    *opacity = 10000.0;
    let mut track = key(0.005);
    track.keys[0].easing = vec![KeyframeEasing::Linear];
    track.keys.push(NumericKeyframe {
        time_millis: 500,
        values: vec![0.009],
        easing: vec![KeyframeEasing::CubicBezier {
            x1: 0.3,
            y1: -1.0,
            x2: 0.7,
            y2: 0.5,
        }],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    });
    let handles = track.keys[1].easing.clone();
    assert_eq!(
        normalize_single_paint(&mut contents, 100.0, Some(&mut track)),
        Some(50.0)
    );
    assert!((track.keys[1].values[0] - 0.9).abs() < 1.0e-14);
    assert_eq!(track.keys[1].easing, handles);
}

#[test]
fn nested_static_and_animated_groups_fold_every_paint_without_touching_group_transforms() {
    let mut contents = vec![
        fill(0.4),
        VectorContent::Group(group(vec![VectorContent::AnimatedGroup(
            group(vec![fill(0.4)]),
            VectorGroupAnimations {
                opacity: Some(key(0.6)),
                ..Default::default()
            },
        )])),
    ];
    assert!(!fold(&mut contents).unwrap());
    let VectorContent::Group(outer) = &contents[1] else {
        panic!("group missing")
    };
    assert_eq!(outer.transform.opacity, 35.0);
    let VectorContent::AnimatedGroup(inner, group_animation) = &outer.contents[0] else {
        panic!("animated group missing")
    };
    assert_eq!(inner.transform.opacity, 35.0);
    assert_eq!(
        group_animation.opacity.as_ref().unwrap().keys[0].values,
        vec![0.6]
    );
    for content in [&contents[0], &inner.contents[0]] {
        let VectorContent::Paint(VectorPaintSpec::Fill {
            paint: ShapePaint::Solid { color },
            opacity,
            animations,
            ..
        }) = content
        else {
            panic!("fill missing")
        };
        assert_eq!(*opacity, 20.0);
        assert_eq!(color[3], 1.0);
        assert!(animations.opacity.is_none());
    }
}

#[test]
fn nested_invalid_alpha_and_independent_opacity_are_rejected() {
    let mut contents = vec![VectorContent::Group(group(vec![fill(1.5)]))];
    assert_eq!(
        fold(&mut contents),
        Err("Shape paint alpha cannot be folded into native paint Opacity")
    );
    let mut nested = fill(1.0);
    let VectorContent::Paint(VectorPaintSpec::Fill { animations, .. }) = &mut nested else {
        unreachable!()
    };
    animations.opacity = Some(key(0.5));
    let mut contents = vec![VectorContent::AnimatedGroup(
        group(vec![nested]),
        VectorGroupAnimations::default(),
    )];
    assert_eq!(
        fold(&mut contents),
        Err("Independent paint Opacity animation cannot be combined with overrange paint folding")
    );
}

#[test]
fn nested_overrange_paint_clamps_without_creating_opacity_keys() {
    let mut paint = fill(1.0);
    let VectorContent::Paint(VectorPaintSpec::Fill { opacity, .. }) = &mut paint else {
        unreachable!()
    };
    *opacity = 500.0;
    let mut contents = vec![VectorContent::Group(group(vec![paint]))];
    assert!(fold(&mut contents).unwrap());
    let VectorContent::Group(group) = &contents[0] else {
        unreachable!()
    };
    let VectorContent::Paint(VectorPaintSpec::Fill {
        opacity,
        animations,
        ..
    }) = &group.contents[0]
    else {
        unreachable!()
    };
    assert_eq!(*opacity, 100.0);
    assert!(animations.opacity.is_none());
}
