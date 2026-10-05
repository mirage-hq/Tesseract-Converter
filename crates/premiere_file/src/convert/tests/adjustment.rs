use crate::{
    convert::tesseract_to_premiere,
    format::{FrameRate, MediaId, PrMedia, PrProjectFile, PrVideoOccurrence, PremiereProjectXml},
    media::{MediaFacts, VideoMedia},
    schema::{
        records::MediaPathField, PrColour, PrCornerPin, PrEffect, PrEffectParamAnimation,
        PrEffectParamKeys, PrEffectParams, PrGaussianBlur, PrInvert, PrKeyframeEasing, PrLevels,
        PrMediaKind, PrMosaic, PrPropertyAnimation, PrRamp, PrScalarKeyframe, PrSourceEffects,
        PrVideoTrack, VideoCodec, GAUSSIAN_BLUR_BLURRINESS, STILL_INTRINSIC_TICKS, TICKS,
        TICKS_PER_MILLISECOND,
    },
    test_support::editable_document,
    tests::support::{
        current_blur_export, directional_blur, exported_blur, nest_of, sequence_of,
        transform_effect, video_media, video_sequence, DEFAULT_PR_TRANSFORM,
    },
    Omission, OmissionKind, OmissionScope,
};
use fx_schema::EditableFxCompositionDocument;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// An adjustment placement's source in-point on the 30 fps test sequences.
const IN_TICKS: i64 = FrameRate::Fps30.generator_in_ticks();

fn adjustment_media() -> (MediaId, PrMedia) {
    (
        MediaId("adjustment".into()),
        PrMedia {
            name: "Adjustment Layer".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: Some(crate::schema::PrVideoStream {
                pixel_aspect: Default::default(),
                interpretation: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                intrinsic_ticks: STILL_INTRINSIC_TICKS,
                frame_rate: (FrameRate::Fps30).into(),
                width: 1920,
                height: 1080,
                kind: PrMediaKind::Adjustment,
            }),
            audio: None,
        },
    )
}

fn adjustment_occurrence(start_secs: i64, end_secs: i64) -> PrVideoOccurrence {
    PrVideoOccurrence {
        id: None,
        media: MediaId("adjustment".into()),
        start_ticks: start_secs * TICKS,
        end_ticks: end_secs * TICKS,
        in_ticks: IN_TICKS,
        out_ticks: IN_TICKS + (end_secs - start_secs) * TICKS,
        playback_rate: 1.0,
        frame_blending: None,
        time_remap: None,
        linear_wipe: None,
        opacity: 100.0,
        blend_mode: crate::schema::PrBlendMode::Normal,
        transform: crate::schema::PrStaticTransform::default(),
        crop: crate::schema::PrStaticCrop::default(),
        opacity_mask: None,
        track_matte: None,
        animations: Vec::new(),
        enabled: true,
        effects: Vec::new(),
        effects_above_mask: 0,
        stroke: None,
        active_transforms: 0,
        source_effects: None,
    }
}

fn key(source_ticks: i64, value: f64, easing: PrKeyframeEasing) -> PrScalarKeyframe {
    PrScalarKeyframe {
        source_ticks,
        value,
        easing,
    }
}

fn levels(rgb: [f64; 5]) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Levels(PrLevels::Master { rgb }),
        animations: Vec::new(),
    }
}

fn gaussian_blur(enabled: bool, blurriness: f64, repeat_edge_pixels: bool) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness,
            repeat_edge_pixels,
        }),
        animations: Vec::new(),
    }
}

/// A static Corner Pin whose lower corners lean inwards by a tenth.
fn corner_pin(enabled: bool) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::CornerPin(PrCornerPin {
            corners: [[0.0, 0.0], [1.0, 0.0], [0.1, 1.0], [0.9, 1.0]],
        }),
        animations: Vec::new(),
    }
}

/// An Invert of every channel with Blend With Original `blend`, whose FX
/// form is a `levels` with complementary outputs (255 · blend / 100 and its
/// complement).
fn invert(blend: f64) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Invert(PrInvert { blend, channel: 0 }),
        animations: Vec::new(),
    }
}

/// A horizontal black-to-white Ramp from a quarter to three quarters of the
/// frame, whose FX form has the same fractions as frame UV: an adjustment's
/// frame is the canvas.
fn ramp() -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Ramp(PrRamp {
            start: [0.25, 0.5],
            start_colour: PrColour { rgb: [0, 0, 0] },
            end: [0.75, 0.5],
            end_colour: PrColour {
                rgb: [255, 255, 255],
            },
            blend: 0.0,
        }),
        animations: Vec::new(),
    }
}

/// `video_sequence` (V1: source 0–5 s) plus `adjustments`, one per upper track.
fn sequence_with_adjustments(adjustments: Vec<PrVideoOccurrence>) -> crate::format::PrSequence {
    let mut sequence = video_sequence();
    sequence.video_tracks.extend(
        adjustments
            .into_iter()
            .map(|clip| PrVideoTrack::media([clip])),
    );
    sequence
}

fn media() -> BTreeMap<MediaId, PrMedia> {
    let mut media = video_media();
    media.extend([adjustment_media()]);
    media
}

fn import(sequence: &crate::format::PrSequence) -> (Value, Vec<Omission>) {
    let media = media();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &media);
    assert_eq!(ids.len(), 1, "adjustment layers receive no asset ID");
    let mut omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    (document, omissions)
}

fn static_wipe() -> crate::schema::PrLinearWipe {
    crate::schema::PrLinearWipe {
        initial_completion: 50.0,
        completion: Vec::new(),
        angle_degrees: 90,
        feather: 0.0,
    }
}

