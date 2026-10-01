use crate::{
    format::{FrameRate, PrProjectFile},
    media::{MediaFacts, VideoMedia},
    schema::{
        PrBrightnessContrast, PrColour, PrColourKeyframe, PrCornerPin, PrEffect,
        PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, PrFilmImpactBlur,
        PrFilmImpactDirectionalBlur, PrGaussianBlur, PrInvert, PrKeyframeEasing, PrLevels,
        PrMosaic, PrPointKeyframe, PrRamp, PrScalarKeyframe, PrTint, PrVideoItem,
        BRIGHTNESS_CONTRAST_BRIGHTNESS, BRIGHTNESS_CONTRAST_CONTRAST, CORNER_PIN,
        FILM_IMPACT_BLUR_AMOUNT, INVERT_BLEND, LEVELS, MOSAIC_HORIZONTAL_BLOCKS,
        MOSAIC_VERTICAL_BLOCKS, RAMP_BLEND, RAMP_END, RAMP_START_COLOR, TICKS,
        TICKS_PER_MILLISECOND, TINT_AMOUNT, TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO,
    },
    test_support::editable_document,
    tests::support::{
        amount, current_blur_export, directional_blur, exported_blur, keyed_directional,
        project_document, video_sequence,
    },
    Omission, OmissionKind, OmissionScope,
};
use fx_schema::{
    animator::AnimatorData, EditableFxCompositionDocument, EffectData, EffectPayload, LayerData,
    LayerEffect, PropertyKeyframeEasing, PropertyTarget, PropertyValue,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Length of the one source that `export` inspects and the documents claim.
const SOURCE_MILLIS: i64 = 10_000;

fn blur(enabled: bool, blurriness: f64, repeat_edge_pixels: bool) -> PrEffect {
    PrEffect {
        enabled,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels,
        }),
        animations: Vec::new(),
    }
}

fn key(source_ticks: i64, value: f64, easing: PrKeyframeEasing) -> PrScalarKeyframe {
    PrScalarKeyframe {
        source_ticks,
        value,
        easing,
    }
}

/// Native blur keys, whose static value is the first key's value.
fn keyed(mut effect: PrEffect, keys: Vec<PrScalarKeyframe>) -> PrEffect {
    match &mut effect.params {
        PrEffectParams::GaussianBlur(blur) => blur.blurriness = keys[0].value,
        PrEffectParams::FilmImpactBlur(blur) => blur.amount = keys[0].value,
        _ => panic!("expected a Gaussian Blur"),
    }
    effect.animations = vec![PrEffectParamAnimation {
        param: effect.spec().bound_param("blurriness").unwrap(),
        keys: PrEffectParamKeys::Scalar(keys),
    }];
    effect
}

fn corner_pin(
    enabled: bool,
    corners: [[f64; 2]; 4],
    animations: Vec<PrEffectParamAnimation>,
) -> PrEffect {
    PrEffect {
        enabled,
        params: PrEffectParams::CornerPin(PrCornerPin { corners }),
        animations,
    }
}

fn point_key(source_ticks: i64, value: [f64; 2], easing: PrKeyframeEasing) -> PrPointKeyframe {
    PrPointKeyframe {
        source_ticks,
        value,
        easing,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    }
}

/// The keys of corner `index` in native order (0 is Upper Left).
fn corner_keys(index: usize, keys: Vec<PrPointKeyframe>) -> PrEffectParamAnimation {
    PrEffectParamAnimation {
        param: &CORNER_PIN.params[index],
        keys: PrEffectParamKeys::Point(keys),
    }
}

/// The id, layer time, value and easing of one imported key.
type ImportedKey = (String, i64, f64, PropertyKeyframeEasing);

/// Each effect-parameter track of an imported document: effect id, parameter
/// name and keys.
fn effect_tracks(document: &EditableFxCompositionDocument) -> Vec<(u64, String, Vec<ImportedKey>)> {
    document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .map(|entry| {
            let PropertyTarget::EffectProperty(target) = &entry.target else {
                panic!("expected only effect-parameter tracks: {:?}", entry.target);
            };
            let AnimatorData::Keyframes {
                track,
                enabled: true,
                ..
            } = entry.animator.data()
            else {
                panic!("expected enabled keyframes");
            };
            let keys = track
                .keyframes()
                .iter()
                .map(|key| {
                    let PropertyValue::Float(value) = key.value() else {
                        panic!("expected float keys");
                    };
                    (
                        key.id().as_str().to_owned(),
                        key.layer_time().as_millis(),
                        *value,
                        key.easing(),
                    )
                })
                .collect();
            (
                target.effect_id().value(),
                target.param_name().to_owned(),
                keys,
            )
        })
        .collect()
}

/// The imported document of the shared one-clip sequence, with `effects`.
fn imported(effects: Vec<PrEffect>) -> Value {
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).effects = effects;
    project_document(&sequence)
}

/// Export input: the hand-written one-clip document with an `effects` stack
/// on its video layer (id 1), independent of the forward mapper.
fn document_with_effects(effects: Value) -> Value {
    let mut document = editable_document();
    let layer = &mut document["composition"]["layers"][0];
    layer["sourceIntrinsicDuration"] = json!(SOURCE_MILLIS);
    layer["effects"] = effects;
    document
}

/// Export an edited document against the facts of its one 10 s source.
fn export(wire: Value) -> (PrProjectFile, Vec<Omission>) {
    export_at(wire, FrameRate::Fps30)
}

fn export_at(wire: Value, frame_rate: FrameRate) -> (PrProjectFile, Vec<Omission>) {
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let facts = BTreeMap::from([(
        "premiere-video-1".to_owned(),
        MediaFacts::Video(VideoMedia {
            orientation: crate::schema::VideoOrientation::Identity,
            codec: crate::schema::VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: 1920,
            height: 1080,
            timing: crate::media::VideoTiming::for_test(
                FrameRate::Fps30,
                SOURCE_MILLIS / 1000 * TICKS,
            ),
        }),
    )]);
    let mut omissions = Vec::new();
    let project = crate::convert::tesseract_to_premiere(
        &document,
        &facts,
        &BTreeMap::new(),
        &BTreeMap::new(),
        frame_rate,
        &mut omissions,
    )
    .unwrap();
    (project, omissions)
}

fn exported_effects(project: &PrProjectFile) -> Vec<PrEffect> {
    project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .effects
        .clone()
}

#[test]
fn imported_stack_is_ordered_editable_and_keeps_bypass() {
    let wire = imported(vec![blur(true, 25.0, false), blur(false, 80.0, true)]);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
            {"id": 2, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 80.0, "repeatEdgePixels": true}},
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let LayerData::Video(video) = document.composition().layers()[0].data() else {
        panic!("expected the imported video layer first");
    };
    assert_eq!(
        video
            .effects
            .iter()
            .map(|record| record.data().clone())
            .collect::<Vec<_>>(),
        [(1, true, 25.0, None), (2, false, 80.0, Some(true))].map(
            |(id, enabled, blurriness, repeat_edge_pixels)| EffectData::Identified {
                id: fx_schema::EffectId::new(id),
                enabled,
                effect: EffectPayload::Known(LayerEffect::GaussianBlur {
                    blurriness: fx_schema::NonNegativeProperty::new(blurriness).unwrap(),
                    repeat_edge_pixels,
                    layer_size: None,
                }),
            }
        )
    );
}

#[test]
fn effect_ids_are_unique_across_layers() {
    let mut sequence = video_sequence();
    let mut second = sequence.video_tracks[0].clip(0).clone();
    second.start_ticks = 5 * TICKS;
    second.end_ticks = 7 * TICKS;
    second.in_ticks = 5 * TICKS;
    second.out_ticks = 7 * TICKS;
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(second));
    for index in 0..2 {
        sequence.video_tracks[0].clip_mut(index).effects = vec![blur(true, 5.0, false)];
    }
    let wire = project_document(&sequence);
    let ids: Vec<_> = wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|layer| layer["effects"][0]["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [1, 2]);
}

#[test]
fn keyed_blurriness_imports_as_effect_parameter_tracks() {
    use PrKeyframeEasing::{Hold, Linear};
    let bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let mut sequence = video_sequence();
    let mut second = sequence.video_tracks[0].clip(0).clone();
    (second.start_ticks, second.end_ticks) = (5 * TICKS, 7 * TICKS);
    (second.in_ticks, second.out_ticks) = (0, 2 * TICKS);
    let clip = sequence.video_tracks[0].clip_mut(0);
    // Source In 1 s: keys before, inside and after the trimmed range stay.
    (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
    clip.effects = vec![
        keyed(
            blur(true, 0.0, true),
            vec![
                key(TICKS / 2, 606.0, Linear),
                key(2 * TICKS, 0.0, Hold),
                key(7 * TICKS, 115.0, bezier),
            ],
        ),
        keyed(
            blur(false, 0.0, false),
            vec![key(TICKS, 10.0, Linear), key(2 * TICKS, 40.0, Linear)],
        ),
    ];
    second.effects = vec![keyed(
        blur(true, 0.0, false),
        vec![key(0, 30.0, Linear), key(TICKS / 5, 0.0, Linear)],
    )];
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(second));
    let wire = project_document(&sequence);
    // A keyed Blurriness keeps its first key as the static value; 606 is above
    // the renderer's cap of 300 and stays exact.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 606.0, "repeatEdgePixels": true}},
            {"id": 2, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let linear = PropertyKeyframeEasing::Linear;
    let expected_key = |id: &str, layer_millis: i64, value: f64, easing| {
        (format!("premiere-effect-{id}"), layer_millis, value, easing)
    };
    // Key ids name the effect and the layer, so every id stays unique.
    assert_eq!(
        effect_tracks(&document),
        [
            (
                1,
                "blurriness".to_owned(),
                vec![
                    expected_key("1-blurriness-1-0", -500, 606.0, linear),
                    expected_key("1-blurriness-1-1", 1000, 0.0, PropertyKeyframeEasing::Hold),
                    expected_key(
                        "1-blurriness-1-2",
                        6000,
                        115.0,
                        PropertyKeyframeEasing::CubicBezier {
                            x1: 0.25,
                            y1: 0.1,
                            x2: 0.75,
                            y2: 0.9,
                        }
                    ),
                ]
            ),
            (
                2,
                "blurriness".to_owned(),
                vec![
                    expected_key("2-blurriness-1-0", 0, 10.0, linear),
                    expected_key("2-blurriness-1-1", 1000, 40.0, linear),
                ]
            ),
            (
                3,
                "blurriness".to_owned(),
                vec![
                    expected_key("3-blurriness-2-0", 0, 30.0, linear),
                    expected_key("3-blurriness-2-1", 200, 0.0, linear),
                ]
            ),
        ]
    );
}

#[test]
fn keyed_corner_pin_imports_as_editable_corner_tracks() {
    use PrKeyframeEasing::{Hold, Linear};
    let bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    // Source In 1 s: keys before and after the trimmed range stay.
    (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
    // A spin of the upper corners above a static blur. Keyed corners start at
    // their first keys.
    clip.effects = vec![
        corner_pin(
            true,
            [[0.0, 0.0], [1.0, 0.0], [0.1, 1.0], [0.9, 1.0]],
            vec![
                corner_keys(
                    0,
                    vec![
                        point_key(TICKS / 2, [0.0, 0.0], Linear),
                        point_key(2 * TICKS, [-0.3, 0.0], Linear),
                        point_key(7 * TICKS, [0.0, 0.0], Hold),
                    ],
                ),
                corner_keys(
                    1,
                    vec![
                        point_key(TICKS / 2, [1.0, 0.0], Linear),
                        point_key(2 * TICKS, [1.3, 0.1], bezier),
                    ],
                ),
            ],
        ),
        blur(true, 10.0, false),
    ];
    let wire = project_document(&sequence);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "cornerPin",
                "upperLeftX": 0.0, "upperLeftY": 0.0, "upperRightX": 1.0, "upperRightY": 0.0,
                "lowerLeftX": 0.1, "lowerLeftY": 1.0, "lowerRightX": 0.9, "lowerRightY": 1.0}},
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut tracks = effect_tracks(&document);
    tracks.sort_by(|left, right| left.1.cmp(&right.1));
    // Each coordinate keeps its corner's key times and easing.
    let fx_bezier = PropertyKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let track = |param: &str, keys: &[(i64, f64, PropertyKeyframeEasing)]| {
        let keys = keys
            .iter()
            .enumerate()
            .map(|(index, &(millis, value, easing))| {
                (
                    format!("premiere-effect-1-{param}-1-{index}"),
                    millis,
                    value,
                    easing,
                )
            })
            .collect();
        (1, param.to_owned(), keys)
    };
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    assert_eq!(
        tracks,
        [
            track(
                "upperLeftX",
                &[(-500, 0.0, linear), (1000, -0.3, linear), (6000, 0.0, hold)]
            ),
            track(
                "upperLeftY",
                &[(-500, 0.0, linear), (1000, 0.0, linear), (6000, 0.0, hold)]
            ),
            track(
                "upperRightX",
                &[(-500, 1.0, linear), (1000, 1.3, fx_bezier)]
            ),
            track(
                "upperRightY",
                &[(-500, 0.0, linear), (1000, 0.1, fx_bezier)]
            ),
        ]
    );
}

#[test]
fn keyed_directional_blur_imports_compensated_and_exports_back() {
    use PrKeyframeEasing::{Hold, Linear};
    let bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    // Source In 1 s: keys before, inside and after the trimmed range stay.
    let native = keyed_directional(
        directional_blur(true, 0.0, 0.0),
        vec![key(TICKS / 2, 45.1, Linear), key(3 * TICKS, 100.9, Linear)],
        vec![
            key(TICKS / 2, 12.7, Linear),
            key(2 * TICKS, 0.3, Hold),
            key(7 * TICKS, 30.0, bezier),
        ],
    );
    // The current Directional Blur imports as the Legacy blur of the same
    // Blur Length, and both export as the current blur.
    for source in [native.clone(), current_blur_export(native.clone())] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
        (clip.transform.scale, clip.transform.rotation) = ([170.0, 170.0], 33.3);
        clip.effects = vec![source];
        let wire = project_document(&sequence);
        // Scale 170 multiplies every Blur Length by 1.7 and Rotation 33.3 turns
        // every Direction, static (the first key's) and keyed; the normalized
        // easing is unchanged.
        let (scale, rotation) = (170.0 / 100.0, 33.3);
        assert_eq!(
            wire["composition"]["layers"][0]["effects"],
            json!([{"id": 1, "enabled": true, "effect": {"type": "directionalBlur", "direction": 45.1 + rotation, "blurLength": 12.7 * scale}}])
        );
        let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
        let tracks: BTreeMap<_, Vec<_>> = effect_tracks(&document)
            .into_iter()
            .map(|(_, param, keys)| {
                let keys = keys
                    .into_iter()
                    .map(|(_, millis, value, easing)| (millis, value, easing));
                (param, keys.collect())
            })
            .collect();
        let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
        let fx_bezier = PropertyKeyframeEasing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.75,
            y2: 0.9,
        };
        #[rustfmt::skip]
        let expected = BTreeMap::from([
            ("blurLength".to_owned(), vec![(-500, 12.7 * scale, linear), (1000, 0.3 * scale, hold), (6000, 30.0 * scale, fx_bezier)]),
            ("direction".to_owned(), vec![(-500, 45.1 + rotation, linear), (2000, 100.9 + rotation, linear)]),
        ]);
        assert_eq!(tracks, expected);
        // Export inverts the map. Dividing by 1.7 and subtracting 33.3 can leave
        // one unit in the last place (12.7 * 1.7 / 1.7 is 12.700000000000001), so
        // the effects compare to 12 decimal places.
        let (project, omissions) = export(wire);
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(
            format!("{:.12?}", exported_effects(&project)),
            format!("{:.12?}", [current_blur_export(native.clone())])
        );
    }
}

#[test]
fn directional_blur_on_a_clip_without_a_static_similarity_is_omitted() {
    use crate::schema::PrPropertyAnimation::{Rotation, UniformScale};
    let keys = vec![
        key(0, 100.0, PrKeyframeEasing::Linear),
        key(TICKS, 50.0, PrKeyframeEasing::Linear),
    ];
    // (Motion keys, static Scale Width and Scale, the form that omits the blur)
    #[rustfmt::skip]
    let clips = [
        (vec![UniformScale(keys.clone())], [100.0; 2], "its clip has keyed Scale"),
        (vec![Rotation(keys.clone())], [100.0; 2], "its clip has keyed Rotation"),
        (Vec::new(), [50.0, 80.0], "its clip's static Scale is nonuniform (50% by 80%)"),
        (Vec::new(), [0.0; 2], "its clip's static Scale 0% is not positive"),
    ];
    for (animations, scale, form) in clips {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        (clip.animations, clip.transform.scale) = (animations, scale);
        clip.effects = vec![directional_blur(true, 0.0, 30.0), blur(true, 10.0, false)];
        let media = crate::tests::support::video_media();
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        // The clip and its Gaussian Blur convert; only the Directional Blur is
        // omitted.
        assert_eq!(
            wire["composition"]["layers"][0]["effects"],
            json!([{"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}}]),
            "{form}"
        );
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "source".to_owned(),
                reason: format!(
                    "Directional Blur effect at stack position 1 was not imported: {form}; {}",
                    super::SIMILARITY_RULE
                ),
            }]
        );
    }
}

