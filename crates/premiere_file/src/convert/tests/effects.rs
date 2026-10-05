#[path = "effects/posterize_time.rs"]
mod posterize_time;

use crate::{
    format::{FrameRate, PrProjectFile},
    media::{MediaFacts, VideoMedia},
    schema::{
        PrBrightnessContrast, PrColour, PrColourKeyframe, PrCornerPin, PrEffect,
        PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, PrFilmImpactBlur,
        PrFilmImpactDirectionalBlur, PrGaussianBlur, PrInvert, PrKeyframeEasing, PrLevels,
        PrMosaic, PrPointKeyframe, PrPosterize, PrRamp, PrReplicate, PrScalarKeyframe,
        PrSourceEffects, PrTint, PrVideoItem, PrVideoTrack, BRIGHTNESS_CONTRAST_BRIGHTNESS,
        BRIGHTNESS_CONTRAST_CONTRAST, CORNER_PIN, FILM_IMPACT_BLUR_AMOUNT, INVERT_BLEND, LEVELS,
        MOSAIC_HORIZONTAL_BLOCKS, MOSAIC_VERTICAL_BLOCKS, POSTERIZE_LEVEL, RAMP_BLEND, RAMP_END,
        RAMP_START_COLOR, REPLICATE_COUNT, TICKS, TICKS_PER_MILLISECOND, TINT_AMOUNT,
        TINT_MAP_BLACK_TO, TINT_MAP_WHITE_TO,
    },
    test_support::editable_document,
    tests::support::{
        amount, clip_of, current_blur_export, directional_blur, exported_blur, keyed_directional,
        left_crop, opacity_mask, project_document, sequence_of, transform_effect, video_media,
        video_sequence, DEFAULT_PR_TRANSFORM,
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
        mask: None,
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
        mask: None,
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
            pixel_aspect: Default::default(),
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

fn legacy_luma_xml() -> String {
    let fragment = include_str!("../../../tests/fixtures/legacy-luma-key.xml")
        .replace("<PremiereData>", "<PremiereData Version=\"3\">");
    find_edges_native_xml(&fragment).replace("ObjectRef=\"537\"", "ObjectRef=\"543\"")
}

#[test]
fn legacy_luma_key_import_and_edited_export_preserve_controls_and_keys() {
    let (original, notes) = find_edges_import(&legacy_luma_xml());
    assert_eq!(
        original["composition"]["layers"][0]["effects"][0]["effect"],
        json!({"type":"lumaKey", "threshold":0.4, "softness":0.2, "invert":0.0})
    );
    assert_eq!(
        original["composition"]["layers"][0]["source"]["assetId"],
        "premiere-video-1"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.kind == OmissionKind::Approximated
                && n.reason.contains("Threshold transparency")),
        "{notes:?}"
    );
    // Bypass and zero falloff are synthetic variants of the pinned controls.
    let (bypassed, _) = find_edges_import(
        &legacy_luma_xml().replace("<ID>3</ID>", "<ID>3</ID><Bypass>true</Bypass>"),
    );
    assert_eq!(
        bypassed["composition"]["layers"][0]["effects"][0]["enabled"],
        false
    );
    let (zero, notes) = find_edges_import(
        &legacy_luma_xml()
            .replace(",20.,0,0,0,0,0,0", ",0.,0,0,0,0,0,0")
            .replace(
                "<CurrentValue>20</CurrentValue>",
                "<CurrentValue>0</CurrentValue>",
            ),
    );
    assert_eq!(
        zero["composition"]["layers"][0]["effects"][0]["effect"]["softness"],
        0.0001
    );
    assert!(notes
        .iter()
        .any(|n| n.reason.contains("equal smoothstep edges")));
    let document = EditableFxCompositionDocument::from_json_value(original.clone()).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].1, "threshold");
    assert_eq!(
        tracks[0]
            .2
            .iter()
            .map(|k| (k.1, k.2, k.3))
            .collect::<Vec<_>>(),
        vec![
            (0, 0.4, PropertyKeyframeEasing::Linear),
            (2412, 0.7, PropertyKeyframeEasing::Linear)
        ]
    );
    for enabled in [true, false] {
        let mut wire = original.clone();
        wire["composition"]["layers"][0]["effects"][0]["enabled"] = json!(enabled);
        wire["composition"]["layers"][0]["effects"][0]["effect"]["softness"] = json!(0.15);
        let entries = &mut wire["composition"]["dynamics"]["entries"];
        entries[0]["animator"]["keyframes"] = json!([
            fx_key("edited-0", 0, 0.25, json!({"type":"linear"})),
            fx_key("edited-1", 2000, 0.6, json!({"type":"linear"}))
        ]);
        let (mut project, notes) = export(wire);
        assert_eq!(exported_effects(&project).len(), 1, "{notes:?}");
        assert!(
            notes.iter().any(|n| n.kind == OmissionKind::Approximated),
            "{notes:?}"
        );
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
        let tree = roxmltree::Document::parse(&xml).unwrap();
        let native = tree
            .descendants()
            .find(|n| {
                n.has_tag_name("VideoFilterComponent")
                    && n.descendants().any(|c| {
                        c.has_tag_name("MatchName") && c.text() == Some("AE.ADBE Legacy Key Luma")
                    })
            })
            .unwrap();
        assert_eq!(
            native
                .descendants()
                .find(|n| n.has_tag_name("Bypass"))
                .and_then(|n| n.text()),
            (!enabled).then_some("true")
        );
        let refs: Vec<_> = native
            .descendants()
            .filter(|n| n.has_tag_name("Param"))
            .map(|n| n.attribute("ObjectRef").unwrap())
            .collect();
        assert_eq!(refs.len(), 2);
        for (id, name, value, object) in [
            ("1", "Threshold", 25.0, refs[0]),
            ("2", "Cutoff", 15.0, refs[1]),
        ] {
            let param = tree
                .descendants()
                .find(|n| n.attribute("ObjectID") == Some(object))
                .unwrap();
            let text = |tag| {
                param
                    .children()
                    .find(|n| n.has_tag_name(tag))
                    .and_then(|n| n.text())
                    .unwrap()
            };
            assert_eq!(text("ParameterID"), id);
            assert_eq!(text("Name"), name);
            assert_eq!(text("LowerBound"), "0");
            assert_eq!(text("UpperBound"), "100");
            assert_eq!(
                text("StartKeyframe")
                    .split(',')
                    .nth(1)
                    .unwrap()
                    .parse::<f64>()
                    .unwrap(),
                value
            );
            if id == "1" {
                let keys: Vec<_> = text("Keyframes")
                    .split(';')
                    .filter(|k| !k.is_empty())
                    .map(|k| {
                        let mut parts = k.split(',');
                        (
                            parts.next().unwrap().parse::<i64>().unwrap(),
                            parts.next().unwrap().parse::<f64>().unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(keys, vec![(0, 25.0), (2 * TICKS, 60.0)]);
            }
        }
        let (roundtrip, _) = find_edges_import(&xml);
        assert_eq!(
            roundtrip["composition"]["layers"][0]["effects"][0]["enabled"],
            enabled
        );
        assert_eq!(
            roundtrip["composition"]["layers"][0]["effects"][0]["effect"]["softness"],
            0.15
        );
    }
}

#[test]
fn legacy_luma_key_boundary_and_unrepresentable_edits_are_explicit() {
    for (payload, expected) in [
        (json!({"type":"lumaKey","softness":0.0}), Some(0.01)),
        (json!({"type":"lumaKey","invert":1.0}), None),
        (json!({"type":"lumaKey","threshold":1.1}), None),
    ] {
        for enabled in [true, false] {
            let (project, notes) = export(document_with_effects(json!([
                {"id":1,"enabled":enabled,"effect":payload},
                {"id":2,"effect":{"type":"gaussianBlur","blurriness":7.0}}
            ])));
            let effects = exported_effects(&project);
            assert_eq!(effects.len(), if expected.is_some() { 2 } else { 1 });
            assert_eq!(
                notes
                    .iter()
                    .any(|n| n.reason.contains("omitting the enabled key")),
                enabled && expected.is_none(),
                "{notes:?}"
            );
            assert!(notes.iter().any(|n|n.reason.contains("stack position 1") || n.reason.contains("effect 1")), "{notes:?}");
            if let Some(expected) = expected {
                let PrEffectParams::LegacyLuma { cutoff, .. } = effects[0].params else {
                    panic!("missing key")
                };
                assert_eq!(cutoff, expected);
                assert!(notes
                    .iter()
                    .any(|n| n.reason.contains("equal smoothstep edges")));
            }
        }
    }
}

#[test]
fn legacy_luma_key_color_matte_retains_key_and_failed_keys_omit_only_the_occurrence() {
    let (mut native, _) =
        crate::format::inspect_project_with_omissions(&legacy_luma_xml(), None).unwrap();
    let mut sequence = native.single_sequence().unwrap().clone();
    // Supplementary host mutation: the authored controls stay unchanged.
    let media = native
        .media
        .values_mut()
        .next()
        .unwrap()
        .video
        .as_mut()
        .unwrap();
    media.kind = crate::schema::PrMediaKind::ColorMatte(crate::schema::PrColorMatte {
        rgb: [128, 128, 128],
    });
    media.intrinsic_ticks = crate::schema::color_matte::COLOR_MATTE_INTRINSIC_TICKS;
    let (wire, notes) = imported_with_omissions(&sequence, &native.media);
    let matte = wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| {
            l["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("Premiere color matte"))
        })
        .unwrap();
    assert_eq!(
        matte["effects"][0]["effect"],
        json!({"type":"lumaKey","threshold":0.4,"softness":0.2,"invert":0.0})
    );
    assert!(notes
        .iter()
        .any(|n| n.reason.contains("Color Matte retains Legacy Luma")));

    // Two native keys round to one FX millisecond: record import fails. The
    // coverage owner must omit that occurrence, not leave an opaque rectangle.
    let mut safe = sequence.video_tracks[0].clip(0).clone();
    safe.id = Some("safe-sibling".to_owned());
    safe.effects.clear();
    safe.start_ticks += 5 * TICKS;
    safe.end_ticks += 5 * TICKS;
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(safe));
    let PrEffectParamKeys::Scalar(keys) =
        &mut sequence.video_tracks[0].clip_mut(0).effects[0].animations[0].keys
    else {
        panic!("scalar")
    };
    keys[1].source_ticks = 1;
    let (wire, notes) = imported_with_omissions(&sequence, &native.media);
    let mattes: Vec<_> = wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| {
            l["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("Premiere color matte"))
        })
        .collect();
    assert_eq!(mattes.len(), 1);
    assert!(mattes[0].get("effects").is_none());
    assert!(
        notes.iter().any(|n| n.scope == OmissionScope::Occurrence
            && n.reason.contains("AE.ADBE Legacy Key Luma")
            && n.reason.contains("stack position 1")
            && n.reason.contains("coverage would change")),
        "{notes:?}"
    );
}

#[test]
fn legacy_luma_key_bezier_export_preserves_values_times_and_hold() {
    let mut wire =
        document_with_effects(json!([{"id":1,"effect":{"type":"lumaKey","invert":0.5}}]));
    wire["composition"]["dynamics"] = json!({"entries":[{
    "target":{"kind":"effectProperty","effectId":1,"paramName":"softness"},
    "animator":{"type":"keyframes","enabled":true,"keyframes":[
        fx_key("a",0,0.0,json!({"type":"linear"})),
        fx_key("b",1000,0.0,json!({"type":"cubicBezier","x1":0.2,"y1":-2.0,"x2":0.8,"y2":2.0})),
        fx_key("c",2000,0.2,json!({"type":"hold"}))
    ]}}]});
    let (project, notes) = export(wire);
    let effects = exported_effects(&project);
    let keys = effects[0].animations[0].keys.scalar().unwrap();
    assert_eq!(
        keys.iter()
            .map(|k| (k.source_ticks, k.value, k.easing))
            .collect::<Vec<_>>(),
        vec![
            (0, 0.01, PrKeyframeEasing::Linear),
            (TICKS, 0.01, PrKeyframeEasing::Linear),
            (2 * TICKS, 20.0, PrKeyframeEasing::Hold)
        ]
    );
    assert!(notes.iter().any(|n| n
        .reason
        .contains("Cutoff Bezier keys approximated as Linear")));
    assert!(notes.iter().any(|n| n
        .reason
        .contains("Cutoff falloff below 0.01 percent was raised")));
}

#[test]
fn legacy_luma_key_animated_invert_omission_warns_only_when_enabled() {
    for enabled in [true, false] {
        let mut wire = document_with_effects(json!([
            {"id":1,"enabled":enabled,"effect":{"type":"lumaKey"}},
            {"id":2,"effect":{"type":"gaussianBlur","blurriness":7.0}}
        ]));
        wire["composition"]["dynamics"] = json!({"entries":[{
        "target":{"kind":"effectProperty","effectId":1,"paramName":"invert"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            fx_key("a",0,0.0,json!({"type":"linear"})),
            fx_key("b",1000,1.0,json!({"type":"linear"}))
        ]}}]});
        let (project, notes) = export(wire);
        assert_eq!(exported_effects(&project).len(), 1);
        assert!(matches!(
            exported_effects(&project)[0].params,
            PrEffectParams::FilmImpactBlur(_)
        ));
        let omission = notes
            .iter()
            .find(|n| n.reason.contains("animated FX invert"))
            .unwrap();
        assert!(omission
            .reason
            .contains("lumaKey effect 1 at stack position 1"));
        assert_eq!(
            omission.reason.contains("omitting the enabled key"),
            enabled
        );
    }
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
fn effect_keys_under_time_remapping_from_another_in_keep_their_static_value() {
    use crate::schema::{PrTimeRemap, PrTimeRemapKeyframe};
    use PrKeyframeEasing::Linear;
    // The first clip, at 0 to 2 s, plays a curve from In 0.4 s at 0.8x; its
    // sibling at 5 to 7 s plays at unit speed. Each has a keyed Blurriness.
    let mut sequence = video_sequence();
    let mut sibling = sequence.video_tracks[0].clip(0).clone();
    (sibling.start_ticks, sibling.end_ticks) = (5 * TICKS, 7 * TICKS);
    (sibling.in_ticks, sibling.out_ticks) = (0, 2 * TICKS);
    sibling.effects = vec![keyed(
        blur(true, 0.0, false),
        vec![key(0, 30.0, Linear), key(TICKS, 0.0, Linear)],
    )];
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.end_ticks = 2 * TICKS;
    (clip.in_ticks, clip.out_ticks) = (2 * TICKS / 5, 2 * TICKS);
    clip.playback_rate = 0.8;
    // The curve's keys, at input 0 and 2 s, in input ticks after In.
    clip.time_remap = Some(PrTimeRemap {
        keys: [(-2 * TICKS / 5, 0), (8 * TICKS / 5, 2 * TICKS)]
            .map(|(timeline_ticks, source_ticks)| PrTimeRemapKeyframe {
                timeline_ticks,
                source_ticks,
                easing: Linear,
            })
            .to_vec(),
    });
    clip.effects = vec![keyed(
        blur(true, 0.0, true),
        vec![key(TICKS, 20.0, Linear), key(2 * TICKS, 0.0, Linear)],
    )];
    sequence.video_tracks[0]
        .items
        .push(PrVideoItem::Media(sibling));
    let media = crate::tests::support::video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    // The remapped clip keeps its whole curve and its blur at the first key's
    // value; only the keys are reported, and no occurrence is omitted.
    let layer = &wire["composition"]["layers"][0];
    let playback = &layer["playback"];
    assert_eq!(playback["inputOffsetMs"], 500);
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
        [(0, 0), (2500, 2000)]
    );
    assert_eq!(
        layer["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 20.0, "repeatEdgePixels": true}},
        ])
    );
    let reason = "Gaussian Blur effect at stack position 1: Blurriness keys were not imported: their clock under Time Remapping from a source In or at another speed is unmeasured; the static value was kept";
    assert_eq!(
        omissions
            .iter()
            .filter(|omission| omission.reason == reason)
            .count(),
        1,
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope == OmissionScope::Feature),
        "{omissions:?}"
    );
    // The sibling's keys still import.
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    assert_eq!(
        effect_tracks(&document)
            .into_iter()
            .map(|(id, param, _)| (id, param))
            .collect::<Vec<_>>(),
        [(2, "blurriness".to_owned())]
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
        mask: None,
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
        mask: None,
        enabled,
        params: PrEffectParams::Invert(PrInvert { blend, channel: 0 }),
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

/// A Color Matte becomes a Rect layer, which imports no effects: each effect
/// on it is reported, and the layer is kept. A still's effects import on its
/// image layer; its source chain is reported as not converted (`still_effects_import_in_stack_order_with_keys_from_the_still_in_point`).
#[test]
fn effects_on_mattes_are_reported_instead_of_dropped() {
    use crate::schema::{
        color_matte::COLOR_MATTE_INTRINSIC_TICKS, MediaId, PrColorMatte, PrMedia, PrMediaKind,
        PrVideoOccurrence, PrVideoStream, PrVideoTrack,
    };
    // Generator placements on the 30 fps test sequence start one hour in.
    const COLOR_MATTE_SOURCE_IN_TICKS: i64 = crate::FrameRate::Fps30.generator_in_ticks();
    let mut sequence = video_sequence();
    let template = sequence.video_tracks[0].clip(0).clone();
    let placement = |media: &str, start: i64, source_in: i64, effects| PrVideoOccurrence {
        media: MediaId(media.into()),
        start_ticks: start,
        end_ticks: start + 2 * TICKS,
        in_ticks: source_in,
        out_ticks: source_in + 2 * TICKS,
        effects,
        source_effects: Some(PrSourceEffects {
            master: format!("MasterClip:{media}"),
            effects: vec![blur(true, 10.0, false)],
            active_transforms: 0,
        }),
        ..template.clone()
    };
    sequence.video_tracks.push(PrVideoTrack::media([placement(
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
    )]));
    let mut media = crate::tests::support::video_media();
    media.insert(
        MediaId("red".into()),
        PrMedia {
            name: "Color Matte".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: Some(PrVideoStream {
                pixel_aspect: Default::default(),
                interpretation: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                intrinsic_ticks: COLOR_MATTE_INTRINSIC_TICKS,
                frame_rate: (FrameRate::Fps30).into(),
                width: 1920,
                height: 1080,
                kind: PrMediaKind::ColorMatte(PrColorMatte { rgb: [255, 0, 0] }),
            }),
            audio: None,
        },
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
        ["Rect", "Video", "Rect"]
    );
    assert!(
        layers.iter().all(|layer| layer.get("effects").is_none()),
        "{layers:?}"
    );
    // The keyed Corner Pin's keys are reported with it, not imported.
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
            omission("red", "Gaussian Blur effect at stack position 1 was not imported: effects on a Color Matte are not converted"),
            omission("red", "Corner Pin effect at stack position 2 was not imported: effects on a Color Matte are not converted"),
            omission("MasterClip:red", "VideoComponentChain not converted"),
        ]
    );
}

/// A trimmed still's In on the 30 fps test sequence: one second past the
/// generator In-point, where an untrimmed still begins.
const STILL_IN_TICKS: i64 = FrameRate::Fps30.generator_in_ticks() + TICKS;

