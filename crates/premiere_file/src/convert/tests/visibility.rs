//! Clip Enable ↔ `isHidden` in both mapping directions.
use crate::{
    convert::{premiere_to_tesseract, tesseract_to_premiere},
    format::FrameRate,
    image_media::{ImageFormat, ValidatedImage},
    media::{MediaFacts, VideoMedia},
    schema::{
        PrColorMatte, PrKeyframeEasing, PrLinearWipe, PrMediaKind, PrScalarKeyframe, PrVideoTrack,
        TICKS, TICKS_PER_MILLISECOND,
    },
    test_support::editable_document,
    tests::support::{project_document, video_media, video_sequence},
    Omission,
};
use fx_schema::EditableFxCompositionDocument;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Exports against inspected facts for the one 30 fps source, `premiere-video-1`,
/// and one opaque canvas-sized PNG, `premiere-image-2`.
fn convert(
    wire: Value,
    source_millis: i64,
) -> crate::error::Result<(crate::format::PrProjectFile, Vec<Omission>)> {
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let media = BTreeMap::from([
        (
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
                    source_millis * TICKS_PER_MILLISECOND,
                ),
            }),
        ),
        (
            "premiere-image-2".to_owned(),
            MediaFacts::Still(ValidatedImage {
                format: ImageFormat::Png,
                width: 1920,
                height: 1080,
                alpha: false,
                icc_profile: false,
            }),
        ),
    ]);
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &media,
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )?;
    Ok((project, omissions))
}

/// Enabled lower clip 0–5 s plus a disabled upper placement 1–3 s of source 6–8 s.
fn sequence_with_disabled_upper_clip() -> crate::format::PrSequence {
    let mut sequence = video_sequence();
    let mut hidden = sequence.video_tracks[0].clip(0).clone();
    hidden.start_ticks = TICKS;
    hidden.end_ticks = 3 * TICKS;
    hidden.in_ticks = 6 * TICKS;
    hidden.out_ticks = 8 * TICKS;
    hidden.enabled = false;
    sequence.video_tracks.push(PrVideoTrack {
        transitions: Vec::new(),
        items: vec![crate::schema::PrVideoItem::Media(hidden)],
        nests: Vec::new(),
    });
    sequence
}

#[test]
fn disabled_clip_becomes_a_hidden_layer_that_stays_editable() {
    let document = project_document(&sequence_with_disabled_upper_clip());
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3);
    let hidden = &layers[0];
    let visible = &layers[1];
    assert_eq!(hidden["isHidden"], json!(true));
    assert_ne!(visible["isHidden"], json!(true));
    assert_eq!(hidden["type"], "Video");
    assert_eq!(hidden["source"]["assetId"], visible["source"]["assetId"]);
    assert_eq!(
        (*crate::test_support::layer_range(hidden)),
        json!({"start": 1000, "duration": 2000})
    );
    assert_eq!(
        hidden["sourceRange"],
        json!({"start": 6000, "duration": 2000})
    );
    assert_eq!(hidden["transform"], visible["transform"]);
    let canvas = &layers[2];
    assert_eq!(canvas["type"], "Rect");
    assert_eq!(
        (*crate::test_support::layer_range(canvas)),
        json!({"start": 0, "duration": 5000})
    );
}

