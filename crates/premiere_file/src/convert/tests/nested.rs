//! Nested sequences as groups (import) and plain groups as nests (export).

use crate::test_support::linear_playback;
use crate::{
    convert::{premiere_to_tesseract, tesseract_to_premiere},
    format::{FrameRate, PrProjectFile, PremiereProjectXml},
    media::{MediaFacts, VideoMedia},
    schema::{
        records::MediaPathField, MediaId, PrColorMatte, PrEffect, PrEffectParams, PrGaussianBlur,
        PrKeyframeEasing, PrLinearWipe, PrMask, PrMediaKind, PrPointKeyframe, PrPropertyAnimation,
        PrScalarKeyframe, PrSequence, PrStaticCrop, PrStaticTransform, PrVideoItem, PrVideoTrack,
        TICKS,
    },
    tesseract_output::asset_ids_in_order,
    tests::support::{
        amount, clip_of, exported_blur, named_media, nest_of, nested_sequence, opacity_mask,
        project_document_with_media, sequence_of, text_graphic,
    },
    Omission, OmissionKind, OmissionScope,
};
use fx_schema::EditableFxCompositionDocument;
use serde_json::{json, Value};
use std::{collections::BTreeMap, ops::Range};

const FRAME: i64 = FrameRate::Fps30.ticks_per_frame();

fn import(sequence: &PrSequence) -> Value {
    let (_, media) = nested_sequence();
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        sequence,
        &media,
        &asset_ids_in_order(sequence, &media),
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    document.to_json_value().unwrap()
}

fn export(document: Value) -> (PrProjectFile, Vec<Omission>) {
    export_result(document).unwrap()
}

/// [`export`], which returns the export's error.
fn export_result(document: Value) -> crate::error::Result<(PrProjectFile, Vec<Omission>)> {
    export_with_audio(document, &BTreeMap::new())
}

fn export_with_audio(
    document: Value,
    audio: &BTreeMap<String, crate::audio_media::SourceSound>,
) -> crate::error::Result<(PrProjectFile, Vec<Omission>)> {
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut media: BTreeMap<_, _> = ["premiere-video-1", "premiere-video-2"]
        .map(|asset| {
            (
                asset.to_owned(),
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
            )
        })
        .into();
    media.insert(
        "premiere-still-1".to_owned(),
        MediaFacts::Still(crate::image_media::ValidatedImage {
            format: crate::image_media::ImageFormat::Png,
            width: 1920,
            height: 1080,
            alpha: false,
            icc_profile: false,
        }),
    );
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &media,
        audio,
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )?;
    Ok((project, omissions))
}

/// Type, name, and active range of each layer in a list.
fn summary(layers: &Value) -> Vec<(String, String, Value)> {
    layers
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| {
            (
                layer["type"].as_str().unwrap().to_owned(),
                layer["name"].as_str().unwrap().to_owned(),
                (*crate::test_support::layer_range(layer)).clone(),
            )
        })
        .collect()
}

/// Active range, source range, and asset of each video layer in a list.
fn videos(layers: &Value) -> Vec<(Value, Value, String)> {
    layers
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|layer| {
            (
                (*crate::test_support::layer_range(layer)).clone(),
                layer["sourceRange"].clone(),
                layer["source"]["assetId"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn layer_ids(layers: &Value, ids: &mut Vec<u64>) {
    for layer in layers.as_array().unwrap() {
        ids.push(layer["id"].as_u64().unwrap());
        if layer["type"] == "Group" {
            layer_ids(&layer["layers"], ids);
        }
    }
}

type NestRow = (Range<i64>, Range<i64>, Vec<(Range<i64>, Range<i64>)>);

/// Each nest: its timeline range, its inner window, and its inner placements.
fn nests(sequence: &PrSequence) -> Vec<NestRow> {
    sequence
        .nest_occurrences()
        .map(|nest| {
            (
                nest.timeline_ticks(),
                nest.in_ticks..nest.out_ticks,
                nest.sequence
                    .video_occurrences()
                    .map(|clip| (clip.timeline_ticks(), clip.source_ticks()))
                    .collect(),
            )
        })
        .collect()
}

fn ms(millis: i64) -> i64 {
    millis * crate::schema::TICKS_PER_MILLISECOND
}

#[test]
fn placements_become_groups_of_clipped_inline_copies() {
    let (outer, _) = nested_sequence();
    let document = import(&outer);
    assert_eq!(document["duration"], 11.0);
    let layers = &document["composition"]["layers"];
    let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
    assert_eq!(
        summary(layers),
        [
            ("Group".into(), "Inner".into(), range(0, 3000)),
            ("Group".into(), "Inner".into(), range(5000, 6000)),
            ("Video".into(), "Premiere video 1".into(), range(0, 10000)),
            (
                "Rect".into(),
                "Premiere black canvas".into(),
                range(0, 11000)
            ),
        ]
    );
    // Children use the group clock and are clipped to the window the placement
    // shows. Inner 1-4 s drops the 5-6 s clip; the untrimmed copy keeps both
    // clips and a transparent 4-5 s gap: no canvas is copied into a group.
    let timecoded = || "premiere-video-2".to_owned();
    assert_eq!(
        videos(&layers[0]["layers"]),
        [(range(0, 3000), range(1000, 3000), timecoded())]
    );
    assert_eq!(summary(&layers[1]["layers"]).len(), 2);
    assert_eq!(
        videos(&layers[1]["layers"]),
        [
            (range(0, 4000), range(0, 4000), timecoded()),
            (range(5000, 1000), range(7000, 1000), timecoded())
        ]
    );
    assert_eq!(
        videos(layers),
        [(range(0, 10000), range(0, 10000), "premiere-video-1".into())]
    );
    for group in &layers.as_array().unwrap()[..2] {
        assert_eq!(group["transform"], layers[3]["transform"], "identity");
        for child in group["layers"].as_array().unwrap() {
            assert_eq!(child["parent"], group["id"]);
        }
    }
}

#[test]
fn a_blended_nest_and_its_blended_clip_keep_their_blend_both_ways() {
    use crate::schema::PrBlendMode;
    // The first placement of Inner blends as Darker Color over the red clip;
    // Inner's first clip screens at half Opacity in both copies.
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.blend_mode = PrBlendMode::DarkerColor;
    let [first, second] = &mut outer.video_tracks[1].nests[..] else {
        panic!("two placements of Inner");
    };
    for copy in [first, second] {
        let clip = copy.sequence.video_tracks[0].clip_mut(0);
        (clip.blend_mode, clip.opacity) = (PrBlendMode::Screen, 50.0);
    }
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let layers = &document["composition"]["layers"];
    let blend = |layer: &Value| {
        (
            layer["blendMode"].clone(),
            layer["transform"]["opacity"].clone(),
        )
    };
    let screen = || (json!("screen"), json!(50.0));
    assert_eq!(blend(&layers[0]), (json!("darkerColor"), json!(100.0)));
    assert_eq!(blend(&layers[1]), (json!("normal"), json!(100.0)));
    assert_eq!(blend(&layers[0]["layers"][0]), screen());
    assert_eq!(blend(&layers[1]["layers"][0]), screen());
    assert_eq!(
        blend(&layers[1]["layers"][1]),
        (json!("normal"), json!(100.0))
    );
    // The Darker Color nest reports its formula; the Normal one, whose group
    // passes its Screen clip through, reports that. Screen itself converts
    // as its measured formula.
    let reports = |omissions: &[Omission]| -> Vec<(OmissionKind, String, String)> {
        omissions
            .iter()
            .filter(|omission| {
                omission.reason.contains("Blend Mode") || omission.reason.contains("passes")
            })
            .map(|omission| {
                (
                    omission.kind,
                    omission.record.clone(),
                    omission.reason.clone(),
                )
            })
            .collect()
    };
    let inner = "nested sequence \"Inner\"".to_owned();
    assert_eq!(
        reports(&omissions),
        [
            (
                OmissionKind::Approximated,
                inner.clone(),
                PrBlendMode::DarkerColor.approximation().unwrap()
            ),
            (
                OmissionKind::Approximated,
                inner,
                super::PASS_THROUGH_APPROXIMATION.to_owned()
            ),
        ]
    );

    // Export repeats both reports on the groups.
    let group = |layer: &Value| format!("layer {} (\"Inner\")", layer["id"]);
    let expected = [
        (
            OmissionKind::Approximated,
            group(&layers[1]),
            super::PASS_THROUGH_APPROXIMATION.to_owned(),
        ),
        (
            OmissionKind::Approximated,
            group(&layers[0]),
            PrBlendMode::DarkerColor.approximation().unwrap(),
        ),
    ];
    let (mut project, omissions) = export(document);
    assert_eq!(reports(&omissions), expected);
    // The written project reads the pairs back on the nest and its clips.
    for (id, media) in &mut project.media {
        let name = format!("{}.mp4", id.as_str());
        media.relative_path = Some(format!("./media/{name}"));
        media.relative_paths = vec![format!("./media/{name}")];
        media.absolute_paths = vec![(MediaPathField::FilePath, format!("/media/{name}").into())];
        media.name = name;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reloaded, _) = PrProjectFile::load(&path).unwrap();
    for project in [&project, &reloaded] {
        let nests: Vec<_> = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .map(|nest| {
                let inner: Vec<_> = nest
                    .sequence
                    .video_occurrences()
                    .map(|clip| (clip.blend_mode, clip.opacity))
                    .collect();
                (nest.blend_mode, nest.opacity, inner)
            })
            .collect();
        assert_eq!(
            nests,
            [
                (
                    PrBlendMode::DarkerColor,
                    100.0,
                    vec![(PrBlendMode::Screen, 50.0)]
                ),
                (
                    PrBlendMode::Normal,
                    100.0,
                    vec![(PrBlendMode::Screen, 50.0), (PrBlendMode::Normal, 100.0)]
                ),
            ]
        );
    }
}

#[test]
fn export_reports_the_pass_through_only_of_a_group_that_fx_draws_into_its_parent() {
    let (outer, _) = nested_sequence();
    let mut base = import(&outer);
    // The first group, layer 2 over 0-3 s, holds a Screen clip.
    base["composition"]["layers"][0]["layers"][0]["blendMode"] = json!("screen");
    let identity = base["composition"]["layers"][0]["transform"].clone();
    let (linear, hold) = (json!({"type": "linear"}), json!({"type": "hold"}));
    let ease = |y1: f64, y2: f64| json!({"type": "cubicBezier", "x1": 0.33, "y1": y1, "x2": 0.67, "y2": y2});
    let (passing, contained) = (ease(0.0, 5.0), ease(0.1, 0.9));
    let blur = |enabled: bool, blurriness: f64| json!({"id": 11, "enabled": enabled, "effect": {"type": "gaussianBlur", "blurriness": blurriness}});
    let effect = |kind: &str| json!({"id": 11, "enabled": true, "effect": {"type": kind}});
    // FX draws the group into its parent, where its Screen clip blends with
    // the red clip below it, at every frame where the group is at full
    // Opacity, unless the change isolates the group or leaves it no blend
    // inside. FX renders nothing on a group for a disabled effect, a person
    // matte, Posterize Time or an effect of a type it does not know. Whether
    // it renders another effect can depend on its values at a frame: a blur
    // of 0, or keyed to 0, renders nothing there. So any other effect, alone
    // or beside those, makes the report conditional; a blur of 10, which
    // isolates the group at every frame, reports so too, a conservative bound
    // rather than a measured drift. The group plays over 0-3 s: keys from
    // full at 0 s, a 100 held from before it starts or after keys that all
    // precede it, and a 100 held before keys that all follow it reach full;
    // Linear or Hold keys from 60 to 30, a key of 100 after the group ends
    // and a Linear fall from 100 before it starts (80 at 0 s) do not. An ease
    // from 90 to 95 that passes its keys (to about 102.8 at 0.7 s) may reach
    // full: only its control values bound it, so it reports conditionally;
    // one that stays between its keys does not.
    let possible = super::POSSIBLE_PASS_THROUGH_APPROXIMATION;
    for (case, reported) in [
        ("unchanged", Some(super::PASS_THROUGH_APPROXIMATION)),
        ("a disabled effect", Some(super::PASS_THROUGH_APPROXIMATION)),
        ("a person matte", Some(super::PASS_THROUGH_APPROXIMATION)),
        ("Posterize Time", Some(super::PASS_THROUGH_APPROXIMATION)),
        (
            "an effect of an unknown type",
            Some(super::PASS_THROUGH_APPROXIMATION),
        ),
        ("no blend inside", None),
        ("a blend", None),
        ("Opacity 60", None),
        (
            "Opacity keys from full",
            Some(super::PASS_THROUGH_APPROXIMATION),
        ),
        ("Linear keys from 60 to 30", None),
        ("Hold keys from 60 to 30", None),
        ("full Opacity after the group ends", None),
        (
            "a Hold of 100 from before the group starts",
            Some(super::PASS_THROUGH_APPROXIMATION),
        ),
        (
            "a 100 held after keys before the group starts",
            Some(super::PASS_THROUGH_APPROXIMATION),
        ),
        (
            "a 100 held before keys after the group ends",
            Some(super::PASS_THROUGH_APPROXIMATION),
        ),
        ("Linear keys from 100 before the group starts", None),
        ("an ease that passes its keys", Some(possible)),
        ("an ease between its keys", None),
        ("a Crop mask", None),
        ("a blur of 10", Some(possible)),
        ("a blur of 0", Some(possible)),
        ("a blur keyed to 0", Some(possible)),
        ("Posterize Time and a blur", Some(possible)),
        ("a blur at Opacity 60", None),
    ] {
        let mut document = base.clone();
        let group = &mut document["composition"]["layers"][0];
        let opacity = |keys: &[(i64, f64, &Value)]| json!([track(2, "opacity", keys)]);
        match case {
            "a disabled effect" => group["effects"] = json!([blur(false, 10.0)]),
            "a person matte" => group["effects"] = json!([effect("personMatte")]),
            "Posterize Time" => group["effects"] = json!([effect("posterizeTime")]),
            "an effect of an unknown type" => group["effects"] = json!([effect("futureEffect")]),
            "no blend inside" => group["layers"][0]["blendMode"] = json!("normal"),
            "a blend" => group["blendMode"] = json!("multiply"),
            "Opacity 60" => group["transform"]["opacity"] = json!(60.0),
            "Opacity keys from full" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(0, 100.0, &linear), (1000, 60.0, &linear)]);
            }
            "Linear keys from 60 to 30" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(0, 60.0, &linear), (1000, 30.0, &linear)]);
            }
            "Hold keys from 60 to 30" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(0, 60.0, &linear), (1000, 30.0, &hold)]);
            }
            "full Opacity after the group ends" => {
                document["composition"]["dynamics"]["entries"] = opacity(&[
                    (0, 60.0, &linear),
                    (3000, 60.0, &linear),
                    (4000, 100.0, &linear),
                ]);
            }
            "a Hold of 100 from before the group starts" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(-1000, 100.0, &linear), (1000, 30.0, &hold)]);
            }
            "a 100 held after keys before the group starts" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(-2000, 60.0, &linear), (-1000, 100.0, &linear)]);
            }
            "a 100 held before keys after the group ends" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(3500, 100.0, &linear), (4000, 60.0, &linear)]);
            }
            "Linear keys from 100 before the group starts" => {
                // 80 when the group starts.
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(-1000, 100.0, &linear), (1000, 60.0, &linear)]);
            }
            "an ease that passes its keys" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(0, 90.0, &linear), (1000, 95.0, &passing)]);
            }
            "an ease between its keys" => {
                document["composition"]["dynamics"]["entries"] =
                    opacity(&[(0, 90.0, &linear), (1000, 95.0, &contained)]);
            }
            "a Crop mask" => {
                group["masks"] = json!([{"id": 30, "mode": "add", "layer": 20}]);
                group["layers"].as_array_mut().unwrap().push(json!({
                    "type": "Rect", "id": 20, "parent": 2, "name": "Crop guide",
                    "activeRange": {"start": 0, "duration": 3000}, "transform": identity,
                    "rect": {"size": [1536.0, 864.0], "position": [192.0, 108.0], "fillColor": [0, 0, 0, 1]}
                }));
            }
            "a blur of 10" => group["effects"] = json!([blur(true, 10.0)]),
            "a blur of 0" => group["effects"] = json!([blur(true, 0.0)]),
            "a blur keyed to 0" => {
                group["effects"] = json!([blur(true, 10.0)]);
                document["composition"]["dynamics"]["entries"] = json!([{
                    "target": {"kind": "effectProperty", "effectId": 11, "paramName": "blurriness"},
                    "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                        {"id": "blur-0", "layerTime": 0, "value": {"type": "float", "value": 10.0}, "easing": linear},
                        {"id": "blur-1", "layerTime": 1000, "value": {"type": "float", "value": 0.0}, "easing": linear}
                    ]}
                }]);
            }
            "Posterize Time and a blur" => {
                let mut second = blur(true, 10.0);
                second["id"] = json!(12);
                group["effects"] = json!([effect("posterizeTime"), second]);
            }
            "a blur at Opacity 60" => {
                group["effects"] = json!([blur(true, 10.0)]);
                group["transform"]["opacity"] = json!(60.0);
            }
            _ => {}
        }
        let (project, omissions) = export(document);
        let report = |kind, reason: &str| Omission {
            scope: OmissionScope::Feature,
            kind,
            record: "layer 2 (\"Inner\")".into(),
            reason: reason.into(),
        };
        // Export omits the effects that have no Premiere effect; the group
        // still exports.
        let unmapped = match case {
            "a person matte" => Some("personMatte"),
            "Posterize Time" | "Posterize Time and a blur" => Some("posterizeTime"),
            "an effect of an unknown type" => Some("futureEffect"),
            _ => None,
        }
        .map(|kind| {
            let reason = if kind == "posterizeTime" {
                "Posterize Time converts only on a plain video clip: FX adjustment effects do not hold time, stills and linked compositions are not physical video owners, and nested, group, staged and retimed clocks have no mapped native equivalent"
            } else {
                "it has no Premiere effect mapping"
            };
            report(
                OmissionKind::Omitted,
                &format!("effects: {kind} effect 11 was not exported: {reason}"),
            )
        });
        let expected: Vec<_> = unmapped
            .into_iter()
            .chain(reported.map(|reason| report(OmissionKind::Approximated, reason)))
            .collect();
        assert_eq!(omissions, expected, "{case}");
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .nest_occurrences()
                .count(),
            2,
            "{case}"
        );
    }
}

#[test]
fn repeated_copies_edit_independently_and_share_one_asset() {
    let (outer, media) = nested_sequence();
    assert_eq!(asset_ids_in_order(&outer, &media).len(), 2);
    let mut document = import(&outer);
    let mut ids = Vec::new();
    layer_ids(&document["composition"]["layers"], &mut ids);
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "every inline copy is its own layer");
    // Slip the first copy only; selection and mapping move together.
    let first = &mut document["composition"]["layers"][0]["layers"][0];
    first["sourceRange"]["start"] = json!(1500);
    first["playback"]["mapping"]["output"]["start"] = json!(1500);
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        nests(project.single_sequence().unwrap()),
        [
            (
                0..3 * TICKS,
                0..3 * TICKS,
                vec![(0..3 * TICKS, ms(1500)..ms(4500))]
            ),
            (
                5 * TICKS..11 * TICKS,
                0..6 * TICKS,
                vec![
                    (0..4 * TICKS, 0..4 * TICKS),
                    (5 * TICKS..6 * TICKS, 7 * TICKS..8 * TICKS)
                ]
            ),
        ]
    );
}

#[test]
fn constant_rate_inner_clips_keep_rate_aware_source_trims() {
    for (rate, source_end, source_start, source_duration) in [
        (0.2, ms(800), 200, 600),
        (2.0, 8 * TICKS, 2000, 6000),
        (-1.0, 4 * TICKS, 6000, 3000),
    ] {
        let (mut outer, _) = nested_sequence();
        let nest = &mut outer.video_tracks[1].nests[0];
        let clip = nest.sequence.video_tracks[0].clip_mut(0);
        clip.playback_rate = rate;
        clip.out_ticks = source_end;
        let document = import(&outer);
        let video = &document["composition"]["layers"][0]["layers"][0];
        assert_eq!(
            video["playback"]["inputRange"],
            json!({"start":0,"duration":3000})
        );
        assert_eq!(
            video["sourceRange"],
            json!({"start":source_start,"duration":source_duration})
        );
        let values: Vec<_> = crate::tests::support::playback_keys(video)
            .iter()
            .map(|key| key["value"].as_i64().unwrap())
            .collect();
        let endpoints = if rate < 0.0 {
            vec![9000, 6000]
        } else {
            vec![source_start, source_start + source_duration]
        };
        assert_eq!(values, endpoints);
    }
}

#[test]
fn nested_explicit_hold_trim_preserves_source_in_and_rebuilds_input_duration() {
    // The held instant comes from cap2-native-frame-hold.xml; these windows
    // are supplementary controls around the native-derived public regression.
    let held = 879_350_472_000;
    for window in [0..4 * TICKS, TICKS..3 * TICKS, 2 * TICKS..6 * TICKS] {
        let (mut outer, _) = nested_sequence();
        let nest = &mut outer.video_tracks[1].nests[0];
        nest.start_ticks = 5 * TICKS;
        nest.end_ticks = nest.start_ticks + window.end - window.start;
        (nest.in_ticks, nest.out_ticks) = (window.start, window.end);
        let clip = nest.sequence.video_tracks[0].clip_mut(0);
        clip.in_ticks = held;
        clip.out_ticks = held + 4 * TICKS;
        clip.time_remap = Some(crate::schema::PrTimeRemap::frame_hold(held, 4 * TICKS));
        let mut reports = Vec::new();
        let content = super::visible_content(nest, &mut reports).unwrap();
        assert!(reports.is_empty(), "{reports:?}");
        let clip = content.video_tracks[0].clip(0);
        let duration = 4 * TICKS - window.start;
        let duration = duration.min(window.end - window.start);
        assert_eq!(clip.timeline_ticks(), 0..duration);
        assert_eq!(clip.source_ticks(), held..held + duration);
        assert_eq!(clip.held_source_ticks(), Some(held));
        assert_eq!(clip.playback_rate, 1.0);
    }
}

#[test]
fn nested_explicit_hold_with_keyed_effect_is_omitted_before_allocating_its_sibling() {
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[1];
    nest.id = Some("held-window".into());
    (nest.in_ticks, nest.out_ticks, nest.end_ticks) = (TICKS, 6 * TICKS, 10 * TICKS);
    let held = 879_350_472_000; // cap2-native-frame-hold.xml, as in the trim control.
    let clip = nest.sequence.video_tracks[0].clip_mut(0);
    clip.id = Some("keyed-held-child".into());
    clip.in_ticks = held;
    clip.out_ticks = held + 4 * TICKS;
    clip.time_remap = Some(crate::schema::PrTimeRemap::frame_hold(held, 4 * TICKS));
    let mut keyed = blur(10.0);
    keyed.animations = vec![crate::schema::PrEffectParamAnimation {
        param: &crate::schema::GAUSSIAN_BLUR_BLURRINESS,
        keys: crate::schema::PrEffectParamKeys::Scalar(vec![
            linear_key(held, 10.0),
            linear_key(held + TICKS, 30.0),
        ]),
    }];
    clip.effects = vec![keyed];
    let sibling = nest.sequence.video_tracks[0].clip_mut(1);
    sibling.in_ticks = held;
    sibling.out_ticks = held + TICKS;
    sibling.time_remap = Some(crate::schema::PrTimeRemap::frame_hold(held, TICKS));
    sibling.effects = vec![blur(12.0)];
    let mut without_keyed_child = outer.clone();
    without_keyed_child.video_tracks[1].nests[1]
        .sequence
        .video_tracks[0]
        .items
        .remove(0);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record: format!(
                "held-window, inner video track 0, keyed-held-child (0..{} ticks)",
                4 * TICKS
            ),
            reason: "held inner clip not converted: Gaussian Blur effect at stack position 1 has an unsupported keyed effect clock under Frame Hold".to_owned(),
        }]
    );
    // No orphan track, allocated effect/layer identity or success diagnostic:
    // the static held sibling and all outside content match removing only it.
    assert_eq!(document, import(&without_keyed_child));
    let sibling = &document["composition"]["layers"][1]["layers"][0];
    assert_eq!(
        sibling["playback"]["inputRange"],
        json!({"start":4000,"duration":1000})
    );
    assert_eq!(sibling["effects"][0]["effect"]["blurriness"], 12.0);
    assert!(crate::tests::support::playback_keys(sibling)
        .iter()
        .all(|key| key["value"] == 3462));
}

