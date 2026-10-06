use crate::{
    convert::tesseract_to_premiere,
    format::FrameRate,
    image_media::{ImageFormat, ValidatedImage},
    media::{MediaFacts, VideoMedia},
    schema::{
        PrMedia, PrMediaKind, PrStaticTransform, PrVideoOccurrence, VideoCodec,
        STILL_INTRINSIC_TICKS, TICKS,
    },
    tests::support::{project_document_with_media, video_media, video_sequence},
    MediaId, Omission, OmissionKind, OmissionScope,
};
use fx_schema::EditableFxCompositionDocument;

/// A still placement's source in-point on the 30 fps test sequences.
const STILL_SOURCE_IN_TICKS: i64 = FrameRate::Fps30.generator_in_ticks();
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn still_media(name: &str, alpha: bool) -> PrMedia {
    PrMedia {
        name: name.into(),
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
            kind: PrMediaKind::Still { alpha },
        }),
        audio: None,
    }
}

fn still_occurrence(media: &str, start_secs: i64, duration_secs: i64) -> PrVideoOccurrence {
    PrVideoOccurrence {
        id: None,
        media: MediaId(media.into()),
        start_ticks: start_secs * TICKS,
        end_ticks: (start_secs + duration_secs) * TICKS,
        in_ticks: STILL_SOURCE_IN_TICKS,
        out_ticks: STILL_SOURCE_IN_TICKS + duration_secs * TICKS,
        playback_rate: 1.0,
        frame_blending: None,
        time_remap: None,
        linear_wipe: None,
        opacity_mask: None,
        track_matte: None,
        opacity: 100.0,
        blend_mode: Default::default(),
        transform: Default::default(),
        crop: Default::default(),
        animations: Vec::new(),
        enabled: true,
        effects: Vec::new(),
        effects_above_mask: 0,
        stroke: None,
        active_transforms: 0,
        source_effects: None,
    }
}

/// V1: 5 s video; V2: an opaque still placed twice and a transparent still once.
fn stills_over_video() -> (crate::format::PrSequence, BTreeMap<MediaId, PrMedia>) {
    let mut sequence = video_sequence();
    sequence
        .video_tracks
        .push(crate::schema::PrVideoTrack::media([
            still_occurrence("photo", 0, 1),
            still_occurrence("overlay", 1, 2),
            still_occurrence("photo", 3, 2),
        ]));
    let mut media = video_media();
    media.insert(MediaId("photo".into()), still_media("photo.jpg", false));
    media.insert(MediaId("overlay".into()), still_media("overlay.png", true));
    (sequence, media)
}

fn image_layers(document: &Value) -> Vec<&Value> {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Image")
        .collect()
}

#[test]
fn image_layer_edits_report_typed_losses_and_a_classic_blend_its_approximation() {
    use crate::{
        export_loss::LossCollector, ExportField, ExportLossDomain, ExportLossKind, ExportLossSource,
    };
    let (sequence, media) = stills_over_video();
    let mut value = project_document_with_media(&sequence, &media);
    let image = image_layers(&value)[0]["id"].as_u64().unwrap();
    let layer = value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|layer| layer["type"] == "Image")
        .unwrap();
    layer["blendMode"] = json!("classicColorBurn");
    layer["source"]["inputTransform"] = json!({"semanticVersion": 1, "transform": {
        "type": "lut3d",
        "assetId": "input-lut",
        "inputEncoding": "camera:apple-log:v1",
        "outputEncoding": "jerboa:sdr-rec709-display:v1",
        "interpolation": "trilinear"
    }});
    layer["source"]["timeRemap"] = json!(1.0);
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let mut collector = LossCollector::default();
    crate::convert::lower_document(
        &document,
        &packaged_facts(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut collector,
    )
    .unwrap();
    let report = collector.finish(true);
    // The classic blend exports as the nearest Premiere mode, reported as an
    // approximation rather than a lost field.
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.kind != ExportLossKind::Field(ExportField::BlendMode))
            && report.diagnostics.iter().any(|omission| omission.kind
                == crate::OmissionKind::Approximated
                && omission
                    .reason
                    .starts_with("FX ClassicColorBurn exports as the nearest")),
        "{report:?}"
    );
    for (field, domain, reason) in [
        (
            ExportField::InputTransform,
            ExportLossDomain::Picture,
            "input transform was not exported",
        ),
        (
            ExportField::TimeRemap,
            ExportLossDomain::SharedContext,
            "time remap was not exported",
        ),
    ] {
        assert!(
            report.losses.iter().any(|loss| loss.source
                == ExportLossSource::Layer(fx_schema::LayerId::new(image))
                && loss.domain == domain
                && loss.kind == ExportLossKind::Field(field)
                && loss.omission.reason == reason),
            "{field:?}: {report:?}"
        );
    }
}