/// The shared one-clip sequence and, on a track above it from 1 s to 3 s, a
/// placement of the 1920x1080 still `photo` from [`STILL_IN_TICKS`] that
/// `edit` changes, with the media of both. The still's layer id is 2.
fn still_over_video(
    edit: impl FnOnce(&mut crate::schema::PrVideoOccurrence),
) -> (
    crate::format::PrSequence,
    BTreeMap<crate::MediaId, crate::schema::PrMedia>,
) {
    use crate::schema::{
        MediaId, PrMedia, PrMediaKind, PrVideoOccurrence, PrVideoStream, PrVideoTrack,
        STILL_INTRINSIC_TICKS,
    };
    let mut sequence = video_sequence();
    let mut still = PrVideoOccurrence {
        media: MediaId("photo".into()),
        start_ticks: TICKS,
        end_ticks: 3 * TICKS,
        in_ticks: STILL_IN_TICKS,
        out_ticks: STILL_IN_TICKS + 2 * TICKS,
        ..sequence.video_tracks[0].clip(0).clone()
    };
    edit(&mut still);
    sequence.video_tracks.push(PrVideoTrack::media([still]));
    let mut media = crate::tests::support::video_media();
    media.insert(
        MediaId("photo".into()),
        PrMedia {
            name: "photo.jpg".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: Some(PrVideoStream {
                pixel_aspect: Default::default(),
                interpretation: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                intrinsic_ticks: STILL_INTRINSIC_TICKS,
                frame_rate: (FrameRate::Fps30).into(),
                width: 1920,
                height: 1080,
                kind: PrMediaKind::Still { alpha: false },
            }),
            audio: None,
        },
    );
    (sequence, media)
}

/// The imported document of `sequence` and the omissions of its import.
fn imported_with_omissions(
    sequence: &crate::format::PrSequence,
    media: &BTreeMap<crate::MediaId, crate::schema::PrMedia>,
) -> (Value, Vec<Omission>) {
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(sequence, media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    (wire, omissions)
}

/// A Brightness & Contrast whose Brightness is keyed at `keys`, its static
/// value the first key's.
fn keyed_brightness(contrast: f64, keys: Vec<PrScalarKeyframe>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness: keys[0].value,
            contrast,
        }),
        animations: vec![PrEffectParamAnimation {
            param: &BRIGHTNESS_CONTRAST_BRIGHTNESS,
            keys: PrEffectParamKeys::Scalar(keys),
        }],
    }
}

#[test]
fn still_effects_import_in_stack_order_with_keys_from_the_still_in_point() {
    use PrKeyframeEasing::{Hold, Linear};
    let (black, white) = ([0, 0, 0], [255, 255, 255]);
    // A Brightness & Contrast keyed half a second before and one second after
    // the still's In, then a bypassed Tint.
    let stack = || {
        vec![
            keyed_brightness(
                15.0,
                vec![
                    key(STILL_IN_TICKS - TICKS / 2, 10.0, Linear),
                    key(STILL_IN_TICKS + TICKS, -20.0, Hold),
                ],
            ),
            tint(false, (black, white, 60.0), (vec![], vec![], vec![])),
        ]
    };
    let effects = json!([
        {"id": 1, "enabled": true, "effect": {"type": "brightnessContrast", "brightness": 10.0, "contrast": 15.0}},
        tint_tritone(2, false, (black, white, 60.0)),
    ]);
    let (sequence, media) = still_over_video(|still| still.effects = stack());
    let (wire, omissions) = imported_with_omissions(&sequence, &media);
    assert!(omissions.is_empty(), "{omissions:?}");
    // The still's image layer holds both effects in stack order, the Tint
    // bypassed.
    let image = &wire["composition"]["layers"][0];
    assert_eq!((&image["type"], &image["id"]), (&json!("Image"), &json!(2)));
    assert_eq!(image["effects"], effects);
    // Its Brightness keys are the one editable track of effect 1, at layer
    // times from the still's In, with key ids that name the image layer.
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    assert_eq!(
        effect_tracks(&document),
        [(
            1,
            "brightness".to_owned(),
            vec![
                (
                    "premiere-effect-1-brightness-2-0".to_owned(),
                    -500,
                    10.0,
                    PropertyKeyframeEasing::Linear
                ),
                (
                    "premiere-effect-1-brightness-2-1".to_owned(),
                    1000,
                    -20.0,
                    PropertyKeyframeEasing::Hold
                ),
            ]
        )]
    );
    // A retimed still keeps the effects at their static values, as it keeps
    // its Motion's: their keys are on the source clock.
    let (sequence, media) = still_over_video(|still| {
        still.effects = stack();
        (still.playback_rate, still.out_ticks) = (2.0, STILL_IN_TICKS + 4 * TICKS);
    });
    let (wire, omissions) = imported_with_omissions(&sequence, &media);
    assert_eq!(wire["composition"]["layers"][0]["effects"], effects);
    assert!(!wire["composition"]["dynamics"]
        .to_string()
        .contains("effectProperty"));
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "photo".into(),
            reason: "effect animation was not imported: keys on a retimed, reversed or time-remapped clip are not converted; static values were kept".into(),
        }]
    );
}

#[test]
fn still_effects_before_a_still_mask_or_on_a_matte_are_reported_and_the_mask_kept() {
    use crate::schema::{PrMatteChannel, PrTrackMatte};
    use crate::tests::support::{left_crop, opacity_mask, transform_effect, DEFAULT_PR_TRANSFORM};
    let (black, white) = ([0, 0, 0], [255, 255, 255]);
    let stack = || {
        let linear = PrKeyframeEasing::Linear;
        vec![
            keyed_brightness(
                15.0,
                vec![
                    key(STILL_IN_TICKS, 10.0, linear),
                    key(STILL_IN_TICKS + TICKS, 20.0, linear),
                ],
            ),
            tint(false, (black, white, 60.0), (vec![], vec![], vec![])),
        ]
    };
    let imported_stack = json!([
        {"id": 1, "enabled": true, "effect": {"type": "brightnessContrast", "brightness": 10.0, "contrast": 15.0}},
        tint_tritone(2, false, (black, white, 60.0)),
    ]);
    // Both effects of the stack, reported for `reason`.
    let both = |reason: &str| {
        vec![
            format!("Brightness & Contrast effect at stack position 1 was not imported: {reason}"),
            format!("bypassed Tint effect at stack position 2 was not imported: {reason}"),
        ]
    };
    let before_mask = |mask: &str| {
        both(&format!("it applies before the still's {mask}, which FX applies before the image layer's effects, and a still imports no stage group"))
    };
    type Edit = fn(&mut crate::schema::PrVideoOccurrence);
    // (case, still edit, whether a Track Matte Key on the video below uses the
    // still as its matte, the still's mask guide type, the reported effects,
    // and whether the stack imports)
    type Case = (
        &'static str,
        Edit,
        bool,
        Option<&'static str>,
        Vec<String>,
        bool,
    );
    let cases: [Case; 5] = [
        // A standard Crop that applies first: FX applies it before the effects too.
        (
            "after a Crop",
            |still| still.crop = left_crop(),
            false,
            Some("Rect"),
            Vec::new(),
            true,
        ),
        (
            "before a Crop",
            |still| (still.crop, still.effects_above_mask) = (left_crop(), 2),
            false,
            Some("Rect"),
            before_mask("Crop"),
            false,
        ),
        // An Opacity mask applies after every effect (`reader/video.rs`).
        (
            "before an Opacity mask",
            |still| (still.opacity_mask, still.effects_above_mask) = (Some(opacity_mask()), 2),
            false,
            Some("Shape"),
            before_mask("Opacity mask"),
            false,
        ),
        (
            "Track Matte Key's matte",
            |still| (still.start_ticks, still.end_ticks) = (0, 5 * TICKS),
            true,
            None,
            both("the still is a Track Matte Key's matte, which converts without effects: a key over a matte still with effects is unmeasured"),
            false,
        ),
        // Only a media clip's stage group carries a Transform; the effects on
        // either side of it keep their order.
        (
            "a Transform between the effects",
            |still| still.effects.insert(1, transform_effect(DEFAULT_PR_TRANSFORM, Vec::new())),
            false,
            None,
            vec![format!(
                "Transform effect at stack position 2 was not imported: {}",
                super::TRANSFORM_HOST_REASON
            )],
            true,
        ),
    ];
    for (case, edit, matte, guide, reported, imports) in cases {
        let (mut sequence, media) = still_over_video(|still| {
            still.effects = stack();
            edit(still);
            still.out_ticks = still.in_ticks + still.end_ticks - still.start_ticks;
        });
        if matte {
            sequence.video_tracks[0].clip_mut(0).track_matte = Some(PrTrackMatte {
                track_index: 1,
                channel: PrMatteChannel::Alpha,
            });
        }
        let (wire, omissions) = imported_with_omissions(&sequence, &media);
        let layers = wire["composition"]["layers"].as_array().unwrap();
        let image = &layers[0];
        assert_eq!(image["type"], "Image", "{case}");
        // The mask keeps its guide, the image's only mask.
        let guides: Vec<_> = image["masks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|mask| {
                let guide = layers
                    .iter()
                    .find(|layer| layer["id"] == mask["layer"])
                    .unwrap();
                guide["type"].as_str().unwrap()
            })
            .collect();
        assert_eq!(guides, Vec::from_iter(guide), "{case}");
        if matte {
            assert_eq!(layers[1]["trackMatte"]["layer"], image["id"], "{case}");
        }
        let reports: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.reason.contains("effect at stack position"))
            .map(|omission| {
                assert_eq!(
                    (omission.scope, omission.kind, omission.record.as_str()),
                    (OmissionScope::Feature, OmissionKind::Omitted, "photo"),
                    "{case}"
                );
                omission.reason.clone()
            })
            .collect();
        assert_eq!(reports, reported, "{case}");
        // An imported stack keeps its Brightness keys; a reported one leaves
        // neither the effects nor their keys.
        let keyed = wire["composition"]["dynamics"]
            .to_string()
            .contains("effectProperty");
        if imports {
            assert_eq!(image["effects"], imported_stack, "{case}");
        } else {
            assert!(image.get("effects").is_none(), "{case}: {image}");
        }
        assert_eq!(keyed, imports, "{case}");
    }
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
        {"id": 9, "effect": {"type": "vignette", "amount": 0.5}},
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
                "effects: vignette effect 9 was not exported: it has no Premiere effect mapping"
                    .to_owned(),
        }]
    );
}

/// The original effect and Crop records, placed on the existing full-canvas
/// CPU host. The human occurrence's placement/media clock is not exercised.
fn alpha_glow_native_xml() -> String {
    let source = include_str!("../../../tests/fixtures/alpha-glow-native.xml");
    let doc = roxmltree::Document::parse(source).unwrap();
    let records: String = doc
        .root_element()
        .children()
        .filter(|node| {
            node.is_element() && !matches!(node.attribute("ObjectID"), Some("328" | "392"))
        })
        .map(|node| &source[node.range()])
        .collect();
    include_str!("../../../tests/fixtures/one-clip.xml")
        .replace("<DefaultOpacity>true</DefaultOpacity><ComponentChain/>", "<DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"553\"/><Component Index=\"1\" ObjectRef=\"554\"/></Components></ComponentChain>")
        .replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

#[test]
fn alpha_glow_native_import_is_editable_after_crop() {
    let (wire, notes) = find_edges_import(&alpha_glow_native_xml());
    let effect = &wire["composition"]["layers"][0]["effects"][0]["effect"];
    assert_eq!(effect["type"], "outerGlow", "{wire:#} {notes:?}");
    assert_eq!(effect["size"], 30.0);
    assert_eq!(
        effect["color"],
        json!([192.0 / 255.0, 192.0 / 255.0, 192.0 / 255.0, 150.0 / 255.0])
    );
    assert_eq!(effect["spread"], 0.0);
    assert_eq!(effect["range"], 0.5);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].1, "size");
    assert_eq!(
        tracks[0].2.iter().map(|k| k.1).collect::<Vec<_>>(),
        [0, 2848]
    );
    assert_eq!(
        tracks[0].2.iter().map(|k| k.2).collect::<Vec<_>>(),
        [30.0, 100.0]
    );
    assert!(notes
        .iter()
        .any(|n| n.kind == OmissionKind::Approximated && n.reason.contains("uncalibrated")));
}

#[test]
fn alpha_glow_edited_export_writes_current_controls_and_order() {
    let mut wire = document_with_effects(json!([
        {"id": 1, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 9, "effect": {"type": "outerGlow", "enabled": true,
            "color": [0.2, 0.4, 0.8, 0.6], "size": 42.4, "spread": 0.0,
            "range": 0.5, "blendMode": "normal"}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 20.0}}
    ]));
    wire["composition"]["layers"][0]["masks"] = json!([
        {"id":1,"mode":"add","layer":2,"feather":[0.0,0.0],"opacity":1.0}
    ]);
    let mut canvas = wire["composition"]["layers"][1].clone();
    canvas["id"] = json!(3);
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(canvas);
    wire["composition"]["layers"][1]["rect"]["position"] = json!([480.0, 270.0]);
    wire["composition"]["layers"][1]["rect"]["size"] = json!([1440.0, 810.0]);
    let (mut project, notes) = export(wire);
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 3, "{notes:?}");
    assert_eq!(effects[0], exported_blur(false, 10.0, false));
    assert_eq!(
        effects[1].params,
        PrEffectParams::AlphaGlow {
            size: 42.0,
            brightness: 153.0,
            color: PrColour {
                rgb: [51, 102, 204]
            }
        }
    );
    assert_eq!(effects[2], exported_blur(true, 20.0, false));
    assert!(notes
        .iter()
        .any(|n| n.kind == OmissionKind::Approximated && n.reason.contains("uncalibrated")));
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
    let tree = roxmltree::Document::parse(&xml).unwrap();
    fn text<'a>(node: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
        node.children()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
    }
    let native = tree
        .descendants()
        .find(|n| {
            n.has_tag_name("VideoFilterComponent")
                && text(*n, "MatchName") == Some("AE.ADBE Alpha Glow")
        })
        .unwrap();
    let params: Vec<_> = native
        .descendants()
        .filter(|n| n.has_tag_name("Param"))
        .map(|n| {
            let id = n.attribute("ObjectRef").unwrap();
            tree.descendants()
                .find(|n| n.attribute("ObjectID") == Some(id))
                .unwrap()
        })
        .collect();
    assert_eq!(params.len(), 6);
    assert_eq!(
        params[0].attribute("ClassID"),
        Some("6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8")
    );
    assert_eq!(text(params[0], "ParameterControlType"), Some("1"));
    assert_eq!(text(params[0], "LowerBound"), Some("0"));
    assert_eq!(text(params[0], "UpperBound"), Some("100"));
    assert_eq!(text(params[1], "UpperBound"), Some("255"));

    for (param, (id, value)) in params.iter().zip([
        (1, "42".to_owned()),
        (2, "153".to_owned()),
        (
            3,
            PrColour {
                rgb: [51, 102, 204],
            }
            .native()
            .to_string(),
        ),
        (
            4,
            PrColour {
                rgb: [51, 102, 204],
            }
            .native()
            .to_string(),
        ),
        (5, "false".to_owned()),
        (6, "true".to_owned()),
    ]) {
        assert_eq!(text(*param, "ParameterID"), Some(id.to_string().as_str()));
        assert_eq!(
            text(*param, "StartKeyframe").unwrap().split(',').nth(1),
            Some(value.as_str())
        );
    }
    // Native descending Index remains the inverse of editable effect order.
    let glow_id = native.attribute("ObjectID").unwrap();
    let glow_index = tree
        .descendants()
        .find(|n| n.has_tag_name("Component") && n.attribute("ObjectRef") == Some(glow_id))
        .unwrap()
        .attribute("Index")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let crop = tree
        .descendants()
        .find(|n| {
            n.has_tag_name("VideoFilterComponent")
                && text(*n, "MatchName") == Some("AE.ADBE AECrop")
        })
        .unwrap();
    let crop_index = tree
        .descendants()
        .find(|n| {
            n.has_tag_name("Component") && n.attribute("ObjectRef") == crop.attribute("ObjectID")
        })
        .unwrap()
        .attribute("Index")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(crop_index > glow_index);
}

/// Mutate only a selected saved Glow control, never a similarly valued Crop.
fn alpha_glow_edit_control(xml: &str, id: &str, edit: impl FnOnce(&str) -> String) -> String {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let range = doc
        .root_element()
        .children()
        .find(|n| n.attribute("ObjectID") == Some(id))
        .unwrap()
        .range();
    format!(
        "{}{}{}",
        &xml[..range.start],
        edit(&xml[range.clone()]),
        &xml[range.end..]
    )
}

#[test]
fn alpha_glow_variant_approximations_keep_crop_and_editable_glow() {
    for (id, from, to, diagnostic) in [
        ("763", ",false,", ",true,", "two-color interpolation"),
        ("764", ",true,", ",false,", "solid falloff replaced"),
    ] {
        let xml = alpha_glow_edit_control(&alpha_glow_native_xml(), id, |control| {
            control.replace(from, to)
        });
        let (native, _) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        let clip = native
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!((clip.crop.left, clip.crop.top), (25.0, 25.0));
        assert_eq!(clip.effects.len(), 1);
        let (wire, notes) = find_edges_import(&xml);
        let effect = &wire["composition"]["layers"][0]["effects"][0]["effect"];
        assert_eq!(effect["type"], "outerGlow", "{notes:?}");
        assert_eq!(effect["size"], 30.0);
        assert_eq!(
            effect["color"],
            json!([192.0 / 255.0, 192.0 / 255.0, 192.0 / 255.0, 150.0 / 255.0])
        );
        assert!(
            notes.iter().any(|n| n.reason.contains(diagnostic)),
            "{notes:?}"
        );
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        assert_eq!(effect_tracks(&document)[0].2.len(), 2);
    }
}

#[test]
fn alpha_glow_style_approximations_keep_siblings() {
    for (field, value) in [
        ("spread", json!(0.2)),
        ("range", json!(0.8)),
        ("blendMode", json!("screen")),
        ("size", json!(101.0)),
    ] {
        let mut wire = document_with_effects(json!([
            {"id":9,"effect":{"type":"outerGlow","size":42.0,"spread":0.0,"range":0.5,"blendMode":"normal"}},
            {"id":2,"effect":{"type":"gaussianBlur","blurriness":20.0}}
        ]));
        wire["composition"]["layers"][0]["effects"][0]["effect"][field] = value;
        let (project, notes) = export(wire);
        let effects = exported_effects(&project);
        assert_eq!(effects.len(), 2, "{notes:?}");
        assert!(
            matches!(effects[0].params, PrEffectParams::AlphaGlow { size, .. } if size == if field == "size" {100.0} else {42.0})
        );
        assert_eq!(effects[1], exported_blur(true, 20.0, false));
        let diagnostic = if field == "size" {
            "limited to the native maximum"
        } else {
            "dilation, falloff and blend differences"
        };
        assert!(
            notes.iter().any(|n| n.reason.contains(diagnostic)),
            "{notes:?}"
        );
    }
}

#[test]
fn alpha_glow_edited_size_keys_export_or_flatten_fractional_endpoints() {
    for endpoint in [70.0, 70.5] {
        let mut wire = document_with_effects(json!([
            {"id":9,"effect":{"type":"outerGlow","size":42.0,"color":[0.1,0.2,0.3,0.4],"spread":0.0,"range":0.5,"blendMode":"normal"}},
            {"id":2,"effect":{"type":"gaussianBlur","blurriness":20.0}}
        ]));
        wire["composition"]["dynamics"] = json!({"entries":[{
            "target":{"kind":"effectProperty","effectId":9,"paramName":"size"},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                fx_key("alpha-size-a",0,12.0,json!({"type":"linear"})),
                fx_key("alpha-size-b",1000,endpoint,json!({"type":"hold"}))
            ]}
        }]});
        let (project, notes) = export(wire);
        let effects = exported_effects(&project);
        if endpoint.fract() == 0.0 {
            assert_eq!(effects.len(), 2, "{notes:?}");
            assert_eq!(
                effects[0].animations[0].keys.scalar().unwrap(),
                [
                    key(0, 12.0, PrKeyframeEasing::Linear),
                    key(TICKS, 70.0, PrKeyframeEasing::Hold)
                ]
            );
        } else {
            assert_eq!(effects.len(), 2);
            assert!(matches!(
                effects[0].params,
                PrEffectParams::AlphaGlow { size: 42.0, .. }
            ));
            assert!(effects[0].animations.is_empty());
            assert_eq!(effects[1], exported_blur(true, 20.0, false));
            assert!(notes
                .iter()
                .any(|n| n.reason.contains("flattened to current static size 42")
                    && n.reason.contains("whole size endpoints")));
        }
    }
}