#[test]
fn adjustment_wipe_preserves_a_matte_binding_across_its_stack_boundary() {
    let mut fill = crate::tests::support::clip_of("source", 0..TICKS, 0);
    fill.track_matte = Some(crate::schema::PrTrackMatte {
        track_index: 2,
        channel: crate::schema::PrMatteChannel::Alpha,
    });
    let mut adjustment = adjustment_occurrence(0, 1);
    adjustment.linear_wipe = Some(static_wipe());
    let sequence = sequence_of(
        "Cross-boundary matte",
        vec![
            PrVideoTrack::media([fill]),
            PrVideoTrack::media([adjustment]),
            PrVideoTrack::media([crate::tests::support::clip_of("source", 0..TICKS, 0)]),
        ],
    );
    let (document, import_omissions) = import(&sequence);
    let (project, omissions) = export_with_duration(document, 10000).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let fill = sequence
        .video_items()
        .filter_map(|item| item.media())
        .find(|clip| clip.track_matte.is_some())
        .expect("exported consumer survives");
    let matte = fill.track_matte.unwrap();
    let source = sequence.video_tracks[matte.track_index].clip(0);
    assert_eq!((fill.start_ticks, fill.end_ticks), (0, TICKS));
    assert_eq!((source.start_ticks, source.end_ticks), (0, TICKS));
    assert_eq!(fill.media, source.media);
    assert_eq!(matte.channel, crate::schema::PrMatteChannel::Alpha);
    assert_eq!(import_omissions.len(), 1, "{import_omissions:?}");
    assert!(
        import_omissions[0]
            .reason
            .contains("crosses its lower-composite boundary"),
        "{import_omissions:?}"
    );
}

#[test]
fn adjustment_wipe_preserves_the_picture_at_the_native_nesting_limit() {
    for count in [
        crate::schema::MAX_NEST_DEPTH,
        crate::schema::MAX_NEST_DEPTH + 1,
    ] {
        let mut wipes = Vec::new();
        for index in 0..count {
            let mut clip = adjustment_occurrence(0, 1);
            clip.start_ticks = index as i64 * TICKS / 10;
            clip.end_ticks = clip.start_ticks + TICKS / 10;
            clip.out_ticks = clip.in_ticks + TICKS / 10;
            clip.linear_wipe = Some(static_wipe());
            wipes.push(clip);
        }
        let sequence = sequence_of(
            "Depth boundary",
            vec![
                PrVideoTrack::media([crate::tests::support::clip_of("source", 0..TICKS, 0)]),
                PrVideoTrack::media(wipes),
            ],
        );
        let (document, import_omissions) = import(&sequence);
        let (project, omissions) = export_with_duration(document, 10000).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let mut sequence = project.single_sequence().unwrap();
        for _ in 0..crate::schema::MAX_NEST_DEPTH {
            sequence = &sequence
                .nest_occurrences()
                .next()
                .expect("admitted Wipe nest survives")
                .sequence;
        }
        let pictures: Vec<_> = sequence
            .video_items()
            .filter_map(|item| item.media())
            .filter(|clip| {
                matches!(
                    project.media(clip).unwrap().video.as_ref().unwrap().kind,
                    PrMediaKind::Video { .. }
                )
            })
            .collect();
        assert_eq!(pictures.len(), 1, "base picture survives all wrappers");
        assert_eq!((pictures[0].start_ticks, pictures[0].end_ticks), (0, TICKS));
        assert_eq!(
            import_omissions.len(),
            count - crate::schema::MAX_NEST_DEPTH,
            "{import_omissions:?}"
        );
        assert!(
            import_omissions
                .iter()
                .all(|omission| omission.reason.contains("native nesting limit")),
            "{import_omissions:?}"
        );
    }
}

#[test]
fn an_adjustments_source_effects_are_reported_and_its_own_effects_convert() {
    // Supplementary: Premiere's order for the source effects of an
    // adjustment clip's master clip is unmeasured, so they do not convert.
    let mut adjustment = adjustment_occurrence(1, 4);
    adjustment.effects = vec![levels([40.0, 220.0, 0.0, 255.0, 100.0])];
    adjustment.source_effects = Some(PrSourceEffects {
        master: "MasterClip:adjustment".to_owned(),
        effects: vec![gaussian_blur(true, 25.0, false)],
        active_transforms: 0,
    });
    let (document, omissions) = import(&sequence_with_adjustments(vec![adjustment]));
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "MasterClip:adjustment".into(),
            reason: crate::schema::SOURCE_CHAIN_NOT_CONVERTED.into(),
        }]
    );
    let adjustment = &document["composition"]["layers"][0];
    assert_eq!(adjustment["type"], "Adjustment");
    let types: Vec<_> = adjustment["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|effect| effect["effect"]["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["levels"]);
}

