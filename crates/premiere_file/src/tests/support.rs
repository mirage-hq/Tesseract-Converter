use crate::{
    error::{ensure, unsupported, Result},
    format::{inspect_project, MediaId, PrMedia, PrProjectFile, PrSequence, PrVideoOccurrence},
    schema::{PrGraphic, PrMask, PrStaticCrop, PrVideoItem, PrVideoStream, PrVideoTrack},
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// The current Gaussian Blur that export writes for an FX `blurriness`.
pub(crate) fn exported_blur(
    enabled: bool,
    blurriness: f64,
    repeat_edge_pixels: bool,
) -> crate::schema::PrEffect {
    crate::schema::PrEffect {
        enabled,
        params: crate::schema::PrEffectParams::FilmImpactBlur(crate::schema::PrFilmImpactBlur {
            amount: amount(blurriness),
            repeat_edge_pixels,
        }),
        animations: Vec::new(),
    }
}

/// The Film Impact Amount that export writes for an FX `blurriness`.
pub(crate) fn amount(blurriness: f64) -> f64 {
    crate::schema::FILM_IMPACT_BLUR_AMOUNT
        .native_value(blurriness)
        .unwrap()
}

/// The current blur that export writes for a Legacy Gaussian or Directional
/// Blur model and its keys; other effects are unchanged.
pub(crate) fn current_blur_export(mut effect: crate::schema::PrEffect) -> crate::schema::PrEffect {
    use crate::schema::{
        PrEffectParamKeys, PrEffectParams, PrFilmImpactDirectionalBlur, DIRECTIONAL_BLUR_DIRECTION,
        DIRECTIONAL_BLUR_LENGTH, FILM_IMPACT_BLUR_AMOUNT, FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT,
        FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, GAUSSIAN_BLUR_BLURRINESS,
    };
    effect.params = match effect.params {
        PrEffectParams::GaussianBlur(blur) => {
            exported_blur(effect.enabled, blur.blurriness, blur.repeat_edge_pixels).params
        }
        PrEffectParams::DirectionalBlur(blur) => {
            PrEffectParams::FilmImpactDirectionalBlur(PrFilmImpactDirectionalBlur {
                angle: blur.direction,
                amount: directional_amount(blur.blur_length),
            })
        }
        _ => return effect,
    };
    for animation in &mut effect.animations {
        let (param, value): (_, fn(f64) -> f64) = match animation.param {
            param if *param == GAUSSIAN_BLUR_BLURRINESS => (&FILM_IMPACT_BLUR_AMOUNT, amount),
            param if *param == DIRECTIONAL_BLUR_DIRECTION => {
                (&FILM_IMPACT_DIRECTIONAL_BLUR_ANGLE, |direction| direction)
            }
            param if *param == DIRECTIONAL_BLUR_LENGTH => {
                (&FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT, directional_amount)
            }
            param => panic!("unexpected keyed {}", param.label),
        };
        animation.param = param;
        let PrEffectParamKeys::Scalar(keys) = &mut animation.keys else {
            panic!("a blur has scalar keys");
        };
        for key in keys {
            key.value = value(key.value);
        }
    }
    effect
}

/// The current Directional Blur Amount that export writes for a Legacy Blur
/// Length in the clip's frame.
pub(crate) fn directional_amount(blur_length: f64) -> f64 {
    crate::schema::FILM_IMPACT_DIRECTIONAL_BLUR_AMOUNT
        .native_value(blur_length)
        .unwrap()
}

/// Premiere's gain along a Linear clip Volume segment between Level gains,
/// from the fader-curve model measured on AME renders (u = g^0.4475 up to
/// 0 dB, 2 - g^-0.4475 above), written out here rather than taken from the
/// converter.
pub(crate) fn premiere_linear_gain(from: f64, to: f64, progress: f64) -> f64 {
    let p = 0.4475;
    let position = |gain: f64| match gain {
        gain if gain <= 0.0 => 0.0,
        gain if gain <= 1.0 => gain.powf(p),
        gain => 2.0 - gain.powf(-p),
    };
    match (1.0 - progress) * position(from) + progress * position(to) {
        u if u <= 0.0 => 0.0,
        u if u <= 1.0 => u.powf(1.0 / p),
        u => (2.0 - u).powf(-1.0 / p),
    }
}

pub(crate) fn video_media() -> BTreeMap<MediaId, PrMedia> {
    use crate::{format::FrameRate, schema::TICKS};
    BTreeMap::from([(
        MediaId("source".into()),
        PrMedia {
            name: "source.mp4".into(),
            relative_path: None,
            relative_paths: Vec::new(),
            absolute_paths: Vec::new(),
            video: Some(PrVideoStream {
                orientation: crate::schema::VideoOrientation::Identity,
                kind: crate::schema::PrMediaKind::Video {
                    codec: None,
                    hdr_profile: None,
                },
                intrinsic_ticks: 10 * TICKS,
                frame_rate: (FrameRate::Fps30).into(),
                width: 1920,
                height: 1080,
            }),
            audio: None,
        },
    )])
}

/// A 10 s source named `name` with the facts of `video_media`.
pub(crate) fn named_media(name: &str) -> (MediaId, PrMedia) {
    let mut media = video_media().remove(&MediaId("source".into())).unwrap();
    media.name = format!("{name}.mp4");
    (MediaId(name.into()), media)
}

/// A normal-speed placement of `media` over `timeline`, starting at `source_in`.
pub(crate) fn clip_of(
    media: &str,
    timeline: std::ops::Range<i64>,
    source_in: i64,
) -> PrVideoOccurrence {
    let mut clip = video_sequence().video_tracks[0].clip(0).clone();
    clip.media = MediaId(media.into());
    clip.in_ticks = source_in;
    clip.out_ticks = source_in + timeline.end - timeline.start;
    (clip.start_ticks, clip.end_ticks) = (timeline.start, timeline.end);
    clip
}

/// A normal-speed placement of `sequence` over `timeline`, starting at inner `source_in`.
pub(crate) fn nest_of(
    sequence: PrSequence,
    timeline: std::ops::Range<i64>,
    source_in: i64,
) -> crate::schema::PrNestOccurrence {
    crate::schema::PrNestOccurrence {
        id: None,
        in_ticks: source_in,
        out_ticks: source_in + timeline.end - timeline.start,
        start_ticks: timeline.start,
        end_ticks: timeline.end,
        transform: Default::default(),
        opacity: 100.0,
        blend_mode: crate::schema::PrBlendMode::Normal,
        animations: Vec::new(),
        crop: Default::default(),
        linear_wipe: None,
        track_matte: None,
        effects: Vec::new(),
        enabled: true,
        sequence,
    }
}

/// A 30 fps 1080p sequence with the given tracks, bottom first, that ends
/// with its last placement.
pub(crate) fn sequence_of(name: &str, video_tracks: Vec<PrVideoTrack>) -> PrSequence {
    let mut sequence = video_sequence();
    sequence.name = name.into();
    sequence.video_tracks = video_tracks;
    sequence.timeline_end_ticks = sequence.occurrence_end_ticks();
    sequence
}

/// The layout of `feature_nested_sequence_strict.prproj` in semantic form.
/// "Inner" shows the timecoded source at 0-4 s and, after a gap, at 5-6 s
/// from source 7 s. "Outer" has the red source at 0-10 s on V1 and places
/// Inner on V2 at 0-3 s from inner 1 s and at 5-11 s untrimmed.
pub(crate) fn nested_sequence() -> (PrSequence, BTreeMap<MediaId, PrMedia>) {
    use crate::schema::TICKS;
    let inner = sequence_of(
        "Inner",
        vec![PrVideoTrack::media([
            clip_of("timecoded", 0..4 * TICKS, 0),
            clip_of("timecoded", 5 * TICKS..6 * TICKS, 7 * TICKS),
        ])],
    );
    let outer = sequence_of(
        "Outer",
        vec![
            PrVideoTrack::media([clip_of("red", 0..10 * TICKS, 0)]),
            PrVideoTrack {
                transitions: Vec::new(),
                items: Vec::new(),
                nests: vec![
                    nest_of(inner.clone(), 0..3 * TICKS, TICKS),
                    nest_of(inner, 5 * TICKS..11 * TICKS, 0),
                ],
            },
        ],
    );
    (
        outer,
        BTreeMap::from([named_media("red"), named_media("timecoded")]),
    )
}

/// Semantic input for model and forward-mapper tests; no native decoder is involved.
pub(crate) fn video_sequence() -> PrSequence {
    use crate::{format::FrameRate, schema::TICKS};
    PrSequence {
        id: None,
        name: "Main".into(),
        top_level: Some(true),
        audio: Vec::new(),
        frame_rate: FrameRate::Fps30,
        width: 1920,
        height: 1080,
        video_tracks: vec![PrVideoTrack {
            items: vec![PrVideoItem::Media(PrVideoOccurrence {
                id: None,
                media: MediaId("source".into()),
                start_ticks: 0,
                end_ticks: 5 * TICKS,
                in_ticks: 0,
                out_ticks: 5 * TICKS,
                playback_rate: 1.0,
                frame_blending: None,
                opacity: 100.0,
                blend_mode: crate::schema::PrBlendMode::Normal,
                transform: Default::default(),
                crop: Default::default(),
                animations: Vec::new(),
                time_remap: None,
                linear_wipe: None,
                opacity_mask: None,
                track_matte: None,
                enabled: true,
                effects: Vec::new(),
                effects_above_mask: 0,
                stroke: None,
                active_transforms: 0,
            })],
            transitions: Vec::new(),
            nests: Vec::new(),
        }],
        timeline_end_ticks: 5 * TICKS,
    }
}

/// The first video clip of the only sequence of `project`.
pub(crate) fn first_clip(project: &PrProjectFile) -> &PrVideoOccurrence {
    project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap()
}

/// A Crop of 7% from the left edge.
pub(crate) fn left_crop() -> PrStaticCrop {
    PrStaticCrop {
        left: 7.0,
        ..PrStaticCrop::default()
    }
}

/// A static Opacity mask: the half-opaque, 12 px feathered rectangle over
/// the middle half of the frame, as a corner path.
pub(crate) fn opacity_mask() -> PrMask {
    use crate::schema::text::{PrPathVertex, PrShapePath};
    let corner = |x: f32, y: f32| PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    PrMask {
        path: PrShapePath {
            vertices: vec![
                corner(0.25, 0.25),
                corner(0.75, 0.25),
                corner(0.75, 0.75),
                corner(0.25, 0.75),
            ],
            closed: true,
        },
        feather: 12.0,
        opacity: 50.0,
        inverted: false,
    }
}

/// A graphic from 1 s to 3 s that sets every modeled text field.
pub(crate) fn text_graphic() -> PrGraphic {
    use crate::schema::{
        text::{
            PrGraphicObject, PrJustification, PrRgb, PrTextDocument, PrTextFrame, PrTextStroke,
            PrTextTransform, PrVerticalAlign,
        },
        PrText, TICKS,
    };
    PrGraphic {
        id: Some("20".into()),
        start_ticks: TICKS,
        end_ticks: 3 * TICKS,
        in_ticks: crate::format::FrameRate::Fps30.generator_in_ticks(),
        vector_motion: None,
        opacity: 100.0,
        blend_mode: crate::schema::PrBlendMode::Normal,
        animations: Vec::new(),
        objects: vec![PrGraphicObject::Text(PrText {
            name: "Title".into(),
            document: PrTextDocument {
                text: "Line one\nLine two".into(),
                font: "OpenSans-Bold".into(),
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
            },
            transform: PrTextTransform {
                position: [480.0, 540.0],
                anchor: [96.0, 54.0],
                scale: 80.0,
                rotation: -15.0,
                opacity: 75.0,
            },
            animations: Vec::new(),
            source_text_keys: Vec::new(),
        })],
        enabled: true,
    }
}

/// [`text_graphic`] with one Shape instead of its text: a filled 200 × 180
/// px rectangle of corners at the frame centre.
pub(crate) fn shape_graphic() -> PrGraphic {
    use crate::schema::text::{
        PrAppearance, PrFill, PrGraphicObject, PrPathVertex, PrRgb, PrShape, PrShapePath,
        PrTextTransform,
    };
    let corner = |x: f32, y: f32| PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    PrGraphic {
        objects: vec![PrGraphicObject::Shape(PrShape {
            name: "Box".into(),
            path: PrShapePath {
                vertices: vec![
                    corner(-100.0, -90.0),
                    corner(100.0, -90.0),
                    corner(100.0, 90.0),
                    corner(-100.0, 90.0),
                ],
                closed: true,
            },
            appearance: PrAppearance {
                fill: Some(PrFill::Solid(PrRgb([0, 96, 255]))),
                stroke: None,
                shadow: None,
            },
            transform: PrTextTransform {
                position: [960.0, 540.0],
                anchor: [0.0, 0.0],
                scale: 100.0,
                rotation: 0.0,
                opacity: 100.0,
            },
            horizontal_scale: None,
        })],
        ..text_graphic()
    }
}

pub(super) fn inspect(xml: &str, sequence_id: Option<&str>) -> Result<PrVideoOccurrence> {
    let mut project = inspect_project(xml, sequence_id)?;
    ensure!(
        project.video_occurrences().count() == 1,
        "{}: expected one video occurrence, found {}",
        project.id.as_deref().unwrap_or("unassigned"),
        project.video_occurrences().count()
    );
    match project.video_tracks[0].items.remove(0) {
        PrVideoItem::Media(clip) => Ok(clip),
        PrVideoItem::Graphic(_) => Err(unsupported("expected a media occurrence")),
    }
}

pub(super) fn convert_tesseract_file(
    source: &Path,
    sequence: Option<&str>,
) -> Result<crate::tesseract_output::PendingTesseractFile> {
    let source = source.canonicalize()?;
    let (mut sequences, media) = crate::format::PrProjectFile::load_selected(&source, sequence)?
        .0
        .into_parts();
    ensure!(sequences.len() == 1, "expected exactly one test sequence");
    let mut omissions = Vec::new();
    let converted = crate::tesseract_output::convert_premiere_sequence(
        &source,
        sequences.remove(0),
        Arc::new(media),
        &mut omissions,
    )?;
    // Tests of a one-sequence project assert on why it could not convert.
    converted.ok_or_else(|| {
        let reasons: Vec<_> = omissions.iter().map(ToString::to_string).collect();
        unsupported(reasons.join("\n"))
    })
}

/// Build one Tesseract file through the same conversion used by read-only checks.
pub(super) fn build_tesseract_file(
    source: &Path,
    output: &Path,
    sequence: Option<&str>,
) -> Result<()> {
    ensure!(
        !output.exists() && !output.is_symlink(),
        "output already exists; choose a fresh .tsrct path"
    );
    convert_tesseract_file(source, sequence)?.write_to_staging(output)
}

/// Build a Premiere package through the same conversion used by public calls.
pub(super) fn tesseract_to_premiere(source: &Path, output: &Path) -> Result<()> {
    crate::premiere_package::save_tesseract_as_premiere(
        source,
        output,
        crate::format::FrameRate::Fps30,
        false,
    )
    .map(|_| ())
}

/// Inspect the serialized document produced by the typed mutation path.
pub(crate) fn project_document(project: &PrSequence) -> Value {
    project_document_with_media(project, &video_media())
}

pub(crate) fn project_document_with_media(
    project: &PrSequence,
    media: &BTreeMap<MediaId, PrMedia>,
) -> Value {
    let ids = crate::tesseract_output::asset_ids_in_order(project, media);
    crate::convert::premiere_to_tesseract(project, media, &ids, &mut Vec::new())
        .unwrap()
        .to_json_value()
        .unwrap()
}

/// A Directional Blur with the native `direction` and `blur_length`.
pub(crate) fn directional_blur(
    enabled: bool,
    direction: f64,
    blur_length: f64,
) -> crate::schema::PrEffect {
    use crate::schema::{PrDirectionalBlur, PrEffect, PrEffectParams};
    PrEffect {
        enabled,
        params: PrEffectParams::DirectionalBlur(PrDirectionalBlur {
            direction,
            blur_length,
        }),
        animations: Vec::new(),
    }
}

/// `effect`, a Directional Blur, with the Direction keys `direction` and the
/// Blur Length keys `blur_length`, either possibly empty. Each keyed static
/// value is its first key's.
pub(crate) fn keyed_directional(
    mut effect: crate::schema::PrEffect,
    direction: Vec<crate::schema::PrScalarKeyframe>,
    blur_length: Vec<crate::schema::PrScalarKeyframe>,
) -> crate::schema::PrEffect {
    use crate::schema::{
        PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, DIRECTIONAL_BLUR_DIRECTION,
        DIRECTIONAL_BLUR_LENGTH,
    };
    let PrEffectParams::DirectionalBlur(blur) = &mut effect.params else {
        panic!("expected a Directional Blur");
    };
    for (param, keys, value) in [
        (&DIRECTIONAL_BLUR_DIRECTION, direction, &mut blur.direction),
        (&DIRECTIONAL_BLUR_LENGTH, blur_length, &mut blur.blur_length),
    ] {
        if let Some(first) = keys.first() {
            *value = first.value;
            effect.animations.push(PrEffectParamAnimation {
                param,
                keys: PrEffectParamKeys::Scalar(keys),
            });
        }
    }
    effect
}

/// A Transform at Premiere's defaults (Oracle run E11, `A-static.xml` but
/// for its Position and Opacity): both points centred, Uniform Scale off at
/// 100/100, no skew or rotation, the composition's shutter angle.
pub(crate) const DEFAULT_PR_TRANSFORM: crate::schema::PrTransform = crate::schema::PrTransform {
    anchor_point: [0.5, 0.5],
    position: [0.5, 0.5],
    uniform_scale: false,
    scale_height: 100.0,
    scale_width: 100.0,
    skew: 0.0,
    skew_axis: 0.0,
    rotation: 0.0,
    opacity: 100.0,
    composition_shutter_angle: true,
    shutter_angle: 0.0,
    bicubic_sampling: false,
};

/// An active Transform with `transform`'s values and `animations` in native
/// order (a keyed static value is its first key's).
pub(crate) fn transform_effect(
    transform: crate::schema::PrTransform,
    animations: Vec<crate::schema::PrEffectParamAnimation>,
) -> crate::schema::PrEffect {
    crate::schema::PrEffect {
        enabled: true,
        params: crate::schema::PrEffectParams::Transform(transform),
        animations,
    }
}

/// Source-clock keys of a canonical mapping, including affine endpoint keys.
pub(crate) fn playback_keys(layer: &serde_json::Value) -> Vec<serde_json::Value> {
    let playback = &layer["playback"];
    assert_eq!(playback["type"], "windowed");
    let offset = playback["inputOffsetMs"].as_i64().unwrap();
    let mapping = &playback["mapping"];
    match mapping["type"].as_str().unwrap() {
        "linear" => {
            let input = &mapping["input"];
            let output = &mapping["output"];
            let start = input["start"].as_u64().unwrap();
            let end = start
                .checked_add(input["duration"].as_u64().unwrap())
                .unwrap();
            let source = output["start"].as_u64().unwrap();
            let source_end = source
                .checked_add(output["duration"].as_u64().unwrap())
                .unwrap();
            [(start, source), (end, source_end)]
                .into_iter()
                .map(|(time, value)| {
                    let time = u64::try_from(i128::from(time) - i128::from(offset)).unwrap();
                    serde_json::json!({"time": time, "value": value, "easing": {"type": "linear"}})
                })
                .collect()
        }
        "timeRemap" => mapping["property"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| {
                let mut key = key.clone();
                let time = key["time"].as_u64().unwrap();
                key["time"] = serde_json::json!(u64::try_from(
                    i128::from(time) - i128::from(offset)
                )
                .unwrap());
                key
            })
            .collect(),
        unexpected => panic!("unexpected fixture mapping {unexpected}"),
    }
}

/// Native transition 1009/component 1610 and controls from Bonsa SHA
/// ace57eb53250a0c41b6aeff89474753fc5c68fe4c41157e4daf89a703b7bbe55.
/// Only the parent-clock range is relocated onto the one-clip fixture.
pub(crate) fn film_impact_tail_xml() -> String {
    let native = include_str!("../../tests/fixtures/film-impact-dissolve-profile.xml")
        .replace("<PremiereData>", "")
        .replace("</PremiereData>", "")
        .replace("30173375232000", "1016064000000")
        .replace("30266607571200", "1270080000000")
        .replace("93232339200", "254016000000");
    include_str!("../../tests/fixtures/one-clip.xml")
        .replace("</ClipItems></ClipTrack>", "</ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef=\"1009\"/></TrackItems></TransitionItems></ClipTrack>")
        .replace("<SubClip ObjectRef=\"5\"/></ClipTrackItem>", "<SubClip ObjectRef=\"5\"/><TailTransition ObjectRef=\"1009\"/></ClipTrackItem>")
        .replace("</PremiereData>", &format!("{native}</PremiereData>"))
}

/// Opposite one-sided topology of native Bonsa End Card transition 1001;
/// its controls equal the pinned tail profile. Relocated to source range 0..1 s.
pub(crate) fn film_impact_head_xml() -> String {
    film_impact_tail_xml()
        .replace(
            "<TailTransition ObjectRef=\"1009\"/>",
            "<HeadTransition ObjectRef=\"1009\"/>",
        )
        .replace(
            "<HasOutgoingClip>true</HasOutgoingClip>",
            "<HasOutgoingClip>false</HasOutgoingClip>",
        )
        .replace(
            "<HasIncomingClip>false</HasIncomingClip>",
            "<HasIncomingClip>true</HasIncomingClip>",
        )
        .replace("<Start>1016064000000</Start>", "<Start>0</Start>")
        .replace("<End>1270080000000</End>\n", "<End>254016000000</End>\n")
        .replace(
            "<Alignment>254016000000</Alignment>",
            "<Alignment>0</Alignment>",
        )
}

/// Native Curve Graph 2690 from End Card transition 1001. Only its graph
/// identity is remapped onto the common profile; UI expansion has no value.
pub(crate) fn film_impact_curve_ui_xml() -> String {
    let mut source = film_impact_tail_xml();
    let start = source
        .find("<ArbVideoComponentParam ObjectID=\"2844\"")
        .unwrap();
    let end = start
        + source[start..].find("</ArbVideoComponentParam>").unwrap()
        + "</ArbVideoComponentParam>".len();
    let native = include_str!("../../tests/fixtures/film-impact-dissolve-curve-ui.xml")
        .replace("ObjectID=\"2690\"", "ObjectID=\"2844\"");
    source.replace_range(start..end, &native);
    source
}

/// Native Bonsa Pop1006 controls; only range moves to the30fps one-clip fixture.
pub(crate) fn film_impact_pop_xml() -> String {
    let native = include_str!("../../tests/fixtures/film-impact-pop-profile.xml")
        .replace("<?xml version='1.0' encoding='utf-8'?>", "")
        .replace("<PremiereData>", "")
        .replace("</PremiereData>", "")
        .replace("50854003200", "0")
        .replace("305124019200", "254016000000");
    include_str!("../../tests/fixtures/one-clip.xml")
        .replace("</ClipItems></ClipTrack>", "</ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef=\"1006\"/></TrackItems></TransitionItems></ClipTrack>")
        .replace("<SubClip ObjectRef=\"5\"/></ClipTrackItem>", "<SubClip ObjectRef=\"5\"/><HeadTransition ObjectRef=\"1006\"/></ClipTrackItem>")
        .replace("</PremiereData>", &format!("{native}</PremiereData>"))
}