#[test]
fn alpha_glow_transformed_host_and_flattened_color_keep_glow() {
    for animated_color in [false, true] {
        let mut wire = document_with_effects(json!([
            {"id":9,"effect":{"type":"outerGlow","size":42.0,"spread":0.0,"range":0.5,"blendMode":"normal"}},
            {"id":2,"effect":{"type":"gaussianBlur","blurriness":20.0}}
        ]));
        if animated_color {
            wire["composition"]["dynamics"] = json!({"entries":[{
                "target":{"kind":"effectProperty","effectId":9,"paramName":"color"},
                "animator":{"type":"keyframes","enabled":true,"keyframes":[
                    {"id":"alpha-color-a","layerTime":0,"value":{"type":"color","value":[1.0,0.0,0.0,1.0]},"easing":{"type":"linear"}},
                    {"id":"alpha-color-b","layerTime":1000,"value":{"type":"color","value":[0.0,1.0,0.0,1.0]},"easing":{"type":"linear"}}
                ]}
            }]});
        } else {
            wire["composition"]["layers"][0]["transform"]["scale"] = json!([150.0, 150.0]);
        }
        let (project, notes) = export(wire);
        let effects = exported_effects(&project);
        assert_eq!(effects.len(), 2, "{notes:?}");
        assert!(matches!(
            effects[0].params,
            PrEffectParams::AlphaGlow { size: 42.0, .. }
        ));
        assert_eq!(effects[1], exported_blur(true, 20.0, false));
        if animated_color {
            assert!(
                notes.iter().any(|n| n
                    .reason
                    .contains("color animation flattened to current static control")),
                "{notes:?}"
            );
        }
    }
}

#[test]
fn alpha_glow_native_animated_brightness_and_modes_keep_first_value() {
    for (id, keys, expected_alpha, reason) in [
        (
            "760",
            "0,100,0,0,0,0,0,0;254016000000,200,0,0,0,0,0,0;",
            100.0 / 255.0,
            "Brightness animation flattened",
        ),
        (
            "763",
            "0,true,0,0,0,0,0,0;254016000000,false,0,0,0,0,0,0;",
            150.0 / 255.0,
            "Use End Color animation flattened",
        ),
    ] {
        let xml = alpha_glow_edit_control(&alpha_glow_native_xml(), id, |control| {
            control.replace("</VideoComponentParam>",&format!("<IsTimeVarying>true</IsTimeVarying><Keyframes>{keys}</Keyframes></VideoComponentParam>"))
        });
        let (wire, notes) = find_edges_import(&xml);
        let effect = &wire["composition"]["layers"][0]["effects"][0]["effect"];
        assert_eq!(effect["type"], "outerGlow", "{notes:?}");
        assert_eq!(effect["color"][3], expected_alpha);
        assert!(notes.iter().any(|n| n.reason.contains(reason)), "{notes:?}");
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        assert_eq!(effect_tracks(&document).len(), 1);
    }
    let malformed = alpha_glow_edit_control(&alpha_glow_native_xml(), "763", |control| {
        control.replace(",false,", ",invalid,")
    });
    let (native, notes) = crate::format::inspect_project_with_omissions(&malformed, None).unwrap();
    let clip = native
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!((clip.crop.left, clip.crop.top), (25.0, 25.0));
    assert!(clip.effects.is_empty());
    assert!(notes.iter().any(|n| n
        .reason
        .contains("invalid Alpha Glow Use End Color boolean")));
}

#[test]
fn alpha_glow_effective_bypass_keeps_current_style_controls() {
    for (record_enabled, style_enabled) in [(false, true), (true, false)] {
        let wire = document_with_effects(json!([
            {"id":9,"enabled":record_enabled,"effect":{"type":"outerGlow","enabled":style_enabled,"size":42.0,"color":[0.2,0.4,0.8,0.6],"spread":0.2,"range":0.8,"blendMode":"screen"}}
        ]));
        let (project, notes) = export(wire);
        let effects = exported_effects(&project);
        assert_eq!(effects.len(), 1, "{notes:?}");
        assert!(!effects[0].enabled);
        assert!(matches!(
            effects[0].params,
            PrEffectParams::AlphaGlow {
                size: 42.0,
                brightness: 153.0,
                ..
            }
        ));
    }
}

#[test]
fn alpha_glow_held_clock_keeps_static_editable_glow() {
    let (project, _) =
        crate::format::inspect_project_with_omissions(&alpha_glow_native_xml(), None).unwrap();
    let mut clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
        .clone();
    clip.time_remap = Some(crate::schema::PrTimeRemap::frame_hold(
        0,
        clip.end_ticks - clip.start_ticks,
    ));
    let mut notes = Vec::new();
    let (effects, tracks) = crate::convert::effects::import_effects(
        &clip,
        fx_schema::LayerId::new(1),
        false, // Explicit Frame Hold retains static effect parameters.
        crate::schema::MaskBoundary::Flat,
        false, // Direct video occurrence, not a nested host.
        false, // Video, not a still image.
        project.media[&clip.media].video.as_ref().unwrap().kind,
        [1920, 1080],
        [1920, 1080],
        [1920, 1080],
        &mut crate::convert::effects::EffectIdAllocator::default(),
        &mut notes,
    );
    assert_eq!(effects.len(), 1, "{notes:?}");
    assert!(tracks.is_empty());
    let EffectData::Identified {
        effect: EffectPayload::Known(LayerEffect::OuterGlow(style)),
        ..
    } = effects[0].data()
    else {
        panic!("expected editable glow");
    };
    assert_eq!(style.size.value(), 30.0);
    assert!(notes
        .iter()
        .any(|n| n.reason.contains("retimed, reversed or time-remapped")
            && n.reason.contains("static value was kept")));
}

#[test]
fn alpha_glow_unrepresentable_size_easing_keeps_validated_static_and_siblings() {
    let keys = "0,30,5,0,0,0.16666666666666666,10,0.16666666666666666;254016000000,30,0,0,10,0.16666666666666666,0,0.16666666666666666;";
    let edit_keys = |keys: &str| {
        alpha_glow_edit_control(&alpha_glow_native_xml(), "759", |control| {
            let start = control.find("<Keyframes>").unwrap() + "<Keyframes>".len();
            let end = control.find("</Keyframes>").unwrap();
            format!("{}{}{}", &control[..start], keys, &control[end..])
        })
    };
    let (project, mut notes) =
        crate::format::inspect_project_with_omissions(&edit_keys(keys), None).unwrap();
    let mut sequence = project.single_sequence().unwrap().clone();
    let clip = sequence.video_tracks[0].clip_mut(0);
    assert_eq!((clip.crop.left, clip.crop.top), (25.0, 25.0));
    assert_eq!(clip.effects.len(), 1, "{notes:?}");
    assert!(matches!(
        clip.effects[0].params,
        PrEffectParams::AlphaGlow { size: 30.0, .. }
    ));
    assert!(clip.effects[0].animations.is_empty());
    // Supplementary convertible sibling; original record759 and Crop were read above.
    clip.effects.push(blur(false, 10.0, false));
    let assets = crate::tesseract_output::asset_ids_in_order(&sequence, &project.media);
    let wire =
        crate::convert::premiere_to_tesseract(&sequence, &project.media, &assets, &mut notes)
            .unwrap()
            .to_json_value()
            .unwrap();
    let effects = &wire["composition"]["layers"][0]["effects"];
    assert_eq!(effects.as_array().unwrap().len(), 2);
    assert_eq!(effects[0]["effect"]["type"], "outerGlow");
    assert_eq!(effects[0]["effect"]["size"], 30.0);
    assert_eq!(effects[1]["effect"]["type"], "gaussianBlur");
    assert_eq!(effects[1]["enabled"], false);
    assert!(
        notes
            .iter()
            .any(|note| note.reason.contains("Glow size animation flattened")
                && note.reason.contains("equal values")),
        "{notes:?}"
    );
    for malformed in [
        keys.replacen(",30,", ",101,", 1),
        keys.replacen("0.16666666666666666", "1.5", 1),
    ] {
        let (project, notes) =
            crate::format::inspect_project_with_omissions(&edit_keys(&malformed), None).unwrap();
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert!(clip.effects.is_empty(), "{notes:?}");
        assert_eq!((clip.crop.left, clip.crop.top), (25.0, 25.0));
    }
}

#[test]
fn lens_distortion_import_normalizes_curvature_and_preserves_clip_clock() {
    let lens = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::LensDistortion(-40.0),
        animations: vec![PrEffectParamAnimation {
            param: &crate::schema::LENS_CURVATURE,
            keys: PrEffectParamKeys::Scalar(vec![
                key(0, -40.0, PrKeyframeEasing::Linear),
                key(510674274420, 40.0, PrKeyframeEasing::Linear),
            ]),
        }],
    };
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS;
    clip.out_ticks = 6 * TICKS;
    clip.effects = vec![lens];
    let wire = project_document(&sequence);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"],
        json!({"type": "lensDistortion", "amount": 0.4, "centerX": 0.5, "centerY": 0.5})
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks[0].1, "amount");
    assert_eq!((tracks[0].2[0].1, tracks[0].2[0].2), (-1000, 0.4));
    assert_eq!((tracks[0].2[1].1, tracks[0].2[1].2), (1010, -0.4));
}

#[test]
fn lens_distortion_edited_export_writes_current_controls_and_keys() {
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 9, "effect": {"type": "lensDistortion", "amount": -0.6, "centerX": 0.5, "centerY": 0.5}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 20.0}},
    ]));
    wire["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 9, "paramName": "amount"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            fx_key("lens-a", 0, -0.6, json!({"type": "linear"})),
            fx_key("lens-b", 1000, 0.2, json!({"type": "hold"}))
        ]}
    }]});
    let (mut project, omissions) = export(wire);
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(omissions[0]
        .reason
        .contains("deliberate slider normalization"));
    let effects = exported_effects(&project);
    assert_eq!(effects[1].params, PrEffectParams::LensDistortion(60.0));
    let keys = effects[1].animations[0].keys.scalar().unwrap();
    assert_eq!(keys[1].value, -20.0);
    assert_eq!(keys[1].easing, PrKeyframeEasing::Hold);
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
    assert!(xml.contains("PR.ADBE Lens Distortion"));
    let native = roxmltree::Document::parse(&xml).unwrap();
    let component = native
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoFilterComponent")
                && node.children().any(|child| {
                    child.has_tag_name("MatchName")
                        && child.text() == Some("PR.ADBE Lens Distortion")
                })
        })
        .unwrap();
    assert!(!component
        .children()
        .any(|node| node.has_tag_name("PremiereFilterPrivateData")));
    let controls: Vec<_> = component
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
        .map(|reference| {
            native
                .descendants()
                .find(|node| node.attribute("ObjectID") == reference.attribute("ObjectRef"))
                .unwrap()
        })
        .collect();
    assert_eq!(controls.len(), 7);
    let text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .unwrap()
            .text()
            .unwrap()
            .to_owned()
    };
    for control in &controls {
        assert_eq!(text(*control, "ParameterID"), "-1");
    }
    assert!(text(controls[0], "StartKeyframe").contains(",60.,"));
    assert!(text(controls[5], "StartKeyframe").contains(",true,"));
    assert_eq!(reread_effects(project), effects);
}

#[test]
fn lens_distortion_static_plane_guard_keeps_blur_sibling() {
    for transform in [
        json!({}),
        json!({"skew": 10.0}),
        json!({"skewAxis": 10.0}),
        json!({"rotationX": 20.0}),
        json!({"rotationY": 20.0}),
        json!({"orientation": [20.0, 0.0, 0.0]}),
        json!({"orientation": [0.0, 20.0, 0.0]}),
        json!({"orientation": [0.0, 0.0, 20.0]}),
        json!({"position": [0.0, 0.0, 250.0]}),
    ] {
        let mut wire = document_with_effects(json!([
            {"id": 9, "effect": {"type": "lensDistortion", "amount": 0.2, "centerX": 0.5, "centerY": 0.5}},
            {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 20.0}},
        ]));
        for (field, value) in transform.as_object().unwrap() {
            wire["composition"]["layers"][0]["transform"][field] = value.clone();
        }
        let (project, omissions) = export(wire);
        let identity = transform.as_object().unwrap().is_empty();
        let expected: Vec<_> = identity
            .then_some(PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::LensDistortion(-20.0),
                animations: Vec::new(),
            })
            .into_iter()
            .chain([exported_blur(true, 20.0, false)])
            .collect();
        assert_eq!(exported_effects(&project), expected, "{transform}");
        let lens_reports: Vec<_> = omissions
            .iter()
            .filter(|item| item.reason.contains("lensDistortion effect 9"))
            .collect();
        assert_eq!(lens_reports.len(), 1, "{transform}: {omissions:?}");
        assert_eq!(lens_reports[0].scope, OmissionScope::Feature);
        assert_eq!(lens_reports[0].record, "layer 1 (\"Source\")");
        if identity {
            assert!(lens_reports[0].reason.contains("converts approximately"));
        } else {
            assert!(lens_reports[0]
                .reason
                .contains("requires an identity input plane"));
            assert_eq!(lens_reports[0].kind, OmissionKind::Omitted);
        }
    }
}

#[test]
fn lens_distortion_unsupported_edits_keep_siblings() {
    for (enabled, center) in [(true, 0.3), (false, 0.5)] {
        let (project, omissions) = export(document_with_effects(json!([
            {"id": 9, "enabled": enabled, "effect": {"type": "lensDistortion", "amount": 0.2, "centerX": center, "centerY": 0.5}},
            {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 20.0}},
        ])));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 20.0, false)]
        );
        assert_eq!(omissions.len(), 1);
        assert!(omissions[0].reason.contains("was not exported"));
    }
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

#[test]
fn ramp_approximation_still_rejects_unbound_animated_effect_parameters() {
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
    ]));
    wire["composition"]["layers"][0]["playback"] = crate::test_support::remapped_playback(
        json!({"start": 0, "duration": 1000}),
        json!({"before": "inactive", "after": "inactive", "keyframes": [
            {"id": "in", "time": 0, "value": 0, "easing": {"type": "linear"}},
            {"id": "middle", "time": 500, "value": 200, "easing": {"type": "linear"}},
            {"id": "out", "time": 1000, "value": 1000, "easing": {"type": "linear"}}
        ]}),
    );
    let animated = |effect_id: u64, parameter: &str| {
        json!({
            "target": {"kind": "effectProperty", "effectId": effect_id, "paramName": parameter},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                fx_key(&format!("{effect_id}-a"), 0, 20.0, json!({"type": "linear"})),
                fx_key(&format!("{effect_id}-b"), 1000, 40.0, json!({"type": "linear"}))
            ]}
        })
    };
    wire["composition"]["dynamics"] = json!({"entries": [
        animated(1, "repeatEdgePixels"), animated(2, "blurriness")
    ]});
    let (project, omissions) = export(wire);
    // The ramp cannot make an unbound parameter admissible. Supported
    // Blurriness retains its authored static 10 rather than its first key 20.
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 10.0, false)]
    );
    assert!(omissions.iter().any(|item| item.reason == "effects: gaussianBlur effect 1 was not exported: animated repeatEdgePixels has no static Premiere value; only static effect parameters export"));
    assert!(omissions
        .iter()
        .any(|item| item.reason.contains("effect blurriness animation")
            && item.reason.contains("static values were kept")));
    assert!(!omissions
        .iter()
        .any(|item| item.reason.contains("repeatEdgePixels")
            && item.reason.contains("static values were kept")));
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
        mask: None,
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
        PrEffect { mask: None, enabled: true, params: PrEffectParams::BlackWhite, animations: Vec::new() },
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
        mask: None,
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
        (with(&[("shape", 1.0)]), vec![], json!({}), vec![], "shape 1 is not 0 (linear); a radial ramp is not converted, because Premiere measures its radius in clip pixels and the FX gradientRamp in frame UV".to_owned()),
        (with(&[("endX", 0.7)]), vec![], json!({}), vec![], "Start of Ramp 0.5:0 to End of Ramp 0.7:1 is not aligned with the frame at every time; Premiere measures a ramp in clip pixels and the FX gradientRamp in frame UV, which agree only along the frame's axes".to_owned()),
        (with(&[("endY", 0.0)]), vec![], json!({}), vec![], "Start of Ramp and End of Ramp are both 0.5:0; a ramp of zero length is not converted".to_owned()),
        (full(), vec![track("endX", [0.5, 0.6])], json!({}), vec![], "Start of Ramp 0.5:0 to End of Ramp 0.5:1 is not aligned with the frame at every time; Premiere measures a ramp in clip pixels and the FX gradientRamp in frame UV, which agree only along the frame's axes".to_owned()),
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
        mask: None,
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
        format!("{what} {value} is not a whole number of blocks; Premiere counts whole blocks and no rounding is applied")
    };
    let hold_rule = "; only Hold keys convert, because the FX mosaic renders fractional block counts between keys and Premiere's stepping there is unmeasured";
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

/// A Replicate of a static `count`, or of the Count `keys` that start at it.
fn replicate(enabled: bool, count: u8, keys: Vec<PrScalarKeyframe>) -> PrEffect {
    let animations = if keys.is_empty() {
        Vec::new()
    } else {
        vec![PrEffectParamAnimation {
            param: &REPLICATE_COUNT,
            keys: PrEffectParamKeys::Scalar(keys),
        }]
    };
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Replicate(PrReplicate { count }),
        animations,
    }
}

/// The FX tiles of a Replicate Count, `(size, centre)`: 100/Count percent,
/// the first tile centred at 1/(2·Count) of the frame.
fn tiles(count: u8) -> (f64, f64) {
    match count {
        2 => (50.0, 0.25),
        3 => (100.0 / 3.0, 1.0 / 6.0),
        4 => (25.0, 0.125),
        5 => (20.0, 0.1),
        16 => (6.25, 0.03125),
        other => panic!("no tiles listed for Count {other}"),
    }
}

/// The `motionTile` fields of whole copies over the whole frame: tiles of
/// `size` percent whose first is centred at `center` on both axes, without
/// mirrored edges or phase.
fn motion_tile_fields((size, center): (f64, f64)) -> Value {
    json!({"type": "motionTile", "tileCenterX": center, "tileCenterY": center,
        "tileWidth": size, "tileHeight": size, "outputWidth": 100.0, "outputHeight": 100.0,
        "mirrorEdges": false, "phase": 0.0})
}

/// An FX `motionTile` effect of [`motion_tile_fields`].
fn fx_motion_tile(id: u64, enabled: bool, tiles: (f64, f64)) -> Value {
    json!({"id": id, "enabled": enabled, "effect": motion_tile_fields(tiles)})
}

/// The approximation that export reports for the `motionTile` effect `id`.
fn replicate_export_approximation(id: u64) -> Omission {
    Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Approximated,
        record: "layer 1 (\"Source\")".to_owned(),
        reason: format!(
            "effects: motionTile effect {id} converts approximately: {}",
            PrReplicate::TILING_APPROXIMATION
        ),
    }
}