#[test]
fn disabled_clip_import_preserves_constant_speed_and_cardinal_wipe() {
    for with_wipe in [false, true] {
        let mut sequence = video_sequence();
        let clip = sequence.video_tracks[0].clip_mut(0);
        clip.enabled = false;
        if with_wipe {
            clip.linear_wipe = Some(PrLinearWipe {
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
        } else {
            clip.out_ticks = 10 * TICKS;
            clip.playback_rate = 2.0;
        }
        let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &video_media());
        let mut omissions = Vec::new();
        let document = premiere_to_tesseract(&sequence, &video_media(), &ids, &mut omissions)
            .unwrap()
            .to_json_value()
            .unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let layers = document["composition"]["layers"].as_array().unwrap();
        let video = layers
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        assert_eq!(video["isHidden"], true);
        if with_wipe {
            let guide = layers
                .iter()
                .find(|layer| layer["id"] == video["masks"][0]["layer"])
                .unwrap();
            assert_ne!(guide["isHidden"], true);
            assert_eq!(guide["transform"]["scale"], json!([0.0, 100.0]));
            assert_eq!(video["masks"][0]["feather"], json!([5.0, 5.0]));
            let track = &document["composition"]["dynamics"]["entries"][0];
            assert_eq!(track["target"]["propertyType"], "scaleX");
            assert_eq!(track["animator"]["keyframes"][1]["layerTime"], 1000);
            assert_eq!(track["animator"]["keyframes"][1]["value"]["value"], 100.0);
        } else {
            assert_eq!(video["sourceRange"]["duration"], 10000);
            assert_eq!(crate::tests::support::playback_keys(video)[1]["time"], 5000);
            assert_eq!(
                crate::tests::support::playback_keys(video)[1]["value"],
                10000
            );
        }
    }
}

#[test]
fn hidden_layer_export_preserves_constant_speed_and_cardinal_wipe() {
    for with_wipe in [false, true] {
        let mut wire = editable_document();
        let video = &mut wire["composition"]["layers"][0];
        video["isHidden"] = json!(true);
        if with_wipe {
            video["masks"] = json!([{
                "id": 4, "mode": "add", "layer": 3, "feather": [5, 5],
                "inverted": false, "expansion": 0, "opacity": 1
            }]);
            let mut guide = wire["composition"]["layers"][1].clone();
            guide["id"] = json!(3);
            guide["name"] = json!("Premiere Linear Wipe guide 1");
            guide["transform"]["scale"] = json!([0, 100]);
            wire["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .insert(0, guide);
            wire["composition"]["dynamics"] = json!({"entries": [{
                "target": {"kind": "layer", "layerId": 3, "propertyType": "scaleX"},
                "animator": {"type": "keyframes", "enabled": true, "keyframes": [
                    {"id": "start", "layerTime": 0, "value": {"type": "float", "value": 0}, "easing": {"type": "linear"}},
                    {"id": "end", "layerTime": 1000, "value": {"type": "float", "value": 100}, "easing": {"type": "linear"}}
                ]}
            }]});
        } else {
            video["sourceIntrinsicDuration"] = json!(2000);
            video["sourceRange"]["duration"] = json!(2000);
            video["playback"] = crate::test_support::remapped_playback(
                crate::test_support::layer_range(video).clone(),
                json!({
                    "keyframes": [
                        {"id": "start", "time": 0, "value": 0, "easing": {"type": "linear"}},
                        {"id": "end", "time": 1000, "value": 2000, "easing": {"type": "linear"}}
                    ],
                    "before": "inactive", "after": "inactive"
                }),
            );
        }
        let (project, omissions) = convert(wire, if with_wipe { 1000 } else { 2000 }).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert!(!clip.enabled);
        if with_wipe {
            let wipe = clip.linear_wipe.as_ref().unwrap();
            assert_eq!(wipe.angle_degrees, 270);
            assert_eq!(wipe.feather, 5.0);
            assert_eq!(wipe.initial_completion, 100.0);
            assert_eq!(
                wipe.completion
                    .iter()
                    .map(|key| (key.source_ticks, key.value))
                    .collect::<Vec<_>>(),
                [(0, 100.0), (TICKS, 0.0)]
            );
        } else {
            assert_eq!(clip.playback_rate, 2.0);
            assert_eq!(clip.source_ticks(), 0..2 * TICKS);
        }
    }
}

#[test]
fn disabled_only_span_imports_as_black_canvas_time() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.end_ticks = 2 * TICKS;
    clip.out_ticks = 2 * TICKS;
    let mut trailing = clip.clone();
    trailing.start_ticks = 2 * TICKS;
    trailing.end_ticks = 5 * TICKS;
    trailing.out_ticks = 3 * TICKS;
    trailing.enabled = false;
    sequence.video_tracks[0]
        .items
        .push(crate::schema::PrVideoItem::Media(trailing));
    let document = project_document(&sequence);
    assert_eq!(document["duration"], 5.0);
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(layers[1]["isHidden"], json!(true));
    assert_eq!(
        (*crate::test_support::layer_range(&layers[2])),
        json!({"start": 0, "duration": 5000})
    );
}

#[test]
fn hidden_fx_video_layer_exports_as_a_disabled_clip() {
    let mut wire = editable_document();
    wire["composition"]["layers"][0]["isHidden"] = json!(true);
    let (project, omissions) = convert(wire, 1000).unwrap();
    // Exported muted, not omitted: no omission is reported for the hidden layer.
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert!(!clip.enabled);
    assert_eq!(clip.timeline_ticks(), 0..TICKS);
    assert_eq!(clip.source_ticks(), 0..TICKS);
    assert_eq!(sequence.end_ticks(), TICKS);
}

#[test]
fn hidden_only_span_exports_without_the_black_canvas() {
    let mut wire = editable_document();
    wire["composition"]["layers"][0]["isHidden"] = json!(true);
    wire["composition"]["layers"].as_array_mut().unwrap().pop();
    let (project, omissions) = convert(wire, 1000).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert!(!clip.enabled);
    assert_eq!(clip.timeline_ticks(), 0..TICKS);
    assert_eq!(clip.source_ticks(), 0..TICKS);
    assert_eq!(sequence.end_ticks(), TICKS);
    assert_eq!(
        sequence.gaps(&project.media),
        Vec::from_iter(Some(0..TICKS))
    );
}

#[test]
fn disabled_clip_round_trips_with_ranges_and_shared_media() {
    let before = sequence_with_disabled_upper_clip();
    let (project, _) = convert(project_document(&before), 10_000).unwrap();
    let after = project.single_sequence().unwrap();
    let table = |sequence: &crate::format::PrSequence| {
        let mut rows: Vec<_> = sequence
            .video_occurrences()
            .map(|clip| (clip.timeline_ticks(), clip.source_ticks(), clip.enabled))
            .collect();
        rows.sort_by_key(|row| row.0.start);
        rows
    };
    assert_eq!(table(after), table(&before));
    assert_eq!(project.media.len(), 1);
    assert_eq!(after.video_tracks().len(), 2);
}

/// Imports the support sequence's one clip with Enable off and its media re-typed as `kind`.
fn import_disabled_clip_as(kind: PrMediaKind) -> (Value, Vec<Omission>) {
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).enabled = false;
    let mut media = video_media();
    for source in media.values_mut() {
        source.video.as_mut().unwrap().kind = kind;
    }
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    (document, omissions)
}