#[test]
fn keyed_brightness_contrast_imports_as_editable_tracks_and_exports_back() {
    use PrKeyframeEasing::{Hold, Linear};
    let bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let effect = |enabled, [brightness, contrast]: [f64; 2], animations| PrEffect {
        enabled,
        params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness,
            contrast,
        }),
        animations,
    };
    let scalar = |param, keys| PrEffectParamAnimation {
        param,
        keys: PrEffectParamKeys::Scalar(keys),
    };
    // Source In 1 s: keys before, inside and after the trimmed range stay. The
    // keyed effect sits above a Gaussian Blur and a bypassed static one.
    #[rustfmt::skip]
    let native = vec![
        effect(true, [10.5, 20.0], vec![
            scalar(&BRIGHTNESS_CONTRAST_BRIGHTNESS, vec![key(TICKS / 2, 10.5, Linear), key(2 * TICKS, -100.0, Hold), key(7 * TICKS, 100.0, bezier)]),
            scalar(&BRIGHTNESS_CONTRAST_CONTRAST, vec![key(TICKS, 20.0, Linear), key(3 * TICKS, -40.0, Linear)]),
        ]),
        blur(true, 10.0, false),
        effect(false, [37.0, -25.0], Vec::new()),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
    clip.effects = native.clone();
    let wire = project_document(&sequence);
    // Values map unchanged, and a keyed static value is its first key's.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "brightnessContrast", "brightness": 10.5, "contrast": 20.0}},
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            {"id": 3, "enabled": false, "effect": {"type": "brightnessContrast", "brightness": 37.0, "contrast": -25.0}},
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    // Key ids name the effect, the parameter and the layer.
    let imported_key = |name: &str, index, layer_millis, value, easing| {
        (
            format!("premiere-effect-1-{name}-1-{index}"),
            layer_millis,
            value,
            easing,
        )
    };
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    let fx_bezier = PropertyKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    #[rustfmt::skip]
    let expected = [
        (1, "brightness".to_owned(), vec![imported_key("brightness", 0, -500, 10.5, linear), imported_key("brightness", 1, 1000, -100.0, hold), imported_key("brightness", 2, 6000, 100.0, fx_bezier)]),
        (1, "contrast".to_owned(), vec![imported_key("contrast", 0, 0, 20.0, linear), imported_key("contrast", 1, 2000, -40.0, linear)]),
    ];
    assert_eq!(effect_tracks(&document), expected);
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let expected: Vec<_> = native.into_iter().map(current_blur_export).collect();
    assert_eq!(exported_effects(&project), expected);
}

/// An Invert of every channel with the native `blend` and its `keys`, possibly
/// empty. A keyed static value is its first key's.
fn invert(enabled: bool, blend: f64, keys: Vec<PrScalarKeyframe>) -> PrEffect {
    PrEffect {
        enabled,
        params: PrEffectParams::Invert(PrInvert { blend }),
        animations: (!keys.is_empty())
            .then_some(PrEffectParamAnimation {
                param: &INVERT_BLEND,
                keys: PrEffectParamKeys::Scalar(keys),
            })
            .into_iter()
            .collect(),
    }
}

/// The FX `levels` of an Invert with Blend With Original `blend`, enabled or
/// not: neutral inputs and Gamma, output white 255 * blend / 100 and output
/// black its complement.
fn invert_levels(id: u64, enabled: bool, blend: f64) -> Value {
    let output_white = blend * 51.0 / 20.0;
    json!({"id": id, "enabled": enabled, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0,
        "gamma": 1.0, "outputBlack": 255.0 - output_white, "outputWhite": output_white}})
}

#[test]
fn keyed_invert_imports_as_complementary_levels_tracks_and_exports_back() {
    use PrKeyframeEasing::{Hold, Linear};
    let bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    // Source In 1 s: keys before, inside and after the trimmed range stay. The
    // keyed effect sits above a Gaussian Blur, a static blend and a bypassed
    // full inversion.
    #[rustfmt::skip]
    let native = vec![
        invert(true, 100.0, vec![key(TICKS / 2, 100.0, Linear), key(2 * TICKS, 20.0, Hold), key(7 * TICKS, 0.0, bezier)]),
        blur(true, 10.0, false),
        invert(true, 30.0, Vec::new()),
        invert(false, 0.0, Vec::new()),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
    clip.effects = native.clone();
    let wire = project_document(&sequence);
    // Each Invert is a Levels with complementary outputs; a keyed static value
    // is its first key's, and bypass is the disabled record.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            invert_levels(1, true, 100.0),
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            invert_levels(3, true, 30.0),
            invert_levels(4, false, 0.0),
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    // The Blend keys animate both outputs at the same times with the same
    // easing: 255 * blend / 100 and its complement. Key ids name the effect,
    // the parameter and the layer.
    let imported_key = |name: &str, index, layer_millis, value, easing| {
        (
            format!("premiere-effect-1-{name}-1-{index}"),
            layer_millis,
            value,
            easing,
        )
    };
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    let fx_bezier = PropertyKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    #[rustfmt::skip]
    let expected = [
        (1, "outputWhite".to_owned(), vec![imported_key("outputWhite", 0, -500, 255.0, linear), imported_key("outputWhite", 1, 1000, 51.0, hold), imported_key("outputWhite", 2, 6000, 0.0, fx_bezier)]),
        (1, "outputBlack".to_owned(), vec![imported_key("outputBlack", 0, -500, 0.0, linear), imported_key("outputBlack", 1, 1000, 204.0, hold), imported_key("outputBlack", 2, 6000, 255.0, fx_bezier)]),
    ];
    assert_eq!(effect_tracks(&document), expected);
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let expected: Vec<_> = native.into_iter().map(current_blur_export).collect();
    assert_eq!(exported_effects(&project), expected);
}

#[test]
fn invalid_model_values_are_reported_instead_of_imported() {
    let mut omissions = Vec::new();
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).effects = vec![blur(true, f64::NAN, false)];
    let ids = crate::tesseract_output::asset_ids_in_order(
        &sequence,
        &crate::tests::support::video_media(),
    );
    let document = crate::convert::premiere_to_tesseract(
        &sequence,
        &crate::tests::support::video_media(),
        &ids,
        &mut omissions,
    )
    .unwrap();
    let wire = document.to_json_value().unwrap();
    assert!(wire["composition"]["layers"][0]["effects"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert!(
        omissions
            .iter()
            .any(|item| item.scope == OmissionScope::Feature
                && item
                    .reason
                    .starts_with("Gaussian Blur effect at stack position 1 was not imported")),
        "{omissions:?}"
    );
}

/// Stills and mattes become Image and Rect layers, which import no effects:
/// each effect on them is reported, and the layer is kept.
#[test]
fn effects_on_stills_and_mattes_are_reported_instead_of_dropped() {
    use crate::schema::{
        color_matte::COLOR_MATTE_INTRINSIC_TICKS, MediaId, PrColorMatte, PrMedia, PrMediaKind,
        PrVideoOccurrence, PrVideoStream, PrVideoTrack, STILL_INTRINSIC_TICKS,
    };
    // Generator placements on the 30 fps test sequence start one hour in.
    const COLOR_MATTE_SOURCE_IN_TICKS: i64 = crate::FrameRate::Fps30.generator_in_ticks();
    const STILL_SOURCE_IN_TICKS: i64 = COLOR_MATTE_SOURCE_IN_TICKS;
    let mut sequence = video_sequence();
    let template = sequence.video_tracks[0].clip(0).clone();
    let placement = |media: &str, start: i64, source_in: i64, effects| PrVideoOccurrence {
        media: MediaId(media.into()),
        start_ticks: start,
        end_ticks: start + 2 * TICKS,
        in_ticks: source_in,
        out_ticks: source_in + 2 * TICKS,
        effects,
        ..template.clone()
    };
    sequence.video_tracks.push(PrVideoTrack::media([
        placement(
            "photo",
            0,
            STILL_SOURCE_IN_TICKS,
            vec![
                keyed(
                    blur(true, 0.0, false),
                    vec![
                        key(STILL_SOURCE_IN_TICKS, 25.0, PrKeyframeEasing::Linear),
                        key(STILL_SOURCE_IN_TICKS + TICKS, 0.0, PrKeyframeEasing::Linear),
                    ],
                ),
                blur(false, 80.0, true),
            ],
        ),
        placement(
            "red",
            2 * TICKS,
            COLOR_MATTE_SOURCE_IN_TICKS,
            vec![
                blur(true, 10.0, false),
                corner_pin(
                    true,
                    [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
                    vec![corner_keys(
                        0,
                        vec![
                            point_key(
                                COLOR_MATTE_SOURCE_IN_TICKS,
                                [0.0, 0.0],
                                PrKeyframeEasing::Linear,
                            ),
                            point_key(
                                COLOR_MATTE_SOURCE_IN_TICKS + TICKS,
                                [0.2, 0.1],
                                PrKeyframeEasing::Linear,
                            ),
                        ],
                    )],
                ),
            ],
        ),
    ]));
    let generated = |name: &str, intrinsic_ticks, kind| PrMedia {
        name: name.into(),
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: Some(PrVideoStream {
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks,
            frame_rate: (FrameRate::Fps30).into(),
            width: 1920,
            height: 1080,
            kind,
        }),
        audio: None,
    };
    let mut media = crate::tests::support::video_media();
    media.insert(
        MediaId("photo".into()),
        generated(
            "photo.jpg",
            STILL_INTRINSIC_TICKS,
            PrMediaKind::Still { alpha: false },
        ),
    );
    media.insert(
        MediaId("red".into()),
        generated(
            "Color Matte",
            COLOR_MATTE_INTRINSIC_TICKS,
            PrMediaKind::ColorMatte(PrColorMatte { rgb: [255, 0, 0] }),
        ),
    );
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let layers = wire["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Image", "Rect", "Video", "Rect"]
    );
    assert!(
        layers.iter().all(|layer| layer.get("effects").is_none()),
        "{layers:?}"
    );
    // The keyed blur's keys are reported with it, not imported.
    assert!(wire["composition"]["dynamics"]["entries"]
        .as_array()
        .is_none_or(Vec::is_empty));
    let omission = |record: &str, reason: &str| Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Omitted,
        record: record.into(),
        reason: reason.into(),
    };
    assert_eq!(
        omissions,
        [
            omission("photo", "Gaussian Blur effect at stack position 1 was not imported: effects on a still image are not converted"),
            omission("photo", "bypassed Gaussian Blur effect at stack position 2 was not imported: effects on a still image are not converted"),
            omission("red", "Gaussian Blur effect at stack position 1 was not imported: effects on a Color Matte are not converted"),
            omission("red", "Corner Pin effect at stack position 2 was not imported: effects on a Color Matte are not converted"),
        ]
    );
}

// Writing and rereading exported stacks is covered by `format::tests::effects`,
// and export plus reimport by the public API tests.
#[test]
fn edited_order_bypass_and_values_export() {
    // An edited stack: blur 2 moved first and re-enabled, blur 1 changed to 40.
    let (project, omissions) = export(document_with_effects(json!([
        {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 80.0, "repeatEdgePixels": true}},
        {"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 40.0}},
    ])));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        exported_effects(&project),
        [
            exported_blur(true, 80.0, true),
            exported_blur(true, 40.0, false)
        ]
    );
}

#[test]
fn bypassed_effect_exports_as_bypassed() {
    let (project, omissions) = export(document_with_effects(json!([
        {"id": 1, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 3.5}},
    ])));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        exported_effects(&project),
        [exported_blur(false, 3.5, false)]
    );
}

#[test]
fn unmapped_effects_are_reported_and_the_rest_keep_their_order() {
    let (project, omissions) = export(document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 9, "effect": {"type": "posterize", "levels": 7.0}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 20.0}},
    ])));
    assert_eq!(
        exported_effects(&project),
        [
            exported_blur(true, 10.0, false),
            exported_blur(true, 20.0, false)
        ]
    );
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".to_owned(),
            reason:
                "effects: posterize effect 9 was not exported: it has no Premiere effect mapping"
                    .to_owned(),
        }]
    );
}

#[test]
fn effect_names_follow_the_persisted_wire_type() {
    for effect_type in ["gaussianBlur", "futureEffect"] {
        let payload: EffectPayload =
            serde_json::from_value(json!({"type": effect_type, "blurriness": 25.0})).unwrap();
        assert_eq!(super::effect_type(&payload), effect_type);
    }
}

#[test]
fn omitted_effects_are_named_by_their_wire_type() {
    let (_, omissions) = export(document_with_effects(json!([
        {"id": 3, "effect": {"type": "personMatte"}},
        {"id": 4, "effect": {"type": "futureEffect", "strength": 2.0}},
    ])));
    assert_eq!(
        omissions
            .iter()
            .map(|item| item.reason.as_str())
            .collect::<Vec<_>>(),
        [
            "effects: personMatte effect 3 was not exported: it has no Premiere effect mapping",
            "effects: futureEffect effect 4 was not exported: it has no Premiere effect mapping",
        ]
    );
}

#[test]
fn animated_effect_parameters_are_omitted_instead_of_flattened() {
    let key = |id: &str, layer_time: i64, value: f64| json!({"id": id, "layerTime": layer_time, "value": {"type": "float", "value": value}, "easing": {"type": "linear"}});
    let animated = |effect_id: u64, param_name: &str| {
        json!({
            "target": {"kind": "effectProperty", "effectId": effect_id, "paramName": param_name},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [key("a", 0, 0.0), key("b", 500, 1.0)]},
        })
    };
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 5.0}},
    ]));
    // A hand-written animation of a parameter that has no Premiere keyframe
    // binding; Blurriness keys export (see below).
    wire["composition"]["dynamics"] = json!({"entries": [animated(1, "repeatEdgePixels")]});
    let (project, omissions) = export(wire);
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 5.0, false)]
    );
    // One omission, from the effect; the generic animation loop skips it.
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".to_owned(),
            reason: "effects: gaussianBlur effect 1 was not exported: animated repeatEdgePixels has no static Premiere value; only static effect parameters export".to_owned(),
        }]
    );

    // A target that no video layer's effect owns keeps the generic omission.
    let mut wire = document_with_effects(json!([]));
    wire["composition"]["layers"][0]
        .as_object_mut()
        .unwrap()
        .remove("effects");
    wire["composition"]["dynamics"] = json!({"entries": [animated(99, "blurriness")]});
    let (_, omissions) = export(wire);
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "composition".to_owned(),
            reason: "unsupported animation target was not exported".to_owned(),
        }]
    );
}

/// A document whose blur 1 (static 25) has Blurriness keys `keys`, on source
/// 2-3 s of its asset.
fn document_with_blurriness_keys(keys: Value) -> Value {
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 25.0, "repeatEdgePixels": true}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
    ]));
    wire["composition"]["layers"][0]["sourceRange"]["start"] = json!(2000);
    wire["composition"]["layers"][0]["playback"]["mapping"]["output"]["start"] = json!(2000);
    wire["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 1, "paramName": "blurriness"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": keys},
    }]});
    wire
}

fn fx_key(id: &str, layer_time: i64, value: f64, easing: Value) -> Value {
    json!({"id": id, "layerTime": layer_time, "value": {"type": "float", "value": value}, "easing": easing})
}

#[test]
fn keyed_blurriness_exports_native_keys_that_start_at_the_first_key() {
    let bezier = json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9});
    // The first key comes 0.5 s after the clip In and differs from the static 25.
    let wire = document_with_blurriness_keys(json!([
        fx_key("a", 500, 80.0, json!({"type": "linear"})),
        fx_key("b", 700, 40.0, json!({"type": "hold"})),
        fx_key("c", 900, 0.0, bezier),
    ]));
    let millisecond = TICKS_PER_MILLISECOND;
    let expected = [
        // Export writes the first key's value as the static value, the value
        // AME renders before that key, so nothing is rejected.
        keyed(
            exported_blur(true, 0.0, true),
            vec![
                key(2500 * millisecond, amount(80.0), PrKeyframeEasing::Linear),
                key(2700 * millisecond, amount(40.0), PrKeyframeEasing::Hold),
                key(
                    2900 * millisecond,
                    0.0,
                    PrKeyframeEasing::CubicBezier {
                        x1: 0.25,
                        y1: 0.1,
                        x2: 0.75,
                        y2: 0.9,
                    },
                ),
            ],
        ),
        exported_blur(true, 10.0, false),
    ];
    // Keys keep their times from the source In at every export rate, as
    // Motion keys do.
    for frame_rate in [FrameRate::Fps30, FrameRate::Fps24] {
        let (project, omissions) = export_at(wire.clone(), frame_rate);
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(exported_effects(&project), expected);
    }
}

#[test]
fn blurriness_keys_premiere_cannot_represent_omit_the_blur() {
    let linear = || json!({"type": "linear"});
    let bezier = json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9});
    for (keys, expected) in [
        (
            json!([
                fx_key("a", 0, 0.0, linear()),
                fx_key("b", 500, 5700.5, linear())
            ]),
            "blurriness key value 5700.5 is outside Premiere's 0 to 5700 range",
        ),
        (
            json!([
                fx_key("a", 0, 40.0, linear()),
                fx_key("b", 500, 40.0, bezier)
            ]),
            "blurriness cubic easing between equal values cannot preserve Premiere velocity",
        ),
    ] {
        let (project, omissions) = export(document_with_blurriness_keys(keys));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert!(
            omissions[0]
                .reason
                .starts_with("effects: gaussianBlur effect 1 was not exported: ")
                && omissions[0].reason.ends_with(expected),
            "{omissions:?}"
        );
    }
}

/// A document whose Corner Pin 1 has the FX `corners` in native order and the
/// keyed coordinates `tracks` (FX parameter, keys), above a static blur 2, on
/// source 2-3 s of its asset.
fn document_with_corner_pin(corners: [[f64; 2]; 4], tracks: &[(&str, Value)]) -> Value {
    let [[ulx, uly], [urx, ury], [llx, lly], [lrx, lry]] = corners;
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "cornerPin",
            "upperLeftX": ulx, "upperLeftY": uly, "upperRightX": urx, "upperRightY": ury,
            "lowerLeftX": llx, "lowerLeftY": lly, "lowerRightX": lrx, "lowerRightY": lry}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
    ]));
    wire["composition"]["layers"][0]["sourceRange"]["start"] = json!(2000);
    wire["composition"]["layers"][0]["playback"]["mapping"]["output"]["start"] = json!(2000);
    let entries: Vec<_> = tracks
        .iter()
        .map(|(param, keys)| {
            json!({
                "target": {"kind": "effectProperty", "effectId": 1, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": keys},
            })
        })
        .collect();
    wire["composition"]["dynamics"] = json!({ "entries": entries });
    wire
}

/// The effects of an exported project after writing it and reading it back.
fn reread_effects(mut project: PrProjectFile) -> Vec<PrEffect> {
    for media in project.media.values_mut() {
        media.name = "source.mp4".to_owned();
        media.relative_path = Some("./media/source.mp4".to_owned());
        media.relative_paths = vec!["./media/source.mp4".to_owned()];
        media.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            "/tmp/source.mp4".into(),
        )];
    }
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("project.prproj");
    crate::format::PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let xml = crate::format::read_xml(&path).unwrap();
    let (reread, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    exported_effects(&reread)
}