#[test]
fn keyed_replicate_imports_as_four_synchronized_tile_tracks_and_exports_back() {
    use PrKeyframeEasing::{Hold, Linear};
    // Source In 0.5 s: Hold Count keys 2, 4 and 3 at source 1, 1.5 and 2.5 s
    // sit above a Gaussian Blur, a static Count 3 and a bypassed Count 16.
    let keys = vec![
        key(TICKS, 2.0, Linear),
        key(3 * TICKS / 2, 4.0, Hold),
        key(5 * TICKS / 2, 3.0, Hold),
    ];
    let native = vec![
        replicate(true, 2, keys),
        blur(true, 10.0, false),
        replicate(true, 3, vec![]),
        replicate(false, 16, vec![]),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.in_ticks, clip.out_ticks) = (TICKS / 2, 7 * TICKS / 2);
    clip.effects = native.clone();
    let media = crate::tests::support::video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            fx_motion_tile(1, true, tiles(2)),
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            fx_motion_tile(3, true, tiles(3)),
            fx_motion_tile(4, false, tiles(16)),
        ])
    );
    // Each Replicate, bypassed or not, is reported once.
    let imported = |state: &str, position: u32| Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Approximated,
        record: "source".to_owned(),
        reason: format!(
            "{state}Replicate effect at stack position {position} converts approximately: {}",
            PrReplicate::TILING_APPROXIMATION
        ),
    };
    assert_eq!(
        omissions,
        [imported("", 1), imported("", 3), imported("bypassed ", 4)]
    );
    // One Count keys the four tile fields at layer 500, 1000 and 2000 ms, with
    // the Hold into the second and third keys.
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    let track = |name: &str, values: [f64; 3]| {
        let keys: Vec<ImportedKey> = [(500, linear), (1000, hold), (2000, hold)]
            .into_iter()
            .zip(values)
            .enumerate()
            .map(|(index, ((millis, easing), value))| {
                (
                    format!("premiere-effect-1-{name}-1-{index}"),
                    millis,
                    value,
                    easing,
                )
            })
            .collect();
        (1, name.to_owned(), keys)
    };
    let counts = [tiles(2), tiles(4), tiles(3)];
    let (sizes, centres) = (
        counts.map(|(size, _)| size),
        counts.map(|(_, center)| center),
    );
    let mut tracks = effect_tracks(&document);
    tracks.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(
        tracks,
        [
            track("tileCenterX", centres),
            track("tileCenterY", centres),
            track("tileHeight", sizes),
            track("tileWidth", sizes),
        ]
    );
    let (project, omissions) = export(wire);
    let exported: Vec<_> = native.into_iter().map(current_blur_export).collect();
    assert_eq!(exported_effects(&project), exported);
    let reasons: Vec<_> = omissions.iter().map(|omission| &omission.reason).collect();
    let expected = [1, 3, 4].map(replicate_export_approximation);
    assert_eq!(
        reasons,
        expected
            .iter()
            .map(|omission| &omission.reason)
            .collect::<Vec<_>>()
    );
    assert!(
        omissions
            .iter()
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
}

#[test]
fn replicates_convert_only_on_a_clip_whose_frame_is_the_canvas() {
    use crate::schema::PrPropertyAnimation::UniformScale;
    let mut small = crate::tests::support::video_media();
    let stream = small
        .get_mut(&crate::schema::MediaId("source".into()))
        .unwrap()
        .video
        .as_mut()
        .unwrap();
    (stream.width, stream.height) = (1280, 720);
    let scale_keys = vec![
        key(0, 100.0, PrKeyframeEasing::Linear),
        key(TICKS, 50.0, PrKeyframeEasing::Linear),
    ];
    let gaussian = |id: u64| json!({"id": id, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}});
    // (Motion keys, static Motion edits, the source, why the Replicate is
    // omitted): the FX grid tiles the layer's transformed content.
    type MotionEdit<'a> = &'a dyn Fn(&mut crate::schema::PrStaticTransform);
    #[rustfmt::skip]
    let clips: [(Vec<_>, MotionEdit<'_>, _, &str); 3] = [
        (vec![UniformScale(scale_keys)], &|_| {}, None, "Motion is animated"),
        (Vec::new(), &|transform| transform.rotation = 30.0, None, "static Motion moves the clip frame off the canvas"),
        (Vec::new(), &|_| {}, Some(small), "the source frame differs from the canvas"),
    ];
    let mut stage_group = masked_directional_blur_sequence(true, true);
    let clip = stage_group.video_tracks[0].clip_mut(0);
    (clip.transform.scale, clip.transform.rotation) = ([100.0; 2], 0.0);
    let staged = (stage_group, crate::tests::support::video_media());
    let mut cases: Vec<_> = clips
        .into_iter()
        .map(|(animations, edit, media, reason)| {
            let mut sequence = video_sequence();
            let clip = sequence.video_tracks[0].clip_mut(0);
            clip.animations = animations;
            edit(&mut clip.transform);
            let media = media.unwrap_or_else(crate::tests::support::video_media);
            (
                (sequence, media),
                format!("{reason}; {}", super::REPLICATE_FRAME_RULE),
            )
        })
        .collect();
    cases.push((staged, super::STAGED_REPLICATE_REASON.to_owned()));
    for ((mut sequence, media), reason) in cases {
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.effects = vec![replicate(true, 2, vec![]), blur(true, 10.0, false)];
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
        let mut omissions = Vec::new();
        let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        // The clip and its Gaussian Blur convert, on a stage group's video.
        let top = &wire["composition"]["layers"][0];
        let video = if top["type"] == "Group" {
            &top["layers"][0]
        } else {
            top
        };
        assert_eq!(video["effects"], json!([gaussian(1)]), "{reason}");
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "source".to_owned(),
                reason: format!("Replicate effect at stack position 1 was not imported: {reason}"),
            }]
        );
    }
}

/// Export input: a Gaussian Blur under a `motionTile` (id 2) with the
/// `fields` given, and the `tracks` of its parameters, on a layer with the
/// `transform` fields given ([`document_with_mosaic`]).
fn document_with_motion_tile(fields: Value, tracks: &[(&str, Value)], transform: Value) -> Value {
    document_with_mosaic(fields, tracks, transform, &[])
}

/// Tracks of the four `motionTile` tile fields keyed together at layer 0, 500
/// and 1500 ms (source 0, 0.5 and 1.5 s) to the tiles of `counts`, Hold into
/// the second and third keys.
fn tile_count_tracks(counts: [u8; 3]) -> Vec<(&'static str, Value)> {
    let track = |name: &'static str, field: fn((f64, f64)) -> f64| {
        let keys: Vec<_> = [0, 500, 1500]
            .into_iter()
            .zip(counts)
            .enumerate()
            .map(|(index, (millis, count))| {
                let easing = if index == 0 { "linear" } else { "hold" };
                fx_key(
                    &format!("{name}-{index}"),
                    millis,
                    field(tiles(count)),
                    json!({"type": easing}),
                )
            })
            .collect();
        (name, json!(keys))
    };
    vec![
        track("tileWidth", |(size, _)| size),
        track("tileHeight", |(size, _)| size),
        track("tileCenterX", |(_, center)| center),
        track("tileCenterY", |(_, center)| center),
    ]
}

#[test]
fn edited_replicates_export_whole_counts_and_hold_count_keys() {
    use PrKeyframeEasing::{Hold, Linear};
    // A Count edited to 5 exports as Count 5.
    let (project, omissions) = export(document_with_motion_tile(
        motion_tile_fields(tiles(5)),
        &[],
        json!({}),
    ));
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 10.0, false), replicate(true, 5, vec![])]
    );
    assert_eq!(omissions, [replicate_export_approximation(2)]);
    // Tile keys of Counts 2, 4 and 3 export as one Count's keys on the
    // source clock (source In 1 s); the first key is the static Count.
    let mut wire = document_with_motion_tile(
        motion_tile_fields(tiles(16)),
        &tile_count_tracks([2, 4, 3]),
        json!({}),
    );
    wire["composition"]["layers"][0]["sourceRange"]["start"] = json!(1000);
    wire["composition"]["layers"][0]["playback"]["mapping"]["output"]["start"] = json!(1000);
    let (project, omissions) = export(wire);
    assert_eq!(
        exported_effects(&project),
        [
            exported_blur(true, 10.0, false),
            replicate(
                true,
                2,
                vec![
                    key(TICKS, 2.0, Linear),
                    key(3 * TICKS / 2, 4.0, Hold),
                    key(5 * TICKS / 2, 3.0, Hold),
                ]
            ),
        ]
    );
    assert_eq!(omissions, [replicate_export_approximation(2)]);
}

#[test]
fn replicates_premiere_cannot_represent_are_not_exported() {
    let with = |edits: &[(&str, Value)]| {
        let mut fields = motion_tile_fields(tiles(2));
        for (name, value) in edits {
            fields[*name] = value.clone();
        }
        fields
    };
    let grid_rule = super::REPLICATE_GRID_RULE;
    let no_grid = |[width, height, x, y]: [&str; 4]| {
        format!("tileWidth {width}, tileHeight {height}, tileCenterX {x} and tileCenterY {y} draw no Replicate grid: {grid_rule}")
    };
    let tracks = tile_count_tracks([2, 4, 3]);
    let edit_track = |name: &str, edit: &dyn Fn(&mut Value)| {
        let mut tracks = tracks.clone();
        let (_, keys) = tracks.iter_mut().find(|(track, _)| *track == name).unwrap();
        edit(keys);
        tracks
    };
    let hold_rule = "; only Hold keys convert, because Premiere's Count between interpolated keys is unmeasured and the FX motionTile would interpolate the tile size and centre, reciprocals of the Count, linearly";
    let all_keyed = "its tileWidth, tileHeight, tileCenterX and tileCenterY";
    // (fields, tracks, layer transform, the reason)
    type TileEdit = (Value, Vec<(&'static str, Value)>, Value, String);
    #[rustfmt::skip]
    let tiles_edits: [TileEdit; 16] = [
        // The FX default centre: half a tile off at an even Count, and at an
        // odd one the same grid, though not import's form.
        (with(&[("tileCenterX", json!(0.5)), ("tileCenterY", json!(0.5))]), vec![], json!({}), no_grid(["50", "50", "0.5", "0.5"])),
        (motion_tile_fields((100.0 / 3.0, 0.5)), vec![], json!({}), no_grid(["33.333333333333336", "33.333333333333336", "0.5", "0.5"])),
        (with(&[("tileHeight", json!(25.0))]), vec![], json!({}), no_grid(["50", "25", "0.25", "0.25"])),
        (motion_tile_fields((33.33, 1.0 / 6.0)), vec![], json!({}), no_grid(["33.33", "33.33", "0.16666666666666666", "0.16666666666666666"])),
        // Count 1, outside Premiere's 2 to 16.
        (motion_tile_fields((100.0, 0.5)), vec![], json!({}), no_grid(["100", "100", "0.5", "0.5"])),
        (with(&[("outputWidth", json!(50.0))]), vec![], json!({}), "outputWidth 50 and outputHeight 100 are not 100; a Replicate's copies fill the whole frame".to_owned()),
        (with(&[("mirrorEdges", json!(true))]), vec![], json!({}), "mirrorEdges is on; a Replicate does not mirror its copies".to_owned()),
        (with(&[("phase", json!(90.0))]), vec![], json!({}), "phase 90 is not 0; a Replicate does not offset its copies".to_owned()),
        (with(&[]), vec![("phase", json!([fx_key("p-0", 0, 0.0, json!({"type": "linear"})), fx_key("p-1", 500, 90.0, json!({"type": "linear"}))]))], json!({}), "animated phase has no static Premiere value; only static effect parameters export".to_owned()),
        // Premiere keys one Count: every tile field, at the same times, with
        // the same easing, at one Count's grid, and held.
        (with(&[]), tracks[..3].to_vec(), json!({}), format!("{all_keyed} must all be keyed, but tileCenterY is not; Premiere keys them as one Count")),
        (with(&[]), edit_track("tileCenterX", &|keys| keys[1]["layerTime"] = json!(600)), json!({}), format!("{all_keyed} keys differ in time or easing, but Premiere keys them as one Count")),
        (with(&[]), edit_track("tileHeight", &|keys| keys[2]["easing"] = json!({"type": "linear"})), json!({}), format!("{all_keyed} keys differ in time or easing, but Premiere keys them as one Count")),
        (with(&[]), edit_track("tileWidth", &|keys| keys[1]["value"]["value"] = json!(40.0)), json!({}), format!("its tile keys at 500 ms (tileWidth 40, tileHeight 25, tileCenterX 0.125 and tileCenterY 0.125) draw no Replicate grid: {grid_rule}")),
        (with(&[]), tile_count_tracks([2, 4, 3]).into_iter().map(|(name, mut keys)| { keys[1]["easing"] = json!({"type": "linear"}); (name, keys) }).collect(), json!({}), format!("Count keys are Linear between source times 0.000 s and 0.500 s{hold_rule}")),
        // A host whose frame is not the canvas at identity Motion.
        (with(&[]), vec![], json!({"scale": [50.0, 50.0]}), format!("static Motion moves the clip frame off the canvas; {}", super::REPLICATE_FRAME_RULE)),
        (with(&[]), vec![], json!({"rotation": 30.0}), format!("static Motion moves the clip frame off the canvas; {}", super::REPLICATE_FRAME_RULE)),
    ];
    for (fields, tracks, transform, reason) in tiles_edits {
        let (project, omissions) = export(document_with_motion_tile(
            fields.clone(),
            &tracks,
            transform,
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
            reason: format!("effects: motionTile effect 2 was not exported: {reason}"),
        };
        assert!(omissions.contains(&omission), "{fields}: {omissions:?}");
        // An omitted motionTile reports no approximation.
        assert!(
            omissions
                .iter()
                .all(|omission| omission.kind == OmissionKind::Omitted),
            "{fields}: {omissions:?}"
        );
    }
}

/// A Posterize with a whole `level` and the keys of its keyed Level (a keyed
/// static Level is its first key's).
fn posterize(enabled: bool, level: u8, animations: Vec<PrEffectParamAnimation>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Posterize(PrPosterize { level }),
        animations,
    }
}

#[test]
fn noise_amount_keys_bypass_and_current_edits_export_as_legacy() {
    let native = PrEffect {
        mask: None,
        enabled: false,
        params: PrEffectParams::Noise { amount: 5.0 },
        animations: vec![PrEffectParamAnimation {
            param: &crate::schema::NOISE_AMOUNT,
            keys: PrEffectParamKeys::Scalar(vec![
                key(0, 5.0, PrKeyframeEasing::Linear),
                key(TICKS, 25.0, PrKeyframeEasing::Hold),
            ]),
        }],
    };
    let wire = imported(vec![native.clone()]);
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks[0].1, "intensity");
    assert_eq!(tracks[0].2[0].2, 2.0);
    assert_eq!(tracks[0].2[1].2, 10.0);
    let (project, warnings) = export(wire);
    assert_eq!(exported_effects(&project), [native]);
    assert!(warnings
        .iter()
        .any(|warning| warning.reason.contains("different random kernels")));
    let expected = exported_effects(&project);
    assert_eq!(reread_effects(project), expected);

    let mut wire = document_with_effects(json!([
        {"id": 1, "enabled": false, "effect": {"type": "grain", "amount": 12.0, "size": 1.0, "softness": 0.0, "aspectRatio": 1.0, "seed": 0.0}}
    ]));
    let (project, _) = export(wire.clone());
    assert_eq!(
        exported_effects(&project)[0].params,
        PrEffectParams::Noise { amount: 30.0 }
    );
    assert!(!exported_effects(&project)[0].enabled);
    let expected = exported_effects(&project);
    assert_eq!(reread_effects(project), expected);
    for (field, value) in [
        ("size", 2.0),
        ("softness", 1.0),
        ("aspectRatio", 2.0),
        ("seed", 1.0),
        ("amount", 41.0),
    ] {
        let mut edited = wire.clone();
        edited["composition"]["layers"][0]["effects"][0]["effect"][field] = json!(value);
        let (project, warnings) = export(edited);
        assert_eq!(
            exported_effects(&project).is_empty(),
            field == "amount",
            "{field}"
        );
        assert!(!warnings.is_empty());
    }
    wire["composition"]["layers"][0]["effects"][0]["effect"]["amount"] = json!(0.0);
    assert_eq!(
        exported_effects(&export(wire).0)[0].params,
        PrEffectParams::Noise { amount: 0.0 }
    );
}

#[test]
fn noise_modern_strength_keys_and_seed_import_then_export_current_legacy() {
    let native = PrEffect {
        mask: None,
        enabled: false,
        params: PrEffectParams::ModernNoise {
            amount: 10.0,
            seed: 17.0,
        },
        animations: vec![PrEffectParamAnimation {
            param: &crate::schema::MODERN_NOISE.params[4],
            keys: PrEffectParamKeys::Scalar(vec![
                key(0, 10.0, PrKeyframeEasing::Linear),
                key(TICKS, 30.0, PrKeyframeEasing::Hold),
            ]),
        }],
    };
    let wire = imported(vec![native]);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"]["seed"],
        json!(17.0)
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks[0].1, "intensity");
    assert_eq!(tracks[0].2[0].2, 4.0);
    assert_eq!(tracks[0].2[1].2, 12.0);
    let (project, warnings) = export(wire);
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].params, PrEffectParams::Noise { amount: 10.0 });
    assert!(!effects[0].enabled);
    assert_eq!(effects[0].animations[0].param, &crate::schema::NOISE_AMOUNT);
    assert_eq!(
        effects[0].animations[0].keys.scalar().unwrap()[1].value,
        30.0
    );
    assert!(warnings
        .iter()
        .any(|warning| warning.reason.contains("seed edits are lost")));
    assert_eq!(reread_effects(project), effects);
}

#[test]
fn noise_authorable_intensity_keys_export_and_conflicting_aliases_omit() {
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "grain", "amount": 12.0, "size": 1.0, "softness": 0.0, "aspectRatio": 1.0, "seed": 0.0}}
    ]));
    let intensity = json!({
        "target": {"kind": "effectProperty", "effectId": 1, "paramName": "intensity"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "noise-a", "layerTime": 0, "value": {"type": "float", "value": 2.0}, "easing": {"type": "linear"}},
            {"id": "noise-b", "layerTime": 1000, "value": {"type": "float", "value": 10.0}, "easing": {"type": "hold"}}
        ]}
    });
    wire["composition"]["dynamics"] = json!({"entries": [intensity.clone()]});
    let (project, _) = export(wire.clone());
    let expected = vec![PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Noise { amount: 5.0 },
        animations: vec![PrEffectParamAnimation {
            param: &crate::schema::NOISE_AMOUNT,
            keys: PrEffectParamKeys::Scalar(vec![
                key(0, 5.0, PrKeyframeEasing::Linear),
                key(TICKS, 25.0, PrKeyframeEasing::Hold),
            ]),
        }],
    }];
    assert_eq!(exported_effects(&project), expected);
    assert_eq!(reread_effects(project), expected);

    let mut amount = intensity.clone();
    amount["target"]["paramName"] = json!("amount");
    amount["animator"]["keyframes"][0]["id"] = json!("noise-amount-a");
    amount["animator"]["keyframes"][1]["id"] = json!("noise-amount-b");
    wire["composition"]["dynamics"]["entries"] = json!([amount.clone()]);
    let (project, _) = export(wire.clone());
    assert_eq!(exported_effects(&project), expected);
    assert_eq!(reread_effects(project), expected);

    for entries in [json!([intensity, amount]), json!([amount, intensity])] {
        wire["composition"]["dynamics"]["entries"] = entries;
        let (project, warnings) = export(wire.clone());
        assert!(exported_effects(&project).is_empty());
        assert!(warnings.iter().any(|warning| warning
            .reason
            .contains("multiple amount/intensity animation tracks")));
    }
}

/// The FX `posterize` of a Posterize: the same Level, written out.
fn fx_posterize(id: u64, enabled: bool, levels: f64) -> Value {
    json!({"id": id, "enabled": enabled, "effect": {"type": "posterize", "levels": levels}})
}

/// Clip D's Level keys of the `feature_posterize_strict` fixture: 3 at
/// source 1 s, `second` at 1.5 s and 5 at 2.5 s, Hold into the second and
/// third keys.
fn posterize_hold_keys(second: f64) -> Vec<PrEffectParamAnimation> {
    use PrKeyframeEasing::{Hold, Linear};
    vec![PrEffectParamAnimation {
        param: &POSTERIZE_LEVEL,
        keys: PrEffectParamKeys::Scalar(vec![
            key(TICKS, 3.0, Linear),
            key(3 * TICKS / 2, second, Hold),
            key(5 * TICKS / 2, 5.0, Hold),
        ]),
    }]
}