#[test]
fn a_blended_still_keeps_its_blend_and_opacity_both_ways() {
    use crate::schema::PrBlendMode;
    let (mut sequence, media) = stills_over_video();
    // The transparent overlay blends as Lighter Color at half Opacity over
    // the video; the photo placements stay Normal.
    let overlay = sequence.video_tracks[1].clip_mut(1);
    (overlay.blend_mode, overlay.opacity) = (PrBlendMode::LighterColor, 50.0);
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    let blends = |layers: Vec<&Value>| -> Vec<(Value, Value)> {
        layers
            .iter()
            .map(|layer| {
                (
                    layer["blendMode"].clone(),
                    layer["transform"]["opacity"].clone(),
                )
            })
            .collect()
    };
    assert_eq!(
        blends(image_layers(&document)),
        [
            (json!("normal"), json!(100.0)),
            (json!("lighterColor"), json!(50.0)),
            (json!("normal"), json!(100.0)),
        ]
    );
    let reports = |omissions: &[Omission]| -> Vec<OmissionKind> {
        omissions
            .iter()
            .filter(|omission| omission.reason.contains("Blend Mode (12, 13)"))
            .map(|omission| omission.kind)
            .collect()
    };
    assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
    let (project, omissions) = export(document).unwrap();
    // One report, and no Normal-loss report for a blend with a code.
    assert_eq!(reports(&omissions), [OmissionKind::Approximated]);
    assert!(
        omissions
            .iter()
            .all(|omission| !omission.reason.contains("blend mode (using normal)")),
        "{omissions:?}"
    );
    let stills: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .filter(|clip| project.media(clip).is_some_and(PrMedia::is_still))
        .map(|clip| (clip.blend_mode, clip.opacity))
        .collect();
    assert_eq!(
        stills,
        [
            (PrBlendMode::Normal, 100.0),
            (PrBlendMode::LighterColor, 50.0),
            (PrBlendMode::Normal, 100.0),
        ]
    );
}

#[test]
fn still_placements_become_image_layers_sharing_one_asset_per_media_record() {
    let (sequence, media) = stills_over_video();
    let document = project_document_with_media(&sequence, &media);
    assert_eq!(document["duration"], 5.0);
    let layers = image_layers(&document);
    assert_eq!(layers.len(), 3);
    // Upper track first; placements keep timeline order within the track.
    let expected = [
        ("photo", 0, 1000, "premiere-image-2"),
        ("overlay", 1000, 2000, "premiere-image-3"),
        ("photo", 3000, 2000, "premiere-image-2"),
    ];
    for (layer, (_, start, duration, asset)) in layers.iter().zip(expected) {
        assert_eq!(
            (*crate::test_support::layer_range(layer)),
            json!({"start": start, "duration": duration})
        );
        assert!(layer.get("sourceRange").is_none());
        assert!(layer.get("sourceIntrinsicDuration").is_none());
        assert_eq!(layer["source"]["assetId"], asset);
        assert_eq!(layer["source"]["fit"], "contain");
        assert_eq!(
            layer["source"]["sourceRect"],
            json!({"x": 0.0, "y": 0.0, "width": 1920.0, "height": 1080.0})
        );
        // Default Motion: the picture's centre at the canvas centre.
        assert_eq!(layer["transform"]["anchorPoint"], json!([960.0, 540.0]));
        assert_eq!(layer["transform"]["position"], json!([960.0, 540.0]));
        assert_eq!(layer["transform"]["opacity"].as_f64(), Some(100.0));
    }
    let video = &document["composition"]["layers"][3];
    assert_eq!(video["type"], "Video");
    assert_eq!(video["source"]["assetId"], "premiere-video-1");
    assert_eq!(document["composition"]["layers"][4]["type"], "Rect");
}