#[test]
fn edited_corner_pin_exports_its_corners_and_point_keys() {
    let (linear, hold) = (json!({"type": "linear"}), json!({"type": "hold"}));
    let bezier = json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9});
    // A spin of the upper corners. Upper Left's static 0.5:0.5 is not written:
    // a keyed corner starts at its first key. Its y stays 0 across the Bezier,
    // which moves the corner.
    let keys = |id: &str, values: [f64; 3], easings: [&Value; 3]| {
        json!([
            fx_key(&format!("{id}0"), 500, values[0], easings[0].clone()),
            fx_key(&format!("{id}1"), 700, values[1], easings[1].clone()),
            fx_key(&format!("{id}2"), 900, values[2], easings[2].clone()),
        ])
    };
    let spin = [&linear, &bezier, &hold];
    let wire = document_with_corner_pin(
        [[0.5, 0.5], [1.0, 0.0], [0.1, 1.0], [0.9, 1.0]],
        &[
            ("upperLeftX", keys("a", [0.0, -0.3, 0.0], spin)),
            ("upperLeftY", keys("b", [0.0, 0.0, 0.0], spin)),
            ("upperRightX", keys("c", [1.0, 1.3, 1.0], [&linear; 3])),
            ("upperRightY", keys("d", [0.0, 0.1, 0.0], [&linear; 3])),
        ],
    );
    let millisecond = TICKS_PER_MILLISECOND;
    let native_bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let (native_linear, native_hold) = (PrKeyframeEasing::Linear, PrKeyframeEasing::Hold);
    let expected = [
        corner_pin(
            true,
            [[0.0, 0.0], [1.0, 0.0], [0.1, 1.0], [0.9, 1.0]],
            vec![
                corner_keys(
                    0,
                    vec![
                        point_key(2500 * millisecond, [0.0, 0.0], native_linear),
                        point_key(2700 * millisecond, [-0.3, 0.0], native_bezier),
                        point_key(2900 * millisecond, [0.0, 0.0], native_hold),
                    ],
                ),
                corner_keys(
                    1,
                    vec![
                        point_key(2500 * millisecond, [1.0, 0.0], native_linear),
                        point_key(2700 * millisecond, [1.3, 0.1], native_linear),
                        point_key(2900 * millisecond, [1.0, 0.0], native_linear),
                    ],
                ),
            ],
        ),
        exported_blur(true, 10.0, false),
    ];
    // Keys keep their times from the source In at every export rate.
    for frame_rate in [FrameRate::Fps30, FrameRate::Fps24] {
        let (project, omissions) = export_at(wire.clone(), frame_rate);
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(exported_effects(&project), expected);
    }
}

#[test]
fn corner_keyed_on_one_axis_exports_its_static_other_coordinate() {
    let linear = || json!({"type": "linear"});
    // Upper Left keys only x and Upper Right only y; each corner keeps its
    // static other coordinate at every key.
    let wire = document_with_corner_pin(
        [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]],
        &[
            (
                "upperLeftX",
                json!([
                    fx_key("a", 0, 0.1, linear()),
                    fx_key("b", 500, -0.2, linear())
                ]),
            ),
            (
                "upperRightY",
                json!([
                    fx_key("c", 250, 0.0, linear()),
                    fx_key("d", 750, 0.2, linear())
                ]),
            ),
        ],
    );
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let millisecond = TICKS_PER_MILLISECOND;
    let linear = PrKeyframeEasing::Linear;
    let expected = vec![
        corner_pin(
            true,
            [[0.1, 0.05], [0.95, 0.0], [0.0, 1.0], [0.85, 0.9]],
            vec![
                corner_keys(
                    0,
                    vec![
                        point_key(2000 * millisecond, [0.1, 0.05], linear),
                        point_key(2500 * millisecond, [-0.2, 0.05], linear),
                    ],
                ),
                corner_keys(
                    1,
                    vec![
                        point_key(2250 * millisecond, [0.95, 0.0], linear),
                        point_key(2750 * millisecond, [0.95, 0.2], linear),
                    ],
                ),
            ],
        ),
        exported_blur(true, 10.0, false),
    ];
    assert_eq!(exported_effects(&project), expected);
    // The native point tracks read back unchanged.
    assert_eq!(reread_effects(project), expected);
}

#[test]
fn corner_tracks_premiere_cannot_represent_omit_the_corner_pin() {
    let (linear, hold) = (|| json!({"type": "linear"}), || json!({"type": "hold"}));
    let bezier = || json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9});
    let identity = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let two_keys = |id: &str, values: [f64; 2], times: [i64; 2], easing: Value| {
        json!([
            fx_key(&format!("{id}0"), times[0], values[0], linear()),
            fx_key(&format!("{id}1"), times[1], values[1], easing),
        ])
    };
    let one_point = "its Upper Left x and y keys differ in time or easing, but Premiere keys each point as one track";
    for (corners, tracks, expected) in [
        (
            identity,
            vec![
                ("upperLeftX", two_keys("a", [0.0, 0.2], [0, 500], linear())),
                ("upperLeftY", two_keys("b", [0.0, 0.2], [0, 600], linear())),
            ],
            one_point.to_owned(),
        ),
        (
            identity,
            vec![
                ("upperLeftX", two_keys("a", [0.0, 0.2], [0, 500], linear())),
                ("upperLeftY", two_keys("b", [0.0, 0.2], [0, 500], hold())),
            ],
            one_point.to_owned(),
        ),
        // Premiere measures Bezier speed along the corner's path.
        (
            identity,
            vec![
                ("upperLeftX", two_keys("a", [0.0, 0.0], [0, 500], bezier())),
                ("upperLeftY", two_keys("b", [0.0, 0.0], [0, 500], bezier())),
            ],
            "Upper Left cubic easing on a stationary point cannot preserve Premiere velocity"
                .to_owned(),
        ),
        // Upper Left moves past the diagonal to 0.9:0.9 at source 2.5 s.
        (
            identity,
            vec![
                ("upperLeftX", two_keys("a", [0.0, 0.9], [0, 500], linear())),
                ("upperLeftY", two_keys("b", [0.0, 0.9], [0, 500], linear())),
            ],
            "the corners form a degenerate or non-convex quad at the key at source time 2.500 s, which no perspective warp of the clip frame produces".to_owned(),
        ),
        // Lower Left and Lower Right swapped: the edges cross.
        (
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            Vec::new(),
            "the static corners form a degenerate or non-convex quad, which no perspective warp of the clip frame produces".to_owned(),
        ),
    ] {
        let (project, omissions) = export(document_with_corner_pin(corners, &tracks));
        assert_eq!(exported_effects(&project), [exported_blur(true, 10.0, false)]);
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".to_owned(),
                reason: format!("effects: cornerPin effect 1 was not exported: {expected}"),
            }]
        );
    }
}

/// Exports Corner Pin 1, the identity quad with the FX `animators`
/// (parameter, animator), between static blurs 3 above and 2 below. The pin
/// must be omitted as one Feature with exactly `reason`: both blurs keep their
/// stack order and no Corner Pin is written, not even one flattened to its
/// static corners.
fn assert_corner_animators_omit_the_corner_pin(animators: &[(&str, Value)], reason: &str) {
    let identity = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let mut wire = document_with_corner_pin(identity, &[]);
    wire["composition"]["layers"][0]["effects"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({"id": 3, "effect": {"type": "gaussianBlur", "blurriness": 5.0}}),
        );
    let entries: Vec<_> = animators
        .iter()
        .map(|(param, animator)| {
            json!({
                "target": {"kind": "effectProperty", "effectId": 1, "paramName": param},
                "animator": animator,
            })
        })
        .collect();
    wire["composition"]["dynamics"] = json!({ "entries": entries });
    let (project, omissions) = export(wire);
    assert_eq!(
        exported_effects(&project),
        [
            exported_blur(true, 5.0, false),
            exported_blur(true, 10.0, false)
        ]
    );
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".to_owned(),
            reason: format!("effects: cornerPin effect 1 was not exported: {reason}"),
        }]
    );
}

/// An enabled animator of two keys, `values[0]` at 0 ms and `values[1]` at
/// 500 ms, arriving at the second with `easing`.
fn corner_animator(id: &str, values: [f64; 2], easing: Value) -> Value {
    json!({"type": "keyframes", "enabled": true, "keyframes": [
        fx_key(&format!("{id}0"), 0, values[0], json!({"type": "linear"})),
        fx_key(&format!("{id}1"), 500, values[1], easing),
    ]})
}

/// `animator` switched off, holding `value` while disabled.
fn disabled_animator(mut animator: Value, value: f64) -> Value {
    animator["enabled"] = json!(false);
    animator["disabledValue"] = json!({"type": "float", "value": value});
    animator
}

const UNKEYED_UPPER_LEFT: &str = "Upper Left animation is disabled or not keyframed";

#[test]
fn disabled_corner_track_omits_the_corner_pin() {
    let linear = || json!({"type": "linear"});
    assert_corner_animators_omit_the_corner_pin(
        &[
            (
                "upperLeftX",
                disabled_animator(corner_animator("a", [0.0, 0.2], linear()), 0.1),
            ),
            ("upperLeftY", corner_animator("b", [0.0, 0.2], linear())),
        ],
        UNKEYED_UPPER_LEFT,
    );
}

#[test]
fn disabled_track_of_a_corner_keyed_on_one_axis_omits_the_corner_pin() {
    // Upper Left has no y track, so export would hold its static y at each key.
    assert_corner_animators_omit_the_corner_pin(
        &[(
            "upperLeftX",
            disabled_animator(
                corner_animator("a", [0.0, 0.2], json!({"type": "linear"})),
                0.1,
            ),
        )],
        UNKEYED_UPPER_LEFT,
    );
}

#[test]
fn constant_corner_animator_omits_the_corner_pin() {
    assert_corner_animators_omit_the_corner_pin(
        &[(
            "upperLeftX",
            json!({"type": "constant", "value": {"type": "float", "value": 0.2}}),
        )],
        UNKEYED_UPPER_LEFT,
    );
}

#[test]
fn non_float_corner_keys_omit_the_corner_pin() {
    // A homogeneous Vector2 track passes document validation: an effect
    // parameter target carries its key values unchecked.
    let point = |id: &str, layer_time: i64, value: [f64; 2]| {
        json!({"id": id, "layerTime": layer_time, "value": {"type": "vector2", "value": value},
            "easing": {"type": "linear"}})
    };
    assert_corner_animators_omit_the_corner_pin(
        &[(
            "upperLeftX",
            json!({"type": "keyframes", "enabled": true, "keyframes": [
                point("a0", 0, [0.0, 0.0]),
                point("a1", 500, [0.2, 0.1]),
            ]}),
        )],
        "Upper Left keys must have float values",
    );
}

#[test]
fn vertical_outgoing_corner_handle_omits_the_corner_pin() {
    // The outgoing handle rises with no time extent: an infinite speed out of
    // the first key.
    let vertical = || json!({"type": "cubicBezier", "x1": 0.0, "y1": 0.5, "x2": 0.75, "y2": 0.9});
    assert_corner_animators_omit_the_corner_pin(
        &[
            ("upperLeftX", corner_animator("a", [0.0, 0.2], vertical())),
            ("upperLeftY", corner_animator("b", [0.0, 0.2], vertical())),
        ],
        "unsupported conversion: vertical outgoing cubic timing handle cannot be represented by Premiere velocity/influence",
    );
}

#[test]
fn vertical_incoming_corner_handle_omits_the_corner_pin() {
    // The incoming handle rises with no time extent: an infinite speed into
    // the second key.
    let vertical = || json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 1.0, "y2": 0.5});
    assert_corner_animators_omit_the_corner_pin(
        &[
            ("upperLeftX", corner_animator("a", [0.0, 0.2], vertical())),
            ("upperLeftY", corner_animator("b", [0.0, 0.2], vertical())),
        ],
        "unsupported conversion: vertical incoming cubic timing handle cannot be represented by Premiere velocity/influence",
    );
}

fn linear_easing() -> Value {
    json!({"type": "linear"})
}

fn shared_bezier_easing() -> Value {
    json!({"type": "cubicBezier", "x1": 0.3, "y1": 0.0, "x2": 0.6, "y2": 1.0})
}

#[test]
fn corner_tracks_that_degenerate_between_keys_omit_the_corner_pin() {
    let between = "the corners form a degenerate or non-convex quad between the keys at source times 2.000 s and 2.500 s, which no perspective warp of the clip frame produces";
    // Each corner keyed to the opposite one: the quad is the square at both
    // keys, but all four corners meet at 0.5:0.5 halfway, Linear and with one
    // Bezier easing that the four share.
    for easing in [linear_easing, shared_bezier_easing] {
        assert_corner_animators_omit_the_corner_pin(
            &[
                ("upperLeftX", corner_animator("a", [0.0, 1.0], easing())),
                ("upperLeftY", corner_animator("b", [0.0, 1.0], easing())),
                ("upperRightX", corner_animator("c", [1.0, 0.0], easing())),
                ("upperRightY", corner_animator("d", [0.0, 1.0], easing())),
                ("lowerLeftX", corner_animator("e", [0.0, 1.0], easing())),
                ("lowerLeftY", corner_animator("f", [1.0, 0.0], easing())),
                ("lowerRightX", corner_animator("g", [1.0, 0.0], easing())),
                ("lowerRightY", corner_animator("h", [1.0, 0.0], easing())),
            ],
            between,
        );
    }
    // Each corner keyed Linear to its mirror image, which turns the other
    // way: the quad folds flat halfway, where every corner has x 0.5. No turn
    // has a vertex between the keys, so only the orientation check catches it.
    let mirrored = [
        ("upperLeftX", "a", [0.0, 1.0]),
        ("upperRightX", "c", [1.0, 0.0]),
        ("lowerLeftX", "e", [0.0, 1.0]),
        ("lowerRightX", "g", [1.0, 0.0]),
    ]
    .map(|(param, id, values)| (param, corner_animator(id, values, linear_easing())));
    assert_corner_animators_omit_the_corner_pin(&mirrored, between);
    // Upper Left eases to 0.4:0.4 but overshoots it to about 0.54:0.54, past
    // the diagonal from Upper Right to Lower Left, before it settles.
    let overshoot = || json!({"type": "cubicBezier", "x1": 0.2, "y1": 0.0, "x2": 0.6, "y2": 2.2});
    assert_corner_animators_omit_the_corner_pin(
        &[
            ("upperLeftX", corner_animator("a", [0.0, 0.4], overshoot())),
            ("upperLeftY", corner_animator("b", [0.0, 0.4], overshoot())),
        ],
        between,
    );
    // Upper Left 0:0 to 1:0 and Upper Right 1:0 to 2:0 keep a convex quad at
    // both keys. With different easings each corner's progress is bounded on
    // its own, which admits Upper Left at 1:0 while Upper Right is still
    // there, so the quad cannot be proven convex.
    let other_bezier = json!({"type": "cubicBezier", "x1": 0.5, "y1": 0.0, "x2": 0.8, "y2": 1.0});
    for (upper_left, upper_right) in [
        (shared_bezier_easing(), other_bezier),
        (linear_easing(), shared_bezier_easing()),
    ] {
        assert_corner_animators_omit_the_corner_pin(
            &[
                ("upperLeftX", corner_animator("a", [0.0, 1.0], upper_left)),
                ("upperRightX", corner_animator("c", [1.0, 2.0], upper_right)),
            ],
            "Upper Left and Upper Right move with different easings between the keys at source times 2.000 s and 2.500 s, so their quad there cannot be proven convex",
        );
    }
}

#[test]
fn corner_tracks_that_stay_convex_between_keys_export() {
    let hold = || json!({"type": "hold"});
    let identity = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let two_keys = |id: &str, values: [f64; 2], easing: Value| {
        json!([
            fx_key(&format!("{id}0"), 0, values[0], linear_easing()),
            fx_key(&format!("{id}1"), 500, values[1], easing),
        ])
    };
    // Each corner's x from `values[0]` to `values[1]`, its y static.
    let horizontal = |values: [[f64; 2]; 4], easing: fn() -> Value| {
        vec![
            ("upperLeftX", two_keys("a", values[0], easing())),
            ("upperRightX", two_keys("c", values[1], easing())),
            ("lowerLeftX", two_keys("e", values[2], easing())),
            ("lowerRightX", two_keys("g", values[3], easing())),
        ]
    };
    let translated = [[0.0, 2.0], [1.0, 3.0], [0.0, 2.0], [1.0, 3.0]];
    let mirrored = [[0.0, 1.0], [1.0, 0.0], [0.0, 1.0], [1.0, 0.0]];
    for (tracks, keyed) in [
        // The square translated by 2:0, Linear and with one Bezier easing that
        // the four corners share.
        (horizontal(translated, linear_easing), 4),
        (horizontal(translated, shared_bezier_easing), 4),
        // Every corner holds, then jumps to the mirrored square: no quad in
        // between is degenerate.
        (horizontal(mirrored, hold), 4),
        // Upper Left eases to 0.4:0.4 without overshooting it.
        (
            vec![
                (
                    "upperLeftX",
                    two_keys("a", [0.0, 0.4], shared_bezier_easing()),
                ),
                (
                    "upperLeftY",
                    two_keys("b", [0.0, 0.4], shared_bezier_easing()),
                ),
            ],
            1,
        ),
    ] {
        let (project, omissions) = export(document_with_corner_pin(identity, &tracks));
        assert!(omissions.is_empty(), "{tracks:?}: {omissions:?}");
        let effects = exported_effects(&project);
        assert_eq!(effects.len(), 2, "{effects:?}");
        assert_eq!(effects[0].animations.len(), keyed, "{effects:?}");
    }
}