/// The report of `record`'s Posterize named `effect`, which every converted
/// Posterize makes in both directions.
fn posterize_approximation(record: &str, effect: &str) -> Omission {
    Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Approximated,
        record: record.to_owned(),
        reason: format!(
            "{effect} converts approximately: {}",
            PrPosterize::QUANTIZER_APPROXIMATION
        ),
    }
}

#[test]
fn keyed_posterize_imports_as_a_hold_level_track_and_exports_back_approximately() {
    // Source In 0.5 s, as the fixture's clip D: its Hold keys sit above a
    // Gaussian Blur, a static Level 16 and a bypassed default 7.
    let native = vec![
        posterize(true, 3, posterize_hold_keys(8.0)),
        blur(true, 10.0, false),
        posterize(true, 16, vec![]),
        posterize(false, 7, vec![]),
    ];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.in_ticks, clip.out_ticks) = (TICKS / 2, 7 * TICKS / 2);
    clip.effects = native.clone();
    let media = crate::tests::support::video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    // Every Level is written out, bypass and order kept, and each Posterize
    // reports its quantizer once.
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([
            fx_posterize(1, true, 3.0),
            {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            fx_posterize(3, true, 16.0),
            fx_posterize(4, false, 7.0),
        ])
    );
    assert_eq!(
        omissions,
        [
            posterize_approximation("source", "Posterize effect at stack position 1"),
            posterize_approximation("source", "Posterize effect at stack position 3"),
            posterize_approximation("source", "bypassed Posterize effect at stack position 4"),
        ]
    );
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    // Layer times count from the source In: source 1.0, 1.5 and 2.5 s are
    // layer 500, 1000 and 2000 ms; the Hold into the second and third keys
    // carries over.
    let (linear, hold) = (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Hold);
    assert_eq!(
        effect_tracks(&document),
        [(
            1,
            "levels".to_owned(),
            vec![
                ("premiere-effect-1-levels-1-0".to_owned(), 500, 3.0, linear),
                ("premiere-effect-1-levels-1-1".to_owned(), 1000, 8.0, hold),
                ("premiere-effect-1-levels-1-2".to_owned(), 2000, 5.0, hold),
            ]
        )]
    );
    // Export writes the same Levels, keys, bypass and order back and reports
    // each Posterize again.
    let layer = &wire["composition"]["layers"][0];
    let record = format!("layer {} ({})", layer["id"], layer["name"]);
    let (project, omissions) = export(wire);
    let exported: Vec<_> = native.into_iter().map(current_blur_export).collect();
    assert_eq!(exported_effects(&project), exported);
    assert_eq!(
        omissions,
        [1, 3, 4]
            .map(|id| posterize_approximation(&record, &format!("effects: posterize effect {id}")))
    );
}

/// Export input: a Gaussian Blur under a `posterize` (id 2) with the `fields`
/// given and the `tracks` of its parameters, from source In 1 s: layer 0, 500
/// and 1500 ms are source 1, 1.5 and 2.5 s.
fn document_with_posterize(fields: Value, tracks: &[(&str, Value)]) -> Value {
    let mut effect = json!({"type": "posterize"});
    for (name, value) in fields.as_object().unwrap() {
        effect[name] = value.clone();
    }
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 2, "enabled": true, "effect": effect},
    ]));
    wire["composition"]["layers"][0]["sourceRange"]["start"] = json!(1000);
    wire["composition"]["layers"][0]["playback"]["mapping"]["output"]["start"] = json!(1000);
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
fn edited_posterizes_export_their_whole_levels_and_hold_keys_approximately() {
    let linear = || json!({"type": "linear"});
    let hold = || json!({"type": "hold"});
    // A Level edited to 5, and the same with Hold keys, whose first key
    // becomes the static value.
    let keys = json!([
        fx_key("l-a", 0, 3.0, linear()),
        fx_key("l-b", 500, 8.0, hold()),
        fx_key("l-c", 1500, 5.0, hold()),
    ]);
    for (tracks, expected) in [
        (vec![], posterize(true, 5, vec![])),
        (
            vec![("levels", keys)],
            posterize(true, 3, posterize_hold_keys(8.0)),
        ),
    ] {
        let (project, omissions) = export(document_with_posterize(json!({"levels": 5.0}), &tracks));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false), expected]
        );
        assert_eq!(
            omissions,
            [posterize_approximation(
                "layer 1 (\"Source\")",
                "effects: posterize effect 2"
            )]
        );
    }
}

#[test]
fn posterizes_premiere_cannot_represent_are_not_exported() {
    let linear = || json!({"type": "linear"});
    let hold = || json!({"type": "hold"});
    let bezier = || json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9});
    let track = |values: [f64; 3], easing: Value| {
        (
            "levels",
            json!([
                fx_key("l-a", 0, values[0], linear()),
                fx_key("l-b", 500, values[1], easing.clone()),
                fx_key("l-c", 1500, values[2], easing)
            ]),
        )
    };
    let levels = |value: f64| json!({"levels": value});
    let whole = " is not a whole number; Premiere's rendering of a fractional Level is unmeasured and no rounding is applied";
    let hold_rule = "; only Hold keys convert, because the FX posterize floors the levels between keys and Premiere's stepping there is unmeasured";
    // (fields, tracks, the reason)
    #[rustfmt::skip]
    let posterizes = [
        (json!({}), vec![], "levels has no value; only a posterize with its levels exports".to_owned()),
        (levels(6.5), vec![], format!("Level 6.5{whole}")),
        (levels(1.0), vec![], "Level 1 is outside Premiere's 2 to 255 range".to_owned()),
        (levels(256.0), vec![], "Level 256 is outside Premiere's 2 to 255 range".to_owned()),
        (levels(5.0), vec![track([3.0, 8.0, 5.0], linear())], format!("Level keys are Linear between source times 1.000 s and 1.500 s{hold_rule}")),
        (levels(5.0), vec![track([3.0, 8.0, 5.0], bezier())], format!("Level keys are Bézier between source times 1.000 s and 1.500 s{hold_rule}")),
        (levels(5.0), vec![track([3.0, 8.5, 5.0], hold())], format!("Level key value 8.5{whole}")),
        (levels(5.0), vec![track([3.0, 300.0, 5.0], hold())], "Level key value 300 is outside Premiere's 2 to 255 range".to_owned()),
    ];
    for (fields, tracks, reason) in posterizes {
        let (project, omissions) = export(document_with_posterize(fields.clone(), &tracks));
        // Only the Posterize is omitted, never rounded or clamped; the blur
        // before it exports.
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
                reason: format!("effects: posterize effect 2 was not exported: {reason}"),
            }],
            "{fields}"
        );
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
    // in descending `Index`: the mask at the highest
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
        {"type": "vignette", "amount": 0.5},
    ])));
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 10.0, false), posterize(true, 7, vec![])]
    );
    // Reports name an effect without an id by its stack position.
    assert_eq!(
        omissions,
        [
            posterize_approximation(
                "layer 1 (\"Source\")",
                "effects: posterize effect at stack position 2"
            ),
            Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 1 (\"Source\")".to_owned(),
                reason: "effects: vignette effect at stack position 3 was not exported: it has no Premiere effect mapping"
                    .to_owned(),
            }
        ]
    );
}

/// A Levels with its master (RGB) values in native order.
fn levels(rgb: [f64; 5], animations: Vec<PrEffectParamAnimation>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Levels(PrLevels::Master { rgb }),
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
        ("source effect on matte", |clip| {
            clip.source_effects = Some(PrSourceEffects {
                master: "MasterClip:matte".to_owned(),
                effects: vec![blur(true, 10.0, false)],
                active_transforms: 0,
            })
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

#[test]
fn sharpen_native_import_keeps_amount_defaults_keys_and_rejects_scaled_host() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_sharpen_strict.prproj");
    let xml = crate::format::read_xml(&path).unwrap();
    let (project, _) = crate::format::inspect_project_with_omissions(
        &xml,
        Some("72a26059-6f85-4033-827d-63692bb9859b"),
    )
    .unwrap();
    let native = &project.sequences[0].video_tracks[0];
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.in_ticks = TICKS / 2;
    clip.out_ticks = 7 * TICKS / 2;
    clip.effects = vec![
        native.clip(3).effects[0].clone(),
        blur(true, 10.0, false),
        native.clip(0).effects[0].clone(),
        native.clip(4).effects[0].clone(),
    ];
    clip.effects[2].enabled = false;
    let media = crate::tests::support::video_media();
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let wire = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert_eq!(omissions.len(), 3);
    assert!(omissions
        .iter()
        .all(|note| note.kind == OmissionKind::Approximated
            && note.reason.contains("different sharpening kernels")));
    let effects = &wire["composition"]["layers"][0]["effects"];
    assert_eq!(
        effects[0]["effect"],
        json!({"type": "sharpen", "amount": 20.0})
    );
    assert_eq!(effects[1]["effect"]["type"], "gaussianBlur");
    assert_eq!(
        effects[2],
        json!({"id": 3, "enabled": false, "effect": {"type": "sharpen", "amount": 0.0}})
    );
    assert_eq!(effects[3]["effect"]["amount"], 4000.0);
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks[0].1, "amount");
    assert_eq!(
        tracks[0]
            .2
            .iter()
            .map(|(_, time, value, easing)| (*time, *value, *easing))
            .collect::<Vec<_>>(),
        vec![
            (500, 20.0, PropertyKeyframeEasing::Linear),
            (1000, 80.0, PropertyKeyframeEasing::Linear),
            (2000, 50.0, PropertyKeyframeEasing::Hold),
        ]
    );
    sequence.video_tracks[0].clip_mut(0).transform = native.clip(5).transform;
    let wire = project_document(&sequence);
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
}

#[test]
fn sharpen_edited_export_keeps_default_amount_order_bypass_and_rejects_fractions() {
    for (amount, expected) in [
        (None, 40),
        (Some(0.0), 0),
        (Some(137.0), 137),
        (Some(4000.0), 4000),
    ] {
        let mut sharpen = json!({"type": "sharpen"});
        if let Some(amount) = amount {
            sharpen["amount"] = json!(amount);
        }
        let (project, omissions) = export(document_with_effects(json!([
            {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            {"id": 2, "enabled": false, "effect": sharpen},
        ])));
        let effects = exported_effects(&project);
        assert_eq!(effects[0], exported_blur(true, 10.0, false));
        assert_eq!(
            effects[1].params,
            PrEffectParams::Sharpen(crate::schema::PrSharpen { amount: expected })
        );
        assert!(!effects[1].enabled);
        assert!(
            omissions
                .iter()
                .any(|note| note.kind == OmissionKind::Approximated
                    && note.reason.contains("different sharpening kernels")),
            "{omissions:?}"
        );
        assert_eq!(reread_effects(project), effects);
    }
    for amount in [-1.0, 40.5, 4001.0] {
        let (project, omissions) = export(document_with_effects(json!([
            {"type": "sharpen", "amount": amount},
            {"type": "gaussianBlur", "blurriness": 10.0},
        ])));
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.reason.contains("was not exported")),
            "{omissions:?}"
        );
    }
}

#[test]
fn sharpen_edited_keys_use_source_in_and_invalid_key_or_host_keeps_sibling() {
    let make = |second: f64| {
        let mut wire = document_with_effects(json!([
            {"id": 1, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
            {"id": 2, "effect": {"type": "sharpen", "amount": 137.0}},
        ]));
        wire["composition"]["layers"][0]["sourceRange"]["start"] = json!(1000);
        wire["composition"]["layers"][0]["playback"]["mapping"]["output"]["start"] = json!(1000);
        wire["composition"]["dynamics"] = json!({"entries": [{
            "target": {"kind": "effectProperty", "effectId": 2, "paramName": "amount"},
            "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                fx_key("s1", 0, 21.0, json!({"type": "linear"})),
                fx_key("s2", 500, second, json!({"type": "linear"})),
                fx_key("s3", 1500, 51.0, json!({"type": "hold"})),
            ]}
        }]});
        wire
    };
    let (project, _) = export(make(81.0));
    let effects = exported_effects(&project);
    assert_eq!(
        effects[1].params,
        PrEffectParams::Sharpen(crate::schema::PrSharpen { amount: 21 })
    );
    assert_eq!(
        effects[1].animations[0].keys.scalar().unwrap(),
        &[
            key(TICKS, 21.0, PrKeyframeEasing::Linear),
            key(3 * TICKS / 2, 81.0, PrKeyframeEasing::Linear),
            key(5 * TICKS / 2, 51.0, PrKeyframeEasing::Hold),
        ]
    );
    assert_eq!(reread_effects(project), effects);
    let mut scaled = make(81.0);
    scaled["composition"]["layers"][0]["transform"]["scale"] = json!([50, 50]);
    for wire in [make(81.5), make(4001.0), scaled] {
        let (project, omissions) = export(wire);
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.reason.contains("was not exported")),
            "{omissions:?}"
        );
    }
}

#[test]
fn sharpen_host_export_omits_skew_and_depth_but_keeps_safe_sibling() {
    for fields in [
        json!({"skew": 20.0}),
        json!({"position": [0.0, 0.0, 250.0]}),
    ] {
        let mut wire = document_with_effects(json!([
            {"id": 1, "effect": {"type": "sharpen", "amount": 40.0}},
            {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        ]));
        let transform = &mut wire["composition"]["layers"][0]["transform"];
        for (name, value) in fields.as_object().unwrap() {
            transform[name] = value.clone();
        }
        let (project, omissions) = export(wire);
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.scope == OmissionScope::Feature
                    && note.record == "layer 1 (\"Source\")"
                    && note.reason.contains("sharpen effect 1 was not exported")
                    && note.reason.contains("identity")),
            "{omissions:?}"
        );
    }
}

#[test]
fn sharpen_host_cubic_amount_keys_are_explicitly_rejected() {
    let mut wire = document_with_effects(json!([
        {"id": 1, "effect": {"type": "sharpen", "amount": 40.0}},
        {"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
    ]));
    wire["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 1, "paramName": "amount"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            fx_key("s1", 0, 20.0, json!({"type": "linear"})),
            fx_key("s2", 500, 80.0, json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.75, "y2": 0.9})),
        ]}
    }]});
    let (project, omissions) = export(wire);
    assert_eq!(
        exported_effects(&project),
        [exported_blur(true, 10.0, false)]
    );
    assert!(
        omissions.iter().any(|note| note
            .reason
            .contains("Sharpen Amount keys must be Linear or Hold; Bezier fidelity is unverified")),
        "{omissions:?}"
    );
}

// A master clip's own chain converts on each placement's picture before the
// placement's own effects. These typed placements are supplementary controls;
// the pinned native case and its XML controls are the public tests.

/// The master clip of the source-effect controls.
const SOURCE_MASTER: &str = "MasterClip:master-1";

/// `effects` as the source effects that the reader carries for a placement of
/// [`SOURCE_MASTER`].
fn source_stack(effects: Vec<PrEffect>) -> Option<PrSourceEffects> {
    Some(PrSourceEffects {
        master: SOURCE_MASTER.to_owned(),
        effects,
        active_transforms: 0,
    })
}

fn omitted(record: &str, reason: impl Into<String>) -> Omission {
    Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Omitted,
        record: record.to_owned(),
        reason: reason.into(),
    }
}

fn gaussian(id: u64, enabled: bool, blurriness: f64) -> Value {
    json!({"id": id, "enabled": enabled, "effect": {"type": "gaussianBlur", "blurriness": blurriness}})
}

#[test]
fn source_posterizes_report_each_quantizer_approximation() {
    let record = "VideoClipTrackItem:posterize-placement";
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.id = Some(record.to_owned());
    clip.source_effects = source_stack(vec![
        posterize(true, 3, vec![]),
        posterize(false, 7, vec![]),
    ]);
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([fx_posterize(1, true, 3.0), fx_posterize(2, false, 7.0)])
    );
    assert_eq!(
        omissions,
        [
            posterize_approximation(
                record,
                &format!("Posterize effect at source stack position 1 of {SOURCE_MASTER}")
            ),
            posterize_approximation(
                record,
                &format!("bypassed Posterize effect at source stack position 2 of {SOURCE_MASTER}")
            ),
            omitted(SOURCE_MASTER, super::LINKED_SOURCE_EDITING_REASON),
        ]
    );
}

#[test]
fn source_effect_keys_move_to_each_placements_clock() {
    use PrKeyframeEasing::Linear;
    // Two placements of one master clip, from source 1 s and 2 s, whose
    // source stack is a blur keyed at source 0, 3 and 6 s.
    let source = source_stack(vec![keyed(
        blur(true, 0.0, false),
        vec![
            key(0, 10.0, Linear),
            key(3 * TICKS, 40.0, Linear),
            key(6 * TICKS, 20.0, Linear),
        ],
    )]);
    let mut first = clip_of("source", TICKS..4 * TICKS, TICKS);
    first.source_effects = source.clone();
    let mut second = clip_of("source", 5 * TICKS..8 * TICKS, 2 * TICKS);
    second.source_effects = source;
    let sequence = sequence_of("Main", vec![PrVideoTrack::media([first, second])]);
    let (wire, _) = imported_with_omissions(&sequence, &video_media());
    // The source keys move to each placement's clock from its source In,
    // the keys outside its trim kept.
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let tracks: Vec<_> = effect_tracks(&document)
        .into_iter()
        .map(|(id, param, keys)| {
            let keys: Vec<_> = keys
                .into_iter()
                .map(|(_, millis, value, _)| (millis, value))
                .collect();
            (id, param, keys)
        })
        .collect();
    assert_eq!(
        tracks,
        [
            (
                1,
                "blurriness".to_owned(),
                vec![(-1000, 10.0), (2000, 40.0), (5000, 20.0)]
            ),
            (
                2,
                "blurriness".to_owned(),
                vec![(-2000, 10.0), (1000, 40.0), (4000, 20.0)]
            ),
        ]
    );
}

#[test]
fn source_effects_stage_the_mask_of_their_placement_after_them() {
    // A Linear Wipe or Opacity mask that converts on one video layer moves to
    // a stage group with the source effects, so that they apply first. The
    // group carries no effect, so none applies twice.
    type Edit = fn(&mut crate::schema::PrVideoOccurrence);
    let cases: [(&str, Edit); 2] = [
        ("Linear Wipe", |clip| {
            clip.linear_wipe = Some(crate::schema::PrLinearWipe {
                initial_completion: 50.0,
                completion: vec![key(0, 50.0, PrKeyframeEasing::Linear)],
                angle_degrees: 90,
                feather: 0.0,
            })
        }),
        ("Opacity mask", |clip| {
            clip.opacity_mask = Some(crate::schema::PrMask {
                raster: None,
                feather: 0.0,
                ..opacity_mask()
            })
        }),
    ];
    for (case, edit) in cases {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        edit(clip);
        clip.source_effects = source_stack(vec![blur(true, 10.0, false)]);
        let (staged, omissions) = imported_with_omissions(&sequence, &video_media());
        let group = &staged["composition"]["layers"][0];
        assert_eq!(
            (&group["type"], group["masks"].as_array().map(Vec::len)),
            (&json!("Group"), Some(1)),
            "{case}"
        );
        assert!(group.get("effects").is_none(), "{case}: {group}");
        let video = &group["layers"][0];
        assert_eq!(video["effects"], json!([gaussian(1, true, 10.0)]), "{case}");
        assert!(video.get("masks").is_none(), "{case}");
        assert_eq!(
            omissions,
            [omitted(SOURCE_MASTER, super::LINKED_SOURCE_EDITING_REASON)],
            "{case}"
        );
    }
}