#[test]
fn an_adjustment_imports_as_an_fx_adjustment_layer_with_opacity_keys_and_effects() {
    use PrKeyframeEasing::{Hold, Linear};
    let mut adjustment = adjustment_occurrence(1, 4);
    adjustment.opacity = 60.0;
    adjustment.animations = vec![PrPropertyAnimation::Opacity(vec![
        key(IN_TICKS + TICKS, 60.0, Linear),
        key(IN_TICKS + 2 * TICKS, 20.0, Hold),
    ])];
    let mut keyed_blur = gaussian_blur(false, 10.0, true);
    keyed_blur.animations = vec![PrEffectParamAnimation {
        param: &GAUSSIAN_BLUR_BLURRINESS,
        keys: PrEffectParamKeys::Scalar(vec![
            key(IN_TICKS, 10.0, Linear),
            key(IN_TICKS + 3 * TICKS, 40.0, Linear),
        ]),
    }];
    // Stack order: Levels, an edge-transparent blur, the keyed repeat-edge
    // blur, a Directional Blur, a Corner Pin, an Invert, a Ramp, a Mosaic;
    // every one converts (Premiere shows black where an adjustment's result is
    // transparent, as FX does; the Invert takes its levels form; the Ramp's
    // frame is the canvas; the Mosaic's grid counts are fractions of it). A
    // Transform is reported, not dropped: FX ignores an adjustment layer's
    // geometric transform.
    adjustment.effects = vec![
        levels([40.0, 220.0, 0.0, 255.0, 100.0]),
        gaussian_blur(true, 25.0, false),
        keyed_blur,
        directional_blur(true, 45.0, 30.0),
        corner_pin(true),
        invert(30.0),
        ramp(),
        PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Mosaic(PrMosaic {
                horizontal: 16,
                vertical: 9,
                sharp_colors: true,
            }),
            animations: Vec::new(),
        },
        transform_effect(DEFAULT_PR_TRANSFORM, Vec::new()),
    ];
    adjustment.enabled = false;
    let (document, omissions) = import(&sequence_with_adjustments(vec![adjustment]));
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "adjustment".into(),
            reason: "Transform effect at stack position 9 was not imported: a Transform converts only as the transform of a media clip's stage group; FX has no transform effect, a still image imports no stage group, and an adjustment layer's geometric transform does not render".into(),
        }]
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .map(|layer| (
                layer["type"].as_str().unwrap(),
                layer["name"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("Adjustment", "Premiere adjustment 2"),
            ("Video", "Premiere video 1"),
            ("Rect", "Premiere black canvas"),
        ]
    );
    let layer = &layers[0];
    assert_eq!(layer["id"], 2);
    assert_eq!(layer["isHidden"], true);
    assert_eq!(
        (*crate::test_support::layer_range(layer)),
        json!({"start": 1000, "duration": 3000})
    );
    assert_eq!(layer["transform"]["opacity"], 60.0);
    assert_eq!(layer["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(layer["transform"]["position"], json!([0.0, 0.0]));
    assert_eq!(
        layer["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "levels", "inputBlack": 40.0, "inputWhite": 220.0, "gamma": 1.0, "outputBlack": 0.0, "outputWhite": 255.0}},
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
            {"id": 3, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 10.0, "repeatEdgePixels": true}},
            {"id": 4, "enabled": true, "effect": {"type": "directionalBlur", "direction": 45.0, "blurLength": 30.0}},
            {"id": 5, "enabled": true, "effect": {"type": "cornerPin",
                "upperLeftX": 0.0, "upperLeftY": 0.0, "upperRightX": 1.0, "upperRightY": 0.0,
                "lowerLeftX": 0.1, "lowerLeftY": 1.0, "lowerRightX": 0.9, "lowerRightY": 1.0}},
            {"id": 6, "enabled": true, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.0, "outputBlack": 178.5, "outputWhite": 76.5}},
            {"id": 7, "enabled": true, "effect": {"type": "gradientRamp",
                "startX": 0.25, "startY": 0.5, "endX": 0.75, "endY": 0.5,
                "startR": 0.0, "startG": 0.0, "startB": 0.0, "endR": 1.0, "endG": 1.0, "endB": 1.0,
                "blend": 1.0, "shape": 0.0}},
            {"id": 8, "enabled": true, "effect": {"type": "mosaic", "horizontalBlocks": 16.0, "verticalBlocks": 9.0, "sharpColors": true}},
        ])
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let targets: Vec<_> = entries
        .iter()
        .map(|entry| entry["target"].clone())
        .collect();
    assert_eq!(
        targets,
        [
            json!({"kind": "layer", "layerId": 2, "propertyType": "opacity"}),
            json!({"kind": "effectProperty", "effectId": 3, "paramName": "blurriness"}),
        ]
    );
    // Opacity keys move from the generator clock to the layer clock.
    let opacity_keys: Vec<_> = entries[0]["animator"]["keyframes"]
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
    assert_eq!(
        opacity_keys,
        [
            (1000, 60.0, "linear".to_owned()),
            (2000, 20.0, "hold".to_owned())
        ]
    );
}

#[test]
fn stacked_adjustments_keep_track_order_and_an_effectless_one_still_imports() {
    let mut lower = adjustment_occurrence(0, 5);
    lower.effects = vec![levels([40.0, 220.0, 0.0, 255.0, 100.0])];
    // Premiere keeps an adjustment whose effects were all removed.
    let upper = adjustment_occurrence(1, 2);
    let (document, omissions) = import(&sequence_with_adjustments(vec![lower, upper]));
    assert!(omissions.is_empty(), "{omissions:?}");
    let layers = document["composition"]["layers"].as_array().unwrap();
    let names: Vec<_> = layers
        .iter()
        .map(|layer| layer["name"].as_str().unwrap())
        .collect();
    // The upper track applies after the lower one: FX lists it first.
    assert_eq!(
        names,
        [
            "Premiere adjustment 3",
            "Premiere adjustment 2",
            "Premiere video 1",
            "Premiere black canvas"
        ]
    );
    assert_eq!(layers[0]["type"], "Adjustment");
    assert_eq!(layers[0]["effects"], Value::Null);
    assert_eq!(layers[1]["effects"].as_array().unwrap().len(), 1);
}

#[test]
fn an_adjustment_inside_a_nest_is_trimmed_with_the_inner_stack() {
    // Inner: source 0–5 s under an adjustment 1–4 s. Outer: V1 source 0–5 s,
    // V2 Inner placed at 2–5 s from inner 2 s.
    let mut inner_adjustment = adjustment_occurrence(1, 4);
    inner_adjustment.effects = vec![levels([0.0, 255.0, 0.0, 255.0, 150.0])];
    let inner = sequence_of(
        "Inner",
        sequence_with_adjustments(vec![inner_adjustment]).video_tracks,
    );
    let mut outer = video_sequence();
    outer.video_tracks.push(PrVideoTrack {
        items: Vec::new(),
        nests: vec![nest_of(inner, 2 * TICKS..5 * TICKS, 2 * TICKS)],
        transitions: Vec::new(),
    });
    let (document, omissions) = import(&outer);
    assert!(omissions.is_empty(), "{omissions:?}");
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers[0]["type"], "Group");
    let group_layers = layers[0]["layers"].as_array().unwrap();
    assert_eq!(
        group_layers
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Adjustment", "Video"]
    );
    // Inner 2–4 s of the adjustment shows, on the group clock from inner 2 s.
    assert_eq!(
        (*crate::test_support::layer_range(&group_layers[0])),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(group_layers[0]["parent"], layers[0]["id"]);
    assert_eq!(group_layers[0]["effects"][0]["effect"]["gamma"], 1.5);
}

/// The first adjustment layer in `layers` or a group inside them.
fn nested_adjustment(layers: &Value) -> Option<&Value> {
    layers
        .as_array()?
        .iter()
        .find_map(|layer| match layer["type"].as_str() {
            Some("Adjustment") => Some(layer),
            _ => nested_adjustment(&layer["layers"]),
        })
}