#[test]
fn variable_retimed_inner_clips_are_omitted_without_losing_their_sibling() {
    let (ramp, omissions) = PrProjectFile::load_selected(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/feature_time_remap_variable_speed_strict.prproj"),
        None,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let ramped_clip = ramp
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert!(ramped_clip.time_remap.is_some());
    {
        let (mut outer, media) = nested_sequence();
        let nest = &mut outer.video_tracks[1].nests[1];
        nest.id = Some("outer-nest".into());
        let clip = nest.sequence.video_tracks[0].clip_mut(0);
        clip.id = Some("retimed-child".into());
        clip.end_ticks = ramped_clip.end_ticks;
        clip.out_ticks = ramped_clip.out_ticks;
        clip.time_remap = ramped_clip.time_remap.clone();
        let inner_end = clip.end_ticks;
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(
            &outer,
            &media,
            &asset_ids_in_order(&outer, &media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        assert_eq!(omissions.len(), 1, "{omissions:?}");
        assert_eq!(omissions[0].scope, crate::OmissionScope::Occurrence);
        assert_eq!(
            omissions[0].record,
            format!(
                "outer-nest, inner video track 0, retimed-child (0..{} ticks)",
                inner_end
            )
        );
        assert_eq!(omissions[0].reason, "retimed inner clip not converted: trimming a source-time remapping inside a nest is not implemented");
        assert_eq!(
            videos(&document["composition"]["layers"][1]["layers"]),
            vec![(
                json!({"start": 5000, "duration": 1000}),
                json!({"start": 7000, "duration": 1000}),
                "premiere-video-2".to_owned(),
            )]
        );
    }
}

#[test]
fn stills_mattes_and_text_inside_a_nest_import_into_its_group() {
    let (mut outer, mut media) = nested_sequence();
    for (name, kind) in [
        ("still", PrMediaKind::Still { alpha: false }),
        (
            "matte",
            PrMediaKind::ColorMatte(PrColorMatte { rgb: [255, 0, 0] }),
        ),
    ] {
        let (id, mut facts) = named_media(name);
        facts.video.as_mut().unwrap().kind = kind;
        media.insert(id, facts);
    }
    // The untrimmed copy: a matte in the 4-5 s gap, a still instead of the
    // 5-6 s clip, and text above; its 0-4 s video stays.
    let nest = &mut outer.video_tracks[1].nests[1];
    nest.id = Some("outer-nest".into());
    let tracks = &mut nest.sequence.video_tracks;
    let still = tracks[0].clip_mut(1);
    (still.id, still.media) = (Some("still-child".into()), MediaId("still".into()));
    let mut matte = clip_of("matte", 4 * TICKS..5 * TICKS, 0);
    matte.id = Some("matte-child".into());
    tracks[0].items.insert(1, PrVideoItem::Media(matte));
    tracks.push(PrVideoTrack::media([]));
    tracks[1].items.push(PrVideoItem::Graphic(text_graphic()));
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // The matte, the still and the text import inside the group as at top level.
    let kinds: Vec<_> = document["composition"]["layers"][1]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap().to_owned())
        .collect();
    for kind in ["Video", "Rect", "Image", "Text"] {
        assert!(kinds.iter().any(|found| found == kind), "{kind}: {kinds:?}");
    }
    for layer in document["composition"]["layers"][1]["layers"]
        .as_array()
        .unwrap()
    {
        assert_eq!(
            layer["parent"], document["composition"]["layers"][1]["id"],
            "{layer}"
        );
    }
}

#[test]
fn inner_sound_is_trimmed_to_the_window_its_nest_shows() {
    use crate::schema::{AudioChannels, PrAudioOccurrence, PrAudioStream};
    // Inner's sound plays 0-4 s from source 0.5 s. The first copy shows inner
    // 1-4 s, so its sound starts 1 s in; the second copy shows all of it.
    let (mut outer, mut media) = nested_sequence();
    media.get_mut(&MediaId("timecoded".into())).unwrap().audio = Some(PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: 10 * TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    });
    for nest in &mut outer.video_tracks[1].nests {
        nest.sequence.audio.push(PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: TICKS / 2,
            out_ticks: 4 * TICKS + TICKS / 2,
            volume: fx_schema::LinearGain::new(0.5).unwrap(),
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        });
    }
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
    for (group, active, source) in [
        (0, range(0, 3000), range(1500, 3000)),
        (1, range(0, 4000), range(500, 4000)),
    ] {
        let group = &document["composition"]["layers"][group];
        let sounds: Vec<_> = group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .collect();
        assert_eq!(sounds.len(), 1, "{group}");
        assert_eq!(sounds[0]["parent"], group["id"]);
        assert_eq!(
            (
                crate::test_support::layer_range(sounds[0]),
                &sounds[0]["sourceRange"]
            ),
            (&active, &source)
        );
        assert_eq!(sounds[0]["volume"], 0.5);
    }
}

#[test]
fn a_nest_that_shows_part_of_an_inner_fade_drops_it() {
    use crate::schema::{
        AudioChannels, PrAudioFade, PrAudioOccurrence, PrAudioStream, PrFadeCurve,
    };
    // Inner's sound fades in over `fade_in` and out over its last 0.5 s. The
    // first copy shows inner 1-4 s: a 1 s fade-in lies wholly before it and
    // goes silently; of a 1.5 s one it shows the end, which no fade keys, so
    // that fade goes with a report. The second copy shows both fades.
    let fade = |id: &str, duration_ticks: i64| PrAudioFade {
        id: Some(id.into()),
        curve: PrFadeCurve::ConstantGain,
        duration_ticks,
    };
    for (fade_in, reported) in [(TICKS, false), (3 * TICKS / 2, true)] {
        let (mut outer, mut media) = nested_sequence();
        media.get_mut(&MediaId("timecoded".into())).unwrap().audio = Some(PrAudioStream {
            prepared_clock: None,
            intrinsic_ticks: 10 * TICKS,
            channels: AudioChannels::Stereo,
            sample_rate: 48_000,
        });
        for nest in &mut outer.video_tracks[1].nests {
            nest.sequence.audio.push(PrAudioOccurrence {
                source_channel: None,
                preserve_audio_pitch: false,
                playback_rate: 1.0,
                id: None,
                media: MediaId("timecoded".into()),
                start_ticks: 0,
                end_ticks: 4 * TICKS,
                in_ticks: TICKS / 2,
                out_ticks: 4 * TICKS + TICKS / 2,
                volume: fx_schema::LinearGain::new(0.5).unwrap(),
                volume_keys: None,
                fade_in: Some(fade("AudioTransitionTrackItem:8", fade_in)),
                fade_out: Some(fade("AudioTransitionTrackItem:9", TICKS / 2)),
            });
        }
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(
            &outer,
            &media,
            &asset_ids_in_order(&outer, &media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        let expected: Vec<_> = reported
            .then(|| {
                (
                    OmissionScope::Feature,
                    "AudioTransitionTrackItem:8".to_owned(),
                    PrAudioFade::PARTLY_PLAYED.to_owned(),
                )
            })
            .into_iter()
            .collect();
        let found: Vec<_> = omissions
            .iter()
            .map(|omission| {
                (
                    omission.scope,
                    omission.record.clone(),
                    omission.reason.clone(),
                )
            })
            .collect();
        assert_eq!(found, expected);
        let composition = &document["composition"];
        let fade_millis = fade_in / crate::schema::TICKS_PER_MILLISECOND;
        for (group, times) in [(0, vec![2500, 3000]), (1, vec![0, fade_millis, 3500, 4000])] {
            let sound = composition["layers"][group]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|layer| layer["type"] == "Audio")
                .unwrap();
            let entry = composition["dynamics"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["target"]["layerId"] == sound["id"])
                .unwrap();
            let keys: Vec<_> = entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| key["layerTime"].as_i64().unwrap())
                .collect();
            assert_eq!(keys, times, "{group}");
        }
    }
}