#[test]
fn a_placement_whose_own_effect_follows_its_mask_converts_without_source_effects() {
    // Source effects before a Crop and the clip's own blur after it are on
    // both sides of the Crop, which one FX mask cannot order, as for the
    // clip's own effects (`MASK_EFFECT_ORDER_REASON`). The clip converts as
    // it does without them: its Crop and blur on one video layer.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.crop = left_crop();
    clip.effects = vec![blur(true, 10.0, false)];
    clip.source_effects = source_stack(vec![blur(true, 40.0, false)]);
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    let video = &wire["composition"]["layers"][0];
    assert_eq!(video["type"], "Video");
    assert_eq!(video["masks"].as_array().map(Vec::len), Some(1));
    assert_eq!(video["effects"], json!([gaussian(1, true, 10.0)]));
    assert_eq!(
        omissions,
        [
            omitted(
                "source",
                "source effects of MasterClip:master-1 were not imported: they apply before the Crop, Linear Wipe or Track Matte Key, and effects of the clip's own chain after it; one FX mask keeps the effects of only one side in order"
            ),
            omitted(SOURCE_MASTER, crate::schema::SOURCE_CHAIN_NOT_CONVERTED),
        ]
    );
}

#[test]
fn retimed_source_effect_keys_keep_source_times_values_and_easing() {
    use PrKeyframeEasing::{CubicBezier, Hold, Linear};
    let curve = CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.75,
        y2: 0.9,
    };
    for (rate, staged) in [
        (2.0_f64, false),
        (-2.0, false),
        (0.5, false),
        (-0.5, false),
        (2.0, true),
        (-2.0, true),
    ] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.start_ticks = TICKS;
        clip.end_ticks = 4 * TICKS;
        clip.in_ticks = TICKS;
        clip.out_ticks = TICKS + (3.0 * rate.abs() * TICKS as f64) as i64;
        clip.playback_rate = rate;
        if staged {
            clip.crop = left_crop();
        }
        clip.source_effects = source_stack(vec![keyed(
            blur(true, 0.0, false),
            vec![
                key(0, 10.0, Hold),
                key(3 * TICKS, 40.0, curve),
                key(6 * TICKS, 20.0, Linear),
            ],
        )]);
        let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
        let (native, export_reports) = export(wire.clone());
        let native_effects = exported_effects(&native);
        assert_eq!(
            native_effects.len(),
            1,
            "{rate}/{staged}: {export_reports:?}"
        );
        let PrEffectParamKeys::Scalar(keys) = &native_effects[0].animations[0].keys else {
            panic!("expected scalar blur keys");
        };
        assert_eq!(
            keys.iter()
                .map(|k| (k.source_ticks, k.easing))
                .collect::<Vec<_>>(),
            [(0, Hold), (3 * TICKS, curve), (6 * TICKS, Linear)]
        );
        let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
        let tracks = effect_tracks(&document);
        assert_eq!(tracks.len(), 1, "{rate}: {omissions:?}");
        let actual: Vec<_> = tracks[0]
            .2
            .iter()
            .map(|(_, time, value, easing)| (*time, *value, *easing))
            .collect();
        assert_eq!(
            actual,
            [
                (0, 10.0, PropertyKeyframeEasing::Hold),
                (3000, 40.0, super::keyframes::fx_easing(curve)),
                (6000, 20.0, PropertyKeyframeEasing::Linear),
            ]
        );
        assert!(
            omissions
                .iter()
                .any(|o| o.kind == OmissionKind::Approximated
                    && o.reason.contains(super::RETIMED_EFFECT_CLOCK_APPROXIMATION)),
            "{omissions:?}"
        );
        assert!(
            !omissions
                .iter()
                .any(|o| o.reason.contains("animation at source stack")
                    && o.kind == OmissionKind::Omitted),
            "{omissions:?}"
        );
    }
}

#[test]
fn source_effects_keep_the_frame_rules_of_the_picture_they_draw_on() {
    let reason = |name: &str, why: String| {
        omitted(
            "source",
            format!("{name} effect at source stack position 1 of {SOURCE_MASTER} was not imported: {why}"),
        )
    };
    let linked = omitted(SOURCE_MASTER, super::LINKED_SOURCE_EDITING_REASON);
    let identity = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let vertical = ([0.5, 0.0], [0.5, 1.0]);

    // 1280 x 720 media: a Ramp needs the clip frame to be the canvas, and a
    // Corner Pin normalizes to the clip's own frame, as for the clip's own.
    let mut small = video_media();
    let stream = small
        .get_mut(&crate::schema::MediaId("source".into()))
        .unwrap()
        .video
        .as_mut()
        .unwrap();
    (stream.width, stream.height) = (1280, 720);
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).source_effects = source_stack(vec![
        ramp(
            true,
            vertical,
            ([0, 0, 0], [255, 255, 255]),
            0.0,
            Vec::new(),
        ),
        corner_pin(true, identity, Vec::new()),
    ]);
    let (wire, omissions) = imported_with_omissions(&sequence, &small);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"]["type"],
        "cornerPin"
    );
    assert_eq!(
        omissions,
        [
            reason(
                "Ramp",
                format!(
                    "the source frame differs from the canvas; {}",
                    super::FRAME_RULE
                )
            ),
            linked.clone(),
        ]
    );

    // Scale 50 and Rotation 30: a Directional Blur maps through the clip's
    // Motion, as the clip's own does.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    (clip.transform.scale, clip.transform.rotation) = ([50.0; 2], 30.0);
    clip.source_effects = source_stack(vec![directional_blur(true, 0.0, 30.0)]);
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([{"id": 1, "enabled": true, "effect": {"type": "directionalBlur", "direction": 30.0, "blurLength": 15.0}}])
    );
    assert_eq!(omissions, std::slice::from_ref(&linked));

    // Under the stage group of a Crop, a Directional Blur and a Mosaic are
    // omitted as the clip's own are there; the blur after them converts.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.crop = left_crop();
    clip.source_effects = source_stack(vec![
        directional_blur(true, 0.0, 30.0),
        mosaic(true, (16, 9), Vec::new()),
        blur(true, 10.0, false),
    ]);
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    let group = &wire["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    assert_eq!(
        group["layers"][0]["effects"],
        json!([gaussian(1, true, 10.0)])
    );
    let staged = |position: u32, name: &str, why: &str| {
        omitted(
            "source",
            format!("{name} effect at source stack position {position} of {SOURCE_MASTER} was not imported: {why}"),
        )
    };
    assert_eq!(
        omissions,
        [
            staged(1, "Directional Blur", super::STAGED_DIRECTIONAL_BLUR_REASON),
            staged(2, "Mosaic (Legacy)", super::STAGED_MOSAIC_REASON),
            linked,
        ]
    );
}

#[test]
fn a_source_transform_is_reported_and_the_placements_own_transform_stages_its_video() {
    // The two Transform counts stay apart: the clip's own one Transform
    // stages its video, and the master clip's is reported.
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.effects = vec![transform_effect(
        crate::schema::PrTransform {
            position: [0.75, 0.5],
            ..DEFAULT_PR_TRANSFORM
        },
        Vec::new(),
    )];
    clip.active_transforms = 1;
    clip.source_effects = Some(PrSourceEffects {
        active_transforms: 1,
        ..source_stack(vec![
            transform_effect(DEFAULT_PR_TRANSFORM, Vec::new()),
            blur(true, 10.0, false),
        ])
        .unwrap()
    });
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    let group = &wire["composition"]["layers"][0];
    assert_eq!(group["type"], "Group");
    let video = &group["layers"][0];
    assert_eq!(video["effects"], json!([gaussian(1, true, 10.0)]));
    assert_eq!(video["transform"]["position"][0], json!(1440.0));
    assert_eq!(
        omissions,
        [
            omitted(
                "source",
                format!(
                    "Transform effect at source stack position 1 of {SOURCE_MASTER} was not imported: {}",
                    super::SOURCE_TRANSFORM_REASON
                )
            ),
            omitted(SOURCE_MASTER, super::LINKED_SOURCE_EDITING_REASON),
        ]
    );
}

#[test]
fn a_source_stack_that_converts_nothing_is_reported_as_not_converted() {
    // Every source effect is omitted: a Transform never converts, and a Ramp
    // needs its picture at identity Motion on a canvas frame, which this
    // moved clip is not. The chain is reported as not converted and no
    // copy's linked editing is claimed. On a Crop the stack still stages the
    // clip's mask, as the order of its effects needs; its picture then
    // carries none of them, under a group that the Crop alone would not need.
    let source = source_stack(vec![
        transform_effect(DEFAULT_PR_TRANSFORM, Vec::new()),
        ramp(
            true,
            ([0.5, 0.0], [0.5, 1.0]),
            ([0, 0, 0], [255, 255, 255]),
            0.0,
            Vec::new(),
        ),
    ]);
    for masked in [false, true] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.transform.scale = [50.0; 2];
        clip.source_effects = source.clone();
        if masked {
            clip.crop = left_crop();
        }
        let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
        let root = &wire["composition"]["layers"][0];
        let picture = if masked {
            assert_eq!(
                (&root["type"], root["masks"].as_array().map(Vec::len)),
                (&json!("Group"), Some(1))
            );
            &root["layers"][0]
        } else {
            assert_eq!(root["type"], "Video");
            root
        };
        assert!(picture.get("effects").is_none(), "{picture}");
        let reasons: Vec<_> = omissions
            .iter()
            .map(|omission| (omission.record.as_str(), omission.reason.as_str()))
            .collect();
        let ramp = if masked {
            super::STAGED_RAMP_REASON.to_owned()
        } else {
            format!(
                "static Motion moves the clip frame off the canvas; {}",
                super::FRAME_RULE
            )
        };
        assert_eq!(reasons.len(), 3, "{reasons:?}");
        assert_eq!(
            reasons[0],
            (
                "source",
                format!(
                    "Transform effect at source stack position 1 of {SOURCE_MASTER} was not imported: {}",
                    super::SOURCE_TRANSFORM_REASON
                )
                .as_str()
            )
        );
        assert!(
            reasons[1].0 == "source"
                && reasons[1].1.starts_with(&format!(
                    "Ramp effect at source stack position 2 of {SOURCE_MASTER} was not imported: "
                ))
                && reasons[1].1.ends_with(&ramp),
            "{masked}: {reasons:?}"
        );
        assert_eq!(
            reasons[2],
            (SOURCE_MASTER, crate::schema::SOURCE_CHAIN_NOT_CONVERTED)
        );
    }
}

#[test]
fn a_curved_corner_path_is_never_drawn_as_its_chords() {
    // Only a master clip's Corner Pin reaches import with a curved path, and
    // `corner_path::straighten` turns it into straight keys first. A curved
    // path on the clip's own Corner Pin, which the reader rejects and only a
    // typed control can carry, is omitted rather than drawn along its chords.
    let mut curved = point_key(TICKS, [0.2, 0.1], PrKeyframeEasing::Linear);
    curved.spatial_in_tangent = Some([0.0, -0.1]);
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).effects = vec![corner_pin(
        true,
        [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        vec![corner_keys(
            0,
            vec![point_key(0, [0.0, 0.0], PrKeyframeEasing::Linear), curved],
        )],
    )];
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    assert!(
        wire["composition"]["layers"][0].get("effects").is_none(),
        "{wire}"
    );
    assert_eq!(
        omissions,
        [omitted(
            "source",
            "Corner Pin effect at stack position 1 was not imported: keyframed Upper Left moves on a curved spatial path that was not straightened"
        )]
    );
}

#[test]
fn source_replicate_stays_omitted_without_losing_safe_source_or_clip_effects() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.source_effects = source_stack(vec![replicate(true, 2, vec![]), blur(true, 10.0, false)]);
    clip.effects = vec![blur(true, 25.0, false)];
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([gaussian(1, true, 10.0), gaussian(2, true, 25.0)])
    );
    assert_eq!(
        omissions,
        [
            omitted(
                "source",
                "Replicate effect at source stack position 1 of MasterClip:master-1 was not imported: Replicate among a master clip's source effects is not converted; its processing order is unverified"
            ),
            omitted(SOURCE_MASTER, super::LINKED_SOURCE_EDITING_REASON),
        ]
    );
}

#[test]
fn shift_channels_exports_only_static_own_or_constant_routes() {
    let effect = json!({"id": 1, "effect": {"type": "shiftChannels", "takeRedFrom": "fullOn", "takeGreenFrom": "green", "takeBlueFrom": "fullOff"}});
    let safe = json!({"id": 2, "effect": {"type": "gaussianBlur", "blurriness": 10.0}});
    for (field, value, reason) in [
        ("route", json!("blue"), "cross-channel routing"),
        ("enabled", json!(false), "disabled Levels"),
        ("keys", json!(null), "only static effect parameters export"),
    ] {
        let mut wire = document_with_effects(json!([effect.clone(), safe.clone()]));
        match field {
            "route" => {
                wire["composition"]["layers"][0]["effects"][0]["effect"]["takeRedFrom"] = value
            }
            "enabled" => wire["composition"]["layers"][0]["effects"][0]["enabled"] = value,
            _ => {
                wire["composition"]["dynamics"] = json!({"entries": [{
                    "target": {"kind": "effectProperty", "effectId": 1, "paramName": "takeRedFrom"},
                    "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                        fx_key("a", 0, 0.0, json!({"type": "linear"})),
                        fx_key("b", 500, 1.0, json!({"type": "linear"})),
                    ]}
                }]})
            }
        }
        let (project, omissions) = export(wire);
        assert_eq!(
            exported_effects(&project),
            [exported_blur(true, 10.0, false)]
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(reason)),
            "{omissions:?}"
        );
    }
}

// Verbatim component 560 and parameters 903–922 from the human-authored
// human-inputs-20261004.prproj, saved in Premiere Pro 2026, SHA-256
// e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76.
// Source: effects sequence, UID b3aecac7-c452-48ba-a5e1-737807cb32ee.
// Only those records were extracted offline; the original was not modified.
// The host is one-clip.xml's synthetic five-second 1080p30 placement and
// ten-second media metadata, not the original footage or placement. This tests
// native effect records and edited export structure, not original-host timing,
// independent Adobe readback, RGB or alpha fidelity. The derived XML hash below
// pins the unchanged extracted records and synthetic host together.
#[test]
fn human_levels_master_import_and_edited_export_preserve_keys() {
    use sha2::{Digest, Sha256};
    let xml = include_str!("../../../tests/fixtures/human_levels_master.xml");
    assert_eq!(
        format!("{:x}", Sha256::digest(xml.as_bytes())),
        "cddbe927f29896c15001ea1506382c3fd57c9b902e2adfcbeb27dee1a6fa8ade"
    );
    let (native, omissions) = crate::format::inspect_project_with_omissions(xml, None).unwrap();
    let sequence = native.single_sequence().unwrap();
    let effects = &sequence.video_tracks[0].clip(0).effects;
    assert_eq!(effects.len(), 2, "{omissions:?}");
    assert_eq!(
        effects[0].params,
        PrEffectParams::Levels(PrLevels::Master {
            rgb: [30.0, 255.0, 0.0, 255.0, 100.0]
        })
    );
    let keys = effects[0].animations[0].keys.scalar().unwrap();
    assert_eq!(
        keys,
        [
            key(0, 30.0, PrKeyframeEasing::Linear),
            key(561_741_701_862, 60.0, PrKeyframeEasing::Linear),
        ]
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    // Preserve the independent scalar-master contract. The complete channel
    // graph, including copied master keys, is asserted by the channel test.
    let mut scalar_sequence = sequence.clone();
    scalar_sequence.video_tracks[0]
        .clip_mut(0)
        .effects
        .truncate(1);
    let (mut wire, omissions) = imported_with_omissions(&scalar_sequence, &native.media);
    assert!(omissions.is_empty(), "{omissions:?}");
    let effect = &mut wire["composition"]["layers"][0]["effects"][0]["effect"];
    assert_eq!(
        effect,
        &json!({"type":"levels", "inputBlack":30.0,
        "inputWhite":255.0,"gamma":1.0,"outputBlack":0.0,"outputWhite":255.0})
    );
    *effect = json!({"type":"levels", "inputBlack":40.0,
        "inputWhite":230.0,"gamma":1.25,"outputBlack":5.0,"outputWhite":245.0});
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["target"]["paramName"], "inputBlack");
    let keys = entries[0]["animator"]["keyframes"].as_array_mut().unwrap();
    assert_eq!(keys.len(), 2);
    for (key, (time, original, edited)) in
        keys.iter_mut().zip([(0, 30.0, 40.0), (2211, 60.0, 75.0)])
    {
        assert_eq!(key["layerTime"], time);
        assert_eq!(key["value"]["value"], original);
        assert_eq!(key["easing"]["type"], "linear");
        key["value"]["value"] = json!(edited);
    }
    let (written, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let reread = reread_effects(written);
    assert_eq!(reread.len(), 1);
    let PrEffectParams::Levels(levels) = &reread[0].params else {
        panic!("expected Levels")
    };
    assert_eq!(
        levels.start_values(),
        [
            40.0, 230.0, 5.0, 245.0, 125.0, 0.0, 255.0, 0.0, 255.0, 100.0, 0.0, 255.0, 0.0, 255.0,
            100.0, 0.0, 255.0, 0.0, 255.0, 100.0,
        ]
    );
    assert_eq!(
        reread[0].animations[0].keys.scalar().unwrap(),
        [
            key(0, 40.0, PrKeyframeEasing::Linear),
            key(2211 * TICKS_PER_MILLISECOND, 75.0, PrKeyframeEasing::Linear),
        ]
    );
}

#[test]
fn source_sharpen_stays_omitted_without_losing_safe_source_or_clip_effects() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.source_effects = source_stack(vec![
        PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Sharpen(crate::schema::PrSharpen { amount: 40 }),
            animations: vec![],
        },
        blur(true, 10.0, false),
    ]);
    clip.effects = vec![blur(true, 25.0, false)];
    let (wire, omissions) = imported_with_omissions(&sequence, &video_media());
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([gaussian(1, true, 10.0), gaussian(2, true, 25.0)])
    );
    assert_eq!(
        omissions,
        [
            omitted(
                "source",
                "Sharpen effect at source stack position 1 of MasterClip:master-1 was not imported: Sharpen among a master clip's source effects is not converted; its processing order is unverified"
            ),
            omitted(SOURCE_MASTER, super::LINKED_SOURCE_EDITING_REASON),
        ]
    );
}

/// Unchanged Adobe-native controls in the existing supported CPU clip harness.
/// This does not import the human project's sequence clock or source media.
fn find_edges_native_xml(fragment: &str) -> String {
    include_str!("../../../tests/fixtures/one-clip.xml")
        .replace(
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"537\"/></Components></ComponentChain></VideoComponentChain>",
        )
        .replace(
            "</PremiereData>",
            &fragment.replace("<PremiereData Version=\"3\">", ""),
        )
}