#[test]
fn an_adjustment_corner_pin_converts_only_in_a_sequence_of_the_document_canvas() {
    use crate::schema::{PrPointKeyframe, CORNER_PIN};
    use PrKeyframeEasing::Linear;
    // Premiere pins a Corner Pin's corners to the adjustment's own frame, its
    // sequence's canvas, and FX draws an adjustment layer's effects in the
    // document frame at any nesting depth. Each adjusted sequence holds the
    // source video under a 1–3 s adjustment, of the sequence's size as the
    // reader requires, whose Corner Pin shifts the frame right by an eighth,
    // (1/8, 0), (9/8, 0), (1/8, 1), (9/8, 1): by 80 pixels on a 640x360
    // canvas, by 160 on the 1280x720 document. Its upper-left corner is keyed
    // back to the identity over the first second, as the reader keys the
    // Corner Pin that it lowers a Geometry2 zoom to, and a keyed Gaussian
    // Blur follows it.
    let mut media = media();
    let mut adjusted = |[width, height]: [u32; 2]| {
        let (_, mut sized) = adjustment_media();
        let stream = sized.video.as_mut().unwrap();
        [stream.width, stream.height] = [width, height];
        let id = MediaId(format!("adjustment {width}x{height}"));
        media.insert(id.clone(), sized);
        let mut adjustment = adjustment_occurrence(1, 3);
        adjustment.media = id;
        let corner = |source_ticks: i64, value: [f64; 2]| PrPointKeyframe {
            source_ticks,
            value,
            easing: Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        };
        let mut blur = gaussian_blur(true, 10.0, false);
        blur.animations = vec![PrEffectParamAnimation {
            param: &GAUSSIAN_BLUR_BLURRINESS,
            keys: PrEffectParamKeys::Scalar(vec![
                key(IN_TICKS, 10.0, Linear),
                key(IN_TICKS + TICKS, 20.0, Linear),
            ]),
        }];
        adjustment.effects = vec![
            PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::CornerPin(PrCornerPin {
                    corners: [[0.125, 0.0], [1.125, 0.0], [0.125, 1.0], [1.125, 1.0]],
                }),
                animations: vec![PrEffectParamAnimation {
                    param: &CORNER_PIN.params[0],
                    keys: PrEffectParamKeys::Point(vec![
                        corner(IN_TICKS, [0.125, 0.0]),
                        corner(IN_TICKS + TICKS, [0.0, 0.0]),
                    ]),
                }],
            },
            blur,
        ];
        let mut sequence = sequence_of(
            "Adjusted",
            sequence_with_adjustments(vec![adjustment]).video_tracks,
        );
        [sequence.width, sequence.height] = [width, height];
        sequence
    };
    // A sequence of `canvas` that places `inner` at 0–3 s from its start.
    let nesting = |name: &str, [width, height]: [u32; 2], inner| {
        let mut sequence = sequence_of(
            name,
            vec![PrVideoTrack {
                items: Vec::new(),
                nests: vec![nest_of(inner, 0..3 * TICKS, 0)],
                transitions: Vec::new(),
            }],
        );
        [sequence.width, sequence.height] = [width, height];
        sequence
    };
    let document = [1280, 720];
    // A 640x360 sequence in the document, and one of the document's canvas
    // inside a 640x360 one, its own parent's canvas being another.
    let shifted = nesting("Outer", document, adjusted([640, 360]));
    let unshifted = nesting(
        "Outer",
        document,
        nesting("Middle", [640, 360], adjusted(document)),
    );
    let convert = |sequence: &crate::format::PrSequence| {
        let ids = crate::tesseract_output::asset_ids_in_order(sequence, &media);
        let mut omissions = Vec::new();
        let document =
            crate::convert::premiere_to_tesseract(sequence, &media, &ids, &mut omissions)
                .unwrap()
                .to_json_value()
                .unwrap();
        (document, omissions)
    };
    // The adjustment's effect types, and each keyed effect parameter by the
    // type of the adjustment effect that it keys ("none" for no such effect).
    let adjustment_effects = |document: &Value| {
        let adjustment = nested_adjustment(&document["composition"]["layers"]).unwrap();
        let effects = adjustment["effects"].as_array().unwrap();
        let effect_type = |id: &Value| {
            effects
                .iter()
                .find(|effect| &effect["id"] == id)
                .map_or("none", |effect| effect["effect"]["type"].as_str().unwrap())
        };
        let keyed: Vec<_> = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["target"]["kind"] == "effectProperty")
            .map(|entry| {
                let target = &entry["target"];
                (
                    effect_type(&target["effectId"]).to_owned(),
                    target["paramName"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let types: Vec<_> = effects
            .iter()
            .map(|effect| effect["effect"]["type"].as_str().unwrap().to_owned())
            .collect();
        (types, keyed)
    };
    let pair = |effect: &str, param: &str| (effect.to_owned(), param.to_owned());

    // FX would shift the 640x360 frame by 160 pixels: the Corner Pin and its
    // keys are omitted with the two frames, the video and the keyed blur stay.
    let (document, omissions) = convert(&shifted);
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "adjustment 640x360".into(),
            reason: "Corner Pin effect at stack position 1 was not imported: its picture's FX layer lays effects over 1280x720 pixels, and Premiere over the clip's 640x360 frame; a Corner Pin, Mosaic or Blur that repeats edge pixels converts only on a picture whose FX layer lays its effects over the clip's own frame, where Premiere applies them; FX lays a linked After Effects composition's group effects over the canvas".into(),
        }]
    );
    let group = &document["composition"]["layers"][0];
    assert!(group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|layer| layer["type"] == "Video"));
    assert_eq!(
        adjustment_effects(&document),
        (
            vec!["gaussianBlur".to_owned()],
            vec![pair("gaussianBlur", "blurriness")]
        )
    );

    // Two levels down, a sequence of the document's canvas keeps its Corner
    // Pin and the Pin's keys, though the sequence around it is smaller.
    let (document, omissions) = convert(&unshifted);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        adjustment_effects(&document),
        (
            vec!["cornerPin".to_owned(), "gaussianBlur".to_owned()],
            vec![
                pair("cornerPin", "upperLeftX"),
                pair("cornerPin", "upperLeftY"),
                pair("gaussianBlur", "blurriness"),
            ]
        )
    );
}