#[test]
fn a_nest_that_ends_inside_an_inner_fade_in_or_starts_inside_its_fade_out_drops_it() {
    use crate::schema::{
        AudioChannels, PrAudioFade, PrAudioOccurrence, PrAudioStream, PrFadeCurve,
    };
    // Inner's sound plays 0-4 s from source 0.5 s with a 1.5 s fade at one
    // edge. A nest of inner 0-1 s ends inside the fade-in, and one of inner
    // 3-4 s starts inside the fade-out: from its far edge, each shows only
    // part of the fade. Its group keeps the sound over its own window, at its
    // level and without the fade, which is reported.
    let fade = |id: &str| PrAudioFade {
        id: Some(id.into()),
        curve: PrFadeCurve::ConstantGain,
        duration_ticks: 3 * TICKS / 2,
    };
    for (fade_in, window_in, transition) in [
        (true, 0, "AudioTransitionTrackItem:8"),
        (false, 3 * TICKS, "AudioTransitionTrackItem:9"),
    ] {
        let (mut outer, mut media) = nested_sequence();
        media.get_mut(&MediaId("timecoded".into())).unwrap().audio = Some(PrAudioStream {
            prepared_clock: None,
            intrinsic_ticks: 10 * TICKS,
            channels: AudioChannels::Stereo,
            sample_rate: 48_000,
        });
        let mut inner = outer.video_tracks[1].nests[1].sequence.clone();
        inner.audio.push(PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: TICKS / 2,
            out_ticks: 4 * TICKS + TICKS / 2,
            volume: fx_schema::LinearGain::new(0.5).unwrap(),
            volume_keys: None,
            fade_in: fade_in.then(|| fade(transition)),
            fade_out: (!fade_in).then(|| fade(transition)),
        });
        outer.video_tracks[1].nests = vec![nest_of(inner, 0..TICKS, window_in)];
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(
            &outer,
            &media,
            &asset_ids_in_order(&outer, &media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        let found: Vec<_> = omissions
            .iter()
            .map(|omission| {
                (
                    omission.scope,
                    omission.record.as_str(),
                    omission.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [(
                OmissionScope::Feature,
                transition,
                PrAudioFade::PARTLY_PLAYED
            )]
        );
        let group = &document["composition"]["layers"][0];
        let sounds: Vec<_> = group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .collect();
        let [sound] = sounds.as_slice() else {
            panic!("{group}");
        };
        let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
        assert_eq!(
            (
                crate::test_support::layer_range(sound),
                &sound["sourceRange"],
                &sound["volume"]
            ),
            (
                &range(0, 1000),
                &range(500 + window_in / crate::schema::TICKS_PER_MILLISECOND, 1000),
                &json!(0.5)
            ),
            "{transition}"
        );
        // Without its fade, the sound plays at its static level.
        let keyed = document["composition"]["dynamics"]["entries"]
            .as_array()
            .is_some_and(|entries| {
                entries
                    .iter()
                    .any(|entry| entry["target"]["layerId"] == sound["id"])
            });
        assert!(!keyed, "{document}");
    }
}

#[test]
fn faded_sounds_whose_level_keys_cannot_import_stay_silent_at_root_and_in_nests() {
    use crate::schema::{
        AudioChannels, PrAudioFade, PrAudioOccurrence, PrAudioStream, PrFadeCurve, PrVolumeKeys,
    };
    // One sound at the root and inside both copies of the nest, with a 0.5 s
    // fade-out. Its two Level keys 0.4 ms apart round onto one millisecond
    // of each layer clock, so they cannot import: every copy keeps its
    // placement at zero gain, and its fade keys do not make it audible.
    let key = |source_ticks: i64| PrScalarKeyframe {
        source_ticks,
        value: 0.5,
        easing: PrKeyframeEasing::Linear,
    };
    let invalid = PrVolumeKeys {
        keys: vec![key(2 * TICKS), key(2 * TICKS + TICKS / 2500)],
        gain: 1.0,
    };
    for keys in [None, Some(invalid)] {
        let (mut outer, mut media) = nested_sequence();
        media.get_mut(&MediaId("timecoded".into())).unwrap().audio = Some(PrAudioStream {
            prepared_clock: None,
            intrinsic_ticks: 10 * TICKS,
            channels: AudioChannels::Stereo,
            sample_rate: 48_000,
        });
        let sound = PrAudioOccurrence {
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: 1.0,
            id: Some("AudioClipTrackItem:7".into()),
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: TICKS / 2,
            out_ticks: 4 * TICKS + TICKS / 2,
            volume: fx_schema::LinearGain::new(0.5).unwrap(),
            volume_keys: keys.clone(),
            fade_in: None,
            fade_out: Some(PrAudioFade {
                id: Some("AudioTransitionTrackItem:8".into()),
                curve: PrFadeCurve::ConstantGain,
                duration_ticks: TICKS / 2,
            }),
        };
        outer.audio.push(sound.clone());
        for nest in &mut outer.video_tracks[1].nests {
            nest.sequence.audio.push(sound.clone());
        }
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(
            &outer,
            &media,
            &asset_ids_in_order(&outer, &media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        let composition = &document["composition"];
        let sounds: Vec<_> = composition["layers"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|layer| match layer["type"].as_str() {
                Some("Group") => layer["layers"].as_array().unwrap().iter().collect(),
                _ => vec![layer],
            })
            .filter(|layer| layer["type"] == "Audio")
            .collect();
        assert_eq!(sounds.len(), 3, "{document}");
        let keyed: Vec<_> = composition["dynamics"]["entries"]
            .as_array()
            .map_or_else(Vec::new, Clone::clone)
            .into_iter()
            .filter(|entry| entry["target"]["propertyType"] == "volume")
            .map(|entry| entry["target"]["layerId"].clone())
            .collect();
        if keys.is_none() {
            // Without the Level keys, each copy's fade converts.
            assert!(omissions.is_empty(), "{omissions:?}");
            assert_eq!(keyed.len(), 3, "{document}");
            for sound in &sounds {
                assert_eq!(sound["volume"], 0.5);
                assert!(keyed.contains(&sound["id"]), "{sound}");
            }
            continue;
        }
        assert!(keyed.is_empty(), "{document}");
        for sound in &sounds {
            assert_eq!(sound["volume"], 0.0, "{sound}");
        }
        assert_eq!(omissions.len(), 3, "{omissions:?}");
        for omission in &omissions {
            assert_eq!(
                (omission.scope, omission.record.as_str()),
                (OmissionScope::Feature, "AudioClipTrackItem:7")
            );
            assert!(
                omission
                    .reason
                    .starts_with("volume animation was not imported: ")
                    && omission
                        .reason
                        .ends_with("; the sound was kept at zero gain"),
                "{}",
                omission.reason
            );
        }
    }
}

#[test]
fn nested_wipe_key_collision_omits_only_its_occurrence() {
    let (mut sequence, media) = nested_sequence();
    let nest = &mut sequence.video_tracks[1].nests[0];
    let record = nest.record();
    nest.linear_wipe = Some(PrLinearWipe {
        initial_completion: 50.0,
        completion: vec![
            PrScalarKeyframe {
                source_ticks: nest.in_ticks,
                value: 50.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: nest.in_ticks + 1,
                value: 75.0,
                easing: PrKeyframeEasing::Linear,
            },
        ],
        angle_degrees: 90,
        feather: 0.0,
    });
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &sequence,
        &media,
        &asset_ids_in_order(&sequence, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].record, record);
    assert_eq!(omissions[0].scope, OmissionScope::Occurrence);
    assert!(
        omissions[0].reason.contains("nested Linear Wipe"),
        "{omissions:?}"
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Group")
            .count(),
        1,
        "unrelated nest survives"
    );
    let (_, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
}

#[test]
fn nested_wipe_guides_share_the_group_parent_and_use_distinct_ids() {
    let (mut outer, _) = nested_sequence();
    outer.video_tracks[1].nests[0].sequence.video_tracks[0]
        .clip_mut(0)
        .linear_wipe = Some(crate::schema::PrLinearWipe {
        initial_completion: 25.0,
        completion: vec![PrScalarKeyframe {
            source_ticks: TICKS,
            value: 25.0,
            easing: PrKeyframeEasing::Linear,
        }],
        angle_degrees: 90,
        feather: 0.0,
    });
    let document = import(&outer);
    let group = &document["composition"]["layers"][0];
    let video = &group["layers"][0];
    let guide = &group["layers"][1];
    assert_eq!(group["layers"].as_array().unwrap().len(), 2);
    assert_eq!(video["parent"], group["id"]);
    assert_eq!(guide["parent"], group["id"]);
    assert_eq!(video["masks"][0]["layer"], guide["id"]);
    assert_eq!(guide["transform"]["scale"], json!([75.0, 100.0]));
    let mut ids = Vec::new();
    layer_ids(&document["composition"]["layers"], &mut ids);
    ids.push(video["masks"][0]["id"].as_u64().unwrap());
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let wipe = project.sequences[0].video_tracks[1].nests[0]
        .sequence
        .video_tracks[0]
        .clip(0)
        .linear_wipe
        .as_ref()
        .unwrap();
    assert_eq!(wipe.initial_completion, 25.0);
    assert_eq!(wipe.angle_degrees, 90);
}

#[test]
fn nested_boundaries_round_once_like_top_level_boundaries() {
    let leaf = sequence_of(
        "Leaf",
        vec![PrVideoTrack::media([clip_of(
            "timecoded",
            FRAME..31 * FRAME,
            0,
        )])],
    );
    let mid = sequence_of(
        "Mid",
        vec![PrVideoTrack {
            transitions: Vec::new(),
            items: Vec::new(),
            nests: vec![nest_of(leaf, FRAME..61 * FRAME, 0)],
        }],
    );
    let outer = sequence_of(
        "Outer",
        vec![PrVideoTrack {
            transitions: Vec::new(),
            items: Vec::new(),
            nests: vec![nest_of(mid, FRAME..91 * FRAME, 0)],
        }],
    );
    let document = import(&outer);
    let mid = &document["composition"]["layers"][0];
    let leaf = &mid["layers"][0];
    let clip = &leaf["layers"][0];
    // Frames 1, 2 and 3 are 33.3, 66.7 and 100 ms. Each group starts at its
    // rounded absolute time, so the parts add up to 33 + 34 + 33 = 100 ms
    // instead of accumulating three separate roundings.
    assert_eq!(
        (*crate::test_support::layer_range(mid)),
        json!({"start": 33, "duration": 3000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(leaf)),
        json!({"start": 34, "duration": 2000})
    );
    assert_eq!(
        (*crate::test_support::layer_range(clip)),
        json!({"start": 33, "duration": 1000})
    );
    assert_eq!(clip["sourceRange"], json!({"start": 0, "duration": 1000}));
}

#[test]
fn plain_groups_export_as_nests_played_from_inner_time_zero() {
    // Independent editable input: export must not rely on import's IDs or names.
    let mut document = crate::test_support::editable_document();
    document["duration"] = json!(11);
    let mut video = document["composition"]["layers"][0].clone();
    video["sourceIntrinsicDuration"] = json!(10000);
    video["playback"] = linear_playback(
        json!({"start": 0, "duration": 10000}),
        json!({"start": 0, "duration": 10000}),
    );
    video["sourceRange"] = json!({"start": 0, "duration": 10000});
    let mut canvas = document["composition"]["layers"][1].clone();
    canvas["activeRange"]["duration"] = json!(11000);
    let child = |id, parent, start, duration, source_start| {
        let mut child = video.clone();
        child["id"] = json!(id);
        child["parent"] = json!(parent);
        child["playback"] = linear_playback(
            json!({"start": start, "duration": duration}),
            json!({"start": source_start, "duration": duration}),
        );
        child["sourceRange"] = json!({"start": source_start, "duration": duration});
        child["source"]["assetId"] = json!("premiere-video-2");
        child
    };
    let group = |id, start, duration, children| {
        json!({
            "type": "Group", "id": id, "name": "Inner", "blendMode": "normal",
            "playback": linear_playback(
                json!({"start": start, "duration": duration}),
                json!({"start": 0, "duration": duration}),
            ),
            "transform": canvas["transform"], "layers": children
        })
    };
    document["composition"]["layers"] = json!([
        group(10, 0, 3000, vec![child(11, 10, 0, 3000, 1000)]),
        group(
            20,
            5000,
            6000,
            vec![child(21, 20, 0, 4000, 0), child(22, 20, 5000, 1000, 7000)]
        ),
        video,
        canvas
    ]);
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        sequence.video_tracks().map(<[_]>::len).collect::<Vec<_>>(),
        [1, 0]
    );
    assert_eq!(
        nests(sequence),
        [
            (
                0..3 * TICKS,
                0..3 * TICKS,
                vec![(0..3 * TICKS, TICKS..4 * TICKS)]
            ),
            (
                5 * TICKS..11 * TICKS,
                0..6 * TICKS,
                vec![
                    (0..4 * TICKS, 0..4 * TICKS),
                    (5 * TICKS..6 * TICKS, 7 * TICKS..8 * TICKS)
                ]
            ),
        ]
    );
    for nest in sequence.nest_occurrences() {
        assert_eq!(nest.sequence.name(), "Inner");
        assert_eq!(nest.sequence.frame_rate, FrameRate::Fps30);
    }
    assert_eq!(project.media.len(), 2);
}

#[test]
fn group_sound_follows_the_audio_rules_and_embedded_sound_is_reported() {
    let mut document = crate::test_support::editable_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut video = layers.remove(0);
    video["parent"] = json!(10);
    video["sourceIntrinsicDuration"] = json!(10000);
    video["volume"] = json!(0.75);
    let sound = json!({
        "type": "Audio", "id": 11, "parent": 10, "name": "Inner sound",
        "playback": video["playback"], "sourceRange": video["sourceRange"],
        "sourceIntrinsicDuration": 10000, "volume": 1.0,
        "source": {"assetId": "uninspected-nested-sound"}
    });
    layers.insert(
        0,
        json!({
            "type": "Group", "id": 10, "name": "Inner", "blendMode": "normal",
            "playback": linear_playback(crate::test_support::layer_range(&video).clone(), json!({"start": 0, "duration": 1000})), "transform": layers[0]["transform"],
            "layers": [video, sound]
        }),
    );
    let (project, omissions) = export(document);
    // The group's sound follows the audio rules (its uninspected source has no
    // sound to export); the embedded sound of the video is reported.
    assert_eq!(
        omissions
            .iter()
            .map(|item| (item.scope, item.record.as_str(), item.reason.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                crate::OmissionScope::Occurrence,
                "layer 11 (\"Inner sound\")",
                "audio source has no sound to export"
            ),
            (
                crate::OmissionScope::Feature,
                "group 10, layer 1 (\"Source\")",
                "embedded clip sound inside a nested sequence is not exported"
            ),
        ],
        "{omissions:?}"
    );
    let outer = project.single_sequence().unwrap();
    assert!(outer.audio.is_empty());
    let inner = &outer.nest_occurrences().next().unwrap().sequence;
    assert!(inner.audio.is_empty());
    assert_eq!(inner.video_occurrences().count(), 1);
    assert_eq!(project.media.len(), 1);
    assert!(project.media.values().all(|media| media.audio.is_none()));
}

#[test]
fn stills_and_rectangles_inside_a_group_export_into_its_nest() {
    let mut document = crate::test_support::editable_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut video = layers.remove(0);
    video["parent"] = json!(10);
    video["sourceIntrinsicDuration"] = json!(10000);
    let (range, transform) = (
        (*crate::test_support::layer_range(&video)).clone(),
        layers[0]["transform"].clone(),
    );
    let image = json!({
        "type": "Image", "id": 11, "parent": 10, "name": "Inner still",
        "activeRange": range, "transform": transform,
        "source": {
            "assetId": "premiere-still-1", "fit": "contain",
            "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}
        }
    });
    let solid = json!({
        "type": "Rect", "id": 13, "parent": 10, "name": "Inner solid",
        "activeRange": range, "transform": transform,
        "rect": {"size": [1920, 1080], "fillColor": [1, 0, 0, 1]}
    });
    layers.insert(
        0,
        json!({
            "type": "Group", "id": 10, "name": "Inner", "blendMode": "normal",
            "playback": linear_playback(range.clone(), json!({"start": 0, "duration": 1000})), "transform": transform,
            "layers": [image, solid, video]
        }),
    );
    let (project, omissions) = export(document);
    assert!(
        omissions
            .iter()
            .all(|item| item.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let outer = project.single_sequence().unwrap();
    let inner = &outer.nest_occurrences().next().unwrap().sequence;
    // The still, the rectangle as a Color Matte and the video; a nested
    // graphic is covered by `a_graphic_group_inside_another_group_exports_into_its_nest`.
    assert_eq!(inner.video_occurrences().count(), 3, "{omissions:?}");
}

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

#[test]
fn inner_clip_effects_stay_on_the_group_child() {
    let (mut outer, _) = nested_sequence();
    outer.video_tracks[0].clip_mut(0).effects = vec![blur(10.0)];
    for nest in &mut outer.video_tracks[1].nests {
        nest.sequence.video_tracks[0].clip_mut(0).effects = vec![blur(25.0)];
    }
    let document = import(&outer);
    let layers = &document["composition"]["layers"];
    let effects = |id: u64, blurriness: f64| json!([{"id": id, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": blurriness}}]);
    // Ids stay unique across the document: the upper track's groups come first.
    assert_eq!(layers[0]["layers"][0]["effects"], effects(1, 25.0));
    assert_eq!(layers[1]["layers"][0]["effects"], effects(2, 25.0));
    assert_eq!(layers[2]["effects"], effects(3, 10.0));
}

/// Supplementary model/edit/writer coverage; the native host regression is
/// `coeditor_native_nested_lumetri_keeps_occurrence_and_editable_effects`.
#[test]
fn nested_occurrence_effects_keep_source_stack_then_outer_motion_and_edited_export() {
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.sequence.video_tracks[0].clip_mut(0).effects = vec![blur(10.0)];
    nest.transform.position = [0.25, 0.5];
    let mut keyed = blur(25.0);
    keyed.animations = vec![crate::schema::PrEffectParamAnimation {
        param: &crate::schema::GAUSSIAN_BLUR_BLURRINESS,
        keys: crate::schema::PrEffectParamKeys::Scalar(vec![
            PrScalarKeyframe {
                source_ticks: TICKS,
                value: 25.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: 2 * TICKS,
                value: 50.0,
                easing: PrKeyframeEasing::Linear,
            },
        ]),
    }];
    nest.effects = vec![blur(5.0), keyed];
    let mut omissions = Vec::new();
    let converted = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let mut document = converted.to_json_value().unwrap();
    let group = &mut document["composition"]["layers"][0];
    assert_eq!(group["transform"]["position"], json!([480.0, 540.0]));
    assert!(group.get("effects").is_none());
    let picture = &mut group["layers"][0];
    assert_eq!(picture["effects"][0]["effect"]["blurriness"], 5.0);
    assert_eq!(picture["effects"][1]["effect"]["blurriness"], 25.0);
    assert_eq!(
        picture["layers"][0]["effects"][0]["effect"]["blurriness"],
        10.0
    );
    let keyed_id = picture["effects"][1]["id"].clone();
    picture["effects"][0]["effect"]["blurriness"] = json!(15.0);
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["effectId"] == keyed_id)
        .unwrap();
    assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 1000);
    let (native, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let moved = native
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(moved.transform.position, [0.25, 0.5]);
    assert!(moved.effects.is_empty());
    let effected = moved.sequence.nest_occurrences().next().unwrap();
    assert_eq!(effected.effects.len(), 2);
    assert_eq!(effected.effects[0], exported_blur(true, 15.0, false));
    assert_eq!(
        effected.effects[1].params,
        exported_blur(true, 25.0, false).params
    );
    assert_eq!(
        effected.effects[1].animations[0].keys.scalar().unwrap()[1].source_ticks,
        TICKS
    );
    assert_eq!(
        effected
            .sequence
            .video_occurrences()
            .next()
            .unwrap()
            .effects
            .len(),
        1
    );
    // Serialize the current editable content and effects, not retained bytes.
    let xml = write_nested_effect_project(native);
    assert_eq!(
        xml.matches("<MatchName>AE.Impact_Blur_FX</MatchName>")
            .count(),
        3
    );
}

#[test]
fn nested_occurrence_contrast_edit_exports_as_native_component() {
    let (mut outer, media) = nested_sequence();
    outer.video_tracks[1].nests[0].effects = vec![PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::BrightnessContrast(crate::schema::PrBrightnessContrast {
            brightness: 0.0,
            contrast: 0.0,
        }),
        animations: Vec::new(),
    }];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let mut wire = document.to_json_value().unwrap();
    wire["composition"]["layers"][0]["layers"][0]["effects"][0]["effect"]["contrast"] = json!(35.0);
    let (native, omissions) = export(wire);
    assert!(omissions.is_empty(), "{omissions:?}");
    let effect = &native
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap()
        .sequence
        .nest_occurrences()
        .next()
        .unwrap()
        .effects[0];
    assert_eq!(
        effect.params,
        PrEffectParams::BrightnessContrast(crate::schema::PrBrightnessContrast {
            brightness: 0.0,
            contrast: 35.0,
        })
    );
    let xml = write_nested_effect_project(native);
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let contrast = dom
        .descendants()
        .find(|node| node.tag_name().name() == "Name" && node.text() == Some("Contrast"))
        .unwrap()
        .parent()
        .unwrap();
    let value = contrast
        .children()
        .find(|node| node.tag_name().name() == "StartKeyframe")
        .unwrap()
        .text()
        .unwrap()
        .split(',')
        .nth(1)
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert_eq!(value, 35.0);
}

fn write_nested_effect_project(mut project: PrProjectFile) -> String {
    for (id, media) in &mut project.media {
        let extension = if media.is_still() { "png" } else { "mp4" };
        let name = format!("{}.{extension}", id.as_str());
        media.relative_path = Some(format!("./media/{name}"));
        media.relative_paths = vec![format!("./media/{name}")];
        media.absolute_paths = vec![(MediaPathField::FilePath, format!("/media/{name}").into())];
        media.name = name;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    crate::format::read_xml(&path).unwrap()
}

fn load_nested_effect_project(xml: &str) -> (PrProjectFile, Vec<Omission>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    std::fs::write(&path, xml).unwrap();
    PrProjectFile::load(&path).unwrap()
}

#[test]
fn nested_occurrence_frame_effect_omission_keeps_supported_stack_and_picture() {
    let (mut outer, media) = nested_sequence();
    let mut repeated = blur(10.0);
    repeated.params = PrEffectParams::GaussianBlur(PrGaussianBlur {
        blurriness: 10.0,
        repeat_edge_pixels: true,
    });
    outer.video_tracks[1].nests[0].effects = vec![repeated, blur(25.0)];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(omissions[0]
        .reason
        .contains("no fixed native canvas bounds"));
    let picture = &document["composition"]["layers"][0]["layers"][0];
    assert_eq!(picture["effects"].as_array().unwrap().len(), 1);
    assert_eq!(picture["effects"][0]["effect"]["blurriness"], 25.0);
    assert_eq!(picture["layers"][0]["type"], "Video");
}

#[test]
fn group_child_effects_export_in_the_inner_sequence() {
    let mut document = crate::test_support::editable_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut video = layers.remove(0);
    video["parent"] = json!(10);
    video["sourceIntrinsicDuration"] = json!(10000);
    video["effects"] = json!([
        {"id": 1, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 25.0}},
        {"id": 2, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 5.0}}
    ]);
    layers.insert(
        0,
        json!({
            "type": "Group", "id": 10, "name": "Inner", "blendMode": "normal",
            "playback": linear_playback(crate::test_support::layer_range(&video).clone(), json!({"start": 0, "duration": 1000})), "transform": layers[0]["transform"],
            "layers": [video]
        }),
    );
    let key = |id: &str, layer_time: i64, value: f64| json!({"id": id, "layerTime": layer_time, "value": {"type": "float", "value": value}, "easing": {"type": "linear"}});
    document["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 2, "paramName": "blurriness"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [key("a", 0, 5.0), key("b", 500, 10.0)]},
    }]});
    let (mut project, omissions) = export(document);
    // The keyed blur exports with its keys, as at the top level.
    assert!(omissions.is_empty(), "{omissions:?}");
    for (id, media) in &mut project.media {
        let name = format!("{}.mp4", id.as_str());
        media.relative_path = Some(format!("./media/{name}"));
        media.relative_paths = vec![format!("./media/{name}")];
        media.absolute_paths = vec![(MediaPathField::FilePath, format!("/media/{name}").into())];
        media.name = name;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    // The outer sequence places only the nest, so both components are the
    // inner placement's.
    let xml = crate::format::read_xml(&path).unwrap();
    assert_eq!(
        xml.matches("<MatchName>AE.Impact_Blur_FX</MatchName>")
            .count(),
        2
    );
    let (reloaded, _) = PrProjectFile::load(&path).unwrap();
    let outer = reloaded.single_sequence().unwrap();
    assert_eq!(outer.video_occurrences().count(), 0);
    let inner = &outer.nest_occurrences().next().unwrap().sequence;
    let effects = &inner.video_occurrences().next().unwrap().effects;
    let key = |millis: i64, value: f64| crate::schema::PrScalarKeyframe {
        source_ticks: millis * crate::schema::TICKS_PER_MILLISECOND,
        value,
        easing: crate::schema::PrKeyframeEasing::Linear,
    };
    let mut keyed = exported_blur(true, 5.0, false);
    keyed.animations = vec![crate::schema::PrEffectParamAnimation {
        param: &crate::schema::FILM_IMPACT_BLUR_AMOUNT,
        keys: crate::schema::PrEffectParamKeys::Scalar(vec![
            key(0, amount(5.0)),
            key(500, amount(10.0)),
        ]),
    }];
    assert_eq!(effects, &[exported_blur(true, 25.0, false), keyed]);
}

#[test]
fn groups_a_nest_cannot_represent_are_omitted_with_their_reason() {
    let (outer, _) = nested_sequence();
    let base = import(&outer);
    let time_remap = json!({
        "keyframes": [
            {"id": "a", "time": 0, "value": 0, "easing": {"type": "linear"}},
            {"id": "b", "time": 3000, "value": 1500, "easing": {"type": "linear"}}
        ],
        "before": "inactive",
        "after": "inactive"
    });
    // Scale keys of the first group from 100 to 200 whose easing dips to
    // -55.1 between them, or to about -4.4e156 with a finite handle that FX
    // accepts.
    let (linear, dip, vast_dip) = (
        json!({"type": "linear"}),
        json!({"type": "cubicBezier", "x1": 0.25, "y1": -4.0, "x2": 0.75, "y2": 1.0}),
        json!({"type": "cubicBezier", "x1": 0.25, "y1": -1e155, "x2": 0.75, "y2": 1.0}),
    );
    let dipping_scale = |easing: &Value| {
        let keys = [(0, 100.0, &linear), (1000, 200.0, easing)];
        ["scaleX", "scaleY"].map(|property| track(2, property, &keys))
    };
    for (field, value, reason) in [
        (
            "paddingTop",
            json!(8.0),
            "group backgrounds are not supported",
        ),
        (
            "playback",
            crate::test_support::remapped_playback(json!({"start": 1000, "duration": 3000}), time_remap),
            "group time remapping is not supported",
        ),
        (
            "name",
            json!(""),
            "the nested sequence name must have 1 to 255 characters",
        ),
        (
            "masks",
            json!([{"id": 1, "mode": "add", "inverted": false, "layer": 2}]),
            "the mask guide is not a rectangle or shape beside the group",
        ),
        // The red clip (layer 1) plays 0-10 s under the 0-3 s nest.
        (
            "trackMatte",
            json!({"mode": "alpha", "layer": 1}),
            "the track matte source's range differs from the clip's; only a source spanning exactly the clip's range converts",
        ),
        (
            "fills",
            json!([{"paint": {"type": "solid", "color": [1.0, 0.0, 0.0, 1.0]}}]),
            "group backgrounds are not supported",
        ),
        (
            "transform",
            json!({"anchorPoint": [0, 0], "position": [0, 0], "scale": [-100, 100], "rotation": 0, "opacity": 100}),
            "flip (negative scale) was not exported",
        ),
        (
            "dynamics",
            json!({ "entries": dipping_scale(&dip) }),
            "flip (negative scale) was not exported",
        ),
        (
            "dynamics",
            json!({ "entries": dipping_scale(&vast_dip) }),
            "flip (negative scale) was not exported",
        ),
    ] {
        let mut document = base.clone();
        // The composition's dynamics, or a field of the first group.
        let owner = match field {
            "dynamics" => &mut document["composition"],
            _ => &mut document["composition"]["layers"][0],
        };
        owner[field] = value;
        let (project, omissions) = export(document);
        assert!(
            omissions.iter().any(|item| item.reason
                == format!("group was not exported as a nested sequence: {reason}")),
            "{field}: {omissions:?}"
        );
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .nest_occurrences()
                .count(),
            1,
            "{field}"
        );
    }
}

/// A keyframe track of `property` on layer `layer`, one key per `(layer time,
/// value, easing)`.
fn track(layer: u64, property: &str, keys: &[(i64, f64, &Value)]) -> Value {
    let keys: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(index, (time, value, easing))| {
            json!({
                "id": format!("{layer}-{property}-{index}"), "layerTime": time,
                "value": {"type": "float", "value": value}, "easing": easing
            })
        })
        .collect();
    json!({
        "target": {"kind": "layer", "layerId": layer, "propertyType": property},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": keys}
    })
}

#[test]
fn an_edited_group_exports_its_motion_opacity_and_keys_on_its_placement() {
    let (outer, _) = nested_sequence();
    let mut document = import(&outer);
    // The first group, layer 2 over 0-3 s, turns about its centre.
    let transform = &mut document["composition"]["layers"][0]["transform"];
    transform["anchorPoint"] = json!([960.0, 540.0]);
    transform["position"] = json!([1060.0, 490.0]);
    transform["scale"] = json!([70.0, 70.0]);
    transform["rotation"] = json!(10.0);
    transform["opacity"] = json!(60.0);
    let (linear, hold) = (json!({"type": "linear"}), json!({"type": "hold"}));
    let bezier = json!({"type": "cubicBezier", "x1": 0.25, "y1": 0.1, "x2": 0.25, "y2": 1.0});
    // Opacity falls, holds from 1 s and eases down after 1.5 s; the key that
    // starts the Hold is reached linearly (F26).
    let opacity = [
        (0, 100.0, &linear),
        (1000, 60.0, &linear),
        (1500, 90.0, &hold),
        (2500, 40.0, &bezier),
    ];
    // Position, Rotation and uniform Scale move linearly over 0-2 s.
    let mut entries = vec![track(2, "opacity", &opacity)];
    for (property, [start, end]) in [
        ("positionX", [1060.0, 1160.0]),
        ("positionY", [490.0, 590.0]),
        ("rotation", [10.0, 30.0]),
        ("scaleX", [70.0, 100.0]),
        ("scaleY", [70.0, 100.0]),
    ] {
        entries.push(track(
            2,
            property,
            &[(0, start, &linear), (2000, end, &linear)],
        ));
    }
    document["composition"]["dynamics"]["entries"] = json!(entries);
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let nest = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(
        (nest.timeline_ticks(), nest.opacity, nest.enabled),
        (0..3 * TICKS, 60.0, true)
    );
    // Position is normalized to the canvas and the anchor to the nest's
    // canvas-sized picture; keys count from the placement start.
    assert_eq!(
        nest.transform,
        PrStaticTransform {
            position: [1060.0 / 1920.0, 490.0 / 1080.0],
            anchor_point: [0.5, 0.5],
            scale: [70.0, 70.0],
            rotation: 10.0,
        }
    );
    let scalar = |keys: &[(i64, f64, PrKeyframeEasing)]| {
        keys.iter()
            .map(|&(millis, value, easing)| PrScalarKeyframe {
                source_ticks: ms(millis),
                value,
                easing,
            })
            .collect::<Vec<_>>()
    };
    let linear = PrKeyframeEasing::Linear;
    let point = |millis, value: [f64; 2]| PrPointKeyframe {
        source_ticks: ms(millis),
        value: [value[0] / 1920.0, value[1] / 1080.0],
        easing: linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    assert_eq!(
        nest.animations,
        [
            PrPropertyAnimation::Opacity(scalar(&[
                (0, 100.0, linear),
                (1000, 60.0, linear),
                (1500, 90.0, PrKeyframeEasing::Hold),
                (
                    2500,
                    40.0,
                    PrKeyframeEasing::CubicBezier {
                        x1: 0.25,
                        y1: 0.1,
                        x2: 0.25,
                        y2: 1.0
                    }
                ),
            ])),
            PrPropertyAnimation::Rotation(scalar(&[(0, 10.0, linear), (2000, 30.0, linear)])),
            PrPropertyAnimation::Position(vec![
                point(0, [1060.0, 490.0]),
                point(2000, [1160.0, 590.0])
            ]),
            PrPropertyAnimation::UniformScale(scalar(&[(0, 70.0, linear), (2000, 100.0, linear)])),
        ]
    );
}

#[test]
fn a_moved_nest_imports_clipped_to_its_frame_and_exports_its_motion_back() {
    let (mut outer, _) = nested_sequence();
    // The first nest (0-3 s, In 1 s) eases its Scale up from 120 to 150 and
    // sits right of centre. The reader gives a keyed Scale its first key's
    // static value.
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.transform.position = [0.6, 0.5];
    nest.transform.scale = [120.0; 2];
    let keys = vec![
        PrScalarKeyframe {
            source_ticks: TICKS + 5 * FRAME,
            value: 120.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: TICKS + 20 * FRAME,
            value: 150.0,
            easing: PrKeyframeEasing::CubicBezier {
                x1: 0.33,
                y1: 0.0,
                x2: 0.67,
                y2: 1.0,
            },
        },
    ];
    nest.animations = vec![PrPropertyAnimation::UniformScale(keys.clone())];
    let document = import(&outer);
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["transform"]["anchorPoint"], json!([960.0, 540.0]));
    assert_eq!(group["transform"]["position"], json!([1152.0, 540.0]));
    let mask = &group["masks"][0];
    let guide = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == mask["layer"])
        .unwrap();
    assert_eq!(
        (
            &guide["parent"],
            &guide["rect"]["size"],
            &guide["transform"]
        ),
        (
            &group["id"],
            &json!([1920.0, 1080.0]),
            &document["composition"]["layers"][3]["transform"]
        )
    );
    let scale: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == group["id"])
        .map(|entry| {
            (
                entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                entry["animator"]["keyframes"][0]["layerTime"].clone(),
                entry["animator"]["keyframes"][1]["layerTime"].clone(),
            )
        })
        .collect();
    assert_eq!(
        scale,
        [
            ("scaleX".to_owned(), json!(167), json!(667)),
            ("scaleY".to_owned(), json!(167), json!(667))
        ]
    );
    // Export writes the same Motion on the placement: the frame mask is the
    // whole canvas, which is no Crop.
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(exported.timeline_ticks(), 0..3 * TICKS);
    assert_eq!(exported.transform.position, [0.6, 0.5]);
    assert!(exported.crop.is_default(), "{:?}", exported.crop);
    let [PrPropertyAnimation::UniformScale(exported_keys)] = exported.animations.as_slice() else {
        panic!("{:?}", exported.animations);
    };
    assert_eq!(
        exported_keys
            .iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        // FX keeps whole milliseconds: 5 and 20 frames are 167 and 667 ms.
        [(ms(167), 120.0), (ms(667), 150.0)]
    );
}

#[test]
fn an_unmoved_nest_group_takes_its_opacity_and_keys() {
    // The first nest plays Inner 1-4 s over 0-3 s and fades by Linear Opacity
    // keys at source 1.5 s and 2.5 s; the second plays Inner from 0 s at
    // Opacity 0. Both keep default Motion on the outer canvas.
    let (mut outer, _) = nested_sequence();
    let [faded, hidden] = &mut outer.video_tracks[1].nests[..] else {
        panic!("two placements of Inner");
    };
    faded.animations = vec![PrPropertyAnimation::Opacity(vec![
        linear_key(3 * TICKS / 2, 100.0),
        linear_key(5 * TICKS / 2, 40.0),
    ])];
    hidden.opacity = 0.0;
    let document = import(&outer);
    let layers = &document["composition"]["layers"];
    // Opacity moves no pixel: each group keeps a plain group's identity
    // transform, the black canvas's, but for its static Opacity, and has no
    // frame mask.
    for (group, opacity) in [(&layers[0], 100.0), (&layers[1], 0.0)] {
        let mut expected = layers[3]["transform"].clone();
        expected["opacity"] = json!(opacity);
        assert_eq!(group["transform"], expected, "{group}");
        assert!(
            group["masks"].as_array().is_none_or(Vec::is_empty),
            "{group}"
        );
    }
    // The keys count on the group clock from In, as Motion keys do.
    let key = |time: i64, value: f64| (json!(time), json!(value));
    assert_eq!(
        motion_keys(&document, &layers[0]),
        [(json!("opacity"), vec![key(500, 100.0), key(1500, 40.0)])]
    );
    assert!(motion_keys(&document, &layers[1]).is_empty());
}

#[test]
fn a_nest_whose_opacity_keys_do_not_convert_is_omitted_beside_its_sibling() {
    // The first nest plays Inner 1-4 s over 0-3 s and fades from full at In
    // to 0 one native tick later: both keys fall at 0 ms of its group clock,
    // where no FX track holds them. Without them its group would show its
    // static Opacity after the fade, so import omits that nest as if it
    // were absent, leaving no id, layer or key of its group; the second
    // nest still converts.
    let (mut outer, media) = nested_sequence();
    outer.video_tracks[1].nests[0].animations = vec![PrPropertyAnimation::Opacity(vec![
        linear_key(TICKS, 100.0),
        linear_key(TICKS + 1, 0.0),
    ])];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let [omission] = &omissions[..] else {
        panic!("{omissions:?}");
    };
    assert_eq!(
        (omission.scope, omission.kind, omission.record.as_str()),
        (
            OmissionScope::Occurrence,
            OmissionKind::Omitted,
            "nested sequence \"Inner\""
        )
    );
    let reason = &omission.reason;
    assert!(
        reason.starts_with("nested sequence Opacity keys not converted: ")
            && reason.contains("Premiere keyframe times/values cannot be imported"),
        "{omission:?}"
    );
    let mut without = outer.clone();
    without.video_tracks[1].nests.remove(0);
    assert_eq!(document, import(&without));
}

#[test]
fn a_mixed_rate_nest_keys_its_opacity_and_reports_its_pass_through_on_its_inner_clock() {
    use crate::schema::PrBlendMode;
    // With Inner at 25 fps, the first nest plays it on its inner clock from
    // In 1 s, so its group shows inner 1-4 s. Its Opacity falls from full at
    // source 0.5 s to 60 at 1 s and holds there: below full wherever the
    // group shows Inner's Screen clip.
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.sequence.frame_rate = FrameRate::Fps25;
    nest.sequence.video_tracks[0].clip_mut(0).blend_mode = PrBlendMode::Screen;
    nest.animations = vec![PrPropertyAnimation::Opacity(vec![
        linear_key(TICKS / 2, 100.0),
        linear_key(TICKS, 60.0),
    ])];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    // So no pass-through is reported, and the unmoved group has no frame
    // mask.
    assert!(omissions.is_empty(), "{omissions:?}");
    let group = &document["composition"]["layers"][0];
    assert!(
        group["masks"].as_array().is_none_or(Vec::is_empty),
        "{group}"
    );
    let range = |start: u64, duration: u64| json!({"start": start, "duration": duration});
    assert_eq!(
        group["playback"]["mapping"],
        json!({"type": "linear", "input": range(0, 3000), "output": range(1000, 3000)})
    );
    // The keys count on the source clock, which the inner clock is.
    let key = |time: i64, value: f64| (json!(time), json!(value));
    assert_eq!(
        motion_keys(&document, group),
        [(json!("opacity"), vec![key(500, 100.0), key(1000, 60.0)])]
    );
}

/// Import reports the pass-through of the group that it builds for a nest
/// with the export rule, at the group's Opacity and keys, and export of that
/// group repeats it: FX draws a Normal group without masks or a matte
/// straight into its parent, passing the blend modes of its layers through,
/// at the frames where it is at full Opacity (`tree_renderer::walk`).
#[test]
fn a_nest_group_reports_its_pass_through_at_its_opacity_with_the_export_rule() {
    use crate::schema::PrBlendMode;
    let eased = |seconds: i64, value: f64, easing: PrKeyframeEasing| PrScalarKeyframe {
        source_ticks: seconds * TICKS,
        value,
        easing,
    };
    let key = |seconds: i64, value: f64| eased(seconds, value, PrKeyframeEasing::Linear);
    let ease = |y1: f64, y2: f64| PrKeyframeEasing::CubicBezier {
        x1: 0.33,
        y1,
        x2: 0.67,
        y2,
    };
    let fade = |keys: Vec<PrScalarKeyframe>| vec![PrPropertyAnimation::Opacity(keys)];
    let (full, possible) = (
        Some(super::PASS_THROUGH_APPROXIMATION),
        Some(super::POSSIBLE_PASS_THROUGH_APPROXIMATION),
    );
    // Each row: the second nest's Opacity, its keys and its group's report.
    // The nest plays Inner 0-6 s over 5-11 s, so its keys are at source
    // seconds on the group clock.
    type Row = (
        &'static str,
        f64,
        Vec<PrPropertyAnimation>,
        Option<&'static str>,
    );
    let rows: [Row; 7] = [
        ("full Opacity", 100.0, Vec::new(), full),
        ("half Opacity", 50.0, Vec::new(), None),
        ("Opacity 0", 0.0, Vec::new(), None),
        (
            "keys that reach full Opacity",
            100.0,
            fade(vec![key(1, 100.0), key(3, 40.0)]),
            full,
        ),
        (
            "keys below full Opacity",
            100.0,
            fade(vec![key(0, 90.0), key(2, 40.0)]),
            None,
        ),
        (
            "an ease that passes its keys",
            100.0,
            fade(vec![key(0, 90.0), eased(1, 95.0, ease(0.0, 5.0))]),
            possible,
        ),
        (
            "full Opacity only after the nest ends",
            100.0,
            fade(vec![key(0, 60.0), key(6, 60.0), key(7, 100.0)]),
            None,
        ),
    ];
    let reported = |omissions: &[Omission]| -> Vec<String> {
        omissions
            .iter()
            .filter(|omission| omission.reason.contains("passes the blend modes"))
            .map(|omission| omission.reason.clone())
            .collect()
    };
    for (row, opacity, animations, report) in rows {
        let (mut outer, media) = nested_sequence();
        let nest = &mut outer.video_tracks[1].nests[1];
        (nest.opacity, nest.animations) = (opacity, animations);
        nest.sequence.video_tracks[0].clip_mut(0).blend_mode = PrBlendMode::Screen;
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(
            &outer,
            &media,
            &asset_ids_in_order(&outer, &media),
            &mut omissions,
        )
        .unwrap();
        let expected: Vec<String> = report.map(str::to_owned).into_iter().collect();
        assert_eq!(reported(&omissions), expected, "{row}: {omissions:?}");
        // Export writes the group back as a nest and repeats the report.
        let (_, exported) = export(document.to_json_value().unwrap());
        assert!(
            !exported
                .iter()
                .any(|omission| omission.reason.starts_with("group was not exported")),
            "{row}: {exported:?}"
        );
        assert_eq!(reported(&exported), expected, "{row}, exported");
    }
}

/// Each nest of `sequence`: its static Opacity and its Opacity keys as
/// (source ticks, value).
fn nest_opacities(sequence: &PrSequence) -> Vec<(f64, Vec<(i64, f64)>)> {
    let mut nests = Vec::new();
    for nest in sequence.nest_occurrences() {
        let mut keys = Vec::new();
        for animation in &nest.animations {
            let PrPropertyAnimation::Opacity(opacity) = animation else {
                panic!("{animation:?}");
            };
            keys.extend(opacity.iter().map(|key| (key.source_ticks, key.value)));
        }
        nests.push((nest.opacity, keys));
    }
    nests
}

#[test]
fn an_edited_group_opacity_exports_on_its_nest_and_reads_back() {
    // The plain groups of Inner, edited: the first (0-3 s) to Opacity 0, the
    // second (5-11 s) to fade by Linear keys from full at 1 s to 40 at 3 s of
    // its clock.
    let (outer, _) = nested_sequence();
    let mut document = import(&outer);
    let layers = &mut document["composition"]["layers"];
    layers[0]["transform"]["opacity"] = json!(0.0);
    let faded = layers[1]["id"].as_u64().unwrap();
    let linear = json!({"type": "linear"});
    let keys = [(1000, 100.0, &linear), (3000, 40.0, &linear)];
    document["composition"]["dynamics"]["entries"] = json!([track(faded, "opacity", &keys)]);
    let (mut project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    // Each placement carries its group's Opacity and keys, counted from its
    // start.
    let expected = [
        (0.0, Vec::new()),
        (100.0, vec![(ms(1000), 100.0), (ms(3000), 40.0)]),
    ];
    assert_eq!(nest_opacities(project.single_sequence().unwrap()), expected);
    // The written project reads both back, and they import as their groups.
    for (id, media) in &mut project.media {
        let name = format!("{}.mp4", id.as_str());
        media.relative_path = Some(format!("./media/{name}"));
        media.relative_paths = vec![format!("./media/{name}")];
        media.absolute_paths = vec![(MediaPathField::FilePath, format!("/media/{name}").into())];
        media.name = name;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reloaded, omissions) = PrProjectFile::load(&path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = reloaded.single_sequence().unwrap();
    assert_eq!(nest_opacities(sequence), expected);
    let mut omissions = Vec::new();
    let again = premiere_to_tesseract(
        sequence,
        &reloaded.media,
        &asset_ids_in_order(sequence, &reloaded.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let layers = &again["composition"]["layers"];
    let opacity = |index: usize| layers[index]["transform"]["opacity"].clone();
    assert_eq!((opacity(0), opacity(1)), (json!(0.0), json!(100.0)));
    let key = |time: i64, value: f64| (json!(time), json!(value));
    assert_eq!(
        motion_keys(&again, &layers[1]),
        [(json!("opacity"), vec![key(1000, 100.0), key(3000, 40.0)])]
    );
}

#[test]
fn a_group_exports_its_crop_or_keyed_wipe_and_its_effects_on_its_placement() {
    let (outer, _) = nested_sequence();
    let mut document = import(&outer);
    let identity = document["composition"]["layers"][0]["transform"].clone();
    let group = &mut document["composition"]["layers"][0];
    // A Crop guide in the group's frame: 10% off each edge of the canvas.
    group["masks"] = json!([{"id": 30, "mode": "add", "layer": 20}]);
    group["layers"].as_array_mut().unwrap().push(json!({
        "type": "Rect", "id": 20, "parent": 2, "name": "Crop guide",
        "activeRange": {"start": 0, "duration": 3000}, "transform": identity,
        "rect": {"size": [1536.0, 864.0], "position": [192.0, 108.0], "fillColor": [0, 0, 0, 1]}
    }));
    group["effects"] = json!([
        {"id": 11, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10.0}},
        {"id": 12, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 5.0}},
        {"id": 13, "enabled": false, "effect": {"type": "gaussianBlur", "blurriness": 30.0}},
        {"id": 14, "enabled": true, "effect": {"type": "vignette", "amount": 0.5}},
        {"id": 15, "enabled": true, "effect": {"type": "mosaic", "horizontalBlocks": 16.0, "verticalBlocks": 9.0, "sharpColors": true}}
    ]);
    let key = |id: &str, layer_time: i64, value: f64| json!({"id": id, "layerTime": layer_time, "value": {"type": "float", "value": value}, "easing": {"type": "linear"}});
    document["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 12, "paramName": "blurriness"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [key("a", 0, 5.0), key("b", 500, 20.0)]},
    }]});
    let (project, omissions) = export(document);
    // Only the Vignette (no mapping) and the Mosaic (no nest host) are
    // reported; the keyed blur exports with its keys.
    assert_eq!(
        omissions,
        [
            Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 2 (\"Inner\")".into(),
                reason: "effects: vignette effect 14 was not exported: it has no Premiere effect mapping".into(),
            },
            Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 2 (\"Inner\")".into(),
                reason: "effects: mosaic effect 15 was not exported: a Mosaic on a stage group or a nested sequence is not converted; no Adobe case verifies the frame over which the FX mosaic lays its grid there".into(),
            },
        ]
    );
    let nest = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(
        nest.crop,
        PrStaticCrop {
            left: 10.0,
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            edge_feather: 0.0
        }
    );
    let mut keyed = exported_blur(true, 5.0, false);
    keyed.animations = vec![crate::schema::PrEffectParamAnimation {
        param: &crate::schema::FILM_IMPACT_BLUR_AMOUNT,
        keys: crate::schema::PrEffectParamKeys::Scalar(
            [(0, 5.0), (500, 20.0)]
                .map(|(millis, value)| PrScalarKeyframe {
                    source_ticks: ms(millis),
                    value: amount(value),
                    easing: PrKeyframeEasing::Linear,
                })
                .into(),
        ),
    }];
    let bypassed = exported_blur(false, 30.0, false);
    assert_eq!(
        nest.effects,
        [exported_blur(true, 10.0, false), keyed, bypassed]
    );
    // The guide paints nothing in the nested sequence.
    assert_eq!(nest.sequence.video_items().count(), 1);

    // The first inner clip's keyed flat wipe (Completion 0 at 1 s to 60 at
    // 2 s from source In 1 s) moves onto its group.
    let (mut outer, _) = nested_sequence();
    let completion =
        [(TICKS, 0.0), (2 * TICKS, 60.0)].map(|(source_ticks, value)| PrScalarKeyframe {
            source_ticks,
            value,
            easing: PrKeyframeEasing::Linear,
        });
    outer.video_tracks[1].nests[0].sequence.video_tracks[0]
        .clip_mut(0)
        .linear_wipe = Some(PrLinearWipe {
        initial_completion: 0.0,
        completion: completion.into(),
        angle_degrees: 90,
        feather: 0.0,
    });
    let mut document = import(&outer);
    let group = &mut document["composition"]["layers"][0];
    group["masks"] = group["layers"][0]["masks"].take();
    group["layers"][0].as_object_mut().unwrap().remove("masks");
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let nest = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    let wipe = nest.linear_wipe.as_ref().unwrap();
    assert_eq!((wipe.angle_degrees, wipe.initial_completion), (90, 0.0));
    assert_eq!(
        wipe.completion
            .iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        [(0, 0.0), (TICKS, 60.0)]
    );
    assert!(nest
        .sequence
        .video_occurrences()
        .all(|clip| clip.linear_wipe.is_none()));
}