fn find_edges_import(xml: &str) -> (Value, Vec<Omission>) {
    let (native, mut omissions) = crate::format::inspect_project_with_omissions(xml, None).unwrap();
    let sequence = native.single_sequence().unwrap();
    let assets = crate::tesseract_output::asset_ids_in_order(sequence, &native.media);
    let wire =
        crate::convert::premiere_to_tesseract(sequence, &native.media, &assets, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
    (wire, omissions)
}

#[test]
fn find_edges_native_controls_import_editably_with_explicit_losses() {
    let fragment = include_str!("../../../tests/fixtures/find-edges-26.5.xml");
    let (wire, omissions) = find_edges_import(&find_edges_native_xml(fragment));
    assert_eq!(
        wire["composition"]["layers"][0]["effects"],
        json!([{"id": 1, "enabled": true, "effect": {"type": "findEdges", "invert": 0.0}}])
    );
    assert_eq!(
        wire["composition"]["layers"][0]["source"]["assetId"],
        "premiere-video-1"
    );
    assert!(wire["composition"]["dynamics"]["entries"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert!(
        omissions.iter().any(|note| {
            note.record == "VideoClipTrackItem:3"
                && note.reason.contains("Find Edges")
                && note.reason.contains("Blend With Original")
                && note.reason.contains("keys")
        }),
        "{omissions:?}"
    );
    assert!(
        omissions.iter().any(|note| {
            note.kind == OmissionKind::Approximated && note.reason.contains("grayscale Sobel")
        }),
        "{omissions:?}"
    );

    // Same native layout, opposite checkbox; not a second native witness.
    let fragment = fragment.replace(",true,0,0,0,0,0,0", ",false,0,0,0,0,0,0");
    let (wire, _) = find_edges_import(&find_edges_native_xml(&fragment));
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"]["invert"],
        1.0
    );
}

#[test]
fn find_edges_edited_export_writes_current_polarity_and_native_layout() {
    let (original, _) = find_edges_import(&find_edges_native_xml(include_str!(
        "../../../tests/fixtures/find-edges-26.5.xml"
    )));
    for (invert, enabled, expected) in [
        (Some(0.0), true, "true"),
        (Some(0.5), true, "true"),
        (Some(0.75), true, "false"),
        (Some(1.0), false, "false"),
        (None, true, "false"),
    ] {
        let mut wire = original.clone();
        let effect = &mut wire["composition"]["layers"][0]["effects"][0];
        effect["enabled"] = json!(enabled);
        if let Some(invert) = invert {
            effect["effect"]["invert"] = json!(invert);
        } else {
            effect["effect"].as_object_mut().unwrap().remove("invert");
        }
        let (mut project, omissions) = export(wire);
        assert_eq!(exported_effects(&project).len(), 1, "{omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|note| note.kind == OmissionKind::Approximated
                    && note.reason.contains("grayscale Sobel")),
            "{omissions:?}"
        );
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
        // Independent XML assertions, not this converter's native reader.
        let tree = roxmltree::Document::parse(&xml).unwrap();
        fn text<'a>(node: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
            node.children()
                .find(|child| child.has_tag_name(tag))
                .and_then(|child| child.text())
        }
        let native = tree
            .descendants()
            .find(|node| {
                node.has_tag_name("VideoFilterComponent")
                    && text(*node, "MatchName") == Some("AE.ADBE Find Edges")
            })
            .unwrap();
        assert_eq!(native.attribute("Version"), Some("9"));
        assert_eq!(
            native.attribute("ClassID"),
            Some("d10da199-beea-4dd1-b941-ed3a78766d50")
        );
        assert_eq!(text(native, "VideoFilterType"), Some("2"));
        let body = native
            .children()
            .find(|node| node.has_tag_name("Component"))
            .unwrap();
        assert_eq!(body.attribute("Version"), Some("7"));
        assert_eq!(text(body, "Bypass"), (!enabled).then_some("true"));
        assert_eq!(text(body, "Intrinsic"), None);
        let params: Vec<_> = body
            .descendants()
            .filter(|node| node.has_tag_name("Param"))
            .collect();
        assert_eq!(params.len(), 2);
        for (index, (class, name, value)) in [
            ("cc12343e-f113-4d3b-ae05-b287db77d461", None, expected),
            (
                "fe47129e-6c94-4fc0-95d5-c056a517aaf3",
                Some("Blend With Original"),
                "0.",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                params[index].attribute("Index"),
                Some(index.to_string().as_str())
            );
            let object = params[index].attribute("ObjectRef").unwrap();
            let param = tree
                .descendants()
                .find(|node| node.attribute("ObjectID") == Some(object))
                .unwrap();
            assert!(param.has_tag_name("VideoComponentParam"));
            assert_eq!(param.attribute("ClassID"), Some(class));
            assert_eq!(param.attribute("Version"), Some("10"));
            assert_eq!(text(param, "Name"), name);
            assert_eq!(
                text(param, "ParameterID"),
                Some((index + 1).to_string().as_str())
            );
            assert_eq!(
                text(param, "StartKeyframe"),
                Some(format!("-91445760000000000,{value},0,0,0,0,0,0").as_str())
            );
            for tag in [
                "Keyframes",
                "CurrentValue",
                "IsTimeVarying",
                "ParameterControlType",
            ] {
                assert_eq!(text(param, tag), None, "{tag}");
            }
            assert_eq!(text(param, "LowerBound"), (index == 1).then_some("0"));
            assert_eq!(text(param, "UpperBound"), (index == 1).then_some("1"));
        }
    }
}

#[test]
fn find_edges_unsupported_controls_keep_the_clip() {
    let fragment = include_str!("../../../tests/fixtures/find-edges-26.5.xml");
    for (fragment, reason) in [
        (fragment.replace("<ParameterID>1</ParameterID>", "<ParameterID>1</ParameterID><IsTimeVarying>true</IsTimeVarying><Keyframes>0,1,4,0,0,0,0,0;</Keyframes>"), "keyframed Invert"),
        (fragment.replace("-91445760000000000,0.25", "-91445760000000000,1.25").replace("<Keyframes>0,0.25,0,0,0,0.16666666666666666,0.24870647761579487,0.16666666666666666;510674274420,0.75,0,0,0.24870647761579487,0.16666666666666666,0,0.16666666666666666;</Keyframes>", "").replace("<IsTimeVarying>true</IsTimeVarying>", "").replace("<CurrentValue>1</CurrentValue>", ""), "not a number from 0 to 1"),
        (fragment.replace("cc12343e-f113-4d3b-ae05-b287db77d461", "fe47129e-6c94-4fc0-95d5-c056a517aaf3"), "unsupported Find Edges Invert parameter layout"),
    ] {
        let (wire, omissions) = find_edges_import(&find_edges_native_xml(&fragment));
        // The retained video, then the importer's bottom black canvas.
        let layers = wire["composition"]["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 2, "{layers:?}");
        assert_eq!(layers[0]["source"]["assetId"], "premiere-video-1");
        assert!(wire["composition"]["layers"][0]["effects"].as_array().is_none_or(Vec::is_empty));
        assert!(omissions.iter().any(|note| note.reason.contains("Find Edges") && note.reason.contains(reason)), "{omissions:?}");
    }
    let mut wire = document_with_effects(
        json!([{"id": 1, "enabled": true, "effect": {"type": "findEdges", "invert": 0.0}}]),
    );
    let mut animated = wire.clone();
    animated["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 1, "paramName": "invert"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "invert-0", "layerTime": 0, "value": {"type": "float", "value": 0.0}, "easing": {"type": "hold"}},
            {"id": "invert-1", "layerTime": 500, "value": {"type": "float", "value": 1.0}, "easing": {"type": "hold"}}
        ]}
    }]});
    let (project, omissions) = export(animated);
    assert!(exported_effects(&project).is_empty());
    assert!(
        omissions
            .iter()
            .any(|note| note.reason.contains("animated invert")),
        "{omissions:?}"
    );

    let xml = find_edges_native_xml(fragment).replace(
        "<FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
        "<FrameRect>0,0,1280,720</FrameRect></VideoStream>",
    );
    let (imported, omissions) = find_edges_import(&xml);
    // A frame-mismatched effect must not drop its video beside the canvas.
    let layers = imported["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 2, "{layers:?}");
    assert_eq!(layers[0]["source"]["assetId"], "premiere-video-1");
    assert!(imported["composition"]["layers"][0]["effects"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert!(
        omissions
            .iter()
            .any(|note| note.reason.contains("Find Edges") && note.reason.contains("frame")),
        "{omissions:?}"
    );

    wire["composition"]["layers"][0]["transform"]["scale"] = json!([200.0, 200.0]);
    let (project, omissions) = export(wire);
    assert!(exported_effects(&project).is_empty());
    assert!(
        omissions
            .iter()
            .any(|note| note.reason.contains("Find Edges") && note.reason.contains("frame")),
        "{omissions:?}"
    );
}

// Native human-inputs-20261004.prproj SHA-256
// e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76,
// effects sequence b3aecac7-c452-48ba-a5e1-737807cb32ee: component 557,
// parameters 772–901 copied verbatim offline into one-clip.xml's synthetic host.
// No claim of original placement fidelity or scalar authority over Lumetri blobs.
#[test]
fn human_lumetri_saved_contrast_replacement_imports_and_exports_edits() {
    use sha2::{Digest, Sha256};
    let xml = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    assert_eq!(
        format!("{:x}", Sha256::digest(xml.as_bytes())),
        "4af7a4075c31e3735461074a18aeb436ff03ec7bfc85c9edd37be08907dac05c"
    );
    let (native, omissions) = crate::format::inspect_project_with_omissions(xml, None).unwrap();
    let sequence = native.single_sequence().unwrap();
    let effects = &sequence.video_tracks[0].clip(0).effects;
    assert_eq!(effects.len(), 6, "{omissions:?}");
    assert_eq!(
        effects[1].params,
        PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness: 0.0,
            contrast: 25.0
        })
    );
    assert!(effects[0].animations.is_empty());
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].record, "VideoFilterComponent:557");
    assert_eq!(omissions[0].kind, OmissionKind::Approximated);
    assert!(omissions[0].reason.contains("saved Exposure"));
    assert!(omissions[0].reason.contains("Exposure"));
    assert!(omissions[0].reason.contains("LUT"));
    assert!(omissions[0]
        .reason
        .contains("not an equivalent Lumetri transfer"));
    let (mut wire, _) = imported_with_omissions(sequence, &native.media);
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][0]["effect"]["exposure"],
        1.0
    );
    assert_eq!(
        wire["composition"]["layers"][0]["effects"][2]["effect"]["saturation"],
        30.0
    );
    let keys = wire["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
        .as_array()
        .unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["value"]["value"], 30.0);
    assert_eq!(keys[1]["value"]["value"], -40.0);
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[1]["layerTime"], 2513);
    let effect = &mut wire["composition"]["layers"][0]["effects"][1]["effect"];
    assert_eq!(effect["type"], "brightnessContrast");
    assert_eq!(effect["contrast"], 25.0);
    effect["contrast"] = json!(40.0);
    effect["brightness"] = json!(15.0);
    let (written, _) = export(wire.clone());
    assert_eq!(
        written_chain(export(wire).0),
        ["AE.ADBE Brightness & Contrast 2"]
    );
    let effects = reread_effects(written);
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].params,
        PrEffectParams::BrightnessContrast(PrBrightnessContrast {
            brightness: 15.0,
            contrast: 40.0
        })
    );
}

#[test]
fn human_lumetri_replacement_rejects_ambiguous_or_unsupported_selected_controls() {
    let xml = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    let start = xml.find("<VideoComponentParam ObjectID=\"792\"").unwrap();
    let end = start + xml[start..].find("</VideoComponentParam>").unwrap();
    for (from, to, reason) in [
        (
            "25.,0,0,0,0,0,0",
            "125.,0,0,0,0,0,0",
            "outside replacement range",
        ),
        (
            "<ParameterID>12</ParameterID>",
            "<ParameterID>13</ParameterID>",
            "duplicate Lumetri ParameterID",
        ),
        (
            "<Name>Contrast</Name>",
            "<Name>Different</Name>",
            "unexpected Lumetri Contrast name",
        ),
        (
            "<StartKeyframe>",
            "<Keyframes>0,25,0,0,0,0,0,0;</Keyframes><StartKeyframe>",
            "requires static",
        ),
    ] {
        assert!(xml[start..end].contains(from));
        let mut modified = format!(
            "{}{}{}",
            &xml[..start],
            xml[start..end].replace(from, to),
            &xml[end..]
        );
        if to.starts_with("125.") {
            modified = modified.replace(
                "<CurrentValue>25</CurrentValue>",
                "<CurrentValue>125</CurrentValue>",
            );
        }
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&modified, None).unwrap();
        let effects = &project.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .effects;
        if reason == "duplicate Lumetri ParameterID" {
            assert!(effects.is_empty());
        } else {
            assert_eq!(effects.len(), 5, "{omissions:?}");
            assert_eq!(effects[0].params, PrEffectParams::LumetriExposure(1.0));
            assert_eq!(effects[1].params, PrEffectParams::LumetriSaturation(130.0));
            assert_eq!(effects[1].animations[0].keys.scalar().unwrap().len(), 2);
            assert!(omissions
                .iter()
                .any(|omission| omission.record == "VideoFilterComponent:557"
                    && omission.reason.contains("Contrast replacement omitted")));
        }
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(reason)),
            "{omissions:?}"
        );
    }
}

#[test]
fn human_lumetri_basic_bypass_preserves_independent_vignette_enable() {
    let xml = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    let start = xml.find("<VideoComponentParam ObjectID=\"775\"").unwrap();
    let end = start + xml[start..].find("</VideoComponentParam>").unwrap();
    let control = xml[start..end]
        .replace("-91445760000000000,true,", "-91445760000000000,false,")
        .replace(
            "<CurrentValue>true</CurrentValue>",
            "<CurrentValue>false</CurrentValue>",
        );
    assert_ne!(control, xml[start..end]);
    let xml = format!("{}{}{}", &xml[..start], control, &xml[end..]);
    let (native, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    let effects = &native.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .effects;
    assert_eq!(effects.len(), 6, "{omissions:?}");
    assert!(effects[..5].iter().all(|effect| !effect.enabled));
    assert!(effects[5].enabled);
}

#[test]
fn human_lumetri_wrong_basic_enable_name_preserves_vignette() {
    let xml = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    let start = xml.find("<VideoComponentParam ObjectID=\"775\"").unwrap();
    let end = start + xml[start..].find("</VideoComponentParam>").unwrap();
    let control = xml[start..end].replace("<Name> </Name>", "<Name>Different</Name>");
    assert_ne!(control, xml[start..end]);
    let modified = format!("{}{}{}", &xml[..start], control, &xml[end..]);
    let (project, omissions) =
        crate::format::inspect_project_with_omissions(&modified, None).unwrap();
    let effects = &project.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .effects;
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].params,
        PrEffectParams::LumetriVignette([0.0, 50.0, 50.0])
    );
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("unexpected Lumetri Basic Correction enable name")),
        "{omissions:?}"
    );
}

// Verbatim Offset records from vhs_slideshow.prproj, SHA-256
// 2ff4967e295dccde90fda159aab8b10ddb3800c8d8e92ea50e0a8e1a56ed207a:
// components889 (static) and927 (keyed), each in one-clip.xml's synthetic host.
// Original clocks/bytes preserved; this is not original placement or pixel proof.
#[test]
fn native_offset_imports_fully_wet_motion_tile_and_straightened_center_keys() {
    use sha2::{Digest, Sha256};
    for (xml, hash, key_count) in [
        (
            include_str!("../../../tests/fixtures/native_offset_static.xml"),
            "ed67c0546a2416650f6e12959e6447c47319382818095c50b5de32336c90a5b4",
            0,
        ),
        (
            include_str!("../../../tests/fixtures/native_offset_keyed.xml"),
            "329db922f88276a4a6350ffbe733895cbcfbfc76a6956f1da3431add8399c84a",
            4,
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(xml.as_bytes())), hash);
        let (native, omissions) = crate::format::inspect_project_with_omissions(xml, None).unwrap();
        let sequence = native.single_sequence().unwrap();
        let effects = &sequence.video_tracks[0].clip(0).effects;
        assert_eq!(effects.len(), 1, "{omissions:?}");
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(omissions[0].kind, OmissionKind::Approximated);
        assert!(omissions[0].reason.contains("Blend With Original"));
        assert!(omissions[0].reason.contains("fully wet"));
        let center = if key_count == 0 {
            [0.5093749761581421, 0.5]
        } else {
            [0.5, 0.5]
        };
        assert_eq!(effects[0].params, PrEffectParams::Offset(center));
        if key_count != 0 {
            let keys = effects[0].animations[0].keys.point().unwrap();
            assert_eq!(keys.len(), key_count);
            assert_eq!(keys[0].source_ticks, 914617723219200);
            assert_eq!(keys[1].value, [0.5, 0.42345675826072693]);
            assert!(keys
                .iter()
                .all(|key| key.spatial_in_tangent.is_none() && key.spatial_out_tangent.is_none()));
        }
        let (wire, losses) = imported_with_omissions(sequence, &native.media);
        assert!(losses.is_empty(), "{losses:?}");
        let effect = &wire["composition"]["layers"][0]["effects"][0]["effect"];
        assert_eq!(effect["type"], "motionTile");
        assert_eq!(effect["tileCenterX"], center[0]);
        assert_eq!(effect["tileWidth"], 100.0);
        assert_eq!(effect["outputHeight"], 100.0);
        assert_eq!(effect["mirrorEdges"], false);
    }
}

#[test]
fn native_offset_rejects_ambiguous_center_but_preserves_bypass() {
    let source = include_str!("../../../tests/fixtures/native_offset_static.xml");
    for (from, to, reason) in [
        (
            "<Name>Shift Center To</Name>",
            "<Name>Different</Name>",
            "unexpected Offset center name",
        ),
        (
            "<ParameterID>2</ParameterID>",
            "<ParameterID>1</ParameterID>",
            "duplicate Offset ParameterID",
        ),
    ] {
        let (native, omissions) =
            crate::format::inspect_project_with_omissions(&source.replace(from, to), None).unwrap();
        assert!(native.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .effects
            .is_empty());
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(reason)),
            "{omissions:?}"
        );
    }
    let (native, omissions) = crate::format::inspect_project_with_omissions(
        &source.replace("<Bypass>false</Bypass>", "<Bypass>true</Bypass>"),
        None,
    )
    .unwrap();
    let effects = &native.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .effects;
    assert_eq!(effects.len(), 1, "{omissions:?}");
    assert!(!effects[0].enabled);
}

#[test]
fn sharpen_still_import_omits_unverified_host_and_keeps_keyed_sibling() {
    let (sequence, media) = still_over_video(|still| {
        still.effects = vec![
            PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::Sharpen(crate::schema::PrSharpen { amount: 40 }),
                animations: vec![PrEffectParamAnimation {
                    param: &crate::schema::SHARPEN_AMOUNT,
                    keys: PrEffectParamKeys::Scalar(vec![key(
                        STILL_IN_TICKS,
                        40.0,
                        PrKeyframeEasing::Hold,
                    )]),
                }],
            },
            keyed_brightness(
                15.0,
                vec![key(STILL_IN_TICKS, 12.0, PrKeyframeEasing::Hold)],
            ),
        ];
    });
    let (wire, omissions) = imported_with_omissions(&sequence, &media);
    let image = &wire["composition"]["layers"][0];
    assert_eq!(image["type"], "Image");
    assert_eq!(
        image["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "brightnessContrast", "brightness": 12.0, "contrast": 15.0}},
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].1, "brightness");
    assert_eq!((tracks[0].2[0].1, tracks[0].2[0].2), (0, 12.0));
    assert!(
        omissions
            .iter()
            .any(|note| note.scope == OmissionScope::Feature
                && note.kind == OmissionKind::Omitted
                && note.record == "photo"
                && note
                    .reason
                    .contains("Sharpen effect at stack position 1 was not imported")
                && note.reason.contains("still")),
        "{omissions:?}"
    );
}

#[test]
fn replicate_still_import_omits_unverified_host_and_keeps_keyed_sibling() {
    let (sequence, media) = still_over_video(|still| {
        still.effects = vec![
            PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::Replicate(crate::schema::PrReplicate { count: 2 }),
                animations: vec![PrEffectParamAnimation {
                    param: &crate::schema::REPLICATE_COUNT,
                    keys: PrEffectParamKeys::Scalar(vec![key(
                        STILL_IN_TICKS,
                        2.0,
                        PrKeyframeEasing::Hold,
                    )]),
                }],
            },
            keyed_brightness(
                15.0,
                vec![key(STILL_IN_TICKS, 12.0, PrKeyframeEasing::Hold)],
            ),
        ];
    });
    let (wire, omissions) = imported_with_omissions(&sequence, &media);
    let image = &wire["composition"]["layers"][0];
    assert_eq!(image["type"], "Image");
    assert_eq!(
        image["effects"],
        json!([
            {"id": 1, "enabled": true, "effect": {"type": "brightnessContrast", "brightness": 12.0, "contrast": 15.0}},
        ])
    );
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let tracks = effect_tracks(&document);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].1, "brightness");
    assert_eq!((tracks[0].2[0].1, tracks[0].2[0].2), (0, 12.0));
    assert!(
        omissions
            .iter()
            .any(|note| note.scope == OmissionScope::Feature
                && note.kind == OmissionKind::Omitted
                && note.record == "photo"
                && note
                    .reason
                    .contains("Replicate effect at stack position 1 was not imported")
                && note.reason.contains("still")),
        "{omissions:?}"
    );
}