/// Inspected facts of the packaged media that `stills_over_video` documents reference.
fn packaged_facts() -> BTreeMap<String, MediaFacts> {
    let still = |format, alpha| {
        MediaFacts::Still(ValidatedImage {
            format,
            width: 1920,
            height: 1080,
            pixel_aspect: Default::default(),
            open_exr_channels: None,
            alpha,
            icc_profile: false,
        })
    };
    BTreeMap::from([
        (
            "premiere-video-1".to_owned(),
            MediaFacts::Video(VideoMedia {
                pixel_aspect: Default::default(),
                orientation: crate::schema::VideoOrientation::Identity,
                codec: VideoCodec::H264,
                bit_depth: 8,
                colour: None,
                width: 1920,
                height: 1080,
                timing: crate::media::VideoTiming::for_test(FrameRate::Fps30, 10 * TICKS),
            }),
        ),
        (
            "premiere-image-2".to_owned(),
            still(ImageFormat::Jpeg, false),
        ),
        ("premiere-image-3".to_owned(), still(ImageFormat::Png, true)),
    ])
}

fn export(document: Value) -> crate::error::Result<(crate::format::PrProjectFile, Vec<Omission>)> {
    let document = EditableFxCompositionDocument::from_json_value(document).unwrap();
    let mut omissions = Vec::new();
    let project = tesseract_to_premiere(
        &document,
        &packaged_facts(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )?;
    Ok((project, omissions))
}

#[test]
fn crop_guides_never_paint_and_reenabled_stills_export_edited_crop() {
    for enabled in [false, true] {
        let (mut sequence, media) = stills_over_video();
        let clip = sequence.video_tracks[1].clip_mut(0);
        clip.enabled = enabled;
        clip.crop = crate::schema::PrStaticCrop {
            left: 25.0,
            right: 12.5,
            ..Default::default()
        };
        let mut document = project_document_with_media(&sequence, &media);
        let layers = document["composition"]["layers"].as_array_mut().unwrap();
        let owner_index = layers
            .iter()
            .position(|layer| layer["type"] == "Image" && layer["activeRange"]["start"] == 0)
            .unwrap();
        let owner = &layers[owner_index];
        let guide_id = owner["masks"][0]["layer"].clone();
        assert_eq!(owner["isHidden"].as_bool().unwrap_or(false), !enabled);
        let range = owner["activeRange"].clone();
        let transform = owner["transform"].clone();
        let guide = layers
            .iter_mut()
            .find(|layer| layer["id"] == guide_id)
            .unwrap();
        assert_eq!(guide["type"], "Rect");
        assert!(!guide["isHidden"].as_bool().unwrap_or(false));
        assert_eq!(guide["rect"]["fillEnabled"], false);
        assert_eq!(guide["rect"]["strokeEnabled"], false);
        assert_eq!(guide["activeRange"], range);
        assert_eq!(guide["transform"], transform);
        assert_eq!(guide["rect"]["position"], json!([480.0, 0.0]));
        assert_eq!(guide["rect"]["size"], json!([1200.0, 1080.0]));
        // Edit the live outline, not source replay data. Re-enabling only the
        // owner must keep this nonhidden guide usable by its existing mask.
        guide["rect"]["size"] = json!([960.0, 1080.0]);
        let (exported, omissions) = export(document.clone()).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let clip = exported
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .find(|clip| clip.media.as_str() == "premiere-image-2" && clip.start_ticks == 0)
            .unwrap();
        assert_eq!(clip.enabled, enabled);
        assert_eq!(clip.crop.left, 25.0);
        assert_eq!(clip.crop.right, 25.0);
        assert_eq!(clip.end_ticks, TICKS);

        // Visible is the canonical omitted default in the editable wire form.
        document["composition"]["layers"][owner_index]
            .as_object_mut()
            .unwrap()
            .remove("isHidden");
        let (exported, omissions) = export(document).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let clip = exported
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .find(|clip| clip.media.as_str() == "premiere-image-2" && clip.start_ticks == 0)
            .unwrap();
        assert!(clip.enabled);
        assert_eq!(clip.crop.left, 25.0);
        assert_eq!(clip.crop.right, 25.0);
        assert_eq!(clip.end_ticks, TICKS);
    }
}

#[test]
fn image_layers_export_as_still_occurrences_on_the_synthetic_still_clock() {
    // Author the editable input independently of the Premiere importer.
    let mut document = crate::test_support::editable_document();
    document["duration"] = json!(5);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 0, "duration": 5000}),
        json!({"start": 0, "duration": 5000}),
    );
    layers[0]["sourceRange"] = json!({"start": 0, "duration": 5000});
    layers[0]["sourceIntrinsicDuration"] = json!(10000);
    layers[1]["activeRange"] = json!({"start": 0, "duration": 5000});
    for (id, asset, start, duration) in [
        (3, "premiere-image-2", 0, 1000),
        (4, "premiere-image-3", 1000, 2000),
        (5, "premiere-image-2", 3000, 2000),
    ] {
        layers.insert(
            0,
            json!({
                "type": "Image", "id": id, "name": "Authored image",
                "transform": {
                    "anchorPoint": [960, 540], "position": [960, 540], "scale": [100, 100],
                    "rotation": 0, "opacity": 100
                },
                "activeRange": {"start": start, "duration": duration},
                "source": {
                    "assetId": asset, "fit": "contain",
                    "sourceRect": {"x": 0, "y": 0, "width": 1920, "height": 1080}
                }
            }),
        );
    }
    let (exported, omissions) = export(document).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = exported.single_sequence().unwrap();
    // A canvas still centred at Scale 100 exports Premiere's default Motion.
    for clip in sequence
        .video_occurrences()
        .filter(|clip| clip.media.as_str().starts_with("premiere-image-"))
    {
        assert_eq!(clip.transform, PrStaticTransform::default());
        assert_eq!(clip.opacity, 100.0);
        assert!(clip.animations.is_empty());
    }
    let tracks: Vec<Vec<_>> = sequence
        .video_tracks()
        .map(|track| {
            track
                .iter()
                .map(|item| {
                    let clip = item.media().unwrap();
                    (
                        clip.media.as_str().to_owned(),
                        clip.timeline_ticks(),
                        clip.source_ticks(),
                    )
                })
                .collect()
        })
        .collect();
    assert_eq!(
        tracks,
        [
            vec![("premiere-video-1".to_owned(), 0..5 * TICKS, 0..5 * TICKS)],
            vec![
                (
                    "premiere-image-2".to_owned(),
                    0..TICKS,
                    STILL_SOURCE_IN_TICKS..STILL_SOURCE_IN_TICKS + TICKS
                ),
                (
                    "premiere-image-3".to_owned(),
                    TICKS..3 * TICKS,
                    STILL_SOURCE_IN_TICKS..STILL_SOURCE_IN_TICKS + 2 * TICKS
                ),
                (
                    "premiere-image-2".to_owned(),
                    3 * TICKS..5 * TICKS,
                    STILL_SOURCE_IN_TICKS..STILL_SOURCE_IN_TICKS + 2 * TICKS
                ),
            ],
        ]
    );
    assert_eq!(exported.media.len(), 3);
    // Alpha comes from the inspected packaged image.
    for (id, alpha) in [("premiere-image-2", false), ("premiere-image-3", true)] {
        let media = &exported.media[&MediaId(id.into())];
        assert_eq!(
            media.video.as_ref().unwrap().kind,
            PrMediaKind::Still { alpha }
        );
        assert_eq!(
            media.video.as_ref().unwrap().intrinsic_ticks,
            STILL_INTRINSIC_TICKS
        );
    }
    assert_eq!(
        exported.media[&MediaId("premiere-video-1".into())]
            .video
            .as_ref()
            .unwrap()
            .kind,
        PrMediaKind::Video {
            codec: Some(VideoCodec::H264),
            hdr_profile: None,
        }
    );
}