#[test]
fn disabled_still_imports_as_a_hidden_image_layer() {
    let (document, omissions) = import_disabled_clip_as(PrMediaKind::Still { alpha: false });
    assert!(omissions.is_empty(), "{omissions:?}");
    let still = &document["composition"]["layers"][0];
    assert_eq!(still["type"], "Image");
    assert_eq!(still["isHidden"], true);
}

#[test]
fn disabled_color_matte_imports_as_a_hidden_rect_layer() {
    let (document, omissions) =
        import_disabled_clip_as(PrMediaKind::ColorMatte(PrColorMatte { rgb: [255, 0, 0] }));
    assert!(omissions.is_empty(), "{omissions:?}");
    let matte = &document["composition"]["layers"][0];
    assert_eq!(matte["name"], "Premiere color matte 1");
    assert_eq!(matte["isHidden"], true);
}

/// Exports the shared document with `layer` inserted hidden on top, returning
/// each exported occurrence's media and enabled state.
fn export_hidden_layer(mut layer: Value) -> Vec<(String, bool)> {
    layer["isHidden"] = json!(true);
    let mut wire = editable_document();
    wire["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, layer);
    let (project, omissions) = convert(wire, 1000).unwrap();
    // Exported disabled, not omitted: no omission is reported for the hidden layer.
    assert!(omissions.is_empty(), "{omissions:?}");
    project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .map(|clip| (clip.media.as_str().to_owned(), clip.enabled))
        .collect()
}

#[test]
fn hidden_image_layer_exports_as_a_disabled_still() {
    let image = json!({
        "type": "Image", "id": 3, "name": "Authored image",
        "transform": {
            "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
            "rotation": 0, "opacity": 100
        },
        "activeRange": {"start": 0, "duration": 1000},
        "source": {
            "assetId": "premiere-image-2", "fit": "contain",
            "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}
        }
    });
    assert_eq!(
        export_hidden_layer(image),
        [
            ("premiere-video-1".to_owned(), true),
            ("premiere-image-2".to_owned(), false)
        ]
    );
}

#[test]
fn hidden_solid_rectangle_exports_as_a_disabled_color_matte() {
    let solid = json!({
        "type": "Rect", "id": 3, "name": "Red solid",
        "transform": {
            "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
            "rotation": 0, "opacity": 100
        },
        "activeRange": {"start": 0, "duration": 1000},
        "rect": {"size": [1920, 1080], "fillColor": [1, 0, 0, 1]}
    });
    assert_eq!(
        export_hidden_layer(solid),
        [
            ("premiere-video-1".to_owned(), true),
            ("color-matte:ff0000".to_owned(), false)
        ]
    );
}
