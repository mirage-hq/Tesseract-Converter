//! Text shadow mapping. Import inputs are Premiere model values and export
//! inputs are FX JSON documents, so neither direction is checked only against
//! the other's output.

use super::*;
use crate::{
    convert::{premiere_to_tesseract, tesseract_to_premiere},
    schema::{
        text::{
            PrGraphicObject, PrJustification, PrTextDocument, PrTextFrame, PrTextStroke,
            PrTextTransform, PrVerticalAlign,
        },
        PrEffect, PrEffectParams, PrGaussianBlur, PrGraphic, PrVideoItem, PrVideoTrack, TICKS,
    },
    test_support::editable_document,
    tests::support::{video_media, video_sequence},
    OmissionKind,
};
use fx_schema::{EditableFxCompositionDocument, LayerData};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The shadow of all 42 caption cues in the pinned
/// `practice_files_transcription_magic.prproj`; its omitted angle reads as the
/// inferred 135°.
const CUE_SHADOW: PrTextShadow = PrTextShadow {
    color: PrRgb([0, 0, 0]),
    opacity: 100.0,
    angle: 135.0,
    distance: 3.0,
    size: 6.0,
    blur: 12.0,
};

const RECORD: &str = "VideoClipTrackItem:20";
const UNPACKAGED_FONT: &str = "font \"MinionPro-Regular\" is not packaged in this document; import it with tsrct project import-font before preview or export.";

/// `CUE_SHADOW` with one Premiere value replaced.
fn cue_shadow_with(field: &str, value: f32) -> PrTextShadow {
    let mut shadow = CUE_SHADOW;
    *match field {
        "opacity" => &mut shadow.opacity,
        "angle" => &mut shadow.angle,
        "distance" => &mut shadow.distance,
        "size" => &mut shadow.size,
        "blur" => &mut shadow.blur,
        other => unreachable!("no shadow field {other}"),
    } = value;
    shadow
}

fn shadowed_graphic(shadow: PrTextShadow) -> PrGraphic {
    PrGraphic {
        id: Some(RECORD.into()),
        start_ticks: 0,
        end_ticks: TICKS,
        in_ticks: crate::format::FrameRate::Fps30.generator_in_ticks(),
        vector_motion: None,
        clip_motion: crate::schema::PrStaticTransform::default(),
        opacity: 100.0,
        blend_mode: crate::schema::PrBlendMode::Normal,
        animations: Vec::new(),
        opacity_mask: None,
        effect_loss: None,
        objects: vec![PrGraphicObject::Text(PrText {
            horizontal_scale: None,
            mask_source: None,
            name: "Caption".into(),
            document: PrTextDocument {
                text: "It's been".into(),
                font: "MinionPro-Regular".into(),
                size: 48.0,
                fill: Some(PrRgb([255, 255, 255])),
                stroke: None,
                shadow: Some(shadow),
                all_caps: false,
                tracking: 0.0,
                leading: 0.0,
                justification: PrJustification::Center,
                frame: PrTextFrame::Point {
                    vertical: PrVerticalAlign::Top,
                },
                background: None,
            },
            transform: PrTextTransform {
                position: [960.0, 972.0],
                anchor: [0.0, 0.0],
                scale: 100.0,
                rotation: 0.0,
                opacity: 100.0,
            },
            animations: Vec::new(),
            source_text_keys: Vec::new(),
        })],
        enabled: true,
    }
}

/// The drop shadow that importing `graphic` adds as a document's first effect,
/// and the omissions.
fn imported(graphic: &PrGraphic) -> (Option<DropShadow>, Vec<Omission>) {
    let mut omissions = Vec::new();
    let instance = import_text_shadow(
        graphic.text(),
        graphic.vector_motion.as_ref(),
        RECORD,
        &mut EffectIdAllocator::default(),
        &mut omissions,
    )
    .unwrap();
    let shadow = instance.map(|instance| match instance.data() {
        EffectData::Identified {
            id,
            enabled: true,
            effect: EffectPayload::Known(LayerEffect::DropShadow(shadow)),
            ..
        } if *id == EffectId::new(1) => shadow.clone(),
        other => panic!("expected enabled drop shadow 1, got {other:?}"),
    });
    (shadow, omissions)
}