#[test]
fn a_directional_blur_that_a_nest_placement_would_map_is_omitted() {
    // No Adobe case verifies a Directional Blur's map through a placement's
    // Motion: only the blur is omitted, on the placement or on a clip inside a
    // nest whose placement has Motion, static or keyed. Inside a plain nest,
    // or one with only Opacity keys, it keeps its clip.
    let (outer, _) = nested_sequence();
    let imported = import(&outer);
    let directional = json!([{"id": 40, "enabled": true, "effect": {"type": "directionalBlur", "direction": 30.0, "blurLength": 15.0}}]);
    let omitted = |record: &str| {
        Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Omitted,
        record: record.into(),
        reason: "effects: directionalBlur effect 40 was not exported: a Directional Blur on a nested sequence, or inside one whose placement has Motion, is not converted; no Adobe case verifies its map through the nest's Motion".into(),
    }
    };
    let video = &imported["composition"]["layers"][0]["layers"][0];
    let video_record = format!("layer {} ({})", video["id"], video["name"]);
    // Linear keys of the group over 0-2 s from its neutral static value.
    let linear = json!({"type": "linear"});
    let keys = |property, [start, end]: [f64; 2]| {
        track(2, property, &[(0, start, &linear), (2000, end, &linear)])
    };
    for (on_group, rotation, entries, expected) in [
        (true, 0.0, vec![], vec![omitted("layer 2 (\"Inner\")")]),
        (false, 10.0, vec![], vec![omitted(&video_record)]),
        (false, 0.0, vec![], vec![]),
        (
            false,
            0.0,
            vec![keys("rotation", [0.0, 30.0])],
            vec![omitted(&video_record)],
        ),
        (
            false,
            0.0,
            vec![keys("scaleX", [100.0, 50.0]), keys("scaleY", [100.0, 50.0])],
            vec![omitted(&video_record)],
        ),
        (
            false,
            0.0,
            vec![
                keys("positionX", [960.0, 1060.0]),
                keys("positionY", [540.0, 540.0]),
            ],
            vec![omitted(&video_record)],
        ),
        (false, 0.0, vec![keys("opacity", [100.0, 40.0])], vec![]),
    ] {
        let mut document = imported.clone();
        let group = &mut document["composition"]["layers"][0];
        // Centred, the neutral statics write Premiere's default Motion.
        group["transform"]["anchorPoint"] = json!([960.0, 540.0]);
        group["transform"]["position"] = json!([960.0, 540.0]);
        group["transform"]["rotation"] = json!(rotation);
        if on_group {
            group["effects"] = directional.clone();
        } else {
            group["layers"][0]["effects"] = directional.clone();
        }
        let keyed = !entries.is_empty();
        document["composition"]["dynamics"]["entries"] = json!(entries);
        let (project, omissions) = export(document);
        assert_eq!(omissions, expected);
        let nest = project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .next()
            .unwrap();
        // The placement keeps its keys, as one animation.
        assert_eq!(nest.animations.len(), usize::from(keyed));
        let blurs = nest.effects.len()
            + nest
                .sequence
                .video_occurrences()
                .next()
                .unwrap()
                .effects
                .len();
        assert_eq!(blurs, usize::from(expected.is_empty()), "{expected:?}");
    }
}

/// A retained two-owner stage with neutral Motion and a rotating inner picture.
fn nested_transform_document() -> Value {
    use crate::tests::support::{transform_effect, DEFAULT_PR_TRANSFORM};

    let (mut outer, media) = nested_sequence();
    outer.video_tracks[1].nests.truncate(1);
    outer.timeline_end_ticks = outer.occurrence_end_ticks();
    outer.video_tracks[1].nests[0].effects = vec![transform_effect(
        crate::schema::PrTransform {
            rotation: 30.0,
            ..DEFAULT_PR_TRANSFORM
        },
        Vec::new(),
    )];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap();
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert!(omissions[0]
        .reason
        .contains("nested Transform is approximated"));
    document.to_json_value().unwrap()
}

/// Supplementary export controls use the saved static canvas-probe values.
/// The ignored native-source regression below exercises the actual reader.
fn differing_canvas_transform_document(frame: [u32; 2], canvas: [u32; 2]) -> Value {
    let mut document = nested_transform_document();
    document["dimensions"]["width"] = json!(canvas[0]);
    document["dimensions"]["height"] = json!(canvas[1]);
    let group = &mut document["composition"]["layers"][0];
    group["transform"]["anchorPoint"] =
        json!([0.4 * f64::from(frame[0]), 0.45 * f64::from(frame[1])]);
    group["transform"]["position"] =
        json!([0.4 * f64::from(canvas[0]), 0.5 * f64::from(canvas[1])]);
    group["transform"]["scale"] = json!([50.0, 50.0]);
    let picture = &mut group["layers"][0];
    picture["transform"]["anchorPoint"] =
        json!([0.2 * f64::from(frame[0]), 0.25 * f64::from(frame[1])]);
    picture["transform"]["position"] =
        json!([0.35 * f64::from(frame[0]), 0.35 * f64::from(frame[1])]);
    picture["transform"]["scale"] = json!([120.0, 120.0]);
    picture["transform"]["rotation"] = json!(0.0);
    let guide_id = picture["masks"][0]["layer"].clone();
    let guide = picture["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["id"] == guide_id)
        .unwrap();
    guide["rect"]["size"] = json!(frame);
    document
}

#[test]
fn group_motion_blur_preserves_flattened_nested_transform_owners() {
    let mut document = nested_transform_document();
    let (baseline, _) = export(document.clone());
    let group = &mut document["composition"]["layers"][0];
    group["motionBlur"] = json!(true);
    group["layers"][0]["motionBlur"] = json!(true);
    let owners = [
        group["id"].as_u64().unwrap(),
        group["layers"][0]["id"].as_u64().unwrap(),
    ];
    let (project, reports) = export(document);
    let native = project.single_sequence().unwrap();
    let before = baseline.single_sequence().unwrap();
    assert_eq!(nests(native), nests(before));
    let nest = native.nest_occurrences().next().unwrap();
    let expected = before.nest_occurrences().next().unwrap();
    assert_eq!(nest.effects, expected.effects);
    assert_eq!(nest.transform, expected.transform);
    assert_eq!(nest.animations, expected.animations);
    for id in owners {
        assert!(
            reports
                .iter()
                .any(|report| report.scope == OmissionScope::Feature
                    && report.record.starts_with(&format!("layer {id} ("))
                    && report.reason.contains("group motion blur was not exported")),
            "{reports:?}"
        );
    }
}

#[test]
fn group_motion_blur_is_not_reported_from_an_omitted_enclosing_nest() {
    let mut document = crate::test_support::editable_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    let mut video = layers.remove(0);
    video["sourceIntrinsicDuration"] = json!(10000);
    video["volume"] = json!(0);
    let mut healthy = video.clone();
    healthy["id"] = json!(30);
    let transform = layers[0]["transform"].clone();
    video["parent"] = json!(10);
    video["sourceRange"] = json!({"start": 0, "duration": 500});
    video["playback"] = linear_playback(
        json!({"start": 0, "duration": 500}),
        video["sourceRange"].clone(),
    );
    let inner_range = json!({"start": 0, "duration": 500});
    let outer_range = json!({"start": 0, "duration": 1000});
    let mut moved = transform.clone();
    moved["position"] = json!([100.0, 0.0]);
    layers.insert(
        0,
        json!({
            "type": "Group", "id": 20, "name": "Rejected enclosing group", "blendMode": "normal",
            "playback": linear_playback(outer_range.clone(), outer_range), "transform": moved,
            "layers": [{"type": "Group", "id": 10, "parent": 20, "name": "Unretained child",
                "blendMode": "normal", "motionBlur": true, "transform": transform,
                "playback": linear_playback(inner_range.clone(), inner_range), "layers": [video]}]
        }),
    );
    layers.insert(1, healthy);
    let (project, reports) = export(document);
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 0);
    assert_eq!(sequence.video_occurrences().count(), 1);
    assert!(
        reports
            .iter()
            .any(|report| report.record.starts_with("layer 20 (")
                && report
                    .reason
                    .contains("group extends past its exported children's end")),
        "{reports:?}"
    );
    assert!(
        !reports
            .iter()
            .any(|report| report.reason.contains("group motion blur was not exported")),
        "{reports:?}"
    );
}

#[test]
fn group_motion_blur_reports_retained_direct_timed_images() {
    let mut document = crate::test_support::editable_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers.remove(0);
    let transform = layers[0]["transform"].clone();
    let images: Vec<_> = [0, 500]
        .into_iter()
        .enumerate()
        .map(|(index, start)| {
            json!({
                "type": "Image", "id": 11 + index, "parent": 10, "name": "Timed still",
                "activeRange": {"start": start, "duration": 500}, "transform": transform,
                "source": {"assetId": "premiere-still-1", "fit": "contain",
                    "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}}
            })
        })
        .collect();
    let range = json!({"start": 0, "duration": 1000});
    layers.insert(
        0,
        json!({
            "type": "Group", "id": 10, "name": "Timed stills", "blendMode": "normal",
            "motionBlur": true, "isHidden": true, "transform": transform,
            "playback": linear_playback(range.clone(), range), "layers": images
        }),
    );
    let (project, reports) = export(document);
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.nest_occurrences().count(), 0);
    let clips: Vec<_> = sequence.video_occurrences().collect();
    assert_eq!(clips.len(), 2);
    assert!(clips.iter().all(|clip| !clip.enabled));
    assert_eq!(clips[0].timeline_ticks(), 0..ms(500));
    assert_eq!(clips[1].timeline_ticks(), ms(500)..ms(1000));
    assert_eq!(
        reports
            .iter()
            .filter(|report| report.scope == OmissionScope::Feature
                && report.record.starts_with("layer 10 (")
                && report.reason.contains("group motion blur was not exported"))
            .count(),
        1,
        "{reports:?}"
    );
}

#[test]
fn group_motion_blur_keeps_crop_stage_as_one_supported_clip() {
    let (mut outer, _) = nested_sequence();
    outer.video_tracks[1].nests[0].sequence.video_tracks[0]
        .clip_mut(0)
        .crop
        .left = 10.0;
    let mut document = import(&outer);
    // Explicitly move the imported Crop onto its existing Group, with the
    // same guide/neutral child. This is the supported one-clip mask stage.
    let group = &mut document["composition"]["layers"][0];
    group["masks"] = group["layers"][0]["masks"].take();
    group["layers"][0].as_object_mut().unwrap().remove("masks");
    // Matching centered Anchor/Position were also identity, but a Group Crop
    // guide uses the exporter's canonical identity fields, not video Motion.
    for child in group["layers"].as_array_mut().unwrap() {
        child["transform"] =
            serde_json::to_value(super::super::background::identity_transform()).unwrap();
    }
    let id = group["id"].as_u64().unwrap();
    let (baseline, before_reports) = export(document.clone());
    assert!(
        baseline
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .any(|clip| clip.crop.left == 10.0),
        "supported Crop-stage control was not retained: {before_reports:?}"
    );
    document["composition"]["layers"][0]["motionBlur"] = json!(true);
    let (project, reports) = export(document);
    let sequence = project.single_sequence().unwrap();
    assert_eq!(nests(sequence), nests(baseline.single_sequence().unwrap()));
    let child = sequence
        .video_occurrences()
        .find(|clip| clip.timeline_ticks() == (0..3 * TICKS))
        .unwrap();
    let expected = baseline
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .find(|clip| clip.timeline_ticks() == (0..3 * TICKS))
        .unwrap();
    assert_eq!(child.crop.left, 10.0);
    assert_eq!(child.timeline_ticks(), expected.timeline_ticks());
    assert_eq!(child.source_ticks(), expected.source_ticks());
    assert_eq!(child.transform, expected.transform);
    assert!(
        reports
            .iter()
            .any(|report| report.scope == OmissionScope::Feature
                && report.record.starts_with(&format!("layer {id} ("))
                && report.reason.contains("group motion blur was not exported")),
        "{reports:?}"
    );
}