#[test]
fn edited_image_layer_properties_are_omitted_with_layer_context() {
    let (sequence, media) = stills_over_video();
    let base = project_document_with_media(&sequence, &media);
    let image_index = base["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .position(|layer| layer["type"] == "Image")
        .unwrap();
    let report = |kind, scope, reason: &str| Omission {
        scope,
        kind,
        record: "layer 2 (\"Premiere still 2\")".to_owned(),
        reason: reason.to_owned(),
    };
    type Edit = Box<dyn Fn(&mut Value)>;
    let edits: [(&str, Edit, Option<Omission>); 5] = [
        (
            "description",
            Box::new(|layer| layer["description"] = json!("note")),
            Some(report(
                OmissionKind::Omitted,
                OmissionScope::Feature,
                "description was not exported",
            )),
        ),
        (
            "skew",
            Box::new(|layer| layer["transform"]["skew"] = json!(10)),
            Some(report(
                OmissionKind::Omitted,
                OmissionScope::Feature,
                "skew was not exported",
            )),
        ),
        // A clip draws a still at its pixel size. Retain the full image when
        // the source framing cannot map exactly, and report the approximation.
        (
            "fit",
            Box::new(|layer| layer["source"]["fit"] = json!("cover")),
            Some(report(
                OmissionKind::Approximated,
                OmissionScope::Feature,
                "media fit Cover was approximated by retaining the full packaged image and existing Motion; framing can differ",
            )),
        ),
        (
            "frame",
            Box::new(|layer| layer["source"]["sourceRect"]["width"] = json!(960)),
            Some(report(
                OmissionKind::Approximated,
                OmissionScope::Feature,
                "sourceRect was not the 1920x1080 image at the origin; the full packaged image was retained, so crop and placement can differ",
            )),
        ),
        (
            "natural frame",
            Box::new(|layer| {
                layer["source"]
                    .as_object_mut()
                    .unwrap()
                    .remove("sourceRect");
            }),
            None,
        ),
    ];
    for (name, edit, expected) in edits {
        let mut document = base.clone();
        edit(&mut document["composition"]["layers"][image_index]);
        let (exported, omissions) = export(document).unwrap();
        let omitted = expected
            .as_ref()
            .is_some_and(|omission| omission.scope == OmissionScope::Occurrence);
        assert_eq!(omissions, Vec::from_iter(expected), "{name}");
        let stills = exported
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .filter(|clip| clip.media.as_str().starts_with("premiere-image-"))
            .count();
        assert_eq!(stills, 3 - usize::from(omitted), "{name}");
    }
}

#[test]
fn a_still_whose_scale_or_rotation_motion_cannot_show_is_omitted() {
    // A still takes a video clip's Motion bounds: Scale 0 to 10000 without a
    // flip, Rotation -32768 to 32767. (fields of the first still's static
    // transform, the value its Scale keys reach from 100 at 500 ms, and the
    // reason that omits it, or "" when it exports unchanged)
    let (sequence, media) = stills_over_video();
    let base = project_document_with_media(&sequence, &media);
    let index = base["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .position(|layer| layer["type"] == "Image")
        .unwrap();
    #[rustfmt::skip]
    let rows = [
        (json!({"scale": [-100.0, 100.0]}), None, "flip (negative scale) was not exported"),
        (json!({}), Some(20000.0), "Scale animation exceeds Premiere's supported range"),
        (json!({"rotation": 40000.0}), None, "static rotation exceeds Premiere's supported range"),
        (json!({"scale": [10000.0, 10000.0], "rotation": -32768.0}), None, ""),
    ];
    for (fields, key, reason) in rows {
        let mut document = base.clone();
        let transform = &mut document["composition"]["layers"][index]["transform"];
        for (field, value) in fields.as_object().unwrap() {
            transform[field] = value.clone();
        }
        if let Some(key) = key {
            let entries = ["scaleX", "scaleY"].map(|property| {
                let keys = [(0, 100.0), (500, key)].map(|(time, value)| {
                    json!({"id": format!("{property}-{time}"), "layerTime": time, "value": {"type": "float", "value": value}, "easing": {"type": "linear"}})
                });
                json!({
                    "target": {"kind": "layer", "layerId": 2, "propertyType": property},
                    "animator": {"type": "keyframes", "enabled": true, "keyframes": keys}
                })
            });
            document["composition"]["dynamics"] = json!({ "entries": entries });
        }
        let case = format!("{fields} {key:?}");
        // Media inspection skips the still that export omits.
        let parsed = EditableFxCompositionDocument::from_json_value(document.clone()).unwrap();
        let composition = parsed.composition();
        let dimensions = parsed.dimensions();
        let inspected = crate::convert::exported_image_layers(
            composition.layers(),
            composition.dynamics(),
            [dimensions.width, dimensions.height],
        )
        .count();
        assert_eq!(inspected, 3, "{case}");
        let (exported, omissions) = export(document).unwrap();
        let stills: Vec<_> = exported
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .filter(|clip| clip.media.as_str().starts_with("premiere-image-"))
            .collect();
        if reason.is_empty() {
            // Premiere's bounds themselves export unchanged.
            assert!(omissions.is_empty(), "{case}: {omissions:?}");
            assert_eq!(stills[0].start_ticks, 0);
            assert_eq!(stills[0].transform.scale, [10000.0, 10000.0]);
            assert_eq!(stills[0].transform.rotation, -32768.0);
            continue;
        }
        assert_eq!(
            stills
                .iter()
                .map(|clip| clip.start_ticks)
                .collect::<Vec<_>>(),
            [0, TICKS, 3 * TICKS],
            "{case}"
        );
        assert!(stills
            .iter()
            .all(|clip| exported.media.contains_key(&clip.media)));
        assert!(
            omissions
                .iter()
                .any(|item| item.scope == OmissionScope::Feature
                    && item.record.starts_with("layer 2")),
            "{omissions:?}"
        );
        assert!(stills[0]
            .transform
            .scale
            .iter()
            .all(|value| (0.0..=10000.0).contains(value)));
        assert!((-32768.0..=32767.0).contains(&stills[0].transform.rotation));
    }
}

#[test]
fn still_placements_fit_the_eleven_hours_after_the_one_hour_in_point() {
    let (sequence, media) = stills_over_video();
    let base = project_document_with_media(&sequence, &media);
    let index = base["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .position(|layer| layer["type"] == "Image")
        .unwrap();
    let with_duration = |duration_ms: i64| {
        let mut document = base.clone();
        document["composition"]["layers"][index]["activeRange"]["duration"] = json!(duration_ms);
        document
    };
    let eleven_hours_ms = 39_600_000;
    let (exported, _) = export(with_duration(eleven_hours_ms)).unwrap();
    let longest = exported
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .max_by_key(|clip| clip.out_ticks)
        .unwrap();
    assert_eq!(
        longest.out_ticks,
        exported.media[&longest.media]
            .video
            .as_ref()
            .unwrap()
            .intrinsic_ticks
    );
    let error = export(with_duration(eleven_hours_ms + 1000))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Premiere still")
            && error.contains("exceeds Premiere's twelve-hour still duration"),
        "{error}"
    );
}

#[test]
fn a_still_lengthened_past_its_source_span_shows_for_its_placement_on_its_source_clock() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    // Premiere 26.5.1 keeps a still's 5 s span on a lengthened 10 s
    // placement. An Opacity key past that saved Out keeps its source time.
    let mut still = still_occurrence("photo", 0, 10);
    still.out_ticks = STILL_SOURCE_IN_TICKS + 5 * TICKS;
    still.animations = vec![PrPropertyAnimation::Opacity(
        [(0, 100.0), (8, 40.0)]
            .map(|(seconds, value)| PrScalarKeyframe {
                source_ticks: STILL_SOURCE_IN_TICKS + seconds * TICKS,
                value,
                easing: PrKeyframeEasing::Linear,
            })
            .to_vec(),
    )];
    let mut sequence = video_sequence();
    sequence
        .video_tracks
        .push(crate::schema::PrVideoTrack::media([still]));
    sequence.timeline_end_ticks = 10 * TICKS;
    let mut media = video_media();
    media.insert(MediaId("photo".into()), still_media("photo.jpg", false));
    sequence.validate_timeline(&media).unwrap();
    let document = project_document_with_media(&sequence, &media);
    let [image] = image_layers(&document)[..] else {
        panic!("one image: {document}");
    };
    assert_eq!(image["activeRange"], json!({"start": 0, "duration": 10000}));
    let keys: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == image["id"])
        .flat_map(|entry| entry["animator"]["keyframes"].as_array().unwrap())
        .map(|key| (key["layerTime"].clone(), key["value"]["value"].clone()))
        .collect();
    assert_eq!(keys, [(json!(0), json!(100.0)), (json!(8000), json!(40.0))]);
}