fn assert_close(actual: [f64; 2], expected: [f64; 2]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() < 1e-9),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn caption_cue_shadow_becomes_one_editable_drop_shadow() {
    let (shadow, omissions) = imported(&shadowed_graphic(CUE_SHADOW));
    assert!(omissions.is_empty(), "{omissions:?}");
    let shadow = shadow.unwrap();
    assert!(shadow.enabled);
    assert_eq!(shadow.color, [0.0, 0.0, 0.0, 1.0]);
    // 3 px at 135° clockwise from up falls down and to the right.
    let leg = 3.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert_close(shadow.offset, [leg, leg]);
    // Blur 12 is a Gaussian σ of 1.1592 px and size 6 a 2.934 px dilation.
    assert_close(
        [shadow.blur_radius.value(), shadow.spread_radius.value()],
        [1.1592, 2.934],
    );
    assert_eq!(shadow.blend_mode, BlendMode::Normal);
}

#[test]
fn shadow_angles_are_degrees_clockwise_from_up() {
    let diagonal = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    for (angle, offset) in [
        (0.0, [0.0, -10.0]),
        (45.0, [diagonal, -diagonal]),
        (90.0, [10.0, 0.0]),
        (180.0, [0.0, 10.0]),
        (270.0, [-10.0, 0.0]),
        (-90.0, [-10.0, 0.0]),
    ] {
        let (shadow, _) = imported(&shadowed_graphic(PrTextShadow {
            angle,
            distance: 10.0,
            ..CUE_SHADOW
        }));
        assert_close(shadow.unwrap().offset, offset);
    }
}

#[test]
fn linear_light_opacities_become_encoded_alphas() {
    // The color is exact. Opacities 50 and 80 are the calibration's; their
    // measured alphas were 0.246 and 0.492.
    for (opacity, alpha) in [
        (0.0, 0.0),
        (40.0, 0.191_717_79),
        (50.0, 0.250_846_46),
        (80.0, 0.488_597_91),
        (100.0, 1.0),
    ] {
        let (shadow, _) = imported(&shadowed_graphic(PrTextShadow {
            color: PrRgb([255, 0, 51]),
            opacity,
            ..CUE_SHADOW
        }));
        let [red, green, blue, actual] = shadow.unwrap().color;
        assert_eq!([red, green, blue], [1.0, 0.0, 0.2]);
        assert!((actual - alpha).abs() < 1e-8, "{opacity}: {actual}");
    }
}

#[test]
fn shadows_the_text_cannot_keep_are_reported_instead() {
    let base = shadowed_graphic(CUE_SHADOW);
    let mut unfilled = base.clone();
    unfilled.text_mut().document.fill = None;
    let mut scaled = base.clone();
    scaled.text_mut().transform.scale = 50.0;
    let mut rotated = base.clone();
    rotated.text_mut().transform.rotation = 15.0;
    for (graphic, reason) in [
        (unfilled, "its text has no fill"),
        (scaled, "its text is scaled or rotated"),
        (rotated, "its text is scaled or rotated"),
    ] {
        let (shadow, omissions) = imported(&graphic);
        assert!(shadow.is_none());
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: RECORD.into(),
                reason: format!("text shadow not converted: {reason}"),
            }]
        );
    }
    let mut unshadowed = base;
    unshadowed.text_mut().document.shadow = None;
    let (shadow, omissions) = imported(&unshadowed);
    assert!(shadow.is_none() && omissions.is_empty());
}