#[test]
fn differing_canvas_transform_export_preserves_source_frame_and_current_edits() {
    for (frame, canvas) in [([1920, 1080], [3840, 2160]), ([3840, 2160], [1920, 1080])] {
        let mut document = differing_canvas_transform_document(frame, canvas);
        let group = &mut document["composition"]["layers"][0];
        // Pixel-valued edits, not hidden native normalized controls.
        group["transform"]["position"] =
            json!([0.3 * f64::from(canvas[0]), 0.6 * f64::from(canvas[1])]);
        let picture = &mut group["layers"][0];
        picture["transform"]["position"] =
            json!([0.4 * f64::from(frame[0]), 0.3 * f64::from(frame[1])]);
        let guide_id = picture["masks"][0]["layer"].clone();
        picture["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == guide_id)
            .unwrap()["name"] = json!("User-renamed source boundary");
        let (native, omissions) = export(document);
        assert!(omissions.is_empty(), "{frame:?}: {omissions:?}");
        let outer = native.single_sequence().unwrap();
        let nest = outer
            .nest_occurrences()
            .next()
            .expect("source-canvas nest retained");
        assert_eq!(outer.dimensions(), canvas);
        assert_eq!(nest.sequence.dimensions(), frame);
        assert_eq!(
            nest.sequence.nest_occurrences().count(),
            0,
            "no extra rasterizing nest"
        );
        assert_eq!(nest.transform.anchor_point, [0.4, 0.45]);
        assert_eq!(nest.transform.position, [0.3, 0.6]);
        assert_eq!(nest.transform.scale, [50.0; 2]);
        assert!(nest.crop.is_default());
        let [effect] = nest.effects.as_slice() else {
            panic!("one editable Transform: {nest:?}")
        };
        let PrEffectParams::Transform(transform) = &effect.params else {
            panic!("affine effect")
        };
        assert_eq!(transform.anchor_point, [0.2, 0.25]);
        assert_eq!(transform.position, [0.4, 0.3]);
        assert_eq!(transform.scale(), [120.0; 2]);
        if frame == [3840, 2160] {
            let child = nest.sequence.video_occurrences().next().unwrap();
            // The 1920x1080 child stays at its authored pixel position inside
            // the larger source canvas, not the smaller placement canvas.
            assert_eq!(child.transform.position, [0.25; 2]);
            assert_eq!(child.transform.anchor_point, [0.5; 2]);
            assert_eq!(child.transform.scale, [100.0; 2]);
            assert_eq!(child.transform.rotation, 0.0);
            assert!(child.animations.is_empty());
        }
        let xml = write_nested_effect_project(native);
        let (reloaded, notes) = load_nested_effect_project(&xml);
        assert!(notes.is_empty(), "{notes:?}");
        let outer = reloaded.single_sequence().unwrap();
        assert_eq!(
            outer
                .nest_occurrences()
                .next()
                .unwrap()
                .sequence
                .dimensions(),
            frame
        );
        // Exercise converter import in ordinary CI, not only native reader
        // admission or the ignored external Geometry2-source regression.
        let mut notes = Vec::new();
        let imported = premiere_to_tesseract(
            outer,
            &reloaded.media,
            &asset_ids_in_order(outer, &reloaded.media),
            &mut notes,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        assert!(
            notes
                .iter()
                .all(|note| note.kind == OmissionKind::Approximated),
            "{notes:?}"
        );
        let motion = imported["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        assert_eq!(
            motion["transform"]["anchorPoint"],
            json!([0.4 * f64::from(frame[0]), 0.45 * f64::from(frame[1])])
        );
        assert_eq!(
            motion["transform"]["position"],
            json!([0.3 * f64::from(canvas[0]), 0.6 * f64::from(canvas[1])])
        );
        let picture = &motion["layers"][0];
        assert_eq!(
            picture["transform"]["anchorPoint"],
            json!([0.2 * f64::from(frame[0]), 0.25 * f64::from(frame[1])])
        );
        assert_eq!(
            picture["transform"]["position"],
            json!([0.4 * f64::from(frame[0]), 0.3 * f64::from(frame[1])])
        );
        let guide_id = picture["masks"][0]["layer"].clone();
        let guide = picture["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == guide_id)
            .unwrap();
        assert_eq!(guide["rect"]["size"], json!(frame.map(f64::from)));
        let mut document = differing_canvas_transform_document(frame, canvas);
        let group = &mut document["composition"]["layers"][0];
        group["transform"]["position"] =
            json!([0.4 * f64::from(canvas[0]), 0.45 * f64::from(canvas[1])]);
        group["transform"]["scale"] = json!([100.0, 100.0]);
        let (native, notes) = export(document);
        assert!(notes.is_empty(), "{notes:?}");
        let nest = native
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .next()
            .unwrap();
        // Equal normalized points are not identity when their pixel bases differ.
        assert_eq!(nest.transform.position, [0.4, 0.45]);
        assert_eq!(nest.transform.anchor_point, [0.4, 0.45]);
        assert_ne!(nest.transform, PrStaticTransform::default());
    }
}

#[test]
fn differing_canvas_transform_refuses_unmeasured_edits_without_losing_sibling() {
    for (pointer, value, reason) in [
        ("/dimensions/height", json!(3840), "16:9"),
        (
            "/composition/layers/0/transform/rotation",
            json!(15.0),
            "without rotation or skew",
        ),
        (
            "/composition/layers/0/transform/scale",
            json!([50.0, 75.0]),
            "positive uniform scale",
        ),
        (
            "/composition/layers/0/transform/opacity",
            json!(75.0),
            "full Motion and Transform Opacity",
        ),
        (
            "/composition/layers/0/layers/0/transform/rotation",
            json!(15.0),
            "without rotation or skew",
        ),
        (
            "/composition/layers/0/layers/0/transform/opacity",
            json!(75.0),
            "full Motion and Transform Opacity",
        ),
    ] {
        let mut document = differing_canvas_transform_document([3840, 2160], [1920, 1080]);
        *document.pointer_mut(pointer).unwrap() = value;
        let (native, notes) = export(document);
        let outer = native.single_sequence().unwrap();
        assert_eq!(outer.nest_occurrences().count(), 0, "{pointer}: {notes:?}");
        assert_eq!(outer.video_occurrences().count(), 1, "healthy sibling");
        assert!(
            notes.iter().any(|note| note.reason.contains(reason)),
            "{pointer}: {notes:?}"
        );
    }
    for owner in [0, 1] {
        let mut document = differing_canvas_transform_document([3840, 2160], [1920, 1080]);
        let group = &document["composition"]["layers"][0];
        let id = if owner == 0 {
            group["id"].as_u64().unwrap()
        } else {
            group["layers"][0]["id"].as_u64().unwrap()
        };
        document["composition"]["dynamics"]["entries"] = json!([track(
            id,
            "rotation",
            &[
                (0, 0.0, &json!({"type": "linear"})),
                (2000, 10.0, &json!({"type": "linear"}))
            ]
        )]);
        let (native, notes) = export(document);
        assert_eq!(
            native.single_sequence().unwrap().nest_occurrences().count(),
            0
        );
        assert!(
            notes
                .iter()
                .any(|note| note.reason.contains("requires static")),
            "{notes:?}"
        );
    }
    for size in [json!([3840.5, 2160.0]), json!([0.0, 2160.0])] {
        let mut document = differing_canvas_transform_document([3840, 2160], [1920, 1080]);
        let picture = &mut document["composition"]["layers"][0]["layers"][0];
        let guide_id = picture["masks"][0]["layer"].clone();
        let guide = picture["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == guide_id)
            .unwrap();
        guide["rect"]["size"] = size;
        let (native, notes) = export(document);
        assert_eq!(
            native.single_sequence().unwrap().nest_occurrences().count(),
            0
        );
        assert!(
            notes
                .iter()
                .any(|note| note.reason.contains("whole-source-canvas guide")),
            "{notes:?}"
        );
    }
}

#[test]
fn nested_transform_exports_nonpainting_and_legacy_crop_frame_guides() {
    for fill_enabled in [false, true] {
        let mut document = nested_transform_document();
        let picture = &mut document["composition"]["layers"][0]["layers"][0];
        let guide_id = picture["masks"][0]["layer"].clone();
        let guide = picture["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| layer["id"] == guide_id)
            .unwrap();
        assert_eq!(guide["rect"]["fillEnabled"], false);
        guide["rect"]["fillEnabled"] = json!(fill_enabled);
        let (project, omissions) = export(document);
        assert!(omissions.is_empty(), "{omissions:?}");
        let outer = project.single_sequence().unwrap();
        assert_eq!(outer.video_occurrences().count(), 1, "healthy sibling");
        assert_eq!(outer.nest_occurrences().count(), 1);
        let nest = outer.nest_occurrences().next().unwrap();
        assert_eq!(nest.effects.len(), 1, "editable Transform retained");
        assert_eq!(nest.sequence.video_occurrences().count(), 1);
    }
}

#[test]
fn nested_transform_omits_descendant_directional_blur_with_neutral_motion() {
    let mut document = nested_transform_document();
    assert!(document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    let group = &mut document["composition"]["layers"][0];
    group["transform"]["anchorPoint"] = json!([960.0, 540.0]);
    group["transform"]["position"] = json!([960.0, 540.0]);
    assert_eq!(group["transform"]["rotation"], json!(0.0));
    assert_eq!(group["layers"][0]["transform"]["rotation"], json!(30.0));
    let video = &mut group["layers"][0]["layers"][0];
    assert_eq!(video["type"], "Video");
    let record = format!("layer {} ({})", video["id"], video["name"]);
    video["effects"] = json!([{"id": 40, "enabled": true, "effect": {
        "type": "directionalBlur", "direction": 30.0, "blurLength": 15.0
    }}]);
    let (project, omissions) = export(document);
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.video_occurrences().count(), 1, "healthy sibling");
    assert_eq!(outer.nest_occurrences().count(), 1);
    let nest = outer.nest_occurrences().next().unwrap();
    assert_eq!(nest.transform, PrStaticTransform::default());
    assert!(nest.animations.is_empty());
    assert_eq!(nest.effects.len(), 1, "inner Transform remains editable");
    assert_eq!(nest.sequence.video_occurrences().count(), 1);
    let video = nest.sequence.video_occurrences().next().unwrap();
    assert!(video.effects.is_empty(), "{:#?}", video.effects);
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record,
            reason: "effects: directionalBlur effect 40 was not exported: a Directional Blur on a nested sequence, or inside one whose placement has Motion, is not converted; no Adobe case verifies its map through the nest's Motion".into(),
        }]
    );
}

#[test]
fn nested_transform_without_picture_drops_standalone_audio_media() {
    use crate::audio_media::{inspect_audio_media, SourceSound};

    let bytes = include_bytes!("../../../tests/fixtures/nest_tone_stereo_8s.wav");
    let sound = inspect_audio_media(
        std::io::Cursor::new(bytes.as_slice()),
        bytes.len() as u64,
        "wav",
    )
    .unwrap()
    .unwrap();
    let audio = BTreeMap::from([("stage-audio".into(), SourceSound::Supported(sound))]);
    let mut document = nested_transform_document();
    let stage = &mut document["composition"]["layers"][0];
    let record = format!("layer {} ({})", stage["id"], stage["name"]);
    let picture = &mut stage["layers"][0];
    let sound = json!({
        "type": "Audio", "id": 40, "parent": picture["id"], "name": "Stage sound",
        "playback": picture["playback"], "sourceRange": {"start": 0, "duration": 3000},
        "sourceIntrinsicDuration": 8000, "volume": 1.0,
        "source": {"assetId": "stage-audio"}
    });
    picture["layers"].as_array_mut().unwrap().push(sound);
    // The same inspected audio really lowers when the stage still has picture.
    let (control, omissions) = export_with_audio(document.clone(), &audio).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let nest = control
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(nest.sequence.audio.len(), 1);
    let audio_id = nest.sequence.audio[0].media.clone();
    assert!(control.media[&audio_id].audio.is_some());

    // Retain the exact guide and audio, deleting only the stage's source video.
    document["composition"]["layers"][0]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["type"] != "Video");
    let (project, omissions) = export_with_audio(document, &audio).unwrap();
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record,
            reason: "nested Transform stage has no exportable source picture".into(),
        }]
    );
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.nest_occurrences().count(), 0, "no fallback nest");
    assert_eq!(outer.video_occurrences().count(), 1, "healthy sibling");
    assert!(outer.audio.is_empty());
    assert!(!project.media.contains_key(&audio_id), "orphan audio media");
    assert_eq!(project.media.len(), 1, "only the sibling's source survives");
}

#[test]
fn hidden_groups_and_stages_inside_an_edited_group_export_disabled() {
    // Mid places a staged clip (a blur before a Crop) and, above it, a nest
    // of Leaf; Outer places Mid over its red clip.
    let leaf = sequence_of(
        "Leaf",
        vec![PrVideoTrack::media([clip_of("timecoded", 0..TICKS, 0)])],
    );
    let mut staged = clip_of("timecoded", 0..TICKS, 0);
    staged.crop.top = 15.0;
    staged.effects = vec![blur(40.0)];
    staged.effects_above_mask = 1;
    let nests = |sequence, range| PrVideoTrack {
        transitions: Vec::new(),
        items: Vec::new(),
        nests: vec![nest_of(sequence, range, 0)],
    };
    let mid = sequence_of(
        "Mid",
        vec![PrVideoTrack::media([staged]), nests(leaf, 0..TICKS)],
    );
    let outer = sequence_of(
        "Outer",
        vec![
            PrVideoTrack::media([clip_of("red", 0..TICKS, 0)]),
            nests(mid, 0..TICKS),
        ],
    );
    let mut document = import(&outer);
    let mid = &mut document["composition"]["layers"][0];
    mid["transform"]["opacity"] = json!(60.0);
    for inner in mid["layers"].as_array_mut().unwrap() {
        inner["isHidden"] = json!(true);
    }
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let mid = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!((mid.opacity, mid.enabled), (60.0, true));
    let leaf = mid.sequence.nest_occurrences().next().unwrap();
    assert!(!leaf.enabled);
    // The hidden stage group stays one disabled clip.
    let clip = mid.sequence.video_occurrences().next().unwrap();
    assert_eq!(
        (
            clip.enabled,
            clip.crop.top,
            clip.effects.as_slice(),
            clip.effects_above_mask
        ),
        (
            false,
            15.0,
            [exported_blur(true, 40.0, false)].as_slice(),
            1
        )
    );
}

#[test]
fn exported_nests_write_one_sequence_per_group_and_read_back() {
    let (outer, _) = nested_sequence();
    let (mut project, _) = export(import(&outer));
    for (id, media) in &mut project.media {
        let name = format!("{}.mp4", id.as_str());
        media.relative_path = Some(format!("./media/{name}"));
        media.relative_paths = vec![format!("./media/{name}")];
        media.absolute_paths = vec![(MediaPathField::FilePath, format!("/media/{name}").into())];
        media.name = name;
    }
    let exported = nests(project.single_sequence().unwrap());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.prproj");
    PremiereProjectXml::new(&project)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let xml = crate::format::read_xml(&path).unwrap();
    // Outer plus one inner sequence per group, all listed in the root bin.
    assert_eq!(xml.matches("<Sequence ObjectUID=").count(), 3);
    assert_eq!(xml.matches("<VideoSequenceSource ObjectID=").count(), 3);
    let root_bin = roxmltree::Document::parse(&xml).unwrap();
    let items = root_bin
        .descendants()
        .find(|node| node.has_tag_name("RootProjectItem") && node.has_attribute("ObjectUID"))
        .unwrap()
        .descendants()
        .filter(|node| node.has_tag_name("Item"))
        .count();
    assert_eq!(items, 5);

    let (reloaded, omissions) = PrProjectFile::load(&path).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequences: Vec<_> = reloaded.sequences().collect();
    assert_eq!(sequences.len(), 1, "inner sequences are not top-level");
    assert_eq!(sequences[0].name(), "Outer");
    assert_eq!(nests(sequences[0]), exported);
    let names: BTreeMap<_, _> = sequences[0]
        .nest_occurrences()
        .flat_map(|nest| nest.sequence.video_occurrences())
        .map(|clip| (reloaded.media(clip).unwrap().name(), ()))
        .collect();
    assert_eq!(
        names.into_keys().collect::<Vec<_>>(),
        ["premiere-video-2.mp4"]
    );
    assert_eq!(reloaded.media.len(), 2);
}

#[test]
fn a_group_nested_below_eight_groups_is_omitted() {
    // Level 0 holds a clip and nests level 1 on another track, down to level 9.
    let mut sequence = sequence_of(
        "Level 9",
        vec![PrVideoTrack::media([clip_of("timecoded", 0..TICKS, 0)])],
    );
    for level in (0..9).rev() {
        sequence = sequence_of(
            &format!("Level {level}"),
            vec![
                PrVideoTrack::media([clip_of("timecoded", 0..TICKS, 0)]),
                PrVideoTrack {
                    transitions: Vec::new(),
                    items: Vec::new(),
                    nests: vec![nest_of(sequence, 0..TICKS, 0)],
                },
            ],
        );
    }
    // An edit of the deepest group does not change why it is omitted.
    let mut document = import(&sequence);
    let mut group = &mut document["composition"]["layers"][0];
    for _ in 0..8 {
        group = &mut group["layers"][0];
    }
    assert_eq!(group["name"], "Level 9");
    group["transform"]["opacity"] = json!(50.0);
    let (project, omissions) = export(document);
    assert!(
        omissions.iter().any(|item| item.record.ends_with("(\"Level 9\")")
            && item.reason
                == "group was not exported as a nested sequence: nesting deeper than 8 levels is not supported"),
        "{omissions:?}"
    );
    let mut depth = 0;
    let mut level = project.single_sequence().unwrap();
    while let Some(nest) = level.nest_occurrences().next() {
        (depth, level) = (depth + 1, &nest.sequence);
    }
    assert_eq!((depth, level.name()), (8, "Level 8"));
}

#[test]
fn a_group_without_exportable_video_is_omitted() {
    // The first group's only child is its own Crop guide, which paints nothing.
    let (outer, _) = nested_sequence();
    let mut document = import(&outer);
    let layers = &mut document["composition"]["layers"];
    let mut rect = layers[3].clone();
    rect["id"] = layers[0]["layers"][0]["id"].clone();
    rect["parent"] = layers[0]["id"].clone();
    rect["transform"] = layers[0]["transform"].clone();
    rect["activeRange"] = json!({"start": 0, "duration": 3000});
    layers[0]["masks"] = json!([{"id": 30, "mode": "add", "layer": rect["id"]}]);
    layers[0]["layers"] = json!([rect]);
    let group = format!("layer {} (\"Inner\")", layers[0]["id"]);
    let (project, omissions) = export(document);
    let reasons: Vec<_> = omissions.iter().map(|item| item.reason.as_str()).collect();
    assert_eq!(
        reasons,
        ["group with no exportable video was not exported"],
        "{omissions:?}"
    );
    assert_eq!(omissions.last().unwrap().record, group);
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .count(),
        1
    );
}

#[test]
fn a_plain_group_longer_than_its_children_keeps_their_clock_and_transparent_tail() {
    let (outer, _) = nested_sequence();
    let mut document = import(&outer);
    let group = format!(
        "layer {} (\"Inner\")",
        document["composition"]["layers"][0]["id"]
    );
    document["composition"]["layers"][0]["playback"] = linear_playback(
        json!({"start": 1000, "duration": 4000}),
        json!({"start": 0, "duration": 4000}),
    );
    let (project, omissions) = export(document);
    project.validate().unwrap();
    assert!(
        omissions.iter().any(|omission| omission.record == group
            && omission
                .reason
                .contains("group extends past its exported children's end")),
        "{omissions:?}"
    );
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .nest_occurrences()
            .count(),
        2
    );
    let nest = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(nest.timeline_ticks(), TICKS..4 * TICKS);
    assert_eq!(nest.in_ticks..nest.out_ticks, 0..3 * TICKS);
    assert_eq!(nest.sequence.timeline_end_ticks, 3 * TICKS);
    let child = nest.sequence.video_occurrences().next().unwrap();
    assert_eq!(child.timeline_ticks(), 0..3 * TICKS);
    assert_eq!(child.source_ticks(), TICKS..4 * TICKS);
    assert!(project
        .single_sequence()
        .unwrap()
        .video_items()
        .next()
        .is_some());
}