#[test]
fn keyed_corner_pin_with_keyed_opacity_and_scale_imports_and_exports_back() {
    use crate::schema::{PrAnimatedProperty, PrPropertyAnimation};
    use PrKeyframeEasing::Linear;
    // As on corpus Corner Pins: keyed Opacity (two of three) and keyed uniform
    // Scale (one of three) on the clip of a keyed Corner Pin.
    let fade = vec![key(0, 0.0, Linear), key(TICKS, 100.0, Linear)];
    let zoom = vec![key(0, 70.0, Linear), key(TICKS, 100.0, Linear)];
    let spin = corner_pin(
        true,
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        vec![corner_keys(
            0,
            vec![
                point_key(0, [0.0, 0.0], Linear),
                point_key(TICKS, [-0.3, 0.0], Linear),
            ],
        )],
    );
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.animations = vec![
        PrPropertyAnimation::Opacity(fade.clone()),
        PrPropertyAnimation::UniformScale(zoom.clone()),
    ];
    clip.effects = vec![spin.clone()];
    let wire = project_document(&sequence);
    // Every track imports on the layer clock.
    let tracks: BTreeMap<String, Vec<(i64, f64)>> = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let target = &entry["target"];
            let name = target["propertyType"]
                .as_str()
                .or(target["paramName"].as_str())
                .unwrap();
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
            (name.to_owned(), keys)
        })
        .collect();
    assert_eq!(
        tracks,
        BTreeMap::from([
            ("opacity".to_owned(), vec![(0, 0.0), (1000, 100.0)]),
            ("scaleX".to_owned(), vec![(0, 70.0), (1000, 100.0)]),
            ("scaleY".to_owned(), vec![(0, 70.0), (1000, 100.0)]),
            ("upperLeftX".to_owned(), vec![(0, 0.0), (1000, -0.3)]),
            ("upperLeftY".to_owned(), vec![(0, 0.0), (1000, 0.0)]),
        ])
    );
    // The imported document exports every key back.
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    let animations: Vec<_> = clip
        .animations
        .iter()
        .map(|animation| {
            (
                animation.property(),
                animation.scalar_keys().unwrap().to_vec(),
            )
        })
        .collect();
    assert_eq!(
        animations,
        [
            (PrAnimatedProperty::Opacity, fade),
            (PrAnimatedProperty::UniformScale, zoom)
        ]
    );
    assert_eq!(clip.effects, [spin]);
}

/// A document whose Directional Blur 1 has the FX `direction` and
/// `blur_length` and the keyed parameters `tracks` (FX parameter, keys),
/// above a static Gaussian Blur 2, on source 2-3 s of its asset, on a layer
/// with Scale 50 and Rotation 30.
fn document_with_directional_blur(
    direction: f64,
    blur_length: f64,
    tracks: &[(&str, Value)],
) -> Value {
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "directionalBlur", "direction": direction, "blurLength": blur_length}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
    ]));
    let layer = &mut wire["composition"]["layers"][0];
    layer["sourceRange"]["start"] = json!(2000);
    layer["playback"]["mapping"]["output"]["start"] = json!(2000);
    layer["transform"]["scale"] = json!([50.0, 50.0]);
    layer["transform"]["rotation"] = json!(30.0);
    let entries: Vec<_> = tracks
        .iter()
        .map(|(param, keys)| {
            json!({
                "target": {"kind": "effectProperty", "effectId": 1, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": keys},
            })
        })
        .collect();
    wire["composition"]["dynamics"] = json!({ "entries": entries });
    wire
}

#[test]
fn edited_directional_blur_exports_native_values_in_the_clip_frame() {
    use PrKeyframeEasing::{Hold, Linear};
    let linear = || json!({"type": "linear"});
    // On Scale 50 and Rotation 30, 15 composition pixels along 75 degrees are
    // 30 clip pixels along 45 degrees in the clip's frame; keys map the same
    // way, in native order whatever the order of the FX tracks, and a keyed
    // parameter's static value is its first key's.
    let mut wire = document_with_directional_blur(
        75.0,
        15.0,
        &[
            (
                "blurLength",
                json!([
                    fx_key("a", 500, 20.0, linear()),
                    fx_key("b", 700, 5.0, json!({"type": "hold"}))
                ]),
            ),
            (
                "direction",
                json!([
                    fx_key("c", 500, 30.0, linear()),
                    fx_key("d", 900, 120.0, linear())
                ]),
            ),
        ],
    );
    wire["composition"]["layers"][0]["effects"][0]["enabled"] = json!(false);
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let millisecond = TICKS_PER_MILLISECOND;
    assert_eq!(
        exported_effects(&project),
        [
            current_blur_export(keyed_directional(
                directional_blur(false, 0.0, 0.0),
                vec![
                    key(2500 * millisecond, 0.0, Linear),
                    key(2900 * millisecond, 90.0, Linear),
                ],
                vec![
                    key(2500 * millisecond, 40.0, Linear),
                    key(2700 * millisecond, 10.0, Hold),
                ],
            )),
            exported_blur(true, 10.0, false),
        ]
    );
}

/// A keyframe track of `property` on layer 1 from 10 to 30.
fn layer_track(property: &str) -> Value {
    json!({
        "target": {"kind": "layer", "layerId": 1, "propertyType": property},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            fx_key(&format!("{property}-0"), 0, 10.0, json!({"type": "linear"})),
            fx_key(&format!("{property}-1"), 500, 30.0, json!({"type": "linear"})),
        ]},
    })
}

#[test]
fn directional_blur_exports_only_on_a_host_whose_motion_is_a_static_similarity() {
    // The Motion export's own report of a layer track it drops.
    let dropped = "only independent Opacity, paired Position, Rotation, uniform Scale, audio volume and text Source Text keyframes can be exported; animation was omitted";
    // (fields of the layer's static transform over Scale 50 and Rotation 30,
    // animated layer properties, the Motion export's report, the form that
    // omits the blur or "" when it exports as 45/30 in the clip's frame)
    #[rustfmt::skip]
    let hosts = [
        // Motion keys scale and rotation, which the blur's static map cannot
        // follow, and drops the other tracks.
        (json!({}), vec!["scaleX", "scaleY"], "", "its layer has animated scale"),
        (json!({}), vec!["rotation"], "", "its layer has animated rotation"),
        (json!({}), vec!["skew"], dropped, "its layer has animated skew"),
        (json!({}), vec!["rotationX"], dropped, "its layer has animated X rotation"),
        (json!({}), vec!["rotationY"], dropped, "its layer has animated Y rotation"),
        (json!({}), vec!["orientationX"], dropped, "its layer has animated orientation"),
        (json!({}), vec!["orientationY"], dropped, "its layer has animated orientation"),
        (json!({}), vec!["orientationZ"], dropped, "its layer has animated orientation"),
        // Only a 3D layer animates its z position.
        (json!({"position": [0.0, 0.0, 0.0]}), vec!["positionZ"], dropped, "its layer has animated z position"),
        // Static forms that Motion cannot write.
        (json!({"skew": 10.0}), vec![], "skew was not exported", "its layer is skewed"),
        (json!({"rotationX": 20.0}), vec![], "3D rotation was not exported", "its layer has 3D rotation"),
        (json!({"position": [0.0, 0.0, 250.0]}), vec![], "", "its layer has z position 250"),
        (json!({"scale": [50.0, 80.0]}), vec![], "", "its layer's static scale is nonuniform (50% by 80%)"),
        (json!({"scale": [0.0, 0.0]}), vec![], "", "its layer's static scale 0% is not positive"),
        // Moving, fading, a skew axis without skew and a 3D position at z 0
        // leave Scale 50 and Rotation 30 the layer's only turn and scale.
        (json!({}), vec!["positionX", "positionY"], "", ""),
        (json!({}), vec!["opacity"], "", ""),
        (json!({}), vec!["anchorPointX", "anchorPointY"], "", ""),
        (json!({}), vec!["skewAxis"], dropped, ""),
        (json!({"skewAxis": 30.0}), vec![], "skew was not exported", ""),
        (json!({"position": [0.0, 0.0, 0.0]}), vec![], "", ""),
    ];
    for (transform, animated, motion, form) in hosts {
        let mut wire = document_with_directional_blur(75.0, 15.0, &[]);
        for (field, value) in transform.as_object().unwrap() {
            wire["composition"]["layers"][0]["transform"][field] = value.clone();
        }
        let tracks: Vec<_> = animated
            .iter()
            .map(|property| layer_track(property))
            .collect();
        wire["composition"]["dynamics"] = json!({ "entries": tracks });
        let (project, omissions) = export(wire);
        let case = format!("{transform} {animated:?}");
        let kept = form
            .is_empty()
            .then(|| current_blur_export(directional_blur(true, 45.0, 30.0)));
        let effects: Vec<_> = kept
            .into_iter()
            .chain([exported_blur(true, 10.0, false)])
            .collect();
        assert_eq!(exported_effects(&project), effects, "{case}");
        // Motion reports a dropped track on its layer and a static value on
        // the clip; `omit` reports identical drops once.
        let omission = |record: &str, reason: String| Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: record.to_owned(),
            reason,
        };
        let clip = "layer 1 (\"Source\")";
        let motion = (!motion.is_empty()).then(|| {
            omission(
                if motion == dropped { "layer 1" } else { clip },
                motion.to_owned(),
            )
        });
        // Motion also reports a nonzero or animated z position (F28).
        let z_position = (transform["position"][2].as_f64().is_some_and(|z| z != 0.0)
            || animated.contains(&"positionZ"))
        .then(|| omission(clip, "3D position was not exported".to_owned()));
        let blur_omission = (!form.is_empty()).then(|| {
            let reason = format!(
                "effects: directionalBlur effect 1 was not exported: {form}; {}",
                super::SIMILARITY_RULE
            );
            omission(clip, reason)
        });
        let expected: Vec<_> = motion
            .into_iter()
            .chain(z_position)
            .chain(blur_omission)
            .collect();
        assert_eq!(omissions, expected, "{case}");
    }
}

#[test]
fn directional_blur_on_a_child_of_an_identity_group_exports_in_the_inner_sequence() {
    // Export writes a group only as an identity nest, so the child's own
    // Scale 50 and Rotation 30 remain its whole map into the composition.
    let mut wire = document_with_directional_blur(75.0, 15.0, &[]);
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    let mut video = layers.remove(0);
    video["parent"] = json!(10);
    layers.insert(
        0,
        json!({
            "type": "Group", "id": 10, "name": "Inner", "blendMode": "normal",
            "playback": crate::test_support::linear_playback(json!(*crate::test_support::layer_range(&video)), json!({"start": 0, "duration": (*crate::test_support::layer_range(&video))["duration"]})), "transform": layers[0]["transform"],
            "layers": [video]
        }),
    );
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.video_occurrences().count(), 0);
    let inner = &outer.nest_occurrences().next().unwrap().sequence;
    assert_eq!(
        inner.video_occurrences().next().unwrap().effects,
        [
            current_blur_export(directional_blur(true, 45.0, 30.0)),
            exported_blur(true, 10.0, false)
        ]
    );
}

/// The one-clip sequence whose clip has a Directional Blur (Direction 0, Blur
/// Length 30) and a Gaussian Blur (10) with a Crop (Top 15) or, with `wipe`, a
/// keyed 270-degree Linear Wipe, on Scale 50 and Rotation 30. With `staged` both
/// blurs apply before the mask, so the clip stages; otherwise they apply after
/// a Crop, which stays on one video layer.
fn masked_directional_blur_sequence(wipe: bool, staged: bool) -> crate::format::PrSequence {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    if wipe {
        clip.linear_wipe = Some(crate::schema::PrLinearWipe {
            initial_completion: 100.0,
            completion: vec![
                key(0, 100.0, PrKeyframeEasing::Linear),
                key(TICKS, 0.0, PrKeyframeEasing::Linear),
            ],
            angle_degrees: 270,
            feather: 0.0,
        });
    } else {
        clip.crop.top = 15.0;
    }
    (clip.transform.scale, clip.transform.rotation) = ([50.0; 2], 30.0);
    clip.effects = vec![directional_blur(true, 0.0, 30.0), blur(true, 10.0, false)];
    clip.effects_above_mask = if staged { 2 } else { 0 };
    sequence
}

#[test]
fn directional_blur_on_a_stage_group_is_omitted_on_import() {
    // (wipe instead of Crop, staged): a staged Crop, a staged wipe, a flat Crop.
    for (wipe, staged) in [(false, true), (true, true), (false, false)] {
        let sequence = masked_directional_blur_sequence(wipe, staged);
        let media = crate::tests::support::video_media();
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        let case = format!("wipe {wipe}, staged {staged}");
        let top = &wire["composition"]["layers"][0];
        let gaussian = |id: u64| json!({"id": id, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}});
        if staged {
            // The group keeps the clip's Motion and mask and the video its
            // Gaussian Blur; only the Directional Blur is omitted.
            assert_eq!(top["type"], "Group", "{case}");
            assert_eq!(top["transform"]["rotation"], json!(30.0), "{case}");
            let [video, guide] = top["layers"].as_array().unwrap().as_slice() else {
                panic!("{case}: expected the video and its guide");
            };
            assert_eq!(top["masks"][0]["layer"], guide["id"], "{case}");
            assert_eq!(video["effects"], json!([gaussian(1)]), "{case}");
            assert_eq!(
                omissions,
                [Omission {
                    scope: OmissionScope::Feature,
                    kind: OmissionKind::Omitted,
                    record: "source".to_owned(),
                    reason: format!(
                        "Directional Blur effect at stack position 1 was not imported: {}",
                        super::STAGED_DIRECTIONAL_BLUR_REASON
                    ),
                }],
                "{case}"
            );
        } else {
            // A flat clip maps the blur through its video's Motion as before.
            assert_eq!(top["type"], "Video", "{case}");
            let directional = json!({"id": 1, "enabled": true, "effect": {"type": "directionalBlur", "direction": 30.0, "blurLength": 15.0}});
            assert_eq!(top["effects"], json!([directional, gaussian(2)]), "{case}");
            assert!(omissions.is_empty(), "{case}: {omissions:?}");
        }
    }
}

#[test]
fn directional_blur_on_a_stage_group_is_omitted_on_export() {
    // (wipe instead of Crop, staged): a stage group with a Crop, one with a
    // wipe, and a flat video with a Crop, each at Scale 50 and Rotation 30.
    for (wipe, staged) in [(false, true), (true, true), (false, false)] {
        let mut wire = project_document(&masked_directional_blur_sequence(wipe, staged));
        let top = &mut wire["composition"]["layers"][0];
        let record = format!("layer {} ({})", top["id"], top["name"]);
        // Each video carries the same FX blurs: 15 composition pixels along 30
        // degrees, then the Gaussian Blur.
        let video = if staged { &mut top["layers"][0] } else { top };
        video["effects"] = json!([
            {"id": 7, "enabled": true, "effect": {"type": "directionalBlur", "direction": 30.0, "blurLength": 15.0}},
            {"id": 8, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        ]);
        let (project, omissions) = export(wire);
        let case = format!("wipe {wipe}, staged {staged}");
        let clip = crate::tests::support::first_clip(&project);
        assert_eq!(
            (clip.transform.scale, clip.transform.rotation),
            ([50.0; 2], 30.0),
            "{case}"
        );
        assert_eq!(
            (clip.crop.top, clip.linear_wipe.is_some()),
            if wipe { (0.0, true) } else { (15.0, false) },
            "{case}"
        );
        if staged {
            // One clip with the group's Motion and mask, the Gaussian Blur
            // applied before the mask; only the Directional Blur is omitted.
            assert_eq!(clip.effects, [exported_blur(true, 10.0, false)], "{case}");
            assert_eq!(clip.effects_above_mask, 1, "{case}");
            assert_eq!(
                omissions,
                [Omission {
                    scope: OmissionScope::Feature,
                    kind: OmissionKind::Omitted,
                    record,
                    reason: format!(
                        "effects: directionalBlur effect 7 was not exported: {}",
                        super::STAGED_DIRECTIONAL_BLUR_REASON
                    ),
                }],
                "{case}"
            );
        } else {
            // A flat clip maps the blur back through its Motion as before.
            assert_eq!(
                clip.effects,
                [
                    current_blur_export(directional_blur(true, 0.0, 30.0)),
                    exported_blur(true, 10.0, false)
                ],
                "{case}"
            );
            assert_eq!(clip.effects_above_mask, 0, "{case}");
            assert!(omissions.is_empty(), "{case}: {omissions:?}");
        }
    }
}

#[test]
fn directional_blur_values_outside_premiere_range_in_the_clip_frame_are_not_exported() {
    let linear = || json!({"type": "linear"});
    let keys = json!([
        fx_key("a", 0, 15.0, linear()),
        fx_key("b", 500, 801.0, linear())
    ]);
    // (FX direction and blur length, keyed parameters, the reason). On Scale
    // 50, 900 composition pixels are 1800 clip pixels and a key of 801 is
    // 1602, above Amount 1000 (Blur Length 1600); Rotation 30 turns -32750 to
    // -32780 in the clip's frame. The reasons name the FX values.
    #[rustfmt::skip]
    let blurs = [
        (75.0, 900.0, vec![], "blurLength 1800 is outside Premiere's 0 to 1600 range"),
        (75.0, 15.0, vec![("blurLength", keys)], "blurLength key value 1602 is outside Premiere's 0 to 1600 range"),
        (-32750.0, 15.0, vec![], "Angle -32780 is outside Premiere's -32768 to 32767 range"),
    ];
    for (direction, blur_length, tracks, reason) in blurs {
        let (project, omissions) = export(document_with_directional_blur(
            direction,
            blur_length,
            &tracks,
        ));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".to_owned(),
                reason: format!("effects: directionalBlur effect 1 was not exported: {reason}"),
            }]
        );
    }
    // 800 composition pixels are 1600 clip pixels, Amount 1000: above the
    // Legacy Blur Length range, within the current blur's.
    let (project, omissions) = export(document_with_directional_blur(75.0, 800.0, &[]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        exported_effects(&project)[0].params,
        PrEffectParams::FilmImpactDirectionalBlur(PrFilmImpactDirectionalBlur {
            angle: 45.0,
            amount: 1000.0,
        })
    );
}