#[test]
fn out_of_range_shadows_are_reported_and_their_text_still_imports() {
    for (field, value, rule) in [
        ("opacity", 150.0, "must be 0 to 100 percent"),
        ("angle", f32::NAN, "must be finite"),
        ("blur", f32::INFINITY, "must be finite and nonnegative"),
    ] {
        let mut sequence = video_sequence();
        sequence.video_tracks.push(PrVideoTrack {
            transitions: Vec::new(),
            items: vec![PrVideoItem::Graphic(shadowed_graphic(cue_shadow_with(
                field, value,
            )))],
            nests: Vec::new(),
        });
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
        let mut omissions = Vec::new();
        let document =
            premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions).unwrap();
        assert_eq!(
            omissions,
            [
                Omission {
                    scope: OmissionScope::Feature,
                    kind: OmissionKind::Omitted,
                    record: RECORD.into(),
                    reason: format!(
                        "text shadow not converted: invalid Premiere project: text shadow {field} {rule}"
                    ),
                },
                Omission {
                    scope: OmissionScope::Feature,
                    kind: OmissionKind::Omitted,
                    record: format!("{RECORD} (\"Caption\")"),
                    reason: UNPACKAGED_FONT.into(),
                },
            ]
        );
        let Some(LayerData::Text(text)) = document
            .composition()
            .layers()
            .first()
            .map(|layer| layer.data())
        else {
            panic!("the text still imports above the video");
        };
        assert!(text.effects.is_empty(), "{field}: {:?}", text.effects);
    }
}

#[test]
fn imported_text_keeps_its_own_stroke_with_the_shadow_as_its_only_effect() {
    let mut sequence = video_sequence();
    let mut graphic = shadowed_graphic(CUE_SHADOW);
    graphic.text_mut().document.stroke = Some(PrTextStroke {
        color: PrRgb([0, 0, 255]),
        width: 4.0,
    });
    sequence.video_tracks.push(PrVideoTrack {
        items: vec![PrVideoItem::Graphic(graphic)],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions).unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].reason, UNPACKAGED_FONT);
    let Some(LayerData::Text(text)) = document
        .composition()
        .layers()
        .first()
        .map(|layer| layer.data())
    else {
        panic!("the graphic is the top layer");
    };
    assert_eq!(text.source_text.font_family.as_ref(), "MinionPro-Regular");
    assert_eq!(text.source_text.font_style.as_ref(), "");
    // The stroke stays the text's paint, drawn with the fill over the shadow.
    assert!(text.source_text.apply_stroke && !text.source_text.stroke_over_fill);
    assert_eq!(text.source_text.stroke_width.value(), 8.0);
    let [effect] = text.effects.as_slice() else {
        panic!("expected one effect: {:?}", text.effects);
    };
    assert!(
        matches!(effect.data(), EffectData::Identified { id, enabled: true, effect: EffectPayload::Known(LayerEffect::DropShadow(shadow)), .. } if *id == EffectId::new(1) && (shadow.spread_radius.value() - 2.934).abs() < 1e-9)
    );
}

#[test]
fn text_shadow_and_clip_effect_ids_stay_distinct_and_read_back() {
    let mut sequence = video_sequence();
    let PrVideoItem::Media(clip) = &mut sequence.video_tracks[0].items[0] else {
        panic!("the base track holds one video clip");
    };
    let blur = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness: 10.0,
            repeat_edge_pixels: false,
        }),
        animations: Vec::new(),
    };
    clip.effects = vec![blur.clone(), blur];
    // The text becomes layer 2; a shadow keyed by its layer id took the second
    // blur's id and the document failed to read back.
    sequence.video_tracks.push(PrVideoTrack {
        items: vec![PrVideoItem::Graphic(shadowed_graphic(CUE_SHADOW))],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions).unwrap();
    let effect_ids: Vec<(u64, Vec<u64>)> = document
        .composition()
        .layers()
        .iter()
        .map(|layer| {
            let effects = match layer.data() {
                LayerData::Text(text) => text.effects.as_slice(),
                LayerData::Video(video) => video.effects.as_slice(),
                _ => &[],
            };
            let ids = effects
                .iter()
                .map(|effect| record_parts(effect).0.unwrap().value())
                .collect();
            (layer.id().value(), ids)
        })
        .collect();
    // One allocator in import order: the upper track's shadow, then the blurs.
    assert_eq!(effect_ids, [(2, vec![1]), (1, vec![2, 3]), (3, vec![])]);
    EditableFxCompositionDocument::from_json_value(document.to_json_value().unwrap()).unwrap();
}