/// The one-clip document (source 0–1 s) with `adjustment` above the video.
fn document_with_adjustment(adjustment: Value) -> Value {
    let mut document = editable_document();
    document["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(1000);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, adjustment);
    document
}

fn adjustment_layer(effects: Value) -> Value {
    json!({
        "type": "Adjustment",
        "id": 3,
        "name": "Adjust",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {
            "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
            "rotation": 0, "opacity": 100
        },
        "effects": effects,
    })
}

fn export_with_omissions(document: Value) -> crate::error::Result<(PrProjectFile, Vec<Omission>)> {
    export_with_duration(document, 1000)
}

fn export_with_duration(
    document: Value,
    duration_ms: i64,
) -> crate::error::Result<(PrProjectFile, Vec<Omission>)> {
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let facts = BTreeMap::from([(
        "premiere-video-1".to_owned(),
        MediaFacts::Video(VideoMedia {
            pixel_aspect: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            codec: VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: 1920,
            height: 1080,
            timing: crate::media::VideoTiming::for_test(
                FrameRate::Fps30,
                duration_ms * TICKS_PER_MILLISECOND,
            ),
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
    )?;
    Ok((project, omissions))
}

/// `(track, kind, start s, end s)` of every occurrence of `sequence`.
fn placements_of(
    sequence: &crate::format::PrSequence,
    project: &PrProjectFile,
) -> Vec<(usize, PrMediaKind, i64, i64)> {
    sequence
        .video_tracks()
        .enumerate()
        .flat_map(|(track, items)| {
            items.iter().map(move |item| {
                let clip = item.media().unwrap();
                let kind = project.media(clip).unwrap().video.as_ref().unwrap().kind;
                (
                    track,
                    kind,
                    clip.start_ticks / TICKS,
                    clip.end_ticks / TICKS,
                )
            })
        })
        .collect()
}

fn placements(project: &PrProjectFile) -> Vec<(usize, PrMediaKind, i64, i64)> {
    placements_of(project.single_sequence().unwrap(), project)
}

#[test]
fn an_fx_adjustment_layer_exports_as_a_flagged_black_video_clip_and_rereads() {
    let mut wire = document_with_adjustment(adjustment_layer(json!([
        {"id": 1, "enabled": true, "effect": {"type": "levels", "inputBlack": 40.0, "inputWhite": 220.0, "gamma": 1.0, "outputBlack": 0.0, "outputWhite": 255.0}},
        {"id": 2, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 10.0, "repeatEdgePixels": true}},
        {"id": 3, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
        {"id": 4, "enabled": true, "effect": {"type": "directionalBlur", "direction": 45.0, "blurLength": 30.0}},
        {"id": 5, "enabled": false, "effect": {"type": "cornerPin",
            "upperLeftX": 0.0, "upperLeftY": 0.0, "upperRightX": 1.0, "upperRightY": 0.0,
            "lowerLeftX": 0.1, "lowerLeftY": 1.0, "lowerRightX": 0.9, "lowerRightY": 1.0}},
        {"id": 6, "enabled": true, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.0, "outputBlack": 178.5, "outputWhite": 76.5}},
        {"id": 7, "enabled": true, "effect": {"type": "gradientRamp",
            "startX": 0.25, "startY": 0.5, "endX": 0.75, "endY": 0.5,
            "startR": 0.0, "startG": 0.0, "startB": 0.0, "endR": 1.0, "endG": 1.0, "endB": 1.0,
            "blend": 1.0, "shape": 0.0}},
    ])));
    wire["composition"]["layers"][0]["transform"]["opacity"] = json!(60.0);
    wire["composition"]["layers"][0]["isHidden"] = json!(true);
    wire["composition"]["dynamics"] = json!({"entries": [
        {
            "target": {"kind": "layer", "layerId": 3, "propertyType": "opacity"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "a", "layerTime": 250, "value": {"type": "float", "value": 60.0}, "easing": {"type": "linear"}},
                {"id": "b", "layerTime": 750, "value": {"type": "float", "value": 20.0}, "easing": {"type": "hold"}}
            ]}
        },
        {
            "target": {"kind": "effectProperty", "effectId": 2, "paramName": "blurriness"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                {"id": "c", "layerTime": 0, "value": {"type": "float", "value": 10.0}, "easing": {"type": "linear"}},
                {"id": "d", "layerTime": 1000, "value": {"type": "float", "value": 40.0}, "easing": {"type": "linear"}}
            ]}
        }
    ]});
    let (mut project, omissions) = export_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        placements(&project),
        [
            (
                0,
                PrMediaKind::Video {
                    codec: Some(VideoCodec::H264),
                    hdr_profile: None,
                },
                0,
                1
            ),
            (1, PrMediaKind::Adjustment, 0, 1),
        ]
    );
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_tracks().nth(1).unwrap()[0]
        .media()
        .unwrap()
        .clone();
    assert_eq!(clip.source_ticks(), IN_TICKS..IN_TICKS + TICKS);
    assert_eq!(clip.opacity, 60.0);
    assert!(!clip.enabled);
    assert_eq!(
        clip.animations,
        [PrPropertyAnimation::Opacity(vec![
            key(IN_TICKS + TICKS / 4, 60.0, PrKeyframeEasing::Linear),
            key(IN_TICKS + 3 * TICKS / 4, 20.0, PrKeyframeEasing::Hold),
        ])]
    );
    assert_eq!(clip.effects.len(), 7);
    assert_eq!(clip.effects[5].params, invert(30.0).params);
    // The gradientRamp exports on the canvas frame of the identity host.
    assert_eq!(clip.effects[6].params, ramp().params);
    assert_eq!(
        clip.effects[0].params,
        PrEffectParams::Levels(PrLevels::Master {
            rgb: [40.0, 220.0, 0.0, 255.0, 100.0]
        })
    );
    assert!(!clip.effects[1].enabled);
    assert_eq!(clip.effects[1].animations.len(), 1);
    // The edge-transparent blur exports; Premiere shows black at its edges.
    assert_eq!(
        clip.effects[2].params,
        exported_blur(true, 25.0, false).params
    );
    // The other edge-transparent effects export on the identity host too; the
    // bypassed Corner Pin keeps its corners, and the invert-form levels
    // returns as an Invert.
    assert_eq!(
        clip.effects[3].params,
        current_blur_export(directional_blur(true, 45.0, 30.0)).params
    );
    assert!(clip.effects[3].enabled && !clip.effects[4].enabled);
    assert_eq!(clip.effects[4].params, corner_pin(false).params);
    let media = project.media(&clip).unwrap();
    assert_eq!(media.name(), "Adjustment Layer");
    assert!(media.is_adjustment() && media.is_generator());

    // The written project carries both flags and rereads as an adjustment.
    let video = project
        .media
        .get_mut(&MediaId("premiere-video-1".into()))
        .unwrap();
    video.name = "source.mp4".into();
    video.relative_path = Some("./media/source.mp4".into());
    video.relative_paths = vec!["./media/source.mp4".into()];
    video.absolute_paths = vec![(MediaPathField::FilePath, "/media/source.mp4".into())];
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let xml = crate::format::read_xml(&path).unwrap();
    assert_eq!(
        xml.matches("<AdjustmentLayer>true</AdjustmentLayer>")
            .count(),
        1
    );
    assert_eq!(
        xml.matches("<IsAdjustmentLayer>true</IsAdjustmentLayer>")
            .count(),
        1
    );
    assert!(
        xml.contains("<FilePath>1112293707</FilePath>")
            && xml.contains("<Title>Black Video</Title>")
    );
    let (reread, omissions) = PrProjectFile::load(&path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        placements(&reread),
        [
            (
                0,
                PrMediaKind::Video {
                    codec: None,
                    hdr_profile: None
                },
                0,
                1
            ),
            (1, PrMediaKind::Adjustment, 0, 1),
        ]
    );
    let reread_clip = reread
        .single_sequence()
        .unwrap()
        .video_tracks()
        .nth(1)
        .unwrap()[0]
        .media()
        .unwrap();
    assert_eq!(reread_clip.animations, clip.animations);
    assert_eq!(reread_clip.effects, clip.effects);
    assert_eq!(reread_clip.opacity, 60.0);
    assert!(!reread_clip.enabled);
}