#[test]
fn human_lumetri_vignette_imports_independent_editable_radial_controls() {
    let source = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    for (amount, expected) in [("0.", 0.0), ("-2.5", 0.5), ("2.5", -0.5)] {
        // Supplementary scalar edits of the pinned native records. The actual
        // independently saved vignette is neutral; do not relabel edits native.
        let start = source
            .find("<VideoComponentParam ObjectID=\"882\"")
            .unwrap();
        let end = start + source[start..].find("</VideoComponentParam>").unwrap();
        let xml = format!(
            "{}{}{}",
            &source[..start],
            source[start..end].replace(
                "-91445760000000000,0.,",
                &format!("-91445760000000000,{amount},")
            ),
            &source[end..]
        );
        let (native, omissions) =
            crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert!(omissions[0].reason.contains("Roundness"));
        let (wire, losses) =
            imported_with_omissions(native.single_sequence().unwrap(), &native.media);
        assert!(losses.is_empty(), "{losses:?}");
        let effect = &wire["composition"]["layers"][0]["effects"][5]["effect"];
        assert_eq!(effect["type"], "vignette");
        assert_eq!(effect["amount"], expected);
        assert_eq!(effect["radius"], 0.5);
        assert_eq!(effect["feather"], 0.5);
    }
}

#[test]
fn human_lumetri_vignette_enable_and_bad_control_do_not_change_basic_correction() {
    let source = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    for (record, from, to, keep_vignette) in [
        (
            "881",
            "-91445760000000000,true,",
            "-91445760000000000,false,",
            true,
        ),
        ("882", "<Name>Amount</Name>", "<Name>Wrong</Name>", false),
    ] {
        let start = source
            .find(&format!("<VideoComponentParam ObjectID=\"{record}\""))
            .unwrap();
        let end = start + source[start..].find("</VideoComponentParam>").unwrap();
        let xml = format!(
            "{}{}{}",
            &source[..start],
            source[start..end].replace(from, to),
            &source[end..]
        );
        let (native, omissions) =
            crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        let effects = &native.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .effects;
        assert_eq!(
            effects.len(),
            if keep_vignette { 6 } else { 5 },
            "{omissions:?}"
        );
        assert!(effects[..5].iter().all(|effect| effect.enabled));
        if keep_vignette {
            assert!(!effects[5].enabled);
        } else {
            assert!(omissions
                .iter()
                .any(|omission| omission.reason.contains("Vignette replacement omitted")));
        }
    }
}

#[test]
fn human_lumetri_keyed_vignette_enable_names_control_and_retains_basic_correction() {
    let source = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    let start = source
        .find("<VideoComponentParam ObjectID=\"881\"")
        .unwrap();
    let end = start + source[start..].find("</VideoComponentParam>").unwrap();
    for animation in [
        "<IsTimeVarying>true</IsTimeVarying>",
        "<Keyframes>0,true,0,0,0,0,0,0</Keyframes>",
    ] {
        let xml = format!("{}{animation}{}", &source[..end], &source[end..]);
        let (native, omissions) =
            crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        let effects = &native.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .effects;
        assert_eq!(effects.len(), 5, "{omissions:?}");
        assert!(effects.iter().all(|effect| effect.enabled));
        assert_eq!(effects[0].params, PrEffectParams::LumetriExposure(1.0));
        assert_eq!(
            effects[1].params,
            PrEffectParams::BrightnessContrast(PrBrightnessContrast {
                brightness: 0.0,
                contrast: 25.0
            })
        );
        assert_eq!(effects[2].params, PrEffectParams::LumetriSaturation(130.0));
        assert_eq!(effects[2].animations[0].keys.scalar().unwrap().len(), 2);
        let omission = omissions
            .iter()
            .find(|omission| omission.reason.contains("Vignette replacement omitted"))
            .unwrap();
        assert!(
            omission.reason.contains("requires static Vignette enable"),
            "{omission:?}"
        );
        assert!(
            !omission.reason.contains("Basic Correction"),
            "{omission:?}"
        );
        assert!(!omission.reason.contains("Contrast"), "{omission:?}");
    }
}

#[test]
fn human_lumetri_white_balance_keeps_saved_controls_editable() {
    let source = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    for edited in [false, true] {
        let mut xml = source.to_owned();
        if edited {
            // Supplementary mutations, not independently authored nonzero proof.
            for (id, value) in [("786", "120"), ("787", "75")] {
                let start = xml
                    .find(&format!("<VideoComponentParam ObjectID=\"{id}\""))
                    .unwrap();
                let end = start + xml[start..].find("</VideoComponentParam>").unwrap();
                let replacement = xml[start..end].replace(
                    "-91445760000000000,0.,",
                    &format!("-91445760000000000,{value},"),
                );
                xml.replace_range(start..end, &replacement);
            }
        }
        let (native, omissions) =
            crate::format::inspect_project_with_omissions(&xml, None).unwrap();
        let (wire, losses) =
            imported_with_omissions(native.single_sequence().unwrap(), &native.media);
        assert!(losses.is_empty(), "{losses:?}");
        assert!(omissions
            .iter()
            .any(|note| note.reason.contains("Tint sign reversed")));
        let effects = &wire["composition"]["layers"][0]["effects"];
        assert_eq!(
            effects[3]["effect"],
            json!({"type":"temperatureTint", "temperature":if edited {40.0} else {0.0}, "tint":0.0})
        );
        assert_eq!(
            effects[4]["effect"],
            json!({"type":"temperatureTint", "temperature":0.0, "tint":if edited {-25.0} else {0.0}})
        );
    }
}

#[test]
fn human_lumetri_white_balance_affine_keys_and_invalid_control_preserve_siblings() {
    let source = include_str!("../../../tests/fixtures/human_lumetri_contrast.xml");
    let mut keyed = source.to_owned();
    for id in ["786", "787"] {
        let start = keyed
            .find(&format!("<VideoComponentParam ObjectID=\"{id}\""))
            .unwrap();
        let end = start + keyed[start..].find("</VideoComponentParam>").unwrap();
        let replacement = keyed[start..end].replace("-91445760000000000,0.,", "-91445760000000000,-300.,")
            + "<IsTimeVarying>true</IsTimeVarying><Keyframes>0,-300.,0,0,0,0,0,0;127008000000,300.,0,0,0,0,0,0;</Keyframes>";
        keyed.replace_range(start..end, &replacement);
    }
    let (native, omissions) = crate::format::inspect_project_with_omissions(&keyed, None).unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    let (wire, losses) = imported_with_omissions(native.single_sequence().unwrap(), &native.media);
    assert!(losses.is_empty(), "{losses:?}");
    for (name, expected) in [("temperature", -100.0), ("tint", 100.0)] {
        let entry = wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["paramName"] == name)
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["value"]["value"], expected);
        assert_eq!(keys[1]["value"]["value"], -expected);
        assert_eq!(keys[1]["layerTime"], 500);
    }
    let broken = source.replace("<Name>Temperature</Name>", "<Name>Unknown</Name>");
    let (native, omissions) = crate::format::inspect_project_with_omissions(&broken, None).unwrap();
    let effects = &native.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .effects;
    assert_eq!(effects.len(), 5);
    assert_eq!(effects[3].params, PrEffectParams::LumetriTint(0.0));
    assert!(omissions
        .iter()
        .any(|note| note.reason.contains("Temperature replacement omitted")));
}

#[test]
fn warp_fisheye_current_native_lens_fit_and_unsafe_center_keep_siblings() {
    for center in [0.5, 0.4] {
        let wire = document_with_effects(json!([
            {"id":1,"effect":{"type":"gaussianBlur","blurriness":10}},
            {"id":9,"effect":{"type":"fisheye","amount":20,"centerX":center,"centerY":0.5}},
            {"id":2,"effect":{"type":"gaussianBlur","blurriness":20}}
        ]));
        let (project, omissions) = export(wire);
        let effects = exported_effects(&project);
        assert_eq!(effects.len(), if center == 0.5 { 3 } else { 2 });
        if center == 0.5 {
            let PrEffectParams::LensDistortion(curvature) = effects[1].params else {
                panic!("retained native Lens")
            };
            assert!(curvature > 0. && curvature <= 100.);
            assert!(effects[1].animations.is_empty());
            assert!(omissions.iter().any(|o| o.reason.contains("quarter-frame")));
        } else {
            assert!(omissions
                .iter()
                .any(|o| o.reason.contains("center (0.5,0.5)")));
        }
    }
}

/// Supplementary boundary mutations; the CLI test owns native Alpha provenance.
#[test]
fn premiere_alpha_video_occurrence_keeps_clock_visibility_and_transfer() {
    use fx_schema::{LayerPlayback, Time, TimeRangeProperty};
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks = TICKS / 2;
    clip.end_ticks = 3 * TICKS / 2;
    clip.in_ticks = TICKS;
    clip.out_ticks = 3 * TICKS;
    clip.playback_rate = 2.0;
    let baseline = project_document(&sequence);
    let original = baseline["composition"]["layers"][0].clone();
    let effect = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Invert(PrInvert {
            channel: 15,
            blend: 0.0,
        }),
        animations: vec![],
    };
    sequence.video_tracks[0].clip_mut(0).effects = vec![effect];
    let wire = project_document(&sequence);
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    let LayerData::Group(graph) = document.composition().layers()[0].data() else {
        panic!("missing Alpha graph: {wire}")
    };
    let range = TimeRangeProperty::new(
        Time::from_millis(500),
        fx_schema::Duration::from_millis(1000),
    );
    assert_eq!(
        graph.playback,
        LayerPlayback::linear(range, range, range, 0).unwrap()
    );
    for (ms, active) in [
        (0, false),
        (499, false),
        (500, true),
        (1000, true),
        (1499, true),
        (1500, false),
        (2000, false),
    ] {
        assert_eq!(
            ms >= range.start.as_millis() && ms < range.end().as_millis(),
            active
        );
    }
    let picture = &wire["composition"]["layers"][0]["layers"][0]["layers"][0];
    for field in ["id", "source", "sourceRange", "playback", "transform"] {
        assert_eq!(picture[field], original[field], "{field}");
    }
    assert!(!picture["source"].is_null());
    let sample = &wire["composition"]["layers"][0]["layers"][1];
    assert_eq!(sample["source"], original["source"]);
    assert_eq!(sample["playback"], original["playback"]);
    assert_ne!(sample["id"], original["id"]);
    for boundary in ["disabled", "bypass", "blend"] {
        let mut seq = sequence.clone();
        let clip = seq.video_tracks[0].clip_mut(0);
        match boundary {
            "disabled" => clip.enabled = false,
            "bypass" => clip.effects[0].enabled = false,
            _ => clip.blend_mode = crate::schema::PrBlendMode::Multiply,
        };
        let wire = project_document(&seq);
        assert_eq!(
            wire["composition"]["layers"][0]["type"], "Video",
            "{boundary}: {wire}"
        );
        assert_eq!(
            wire["composition"]["layers"][0]["source"],
            original["source"]
        );
        let (native, _) = export(wire);
        let clip = native.sequences[0].video_occurrences().next().unwrap();
        if boundary == "disabled" {
            assert!(!clip.enabled);
        }
        if boundary == "blend" {
            assert_eq!(clip.blend_mode, crate::schema::PrBlendMode::Multiply);
        }
    }
}

#[test]
fn premiere_alpha_disabled_picture_keeps_live_source_audio() {
    use crate::{
        format::MediaId,
        schema::{AudioChannels, PrAudioOccurrence, PrAudioStream},
        tests::support::project_document_with_media,
    };
    let mut sequence = video_sequence();
    let mut media = video_media();
    media.get_mut(&MediaId("source".into())).unwrap().audio = Some(PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: 10 * TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48000,
    });
    sequence.audio.push(PrAudioOccurrence {
        source_channel: None,
        id: None,
        media: MediaId("source".into()),
        start_ticks: TICKS / 2,
        end_ticks: 3 * TICKS / 2,
        in_ticks: TICKS,
        out_ticks: 2 * TICKS,
        playback_rate: 1.0,
        preserve_audio_pitch: false,
        volume: fx_schema::LinearGain::new(0.5).unwrap(),
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    });
    sequence.video_tracks[0].clip_mut(0).enabled = false;
    let baseline = project_document_with_media(&sequence, &media);
    sequence.video_tracks[0].clip_mut(0).effects = vec![PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::Invert(PrInvert {
            channel: 15,
            blend: 0.0,
        }),
        animations: vec![],
    }];
    let current = project_document_with_media(&sequence, &media);
    assert_eq!(
        current, baseline,
        "disabled picture must introduce no helpers, backing, audio changes or clock shifts"
    );
    let sound = current["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["type"] == "Audio")
        .unwrap();
    assert_eq!(sound["volume"], 0.5);
    assert_eq!(sound["sourceRange"], json!({"start":1000,"duration":1000}));
}

#[test]
fn premiere_alpha_effect_mask_retains_picture_without_replacement() {
    let mut sequence = video_sequence();
    let baseline = project_document(&sequence);
    let picture: fx_schema::Layer =
        serde_json::from_value(baseline["composition"]["layers"][0].clone()).unwrap();
    sequence.video_tracks[0].clip_mut(0).effects = vec![PrEffect {
        mask: Some(opacity_mask()),
        enabled: true,
        params: PrEffectParams::Invert(PrInvert {
            blend: 0.0,
            channel: 15,
        }),
        animations: Vec::new(),
    }];
    let mut next = 100;
    let mut omissions = Vec::new();
    let canvas = [sequence.width, sequence.height];
    let retained = crate::convert::invert_alpha::lower(
        sequence.video_tracks[0].clip(0),
        canvas,
        canvas,
        picture.clone(),
        &mut next,
        &mut omissions,
    )
    .unwrap();
    assert_eq!(retained, picture);
    assert_eq!(next, 100, "no backing or helper identities allocated");
    assert!(omissions
        .iter()
        .any(|note| note.reason.contains("without masks")));
}

#[test]
fn human_levels_channel_row_is_retained_for_editable_lowering() {
    let xml = include_str!("../../../tests/fixtures/human_levels_master.xml");
    let (native, omissions) = crate::format::inspect_project_with_omissions(xml, None).unwrap();
    assert!(
        !omissions
            .iter()
            .any(|o| o.reason.contains("Levels (R) row") && o.reason.contains("was omitted")),
        "{omissions:?}"
    );
    assert_eq!(
        native.single_sequence().unwrap().video_tracks[0]
            .clip(0)
            .effects
            .len(),
        2
    );
    let (wire, notes) = imported_with_omissions(native.single_sequence().unwrap(), &native.media);
    assert!(
        notes.iter().any(|n| n.reason.contains("RGB Levels uses")),
        "{notes:?}"
    );
    let root = &wire["composition"]["layers"][0];
    assert_eq!(root["name"], "Premiere channel Levels");
    let branches = root["layers"][0]["layers"][1]["layers"].as_array().unwrap();
    for (index, (white, gamma)) in [(200.0, 0.02), (255.0, 1.0), (255.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(branches[index]["blendMode"], "screen");
        assert_eq!(branches[index]["effects"][0]["effect"]["inputBlack"], 30.0);
        assert_eq!(branches[index]["effects"][1]["effect"]["inputWhite"], white);
        assert_eq!(branches[index]["effects"][1]["effect"]["gamma"], gamma);
    }
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 3);
    for entry in entries {
        assert_eq!(entry["target"]["paramName"], "inputBlack");
        assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 2211);
    }
}

/// Supplementary placement/audio/bypass mutations of the pinned saved controls.
#[test]
fn channel_levels_video_keeps_source_clocks_audio_and_unsupported_siblings() {
    use crate::{
        schema::{AudioChannels, PrAudioOccurrence, PrAudioStream},
        tests::support::project_document_with_media,
    };
    let (native, _) = crate::format::inspect_project_with_omissions(
        include_str!("../../../tests/fixtures/human_levels_master.xml"),
        None,
    )
    .unwrap();
    let rows = native.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .effects
        .clone();
    let mut sequence = video_sequence();
    let mut media = video_media();
    let media_id = sequence.video_tracks[0].clip(0).media.clone();
    media.get_mut(&media_id).unwrap().audio = Some(PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: 10 * TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48000,
    });
    sequence.audio.push(PrAudioOccurrence {
        playback_rate: 1.0,
        preserve_audio_pitch: false,
        source_channel: None,
        id: None,
        media: media_id,
        start_ticks: TICKS / 2,
        end_ticks: 3 * TICKS / 2,
        in_ticks: TICKS,
        out_ticks: 2 * TICKS,
        volume: fx_schema::LinearGain::new(0.5).unwrap(),
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    });
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.start_ticks = TICKS / 2;
    clip.end_ticks = 3 * TICKS / 2;
    clip.in_ticks = TICKS;
    clip.out_ticks = 3 * TICKS;
    clip.playback_rate = 2.0;
    clip.effects = vec![rows[0].clone()];
    clip.effects[0].animations.clear();
    let baseline = project_document_with_media(&sequence, &media);
    sequence.video_tracks[0]
        .clip_mut(0)
        .effects
        .push(rows[1].clone());
    let result = project_document_with_media(&sequence, &media);
    let root = &result["composition"]["layers"][0];
    let leaf = &root["layers"][0]["layers"][1]["layers"][0];
    let old = &baseline["composition"]["layers"][0];
    for field in [
        "id",
        "source",
        "sourceRange",
        "playback",
        "transform",
        "timeRemap",
    ] {
        assert_eq!(leaf[field], old[field], "{field}");
    }
    let range = fx_schema::TimeRangeProperty::new(
        fx_schema::Time::from_millis(500),
        fx_schema::Duration::from_millis(1000),
    );
    let identity = fx_schema::LayerPlayback::linear(range, range, range, 0).unwrap();
    assert_eq!(root["playback"], serde_json::to_value(identity).unwrap());
    let sound = |doc: &Value| {
        let mut value = doc["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["type"] == "Audio")
            .unwrap()
            .clone();
        value.as_object_mut().unwrap().remove("id");
        value
    };
    assert_eq!(sound(&result), sound(&baseline));
    for unsupported in ["hidden", "blend", "sibling"] {
        let mut current = sequence.clone();
        let clip = current.video_tracks[0].clip_mut(0);
        match unsupported {
            "hidden" => clip.enabled = false,
            "blend" => clip.blend_mode = crate::schema::PrBlendMode::Multiply,
            _ => clip.effects.insert(0, blur(true, 10.0, false)),
        };
        let mut expected = current.clone();
        expected.video_tracks[0]
            .clip_mut(0)
            .effects
            .retain(|e| !matches!(e.params, PrEffectParams::Levels(PrLevels::Corrections(_))));
        assert_eq!(
            project_document_with_media(&current, &media),
            project_document_with_media(&expected, &media),
            "{unsupported}"
        );
    }
    for effect in &mut sequence.video_tracks[0].clip_mut(0).effects {
        effect.enabled = false;
    }
    let disabled = project_document_with_media(&sequence, &media);
    let branches = disabled["composition"]["layers"][0]["layers"][0]["layers"][1]["layers"]
        .as_array()
        .unwrap();
    for branch in &branches[..3] {
        assert_eq!(branch["effects"][0]["enabled"], false);
        assert_eq!(branch["effects"][1]["enabled"], false);
        assert_eq!(branch["effects"][2]["enabled"], true);
    }
}