/// An enabled record with effect id 1 holding `shadow`.
fn shadow_record(shadow: &DropShadow) -> EffectRecord {
    EffectRecord::from_data(&EffectData::Identified {
        id: EffectId::new(1),
        enabled: true,
        effect: EffectPayload::Known(LayerEffect::DropShadow(shadow.clone())),
        compositing_options: None,
        extensions: Default::default(),
    })
    .unwrap()
}

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

/// Export a text-only FX document whose text layer takes `edit` and whose
/// composition animates `dynamics`, with the omissions for that layer.
fn exported(edit: Value, dynamics: Value) -> (Option<PrTextShadow>, Vec<(OmissionScope, String)>) {
    let (shadow, omissions) = exported_with_all_omissions(edit, dynamics);
    let reasons = omissions
        .into_iter()
        .filter(|item| item.record == "layer 9 (\"Title\")")
        .map(|item| (item.scope, item.reason))
        .collect();
    (shadow, reasons)
}

fn exported_with_all_omissions(
    edit: Value,
    dynamics: Value,
) -> (Option<PrTextShadow>, Vec<Omission>) {
    let mut text = json!({
        "type": "Text",
        "id": 9,
        "name": "Title",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"anchorPoint": [0, 0], "position": [960, 540], "scale": [100, 100], "rotation": 0, "opacity": 100},
        "sourceText": {"text": "Shadow", "fontFamily": "Inter-Bold", "fontStyle": "", "fontSize": 80, "fillColor": [1, 1, 1, 1]}
    });
    merge(&mut text, edit);
    let mut wire = editable_document();
    // The text replaces the video above the black canvas.
    wire["composition"]["layers"][0] = text;
    if !dynamics.is_null() {
        wire["composition"]["dynamics"] = json!({ "entries": dynamics });
    }
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    let shadow = project
        .single_sequence()
        .unwrap()
        .video_items()
        .find_map(|item| match item {
            PrVideoItem::Graphic(graphic) => Some(graphic.text().document.shadow),
            PrVideoItem::Media(_) | PrVideoItem::Capsule(_) => None,
        })
        .expect("the text still exports");
    (shadow, omissions)
}

fn shadow_effect(effect: Value) -> Value {
    json!({"effects": [{"id": 5, "effect": effect}]})
}

#[test]
fn one_static_drop_shadow_exports_as_the_premiere_shadow() {
    let (shadow, omissions) = exported(
        shadow_effect(json!({
            "type": "dropShadow", "color": [0.5, 0.4, 0.6, 0.8], "offset": [0, 10],
            "blurRadius": 4, "spreadRadius": 2
        })),
        Value::Null,
    );
    // The black-shadow opacity calibration cannot express a translucent
    // colored shadow's linear-light blend; that is reported, not hidden.
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(
        omissions[0].1.contains("translucent non-black shadow"),
        "{omissions:?}"
    );
    let shadow = shadow.unwrap();
    // 0.5 lies halfway between two 8-bit steps, the 1/510 worst case of the
    // export color bound, and rounds up to 128.
    assert_eq!(
        (shadow.color, shadow.angle, shadow.distance),
        (PrRgb([128, 102, 153]), 180.0, 10.0)
    );
    // Alpha 0.8 in encoded values is linear-light opacity 97.899; σ 4 px is
    // blur 41.408, and a 2 px dilation is size 4.090.
    for (actual, expected) in [
        (shadow.opacity, 97.898_78),
        (shadow.blur, 41.407_87),
        (shadow.size, 4.089_98),
    ] {
        assert!((actual - expected).abs() < 1e-4, "{shadow:?}");
    }
}