#[test]
fn a_blended_adjustment_keeps_its_blend_both_ways() {
    use crate::schema::PrBlendMode;
    let reports = |omissions: &[Omission]| -> Vec<OmissionKind> {
        omissions
            .iter()
            .filter(|omission| omission.reason.contains("Blend Mode (4, 7)"))
            .map(|omission| omission.kind)
            .collect()
    };
    // Import: the effected composite blends as Darker Color over the
    // untouched one at 60%.
    let mut adjustment = adjustment_occurrence(0, 5);
    (adjustment.blend_mode, adjustment.opacity) = (PrBlendMode::DarkerColor, 60.0);
    adjustment.effects = vec![levels([40.0, 220.0, 1.0, 0.0, 255.0])];
    let (document, omissions) = import(&sequence_with_adjustments(vec![adjustment]));
    let layer = &document["composition"]["layers"][0];
    assert_eq!(
        (
            &layer["type"],
            &layer["blendMode"],
            &layer["transform"]["opacity"]
        ),
        (&json!("Adjustment"), &json!("darkerColor"), &json!(60.0))
    );
    assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
    // Export of an authored Darker Color adjustment writes the pair on its clip.
    let mut layer = adjustment_layer(json!([
        {"id": 1, "enabled": true, "effect": {"type": "levels", "inputBlack": 40.0, "inputWhite": 220.0, "gamma": 1.0, "outputBlack": 0.0, "outputWhite": 255.0}},
    ]));
    layer["blendMode"] = json!("darkerColor");
    let (project, omissions) = export_with_omissions(document_with_adjustment(layer)).unwrap();
    let blends: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .map(|clip| {
            (
                project.media(clip).unwrap().is_adjustment(),
                clip.blend_mode,
            )
        })
        .collect();
    assert_eq!(
        blends,
        [
            (false, PrBlendMode::Normal),
            (true, PrBlendMode::DarkerColor)
        ]
    );
    assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
}