#[test]
fn an_edited_nested_crop_survives_outer_reimport() {
    // Derive the smaller canvas from the pinned native nest fixture. The
    // guide edit and writer readback prove structure, not native rendering.
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_nested_sequence_strict.prproj");
    let (native, omissions) =
        PrProjectFile::load_selected(&source, Some("dab91e14-ca76-47e7-93fc-99bf6bcc94be"))
            .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let mut sequence = native.single_sequence().unwrap().clone();
    for nest in sequence
        .video_tracks
        .iter_mut()
        .flat_map(|track| &mut track.nests)
    {
        [nest.sequence.width, nest.sequence.height] = [960, 540];
    }
    let mut document = premiere_to_tesseract(
        &sequence,
        &native.media,
        &asset_ids_in_order(&sequence, &native.media),
        &mut Vec::new(),
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let groups = document["composition"]["layers"].as_array_mut().unwrap();
    let group = groups
        .iter_mut()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(group["transform"]["anchorPoint"], json!([480.0, 270.0]));
    let guide_id = group["masks"][0]["layer"].clone();
    let guide = group["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["id"] == guide_id)
        .unwrap();
    assert_eq!(guide["rect"]["size"], json!([960.0, 540.0]));
    guide["rect"]["position"] = json!([120.0, 54.0]);
    guide["rect"]["size"] = json!([720.0, 432.0]);
    let expected_group = group.clone();
    let sibling_videos = videos(&document["composition"]["layers"]);
    assert!(!sibling_videos.is_empty());
    let (mut exported, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let expected_nests = nests(exported.single_sequence().unwrap());
    assert!(!expected_nests.is_empty());
    for (id, media) in &mut exported.media {
        media.name = format!("{}.mp4", id.as_str());
        media.relative_path = Some(format!("./media/{}.mp4", id.as_str()));
        media.relative_paths = vec![media.relative_path.clone().unwrap()];
        media.absolute_paths = vec![(
            MediaPathField::FilePath,
            format!("/media/{}.mp4", id.as_str()).into(),
        )];
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("edited-crop.prproj");
    PremiereProjectXml::new(&exported)
        .unwrap()
        .write_new(&path)
        .unwrap();
    let (reloaded, omissions) = PrProjectFile::load(&path).unwrap();
    let outer = reloaded.single_sequence().unwrap();
    assert_eq!(
        nests(outer),
        expected_nests,
        "outer Crop lost its children: {omissions:?}"
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let reimported = premiere_to_tesseract(
        outer,
        &reloaded.media,
        &asset_ids_in_order(outer, &reloaded.media),
        &mut Vec::new(),
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let group = reimported["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group")
        .unwrap();
    assert_eq!(group["transform"], expected_group["transform"]);
    assert_eq!(group["playback"], expected_group["playback"]);
    assert_eq!(videos(&group["layers"]), videos(&expected_group["layers"]));
    assert_eq!(videos(&reimported["composition"]["layers"]), sibling_videos);
    let guide = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == group["masks"][0]["layer"])
        .unwrap();
    assert_eq!(guide["parent"], group["id"]);
    assert_eq!(guide["rect"]["position"], json!([120.0, 54.0]));
    // Pixel-to-percent-to-pixel conversion can differ by a floating-point bit.
    let size = guide["rect"]["size"].as_array().unwrap();
    assert_eq!(size.len(), 2);
    for (actual, expected) in size.iter().zip([720.0, 432.0]) {
        assert!(
            (actual.as_f64().unwrap() - expected).abs() < 1e-9,
            "{size:?}"
        );
    }
}

#[test]
fn an_extended_plain_group_from_the_native_fixture_keeps_its_child() {
    // Native source and identity are pinned in the conversion manifest. Only
    // the imported first group's FX window is edited for this export case;
    // this is source-based structure, not fresh Adobe acceptance of the edit.
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_nested_sequence_strict.prproj");
    let original = std::fs::read(&source).unwrap();
    let (native, omissions) =
        PrProjectFile::load_selected(&source, Some("dab91e14-ca76-47e7-93fc-99bf6bcc94be"))
            .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = native.single_sequence().unwrap();
    let mut document = premiere_to_tesseract(
        sequence,
        &native.media,
        &asset_ids_in_order(sequence, &native.media),
        &mut Vec::new(),
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    document["composition"]["layers"][0]["playback"] = linear_playback(
        json!({"start": 1000, "duration": 4000}),
        json!({"start": 0, "duration": 4000}),
    );
    let (exported, _) = export(document);
    let nest = exported
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .next()
        .unwrap();
    assert_eq!(nest.timeline_ticks(), TICKS..4 * TICKS);
    assert_eq!(nest.in_ticks..nest.out_ticks, 0..3 * TICKS);
    let child = nest.sequence.video_occurrences().next().unwrap();
    assert_eq!(child.timeline_ticks(), 0..3 * TICKS);
    assert_eq!(child.source_ticks(), TICKS..4 * TICKS);
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn a_long_group_with_own_compositing_or_animation_is_still_omitted() {
    for control in [
        "effect",
        "opacity",
        "motion",
        "skew",
        "blend",
        "opacity keys",
        "motion keys",
        "mask",
        "matte",
    ] {
        let (outer, _) = nested_sequence();
        let mut document = import(&outer);
        let group_id = document["composition"]["layers"][0]["id"].clone();
        let transform = document["composition"]["layers"][3]["transform"].clone();
        document["composition"]["layers"][0]["playback"] = linear_playback(
            json!({"start": 1000, "duration": 4000}),
            json!({"start": 0, "duration": 4000}),
        );
        let group = &mut document["composition"]["layers"][0];
        if matches!(control, "mask" | "matte") {
            // Keep this a general nest rather than the single-video stage
            // route, whose separate span gate already rejects a short child.
            let mut second = group["layers"][0].clone();
            second["id"] = json!(102);
            group["layers"].as_array_mut().unwrap().push(second);
        }
        match control {
            "effect" => {
                group["effects"] =
                    json!([{"id":100, "effect":{"type":"gaussianBlur", "blurriness":5}}])
            }
            "opacity" => group["transform"]["opacity"] = json!(50),
            "motion" => group["transform"]["rotation"] = json!(30),
            "skew" => group["transform"]["skew"] = json!(10),
            "blend" => group["blendMode"] = json!("multiply"),
            "mask" => {
                group["masks"] = json!([{"id":101, "mode":"add", "layer":100}]);
                group["layers"].as_array_mut().unwrap().push(json!({
                    "type":"Rect", "id":100, "parent":group_id, "name":"Group crop guide",
                    "activeRange":{"start":0,"duration":4000}, "transform":transform,
                    "rect":{"size":[1920,1080],"fillColor":[0,0,0,1]}
                }));
            }
            "matte" => {
                group["trackMatte"] = json!({"mode":"alpha", "layer":1});
                let matte = &mut document["composition"]["layers"][2];
                assert_eq!(matte["id"], 1);
                matte["playback"] = linear_playback(
                    json!({"start":1000,"duration":4000}),
                    json!({"start":0,"duration":4000}),
                );
                matte["sourceRange"]["duration"] = json!(4000);
            }
            "opacity keys" | "motion keys" => {
                let property = if control == "opacity keys" {
                    "opacity"
                } else {
                    "rotation"
                };
                document["composition"]["dynamics"] = json!({"entries":[{
                    "target":{"kind":"layer","layerId":group_id,"propertyType":property},
                    "animator":{"type":"keyframes","enabled":true,"keyframes":[
                        {"id":"own-0","layerTime":0,"value":{"type":"float","value":0},"easing":{"type":"linear"}},
                        {"id":"own-1","layerTime":1000,"value":{"type":"float","value":40},"easing":{"type":"linear"}}
                    ]}
                }]});
            }
            _ => unreachable!(),
        }
        let (project, omissions) = export(document);
        assert_eq!(
            project
                .single_sequence()
                .unwrap()
                .nest_occurrences()
                .count(),
            1,
            "{control}"
        );
        assert!(
            omissions.iter().any(|omission| omission.record
                == format!("layer {group_id} (\"Inner\")")
                && omission
                    .reason
                    .contains("group extends past its exported children's end")),
            "{control}: {omissions:?}"
        );
    }
}

#[test]
fn a_nest_keeps_transparent_gaps_without_requiring_canvas_coverage() {
    // Nest 5-11 s can be transparent at 10-11 s. A short root canvas must not
    // change the nest's content or the sequence duration.
    let (outer, _) = nested_sequence();
    let mut document = import(&outer);
    let canvas = &mut document["composition"]["layers"][3];
    assert_eq!(canvas["name"], "Premiere black canvas");
    canvas["activeRange"]["duration"] = json!(10000);
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.end_ticks(), 11 * TICKS);
    assert_eq!(sequence.nest_occurrences().count(), 2);
    assert_eq!(
        sequence.gaps(&project.media),
        Vec::from_iter(Some(10 * TICKS..11 * TICKS))
    );
    let nest = sequence.nest_occurrences().last().unwrap();
    assert_eq!(
        nest.sequence.gaps(&project.media),
        Vec::from_iter(Some(4 * TICKS..5 * TICKS))
    );
}

#[test]
fn nested_crop_guides_stay_with_their_video_and_export_as_crop() {
    let (mut outer, _) = nested_sequence();
    outer.video_tracks[1].nests[0].sequence.video_tracks[0]
        .clip_mut(0)
        .crop
        .left = 25.0;
    let document = import(&outer);
    let group = &document["composition"]["layers"][0];
    let video = &group["layers"][0];
    let guide = &group["layers"][1];
    assert_eq!(group["layers"].as_array().unwrap().len(), 2);
    assert_eq!(video["parent"], group["id"]);
    assert_eq!(guide["parent"], group["id"]);
    assert_eq!(video["masks"][0]["layer"], guide["id"]);
    assert_eq!(guide["rect"]["position"], json!([480.0, 0.0]));
    assert_eq!(guide["rect"]["size"], json!([1440.0, 1080.0]));
    let (project, omissions) = export(document);
    assert!(omissions.is_empty(), "{omissions:?}");
    let nests: Vec<_> = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .collect();
    assert_eq!(nests[0].sequence.video_tracks[0].clip(0).crop.left, 25.0);
    assert!(nests[1].sequence.video_tracks[0].clip(0).crop.is_default());
}

#[test]
fn native_opening_slow_clip_trims_to_the_nested_window() {
    // Selected timing fields of native255/648; the outer placement shows
    // inner frames9..19 at24000/1001. Media and unrelated metadata are staged.
    let native = roxmltree::Document::parse(include_str!(
        "../../../tests/fixtures/cap2-native-retime.xml"
    ))
    .unwrap();
    let ticks = |name| {
        native
            .descendants()
            .find(|node| node.has_tag_name(name))
            .unwrap()
            .text()
            .unwrap()
            .parse::<i64>()
            .unwrap()
    };
    let rate = native
        .descendants()
        .find(|node| node.has_tag_name("PlaybackSpeed"))
        .unwrap()
        .text()
        .unwrap()
        .parse()
        .unwrap();
    let frame = 10594584000;
    let mut clip = clip_of("timecoded", ticks("Start")..ticks("End"), ticks("InPoint"));
    clip.id = Some("VideoClipTrackItem:255".into());
    clip.out_ticks = ticks("OutPoint");
    clip.playback_rate = rate;
    let mut inner = sequence_of("Native opening", vec![PrVideoTrack::media([clip])]);
    inner.frame_rate = FrameRate::from_ticks_per_frame(frame).unwrap();
    let nest = nest_of(inner, 0..10 * frame, 9 * frame);
    let mut outer = sequence_of(
        "Opening window",
        vec![PrVideoTrack {
            items: Vec::new(),
            transitions: Vec::new(),
            nests: vec![nest],
        }],
    );
    outer.frame_rate = FrameRate::from_ticks_per_frame(frame).unwrap();
    let document = import(&outer);
    let video = &document["composition"]["layers"][0]["layers"][0];
    assert_eq!(
        video["playback"]["inputRange"],
        json!({"start":0,"duration":417})
    );
    assert_eq!(video["sourceRange"], json!({"start":67,"duration":83}));
    let keys = crate::tests::support::playback_keys(video);
    assert_eq!(keys[0]["value"], 67);
    assert_eq!(keys[1]["value"], 150);
    assert_eq!(video["source"]["assetId"], "premiere-video-1");
}

/// A Linear key at a source time.
fn linear_key(source_ticks: i64, value: f64) -> PrScalarKeyframe {
    PrScalarKeyframe {
        source_ticks,
        value,
        easing: PrKeyframeEasing::Linear,
    }
}

#[test]
fn reverse_nest_separates_occurrence_keys_from_authored_descendant_clocks() {
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[1];
    nest.playback_rate = -1.0;
    nest.reverse_source_duration = Some(12 * TICKS);
    nest.in_ticks = 6 * TICKS;
    nest.out_ticks = 12 * TICKS;
    nest.animations = vec![
        PrPropertyAnimation::UniformScale(vec![
            linear_key(6 * TICKS, 100.0),
            linear_key(12 * TICKS, 200.0),
        ]),
        PrPropertyAnimation::Opacity(vec![
            linear_key(6 * TICKS, 100.0),
            linear_key(12 * TICKS, 0.0),
        ]),
    ];
    nest.sequence.video_tracks[0].clip_mut(1).animations =
        vec![PrPropertyAnimation::Opacity(vec![
            linear_key(7 * TICKS, 0.0),
            linear_key(8 * TICKS, 100.0),
        ])];
    let document = project_document_with_media(&outer, &media);
    let owner = &document["composition"]["layers"][1];
    let picture = &owner["layers"][0];
    assert_eq!(
        owner["playback"]["mapping"]["output"],
        json!({"start": 0, "duration": 6000})
    );
    let keys = crate::tests::support::playback_keys(picture);
    assert_eq!(
        (keys[0]["time"].clone(), keys[0]["value"].clone()),
        (json!(0), json!(6000))
    );
    assert_eq!(
        (keys[1]["time"].clone(), keys[1]["value"].clone()),
        (json!(6000), json!(0))
    );
    assert_eq!(
        videos(&picture["layers"]),
        vec![
            (
                json!({"start": 0, "duration": 4000}),
                json!({"start": 0, "duration": 4000}),
                "premiere-video-2".into()
            ),
            (
                json!({"start": 5000, "duration": 1000}),
                json!({"start": 7000, "duration": 1000}),
                "premiere-video-2".into()
            ),
        ]
    );
    let descendants = picture["layers"].as_array().unwrap();
    let late = descendants
        .iter()
        .find(|layer| crate::test_support::layer_range(layer)["start"] == 5000)
        .unwrap();
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    for (id, property, last_time) in [
        (owner["id"].clone(), "scaleX", 6000),
        (owner["id"].clone(), "scaleY", 6000),
        (owner["id"].clone(), "opacity", 6000),
        (late["id"].clone(), "opacity", 1000),
    ] {
        let entry = entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == id && entry["target"]["propertyType"] == property
            })
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys[0]["layerTime"], 0);
        assert_eq!(keys[1]["layerTime"], last_time);
    }
}

#[test]
fn reverse_nest_keeps_unmasked_effect_parameters_and_keys_on_occurrence_clock() {
    use crate::schema::{PrEffectParamAnimation, PrEffectParamKeys, GAUSSIAN_BLUR_BLURRINESS};
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[1];
    nest.playback_rate = -1.0;
    nest.reverse_source_duration = Some(12 * TICKS);
    nest.in_ticks = 6 * TICKS;
    nest.out_ticks = 12 * TICKS;
    let mut effect = blur(25.0);
    effect.animations.push(PrEffectParamAnimation {
        param: &GAUSSIAN_BLUR_BLURRINESS,
        keys: PrEffectParamKeys::Scalar(vec![
            linear_key(6 * TICKS, 25.0),
            linear_key(12 * TICKS, 50.0),
        ]),
    });
    nest.effects.push(effect);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let owner = &document["composition"]["layers"][1];
    let stage = &owner["layers"][0];
    assert_eq!(stage["name"], "Nested sequence effects");
    assert_eq!(
        stage["playback"]["mapping"]["output"],
        json!({"start": 0, "duration": 6000})
    );
    assert_eq!(stage["effects"].as_array().unwrap().len(), 1);
    assert_eq!(stage["effects"][0]["effect"]["blurriness"], 25.0);
    let picture = &stage["layers"][0];
    assert_eq!(
        crate::tests::support::playback_keys(picture)[0]["value"],
        6000
    );
    assert_eq!(crate::tests::support::playback_keys(picture)[1]["value"], 0);
    let effect_id = stage["effects"][0]["id"].clone();
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["effectId"] == effect_id)
        .unwrap();
    assert_eq!(entry["target"]["paramName"], "blurriness");
    assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 6000);
    assert!(
        omissions
            .iter()
            .any(|note| note.kind == OmissionKind::Approximated
                && note.reason.contains(
                    "reverse nested effect keys retained on the increasing occurrence clock"
                )),
        "{omissions:?}"
    );
    assert!(
        !omissions
            .iter()
            .any(|note| note.reason.contains("keys were not imported")),
        "{omissions:?}"
    );
}

#[test]
fn reverse_nest_keeps_content_and_other_keys_when_one_motion_parameter_is_unrepresentable() {
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[1];
    nest.playback_rate = -1.0;
    nest.reverse_source_duration = Some(12 * TICKS);
    nest.in_ticks = 6 * TICKS;
    nest.out_ticks = 12 * TICKS;
    nest.opacity = 40.0;
    nest.animations = vec![
        // Both distinct native times round to the same FX millisecond. This
        // parameter cannot retain its keys, but the nest and Rotation can.
        PrPropertyAnimation::Opacity(vec![
            linear_key(6 * TICKS, 40.0),
            linear_key(6 * TICKS + 1, 60.0),
        ]),
        PrPropertyAnimation::Rotation(vec![
            linear_key(6 * TICKS, 10.0),
            linear_key(12 * TICKS, 40.0),
        ]),
    ];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let owner = &document["composition"]["layers"][1];
    assert_eq!(owner["type"], "Group");
    assert_eq!(owner["transform"]["opacity"], 40.0);
    let picture = &owner["layers"][0];
    assert_eq!(
        crate::tests::support::playback_keys(picture)[0]["value"],
        6000
    );
    assert_eq!(picture["layers"][0]["type"], "Video");
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let rotation = entries
        .iter()
        .find(|entry| {
            entry["target"]["layerId"] == owner["id"]
                && entry["target"]["propertyType"] == "rotation"
        })
        .unwrap();
    assert_eq!(rotation["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(rotation["animator"]["keyframes"][1]["layerTime"], 6000);
    assert!(!entries
        .iter()
        .any(|entry| entry["target"]["layerId"] == owner["id"]
            && entry["target"]["propertyType"] == "opacity"));
    assert_eq!(omissions.len(), 1, "{omissions:?}");
    assert_eq!(omissions[0].scope, OmissionScope::Feature);
    assert!(
        omissions[0]
            .reason
            .contains("Opacity animation was not imported"),
        "{omissions:?}"
    );
    assert!(
        omissions[0]
            .reason
            .contains("static value and other parameters were kept"),
        "{omissions:?}"
    );
}

#[test]
fn reverse_nest_does_not_expose_masked_occurrence_content_to_unmapped_effects() {
    for masked in [false, true] {
        let (mut outer, media) = nested_sequence();
        let frame_rate = outer.native_frame_rate();
        let nest = &mut outer.video_tracks[1].nests[1];
        nest.playback_rate = -1.0;
        nest.reverse_source_duration = Some(12 * TICKS);
        nest.in_ticks = 6 * TICKS;
        nest.out_ticks = 12 * TICKS;
        nest.effects.push(blur(25.0));
        if masked {
            nest.opacity_mask = Some(opacity_mask());
        } else {
            nest.crop.left = 10.0;
        }
        let error = nest.validate(frame_rate, &media).unwrap_err().to_string();
        let reason = if masked {
            "nested Opacity mask requires a static vector outline, a unit-forward matching clock"
        } else {
            "masked occurrence effect/coverage clocks"
        };
        assert!(error.contains(reason), "{error}");
    }
}

#[test]
fn a_retimed_nest_maps_its_window_onto_children_on_the_inner_clock() {
    // The untrimmed copy of Inner (outer 5-11 s) plays inner 4.5-6 s at a
    // quarter speed. Inner's 0-4 s clip is outside that window; its 5-6 s
    // clip from source 7 s keeps its inner place and fades in from a key at
    // source 7.5 s. The nest eases its Scale up from 150.
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[1];
    nest.id = Some("retimed-nest".into());
    (nest.in_ticks, nest.out_ticks) = (4 * TICKS + TICKS / 2, 6 * TICKS);
    nest.transform.scale = [150.0; 2];
    nest.animations = vec![PrPropertyAnimation::UniformScale(vec![
        linear_key(5 * TICKS, 150.0),
        linear_key(6 * TICKS, 200.0),
    ])];
    nest.sequence.video_tracks[0].clip_mut(1).animations =
        vec![PrPropertyAnimation::Opacity(vec![
            linear_key(7 * TICKS + TICKS / 2, 0.0),
            linear_key(8 * TICKS, 100.0),
        ])];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    // The nest's keys are not converted, as a retimed clip's are not; its
    // static Scale stays.
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "retimed-nest".into(),
            reason: "UniformScale animation was not imported: keys on a retimed nested sequence occurrence are not converted; static values were kept".into(),
        }]
    );
    let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
    let layers = &document["composition"]["layers"];
    // The plain copy keeps its plain clock.
    assert_eq!(
        layers[0]["playback"]["mapping"],
        json!({"type": "linear", "input": range(0, 3000), "output": range(0, 3000)})
    );
    // Outer 5-11 s maps onto inner 4.5-6 s once.
    let group = &layers[1];
    assert_eq!(
        group["playback"],
        json!({
            "type": "windowed",
            "inputRange": range(5000, 6000),
            "mapping": {
                "type": "linear",
                "input": range(5000, 6000),
                "output": range(4500, 1500)
            },
            "inputOffsetMs": 0
        })
    );
    assert_eq!(group["transform"]["scale"], json!([150.0, 150.0]));
    // Neither shifted by In nor trimmed to the window: the clip stays at
    // inner 5-6 s, from source 7 s.
    assert_eq!(
        videos(&group["layers"]),
        [(
            range(5000, 1000),
            range(7000, 1000),
            "premiere-video-2".to_owned()
        )]
    );
    // The frame guide covers the window on the inner clock.
    let guide = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == group["masks"][0]["layer"])
        .unwrap();
    assert_eq!(guide["activeRange"], range(4500, 1500));
    // Only the clip's own fade is keyed, on its own clock from its In.
    let video = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let keyed: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["target"]["layerId"].clone(),
                entry["target"]["propertyType"].as_str().unwrap().to_owned(),
                entry["animator"]["keyframes"][0]["layerTime"].clone(),
                entry["animator"]["keyframes"][1]["layerTime"].clone(),
            )
        })
        .collect();
    assert_eq!(
        keyed,
        [(
            video["id"].clone(),
            "opacity".to_owned(),
            json!(500),
            json!(1000)
        )]
    );
    // Export keeps its existing scope: a group with another clock than the
    // plain one exports as no clip or nest, its clip's keys with it.
    let (group_id, video_id) = (group["id"].clone(), video["id"].clone());
    let (project, omissions) = export(document);
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.record.clone(), omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                format!("layer {group_id} (\"Inner\")"),
                "stage group was not exported as one clip: the group has a nonidentity content clock"
            ),
            (
                format!("layer {video_id}"),
                "animation on an omitted or unsupported layer was not exported"
            ),
            (
                "document.duration".to_owned(),
                "duration differs from the last occurrence; exported duration is 2540160000000 ticks"
            ),
        ]
    );
    assert_eq!(
        nests(project.single_sequence().unwrap())
            .into_iter()
            .map(|(timeline, window, _)| (timeline, window))
            .collect::<Vec<_>>(),
        [(0..3 * TICKS, 0..3 * TICKS)]
    );
}

#[test]
fn nested_occurrence_effects_preserve_retimed_nonzero_in_content_audio_and_guide() {
    use crate::schema::{AudioChannels, PrAudioOccurrence, PrAudioStream};

    let (mut outer, mut media) = nested_sequence();
    media.get_mut(&MediaId("timecoded".into())).unwrap().audio = Some(PrAudioStream {
        prepared_clock: None,
        intrinsic_ticks: 10 * TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    });
    let nest = &mut outer.video_tracks[1].nests[1];
    (nest.in_ticks, nest.out_ticks) = (4 * TICKS + TICKS / 2, 6 * TICKS);
    nest.playback_rate = 0.25;
    nest.effects = vec![blur(25.0)];
    nest.sequence.video_tracks[0].clip_mut(1).animations =
        vec![PrPropertyAnimation::Opacity(vec![
            linear_key(7 * TICKS + TICKS / 2, 0.0),
            linear_key(8 * TICKS, 100.0),
        ])];
    nest.sequence.audio.push(PrAudioOccurrence {
        playback_rate: 1.0,
        preserve_audio_pitch: false,
        source_channel: None,
        id: None,
        media: MediaId("timecoded".into()),
        start_ticks: 5 * TICKS,
        end_ticks: 6 * TICKS,
        in_ticks: 7 * TICKS,
        out_ticks: 8 * TICKS,
        volume: fx_schema::LinearGain::new(0.5).unwrap(),
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    });
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
    let group = &document["composition"]["layers"][1];
    assert_eq!(group["playback"]["mapping"]["output"], range(4500, 1500));
    let picture = &group["layers"][0];
    assert_eq!(picture["playback"]["inputRange"], range(4500, 1500));
    assert_eq!(picture["playback"]["mapping"]["output"], range(4500, 1500));
    assert_eq!(picture["effects"][0]["effect"]["blurriness"], 25.0);
    let children = picture["layers"].as_array().unwrap();
    let video = children
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let audio = children
        .iter()
        .find(|layer| layer["type"] == "Audio")
        .unwrap();
    for child in [video, audio] {
        assert_eq!(*crate::test_support::layer_range(child), range(5000, 1000));
        assert_eq!(child["sourceRange"], range(7000, 1000));
        assert_eq!(child["parent"], picture["id"]);
    }
    let guide = children
        .iter()
        .find(|layer| layer["id"] == picture["masks"][0]["layer"])
        .unwrap();
    assert_eq!(guide["activeRange"], range(4500, 1500));
    assert_eq!(guide["parent"], picture["id"]);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["target"]["layerId"], video["id"]);
    assert_eq!(entries[0]["animator"]["keyframes"][0]["layerTime"], 500);
    assert_eq!(entries[0]["animator"]["keyframes"][1]["layerTime"], 1000);
}

#[test]
fn nested_occurrence_effects_preserve_mixed_rate_nonzero_in_and_keyed_blur_clock() {
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.sequence.frame_rate = FrameRate::Fps25;
    let mut effect = blur(25.0);
    effect.animations = vec![crate::schema::PrEffectParamAnimation {
        param: &crate::schema::GAUSSIAN_BLUR_BLURRINESS,
        keys: crate::schema::PrEffectParamKeys::Scalar(vec![
            linear_key(TICKS / 2, 25.0),
            linear_key(3 * TICKS / 2, 50.0),
        ]),
    }];
    nest.effects = vec![effect];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["playback"]["mapping"]["output"], range(1000, 3000));
    let picture = &group["layers"][0];
    assert_eq!(picture["playback"]["inputRange"], range(1000, 3000));
    assert_eq!(picture["playback"]["mapping"]["output"], range(1000, 3000));
    assert_eq!(videos(&picture["layers"])[0].0, range(0, 4000));
    let guide = picture["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == picture["masks"][0]["layer"])
        .unwrap();
    assert_eq!(guide["activeRange"], range(1000, 3000));
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["target"]["effectId"],
        picture["effects"][0]["id"]
    );
    assert_eq!(entries[0]["animator"]["keyframes"][0]["layerTime"], 500);
    assert_eq!(entries[0]["animator"]["keyframes"][1]["layerTime"], 1500);
}

#[test]
fn a_retimed_nest_inside_a_trimmed_nest_keeps_its_rate() {
    // Mid plays Leaf 0-2 s at half speed over Mid 0-4 s. Outer shows Mid
    // 1-3 s at normal speed, where Leaf plays 0.5-1.5 s.
    let leaf = sequence_of(
        "Leaf",
        vec![PrVideoTrack::media([clip_of("timecoded", 0..4 * TICKS, 0)])],
    );
    let mut retimed = nest_of(leaf, 0..4 * TICKS, 0);
    retimed.out_ticks = 2 * TICKS;
    let mid = sequence_of(
        "Mid",
        vec![PrVideoTrack {
            transitions: Vec::new(),
            items: Vec::new(),
            nests: vec![retimed],
        }],
    );
    let outer = sequence_of(
        "Outer",
        vec![PrVideoTrack {
            transitions: Vec::new(),
            items: Vec::new(),
            nests: vec![nest_of(mid, 0..2 * TICKS, TICKS)],
        }],
    );
    let document = import(&outer);
    let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
    let mid = &document["composition"]["layers"][0];
    let leaf = &mid["layers"][0];
    assert_eq!(
        mid["playback"]["mapping"],
        json!({"type": "linear", "input": range(0, 2000), "output": range(0, 2000)})
    );
    assert_eq!(
        leaf["playback"],
        json!({
            "type": "windowed",
            "inputRange": range(0, 2000),
            "mapping": {
                "type": "linear",
                "input": range(0, 2000),
                "output": range(500, 1000)
            },
            "inputOffsetMs": 0
        })
    );
    // Leaf's clip keeps its place on Leaf's clock.
    assert_eq!(
        videos(&leaf["layers"]),
        [(
            range(0, 4000),
            range(0, 4000),
            "premiere-video-1".to_owned()
        )]
    );
}