#[test]
fn exported_offsets_become_a_clockwise_angle_and_a_distance() {
    let diagonal = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
    for (offset, angle, distance) in [
        (json!([5, 0]), 90.0, 5.0),
        (json!([0, -5]), 0.0, 5.0),
        (json!([-5, 0]), 270.0, 5.0),
        (json!([diagonal, diagonal]), 135.0, 5.0),
        (json!([-diagonal, -diagonal]), 315.0, 5.0),
        // Any angle draws a zero-distance shadow; Premiere's default is kept.
        (json!([0, 0]), 135.0, 0.0),
    ] {
        let (shadow, omissions) = exported(
            shadow_effect(json!({"type": "dropShadow", "offset": offset})),
            Value::Null,
        );
        assert!(omissions.is_empty(), "{omissions:?}");
        let shadow = shadow.unwrap();
        assert!(
            (shadow.angle - angle).abs() < 1e-4 && (shadow.distance - distance).abs() < 1e-5,
            "{offset}: {shadow:?}"
        );
    }
}

#[test]
fn a_disabled_sibling_does_not_discard_the_one_active_shadow() {
    let shadow = |extra: Value| {
        let mut effect = json!({"type": "dropShadow", "offset": [3, 0]});
        effect
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        effect
    };
    for edit in [
        json!({"effects": [
            {"id": 5, "enabled": false, "effect": shadow(json!({}))},
            {"id": 6, "effect": shadow(json!({}))}
        ]}),
        json!({"effects": [
            {"id": 5, "effect": shadow(json!({"enabled": false}))},
            {"id": 6, "effect": shadow(json!({}))}
        ]}),
    ] {
        let (exported, omissions) = exported(edit, Value::Null);
        let exported = exported.expect("the enabled shadow still exports");
        assert!((exported.distance - 3.0).abs() < 1e-5, "{exported:?}");
        assert_eq!(
            omissions,
            [(
                OmissionScope::Feature,
                "drop shadow 5 was not exported: unsupported conversion: it is disabled".to_owned()
            )]
        );
    }
}