#[test]
fn fx_adjustment_layers_that_premiere_cannot_carry_are_omitted_or_reported() {
    let levels = json!([
        {"id": 1, "enabled": true, "effect": {"type": "levels", "inputBlack": 40.0, "inputWhite": 220.0, "gamma": 1.0, "outputBlack": 0.0, "outputWhite": 255.0}},
    ]);
    let record = "layer 3 (\"Adjust\")";
    // Omitted whole: the FX mask gate has no Premiere counterpart.
    let mut wire = document_with_adjustment(adjustment_layer(levels.clone()));
    wire["composition"]["layers"][0]["masks"] =
        json!([{"id": 11, "mode": "add", "layer": 1, "feather": [0.0, 0.0]}]);
    let (project, omissions) = export_with_omissions(wire).unwrap();
    assert_eq!(
        placements(&project),
        [(
            0,
            PrMediaKind::Video {
                codec: Some(VideoCodec::H264),
                hdr_profile: None,
            },
            0,
            1
        )]
    );
    assert!(
        omissions.contains(&Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record: record.into(),
            reason:
                "adjustment layer was not exported: masks on an adjustment layer are not exported"
                    .into(),
        }),
        "{omissions:?}"
    );
    // A matted adjustment is omitted, and so is its matte source, which FX
    // draws only through the layers that it keys: nothing is left to export.
    let mut wire = document_with_adjustment(adjustment_layer(levels.clone()));
    wire["composition"]["layers"][0]["trackMatte"] = json!({"layer": 1, "type": "alpha"});
    let error = export_with_omissions(wire).unwrap_err().to_string();
    for reason in [
        "layer 3 (\"Adjust\"): adjustment layer was not exported: a track matte on an adjustment layer is not exported",
        "layer 1 (\"Source\"): track matte source of no exported clip was not exported",
    ] {
        assert!(error.contains(reason), "{error}");
    }
    // Exported at default Motion: the FX transform is an FX no-op.
    for (case, edit, reason) in [
        (
            "moved transform",
            (|layer: &mut Value| layer["transform"]["scale"] = json!([50.0, 50.0])) as fn(&mut Value),
            "adjustment layer Motion was not exported: FX ignores an adjustment layer's geometric transform, which has no Premiere counterpart; default Motion was written",
        ),
        (
            "description",
            |layer| layer["description"] = json!("why"),
            "description was not exported",
        ),
    ] {
        let mut wire = document_with_adjustment(adjustment_layer(levels.clone()));
        edit(&mut wire["composition"]["layers"][0]);
        let (project, omissions) = export_with_omissions(wire).unwrap();
        assert_eq!(placements(&project).len(), 2, "{case}");
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: record.into(),
                reason: reason.into(),
            }],
            "{case}"
        );
        let clip = project.single_sequence().unwrap().video_tracks().nth(1).unwrap()[0]
            .media()
            .unwrap();
        assert_eq!(clip.transform, crate::schema::PrStaticTransform::default(), "{case}");
    }
    // A canvas-centred pivot moves nothing and is not reported.
    let mut wire = document_with_adjustment(adjustment_layer(levels.clone()));
    wire["composition"]["layers"][0]["transform"]["anchorPoint"] = json!([960.0, 540.0]);
    wire["composition"]["layers"][0]["transform"]["position"] = json!([960.0, 540.0]);
    let (_, omissions) = export_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Geometric keys are reported once each; the layer still exports.
    let mut wire = document_with_adjustment(adjustment_layer(levels));
    wire["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "layer", "layerId": 3, "propertyType": "rotation"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "a", "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "linear"}},
            {"id": "b", "layerTime": 1000, "value": {"type": "float", "value": 90.0}, "easing": {"type": "linear"}}
        ]}
    }]});
    let (project, omissions) = export_with_omissions(wire).unwrap();
    assert_eq!(placements(&project).len(), 2);
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: record.into(),
            reason: "Rotation animation was not exported: FX ignores an adjustment layer's geometric transform, which has no Premiere counterpart".into(),
        }]
    );
}

#[test]
fn an_fx_adjustment_layer_inside_a_group_exports_inside_the_nest() {
    let mut wire = document_with_adjustment(adjustment_layer(json!([
        {"id": 1, "enabled": true, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.5, "outputBlack": 0.0, "outputWhite": 255.0}},
    ])));
    // Group the adjustment with the video; the canvas stays at the root.
    let layers = wire["composition"]["layers"].as_array_mut().unwrap();
    let mut adjustment = layers.remove(0);
    let mut video = layers.remove(0);
    adjustment["parent"] = json!(9);
    video["parent"] = json!(9);
    layers.insert(
        0,
        json!({
            "type": "Group",
            "id": 9,
            "name": "Nest",
            "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 1000}), json!({"start": 0, "duration": 1000})),
            "transform": {
                "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
                "rotation": 0, "opacity": 100
            },
            "layers": [adjustment, video],
        }),
    );
    let (project, omissions) = export_with_omissions(wire).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert!(placements(&project).is_empty());
    let nest = sequence.nest_occurrences().next().unwrap();
    assert_eq!(
        placements_of(&nest.sequence, &project),
        [
            (
                0,
                PrMediaKind::Video {
                    codec: Some(VideoCodec::H264),
                    hdr_profile: None,
                },
                0,
                1
            ),
            (1, PrMediaKind::Adjustment, 0, 1),
        ]
    );
}

#[test]
fn an_adjustment_covers_no_gap_while_a_disabled_clip_still_ends_the_timeline() {
    let media = media();
    for (case, video_enabled, adjustment, timeline_end_secs, expected) in [
        (
            "adjustment over the picture",
            true,
            adjustment_occurrence(0, 5),
            5,
            None,
        ),
        (
            "adjustment past the picture",
            true,
            adjustment_occurrence(0, 7),
            7,
            Some(5 * TICKS..7 * TICKS),
        ),
        (
            "adjustment over a disabled picture",
            false,
            adjustment_occurrence(0, 5),
            5,
            Some(0..5 * TICKS),
        ),
        (
            "disabled adjustment past the picture",
            true,
            PrVideoOccurrence {
                enabled: false,
                ..adjustment_occurrence(0, 7)
            },
            7,
            Some(5 * TICKS..7 * TICKS),
        ),
    ] {
        let mut sequence = sequence_with_adjustments(vec![adjustment]);
        sequence.video_tracks[0].clip_mut(0).enabled = video_enabled;
        sequence.timeline_end_ticks = timeline_end_secs * TICKS;
        sequence.validate_timeline(&media).unwrap();
        assert_eq!(sequence.gaps(&media), Vec::from_iter(expected), "{case}");
        assert_eq!(sequence.end_ticks(), timeline_end_secs * TICKS, "{case}");
    }
}

#[test]
fn an_exported_adjustment_preserves_gaps_without_requiring_a_black_canvas() {
    let levels = json!([
        {"id": 1, "enabled": true, "effect": {"type": "levels", "inputBlack": 0.0, "inputWhite": 255.0, "gamma": 1.5, "outputBlack": 0.0, "outputWhite": 255.0}},
    ]);
    // The video covers 0–1 s of a 2 s document; the adjustment covers all of it.
    let mut two_seconds = document_with_adjustment(adjustment_layer(levels));
    two_seconds["duration"] = json!(2.0);
    two_seconds["composition"]["layers"][0]["activeRange"] = json!({"start": 0, "duration": 2000});
    let mut leading_gap = two_seconds.clone();
    leading_gap["composition"]["layers"][1]["playback"] = crate::test_support::linear_playback(
        json!({"start": 1000, "duration": 1000}),
        json!({"start": 0, "duration": 1000}),
    );
    let mut canvas = two_seconds.clone();
    canvas["composition"]["layers"][2]["activeRange"] = json!({"start": 0, "duration": 2000});
    for (case, mut wire, video_start, video_end, gap) in [
        ("uncovered tail", two_seconds, 0, 1, TICKS..2 * TICKS),
        ("leading gap", leading_gap, 1, 2, 0..TICKS),
        ("explicit canvas", canvas, 0, 1, TICKS..2 * TICKS),
    ] {
        if case != "explicit canvas" {
            // The adjustment has no picture of its own over the gap.
            wire["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .remove(2);
        }
        let (project, omissions) = export_with_omissions(wire).unwrap();
        assert_eq!(
            placements(&project),
            [
                (
                    0,
                    PrMediaKind::Video {
                        codec: Some(VideoCodec::H264),
                        hdr_profile: None
                    },
                    video_start,
                    video_end
                ),
                (1, PrMediaKind::Adjustment, 0, 2),
            ],
            "{case}"
        );
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.end_ticks(), 2 * TICKS, "{case}");
        assert_eq!(sequence.gaps(&project.media), [gap], "{case}");
    }
}