#[test]
fn brightness_contrast_values_outside_premiere_range_are_not_exported() {
    let linear = || json!({"type": "linear"});
    // (FX brightness and contrast, the keyed parameter and its second key,
    // the reason). FX allows a brightness of up to 150.
    #[rustfmt::skip]
    let effects = [
        (150.0, 0.0, None, "Brightness 150 is outside Premiere's -100 to 100 range"),
        (0.0, -100.5, None, "Contrast -100.5 is outside Premiere's -100 to 100 range"),
        (0.0, 0.0, Some(("brightness", 120.0)), "Brightness key value 120 is outside Premiere's -100 to 100 range"),
        (0.0, 0.0, Some(("contrast", -101.0)), "Contrast key value -101 is outside Premiere's -100 to 100 range"),
    ];
    for (brightness, contrast, keyed, reason) in effects {
        let mut wire = document_with_effects(json!([
            {"id": 1, "effect": {"type": "brightnessContrast", "brightness": brightness, "contrast": contrast}},
            {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        ]));
        if let Some((param, value)) = keyed {
            wire["composition"]["dynamics"] = json!({"entries": [{
                "target": {"kind": "effectProperty", "effectId": 1, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                    fx_key("a", 0, 0.0, linear()),
                    fx_key("b", 500, value, linear()),
                ]},
            }]});
        }
        let (project, omissions) = export(wire);
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".to_owned(),
                reason: format!("effects: brightnessContrast effect 1 was not exported: {reason}"),
            }]
        );
    }
}

/// Export input: a Gaussian Blur under a Levels (id 2) with the FX `values`
/// in FX order (input black, input white, gamma, output black, output white),
/// `enabled` or not, and one keyframe track per `(parameter, keys)` on it.
fn document_with_levels(enabled: bool, values: [f64; 5], tracks: &[(&str, Value)]) -> Value {
    let [input_black, input_white, gamma, output_black, output_white] = values;
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 2, "enabled": enabled, "effect": {"type": "levels", "inputBlack": input_black,
            "inputWhite": input_white, "gamma": gamma, "outputBlack": output_black, "outputWhite": output_white}},
    ]));
    let entries: Vec<_> = tracks
        .iter()
        .map(|(param, keys)| {
            json!({
                "target": {"kind": "effectProperty", "effectId": 2, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": keys},
            })
        })
        .collect();
    wire["composition"]["dynamics"] = json!({"entries": entries});
    wire
}

#[test]
fn levels_in_invert_form_export_as_invert_and_others_as_levels() {
    use PrKeyframeEasing::{Hold, Linear};
    let linear = || json!({"type": "linear"});
    let hold = || json!({"type": "hold"});
    // Output white and black keys of a Blend of 100, then `second` held: 255
    // and 0, then the values given.
    let white = |second: f64, easing: Value| {
        json!([
            fx_key("a", 0, 255.0, linear()),
            fx_key("b", 500, second, easing)
        ])
    };
    let black = |millis: i64, second: f64, easing: Value| {
        json!([
            fx_key("c", 0, 0.0, linear()),
            fx_key("d", millis, second, easing)
        ])
    };
    let identity = [0.0, 255.0, 1.0, 0.0, 255.0];
    let millisecond = TICKS_PER_MILLISECOND;
    // (enabled, FX values, tracks, the Invert it exports as).
    #[rustfmt::skip]
    let inverts = [
        (true, [0.0, 255.0, 1.0, 255.0, 0.0], vec![], invert(true, 0.0, Vec::new())),
        (true, [0.0, 255.0, 1.0, 178.5, 76.5], vec![], invert(true, 30.0, Vec::new())),
        (true, [0.0, 255.0, 1.0, 76.5, 178.5], vec![], invert(true, 70.0, Vec::new())),
        // A bypassed Invert, where a disabled Levels of another form is omitted.
        (false, [0.0, 255.0, 1.0, 178.5, 76.5], vec![], invert(false, 30.0, Vec::new())),
        // Keyed complements: a Blend of 100 held at 60 from 0.5 s.
        (true, identity, vec![("outputWhite", white(153.0, hold())), ("outputBlack", black(500, 102.0, hold()))], invert(true, 100.0, vec![key(0, 100.0, Linear), key(500 * millisecond, 60.0, Hold)])),
    ];
    for (enabled, values, tracks, expected) in inverts {
        let (project, omissions) = export(document_with_levels(enabled, values, &tracks));
        assert!(omissions.is_empty(), "{values:?}: {omissions:?}");
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false), expected]
        );
    }
    // Any other Levels keeps its own export, whole native values and the
    // tracks in document order included: (FX values, tracks, the Levels it
    // exports as).
    let white_keys = |second: f64, easing| {
        levels_keys(
            3,
            vec![
                key(0, 255.0, Linear),
                key(500 * millisecond, second, easing),
            ],
        )
    };
    let black_keys = |millis: i64, second: f64, easing| {
        levels_keys(
            2,
            vec![
                key(0, 0.0, Linear),
                key(millis * millisecond, second, easing),
            ],
        )
    };
    #[rustfmt::skip]
    let others = [
        // The identity: an Invert with a Blend of 100 shows the original.
        (identity, vec![], levels([0.0, 255.0, 0.0, 255.0, 100.0], Vec::new())),
        // Outputs that are not complements.
        ([0.0, 255.0, 1.0, 76.5, 178.4], vec![], levels([0.0, 255.0, 77.0, 178.0, 100.0], Vec::new())),
        // One keyed output, complementary keys with a keyed Gamma, and keys
        // that differ in time, easing or value.
        (identity, vec![("outputWhite", white(153.0, hold()))], levels([0.0, 255.0, 0.0, 255.0, 100.0], vec![white_keys(153.0, Hold)])),
        (identity, vec![("outputWhite", white(153.0, hold())), ("outputBlack", black(500, 102.0, hold())), ("gamma", json!([fx_key("e", 0, 1.0, linear()), fx_key("f", 500, 1.5, linear())]))],
            levels([0.0, 255.0, 0.0, 255.0, 100.0], vec![white_keys(153.0, Hold), black_keys(500, 102.0, Hold), levels_keys(4, vec![key(0, 100.0, Linear), key(500 * millisecond, 150.0, Linear)])])),
        (identity, vec![("outputWhite", white(153.0, hold())), ("outputBlack", black(600, 102.0, hold()))], levels([0.0, 255.0, 0.0, 255.0, 100.0], vec![white_keys(153.0, Hold), black_keys(600, 102.0, Hold)])),
        (identity, vec![("outputWhite", white(153.0, hold())), ("outputBlack", black(500, 102.0, linear()))], levels([0.0, 255.0, 0.0, 255.0, 100.0], vec![white_keys(153.0, Hold), black_keys(500, 102.0, Linear)])),
        (identity, vec![("outputWhite", white(153.0, hold())), ("outputBlack", black(500, 101.0, hold()))], levels([0.0, 255.0, 0.0, 255.0, 100.0], vec![white_keys(153.0, Hold), black_keys(500, 101.0, Hold)])),
    ];
    for (values, tracks, expected) in others {
        let (project, omissions) = export(document_with_levels(true, values, &tracks));
        assert!(omissions.is_empty(), "{values:?}: {omissions:?}");
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false), expected]
        );
    }
    // An inverted Levels of another form is one that no Adobe render measured,
    // so it is omitted rather than written as an Invert.
    for values in [
        [3.0, 255.0, 1.0, 255.0, 0.0],
        [0.0, 255.0, 1.5, 255.0, 0.0],
        [0.0, 255.0, 1.0, 178.5, 76.4],
    ] {
        let (project, omissions) = export(document_with_levels(true, values, &[]));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert_eq!(omissions.len(), 1, "{values:?}: {omissions:?}");
        assert!(
            omissions[0]
                .reason
                .ends_with("a Levels form that no Adobe render measured"),
            "{values:?}: {}",
            omissions[0].reason
        );
    }
}

#[test]
fn invert_blends_outside_premiere_range_are_not_exported() {
    let linear = || json!({"type": "linear"});
    // Complementary outputs beyond 255 map to a Blend beyond 100, which is
    // never clamped: (FX values, tracks, the reason).
    #[rustfmt::skip]
    let levels = [
        ([0.0, 255.0, 1.0, -45.0, 300.0], vec![], "Blend With Original 117.6470588235294 is outside Premiere's 0 to 100 range"),
        ([0.0, 255.0, 1.0, 255.0, 0.0], vec![
            ("outputWhite", json!([fx_key("a", 0, 0.0, linear()), fx_key("b", 500, 300.0, linear())])),
            ("outputBlack", json!([fx_key("c", 0, 255.0, linear()), fx_key("d", 500, -45.0, linear())])),
        ], "Blend With Original key value 117.6470588235294 is outside Premiere's 0 to 100 range"),
    ];
    for (values, tracks, reason) in levels {
        let (project, omissions) = export(document_with_levels(true, values, &tracks));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".to_owned(),
                reason: format!("effects: levels effect 2 was not exported: {reason}"),
            }]
        );
    }
}

/// A Tint with the static `black`, `white` and `amount` and the keys of its
/// parameters in native order, possibly empty. A keyed static value is its
/// first key's.
fn tint(
    enabled: bool,
    (black, white, amount): ([u8; 3], [u8; 3], f64),
    (black_keys, white_keys, amount_keys): (
        Vec<PrColourKeyframe>,
        Vec<PrColourKeyframe>,
        Vec<PrScalarKeyframe>,
    ),
) -> PrEffect {
    let mut animations = Vec::new();
    for (param, keys) in [
        (&TINT_MAP_BLACK_TO, black_keys),
        (&TINT_MAP_WHITE_TO, white_keys),
    ] {
        if !keys.is_empty() {
            animations.push(PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Colour(keys),
            });
        }
    }
    if !amount_keys.is_empty() {
        animations.push(PrEffectParamAnimation {
            param: &TINT_AMOUNT,
            keys: PrEffectParamKeys::Scalar(amount_keys),
        });
    }
    PrEffect {
        enabled,
        params: PrEffectParams::Tint(PrTint {
            black: PrColour { rgb: black },
            white: PrColour { rgb: white },
            amount,
        }),
        animations,
    }
}

fn colour_key(source_ticks: i64, rgb: [u8; 3], easing: PrKeyframeEasing) -> PrColourKeyframe {
    PrColourKeyframe {
        source_ticks,
        value: PrColour { rgb },
        easing,
    }
}

/// The FX `tintTritone` of a Tint: each channel a share of 255.
fn tint_tritone(id: u64, enabled: bool, (black, white, amount): ([u8; 3], [u8; 3], f64)) -> Value {
    let share = |channel: u8| f64::from(channel) / 255.0;
    json!({"id": id, "enabled": enabled, "effect": {"type": "tintTritone",
        "blackR": share(black[0]), "blackG": share(black[1]), "blackB": share(black[2]),
        "whiteR": share(white[0]), "whiteG": share(white[1]), "whiteB": share(white[2]),
        "amount": amount}})
}

#[test]
fn keyed_tint_imports_as_channel_and_amount_tracks_and_exports_back() {
    use PrKeyframeEasing::{Hold, Linear};
    let bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let (black, white, orange, blue) = ([0, 0, 0], [255, 255, 255], [255, 128, 0], [0, 128, 255]);
    // Source In 1 s: keys before, inside and after the trimmed range stay.
    // The keyed Tint (Map White To Linear then Hold, Amount with a Bezier)
    // sits above a Gaussian Blur, a static Tint with the effect_stack colours,
    // a Black & White, a half Tint and a bypassed one.
    #[rustfmt::skip]
    let native = vec![
        tint(true, (black, white, 0.0), (vec![], vec![colour_key(TICKS / 2, white, Linear), colour_key(2 * TICKS, blue, Linear), colour_key(7 * TICKS, orange, Hold)], vec![key(TICKS / 2, 0.0, Linear), key(2 * TICKS, 100.0, Hold), key(7 * TICKS, 50.0, bezier)])),
        blur(true, 10.0, false),
        tint(true, ([163, 247, 143], [240, 242, 22], 100.0), (vec![], vec![], vec![])),
        PrEffect { enabled: true, params: PrEffectParams::BlackWhite, animations: Vec::new() },
        tint(true, (black, orange, 50.0), (vec![], vec![], vec![])),
        tint(false, (black, white, 100.0), (vec![], vec![], vec![])),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
    clip.effects = native.clone();
    let wire = project_document(&sequence);
    // Every effect is a tintTritone with all seven values; a Black & White is
    // Tint's defaults; a keyed static value is its first key's.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            tint_tritone(1, true, (black, white, 0.0)),
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            tint_tritone(3, true, ([163, 247, 143], [240, 242, 22], 100.0)),
            tint_tritone(4, true, (black, white, 100.0)),
            tint_tritone(5, true, (black, orange, 50.0)),
            tint_tritone(6, false, (black, white, 100.0)),
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    // A colour key is one key on each channel track, at the same time with
    // the same easing; key ids name the effect, the parameter and the layer.
    let imported_key = |name: &str, index, layer_millis, value, easing| {
        (
            format!("premiere-effect-1-{name}-1-{index}"),
            layer_millis,
            value,
            easing,
        )
    };
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    let fx_bezier = PropertyKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let channel = |name: &str, values: [f64; 3]| {
        (
            1,
            name.to_owned(),
            vec![
                imported_key(name, 0, -500, values[0], linear),
                imported_key(name, 1, 1000, values[1], linear),
                imported_key(name, 2, 6000, values[2], hold),
            ],
        )
    };
    #[rustfmt::skip]
    let expected = [
        channel("whiteR", [1.0, 0.0, 1.0]),
        channel("whiteG", [1.0, 128.0 / 255.0, 128.0 / 255.0]),
        channel("whiteB", [1.0, 1.0, 0.0]),
        (1, "amount".to_owned(), vec![imported_key("amount", 0, -500, 0.0, linear), imported_key("amount", 1, 1000, 100.0, hold), imported_key("amount", 2, 6000, 50.0, fx_bezier)]),
    ];
    assert_eq!(effect_tracks(&document), expected);
    // Every tintTritone exports as a Tint: the Black & White returns as the
    // default Tint, its identity not kept.
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let mut exported: Vec<_> = native.into_iter().map(current_blur_export).collect();
    exported[3] = tint(true, (black, white, 100.0), (vec![], vec![], vec![]));
    assert_eq!(exported_effects(&project), exported);
}

/// Export input: a Gaussian Blur under a `tintTritone` (id 2) with the
/// `fields` given, and the `tracks` of its parameters.
fn document_with_tint(enabled: bool, fields: Value, tracks: &[(&str, Value)]) -> Value {
    let mut effect = json!({"type": "tintTritone"});
    for (name, value) in fields.as_object().unwrap() {
        effect[name] = value.clone();
    }
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 2, "enabled": enabled, "effect": effect},
    ]));
    let entries: Vec<_> = tracks
        .iter()
        .map(|(param, keys)| {
            json!({
                "target": {"kind": "effectProperty", "effectId": 2, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": keys},
            })
        })
        .collect();
    wire["composition"]["dynamics"] = json!({"entries": entries});
    wire
}

/// All seven `tintTritone` fields from 8-bit colours and an amount.
fn tint_fields((black, white, amount): ([u8; 3], [u8; 3], f64)) -> Value {
    tint_tritone(0, true, (black, white, amount))["effect"].clone()
}

#[test]
fn edited_tints_export_eight_bit_colours_and_colour_keys() {
    use PrKeyframeEasing::{Hold, Linear};
    let linear = || json!({"type": "linear"});
    let hold = || json!({"type": "hold"});
    let millisecond = TICKS_PER_MILLISECOND;
    let (black, white, orange) = ([0, 0, 0], [255, 255, 255], [255, 128, 0]);
    // Map White To white at 0, then (255, 128, 0) held from 0.5 s: one track
    // per channel.
    let white_keys = |easing: &dyn Fn() -> Value| {
        [
            (
                "whiteR",
                json!([
                    fx_key("a", 0, 1.0, linear()),
                    fx_key("b", 500, 1.0, easing())
                ]),
            ),
            (
                "whiteG",
                json!([
                    fx_key("c", 0, 1.0, linear()),
                    fx_key("d", 500, 128.0 / 255.0, easing())
                ]),
            ),
            (
                "whiteB",
                json!([
                    fx_key("e", 0, 1.0, linear()),
                    fx_key("f", 500, 0.0, easing())
                ]),
            ),
        ]
    };
    let mut fields = tint_fields((black, white, 100.0));
    // A channel between two 8-bit values rounds to the nearer one (at most
    // 1/510 away); the Amount is written as it is.
    fields["blackR"] = json!(0.5);
    fields["whiteG"] = json!(0.9);
    fields["amount"] = json!(33.5);
    // (enabled, fields, tracks, the Tint it exports as).
    #[rustfmt::skip]
    let tints = [
        (true, tint_fields((black, white, 100.0)), vec![], tint(true, (black, white, 100.0), (vec![], vec![], vec![]))),
        (true, fields, vec![], tint(true, ([128, 0, 0], [255, 230, 255], 33.5), (vec![], vec![], vec![]))),
        (false, tint_fields(([163, 247, 143], [240, 242, 22], 100.0)), vec![], tint(false, ([163, 247, 143], [240, 242, 22], 100.0), (vec![], vec![], vec![]))),
        (true, tint_fields((black, white, 100.0)), white_keys(&hold).to_vec(), tint(true, (black, white, 100.0), (vec![], vec![colour_key(0, white, Linear), colour_key(500 * millisecond, orange, Hold)], vec![]))),
        (true, tint_fields((black, white, 0.0)), vec![("amount", json!([fx_key("a", 0, 0.0, linear()), fx_key("b", 500, 100.0, hold())]))], tint(true, (black, white, 0.0), (vec![], vec![], vec![key(0, 0.0, Linear), key(500 * millisecond, 100.0, Hold)]))),
    ];
    for (enabled, fields, tracks, expected) in tints {
        let (project, omissions) = export(document_with_tint(enabled, fields.clone(), &tracks));
        assert!(omissions.is_empty(), "{fields}: {omissions:?}");
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false), expected]
        );
    }
}