#[test]
fn a_retimed_window_shorter_than_a_millisecond_omits_only_its_nest() {
    // Inner 0-0.25 ms over outer 5-11 s rounds to no millisecond of content.
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[1];
    nest.id = Some("sliver-nest".into());
    nest.out_ticks = nest.in_ticks + crate::schema::TICKS_PER_MILLISECOND / 4;
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.scope, omission.record.as_str(), omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            OmissionScope::Occurrence,
            "sliver-nest",
            "nested sequence clock not converted: windowed playback mapping range must be positive and exact"
        )]
    );
    let range = |start: i64, duration: i64| json!({"start": start, "duration": duration});
    assert_eq!(
        summary(&document["composition"]["layers"]),
        [
            ("Group".into(), "Inner".into(), range(0, 3000)),
            ("Video".into(), "Premiere video 1".into(), range(0, 10000)),
            (
                "Rect".into(),
                "Premiere black canvas".into(),
                range(0, 11000)
            ),
        ]
    );
}

#[test]
fn a_clip_that_the_export_grid_collapses_omits_only_the_nest_around_it() {
    // Inner60 runs at 60 fps; its clip of the second frame, 1/60-2/60 s,
    // imports at 17-33 ms of its group, and a 30 fps export snaps both of
    // its ends to one frame. Outer holds the red source at 0-10 s, Inner60 at
    // 0-3 s, and Holder at 3-6 s: the timecoded source under Inner60, at
    // 30 fps. Export omits each Inner60 group, the smallest nest around that
    // clip, and keeps the red clip and Holder with its own clip.
    const SIXTIETH: i64 = FrameRate::Fps60.ticks_per_frame();
    let mut inner = sequence_of(
        "Inner60",
        vec![PrVideoTrack::media([
            clip_of("timecoded", SIXTIETH..2 * SIXTIETH, 0),
            clip_of("timecoded", 2 * SIXTIETH..3 * TICKS, TICKS),
        ])],
    );
    inner.frame_rate = FrameRate::Fps60;
    let nest_track = |nest| PrVideoTrack {
        items: Vec::new(),
        nests: vec![nest],
        transitions: Vec::new(),
    };
    let holder = sequence_of(
        "Holder",
        vec![
            PrVideoTrack::media([clip_of("timecoded", 0..3 * TICKS, 0)]),
            nest_track(nest_of(inner.clone(), 0..3 * TICKS, 0)),
        ],
    );
    let outer = sequence_of(
        "Outer",
        vec![
            PrVideoTrack::media([clip_of("red", 0..10 * TICKS, 0)]),
            nest_track(nest_of(inner, 0..3 * TICKS, 0)),
            nest_track(nest_of(holder, 3 * TICKS..6 * TICKS, 0)),
        ],
    );
    let document = import(&outer);
    // The omission of an Inner60 group, which names its 17-33 ms clip.
    let collapsed = |group: &Value| {
        let clip = group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["playback"]["inputRange"] == json!({"start": 17, "duration": 16}))
            .unwrap();
        (
            format!("layer {} (\"Inner60\")", group["id"]),
            format!(
                "group was not exported as a nested sequence: its layer {} ({:?}) activeRange 17..33 ms collapses to zero duration on the 30 fps sequence grid",
                clip["id"],
                clip["name"].as_str().unwrap()
            ),
        )
    };
    let named = |layers: &Value, name: &str| {
        layers
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Group" && layer["name"] == name)
            .unwrap()
            .clone()
    };
    let layers = &document["composition"]["layers"];
    let mut expected = vec![
        collapsed(&named(layers, "Inner60")),
        collapsed(&named(&named(layers, "Holder")["layers"], "Inner60")),
    ];
    expected.sort();
    let (project, omissions) = export(document);
    let mut lost: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| (omission.record.clone(), omission.reason.clone()))
        .collect();
    lost.sort();
    assert_eq!(lost, expected);
    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        sequence
            .video_occurrences()
            .map(|clip| (clip.start_ticks, clip.end_ticks))
            .collect::<Vec<_>>(),
        [(0, 10 * TICKS)]
    );
    let [holder] = sequence.nest_occurrences().collect::<Vec<_>>()[..] else {
        panic!("one exported nest");
    };
    assert_eq!(
        (
            holder.sequence.name.as_str(),
            holder.start_ticks..holder.end_ticks
        ),
        ("Holder", 3 * TICKS..6 * TICKS)
    );
    assert_eq!(
        (
            holder.sequence.video_occurrences().count(),
            holder.sequence.nest_occurrences().count()
        ),
        (1, 0)
    );
}

#[test]
fn only_a_collapsing_layer_that_export_would_place_omits_its_nest() {
    // Group "Inner" holds the 0-1 s video under layers at 17-33 ms, which a
    // 30 fps export snaps to one frame, over a copy of that video beside
    // it. Such a layer omits the group only when export would place it. One
    // that export omits by its own rule before snapping its range keeps the
    // group and its video and keeps its own reason; a still's conflicting
    // media still rejects the export, beside a layer that collapses too.
    let mut base = crate::test_support::editable_document();
    base["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(10000);
    let mut video = base["composition"]["layers"][0].clone();
    video["parent"] = json!(10);
    let transform = base["composition"]["layers"][1]["transform"].clone();
    let (whole, short) = (
        json!({"start": 0, "duration": 1000}),
        json!({"start": 17, "duration": 16}),
    );
    let still = |source: Value| {
        let mut image = json!({
            "type": "Image", "id": 11, "parent": 10, "name": "Short still",
            "activeRange": short, "transform": transform,
            "source": {
                "assetId": "premiere-still-1", "fit": "contain",
                "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}
            }
        });
        for (field, value) in source.as_object().unwrap() {
            image["source"][field] = value.clone();
        }
        image
    };
    // A 16 ms video of `parent`'s list over `range` of its clock.
    let short_video = |id: u64, name: &str, parent: u64, range: &Value| {
        let mut layer = video.clone();
        layer["id"] = json!(id);
        layer["name"] = json!(name);
        layer["parent"] = json!(parent);
        layer["playback"] = linear_playback(range.clone(), json!({"start": 0, "duration": 16}));
        layer["sourceRange"] = json!({"start": 0, "duration": 16});
        layer
    };
    // Layer 12 is the track matte source of an adjustment, of a Transform
    // stage, which keys its child where a nest would key its sibling, or of
    // a clip.
    let source = short_video(12, "Short source", 10, &short);
    let alpha = json!({"layer": 12, "mode": "alpha"});
    let adjustment = json!({
        "type": "Adjustment", "id": 13, "parent": 10, "name": "Matted adjustment",
        "activeRange": short, "transform": transform, "trackMatte": alpha
    });
    let stage = json!({
        "type": "Group", "id": 14, "parent": 10, "name": "Premiere stage 1",
        "playback": linear_playback(short.clone(), json!({"start": 0, "duration": 16})),
        "transform": transform, "trackMatte": alpha,
        "layers": [short_video(15, "Staged video", 14, &json!({"start": 0, "duration": 16}))]
    });
    let mut keyed = short_video(16, "Short keyed", 10, &short);
    keyed["trackMatte"] = alpha.clone();
    let mut conflicting = still(json!({"assetId": "premiere-video-1"}));
    conflicting["id"] = json!(17);
    conflicting["name"] = json!("Video still");
    // `image` under an Opacity mask whose guide, layer `guide`, is a triangle
    // beside it with its transform and range: a still that export places.
    let masked = |mut image: Value, guide: u64| {
        image["masks"] =
            json!([{"id": guide + 100, "mode": "add", "layer": guide, "feather": [0.0, 0.0]}]);
        let shape = json!({
            "type": "Shape", "id": guide, "parent": 10, "name": "Opacity mask guide",
            "activeRange": image["activeRange"], "transform": image["transform"],
            "shape": {"path": {"commands": [
                {"type": "moveTo", "x": 480.0, "y": 270.0},
                {"type": "lineTo", "x": 1440.0, "y": 270.0},
                {"type": "lineTo", "x": 960.0, "y": 810.0},
                {"type": "close"}
            ]}}
        });
        [image, shape]
    };
    let [masked_video_still, video_still_guide] = masked(conflicting.clone(), 19);
    // The occurrence omissions, the outer clip count and each nest's clip
    // count, or the error.
    type Outcome = Result<(Vec<(String, String)>, usize, Vec<usize>), String>;
    let kept = |omitted: &[(&str, &str)]| -> Outcome {
        let omitted = omitted
            .iter()
            .map(|(record, reason)| (record.to_string(), reason.to_string()));
        Ok((omitted.collect(), 1, vec![1]))
    };
    let collapsed = |layer: &str| -> Outcome {
        Ok((
            vec![(
                "layer 10 (\"Inner\")".to_owned(),
                format!("group was not exported as a nested sequence: its layer {layer} activeRange 17..33 ms collapses to zero duration on the 30 fps sequence grid"),
            )],
            1,
            Vec::new(),
        ))
    };
    let orphan = (
        "layer 12 (\"Short source\")",
        "track matte source of no exported clip was not exported; FX draws it only through the clips that it keys",
    );
    let rows: [(&str, Vec<Value>, Outcome); 9] = [
        (
            "Cover still",
            vec![still(json!({"fit": "cover"}))],
            kept(&[("layer 11 (\"Short still\")", "still was not exported: its media fit is not Contain")]),
        ),
        (
            "cropped still",
            vec![still(json!({"sourceRect": {"x": 0, "y": 0, "width": 960, "height": 1080}}))],
            kept(&[("layer 11 (\"Short still\")", "still was not exported: its sourceRect is not the 1920x1080 image at the origin")]),
        ),
        (
            "matte source of an omitted adjustment",
            vec![adjustment, source.clone()],
            kept(&[
                ("layer 13 (\"Matted adjustment\")", "adjustment layer was not exported: a track matte on an adjustment layer is not exported"),
                orphan,
            ]),
        ),
        (
            "matte source of an omitted stage",
            vec![stage, source.clone()],
            kept(&[
                ("layer 14 (\"Premiere stage 1\")", "stage group was not exported as one clip: the track matte source is not beside the clip"),
                orphan,
            ]),
        ),
        (
            "still of a video asset under a placed still",
            vec![still(json!({})), conflicting],
            Err("layer 10 (\"Inner\"): layer 17 (\"Video still\"): unsupported conversion: asset \"premiere-video-1\" is used with conflicting media kinds across layers".to_owned()),
        ),
        // Controls: a still and a keyed clip that export places, with the
        // source that spans the keyed clip's range.
        ("placed still", vec![still(json!({}))], collapsed("11 (\"Short still\")")),
        (
            "keyed clip",
            vec![keyed, source],
            collapsed("16 (\"Short keyed\")"),
        ),
        // A still under an Opacity mask is placed too, and its media facts
        // are checked, beside a layer that collapses.
        (
            "masked still",
            Vec::from(masked(still(json!({})), 18)),
            collapsed("11 (\"Short still\")"),
        ),
        (
            "masked still of a video asset under a placed still",
            vec![still(json!({})), masked_video_still, video_still_guide],
            Err("layer 10 (\"Inner\"): layer 17 (\"Video still\"): unsupported conversion: asset \"premiere-video-1\" is used with conflicting media kinds across layers".to_owned()),
        ),
    ];
    let mut outcomes = Vec::new();
    let mut expected = Vec::new();
    for (case, short_layers, outcome) in rows {
        let mut document = base.clone();
        let layers = document["composition"]["layers"].as_array_mut().unwrap();
        // A copy of the video goes into the group, below the short layers.
        layers[0]["id"] = json!(3);
        let mut children = short_layers;
        children.push(video.clone());
        layers.insert(
            0,
            json!({
                "type": "Group", "id": 10, "name": "Inner", "blendMode": "normal",
                "playback": linear_playback(whole.clone(), whole.clone()),
                "transform": transform, "layers": children
            }),
        );
        let actual = export_result(document)
            .map(|(project, omissions)| {
                let omitted = omissions
                    .into_iter()
                    .filter(|omission| omission.scope == OmissionScope::Occurrence)
                    .map(|omission| (omission.record, omission.reason));
                let sequence = project.single_sequence().unwrap();
                let nests = sequence
                    .nest_occurrences()
                    .map(|nest| nest.sequence.video_occurrences().count())
                    .collect();
                (
                    omitted.collect(),
                    sequence.video_occurrences().count(),
                    nests,
                )
            })
            .map_err(|error| error.to_string());
        outcomes.push((case, actual));
        expected.push((case, outcome));
    }
    assert_eq!(outcomes, expected);
}

#[test]
fn a_nested_still_crop_guide_stays_with_its_image_and_exports_as_its_crop() {
    // As above, with the first nest's first clip a 1920x1080 still: its Crop
    // imports as the guide beside the image inside the group, and exports in
    // the nested sequence as the still's Crop.
    let (mut outer, mut media) = nested_sequence();
    let (still_id, mut still) = named_media("still");
    still.video.as_mut().unwrap().kind = PrMediaKind::Still { alpha: false };
    media.insert(still_id.clone(), still);
    let clip = outer.video_tracks[1].nests[0].sequence.video_tracks[0].clip_mut(0);
    clip.media = still_id;
    clip.crop.left = 25.0;
    let asset_ids = asset_ids_in_order(&outer, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&outer, &media, &asset_ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let group = &document["composition"]["layers"][0];
    let image = &group["layers"][0];
    let guide = &group["layers"][1];
    assert_eq!(group["layers"].as_array().unwrap().len(), 2);
    assert_eq!(image["type"], "Image");
    assert_eq!(
        (&image["parent"], &guide["parent"]),
        (&group["id"], &group["id"])
    );
    assert_eq!(image["masks"][0]["layer"], guide["id"]);
    assert_eq!(guide["rect"]["position"], json!([480.0, 0.0]));
    assert_eq!(guide["rect"]["size"], json!([1440.0, 1080.0]));
    assert_eq!(guide["transform"], image["transform"]);
    // The facts of each packaged asset: two videos and the still.
    let facts: BTreeMap<_, _> = asset_ids
        .iter()
        .map(|(media_id, asset)| {
            let facts = if media[media_id].is_still() {
                MediaFacts::Still(crate::image_media::ValidatedImage {
                    format: crate::image_media::ImageFormat::Png,
                    width: 1920,
                    height: 1080,
                    alpha: false,
                    icc_profile: false,
                })
            } else {
                MediaFacts::Video(VideoMedia {
                    pixel_aspect: Default::default(),
                    orientation: crate::schema::VideoOrientation::Identity,
                    codec: crate::schema::VideoCodec::H264,
                    bit_depth: 8,
                    colour: None,
                    width: 1920,
                    height: 1080,
                    timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
                })
            };
            (asset.as_str().to_owned(), facts)
        })
        .collect();
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
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
    let nests: Vec<_> = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .collect();
    let still = nests[0].sequence.video_tracks[0].clip(0);
    assert!(project.media[&still.media].is_still());
    assert_eq!(
        still.crop,
        PrStaticCrop {
            left: 25.0,
            ..PrStaticCrop::default()
        }
    );
    assert!(nests[1].sequence.video_tracks[0].clip(0).crop.is_default());
}

#[test]
fn a_nested_still_opacity_mask_draws_its_guide_in_the_still_frame_beside_the_image() {
    // The trimmed copy's first clip as a still at Scale 57.3 under an
    // inverted, feathered Opacity mask over the middle half of its frame: an
    // expanded background whose hole shows what lies below. The guide draws
    // the outline in the still's pixels beside the image, inside the group,
    // with the image's transform. Export writes the mask back on the still
    // in the nest's sequence.
    let (mut outer, mut media) = nested_sequence();
    let (still_id, mut still) = named_media("still");
    still.video.as_mut().unwrap().kind = PrMediaKind::Still { alpha: false };
    media.insert(still_id.clone(), still);
    let clip = outer.video_tracks[1].nests[0].sequence.video_tracks[0].clip_mut(0);
    clip.media = still_id;
    clip.transform.scale = [57.3; 2];
    clip.opacity_mask = Some(PrMask {
        raster: None,
        feather: 191.0,
        opacity: 100.0,
        inverted: true,
        ..opacity_mask()
    });
    let asset_ids = asset_ids_in_order(&outer, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&outer, &media, &asset_ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.scope, omission.kind, omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            OmissionScope::Feature,
            OmissionKind::Approximated,
            crate::schema::MASK_FEATHER_APPROXIMATION
        )]
    );
    let group = &document["composition"]["layers"][0];
    let [image, guide] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the image and its guide: {group}");
    };
    assert_eq!(
        (&image["type"], &guide["type"]),
        (&json!("Image"), &json!("Shape"))
    );
    assert_eq!(
        (&image["parent"], &guide["parent"]),
        (&group["id"], &group["id"])
    );
    assert_eq!(
        image["masks"],
        json!([{"id": image["masks"][0]["id"], "mode": "add", "inverted": true, "layer": guide["id"], "feather": [191.0, 191.0], "expansion": 0.0, "opacity": 1.0}])
    );
    assert_eq!(
        guide["shape"],
        json!({"path": {"commands": [
            {"type": "moveTo", "x": 480.0, "y": 270.0},
            {"type": "lineTo", "x": 1440.0, "y": 270.0},
            {"type": "lineTo", "x": 1440.0, "y": 810.0},
            {"type": "lineTo", "x": 480.0, "y": 810.0},
            {"type": "close"}
        ]}})
    );
    assert_eq!(guide["transform"], image["transform"]);
    assert_eq!(guide["activeRange"], image["activeRange"]);
    assert_eq!(image["transform"]["scale"], json!([57.3, 57.3]));
    // The facts of each packaged asset: two videos and the still.
    let facts: BTreeMap<_, _> = asset_ids
        .iter()
        .map(|(media_id, asset)| {
            let facts = if media[media_id].is_still() {
                MediaFacts::Still(crate::image_media::ValidatedImage {
                    format: crate::image_media::ImageFormat::Png,
                    width: 1920,
                    height: 1080,
                    alpha: false,
                    icc_profile: false,
                })
            } else {
                MediaFacts::Video(VideoMedia {
                    pixel_aspect: Default::default(),
                    orientation: crate::schema::VideoOrientation::Identity,
                    codec: crate::schema::VideoCodec::H264,
                    bit_depth: 8,
                    colour: None,
                    width: 1920,
                    height: 1080,
                    timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
                })
            };
            (asset.as_str().to_owned(), facts)
        })
        .collect();
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
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
    assert_eq!(
        omissions
            .iter()
            .map(|omission| (omission.scope, omission.kind, omission.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            OmissionScope::Feature,
            OmissionKind::Approximated,
            crate::schema::MASK_FEATHER_APPROXIMATION
        )]
    );
    // The still's clip in the nest carries the mask; its guide paints nothing.
    let nests: Vec<_> = project
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .collect();
    let still = nests[0].sequence.video_tracks[0].clip(0);
    assert!(project.media[&still.media].is_still());
    assert_eq!(
        still.opacity_mask,
        Some(PrMask {
            raster: None,
            feather: 191.0,
            opacity: 100.0,
            inverted: true,
            ..opacity_mask()
        })
    );
    assert!(nests[0].sequence.video_tracks.iter().all(|track| track
        .items
        .iter()
        .all(|item| matches!(item, PrVideoItem::Media(_)))));
    // The guide is in the still's own pixels, not the canvas's: a portrait
    // still of another size draws the outline over the middle half of its
    // own frame, about the image's own centre.
    let (mut outer, mut media) = nested_sequence();
    let (still_id, mut still) = named_media("still");
    let stream = still.video.as_mut().unwrap();
    stream.kind = PrMediaKind::Still { alpha: false };
    [stream.width, stream.height] = [1080, 1350];
    media.insert(still_id.clone(), still);
    let clip = outer.video_tracks[1].nests[0].sequence.video_tracks[0].clip_mut(0);
    clip.media = still_id;
    clip.opacity_mask = Some(opacity_mask());
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    let group = &document["composition"]["layers"][0];
    let [image, guide] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the image and its guide: {group}");
    };
    assert_eq!(
        [
            &image["source"]["sourceRect"]["width"],
            &image["source"]["sourceRect"]["height"]
        ],
        [&json!(1080.0), &json!(1350.0)]
    );
    assert_eq!(image["transform"]["anchorPoint"], json!([540.0, 675.0]));
    assert_eq!(guide["transform"], image["transform"]);
    assert_eq!(
        guide["shape"],
        json!({"path": {"commands": [
            {"type": "moveTo", "x": 270.0, "y": 337.5},
            {"type": "lineTo", "x": 810.0, "y": 337.5},
            {"type": "lineTo", "x": 810.0, "y": 1012.5},
            {"type": "lineTo", "x": 270.0, "y": 1012.5},
            {"type": "close"}
        ]}})
    );
}