#[test]
fn one_asset_cannot_be_both_video_and_still() {
    let (sequence, media) = stills_over_video();
    let base = project_document_with_media(&sequence, &media);
    let position = |kind: &str| {
        base["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .position(|layer| layer["type"] == kind)
            .unwrap()
    };
    // Inspection keeps one kind per asset, so the other layer kind cannot reuse it.
    for (layer, asset) in [
        (position("Image"), "premiere-video-1"),
        (position("Video"), "premiere-image-2"),
    ] {
        let mut document = base.clone();
        document["composition"]["layers"][layer]["source"]["assetId"] = json!(asset);
        let error = export(document).unwrap_err().to_string();
        assert!(
            error.contains(&format!(
                "asset {asset:?} is used with conflicting media kinds across layers"
            )),
            "{error}"
        );
    }
}

#[test]
fn legacy_luma_key_bypassed_matte_still_keeps_its_exportable_consumer() {
    use crate::schema::{PrEffect, PrEffectParams, PrMatteChannel, PrTrackMatte, PrVideoTrack};
    let mut sequence = video_sequence();
    sequence.video_tracks[0].clip_mut(0).track_matte = Some(PrTrackMatte {
        track_index: 1,
        channel: PrMatteChannel::Alpha,
    });
    let mut still = still_occurrence("photo", 0, 5);
    still.effects.push(PrEffect {
        mask: None,
        enabled: false,
        params: PrEffectParams::LegacyLuma {
            threshold: 40.0,
            cutoff: 20.0,
        },
        animations: Vec::new(),
    });
    sequence.video_tracks.push(PrVideoTrack::media([still]));
    let mut media = video_media();
    media.insert(MediaId("photo".into()), still_media("photo.jpg", false));
    let ids = crate::tesseract_output::asset_ids_in_order(&sequence, &media);
    let mut omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(&sequence, &media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    assert!(image_layers(&document)[0].get("effects").is_none());
    assert!(
        omissions
            .iter()
            .any(|note| note.reason.contains("bypassed Luma Key")
                && note.reason.contains("still is a Track Matte Key's matte")),
        "{omissions:?}"
    );
    let (project, notes) = export(document).unwrap();
    assert!(project.single_sequence().unwrap().video_tracks.iter()
        .flat_map(|track| &track.items)
        .any(|item| matches!(item, crate::schema::PrVideoItem::Media(clip) if clip.track_matte.is_some())), "{notes:?}");
}