#[test]
fn tints_premiere_cannot_represent_are_not_exported() {
    let linear = || json!({"type": "linear"});
    let bezier = || json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9});
    let (black, white) = ([0, 0, 0], [255, 255, 255]);
    let full = || tint_fields((black, white, 100.0));
    let without = |name: &str| {
        let mut fields = full();
        fields.as_object_mut().unwrap().remove(name);
        fields
    };
    let with = |name: &str, value: f64| {
        let mut fields = full();
        fields[name] = json!(value);
        fields
    };
    // Key ids are unique per document: they carry the channel name.
    let white_track = |name: &'static str, second: f64, easing: Value| {
        (
            name,
            json!([
                fx_key(&format!("{name}-a"), 0, 1.0, linear()),
                fx_key(&format!("{name}-b"), 500, second, easing)
            ]),
        )
    };
    // (fields, tracks, the reason).
    #[rustfmt::skip]
    let tints = [
        (without("whiteB"), vec![], "whiteB has no value; only a tintTritone with all seven values exports"),
        (without("amount"), vec![], "amount has no value; only a tintTritone with all seven values exports"),
        (with("blackR", 1.2), vec![], "Map Black To [1.2, 0.0, 0.0] has a channel outside 0 to 1"),
        (with("whiteG", -0.01), vec![], "Map White To [1.0, -0.01, 1.0] has a channel outside 0 to 1"),
        (with("amount", 100.5), vec![], "Amount to Tint 100.5 is outside Premiere's 0 to 100 range"),
        (full(), vec![("amount", json!([fx_key("a", 0, 0.0, linear()), fx_key("b", 500, 101.0, linear())]))], "Amount to Tint key value 101 is outside Premiere's 0 to 100 range"),
        // One keyed channel, channels keyed at different times, a channel
        // key outside 0 to 1, and a Bezier segment between colours.
        (full(), vec![white_track("whiteR", 0.0, linear())], "its Map White To whiteR, whiteG and whiteB keys must all be keyed, but whiteG is not; Premiere keys a colour as one track"),
        (full(), vec![white_track("whiteR", 0.0, linear()), white_track("whiteG", 0.0, linear()), ("whiteB", json!([fx_key("c", 0, 1.0, linear()), fx_key("d", 600, 0.0, linear())]))], "its Map White To whiteR, whiteG and whiteB keys differ in time or easing, but Premiere keys a colour as one track"),
        (full(), vec![white_track("whiteR", 1.5, linear()), white_track("whiteG", 0.0, linear()), white_track("whiteB", 0.0, linear())], "Map White To [1.5, 0.0, 0.0] has a channel outside 0 to 1 at 500 ms"),
        (full(), vec![white_track("whiteR", 0.0, bezier()), white_track("whiteG", 0.0, bezier()), white_track("whiteB", 0.0, bezier())], "Map White To key at 500 ms has Bezier easing; Premiere's Bezier interpolation between colours is unverified"),
    ];
    for (fields, tracks, reason) in tints {
        let (project, omissions) = export(document_with_tint(true, fields.clone(), &tracks));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)],
            "{fields}"
        );
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".to_owned(),
                reason: format!("effects: tintTritone effect 2 was not exported: {reason}"),
            }],
            "{fields}"
        );
    }
}

/// A Ramp with static points, colours and `blend`, and the keys of its keyed
/// parameters in native order (a keyed static value is its first key's).
fn ramp(
    enabled: bool,
    (start, end): ([f64; 2], [f64; 2]),
    (start_colour, end_colour): ([u8; 3], [u8; 3]),
    blend: f64,
    animations: Vec<PrEffectParamAnimation>,
) -> PrEffect {
    PrEffect {
        enabled,
        params: PrEffectParams::Ramp(PrRamp {
            start,
            start_colour: PrColour { rgb: start_colour },
            end,
            end_colour: PrColour { rgb: end_colour },
            blend,
        }),
        animations,
    }
}

/// The FX `gradientRamp` of a Ramp: frame-UV points, channel shares of 255,
/// `blend` one minus Blend With Original, the linear shape.
fn gradient_ramp(
    id: u64,
    enabled: bool,
    (start, end): ([f64; 2], [f64; 2]),
    (start_colour, end_colour): ([u8; 3], [u8; 3]),
    blend: f64,
) -> Value {
    let share = |channel: u8| f64::from(channel) / 255.0;
    json!({"id": id, "enabled": enabled, "effect": {"type": "gradientRamp",
        "startX": start[0], "startY": start[1], "endX": end[0], "endY": end[1],
        "startR": share(start_colour[0]), "startG": share(start_colour[1]), "startB": share(start_colour[2]),
        "endR": share(end_colour[0]), "endG": share(end_colour[1]), "endB": share(end_colour[2]),
        "blend": 1.0 - blend, "shape": 0.0}})
}

#[test]
fn keyed_ramp_imports_as_point_colour_and_blend_tracks_and_exports_back() {
    use PrKeyframeEasing::{Hold, Linear};
    let bezier = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let (black, white, red, blue) = ([0, 0, 0], [255, 255, 255], [200, 40, 40], [40, 40, 200]);
    let vertical = ([0.5, 0.0], [0.5, 1.0]);
    // Source In 1 s: keys before, inside and after the trimmed range stay.
    // The keyed vertical Ramp (Start Color Linear then Hold, End of Ramp along
    // its axis, Blend with a Bezier) sits above a Gaussian Blur, a horizontal
    // Ramp with colours and Blend 0.25, and a bypassed default Ramp. (Blend
    // 0.3 would return as 0.30000000000000004: `1 - (1 - v)` is exact only
    // for a dyadic `v`, and Premiere stores the fraction as a float32 anyway.)
    #[rustfmt::skip]
    let native = vec![
        ramp(true, vertical, (black, white), 1.0, vec![
            PrEffectParamAnimation { param: &RAMP_START_COLOR, keys: PrEffectParamKeys::Colour(vec![colour_key(TICKS / 2, black, Linear), colour_key(2 * TICKS, red, Linear), colour_key(7 * TICKS, blue, Hold)]) },
            PrEffectParamAnimation { param: &RAMP_END, keys: PrEffectParamKeys::Point(vec![point_key(TICKS / 2, [0.5, 1.0], Linear), point_key(2 * TICKS, [0.5, 0.6], Hold), point_key(7 * TICKS, [0.5, 0.8], Linear)]) },
            PrEffectParamAnimation { param: &RAMP_BLEND, keys: PrEffectParamKeys::Scalar(vec![key(TICKS / 2, 1.0, Linear), key(2 * TICKS, 0.0, Hold), key(7 * TICKS, 0.5, bezier)]) },
        ]),
        blur(true, 10.0, false),
        ramp(true, ([0.2, 0.5], [0.8, 0.5]), (red, blue), 0.25, vec![]),
        ramp(false, vertical, (black, white), 0.0, vec![]),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
    clip.effects = native.clone();
    let wire = project_document(&sequence);
    // Every Ramp is a gradientRamp with all twelve values; a keyed static
    // value is its first key's; blend is one minus Blend With Original.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            gradient_ramp(1, true, vertical, (black, white), 1.0),
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            gradient_ramp(3, true, ([0.2, 0.5], [0.8, 0.5]), (red, blue), 0.25),
            gradient_ramp(4, false, vertical, (black, white), 0.0),
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    // A colour key is one key per channel track and a point key one per
    // coordinate track (the aligned x stays 0.5), at the same time with the
    // same easing; the Blend keys map to 1 − value with their easing.
    let imported_key = |name: &str, index, layer_millis, value, easing| {
        (
            format!("premiere-effect-1-{name}-1-{index}"),
            layer_millis,
            value,
            easing,
        )
    };
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    let fx_bezier = PropertyKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    let track = |name: &str, values: [f64; 3], easings: [PropertyKeyframeEasing; 3]| {
        (
            1,
            name.to_owned(),
            vec![
                imported_key(name, 0, -500, values[0], easings[0]),
                imported_key(name, 1, 1000, values[1], easings[1]),
                imported_key(name, 2, 6000, values[2], easings[2]),
            ],
        )
    };
    let colour_easing = [linear, linear, hold];
    #[rustfmt::skip]
    let expected = [
        track("startR", [0.0, 200.0 / 255.0, 40.0 / 255.0], colour_easing),
        track("startG", [0.0, 40.0 / 255.0, 40.0 / 255.0], colour_easing),
        track("startB", [0.0, 40.0 / 255.0, 200.0 / 255.0], colour_easing),
        track("endX", [0.5, 0.5, 0.5], [linear, hold, linear]),
        track("endY", [1.0, 0.6, 0.8], [linear, hold, linear]),
        track("blend", [0.0, 1.0, 0.5], [linear, hold, fx_bezier]),
    ];
    assert_eq!(effect_tracks(&document), expected);
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported: Vec<_> = native.into_iter().map(current_blur_export).collect();
    assert_eq!(exported_effects(&project), exported);
}

#[test]
fn ramps_on_a_host_whose_frame_is_not_the_canvas_are_omitted_on_import() {
    use crate::schema::PrPropertyAnimation::{Opacity, Rotation, UniformScale};
    let keys = vec![
        key(0, 100.0, PrKeyframeEasing::Linear),
        key(TICKS, 50.0, PrKeyframeEasing::Linear),
    ];
    let mut small = crate::tests::support::video_media();
    let stream = small
        .get_mut(&crate::schema::MediaId("source".into()))
        .unwrap()
        .video
        .as_mut()
        .unwrap();
    (stream.width, stream.height) = (1280, 720);
    // (Motion keys, static Motion edits, the source, why the Ramp is omitted
    // or "" when it converts)
    let moved = "static Motion moves the clip frame off the canvas";
    type MotionEdit<'a> = &'a dyn Fn(&mut crate::schema::PrStaticTransform);
    #[rustfmt::skip]
    let clips: [(Vec<_>, MotionEdit<'_>, _, &str); 8] = [
        (vec![UniformScale(keys.clone())], &|_| {}, None, "Motion is animated"),
        (vec![Rotation(keys.clone())], &|_| {}, None, "Motion is animated"),
        (Vec::new(), &|transform| transform.scale = [50.0; 2], None, moved),
        (Vec::new(), &|transform| transform.rotation = 30.0, None, moved),
        (Vec::new(), &|transform| transform.position = [0.7, 0.5], None, moved),
        (Vec::new(), &|_| {}, Some(small.clone()), "the source frame differs from the canvas"),
        // Opacity keys and a pivot moved with its position leave the frame.
        (vec![Opacity(keys.clone())], &|_| {}, None, ""),
        (Vec::new(), &|transform| (transform.anchor_point, transform.position) = ([0.3, 0.3], [0.3, 0.3]), None, ""),
    ];
    for (animations, edit, media, reason) in clips {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.animations = animations;
        edit(&mut clip.transform);
        clip.effects = vec![
            ramp(
                true,
                ([0.5, 0.0], [0.5, 1.0]),
                ([0, 0, 0], [255, 255, 255]),
                0.0,
                vec![],
            ),
            blur(true, 10.0, false),
        ];
        let media = media.unwrap_or_else(crate::tests::support::video_media);
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        let gaussian = |id: u64| json!({"id": id, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}});
        if reason.is_empty() {
            assert_eq!(
                wire["composition"]["layers"][0]["effects"],
                json!([
                    gradient_ramp(
                        1,
                        true,
                        ([0.5, 0.0], [0.5, 1.0]),
                        ([0, 0, 0], [255, 255, 255]),
                        0.0
                    ),
                    gaussian(2)
                ])
            );
            assert!(omissions.is_empty(), "{omissions:?}");
            continue;
        }
        // The clip and its Gaussian Blur convert; only the Ramp is omitted.
        assert_eq!(
            wire["composition"]["layers"][0]["effects"],
            json!([gaussian(1)]),
            "{reason}"
        );
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "source".to_owned(),
                reason: format!(
                    "Ramp effect at stack position 1 was not imported: {reason}; {}",
                    super::FRAME_RULE
                ),
            }]
        );
    }
}

#[test]
fn ramp_on_a_stage_group_is_omitted_on_import() {
    let mut sequence = masked_directional_blur_sequence(true, true);
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.transform.scale, clip.transform.rotation) = ([100.0; 2], 0.0);
    clip.effects = vec![
        ramp(
            true,
            ([0.5, 0.0], [0.5, 1.0]),
            ([0, 0, 0], [255, 255, 255]),
            0.0,
            vec![],
        ),
        blur(true, 10.0, false),
    ];
    let media = crate::tests::support::video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let top = &wire["composition"]["layers"][0];
    assert_eq!(top["type"], "Group");
    assert_eq!(
        top["layers"][0]["effects"],
        json!([{"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}}])
    );
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "source".to_owned(),
            reason: format!(
                "Ramp effect at stack position 1 was not imported: {}",
                super::STAGED_RAMP_REASON
            ),
        }]
    );
}

/// Export input: a Gaussian Blur under a `gradientRamp` (id 2) with the
/// `fields` given, and the `tracks` of its parameters, on a layer with the
/// `transform` fields given and the layer `properties` animated.
fn document_with_ramp(
    fields: Value,
    tracks: &[(&str, Value)],
    transform: Value,
    properties: &[&str],
) -> Value {
    let mut effect = json!({"type": "gradientRamp"});
    for (name, value) in fields.as_object().unwrap() {
        effect[name] = value.clone();
    }
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 2, "enabled": true, "effect": effect},
    ]));
    for (name, value) in transform.as_object().unwrap() {
        wire["composition"]["layers"][0]["transform"][name] = value.clone();
    }
    let mut entries: Vec<_> = tracks
        .iter()
        .map(|(param, keys)| {
            json!({
                "target": {"kind": "effectProperty", "effectId": 2, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": keys},
            })
        })
        .collect();
    entries.extend(properties.iter().map(|property| layer_track(property)));
    wire["composition"]["dynamics"] = json!({"entries": entries});
    wire
}

/// All twelve `gradientRamp` fields of a linear ramp.
fn ramp_fields(
    (start, end): ([f64; 2], [f64; 2]),
    (start_colour, end_colour): ([u8; 3], [u8; 3]),
    blend: f64,
) -> Value {
    gradient_ramp(0, true, (start, end), (start_colour, end_colour), blend)["effect"].clone()
}

#[test]
fn edited_ramps_export_eight_bit_colours_points_and_blend_keys() {
    use PrKeyframeEasing::{Hold, Linear};
    let linear = || json!({"type": "linear"});
    let hold = || json!({"type": "hold"});
    let millisecond = TICKS_PER_MILLISECOND;
    let (black, white) = ([0, 0, 0], [255, 255, 255]);
    // FX blend 0.3 → Blend With Original 0.7; colours round to 8 bits; the
    // end point keyed on y alone keeps its x; the blend keys map to 1 − value.
    let mut fields = ramp_fields(([0.2, 0.5], [0.8, 0.5]), (black, white), 0.7);
    (fields["startR"], fields["endG"]) = (json!(0.5), json!(0.9));
    let tracks = [
        (
            "blend",
            json!([
                fx_key("a", 0, 1.0, linear()),
                fx_key("b", 500, 0.25, hold()),
                fx_key("c", 800, 0.0, linear())
            ]),
        ),
        (
            "endX",
            json!([
                fx_key("d", 0, 0.8, linear()),
                fx_key("e", 500, 0.9, linear())
            ]),
        ),
    ];
    let (project, omissions) = export(document_with_ramp(fields, &tracks, json!({}), &[]));
    assert!(omissions.is_empty(), "{omissions:?}");
    #[rustfmt::skip]
    let expected = ramp(true, ([0.2, 0.5], [0.8, 0.5]), ([128, 0, 0], [255, 230, 255]), 0.0, vec![
        PrEffectParamAnimation { param: &RAMP_END, keys: PrEffectParamKeys::Point(vec![point_key(0, [0.8, 0.5], Linear), point_key(500 * millisecond, [0.9, 0.5], Linear)]) },
        PrEffectParamAnimation { param: &RAMP_BLEND, keys: PrEffectParamKeys::Scalar(vec![key(0, 0.0, Linear), key(500 * millisecond, 0.75, Hold), key(800 * millisecond, 1.0, Linear)]) },
    ]);
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 10.0, false), expected]
    );
}

#[test]
fn ramps_premiere_cannot_represent_are_not_exported() {
    let linear = || json!({"type": "linear"});
    let (black, white) = ([0, 0, 0], [255, 255, 255]);
    let vertical = ([0.5, 0.0], [0.5, 1.0]);
    let full = || ramp_fields(vertical, (black, white), 0.0);
    let without = |name: &str| {
        let mut fields = full();
        fields.as_object_mut().unwrap().remove(name);
        fields
    };
    let with = |edits: &[(&str, f64)]| {
        let mut fields = full();
        for (name, value) in edits {
            fields[*name] = json!(value);
        }
        fields
    };
    let track = |name: &'static str, values: [f64; 2]| {
        (
            name,
            json!([
                fx_key(&format!("{name}-a"), 0, values[0], linear()),
                fx_key(&format!("{name}-b"), 500, values[1], linear())
            ]),
        )
    };
    let frame = |reason: &str| format!("{reason}; {}", super::FRAME_RULE);
    // (fields, tracks, the layer's static transform edits, animated layer
    // properties, the reason)
    #[rustfmt::skip]
    let ramps = [
        (without("endY"), vec![], json!({}), vec![], "endY has no value; only a gradientRamp with all twelve values exports".to_owned()),
        (without("shape"), vec![], json!({}), vec![], "shape has no value; only a gradientRamp with all twelve values exports".to_owned()),
        (with(&[("shape", 1.0)]), vec![], json!({}), vec![], "shape 1 is not 0 (linear); a radial ramp is not converted, because Premiere measures its radius in clip pixels and the FX gradientRamp in frame UV (Oracle run E10 probe)".to_owned()),
        (with(&[("endX", 0.7)]), vec![], json!({}), vec![], "Start of Ramp 0.5:0 to End of Ramp 0.7:1 is not aligned with the frame at every time; Premiere measures a ramp in clip pixels and the FX gradientRamp in frame UV, which agree only along the frame's axes (Oracle run E10 probe; supervisor decision D-24a)".to_owned()),
        (with(&[("endY", 0.0)]), vec![], json!({}), vec![], "Start of Ramp and End of Ramp are both 0.5:0; a ramp of zero length is not converted".to_owned()),
        (full(), vec![track("endX", [0.5, 0.6])], json!({}), vec![], "Start of Ramp 0.5:0 to End of Ramp 0.5:1 is not aligned with the frame at every time; Premiere measures a ramp in clip pixels and the FX gradientRamp in frame UV, which agree only along the frame's axes (Oracle run E10 probe; supervisor decision D-24a)".to_owned()),
        (full(), vec![track("endY", [1.0, 0.0])], json!({}), vec![], "Start of Ramp and End of Ramp meet: their y coordinates reach 0..0 and 0..1 over their keys, and a ramp of zero length is not converted".to_owned()),
        (full(), vec![track("endY", [1.0, 0.002])], json!({}), vec![], "Start of Ramp and End of Ramp come within 0.0020 of the frame of each other along y (their coordinates reach 0..0 and 0.002..1 over their keys); a ramp shorter than 0.0032 of the frame is not converted, because the FX gradientRamp floors its squared length at 1e-5 and would stretch it over 0.0032 of the frame".to_owned()),
        (with(&[("startR", 1.2)]), vec![], json!({}), vec![], "Start Color [1.2, 0.0, 0.0] has a channel outside 0 to 1".to_owned()),
        (with(&[("blend", 2.0)]), vec![], json!({}), vec![], "Blend With Original -1 is outside Premiere's 0 to 1 range".to_owned()),
        (full(), vec![track("blend", [1.0, 2.0])], json!({}), vec![], "Blend With Original key value -1 is outside Premiere's 0 to 1 range".to_owned()),
        // A host whose frame is not the canvas.
        (full(), vec![], json!({"scale": [50.0, 50.0]}), vec![], frame("static Motion moves the clip frame off the canvas")),
        (full(), vec![], json!({"rotation": 30.0}), vec![], frame("static Motion moves the clip frame off the canvas")),
        (full(), vec![], json!({"position": [100.0, 0.0]}), vec![], frame("static Motion moves the clip frame off the canvas")),
        (full(), vec![], json!({}), vec!["positionX", "positionY"], frame("Motion is animated")),
        (full(), vec![], json!({}), vec!["scaleX", "scaleY"], frame("Motion is animated")),
    ];
    for (fields, tracks, transform, properties, reason) in ramps {
        let (project, omissions) = export(document_with_ramp(
            fields.clone(),
            &tracks,
            transform.clone(),
            &properties,
        ));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)],
            "{fields} {transform}"
        );
        let omission = Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".to_owned(),
            reason: format!("effects: gradientRamp effect 2 was not exported: {reason}"),
        };
        assert!(
            omissions.contains(&omission),
            "{fields} {transform} {properties:?}: {omissions:?}"
        );
    }
    // Opacity keys leave the frame.
    let (project, omissions) = export(document_with_ramp(full(), &[], json!({}), &["opacity"]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        exported_effects(&project),
        [
            exported_blur(true, 10.0, false),
            ramp(true, vertical, (black, white), 0.0, vec![])
        ]
    );
}