/// The `(propertyType, [(layerTime, value)])` tracks of `layer` in `document`.
fn motion_keys(document: &Value, layer: &Value) -> Vec<(Value, Vec<(Value, Value)>)> {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == layer["id"])
        .map(|entry| {
            let keys = entry["animator"]["keyframes"].as_array().unwrap().iter();
            (
                entry["target"]["propertyType"].clone(),
                keys.map(|key| (key["layerTime"].clone(), key["value"]["value"].clone()))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn a_nested_still_opacity_mask_guide_keeps_the_image_motion_keys_on_the_nest_clock() {
    // The trimmed copy's first clip as a 1080x1350 still under an unfeathered
    // Opacity mask with Position keys 1 s and 2 s after its InPoint: as the Crop guide
    // does, the mask guide repeats the image's Motion keys, which the nest
    // window from 1 s puts at 0 and 1 s of the group clock, in canvas pixels.
    let (mut outer, mut media) = nested_sequence();
    let (still_id, mut still) = named_media("still");
    let stream = still.video.as_mut().unwrap();
    stream.kind = PrMediaKind::Still { alpha: false };
    [stream.width, stream.height] = [1080, 1350];
    media.insert(still_id.clone(), still);
    let clip = outer.video_tracks[1].nests[0].sequence.video_tracks[0].clip_mut(0);
    clip.media = still_id;
    clip.opacity_mask = Some(PrMask {
        raster: None,
        feather: 0.0,
        ..opacity_mask()
    });
    let key = |seconds: i64, value: [f64; 2]| PrPointKeyframe {
        source_ticks: clip.in_ticks + seconds * TICKS,
        value,
        easing: PrKeyframeEasing::Linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    clip.animations = vec![PrPropertyAnimation::Position(vec![
        key(1, [0.25, 0.5]),
        key(2, [0.75, 0.4]),
    ])];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let group = &document["composition"]["layers"][0];
    let [image, guide] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the image and its guide: {group}");
    };
    assert_eq!(
        (&image["type"], &guide["type"]),
        (&json!("Image"), &json!("Shape"))
    );
    assert_eq!(image["masks"][0]["layer"], guide["id"]);
    let expected = [
        ("positionX", [(0, 480.0), (1000, 1440.0)]),
        ("positionY", [(0, 540.0), (1000, 432.0)]),
    ]
    .map(|(property, keys)| {
        (
            json!(property),
            keys.map(|(time, value)| (json!(time), json!(value)))
                .to_vec(),
        )
    });
    assert_eq!(motion_keys(&document, image), expected);
    assert_eq!(motion_keys(&document, guide), expected);
}

#[test]
fn a_nested_still_opacity_mask_guide_keeps_the_image_motion_keys_on_an_inner_clock_and_canvas() {
    // The same keyed, unfeathered masked still in Inner at 25 fps and
    // 1280x720, with the first placement trimmed one outer frame later, so
    // that In (1 s and 1/30 s) falls between inner frames. The group plays
    // the inner clock from In at unit rate and centres the inner frame. The
    // image and its guide keep their untrimmed inner place and their keys,
    // 1 s and 2 s after the image's InPoint, in inner-canvas pixels.
    let (mut outer, mut media) = nested_sequence();
    for nest in &mut outer.video_tracks[1].nests {
        nest.sequence.frame_rate = FrameRate::Fps25;
        [nest.sequence.width, nest.sequence.height] = [1280, 720];
    }
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.in_ticks += FRAME;
    nest.out_ticks += FRAME;
    let (still_id, mut still) = named_media("still");
    let stream = still.video.as_mut().unwrap();
    stream.kind = PrMediaKind::Still { alpha: false };
    [stream.width, stream.height] = [1080, 1350];
    media.insert(still_id.clone(), still);
    let clip = nest.sequence.video_tracks[0].clip_mut(0);
    clip.media = still_id;
    clip.opacity_mask = Some(PrMask {
        raster: None,
        feather: 0.0,
        ..opacity_mask()
    });
    let key = |seconds: i64, value: [f64; 2]| PrPointKeyframe {
        source_ticks: clip.in_ticks + seconds * TICKS,
        value,
        easing: PrKeyframeEasing::Linear,
        spatial_in_tangent: None,
        spatial_out_tangent: None,
    };
    clip.animations = vec![PrPropertyAnimation::Position(vec![
        key(1, [0.25, 0.5]),
        key(2, [0.75, 0.4]),
    ])];
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let group = &document["composition"]["layers"][0];
    let range = |start: u64, duration: u64| json!({"start": start, "duration": duration});
    // The 0-3 s placement onto the inner clock from In, rounded once.
    assert_eq!(
        group["playback"]["mapping"],
        json!({"type": "linear", "input": range(0, 3000), "output": range(1033, 3000)})
    );
    assert_eq!(
        [
            &group["transform"]["anchorPoint"],
            &group["transform"]["position"]
        ],
        [&json!([640.0, 360.0]), &json!([960.0, 540.0])]
    );
    let [image, guide, frame] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("expected the image, its guide and the frame guide: {group}");
    };
    assert_eq!(
        (&group["masks"][0]["layer"], &frame["rect"]["size"]),
        (&frame["id"], &json!([1280.0, 720.0]))
    );
    assert_eq!(image["masks"][0]["layer"], guide["id"]);
    // Half the still about the middle of the inner canvas.
    assert_eq!(
        [
            &image["transform"]["anchorPoint"],
            &image["transform"]["position"]
        ],
        [&json!([540.0, 675.0]), &json!([640.0, 360.0])]
    );
    // 0.25 and 0.75 of the inner width, 0.5 and 0.4 of its height.
    let expected = [
        ("positionX", [(1000, 320.0), (2000, 960.0)]),
        ("positionY", [(1000, 360.0), (2000, 288.0)]),
    ]
    .map(|(property, keys)| {
        (
            json!(property),
            keys.map(|(time, value)| (json!(time), json!(value)))
                .to_vec(),
        )
    });
    assert_eq!(guide["transform"], image["transform"]);
    for layer in [image, guide] {
        assert_eq!(layer["parent"], group["id"]);
        // The still's whole 0-4 s inner place.
        assert_eq!(layer["activeRange"], range(0, 4000));
        assert_eq!(motion_keys(&document, layer), expected);
    }
}

#[test]
fn sharpen_host_import_omits_identity_child_in_moved_nest_but_keeps_safe_sibling() {
    let (mut outer, media) = nested_sequence();
    outer.video_tracks[1].nests.truncate(1);
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.transform.scale = [50.0, 50.0];
    let child = nest.sequence.video_tracks[0].clip_mut(0);
    assert_eq!(child.transform, PrStaticTransform::default());
    child.effects = vec![
        PrEffect {
            mask: None,
            enabled: true,
            params: PrEffectParams::Sharpen(crate::schema::PrSharpen { amount: 40 }),
            animations: vec![],
        },
        blur(25.0),
    ];
    let record = child.record().to_owned();
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let group = &document["composition"]["layers"][0];
    assert_eq!(group["transform"]["scale"], json!([50.0, 50.0]));
    let child = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let effects = child["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0]["effect"],
        json!({"type": "gaussianBlur", "blurriness": 25.0})
    );
    assert!(
        omissions
            .iter()
            .any(|note| note.scope == OmissionScope::Feature
                && note.record == record
                && note
                    .reason
                    .contains("Sharpen effect at stack position 1 was not imported")
                && note.reason.contains("nested")),
        "{omissions:?}"
    );
}

#[test]
fn sharpen_still_in_nest_is_omitted_without_losing_safe_effects() {
    for scale in [100.0, 50.0] {
        let (mut outer, mut media) = nested_sequence();
        let (id, mut facts) = named_media("still");
        facts.video.as_mut().unwrap().kind = PrMediaKind::Still { alpha: false };
        media.insert(id, facts);
        outer.video_tracks[1].nests.remove(0);
        let nest = &mut outer.video_tracks[1].nests[0];
        nest.transform.scale = [scale; 2];
        let still = nest.sequence.video_tracks[0].clip_mut(1);
        still.media = MediaId("still".into());
        still.effects = vec![
            PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::Sharpen(crate::schema::PrSharpen { amount: 40 }),
                animations: vec![],
            },
            blur(25.0),
        ];
        let record = still.record().to_owned();
        let mut omissions = Vec::new();
        let wire = premiere_to_tesseract(
            &outer,
            &media,
            &asset_ids_in_order(&outer, &media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        let group = wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        let image = group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Image")
            .unwrap();
        let effects = image["effects"].as_array().unwrap();
        assert_eq!(effects.len(), 1, "scale {scale}: {image:?}");
        assert_eq!(
            effects[0]["effect"],
            json!({"type": "gaussianBlur", "blurriness": 25.0})
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.scope == OmissionScope::Feature
                    && note.kind == OmissionKind::Omitted
                    && note.record == record
                    && note
                        .reason
                        .contains("Sharpen effect at stack position 1 was not imported")
                    && note.reason.contains("still")),
            "scale {scale}: {omissions:?}"
        );
    }
}

#[test]
fn replicate_still_in_nest_is_omitted_without_losing_safe_effects() {
    for scale in [100.0, 50.0] {
        let (mut outer, mut media) = nested_sequence();
        let (id, mut facts) = named_media("still");
        facts.video.as_mut().unwrap().kind = PrMediaKind::Still { alpha: false };
        media.insert(id, facts);
        outer.video_tracks[1].nests.remove(0);
        let nest = &mut outer.video_tracks[1].nests[0];
        nest.transform.scale = [scale; 2];
        let still = nest.sequence.video_tracks[0].clip_mut(1);
        still.media = MediaId("still".into());
        still.effects = vec![
            PrEffect {
                mask: None,
                enabled: true,
                params: PrEffectParams::Replicate(crate::schema::PrReplicate { count: 2 }),
                animations: vec![],
            },
            blur(25.0),
        ];
        let record = still.record().to_owned();
        let mut omissions = Vec::new();
        let wire = premiere_to_tesseract(
            &outer,
            &media,
            &asset_ids_in_order(&outer, &media),
            &mut omissions,
        )
        .unwrap()
        .to_json_value()
        .unwrap();
        let group = wire["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Group")
            .unwrap();
        let image = group["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Image")
            .unwrap();
        let effects = image["effects"].as_array().unwrap();
        assert_eq!(effects.len(), 1, "scale {scale}: {image:?}");
        assert_eq!(
            effects[0]["effect"],
            json!({"type": "gaussianBlur", "blurriness": 25.0})
        );
        assert!(
            omissions
                .iter()
                .any(|note| note.scope == OmissionScope::Feature
                    && note.kind == OmissionKind::Omitted
                    && note.record == record
                    && note
                        .reason
                        .contains("Replicate effect at stack position 1 was not imported")
                    && note.reason.contains("still")),
            "scale {scale}: {omissions:?}"
        );
    }
}

/// Supplementary clock/boundary controls; native source proof is the ignored
/// actual-reader regression above, not this semantic model.
#[test]
fn nested_remap_signed_preroll_source_guide_and_bounds() {
    use crate::schema::{PrTimeRemap, PrTimeRemapKeyframe};
    let (mut outer, media) = nested_sequence();
    let nest = &mut outer.video_tracks[1].nests[0];
    nest.start_ticks = 0;
    nest.end_ticks = 2 * TICKS;
    nest.in_ticks = TICKS;
    nest.out_ticks = 2 * TICKS;
    nest.playback_rate = 0.5;
    nest.sequence.width = 3840;
    nest.time_remap = Some(PrTimeRemap {
        keys: [(-1, 0), (0, 1), (1, 2), (10, 11)]
            .into_iter()
            .map(|(input, source)| PrTimeRemapKeyframe {
                timeline_ticks: input * TICKS,
                source_ticks: source * TICKS,
                easing: PrKeyframeEasing::Linear,
            })
            .collect(),
    });
    nest.validate(outer.frame_rate, &media).unwrap();
    let valid = nest.clone();
    let document = import(&outer);
    let group = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Group" && layer["playback"]["inputOffsetMs"] == 2000)
        .unwrap();
    assert_eq!(
        group["playback"]["inputRange"],
        json!({"start":0,"duration":2000})
    );
    assert_eq!(group["playback"]["inputOffsetMs"], 2000);
    let keys = group["playback"]["mapping"]["property"]["keyframes"]
        .as_array()
        .unwrap();
    assert_eq!(
        keys.iter()
            .map(|key| (
                key["time"].as_u64().unwrap(),
                key["value"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [(0, 0), (2000, 1000), (4000, 2000), (22000, 11000)]
    );
    // Native input In/Out is 1..2 s; the frame guide must cover source 0..6 s,
    // not clip away a child whose mapped source time lies outside input bounds.
    let guide = group["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == group["masks"][0]["layer"])
        .unwrap();
    assert_eq!(guide["activeRange"], json!({"start":0,"duration":6000}));
    assert_eq!(videos(&group["layers"]).len(), 2);
    let (exported, reports) = export(document);
    assert!(reports.iter().any(|report| report
        .reason
        .contains("group time remapping is not supported")));
    assert!(
        exported
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .is_some(),
        "unrelated outer video still exports"
    );

    // Endpoint equality is bounded; one native tick past it is not. Do not
    // clamp an unused control or extend the child's authored source domain.
    let mut endpoint = valid.clone();
    endpoint.end_ticks = 10 * TICKS;
    endpoint.out_ticks = 6 * TICKS;
    endpoint.validate(outer.frame_rate, &media).unwrap();
    endpoint.time_remap.as_mut().unwrap().keys[3].source_ticks += 1;
    assert!(endpoint.validate(outer.frame_rate, &media).is_err());
    let mut uncovered = valid.clone();
    uncovered.time_remap.as_mut().unwrap().keys[0].timeline_ticks = 1;
    assert!(uncovered.validate(outer.frame_rate, &media).is_err());
    let mut held = valid.clone();
    held.time_remap.as_mut().unwrap().keys[1].source_ticks = 0;
    assert!(held.validate(outer.frame_rate, &media).is_err());
    let mut unsupported = valid;
    unsupported.time_remap.as_mut().unwrap().keys[2].easing = PrKeyframeEasing::Hold;
    assert!(unsupported.validate(outer.frame_rate, &media).is_err());
}

#[test]
fn audio_clock_visible_retimed_nest_window_preserves_trimmed_source() {
    use crate::schema::PrAudioOccurrence;
    let (outer, _) = nested_sequence();
    let mut nest = outer.video_tracks[1].nests[0].clone();
    nest.start_ticks = 10 * TICKS;
    nest.end_ticks = 13 * TICKS;
    nest.in_ticks = TICKS;
    nest.out_ticks = 4 * TICKS;
    nest.sequence.video_tracks.clear();
    for rate in [2.0_f64, 0.5, -2.0] {
        nest.sequence.audio = vec![PrAudioOccurrence {
            id: None,
            media: MediaId("timecoded".into()),
            source_channel: None,
            preserve_audio_pitch: false,
            playback_rate: rate,
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: TICKS,
            out_ticks: TICKS + (4.0 * rate.abs()) as i64 * TICKS,
            volume: fx_schema::LinearGain::UNITY,
            volume_keys: None,
            fade_in: None,
            fade_out: None,
        }];
        let content = super::visible_content(&nest, &mut Vec::new()).unwrap();
        assert_eq!(
            (content.audio[0].start_ticks, content.audio[0].end_ticks),
            (0, 3 * TICKS)
        );
        assert_eq!(
            content.audio[0].in_ticks,
            TICKS + (rate.abs() * TICKS as f64) as i64
        );
        assert_eq!(content.audio[0].out_ticks, nest.sequence.audio[0].out_ticks);
    }
}

#[test]
fn nested_opacity_mask_keeps_differing_canvas_transform_below_equal_canvas_coverage() {
    use crate::tests::support::{transform_effect, DEFAULT_PR_TRANSFORM};

    // Supplementary composition control, not a native masked-canvas oracle.
    let (mut outer, media) = nested_sequence();
    outer.video_tracks[1].nests.truncate(1);
    let owner = &mut outer.video_tracks[1].nests[0];
    let mut child = owner.clone();
    child.sequence.width = 3840;
    child.sequence.height = 2160;
    child.sequence.video_tracks[0]
        .clip_mut(0)
        .transform
        .position = [0.25; 2];
    child.transform = PrStaticTransform {
        anchor_point: [0.4, 0.45],
        position: [0.4, 0.5],
        scale: [50.0; 2],
        ..PrStaticTransform::default()
    };
    child.effects = vec![transform_effect(
        crate::schema::PrTransform {
            anchor_point: [0.2, 0.25],
            position: [0.35, 0.35],
            uniform_scale: true,
            scale_height: 120.0,
            ..DEFAULT_PR_TRANSFORM
        },
        Vec::new(),
    )];
    owner.sequence = sequence_of(
        "Covered source",
        vec![PrVideoTrack {
            items: Vec::new(),
            nests: vec![child],
            transitions: Vec::new(),
        }],
    );
    owner.in_ticks = 0;
    owner.out_ticks = 3 * TICKS;
    let mut mask = opacity_mask();
    mask.feather = 0.0;
    mask.opacity = 100.0;
    owner.opacity_mask = Some(mask.clone());
    outer.timeline_end_ticks = outer.occurrence_end_ticks();
    let mut reports = Vec::new();
    let document = premiere_to_tesseract(
        &outer,
        &media,
        &asset_ids_in_order(&outer, &media),
        &mut reports,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(
        reports
            .iter()
            .all(|report| report.kind == OmissionKind::Approximated),
        "{reports:?}"
    );
    let (native, reports) = export(document);
    assert!(reports.is_empty(), "{reports:?}");
    let assert_owners = |native: &PrProjectFile| {
        let outer = native.single_sequence().unwrap();
        assert_eq!(outer.dimensions(), [1920, 1080]);
        assert_eq!(
            outer.video_items().count(),
            1,
            "only the original background"
        );
        assert_eq!(outer.nest_occurrences().count(), 1);
        let owner = outer.nest_occurrences().next().unwrap();
        assert_eq!(owner.opacity_mask.as_ref(), Some(&mask));
        assert!(owner.effects.is_empty());
        assert_eq!(owner.sequence.dimensions(), [1920, 1080]);
        assert_eq!(owner.sequence.video_items().count(), 0, "no Shape Graphic");
        let picture = owner.sequence.nest_occurrences().next().unwrap();
        assert_eq!(picture.sequence.dimensions(), [1920, 1080]);
        assert_eq!(
            picture.sequence.video_items().count(),
            0,
            "no frame-guide Graphic"
        );
        let child = picture.sequence.nest_occurrences().next().unwrap();
        assert_eq!(child.sequence.dimensions(), [3840, 2160]);
        assert!(child.opacity_mask.is_none());
        assert_eq!(child.transform.anchor_point, [0.4, 0.45]);
        assert_eq!(child.transform.position, [0.4, 0.5]);
        let [effect] = child.effects.as_slice() else {
            panic!("one Transform below coverage: {child:?}")
        };
        let PrEffectParams::Transform(transform) = &effect.params else {
            panic!("ordinary Transform identity retained")
        };
        assert_eq!(transform.anchor_point, [0.2, 0.25]);
        assert_eq!(transform.position, [0.35, 0.35]);
        assert_eq!(transform.scale(), [120.0; 2]);
        assert_eq!(child.sequence.video_items().count(), 1);
        let lower = child.sequence.video_occurrences().next().unwrap();
        assert_eq!(lower.transform.position, [0.25; 2]);
        assert_eq!(lower.source_ticks(), TICKS..4 * TICKS);
    };
    assert_owners(&native);
    let xml = write_nested_effect_project(native);
    let (reread, reports) = load_nested_effect_project(&xml);
    assert!(reports.is_empty(), "{reports:?}");
    assert_owners(&reread);
}

#[test]
fn nested_opacity_mask_keeps_effects_below_coverage_and_edits_numeric_keys() {
    // Supplementary model of the native Opacity owner, not a native nested
    // animation/render oracle. Placement must not shift source-minus-In keys.
    let (mut sequence, media) = nested_sequence();
    let nest = &mut sequence.video_tracks[1].nests[0];
    nest.start_ticks = TICKS;
    nest.end_ticks = 4 * TICKS;
    nest.effects = vec![blur(30.0)];
    nest.effects_above_mask = 1;
    let mut mask = opacity_mask();
    mask.feather = 0.0;
    mask.opacity = 100.0;
    mask.opacity_keys = vec![linear_key(TICKS, 100.0), linear_key(2 * TICKS, 50.0)];
    nest.opacity_mask = Some(mask);
    let mut reports = Vec::new();
    let mut wire = premiere_to_tesseract(
        &sequence,
        &media,
        &asset_ids_in_order(&sequence, &media),
        &mut reports,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(reports.is_empty(), "{reports:?}");
    let owner = &wire["composition"]["layers"][0];
    assert_eq!(owner["playback"]["inputRange"]["start"], 1000);
    assert!(owner.get("effects").is_none());
    let picture = &owner["layers"][0];
    assert_eq!(picture["effects"][0]["effect"]["blurriness"], 30.0);
    assert!(picture["masks"][0]["layer"] != owner["masks"][0]["layer"]);
    let mask_id = owner["masks"][0]["id"].clone();
    let entry = wire["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["target"]["itemId"] == mask_id)
        .unwrap();
    assert_eq!(entry["animator"]["keyframes"][0]["layerTime"], 0);
    assert_eq!(entry["animator"]["keyframes"][1]["layerTime"], 1000);
    entry["animator"]["keyframes"][1]["value"]["value"] = json!(0.75);
    let (exported, reports) = export(wire);
    assert!(reports.is_empty(), "{reports:?}");
    let nest = exported
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .find(|nest| nest.opacity_mask.is_some())
        .unwrap();
    assert_eq!(nest.start_ticks, TICKS);
    let mask = nest.opacity_mask.as_ref().unwrap();
    assert_eq!(
        mask.opacity_keys
            .iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        [(0, 100.0), (TICKS, 75.0)]
    );
    assert!(
        nest.effects.is_empty(),
        "effects stay below the mask in the editable picture nest"
    );
    assert_eq!(
        nest.sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .count(),
        0,
        "the mask guide is never a Graphic clip"
    );
    let picture = nest.sequence.nest_occurrences().next().unwrap();
    assert_eq!(picture.effects, [exported_blur(true, 30.0, false)]);
}

#[test]
fn nested_opacity_mask_key_collision_omits_owner_without_guides_or_id_reservations() {
    let (mut sequence, media) = nested_sequence();
    let mut mask = opacity_mask();
    mask.feather = 0.0;
    mask.opacity = 100.0;
    mask.opacity_keys = vec![
        linear_key(TICKS, 100.0),
        linear_key(TICKS + ms(1) / 3, 50.0),
    ];
    sequence.video_tracks[1].nests[0].opacity_mask = Some(mask);
    let mut reports = Vec::new();
    let wire = premiere_to_tesseract(
        &sequence,
        &media,
        &asset_ids_in_order(&sequence, &media),
        &mut reports,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    assert!(
        reports
            .iter()
            .any(|report| report.scope == OmissionScope::Occurrence
                && report.reason.contains("nested Opacity mask not converted")),
        "{reports:?}"
    );
    let groups: Vec<_> = wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .collect();
    assert_eq!(groups.len(), 1);
    let mut without_omitted = sequence.clone();
    without_omitted.video_tracks[1].nests.remove(0);
    let expected = import(&without_omitted);
    let (mut actual_ids, mut expected_ids) = (Vec::new(), Vec::new());
    layer_ids(&wire["composition"]["layers"], &mut actual_ids);
    layer_ids(&expected["composition"]["layers"], &mut expected_ids);
    assert_eq!(
        actual_ids, expected_ids,
        "omitted owner reserves no layer IDs"
    );
    assert!(wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|layer| layer["type"] != "Shape"));
    assert!(wire["composition"]["dynamics"]["entries"]
        .as_array()
        .is_none_or(|entries| entries.is_empty()));
}

#[test]
fn nested_opacity_mask_empty_picture_omits_approximation_reports() {
    let (mut sequence, _) = nested_sequence();
    let mut mask = opacity_mask();
    mask.feather = 0.0;
    sequence.video_tracks[1].nests[0].opacity_mask = Some(mask);
    let mut wire = import(&sequence);
    let owner = &mut wire["composition"]["layers"][0];
    let record = format!(
        "layer {} ({:?})",
        owner["id"].as_u64().unwrap(),
        owner["name"].as_str().unwrap()
    );
    owner["masks"][0]["feather"] = json!([20.0, 20.0]);
    let (exported, reports) = export(wire.clone());
    assert!(exported
        .single_sequence()
        .unwrap()
        .nest_occurrences()
        .any(|nest| nest
            .opacity_mask
            .as_ref()
            .is_some_and(|mask| mask.feather == 20.0)));
    assert!(
        reports.iter().any(|report| report.record == record
            && report.kind == OmissionKind::Approximated
            && report.reason.contains("Mask Feather")),
        "{reports:?}"
    );

    // Removing only the picture leaves both unpainted guides valid. Neither
    // guide can retain the occurrence or publish its mask approximations.
    wire["composition"]["layers"][0]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["type"] != "Video");
    let (exported, reports) = export(wire);
    let outer = exported.single_sequence().unwrap();
    assert_eq!(outer.nest_occurrences().count(), 1, "{reports:?}");
    assert!(outer
        .nest_occurrences()
        .all(|nest| nest.opacity_mask.is_none()));
    assert_eq!(outer.video_items().count(), 1, "{reports:?}");
    assert!(
        reports.iter().any(|report| report.record == record
            && report.scope == OmissionScope::Occurrence
            && report.kind == OmissionKind::Omitted
            && report.reason.contains("group with no exportable video")),
        "{reports:?}"
    );
    assert!(
        reports
            .iter()
            .filter(|report| report.record == record)
            .all(|report| report.kind == OmissionKind::Omitted),
        "{reports:?}"
    );
}

#[test]
fn nested_opacity_mask_invalid_export_scope_never_falls_back_unmasked() {
    let (mut sequence, _) = nested_sequence();
    let mut mask = opacity_mask();
    mask.feather = 0.0;
    sequence.video_tracks[1].nests[0].opacity_mask = Some(mask);
    let base = import(&sequence);
    for moved_guide in [false, true] {
        let mut wire = base.clone();
        let owner = &mut wire["composition"]["layers"][0];
        if moved_guide {
            let guide_id = owner["masks"][0]["layer"].clone();
            let guide = owner["layers"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|layer| layer["id"] == guide_id)
                .unwrap();
            guide["transform"]["position"] = json!([20.0, 0.0]);
        } else {
            owner["effects"] =
                json!([{"id": 99, "effect": {"type": "gaussianBlur", "blurriness": 20.0}}]);
        }
        let (exported, reports) = export(wire);
        let outer = exported.single_sequence().unwrap();
        assert_eq!(outer.nest_occurrences().count(), 1, "{reports:?}");
        assert_eq!(
            outer.nest_occurrences().next().unwrap().timeline_ticks(),
            5 * TICKS..11 * TICKS
        );
        // Only the original background may remain outside that later nest;
        // flattening the refused owner into unmasked clips must also fail.
        assert_eq!(outer.video_items().count(), 1, "{reports:?}");
        let background = outer.video_occurrences().next().unwrap();
        assert_eq!(background.media, MediaId("premiere-video-1".into()));
        assert_eq!(background.timeline_ticks(), 0..10 * TICKS);
        assert!(
            reports
                .iter()
                .any(|report| report.scope == OmissionScope::Occurrence
                    && report.reason.contains(if moved_guide {
                        "guide under the group is not at the identity"
                    } else {
                        "effects below its owner"
                    })),
            "{reports:?}"
        );
    }
}