#[test]
fn shadows_and_effects_premiere_text_cannot_express_are_reported() {
    let shadow = || json!({"type": "dropShadow", "offset": [3, 3]});
    let animated = json!([{
        "target": {"kind": "effectProperty", "effectId": 5, "paramName": "blurRadius"},
        "animator": {"type": "jsScript", "code": "return input.time.seconds;"}
    }]);
    for (edit, dynamics, reason) in [
        (
            json!({"effects": [{"id": 5, "effect": shadow()}, {"id": 6, "effect": shadow()}]}),
            Value::Null,
            "2 drop shadows were not exported: Premiere text has one shadow",
        ),
        (
            json!({"effects": [{"id": 5, "enabled": false, "effect": shadow()}]}),
            Value::Null,
            "drop shadow 5 was not exported: unsupported conversion: it is disabled",
        ),
        (
            shadow_effect(json!({"type": "dropShadow", "enabled": false})),
            Value::Null,
            "drop shadow 5 was not exported: unsupported conversion: it is disabled",
        ),
        (
            shadow_effect(shadow()),
            animated,
            "drop shadow 5 was not exported: unsupported conversion: animated text shadows are unsupported",
        ),
        (
            shadow_effect(json!({"type": "dropShadow", "blendMode": "multiply"})),
            Value::Null,
            "drop shadow 5 was not exported: unsupported conversion: Premiere text shadows blend normally",
        ),
        (
            shadow_effect(json!({"type": "dropShadow", "color": [1.5, 0, 0, 1]})),
            Value::Null,
            "drop shadow 5 was not exported: unsupported conversion: its color channels must be 0 to 1",
        ),
        (
            json!({"effects": [{"id": 5, "effect": shadow()}], "transform": {"scale": [50, 50]}}),
            Value::Null,
            "drop shadow 5 was not exported: unsupported conversion: its text is scaled or rotated",
        ),
        (
            json!({"effects": [{"id": 5, "effect": shadow()}], "transform": {"rotation": 10}}),
            Value::Null,
            "drop shadow 5 was not exported: unsupported conversion: its text is scaled or rotated",
        ),
        (
            json!({"effects": [{"id": 5, "effect": shadow()}], "sourceText": {"applyFill": false}}),
            Value::Null,
            "drop shadow 5 was not exported: unsupported conversion: its text has no fill",
        ),
        (
            shadow_effect(json!({"type": "dropShadow", "offset": [1e300, 0]})),
            Value::Null,
            "drop shadow 5 was not exported: invalid Premiere project: text shadow distance must be finite and nonnegative",
        ),
        (
            json!({"effects": [{"id": 6, "effect": {"type": "stroke", "color": [0, 0, 1, 1], "width": 4}}]}),
            Value::Null,
            "stroke effect 6 was not exported; only the text's own stroke converts",
        ),
        (
            json!({"effects": [{"id": 6, "effect": {"type": "innerShadow"}}]}),
            Value::Null,
            "inner shadow effect 6 was not exported: Premiere text has no inner shadow",
        ),
        (
            json!({"effects": [{"id": 6, "effect": {"type": "satin"}}]}),
            Value::Null,
            "satin effect 6 was not exported: Premiere text has no satin",
        ),
        (
            json!({"effects": [{"id": 6, "effect": {"type": "bevelEmboss"}}]}),
            Value::Null,
            "bevel and emboss effect 6 was not exported: Premiere text has no bevel and emboss",
        ),
        (
            json!({"effects": [{"id": 6, "effect": {"type": "gaussianBlur", "blurriness": 4}}]}),
            Value::Null,
            "gaussianBlur effect 6 was not exported",
        ),
    ] {
        let (exported, omissions) = exported(edit, dynamics);
        assert_eq!(exported, None, "{reason}");
        assert_eq!(
            omissions,
            [(OmissionScope::Feature, reason.to_owned())],
            "{reason}"
        );
    }
}

#[test]
fn legacy_inline_and_style_shadows_are_reported_by_the_reader_and_not_exported() {
    for (field, value) in [
        ("dropShadow", json!({"offset": [3, 3]})),
        (
            "layerStyles",
            json!([{"id": 50, "style": {"type": "dropShadow", "offset": [3, 3]}}]),
        ),
    ] {
        let (shadow, omissions) = exported_with_all_omissions(json!({ field: value }), Value::Null);
        assert_eq!(shadow, None, "{field}");
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "composition".into(),
                reason: format!("unknown field layers.0.{field} was not exported"),
            }],
            "{field}"
        );
    }
}

#[test]
fn imported_shadows_export_unchanged() {
    for angle in [0.0, 45.0, 135.0, 200.0, 315.0] {
        let expected = PrTextShadow {
            opacity: 80.0,
            angle,
            distance: 12.5,
            ..CUE_SHADOW
        };
        let graphic = shadowed_graphic(expected);
        let shadow = drop_shadow(expected).unwrap();
        let instance = shadow_record(&shadow);
        let exported = premiere_shadow(
            &instance,
            &shadow,
            LayerId::new(1),
            &AnimationGraph::default(),
            graphic.text(),
            None,
        )
        .unwrap();
        assert!((exported.angle - angle).abs() < 1e-4, "{exported:?}");
        assert_eq!(PrTextShadow { angle, ..exported }, expected);
    }
}