/// A Mosaic with Sharp Colors on, static counts and the keys of its keyed
/// counts in native order (a keyed static value is its first key's).
fn mosaic(
    enabled: bool,
    (horizontal, vertical): (u32, u32),
    animations: Vec<PrEffectParamAnimation>,
) -> PrEffect {
    PrEffect {
        enabled,
        params: PrEffectParams::Mosaic(PrMosaic {
            horizontal,
            vertical,
            sharp_colors: true,
        }),
        animations,
    }
}

/// The FX `mosaic` of a Mosaic: the same counts and Sharp Colors.
fn fx_mosaic(id: u64, enabled: bool, (horizontal, vertical): (u32, u32)) -> Value {
    json!({"id": id, "enabled": enabled, "effect": {"type": "mosaic", "horizontalBlocks": f64::from(horizontal), "verticalBlocks": f64::from(vertical), "sharpColors": true}})
}

/// Clip D's Hold keys of the run E8 fixture: 10 at source 1 s, `second` at
/// 1.5 s, 20 at 2.5 s.
fn mosaic_hold_keys(second: f64) -> Vec<PrScalarKeyframe> {
    use PrKeyframeEasing::{Hold, Linear};
    vec![
        key(TICKS, 10.0, Linear),
        key(3 * TICKS / 2, second, Hold),
        key(5 * TICKS / 2, 20.0, Hold),
    ]
}

/// Export input: a Gaussian Blur under a `mosaic` (id 2) with the `fields`
/// given, and the `tracks` of its parameters, on a layer with the
/// `transform` fields given and the layer `properties` animated.
fn document_with_mosaic(
    fields: Value,
    tracks: &[(&str, Value)],
    transform: Value,
    properties: &[&str],
) -> Value {
    let mut effect = json!({"type": "mosaic"});
    for (name, value) in fields.as_object().unwrap() {
        effect[name] = value.clone();
    }
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 2, "enabled": true, "effect": effect},
    ]));
    for (name, value) in transform.as_object().unwrap() {
        wire["composition"]["layers"][0]["transform"][name] = value.clone();
    }
    let mut entries: Vec<_> = tracks
        .iter()
        .map(|(param, keys)| {
            json!({
                "target": {"kind": "effectProperty", "effectId": 2, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": keys},
            })
        })
        .collect();
    entries.extend(properties.iter().map(|property| layer_track(property)));
    wire["composition"]["dynamics"] = json!({"entries": entries});
    wire
}

#[test]
fn keyed_mosaic_imports_as_hold_count_tracks_and_exports_back() {
    // Source In 0.5 s, as clip D of the E8 fixture: the Hold keys on both
    // counts sit above a Gaussian Blur, a static 48 x 27 Mosaic and a
    // bypassed default one.
    let native = vec![
        mosaic(
            true,
            (10, 10),
            vec![
                PrEffectParamAnimation {
                    param: &MOSAIC_HORIZONTAL_BLOCKS,
                    keys: PrEffectParamKeys::Scalar(mosaic_hold_keys(40.0)),
                },
                PrEffectParamAnimation {
                    param: &MOSAIC_VERTICAL_BLOCKS,
                    keys: PrEffectParamKeys::Scalar(mosaic_hold_keys(30.0)),
                },
            ],
        ),
        blur(true, 10.0, false),
        mosaic(true, (48, 27), vec![]),
        mosaic(false, (10, 10), vec![]),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.in_ticks, clip.out_ticks) = (TICKS / 2, 7 * TICKS / 2);
    clip.effects = native.clone();
    let wire = project_document(&sequence);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            fx_mosaic(1, true, (10, 10)),
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            fx_mosaic(3, true, (48, 27)),
            fx_mosaic(4, false, (10, 10)),
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    // Layer times count from the source In: source 1.0, 1.5 and 2.5 s are
    // layer 500, 1000 and 2000 ms; the Hold into the second and third keys
    // carries over.
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    let track = |name: &str, second: f64| {
        (
            1,
            name.to_owned(),
            vec![
                (format!("premiere-effect-1-{name}-1-0"), 500, 10.0, linear),
                (format!("premiere-effect-1-{name}-1-1"), 1000, second, hold),
                (format!("premiere-effect-1-{name}-1-2"), 2000, 20.0, hold),
            ],
        )
    };
    assert_eq!(
        effect_tracks(&document),
        [
            track("horizontalBlocks", 40.0),
            track("verticalBlocks", 30.0)
        ]
    );
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported: Vec<_> = native.into_iter().map(current_blur_export).collect();
    assert_eq!(exported_effects(&project), exported);
}

#[test]
fn mosaics_convert_at_any_motion_but_not_on_a_stage_group() {
    use crate::schema::PrPropertyAnimation::{Rotation, UniformScale};
    // Block counts are fractions of the clip frame in both engines and
    // Premiere renders clip effects before Motion: a scaled, rotated, moved
    // or keyed Motion and another source
    // size all convert.
    let keys = vec![
        key(0, 100.0, PrKeyframeEasing::Linear),
        key(TICKS, 50.0, PrKeyframeEasing::Linear),
    ];
    let mut small = crate::tests::support::video_media();
    let stream = small
        .get_mut(&crate::schema::MediaId("source".into()))
        .unwrap()
        .video
        .as_mut()
        .unwrap();
    (stream.width, stream.height) = (1280, 720);
    type MotionEdit<'a> = &'a dyn Fn(&mut crate::schema::PrStaticTransform);
    #[rustfmt::skip]
    let clips: [(Vec<_>, MotionEdit<'_>, _); 5] = [
        (vec![UniformScale(keys.clone())], &|_| {}, None),
        (vec![Rotation(keys.clone())], &|_| {}, None),
        (Vec::new(), &|transform| transform.scale = [50.0; 2], None),
        (Vec::new(), &|transform| (transform.rotation, transform.position) = (30.0, [0.7, 0.5]), None),
        (Vec::new(), &|_| {}, Some(small.clone())),
    ];
    for (animations, edit, media) in clips {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.animations = animations;
        edit(&mut clip.transform);
        clip.effects = vec![mosaic(true, (16, 9), vec![]), blur(true, 10.0, false)];
        let media = media.unwrap_or_else(crate::tests::support::video_media);
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(
            wire["composition"]["layers"][0]["effects"],
            json!([
                fx_mosaic(1, true, (16, 9)),
                {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            ])
        );
    }
    // A stage group's video is no host: the frame the FX grid spans there is
    // unverified.
    let mut sequence = masked_directional_blur_sequence(true, true);
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.effects = vec![mosaic(true, (16, 9), vec![]), blur(true, 10.0, false)];
    let media = crate::tests::support::video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let top = &wire["composition"]["layers"][0];
    assert_eq!(top["type"], "Group");
    assert_eq!(
        top["layers"][0]["effects"],
        json!([{"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}}])
    );
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "source".to_owned(),
            reason: format!(
                "Mosaic (Legacy) effect at stack position 1 was not imported: {}",
                super::STAGED_MOSAIC_REASON
            ),
        }]
    );
}

#[test]
fn edited_mosaics_export_whole_counts_and_hold_keys_at_any_motion() {
    let hold = || json!({"type": "hold"});
    let linear = || json!({"type": "linear"});
    // Counts edited to 32 x 18 on a Scale 50, rotated layer with keyed
    // position, and Hold keys on the horizontal count only (source In 1 s:
    // layer 0, 500 and 1500 ms are source 1, 1.5 and 2.5 s); the first key
    // becomes the static value.
    let mut wire = document_with_mosaic(
        json!({"horizontalBlocks": 32.0, "verticalBlocks": 18.0, "sharpColors": true}),
        &[(
            "horizontalBlocks",
            json!([
                fx_key("h-a", 0, 10.0, linear()),
                fx_key("h-b", 500, 40.0, hold()),
                fx_key("h-c", 1500, 20.0, hold()),
            ]),
        )],
        json!({"scale": [50.0, 50.0], "rotation": 30.0}),
        &["positionX", "positionY"],
    );
    wire["composition"]["layers"][0]["sourceRange"]["start"] = json!(1000);
    wire["composition"]["layers"][0]["playback"]["mapping"]["output"]["start"] = json!(1000);
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        exported_effects(&project),
        [
            exported_blur(true, 10.0, false),
            mosaic(
                true,
                (10, 18),
                vec![PrEffectParamAnimation {
                    param: &MOSAIC_HORIZONTAL_BLOCKS,
                    keys: PrEffectParamKeys::Scalar(mosaic_hold_keys(40.0)),
                }]
            ),
        ]
    );
}

#[test]
fn mosaics_premiere_cannot_represent_are_not_exported() {
    let linear = || json!({"type": "linear"});
    let hold = || json!({"type": "hold"});
    let full = || json!({"horizontalBlocks": 16.0, "verticalBlocks": 9.0, "sharpColors": true});
    let with = |edits: &[(&str, Value)]| {
        let mut fields = full();
        for (name, value) in edits {
            fields[*name] = value.clone();
        }
        fields
    };
    let track = |name: &'static str, values: [f64; 3], easing: Value| {
        (
            name,
            json!([
                fx_key(&format!("{name}-a"), 0, values[0], linear()),
                fx_key(&format!("{name}-b"), 500, values[1], easing.clone()),
                fx_key(&format!("{name}-c"), 1500, values[2], easing)
            ]),
        )
    };
    let whole = |what: &str, value: &str| {
        format!("{what} {value} is not a whole number of blocks; Premiere counts whole blocks and no rounding is applied (supervisor decision D-18a-2)")
    };
    let hold_rule = "; only Hold keys convert, because the FX mosaic renders fractional block counts between keys and Premiere's stepping there is unmeasured (supervisor decision D-18a-2)";
    // (fields, tracks, the reason)
    #[rustfmt::skip]
    let mosaics = [
        (with(&[("sharpColors", json!(false))]), vec![], PrMosaic::SHARP_COLORS_OFF.to_owned()),
        (json!({"horizontalBlocks": 16.0, "verticalBlocks": 9.0}), vec![], PrMosaic::SHARP_COLORS_OFF.to_owned()),
        (with(&[("horizontalBlocks", json!(12.5))]), vec![], whole("Horizontal Blocks", "12.5")),
        (with(&[("verticalBlocks", json!(4001.0))]), vec![], "Vertical Blocks 4001 is outside Premiere's 1 to 4000 range".to_owned()),
        (with(&[("verticalBlocks", json!(2.5))]), vec![], whole("Vertical Blocks", "2.5")),
        (full(), vec![track("horizontalBlocks", [10.0, 40.0, 20.0], linear())], format!("Horizontal Blocks keys are Linear between source times 0.000 s and 0.500 s{hold_rule}")),
        (full(), vec![track("verticalBlocks", [10.0, 30.0, 20.0], json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9}))], format!("Vertical Blocks keys are Bézier between source times 0.000 s and 0.500 s{hold_rule}")),
        (full(), vec![track("horizontalBlocks", [10.0, 40.5, 20.0], hold())], whole("Horizontal Blocks key value", "40.5")),
        (full(), vec![track("horizontalBlocks", [10.0, 5000.0, 20.0], hold())], "Horizontal Blocks key value 5000 is outside Premiere's 1 to 4000 range".to_owned()),
    ];
    for (fields, tracks, reason) in mosaics {
        let (project, omissions) = export(document_with_mosaic(
            fields.clone(),
            &tracks,
            json!({}),
            &[],
        ));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)],
            "{fields}"
        );
        let omission = Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".to_owned(),
            reason: format!("effects: mosaic effect 2 was not exported: {reason}"),
        };
        assert!(omissions.contains(&omission), "{fields}: {omissions:?}");
    }
}

#[test]
fn blur_settings_without_a_premiere_equivalent_are_omitted() {
    for (field, value, expected) in [
        (
            "layerSize",
            json!([640.0, 360.0]),
            "layerSize sets the Repeat Edge Pixels clamp",
        ),
        (
            "blurriness",
            json!(5700.5),
            "blurriness 5700.5 is outside Premiere's 0 to 5700 range",
        ),
    ] {
        let mut blur = json!({"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 25.0}});
        blur["effect"][field] = value;
        let (project, omissions) = export(document_with_effects(json!([blur])));
        assert!(exported_effects(&project).is_empty(), "{field}");
        assert!(
            omissions.iter().any(|item| item.reason.contains(expected)),
            "{field}: {omissions:?}"
        );
    }
}

/// The match names of the one clip's components in the written chain's
/// document order, which is its ascending `Index` order, after `project` is
/// written as Premiere XML.
fn written_chain(mut project: PrProjectFile) -> Vec<String> {
    for media in project.media.values_mut() {
        media.name = "source.mp4".to_owned();
        media.relative_path = Some("./media/source.mp4".to_owned());
        media.relative_paths = vec!["./media/source.mp4".to_owned()];
        media.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            "/tmp/source.mp4".into(),
        )];
    }
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("masked.prproj");
    crate::format::PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let xml = crate::format::read_xml(&path).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let records = document.root_element().children();
    let record = |id: &str| {
        records
            .clone()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    };
    records
        .clone()
        .filter(|node| node.has_tag_name("VideoComponentChain"))
        .flat_map(|chain| chain.descendants())
        .filter(|node| node.has_tag_name("Component"))
        .filter_map(|node| node.attribute("ObjectRef"))
        .map(|id| {
            record(id)
                .children()
                .find(|child| child.has_tag_name("MatchName"))
                .and_then(|child| child.text())
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn flat_crop_or_linear_wipe_exports_its_effects_after_it() {
    // FX applies a video's mask before its blur, and Premiere applies the chain
    // in descending `Index` (Oracle run C6, F24): the mask at the highest
    // Index first, then the blur, then Motion.
    for (mask_name, mask) in [("AE.ADBE AECrop", 1), ("AE.ADBE Linear Wipe", 4)] {
        let mut wire = document_with_effects(json!([
            {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 25.0}}
        ]));
        let feather = if mask == 1 { 0.0 } else { 5.0 };
        wire["composition"]["layers"][0]["masks"] = json!([
            {"id": mask, "mode": "add", "layer": 2, "feather": [feather, feather], "opacity": 1.0}
        ]);
        let mut canvas = wire["composition"]["layers"][1].clone();
        canvas["id"] = json!(3);
        wire["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(canvas);
        let guide = &mut wire["composition"]["layers"][1];
        if mask == 1 {
            guide["rect"]["position"] = json!([0.0, 162.0]);
            guide["rect"]["size"] = json!([1920.0, 918.0]);
        } else {
            guide["name"] = json!("Premiere Linear Wipe guide 1");
            guide["transform"]["scale"] = json!([0.0, 100.0]);
            wire["composition"]["dynamics"] = json!({"entries": [{
                "target": {"kind": "layer", "layerId": 2, "propertyType": "scaleX"},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                    {"id": "start", "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "linear"}},
                    {"id": "end", "layerTime": 1000, "value": {"type": "float", "value": 100.0}, "easing": {"type": "linear"}}
                ]}
            }]});
        }
        let (project, omissions) = export(wire);
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        if mask == 1 {
            assert_eq!(clip.crop.top, 15.0);
            assert!(omissions.is_empty(), "{omissions:?}");
        } else {
            let wipe = clip.linear_wipe.as_ref().unwrap();
            assert_eq!((wipe.angle_degrees, wipe.feather), (270, 5.0));
            assert_eq!(wipe.initial_completion, 100.0);
            assert_eq!(
                wipe.completion
                    .iter()
                    .map(|key| (key.source_ticks, key.value))
                    .collect::<Vec<_>>(),
                [(0, 100.0), (TICKS, 0.0)]
            );
            assert!(omissions.is_empty(), "{omissions:?}");
        }
        assert_eq!(clip.effects, [exported_blur(true, 25.0, false)]);
        assert_eq!(clip.effects_above_mask, 0);
        assert_eq!(
            written_chain(project),
            ["AE.ADBE Motion", "AE.Impact_Blur_FX", mask_name]
        );
    }
}

#[test]
fn legacy_effects_without_ids_export_as_enabled_effects() {
    let (project, omissions) = export(document_with_effects(json!([
        {"type": "gaussianBlur", "blurriness": 10.0},
        {"type": "posterize", "levels": 7.0},
    ])));
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 10.0, false)]
    );
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".to_owned(),
            reason: "effects: posterize effect at stack position 2 was not exported: it has no Premiere effect mapping"
                .to_owned(),
        }]
    );
}

