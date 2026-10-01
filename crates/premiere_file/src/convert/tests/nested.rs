//! Nested sequences as groups (import) and plain groups as nests (export).

use crate::test_support::linear_playback;
use crate::{
    convert::{premiere_to_tesseract, tesseract_to_premiere},
    format::{FrameRate, PrProjectFile, PremiereProjectXml},
    media::{MediaFacts, VideoMedia},
    schema::{
        records::MediaPathField, MediaId, PrColorMatte, PrEffect, PrEffectParams, PrGaussianBlur,
        PrKeyframeEasing, PrLinearWipe, PrMediaKind, PrPointKeyframe, PrPropertyAnimation,
        PrScalarKeyframe, PrSequence, PrStaticCrop, PrStaticTransform, PrVideoItem, PrVideoTrack,
        TICKS,
    },
    tesseract_output::asset_ids_in_order,
    tests::support::{
        amount, clip_of, exported_blur, named_media, nest_of, nested_sequence, sequence_of,
        text_graphic,
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
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut media: BTreeMap<_, _> = ["premiere-video-1", "premiere-video-2"]
        .map(|asset| {
            (
                asset.to_owned(),
                MediaFacts::Video(VideoMedia {
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
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    (project, omissions)
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
            report(
                OmissionKind::Omitted,
                &format!(
                    "effects: {kind} effect 11 was not exported: it has no Premiere effect mapping"
                ),
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
        intrinsic_ticks: 10 * TICKS,
        channels: AudioChannels::Stereo,
        sample_rate: 48_000,
    });
    for nest in &mut outer.video_tracks[1].nests {
        nest.sequence.audio.push(PrAudioOccurrence {
            id: None,
            media: MediaId("timecoded".into()),
            start_ticks: 0,
            end_ticks: 4 * TICKS,
            in_ticks: TICKS / 2,
            out_ticks: 4 * TICKS + TICKS / 2,
            volume: fx_schema::LinearGain::new(0.5).unwrap(),
            volume_keys: None,
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
            "motionBlur",
            json!(true),
            "group motion blur is not supported",
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
        {"id": 14, "enabled": true, "effect": {"type": "posterize", "levels": 7.0}},
        {"id": 15, "enabled": true, "effect": {"type": "mosaic", "horizontalBlocks": 16.0, "verticalBlocks": 9.0, "sharpColors": true}}
    ]);
    let key = |id: &str, layer_time: i64, value: f64| json!({"id": id, "layerTime": layer_time, "value": {"type": "float", "value": value}, "easing": {"type": "linear"}});
    document["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "effectProperty", "effectId": 12, "paramName": "blurriness"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [key("a", 0, 5.0), key("b", 500, 20.0)]},
    }]});
    let (project, omissions) = export(document);
    // Only the Posterize (no mapping) and the Mosaic (no nest host) are
    // reported; the keyed blur exports with its keys.
    assert_eq!(
        omissions,
        [
            Omission {
                scope: OmissionScope::Feature,
                kind: OmissionKind::Omitted,
                record: "layer 2 (\"Inner\")".into(),
                reason: "effects: posterize effect 14 was not exported: it has no Premiere effect mapping".into(),
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

fn try_export(document: Value) -> crate::error::Result<PrProjectFile> {
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let media = ["premiere-video-1", "premiere-video-2"]
        .map(|asset| {
            (
                asset.to_owned(),
                MediaFacts::Video(VideoMedia {
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
    tesseract_to_premiere(
        &document,
        &media,
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut Vec::new(),
    )
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
fn a_group_longer_than_its_children_is_omitted_without_losing_siblings() {
    // A longer group has no proven native nest, but must not prevent the
    // supported siblings from exporting or the caller from replacing it.
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
        1
    );
    assert!(project
        .single_sequence()
        .unwrap()
        .video_items()
        .next()
        .is_some());
}

#[test]
fn a_nest_needs_the_black_canvas_where_no_media_is_below_it() {
    // Nest 5-11 s covers 10-11 s, but a nest can be transparent, so the canvas
    // must still reach 11 s.
    let (outer, _) = nested_sequence();
    let mut document = import(&outer);
    let canvas = &mut document["composition"]["layers"][3];
    assert_eq!(canvas["name"], "Premiere black canvas");
    canvas["activeRange"]["duration"] = json!(10000);
    let error = try_export(document).unwrap_err().to_string();
    assert!(
        error.contains("gaps require an explicit bottommost opaque black canvas covering each gap"),
        "{error}"
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