#[test]
fn legacy_animation_addresses_and_non_finite_offsets_block_export() {
    let text = shadowed_graphic(CUE_SHADOW).text().clone();
    let shadow = drop_shadow(CUE_SHADOW).unwrap();
    let inline = shadow_record(&shadow);
    let style: EffectRecord = serde_json::from_value(json!({
        "id": 1, "effect": {"type": "dropShadow"},
        "legacySource": {"kind": "layerStyle", "itemId": 50}
    }))
    .unwrap();
    // The reader keeps legacy compatibility addresses as written.
    let animating = |target: Value| -> AnimationGraph {
        serde_json::from_value(json!({"entries": [{
            "target": target,
            "animator": {"type": "jsScript", "layerTimeJsCode": "return [3, 3];"}
        }]}))
        .unwrap()
    };
    let export = |instance: &EffectRecord, shadow: &DropShadow, dynamics: &AnimationGraph| {
        premiere_shadow(instance, shadow, LayerId::new(1), dynamics, &text, None)
    };
    let animated = "unsupported conversion: animated text shadows are unsupported";
    for (instance, target, blocked) in [
        (
            &inline,
            json!({"kind": "layer", "layerId": 1, "propertyType": "dropShadowOffset"}),
            true,
        ),
        (
            &inline,
            json!({"kind": "layer", "layerId": 2, "propertyType": "dropShadowOffset"}),
            false,
        ),
        (
            &inline,
            json!({"kind": "layer", "layerId": 1, "propertyType": "opacity"}),
            false,
        ),
        (
            &style,
            json!({"kind": "fxItemProperty", "itemId": 50, "propertyName": "offset"}),
            true,
        ),
        (
            &style,
            json!({"kind": "fxItemProperty", "itemId": 51, "propertyName": "offset"}),
            false,
        ),
    ] {
        let result = export(instance, &shadow, &animating(target.clone()));
        if blocked {
            assert_eq!(result.unwrap_err().to_string(), animated, "{target}");
        } else {
            assert!(result.is_ok(), "{target}: {result:?}");
        }
    }
    // FX JSON cannot carry a non-finite offset; the model can.
    for offset in [[f64::NAN, 0.0], [0.0, f64::INFINITY]] {
        let shadow = DropShadow {
            offset,
            ..shadow.clone()
        };
        let error = export(&inline, &shadow, &AnimationGraph::default()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "unsupported conversion: its offset must be finite"
        );
    }
}

#[test]
fn shadow_values_outside_premiere_ranges_are_invalid() {
    for (field, value, rule) in [
        ("opacity", 100.5, "must be 0 to 100 percent"),
        ("opacity", -1.0, "must be 0 to 100 percent"),
        ("opacity", f32::NAN, "must be 0 to 100 percent"),
        ("angle", f32::INFINITY, "must be finite"),
        ("distance", -1.0, "must be finite and nonnegative"),
        ("distance", f32::INFINITY, "must be finite and nonnegative"),
        ("size", f32::NAN, "must be finite and nonnegative"),
        ("size", f32::INFINITY, "must be finite and nonnegative"),
        ("blur", -0.5, "must be finite and nonnegative"),
        ("blur", f32::INFINITY, "must be finite and nonnegative"),
    ] {
        let error = cue_shadow_with(field, value).validate().unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("invalid Premiere project: text shadow {field} {rule}"),
            "{field} = {value}"
        );
    }
    assert!(CUE_SHADOW.validate().is_ok());
}

#[test]
fn legacy_luma_key_graphic_omission_warns_only_when_enabled() {
    for enabled in [true, false] {
        let effect: EffectRecord = serde_json::from_value(json!({
            "id": 42, "enabled": enabled,
            "effect": {"type": "lumaKey", "threshold": 0.4, "softness": 0.2, "invert": 0.0}
        }))
        .unwrap();
        for owner in ["text", "shape"] {
            let mut omissions = Vec::new();
            assert!(
                one_drop_shadow(std::slice::from_ref(&effect), owner, &mut omissions, RECORD)
                    .is_none()
            );
            assert_eq!(omissions.len(), 1);
            assert!(omissions[0]
                .reason
                .contains("lumaKey effect 42 was not exported"));
            assert_eq!(
                omissions[0]
                    .reason
                    .contains("may expose previously keyed pixels"),
                enabled
            );
        }
    }
}