/// A Levels with its master (RGB) values in native order.
fn levels(rgb: [f64; 5], animations: Vec<PrEffectParamAnimation>) -> PrEffect {
    PrEffect {
        enabled: true,
        params: PrEffectParams::Levels(PrLevels { rgb }),
        animations,
    }
}

/// Native keys of the Levels parameter at `index` (4 is Gamma).
fn levels_keys(index: usize, keys: Vec<PrScalarKeyframe>) -> PrEffectParamAnimation {
    PrEffectParamAnimation {
        param: &LEVELS.params[index],
        keys: PrEffectParamKeys::Scalar(keys),
    }
}

#[test]
fn levels_import_as_fx_levels_with_gamma_in_units() {
    use PrKeyframeEasing::{Hold, Linear};
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    // Source In 1 s: the keys land on the layer clock.
    (clip.in_ticks, clip.out_ticks) = (TICKS, 6 * TICKS);
    clip.effects = vec![
        levels([20.0, 235.0, 16.0, 240.0, 70.0], Vec::new()),
        // A keyed Gamma starts at its first key.
        levels(
            [0.0, 255.0, 0.0, 255.0, 150.0],
            vec![levels_keys(
                4,
                vec![key(TICKS, 150.0, Linear), key(2 * TICKS, 70.0, Hold)],
            )],
        ),
    ];
    let wire = project_document(&sequence);
    // Native Gamma is FX gamma in hundredths; the levels are unchanged.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "levels", "inputBlack": 20.0, "inputWhite": 235.0, "gamma": 0.7, "outputBlack": 16.0, "outputWhite": 240.0}},
            {"id": 2, "enabled": true, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.5, "outputBlack": 0.0, "outputWhite": 255.0}},
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    assert_eq!(
        effect_tracks(&document),
        [(
            2,
            "gamma".to_owned(),
            vec![
                (
                    "premiere-effect-2-gamma-1-0".to_owned(),
                    0,
                    1.5,
                    PropertyKeyframeEasing::Linear
                ),
                (
                    "premiere-effect-2-gamma-1-1".to_owned(),
                    1000,
                    0.7,
                    PropertyKeyframeEasing::Hold
                ),
            ]
        )]
    );
}

#[test]
fn edited_levels_export_whole_native_values_around_a_blur() {
    use PrKeyframeEasing::{Hold, Linear};
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "levels", "inputBlack": 3.4, "inputWhite": 235.0, "gamma": 0.7049, "outputBlack": 16.0, "outputWhite": 239.6}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 3, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.0, "outputBlack": 0.0, "outputWhite": 255.0}},
    ]));
    wire["composition"]["layers"][0]["sourceRange"]["start"] = json!(2000);
    wire["composition"]["layers"][0]["playback"]["mapping"]["output"]["start"] = json!(2000);
    // Output black overshoots its last key to 159.45 (158.9 rounded), below
    // output white 255.
    let overshoot = json!({"type": "cubicBezier", "x1": 0.2, "y1": 0.0, "x2": 0.6, "y2": 2.2});
    wire["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 3, "paramName": "outputBlack"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            fx_key("c", 0, 0.0, json!({"type": "linear"})),
            fx_key("d", 500, 117.4, overshoot),
        ]},
    }, {
        "target": {"kind": "effectProperty", "effectId": 3, "paramName": "gamma"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            fx_key("a", 0, 1.456, json!({"type": "linear"})),
            fx_key("b", 500, 0.5, json!({"type": "hold"})),
        ]},
    }]});
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    // Levels round to Premiere's whole levels and Gamma to hundredths;
    // a keyed Gamma's static value is its first key's.
    let millisecond = TICKS_PER_MILLISECOND;
    assert_eq!(
        exported_effects(&project),
        [
            levels([3.0, 235.0, 16.0, 240.0, 70.0], Vec::new()),
            exported_blur(true, 10.0, false),
            levels(
                [0.0, 255.0, 0.0, 255.0, 146.0],
                vec![
                    levels_keys(
                        2,
                        vec![
                            key(2000 * millisecond, 0.0, Linear),
                            key(
                                2500 * millisecond,
                                117.0,
                                PrKeyframeEasing::CubicBezier {
                                    x1: 0.2,
                                    y1: 0.0,
                                    x2: 0.6,
                                    y2: 2.2,
                                },
                            ),
                        ],
                    ),
                    levels_keys(
                        4,
                        vec![
                            key(2000 * millisecond, 146.0, Linear),
                            key(2500 * millisecond, 50.0, Hold),
                        ],
                    ),
                ],
            ),
        ]
    );
}

#[test]
fn levels_premiere_cannot_represent_omit_the_levels() {
    // FX order: inputBlack, inputWhite, gamma, outputBlack, outputWhite.
    let neutral = [0.0, 255.0, 1.0, 0.0, 255.0];
    let with = |index: usize, value: f64| {
        let mut fx = neutral;
        fx[index] = value;
        fx
    };
    let linear = || json!({"type": "linear"});
    // Eases 0 to 117.4 and overshoots to 159.45 between the keys; rounded to
    // 0 to 117 it reaches 158.9, below output white 159.4 rounded to 159.
    let overshoot = || json!({"type": "cubicBezier", "x1": 0.2, "y1": 0.0, "x2": 0.6, "y2": 2.2});
    for (enabled, fx, keys, reason) in [
        (true, with(0, 255.5), None, "(RGB) Black Input Level 255.5 is outside Premiere's 0 to 255 range"),
        (true, with(2, 10.5), None, "(RGB) Gamma 10.5 is outside Premiere's 0 to 10 range"),
        (true, with(3, -0.4), None, "(RGB) Black Output Level -0.4 is outside Premiere's 0 to 255 range"),
        (true, neutral, Some(("gamma", [1.0, 10.01], linear())), "(RGB) Gamma key value 10.01 is outside Premiere's 0 to 10 range"),
        (false, neutral, None, "a disabled Levels has no verified Premiere bypass form"),
        (true, [200.0, 100.0, 1.0, 0.0, 255.0], None, "input black 200 reaches input white 100, a Levels form that no Adobe render measured"),
        (true, [0.0, 255.0, 1.0, 200.0, 100.0], None, "output black 200 exceeds output white 100, a Levels form that no Adobe render measured"),
        // Reversed outputs that round to 100 and 100.
        (true, [0.0, 255.0, 1.0, 100.4, 100.3], None, "output black 100.4 exceeds output white 100.3, a Levels form that no Adobe render measured"),
        // Ordered inputs that round to 101 and 101.
        (true, [100.6, 100.9, 1.0, 0.0, 255.0], None, "input black 101 reaches input white 101, a Levels form that no Adobe render measured"),
        (true, with(4, 159.4), Some(("outputBlack", [0.0, 117.4], overshoot())), "output black 159.4483673469388 exceeds output white 159.4, a Levels form that no Adobe render measured"),
    ] {
        let [input_black, input_white, gamma, output_black, output_white] = fx;
        let mut wire = document_with_effects(json!([
            {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            {"id": 2, "enabled": enabled, "effect": {"type": "levels", "inputBlack": input_black,
                "inputWhite": input_white, "gamma": gamma, "outputBlack": output_black, "outputWhite": output_white}},
        ]));
        if let Some((param, [first, second], easing)) = keys {
            wire["composition"]["dynamics"] = json!({"entries": [{
                "target": {"kind": "effectProperty", "effectId": 2, "paramName": param},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                    fx_key("a", 0, first, linear()),
                    fx_key("b", 500, second, easing),
                ]},
            }]});
        }
        let (project, omissions) = export(wire);
        assert_eq!(exported_effects(&project), [exported_blur(true, 10.0, false)]);
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(
            omissions[0].reason,
            format!("effects: levels effect 2 was not exported: {reason}")
        );
    }
    // JSON cannot carry a non-finite value, but the FX model can: the static
    // values of an exported Levels convert through `native_value`.
    assert_eq!(
        super::native_value(&LEVELS.params[0], "", f64::NAN),
        Err("(RGB) Black Input Level NaN is outside Premiere's 0 to 255 range".to_owned())
    );
}

fn film_impact_fixture() -> PrProjectFile {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_film_impact_blur_26_5_derived.prproj");
    let xml = crate::format::read_xml(&path).unwrap();
    crate::format::inspect_project_with_omissions(
        &xml,
        Some("093ea8ea-ef35-4657-a134-f0f1bbd260fb"),
    )
    .unwrap()
    .0
}

#[test]
fn film_impact_native_default_imports_editably_and_exports_as_current_gaussian_blur() {
    let native = film_impact_fixture();
    let original = native.sequences[0].video_tracks[1].clip(3).effects.clone();
    // CPU mapping assertion only: put the native controls on the supported
    // unit-test clip. This is not a conversion/render of the QTRLE fixture.
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).effects = original.clone();
    let media = crate::tests::support::video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Amount 20 is FX Blurriness 114, the Legacy Blurriness of the same spread.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"],
        json!({
            "type": "gaussianBlur", "blurriness": 114.0, "repeatEdgePixels": true
        })
    );
    let (project, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let effects = exported_effects(&project);
    assert_eq!(effects, original);
    assert_eq!(reread_effects(project), original);
}

#[test]
fn film_impact_native_amount_keys_keep_trim_easing_and_edits_in_both_directions() {
    let native = film_impact_fixture();
    for index in [16, 17, 18] {
        let source = native.sequences[0].video_tracks[1].clip(index);
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.effects = source.effects.clone();
        (clip.in_ticks, clip.out_ticks) = (source.in_ticks, source.out_ticks);
        clip.end_ticks = clip.start_ticks + source.out_ticks - source.in_ticks;
        sequence.timeline_end_ticks = clip.end_ticks;
        let mut wire = project_document(&sequence);
        assert_eq!(
            wire["composition"]["layers"][0]["effects"][0]["effect"]["blurriness"],
            28.5
        );
        let keys = wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
            .as_array_mut()
            .unwrap();
        let values: Vec<_> = keys
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(values, [(500, 28.5), (1500, 285.0), (2500, 114.0)]);
        keys[1]["value"]["value"] = json!(228.0);
        keys[1]["layerTime"] = json!(1750);

        let mut expected = source.effects.clone();
        let PrEffectParamKeys::Scalar(keys) = &mut expected[0].animations[0].keys else {
            panic!("Amount has scalar keys");
        };
        keys[1].value = 40.0;
        keys[1].source_ticks = 11 * TICKS / 4;
        for fps in [FrameRate::Fps30, FrameRate::Fps24] {
            let (project, omissions) = export_at(wire.clone(), fps);
            assert!(omissions.is_empty(), "{omissions:?}");
            for actual in [exported_effects(&project), reread_effects(project)] {
                assert_eq!(actual.len(), 1);
                assert_eq!(
                    (actual[0].enabled, &actual[0].params),
                    (expected[0].enabled, &expected[0].params)
                );
                assert_eq!(actual[0].animations.len(), 1);
                assert_eq!(actual[0].animations[0].param, &FILM_IMPACT_BLUR_AMOUNT);
                let actual = actual[0].animations[0].keys.scalar().unwrap();
                let expected = expected[0].animations[0].keys.scalar().unwrap();
                assert_eq!(actual.len(), expected.len());
                for (actual, expected) in actual.iter().zip(expected) {
                    assert_eq!(
                        (actual.source_ticks, actual.value),
                        (expected.source_ticks, expected.value)
                    );
                    match (&actual.easing, &expected.easing) {
                        (
                            PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 },
                            PrKeyframeEasing::CubicBezier {
                                x1: a,
                                y1: b,
                                x2: c,
                                y2: d,
                            },
                        ) => {
                            // The editable JSON round trip can change a normalized
                            // handle by one floating-point unit, not its easing kind.
                            for (actual, expected) in [(x1, a), (y1, b), (x2, c), (y2, d)] {
                                assert!((actual - expected).abs() < 1e-12, "clip {index}, {fps:?}");
                            }
                        }
                        _ => assert_eq!(actual.easing, expected.easing),
                    }
                }
            }
        }
    }
}

#[test]
fn gaussian_blur_exports_as_the_current_blur_up_to_amount_1000() {
    for (blurriness, amount) in [(0.0, 0.0), (114.0, 20.0), (5700.0, 1000.0)] {
        let (project, omissions) = export(document_with_effects(json!([
            {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": blurriness}}
        ])));
        assert!(omissions.is_empty(), "{omissions:?}");
        let current = PrFilmImpactBlur {
            amount,
            repeat_edge_pixels: false,
        };
        assert_eq!(
            exported_effects(&project)[0].params,
            PrEffectParams::FilmImpactBlur(current)
        );
    }
    // A key above Amount 1000 omits the blur (`blur_settings_without_a_premiere_equivalent_are_omitted`
    // covers the static value); nothing is clamped and no Legacy record is written.
    let linear = || json!({"type": "linear"});
    let (project, omissions) = export(document_with_blurriness_keys(json!([
        fx_key("a", 500, 0.0, linear()),
        fx_key("b", 700, 6000.0, linear()),
    ])));
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 10.0, false)]
    );
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "layer 1 (\"Source\")".to_owned(),
            reason: "effects: gaussianBlur effect 1 was not exported: blurriness key value 6000 is outside Premiere's 0 to 5700 range".to_owned(),
        }]
    );
}

#[test]
fn transform_alpha_key_requires_a_static_canvas_sized_still_matte() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_transform_track_matte_26_5_strict.prproj");
    type Edit = fn(&mut crate::schema::PrVideoOccurrence);
    for (case, edit) in [
        (
            "moved matte",
            (|clip: &mut crate::schema::PrVideoOccurrence| clip.transform.position = [0.6, 0.5])
                as Edit,
        ),
        ("keyed matte", |clip| {
            clip.animations = vec![crate::schema::PrPropertyAnimation::Opacity(vec![
                key(0, 100.0, PrKeyframeEasing::Linear),
                key(TICKS, 50.0, PrKeyframeEasing::Linear),
            ])]
        }),
        ("transparent matte", |clip| clip.opacity = 50.0),
        ("retimed matte", |clip| clip.playback_rate = 2.0),
        ("effect on matte", |clip| {
            clip.effects = vec![blur(true, 10.0, false)]
        }),
    ] {
        let (mut project, _) = PrProjectFile::load(&source).unwrap();
        let sequence = &mut project.sequences[0];
        edit(sequence.video_tracks[2].clip_mut(0));
        let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
        let mut omissions = Vec::new();
        let wire =
            crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
                .unwrap()
                .to_json_value()
                .unwrap();
        assert!(
            omissions.iter().any(|omission| omission
                .reason
                .contains("requires a canvas-sized still matte")),
            "{case}: {omissions:#?}"
        );
        let groups: Vec<_> = wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer.get("trackMatte").is_some())
            .collect();
        assert_eq!(groups.len(), 2, "{case}");
        assert_eq!((*crate::test_support::layer_range(groups[0]))["start"], 0);
        assert_eq!(
            (*crate::test_support::layer_range(groups[1]))["start"],
            2500
        );
        assert_eq!(
            groups[0]["type"], "Video",
            "{case}: legacy keyed fill remains flat"
        );
        assert_eq!(
            groups[1]["type"], "Group",
            "{case}: second pair retains measured A4"
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Feature
                    && omission.reason.contains("Transform omitted")),
            "{case}: {omissions:#?}"
        );
        let layers = wire["composition"]["layers"].as_array().unwrap();
        assert!(
            layers
                .iter()
                .filter(|layer| layer["type"] == "Image")
                .all(|matte| groups
                    .iter()
                    .any(|fill| fill["trackMatte"]["layer"] == matte["id"])),
            "{case}: every root matte is claimed by a retained keyed fill"
        );
    }
    let (mut project, _) = PrProjectFile::load(&source).unwrap();
    let matte_id = project.sequences[0].video_tracks[2]
        .clip_mut(0)
        .media
        .clone();
    project
        .media
        .get_mut(&matte_id)
        .unwrap()
        .video
        .as_mut()
        .unwrap()
        .width = 1280;
    let sequence = &project.sequences[0];
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut omissions = Vec::new();
    crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions).unwrap();
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("requires a canvas-sized still matte")),
        "{omissions:#?}"
    );
}

#[test]
fn transform_alpha_key_preserves_fills_that_share_a_matte() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_transform_track_matte_26_5_strict.prproj");
    let (mut project, _) = PrProjectFile::load(&source).unwrap();
    let sequence = &mut project.sequences[0];
    let mut sibling = sequence.video_tracks[1].clip_mut(0).clone();
    sibling.id = Some("shared-matte-fill".to_owned());
    sequence.video_tracks[0] = crate::schema::PrVideoTrack::media([sibling]);
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut omissions = Vec::new();
    let wire =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    let layers = wire["composition"]["layers"].as_array().unwrap();
    let fills: Vec<_> = layers
        .iter()
        .filter(|layer| layer.get("trackMatte").is_some())
        .collect();
    assert_eq!(fills.len(), 3, "{omissions:#?}");
    let flat: Vec<_> = fills
        .iter()
        .filter(|layer| (*crate::test_support::layer_range(layer))["start"] == 0)
        .collect();
    assert_eq!(flat.len(), 2);
    assert!(flat.iter().all(|fill| fill["type"] == "Video"));
    assert_eq!(flat[0]["trackMatte"], flat[1]["trackMatte"]);
    let matte = layers
        .iter()
        .find(|layer| layer["id"] == flat[0]["trackMatte"]["layer"])
        .unwrap();
    assert_eq!(matte["type"], "Image");
    assert_eq!(
        fills
            .iter()
            .filter(|fill| fill["type"] == "Group"
                && (*crate::test_support::layer_range(fill))["start"] == 2500)
            .count(),
        1
    );
    assert_eq!(
        omissions
            .iter()
            .filter(|omission| omission.scope == OmissionScope::Feature
                && omission.reason.contains("Transform omitted"))
            .count(),
        2,
        "{omissions:#?}"
    );
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence),
        "{omissions:#?}"
    );
}