#[test]
fn adjustment_coverage_guide_shares_nested_parent_and_trimmed_range() {
    let mut adjustment = adjustment_occurrence(1, 4);
    adjustment.transform.scale = [50.0; 2];
    adjustment.effects = vec![invert(0.0)];
    let inner = sequence_of(
        "Inner",
        sequence_with_adjustments(vec![adjustment]).video_tracks,
    );
    let mut outer = video_sequence();
    outer.video_tracks.push(PrVideoTrack {
        items: Vec::new(),
        nests: vec![nest_of(inner, 2 * TICKS..5 * TICKS, 2 * TICKS)],
        transitions: Vec::new(),
    });
    let (document, omissions) = import(&outer);
    assert!(omissions.is_empty(), "{omissions:?}");
    let group = &document["composition"]["layers"][0];
    let children = group["layers"].as_array().unwrap();
    let adjustment = children
        .iter()
        .find(|layer| layer["type"] == "Adjustment")
        .unwrap();
    let mask = &adjustment["masks"][0];
    let guide = children
        .iter()
        .find(|layer| layer["id"] == mask["layer"])
        .unwrap();
    assert_eq!(
        (*crate::test_support::layer_range(adjustment)),
        json!({"start": 0, "duration": 2000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(guide)),
        (*crate::test_support::layer_range(adjustment))
    );
    assert_eq!(guide["parent"], group["id"]);
    assert_eq!(adjustment["parent"], group["id"]);
    assert_eq!(guide["transform"]["scale"], json!([50.0, 50.0]));
    assert_eq!(adjustment["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(guide["rect"]["fillEnabled"], false);
}

#[test]
fn adjustment_coverage_export_rejects_nonuniform_geometry_and_unmeasured_effects() {
    let mut native = adjustment_occurrence(0, 1);
    native.transform.scale = [50.0; 2];
    native.effects = vec![invert(0.0)];
    let (imported, omissions) = import(&sequence_with_adjustments(vec![native]));
    assert!(omissions.is_empty(), "{omissions:?}");
    let layers = imported["composition"]["layers"].as_array().unwrap();
    let mut adjustment = layers
        .iter()
        .find(|layer| layer["type"] == "Adjustment")
        .unwrap()
        .clone();
    let mut guide = layers
        .iter()
        .find(|layer| layer["id"] == adjustment["masks"][0]["layer"])
        .unwrap()
        .clone();
    adjustment["id"] = json!(3);
    adjustment["masks"][0]["id"] = json!(5);
    adjustment["masks"][0]["layer"] = json!(4);
    guide["id"] = json!(4);
    let mut document = document_with_adjustment(adjustment);
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, guide);
    let (exported, omissions) = export_with_omissions(document.clone()).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(exported.single_sequence().unwrap().video_items().count(), 2);
    // A guide never paints, on either side of its adjustment in layer order.
    let mut reordered = document.clone();
    reordered["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    let (reordered, omissions) = export_with_omissions(reordered).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        reordered.single_sequence().unwrap().video_items().count(),
        2
    );

    let mut invalid_reference = document.clone();
    let layers = invalid_reference["composition"]["layers"]
        .as_array_mut()
        .unwrap();
    layers.remove(1);
    let video_id = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap()["id"]
        .clone();
    layers[0]["masks"][0]["layer"] = video_id;
    let (retained, omissions) = export_with_omissions(invalid_reference).unwrap();
    assert_eq!(retained.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("masks on an adjustment layer are not exported")),
        "{omissions:?}"
    );

    document["composition"]["layers"][1]["transform"]["scale"] = json!([50.0, 75.0]);
    let (rejected, omissions) = export_with_omissions(document.clone()).unwrap();
    assert_eq!(rejected.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("positive uniform")),
        "{omissions:?}"
    );

    document["composition"]["layers"][1]["transform"]["scale"] = json!([50.0, 50.0]);
    document["composition"]["layers"][0]["effects"] = json!([
        {"type": "gaussianBlur", "blurriness": 25.0}
    ]);
    let (rejected, omissions) = export_with_omissions(document).unwrap();
    assert_eq!(rejected.single_sequence().unwrap().video_items().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.reason.contains("static full RGB Invert")),
        "{omissions:?}"
    );
}

#[test]
fn adjustment_coverage_and_video_crop_allocate_distinct_mask_guides() {
    let mut adjustment = adjustment_occurrence(1, 4);
    adjustment.transform.scale = [50.0; 2];
    let mut sequence = sequence_with_adjustments(vec![adjustment]);
    let crate::schema::PrVideoItem::Media(video) = &mut sequence.video_tracks[0].items[0] else {
        unreachable!()
    };
    video.crop.left = 10.0;
    let (document, omissions) = import(&sequence);
    assert!(omissions.is_empty(), "{omissions:?}");
    let layers = document["composition"]["layers"].as_array().unwrap();
    let ids = layers
        .iter()
        .map(|layer| layer["id"].as_u64().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), layers.len());
    let masks = layers
        .iter()
        .flat_map(|layer| layer["masks"].as_array().into_iter().flatten())
        .collect::<Vec<_>>();
    assert_eq!(masks.len(), 2);
    assert_ne!(masks[0]["id"], masks[1]["id"]);
    assert_ne!(masks[0]["layer"], masks[1]["layer"]);
    assert!(masks
        .iter()
        .all(|mask| ids.contains(&mask["layer"].as_u64().unwrap())));
}
