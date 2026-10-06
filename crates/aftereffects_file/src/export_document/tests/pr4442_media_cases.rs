//! PR #4442 media/source export cases.
//!
//! These cases author explicit current FX inputs and inspect a freshly encoded
//! AEP with this crate's structural reader. That inspection is supplementary:
//! it is not independent Adobe acceptance or render/audio proof.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use fx_conv::{ConversionMode, ExportFromTesseract};
use fx_schema::{
    EditableFxCompositionDocument, KeyframeId, Layer, LayerId, PropType, PropertyKeyframeEasing,
    PropertyTarget, PropertyValue, TimeOffset,
    animator::{
        AnimationGraphEntry, AnimatorData, PropertyAnimator, PropertyKeyframe,
        PropertyKeyframeTrack,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::*;
use crate::{
    AfterEffects,
    media::MediaKind,
    properties::{read_transform, root_runs, runs},
    writer::footage::{NativeFrameRate, NativeSourceFormat, RelativeMediaPath},
};

mod p001_video_compositing;
mod remapped_opacity;
mod rounded_visibility;

fn document(
    layers: Vec<Value>,
    entries: Vec<AnimationGraphEntry>,
) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": entries});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn transform() -> Value {
    serde_json::to_value(identity_fx_transform()).unwrap()
}

fn image(id: u64, asset: &str) -> Value {
    json!({
        "type":"Image", "id":id, "name":format!("image-{id}"), "parent":null,
        "activeRange":{"start":0,"duration":2000}, "transform":transform(),
        "source":{"assetId":asset,"fit":"contain"}
    })
}

fn video(id: u64, asset: &str) -> Value {
    json!({
        "type":"Video", "id":id, "name":format!("video-{id}"), "parent":null,
        "activeRange":{"start":1000,"duration":2000},
        "sourceRange":{"start":500,"duration":1000},
        "sourceIntrinsicDuration":4000, "transform":transform(),
        "source":{"assetId":asset,"fit":"contain"}
    })
}

fn audio(id: u64, asset: &str, intrinsic: u64) -> Value {
    json!({
        "type":"Audio", "id":id, "name":format!("audio-{id}"), "parent":null,
        "activeRange":{"start":1000,"duration":2000},
        "sourceRange":{"start":500,"duration":1000},
        "sourceIntrinsicDuration":intrinsic, "volume":1.0,
        "source":{"assetId":asset}
    })
}

fn group(id: u64, name: &str, children: Vec<Value>) -> Value {
    let mut value = imported()["composition"]["layers"][0].clone();
    value["id"] = json!(id);
    value["name"] = json!(name);
    value["parent"] = Value::Null;
    value["activeRange"] = json!({"start":0,"duration":30_000});
    value["transform"] = transform();
    value["masks"] = json!([]);
    value["layers"] = Value::Array(children);
    value
}

fn mask_atom_count(layer: &crate::structure::Layer) -> usize {
    root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Mask Parade")
        .map_or(0, |(_, parade)| {
            runs(parade)
                .unwrap()
                .into_iter()
                .filter(|(name, _)| *name == "ADBE Mask Atom")
                .count()
        })
}

fn resolved(
    asset: &str,
    path: &str,
    format: NativeSourceFormat,
    dimensions: [u16; 2],
    duration_millis: u64,
    frame_rate: NativeFrameRate,
    audio_sample_rate: f64,
) -> media::ResolvedMediaSource {
    media::ResolvedMediaSource {
        asset_id: fx_schema::AssetId::new(asset).unwrap(),
        path: RelativeMediaPath::new(path).unwrap(),
        format,
        dimensions,
        duration_millis,
        duration_millis_floor: duration_millis,
        duration_native_ticks: None,
        frame_rate,
        audio_sample_rate,
        wave_metadata: None,
        native_duration: None,
    }
}

#[test]
fn exact_video_duration_quantization_preserves_authored_clock() {
    for (intrinsic, floor, ceil, accepted) in [
        (1433, 1433, 1434, true),
        (1434, 1433, 1434, true),
        (1432, 1433, 1434, false),
        (1435, 1433, 1434, false),
        (1433, 1432, 1434, false),
        (1434, 1435, 1434, false),
    ] {
        let mut input = video(400572, "stripe");
        input["sourceIntrinsicDuration"] = json!(intrinsic);
        input.as_object_mut().unwrap().remove("activeRange");
        input["sourceRange"] = json!({"start":250,"duration":1000});
        input["playback"] = json!({
            "type":"windowed", "inputRange":{"start":1000,"duration":1000},
            "mapping":{"type":"timeRemap", "property":{
                "keyframes":[
                    {"id":"start","time":1000,"value":250,"easing":{"type":"linear"}},
                    {"id":"end","time":2000,"value":1000,"easing":{"type":"linear"}}
                ], "before":"inactive", "after":"inactive"
            }},
            "inputOffsetMs":0
        });
        let mut invalid = input.clone();
        invalid["sourceIntrinsicDuration"] = json!(0);
        let mut invalid_document = imported();
        invalid_document["composition"]["layers"] = json!([invalid]);
        assert!(EditableFxCompositionDocument::from_json_value(invalid_document).is_err());

        let doc = document(vec![input], Vec::new());
        let mut source = resolved(
            "stripe",
            "media/stripe.mov",
            NativeSourceFormat::QuickTime,
            [1920, 1080],
            ceil,
            NativeFrameRate::integer(30),
            0.0,
        );
        source.duration_millis_floor = floor;
        let sources = BTreeMap::from([("stripe".into(), source)]);
        let output = to_aep_with_media(&doc, &sources).unwrap();
        let native = read_project(&output.bytes).unwrap();
        if accepted {
            let native_layers = layers(&native);
            let owner = native_layers
                .iter()
                .find(|owner| owner.name.as_ref() == "video-400572")
                .unwrap_or_else(|| panic!("{intrinsic}: {:?}", output.diagnostics));
            let remap = super::stroke_keys::numeric(&owner.content, "ADBE Time Remapping")
                .expect("authored source clock retained");
            assert!(!remap.keyframes.is_empty());
            assert_eq!(sources["stripe"].duration_millis, 1434);
        } else {
            assert!(layers(&native).is_empty());
            assert!(output.diagnostics.iter().any(|diagnostic| {
                diagnostic.layer_id == Some(LayerId::new(400572))
                    && diagnostic.message.contains("intrinsic duration differs")
            }));
        }
    }
}

fn property_values(layer: &crate::structure::Layer, name: &str) -> Vec<f64> {
    read_transform(&layer.content)
        .unwrap()
        .into_iter()
        .find(|property| property.match_name == name)
        .unwrap()
        .numeric
        .unwrap()
        .values
}

fn synthetic_exr(width: i32, height: i32) -> Vec<u8> {
    let mut bytes = 20_000_630_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(b"dataWindow\0box2i\0");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&0_i32.to_le_bytes());
    bytes.extend_from_slice(&(width - 1).to_le_bytes());
    bytes.extend_from_slice(&(height - 1).to_le_bytes());
    bytes
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn fresh_aep_has_exact_typed_exr_wave_quicktime_descriptors_and_shared_av_source() {
    let native_exr_bytes = include_bytes!("../../../tests/fixtures/media/footage_not_missing.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(native_exr_bytes)),
        "692fa37886a98a8deb63addda6da0193d6f9cc4c6725f42e13a424d0e8bd9104"
    );
    let native_exr = read_project(native_exr_bytes).unwrap();
    let native_exr_source = native_exr
        .item(1)
        .unwrap()
        .media
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(native_exr_source.source_format, *b"oEXR");
    assert_eq!(native_exr_source.kind, MediaKind::StillImage);
    assert_eq!(
        (native_exr_source.width, native_exr_source.height),
        (200, 200)
    );
    assert_eq!(native_exr_source.duration.seconds(), 0.0);
    assert_eq!(
        native_exr
            .item(2)
            .and_then(|item| match &item.kind {
                ItemKind::Composition(comp) =>
                    comp.layers.iter().find(|layer| layer.record.id() == 14),
                _ => None,
            })
            .unwrap()
            .record
            .source_id(),
        1
    );

    let native_wave_bytes = include_bytes!("../../../tests/fixtures/media/audioEnabled.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(native_wave_bytes)),
        "b6b887df9e2b055f4a2aac128b24f510ff116012e9a0e7cd6929c8dfb467f3d2"
    );
    let native_wave = read_project(native_wave_bytes).unwrap();
    let native_wave_source = native_wave
        .item(13)
        .unwrap()
        .media
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(native_wave_source.source_format, *b"WAVE");
    assert_eq!(native_wave_source.kind, MediaKind::Audio);
    assert_eq!(
        (native_wave_source.width, native_wave_source.height),
        (0, 0)
    );
    assert!((native_wave_source.duration.seconds() - 5.943_174_603_174_6).abs() < 1e-12);
    assert_eq!(
        native_wave
            .item(1)
            .and_then(|item| match &item.kind {
                ItemKind::Composition(comp) =>
                    comp.layers.iter().find(|layer| layer.record.id() == 14),
                _ => None,
            })
            .unwrap()
            .record
            .source_id(),
        13
    );

    let mut occurrences = vec![
        image(100, "still-exr"),
        video(101, "shared-mov"),
        audio(102, "shared-mov", 4000),
        audio(103, "voice-wave", 2000),
    ];
    for occurrence in &mut occurrences[1..] {
        occurrence.as_object_mut().unwrap().remove("activeRange");
        occurrence["playback"] = fixture_linear_playback(
            json!({"start":1000,"duration":2000}),
            json!({"start":500,"duration":1000}),
        );
    }
    let doc = document(occurrences, Vec::new());
    let mut sources = BTreeMap::new();
    sources.insert(
        "still-exr".into(),
        resolved(
            "still-exr",
            "media/still.exr",
            NativeSourceFormat::OpenExr,
            [640, 360],
            0,
            NativeFrameRate::integer(0),
            0.0,
        ),
    );
    sources.insert(
        "shared-mov".into(),
        resolved(
            "shared-mov",
            "media/shared.mov",
            NativeSourceFormat::QuickTime,
            [1920, 1080],
            4000,
            NativeFrameRate {
                integer: 23,
                fractional: 63_963,
            },
            48_000.0,
        ),
    );
    sources.insert(
        "voice-wave".into(),
        resolved(
            "voice-wave",
            "media/voice.wav",
            NativeSourceFormat::Wave,
            [0, 0],
            2000,
            NativeFrameRate::integer(0),
            44_100.0,
        ),
    );

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let descriptors = native
        .items
        .iter()
        .filter_map(|item| {
            item.media
                .as_ref()
                .map(|media| (item.id, media.as_ref().unwrap()))
        })
        .collect::<Vec<_>>();
    assert_eq!(descriptors.len(), 3, "{:?}", output.diagnostics);

    let exr = descriptors
        .iter()
        .find(|(_, value)| value.source_format == *b"oEXR")
        .unwrap()
        .1;
    assert_eq!(exr.kind, MediaKind::StillImage);
    assert_eq!((exr.width, exr.height), (640, 360));
    assert_eq!(exr.duration.seconds(), 0.0);
    assert_eq!(
        (
            exr.native_frame_rate.integer,
            exr.native_frame_rate.fractional
        ),
        (0, 0)
    );
    assert_eq!(exr.authored_path, "./media/still.exr");

    let movie = descriptors
        .iter()
        .find(|(_, value)| value.source_format == *b"MOoV")
        .unwrap()
        .1;
    assert_eq!(movie.kind, MediaKind::AudioVideo);
    assert_eq!((movie.width, movie.height), (1920, 1080));
    assert_eq!(movie.duration.seconds(), 4.0);
    assert_eq!(
        (
            movie.native_frame_rate.integer,
            movie.native_frame_rate.fractional
        ),
        (23, 63_963)
    );
    assert_eq!(movie.audio_sample_rate, 48_000.0);
    assert_eq!(movie.authored_path, "./media/shared.mov");

    let wave = descriptors
        .iter()
        .find(|(_, value)| value.source_format == *b"WAVE")
        .unwrap()
        .1;
    assert_eq!(wave.kind, MediaKind::Audio);
    assert_eq!((wave.width, wave.height), (0, 0));
    assert_eq!(wave.duration.seconds(), 2.0);
    assert_eq!(wave.audio_sample_rate, 44_100.0);

    let timeline = layers(&native);
    let video = timeline
        .iter()
        .find(|layer| layer.name.as_ref() == "video-101")
        .unwrap();
    let embedded_audio = timeline
        .iter()
        .find(|layer| layer.name.as_ref() == "audio-102")
        .unwrap();
    assert_eq!(video.record.source_id(), embedded_audio.record.source_id());
    assert_eq!(video.record.start_time_fraction(), (0, 1));
    assert_eq!(video.record.in_point_fraction(), (1999, 4000));
    assert_eq!(video.record.out_point_fraction(), (5999, 4000));
    assert_eq!(video.record.stretch_fraction(), (2, 1));
    assert_eq!(embedded_audio.record.start_time_fraction(), (0, 1));
    assert_eq!(embedded_audio.record.stretch_fraction(), (2, 1));
    assert_eq!(embedded_audio.record.in_point_fraction(), (1, 2));
    assert_eq!(embedded_audio.record.out_point_fraction(), (3, 2));
    assert!(!video.record.flags().audio_enabled);
    assert!(embedded_audio.record.flags().audio_enabled);
    assert!(!embedded_audio.record.flags().enabled);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn emitted_media_paths_exclude_prepared_assets_owned_only_by_omitted_layers() {
    let retained = image(180, "retained-exr");
    let omitted = image(181, "omitted-exr");
    let doc = document(
        vec![retained, omitted],
        vec![constant_entry(
            LayerId::new(181),
            PropType::RectRoundness,
            PropertyValue::Float(12.0),
        )],
    );
    let sources = BTreeMap::from([
        (
            "retained-exr".into(),
            resolved(
                "retained-exr",
                "media/retained.exr",
                NativeSourceFormat::OpenExr,
                [640, 360],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
        (
            "omitted-exr".into(),
            resolved(
                "omitted-exr",
                "media/omitted.exr",
                NativeSourceFormat::OpenExr,
                [640, 360],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
    ]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    assert_eq!(
        output.emitted_media_paths,
        BTreeSet::from([RelativeMediaPath::new("media/retained.exr").unwrap()])
    );
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert_eq!(layers(&native)[0].name.as_ref(), "image-180");
}

#[test]
fn nested_image_luma_matte_retains_both_editable_sources_without_provider_paint() {
    let mut owner = image(20, "archival-photo");
    owner["trackMatte"] = json!({"layer": 21, "mode": "luma"});
    let provider = image(21, "segmentation-gray");
    let mut matte_group = group(24, "Kasparov matte group", vec![owner, provider]);
    matte_group.as_object_mut().unwrap().remove("activeRange");
    // An effectful owner exercises the same native precomposition boundary as
    // the source group. The original custom WGSL remains separately unsupported.
    matte_group["effects"] = json!([{
        "id": 120, "enabled": true,
        "effect": {"type": "glow", "glowThreshold": 20, "glowRadius": 8, "glowIntensity": 0.5}
    }]);
    let doc = document(vec![matte_group], vec![]);
    let sources = BTreeMap::from([
        (
            "archival-photo".into(),
            resolved(
                "archival-photo",
                "media/archival-photo.exr",
                NativeSourceFormat::OpenExr,
                [640, 360],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
        (
            "segmentation-gray".into(),
            resolved(
                "segmentation-gray",
                "media/segmentation-gray.exr",
                NativeSourceFormat::OpenExr,
                [640, 360],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
    ]);
    let output = to_aep_with_media(&doc, &sources).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(24)),
        "{:?}",
        output.diagnostics
    );
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(20)));
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(21)));
    assert_eq!(
        output.emitted_media_paths,
        BTreeSet::from([
            RelativeMediaPath::new("media/archival-photo.exr").unwrap(),
            RelativeMediaPath::new("media/segmentation-gray.exr").unwrap(),
        ])
    );
    let native = read_project(&output.bytes).unwrap();
    let mut seen = false;
    for item in &native.items {
        let ItemKind::Composition(comp) = &item.kind else {
            continue;
        };
        let Some(owner) = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "image-20")
        else {
            continue;
        };
        let provider = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "image-21")
            .unwrap();
        assert_eq!(owner.record.track_matte_type(), 3);
        assert_eq!(owner.record.matte_layer_id(), Some(provider.record.id()));
        assert!(owner.record.flags().enabled);
        assert!(!provider.record.flags().enabled);
        seen = true;
    }
    assert!(
        seen,
        "native image owner and matte provider must share a composition"
    );
}

#[test]
fn hidden_reference_image_retains_editable_source_and_native_eye_switch() {
    let mut hidden = image(180, "reference-exr");
    hidden["isHidden"] = json!(true);
    let visible = rect(&imported(), 181);
    let doc = document(vec![hidden.clone(), visible], Vec::new());
    let sources = BTreeMap::from([(
        "reference-exr".into(),
        resolved(
            "reference-exr",
            "media/reference.exr",
            NativeSourceFormat::OpenExr,
            [640, 360],
            0,
            NativeFrameRate::integer(0),
            0.0,
        ),
    )]);
    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert_eq!(layers(&native)[0].name.as_ref(), "image-180");
    assert!(!layers(&native)[0].record.flags().enabled);
    assert!(layers(&native)[1].record.flags().enabled);
    let source_id = layers(&native)[0].record.source_id();
    assert_ne!(source_id, 0);
    assert_eq!(
        native
            .item(source_id)
            .unwrap()
            .media
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .authored_path,
        "./media/reference.exr"
    );
    // The AEP alias is Adobe-relative; the package manifest key stays bare.
    assert_eq!(
        output.emitted_media_paths,
        BTreeSet::from([RelativeMediaPath::new("media/reference.exr").unwrap()])
    );

    hidden["motionBlur"] = json!(true);
    let blurred = to_aep_with_media(&document(vec![hidden], Vec::new()), &sources).unwrap();
    let native = read_project(&blurred.bytes).unwrap();
    let retained = &layers(&native)[0];
    assert_eq!(retained.name.as_ref(), "image-180");
    assert!(!retained.record.flags().enabled);
    assert!(retained.record.flags().motion_blur);
    assert!(blurred.omitted_layer_ids.is_empty());
    assert_eq!(blurred.emitted_media_paths, output.emitted_media_paths);
}

#[test]
fn image_native_options_keep_screen_and_motion_blur_inside_clocked_scene() {
    // The real native EXR descriptor pins the still interpretation, not the
    // Screen/blur appearance: these explicit FX inputs are structural evidence.
    let fixture = include_bytes!("../../../tests/fixtures/media/footage_not_missing.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(fixture)),
        "692fa37886a98a8deb63addda6da0193d6f9cc4c6725f42e13a424d0e8bd9104"
    );
    let source_project = read_project(fixture).unwrap();
    let descriptor = source_project
        .item(1)
        .unwrap()
        .media
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(descriptor.source_format, *b"oEXR");
    let sources = BTreeMap::from([(
        "native-options-exr".into(),
        resolved(
            "native-options-exr",
            "media/native-options.exr",
            NativeSourceFormat::OpenExr,
            [descriptor.width, descriptor.height],
            0,
            NativeFrameRate::integer(0),
            0.0,
        ),
    )]);
    for (blend, native_blend) in [("normal", 2), ("screen", 6)] {
        for motion_blur in [false, true] {
            let mut still = image(1200048, "native-options-exr");
            still["blendMode"] = json!(blend);
            still["motionBlur"] = json!(motion_blur);
            still["transform"]["position"] = json!([43, 19, 0]);
            still["activeRange"] = json!({"start":1000,"duration":2000});
            let mut sibling = rect(&imported(), 1200049);
            sibling["activeRange"] = json!({"start":0,"duration":30000});
            let mut scene = group(1200000, "clocked image scene", vec![still, sibling]);
            scene.as_object_mut().unwrap().remove("activeRange");
            scene["playback"] = fixture_linear_playback(
                json!({"start":0,"duration":30000}),
                json!({"start":0,"duration":30000}),
            );
            let output = to_aep_with_media(&document(vec![scene], Vec::new()), &sources).unwrap();
            for id in [1200000, 1200048, 1200049] {
                assert!(
                    !output.omitted_layer_ids.contains(&LayerId::new(id)),
                    "{blend}/{motion_blur}: {:?}",
                    output.diagnostics
                );
            }
            let native = read_project(&output.bytes).unwrap();
            let occurrence = layers(&native)
                .iter()
                .find(|layer| layer.name.as_ref() == "clocked image scene")
                .unwrap();
            assert_eq!(occurrence.record.in_point(), Some(0.0));
            assert_eq!(occurrence.record.out_point(), Some(30.0));
            let ItemKind::Composition(comp) =
                &native.item(occurrence.record.source_id()).unwrap().kind
            else {
                panic!("clocked scene must retain its editable precomposition");
            };
            let still = comp
                .layers
                .iter()
                .find(|layer| layer.name.as_ref() == "image-1200048")
                .unwrap();
            assert_eq!(still.record.blend_mode(), native_blend);
            assert_eq!(still.record.flags().motion_blur, motion_blur);
            assert_eq!(
                property_values(still, "ADBE Position"),
                vec![43.0, 19.0, 0.0]
            );
            // Native active endpoints are relative to the layer's start time.
            assert_eq!(still.record.start_time(), Some(1.0));
            assert_eq!(still.record.in_point(), Some(0.0));
            assert_eq!(still.record.out_point(), Some(2.0));
            assert!(
                comp.layers
                    .iter()
                    .any(|layer| layer.name.as_ref() == "Current solid 1200049")
            );
            let media = native
                .item(still.record.source_id())
                .unwrap()
                .media
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            assert_eq!(media.authored_path, "./media/native-options.exr");
            assert_eq!(
                output.emitted_media_paths,
                BTreeSet::from([RelativeMediaPath::new("media/native-options.exr").unwrap()])
            );
        }
    }
}

#[test]
fn multiply_video_keeps_its_editable_footage_and_normal_sibling() {
    // The private S11 scene uses a Multiply print pass over the same source.
    let mut normal = video(12400, "cyclist-mov");
    let mut print_pass = video(12401, "cyclist-mov");
    let active = json!({"start": 1000, "duration": 2000});
    let source = json!({"start": 500, "duration": 2000});
    for layer in [&mut normal, &mut print_pass] {
        layer.as_object_mut().unwrap().remove("activeRange");
        layer["sourceRange"] = source.clone();
        layer["playback"] = fixture_linear_playback(active.clone(), source.clone());
    }
    print_pass["blendMode"] = json!("multiply");
    let mut scene = group(12000, "S11 print pass", vec![print_pass, normal]);
    scene.as_object_mut().unwrap().remove("activeRange");
    let doc = document(vec![scene], Vec::new());
    let sources = BTreeMap::from([(
        "cyclist-mov".into(),
        resolved(
            "cyclist-mov",
            "media/cyclist.mov",
            NativeSourceFormat::QuickTime,
            [1920, 1080],
            4000,
            NativeFrameRate::integer(24),
            0.0,
        ),
    )]);
    let output = to_aep_with_media(&doc, &sources).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(12000)),
        "{:?}",
        output.diagnostics
    );
    assert!(!output.omitted_layer_ids.contains(&LayerId::new(12401)));
    assert_eq!(
        output.emitted_media_paths,
        BTreeSet::from([RelativeMediaPath::new("media/cyclist.mov").unwrap()])
    );
    let native = read_project(&output.bytes).unwrap();
    let pair = native.items.iter().find_map(|item| {
        let ItemKind::Composition(comp) = &item.kind else {
            return None;
        };
        let print_pass = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "video-12401")?;
        let normal = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "video-12400")?;
        Some((print_pass, normal))
    });
    let (print_pass, normal) = pair.expect("both editable videos must share a native composition");
    assert_eq!(print_pass.record.blend_mode(), 5);
    assert_eq!(normal.record.blend_mode(), 2);
    assert_ne!(print_pass.record.source_id(), 0);
    assert_eq!(print_pass.record.source_id(), normal.record.source_id());

    // Screen now has independent P004 AV-layer proof; other unverified
    // media blends remain outside the bounded admission.
    let mut screen = video(12402, "cyclist-mov");
    screen.as_object_mut().unwrap().remove("activeRange");
    screen["sourceRange"] = source.clone();
    screen["playback"] = fixture_linear_playback(active, source);
    screen["blendMode"] = json!("screen");
    let accepted =
        to_aep_with_media(&document(vec![screen.clone()], Vec::new()), &sources).unwrap();
    assert!(!accepted.omitted_layer_ids.contains(&LayerId::new(12402)));
    screen["blendMode"] = json!("difference");
    let rejected = to_aep_with_media(&document(vec![screen], Vec::new()), &sources).unwrap();
    assert!(rejected.omitted_layer_ids.contains(&LayerId::new(12402)));
}

#[test]
fn alpha_matted_video_keeps_its_editable_source_and_shape_provider() {
    // B02's two video counters use sibling Shape alpha guides in their own Groups.
    let mut owner = video(9040, "singer-mov");
    owner.as_object_mut().unwrap().remove("activeRange");
    owner["sourceRange"] = json!({"start": 500, "duration": 2000});
    owner["playback"] = fixture_linear_playback(
        json!({"start": 1000, "duration": 2000}),
        json!({"start": 500, "duration": 2000}),
    );
    owner["trackMatte"] = json!({"layer": 9382, "mode": "alpha"});
    let mut provider = rect(&imported(), 9382);
    provider["name"] = json!("Singer alpha window");
    provider["type"] = json!("Shape");
    provider.as_object_mut().unwrap().remove("rect");
    provider["shape"] = json!({
        "path": {"commands": [
            {"type": "moveTo", "x": 0.0, "y": 0.0},
            {"type": "lineTo", "x": 120.0, "y": 0.0},
            {"type": "lineTo", "x": 120.0, "y": 80.0},
            {"type": "close"}
        ]},
        "fills": [{"paint": {"type": "solid", "color": [1.0, 1.0, 1.0, 1.0]}}],
        "strokes": []
    });
    provider["activeRange"] = json!({"start": 1000, "duration": 2000});
    let mut scene = group(
        9000,
        "B02 singer counter",
        vec![owner.clone(), provider.clone()],
    );
    scene.as_object_mut().unwrap().remove("activeRange");
    let sources = BTreeMap::from([(
        "singer-mov".into(),
        resolved(
            "singer-mov",
            "media/singer.mov",
            NativeSourceFormat::QuickTime,
            [1920, 1080],
            4000,
            NativeFrameRate::integer(24),
            0.0,
        ),
    )]);
    let output = to_aep_with_media(&document(vec![scene], Vec::new()), &sources).unwrap();
    assert!(
        !output.omitted_layer_ids.contains(&LayerId::new(9000)),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let pair = native.items.iter().find_map(|item| {
        let ItemKind::Composition(comp) = &item.kind else {
            return None;
        };
        let video = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "video-9040")?;
        let provider = comp
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Singer alpha window")?;
        Some((video, provider))
    });
    let (video, native_provider) =
        pair.expect("video and shape matte must share a native composition");
    assert_ne!(video.record.source_id(), 0);
    assert_eq!(video.record.track_matte_type(), 1);
    assert_eq!(
        video.record.matte_layer_id(),
        Some(native_provider.record.id())
    );
    assert!(!native_provider.record.flags().enabled);

    // A different matte mode is not admitted by this source-derived profile.
    let mut inverted_alpha_owner = owner;
    inverted_alpha_owner["trackMatte"]["mode"] = json!("alphaInverted");
    let rejected = to_aep_with_media(
        &document(vec![inverted_alpha_owner, provider], Vec::new()),
        &sources,
    )
    .unwrap();
    assert!(rejected.omitted_layer_ids.contains(&LayerId::new(9040)));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn fresh_aep_composes_cover_geometry_user_crop_mask_and_stored_3d_transform() {
    let mut guide = rect(&imported(), 210);
    guide["name"] = json!("authored mask guide");
    guide["rect"]["position"] = json!([30.0, 40.0]);
    guide["rect"]["size"] = json!([120.0, 80.0]);
    let mut layer = image(211, "cover-exr");
    layer["source"] = json!({
        "assetId":"cover-exr", "fit":"cover",
        "sourceRect":{"x":100.0,"y":50.0,"width":400.0,"height":400.0}
    });
    layer["transform"] = json!({
        "anchorPoint":[200.0,100.0], "position":[600.0,300.0,40.0],
        "scale":[120.0,80.0], "rotation":17.0, "rotationX":11.0,
        "rotationY":-9.0, "orientation":[3.0,4.0,5.0], "opacity":75.0
    });
    layer["masks"] = json!([{"id":9001,"mode":"add","layer":210}]);
    let doc = document(vec![guide, layer], Vec::new());
    let sources = BTreeMap::from([(
        "cover-exr".into(),
        resolved(
            "cover-exr",
            "media/cover.exr",
            NativeSourceFormat::OpenExr,
            [1920, 1080],
            0,
            NativeFrameRate::integer(0),
            0.0,
        ),
    )]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let layer = &layers(&native)[0];
    assert!(layer.record.flags().three_d_layer);
    assert_eq!(
        property_values(layer, "ADBE Position"),
        vec![600.0, 300.0, 40.0]
    );
    assert_eq!(
        property_values(layer, "ADBE Orientation"),
        vec![3.0, 4.0, 5.0]
    );
    let anchor = property_values(layer, "ADBE Anchor Point");
    assert!((anchor[0] - 690.0 / 1920.0).abs() < 1e-9);
    assert!((anchor[1] - 135.0 / 1080.0).abs() < 1e-9);
    let scale = property_values(layer, "ADBE Scale");
    assert!((scale[0] - 0.444_444_444_444_444_4).abs() < 1e-12);
    assert!((scale[1] - 0.296_296_296_296_296_3).abs() < 1e-12);

    let mask_parade = root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Mask Parade")
        .unwrap();
    let mask_atoms = runs(mask_parade.1)
        .unwrap()
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Mask Atom")
        .count();
    assert_eq!(
        mask_atoms, 2,
        "authored mask must precede the appended Intersect crop mask"
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn fresh_aep_preserves_affine_static_and_playback_clocks_and_rejects_guessed_boundaries() {
    let mut static_video = video(300, "static-mov");
    static_video["activeRange"] = json!({"start":500,"duration":1000});
    static_video["sourceRange"] = json!({"start":250,"duration":1500});
    static_video["source"]["timeRemap"] = json!(0.75);

    let mut remapped = video(301, "remap-mov");
    remapped["activeRange"] = json!({"start":1000,"duration":1000});
    remapped["sourceRange"] = json!({"start":500,"duration":1000});
    remapped["playback"] = json!({
        "keyframes":[
            {"id":"guard-before","time":0,"value":500,"easing":{"type":"linear"}},
            {"id":"inside-a","time":1250,"value":750,"easing":{"type":"linear"}},
            {"id":"inside-b","time":1750,"value":1000,"easing":{"type":"linear"}},
            {"id":"guard-after","time":3000,"value":1250,"easing":{"type":"linear"}}
        ],
        "before":"inactive", "after":"inactive"
    });

    let mut rejected = video(302, "bad-mov");
    rejected["sourceRange"] = json!({"start":3500,"duration":1000});
    let sibling = rect(&imported(), 303);
    let doc = document(vec![static_video, remapped, rejected, sibling], Vec::new());
    let movie = resolved(
        "static-mov",
        "media/static.mov",
        NativeSourceFormat::QuickTime,
        [1280, 720],
        4000,
        NativeFrameRate::integer(24),
        0.0,
    );
    let sources = BTreeMap::from([
        ("static-mov".into(), movie),
        (
            "remap-mov".into(),
            resolved(
                "remap-mov",
                "media/remap.mov",
                NativeSourceFormat::QuickTime,
                [1280, 720],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
        (
            "bad-mov".into(),
            resolved(
                "bad-mov",
                "media/bad.mov",
                NativeSourceFormat::QuickTime,
                [1280, 720],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
    ]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 3, "{:?}", output.diagnostics);
    let static_layer = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "video-300")
        .unwrap();
    let static_remap = root_runs(&static_layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Time Remapping")
        .unwrap();
    assert!(!static_remap.1.is_empty());
    let playback = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "video-301")
        .unwrap();
    let playback_remap = root_runs(&playback.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Time Remapping")
        .unwrap();
    assert!(!playback_remap.1.is_empty());
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(302))
            && diagnostic
                .message
                .contains("source range is empty or exceeds")
    }));
    assert!(
        layers(&native)
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 303")
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn fresh_aep_sets_layer_and_owner_composition_frame_blending_masters() {
    let mut mixed = video(400, "mix-mov");
    mixed["frameBlending"] = json!(true);
    let mut optical = video(401, "flow-mov");
    optical["frameBlending"] = json!("opticalFlow");
    let doc = document(vec![mixed, optical], Vec::new());
    let sources = BTreeMap::from([
        (
            "mix-mov".into(),
            resolved(
                "mix-mov",
                "media/mix.mov",
                NativeSourceFormat::QuickTime,
                [640, 360],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
        (
            "flow-mov".into(),
            resolved(
                "flow-mov",
                "media/flow.mov",
                NativeSourceFormat::QuickTime,
                [640, 360],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
    ]);
    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let timeline = layers(&native);
    let frame_mix = timeline
        .iter()
        .find(|layer| layer.name.as_ref() == "video-400")
        .unwrap();
    let pixel_motion = timeline
        .iter()
        .find(|layer| layer.name.as_ref() == "video-401")
        .unwrap();
    assert!(frame_mix.record.flags().frame_blending);
    assert!(!frame_mix.record.flags().frame_blending_mode);
    assert!(pixel_motion.record.flags().frame_blending);
    assert!(pixel_motion.record.flags().frame_blending_mode);
    let ItemKind::Composition(composition) = &native.item(1).unwrap().kind else {
        panic!("root composition")
    };
    assert_ne!(composition.record.flags()[1] & 16, 0);
}

fn source_selector_entry(layer: LayerId, keys: Vec<PropertyKeyframe>) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(layer, PropType::MediaSourceAssetId),
        animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn two_source_selector(layer: LayerId) -> AnimationGraphEntry {
    source_selector_entry(
        layer,
        vec![
            PropertyKeyframe::new(
                KeyframeId::new(format!("review-source-a-{}", layer.value())),
                TimeOffset::ZERO,
                PropertyValue::String("variant-a".into()),
                PropertyKeyframeEasing::Linear,
            ),
            PropertyKeyframe::new(
                KeyframeId::new(format!("review-source-b-{}", layer.value())),
                TimeOffset::from_millis(1_000),
                PropertyValue::String("variant-b".into()),
                PropertyKeyframeEasing::Hold,
            ),
        ],
    )
}

fn review_variant_sources() -> BTreeMap<String, media::ResolvedMediaSource> {
    BTreeMap::from([
        (
            "variant-a".into(),
            resolved(
                "variant-a",
                "media/review-variant-a.exr",
                NativeSourceFormat::OpenExr,
                [320, 180],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
        (
            "variant-b".into(),
            resolved(
                "variant-b",
                "media/review-variant-b.exr",
                NativeSourceFormat::OpenExr,
                [320, 180],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
    ])
}

#[test]
fn review_referenced_source_variants_exclude_consumed_selector_from_wrapper_tracks() {
    let source_id = LayerId::new(8_000);
    let mut dependent = rect(&imported(), 8_001);
    dependent["name"] = json!("Review variant dependent");
    dependent["trackMatte"] = json!({"mode":"alpha","layer":source_id.value()});
    let document = document(
        vec![image(source_id.value(), "persisted"), dependent],
        vec![two_source_selector(source_id)],
    );

    let output = to_aep_with_media(&document, &review_variant_sources()).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let wrapper = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "image-8000")
        .expect("referenced source variants retain their wrapper");
    let ItemKind::Composition(precomposition) =
        &native.item(wrapper.record.source_id()).unwrap().kind
    else {
        panic!("referenced variants require a precomposition wrapper")
    };
    assert_eq!(precomposition.layers.len(), 2, "{:?}", output.diagnostics);
    assert_eq!(
        layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == "Review variant dependent")
            .expect("matte consumer is retained")
            .record
            .matte_layer_id(),
        Some(wrapper.record.id())
    );
    assert!(!output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(source_id)
            && diagnostic
                .message
                .contains("animator target without a native editable mapping")
    }));
}

#[test]
fn review_referenced_source_variant_anchor_origin_is_applied_once() {
    let source_id = LayerId::new(8_010);
    let mut source = image(source_id.value(), "persisted");
    source["transform"]["anchorPoint"] = json!([150.0, 90.0]);
    source["transform"]["position"] = json!([500.0, 300.0]);
    let mut dependent = rect(&imported(), 8_011);
    dependent["trackMatte"] = json!({"mode":"alpha","layer":source_id.value()});
    let anchor_x = keyed_entry(
        source_id,
        PropType::AnchorPointX,
        [
            (0, PropertyValue::Float(150.0)),
            (1_000, PropertyValue::Float(170.0)),
        ],
    );
    let document = document(
        vec![source, dependent],
        vec![
            two_source_selector(source_id),
            anchor_x,
            keyed_entry(
                source_id,
                PropType::PositionY,
                [
                    (0, PropertyValue::Float(40.0)),
                    (1_000, PropertyValue::Float(40.0)),
                ],
            ),
        ],
    );

    let output = to_aep_with_media(&document, &review_variant_sources()).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let wrapper = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "image-8010")
        .expect("referenced source wrapper");
    let anchor = super::stroke_keys::numeric(&wrapper.content, "ADBE Anchor Point")
        .expect("animated wrapper anchor");
    assert_eq!(anchor.keyframes.len(), 2);
    // The animated content bounds are [-170, 40]..[170, 220]. Native
    // precomposition Anchor keys are source-dimension fractions, not pixels.
    // In particular, the unkeyed Y must be (90 - 40), not (90 - 2 * 40).
    assert_eq!(
        anchor.keyframes[0].values,
        [320.0 / 340.0, 50.0 / 180.0, 0.0]
    );
    assert_eq!(anchor.keyframes[1].values, [1.0, 50.0 / 180.0, 0.0]);
}

#[test]
fn review_disabled_owner_track_rebases_as_effective_constant_for_source_variants() {
    let layer: Layer = serde_json::from_value(image(8_020, "persisted")).unwrap();
    let mut position = keyed_entry(
        layer.id(),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(10.0)),
            (1_500, PropertyValue::Float(40.0)),
        ],
    );
    let mut animator = position.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = &mut animator
    else {
        panic!("Position X fixture is keyed")
    };
    *enabled = false;
    *disabled_value = Some(PropertyValue::Float(333.0));
    position.animator = PropertyAnimator::from_data(&animator).unwrap();
    let mut occupied = BTreeSet::from([layer.id()]);

    let source_variants::SourceVariantDecision::Ready(plan) = source_variants::plan_layer(
        &layer,
        &[two_source_selector(layer.id()), position],
        Default::default(),
        &mut occupied,
    )
    .unwrap() else {
        panic!("a disabled owner track is an effective constant and needs no clock rebasing")
    };
    assert_eq!(plan.variants.len(), 2);
    for variant in plan.variants {
        assert_eq!(variant.owner_entries.len(), 1);
        assert_eq!(
            variant.owner_entries[0].target,
            PropertyTarget::layer(variant.occurrence_id, PropType::PositionX)
        );
        assert!(matches!(
            variant.owner_entries[0].animator.data(),
            AnimatorData::Constant { value: PropertyValue::Float(value) }
                if *value == 333.0
        ));
    }
}

#[test]
fn review_disabled_source_selector_uses_runtime_visible_asset() {
    let layer: Layer = serde_json::from_value(image(8_030, "persisted")).unwrap();
    let mut selector = two_source_selector(layer.id());
    let mut animator = selector.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = &mut animator
    else {
        panic!("source selector fixture is keyed")
    };
    *enabled = false;
    *disabled_value = Some(PropertyValue::String("variant-b".into()));
    selector.animator = PropertyAnimator::from_data(&animator).unwrap();
    let mut occupied = BTreeSet::from([layer.id()]);

    let source_variants::SourceVariantDecision::Ready(plan) =
        source_variants::plan_layer(&layer, &[selector], Default::default(), &mut occupied)
            .unwrap()
    else {
        panic!("a disabled source selector is its runtime-visible constant")
    };
    assert_eq!(plan.variants.len(), 1);
    assert_eq!(plan.variants[0].occurrence_id, layer.id());
    assert_eq!(plan.variants[0].asset_id.as_str(), "variant-b");
    assert_eq!(
        source_variants::media_requests(&plan).unwrap()[0]
            .asset_id
            .as_str(),
        "variant-b"
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn source_variants_scope_original_identity_remint_occurrences_and_rebase_owned_keys() {
    let layer: Layer = serde_json::from_value(image(500, "persisted-exr")).unwrap();
    let selector = source_selector_entry(
        layer.id(),
        vec![
            PropertyKeyframe::new(
                KeyframeId::new("source-a"),
                TimeOffset::from_millis(0),
                PropertyValue::String("variant-a".into()),
                PropertyKeyframeEasing::Linear,
            ),
            PropertyKeyframe::new(
                KeyframeId::new("source-b"),
                TimeOffset::from_millis(1000),
                PropertyValue::String("variant-b".into()),
                PropertyKeyframeEasing::Hold,
            ),
        ],
    );
    let owned = keyed_entry(
        layer.id(),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(10.0)),
            (1500, PropertyValue::Float(40.0)),
        ],
    );
    let mut occupied = BTreeSet::from([layer.id(), LayerId::new(900)]);
    let source_variants::SourceVariantDecision::Ready(plan) = source_variants::plan_layer(
        &layer,
        &[selector.clone(), owned.clone()],
        source_variants::SourceVariantEligibility::default(),
        &mut occupied,
    )
    .unwrap() else {
        panic!("finite held variants")
    };
    assert_eq!(plan.consumed_source_entries, vec![0]);
    assert_eq!(
        plan.variants
            .iter()
            .map(|variant| variant.occurrence_id.value())
            .collect::<Vec<_>>(),
        vec![500, 901]
    );
    assert_eq!(
        plan.variants
            .iter()
            .map(|variant| variant.asset_id.as_str())
            .collect::<Vec<_>>(),
        vec!["variant-a", "variant-b"]
    );
    assert_eq!(plan.variants[0].owner_entries.len(), 1);
    assert_eq!(plan.variants[1].owner_entries.len(), 1);
    for (index, variant) in plan.variants.iter().enumerate() {
        let AnimatorData::Keyframes { track, .. } = variant.owner_entries[0].animator.data() else {
            panic!("owned key track")
        };
        assert!(track.keyframes().iter().all(|key| {
            key.id()
                .as_str()
                .starts_with(&format!("ae-sv-{}-", variant.occurrence_id.value()))
        }));
        let expected_first = if index == 0 { 0 } else { -1000 };
        assert_eq!(
            track.keyframes()[0].layer_time().as_millis(),
            expected_first
        );
    }

    let variant_document = document(
        vec![image(500, "persisted-exr")],
        vec![selector.clone(), owned.clone()],
    );
    let variant_sources = BTreeMap::from([
        (
            "variant-a".into(),
            resolved(
                "variant-a",
                "media/variant-a.exr",
                NativeSourceFormat::OpenExr,
                [320, 180],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
        (
            "variant-b".into(),
            resolved(
                "variant-b",
                "media/variant-b.exr",
                NativeSourceFormat::OpenExr,
                [320, 180],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
    ]);
    let exported = to_aep_with_media(&variant_document, &variant_sources).unwrap();
    let native = read_project(&exported.bytes).unwrap();
    assert_eq!(
        layers(&native)
            .iter()
            .map(|layer| layer.record.id())
            .collect::<Vec<_>>(),
        vec![500, 501]
    );
    assert_eq!(layers(&native)[0].record.in_point(), Some(0.0));
    assert_eq!(layers(&native)[0].record.out_point(), Some(1.0));
    assert_eq!(layers(&native)[1].record.in_point(), Some(1.0));
    assert_eq!(layers(&native)[1].record.out_point(), Some(2.0));
    assert!(
        native
            .items
            .iter()
            .filter_map(|item| item.media.as_ref())
            .count()
            >= 2
    );

    let constant_entry = AnimationGraphEntry {
        target: PropertyTarget::layer(layer.id(), PropType::MediaSourceAssetId),
        animator: PropertyAnimator::constant(PropertyValue::String("constant-exr".into())).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let mut constant_occupied = BTreeSet::from([layer.id()]);
    let source_variants::SourceVariantDecision::Ready(constant_plan) = source_variants::plan_layer(
        &layer,
        &[constant_entry],
        Default::default(),
        &mut constant_occupied,
    )
    .unwrap() else {
        panic!("constant source override")
    };
    assert_eq!(constant_plan.variants.len(), 1);
    assert_eq!(constant_plan.variants[0].occurrence_id, layer.id());
    assert_eq!(constant_plan.variants[0].asset_id.as_str(), "constant-exr");
    assert_eq!(
        source_variants::media_requests(&constant_plan).unwrap()[0]
            .asset_id
            .as_str(),
        "constant-exr"
    );

    let mut referenced_occupied = BTreeSet::from([layer.id()]);
    let source_variants::SourceVariantDecision::Ready(referenced_plan) =
        source_variants::plan_layer_with_precomposition(
            &layer,
            &[selector, owned],
            source_variants::SourceVariantEligibility {
                referenced_as_matte: true,
                ..Default::default()
            },
            &mut referenced_occupied,
            Some(source_variants::SourceVariantPrecompositionInput {
                source_dimensions: [320, 180],
                all_time_bounds: source_variants::SourceVariantBounds {
                    min: [-10.25, 20.5],
                    max: [100.25, 220.5],
                },
            }),
        )
        .unwrap()
    else {
        panic!("referenced variants need one identity wrapper")
    };
    let source_variants::SourceVariantPublication::Precomposition(handoff) =
        referenced_plan.publication
    else {
        panic!("referenced variants must publish through a precomposition")
    };
    assert_eq!(handoff.wrapper_layer_id, layer.id());
    assert!(
        referenced_plan
            .variants
            .iter()
            .all(|variant| variant.occurrence_id != layer.id())
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn source_variant_unsafe_values_dependencies_and_malformed_identity_discovery_fail_closed() {
    let layer: Layer = serde_json::from_value(image(600, "persisted-exr")).unwrap();
    let unsafe_selector = source_selector_entry(
        layer.id(),
        vec![PropertyKeyframe::new(
            KeyframeId::new("unsafe"),
            TimeOffset::ZERO,
            PropertyValue::String("../escape".into()),
            PropertyKeyframeEasing::Hold,
        )],
    );
    let mut occupied = BTreeSet::from([layer.id()]);
    assert!(matches!(
        source_variants::plan_layer(&layer, &[unsafe_selector], Default::default(), &mut occupied),
        Err(source_variants::SourceVariantPlanError::UnsafeAssetIdentity { layer_id, .. }) if layer_id == layer.id()
    ));

    let mut dependent = source_selector_entry(
        layer.id(),
        vec![PropertyKeyframe::new(
            KeyframeId::new("safe"),
            TimeOffset::ZERO,
            PropertyValue::String("safe-asset".into()),
            PropertyKeyframeEasing::Hold,
        )],
    );
    dependent
        .dependencies
        .push(PropertyTarget::layer(layer.id(), PropType::PositionX));
    let source_variants::SourceVariantDecision::Unsupported { layer_id, reason } =
        source_variants::plan_layer(&layer, &[dependent], Default::default(), &mut occupied)
            .unwrap()
    else {
        panic!("dependent selector must not be sampled")
    };
    assert_eq!(layer_id, layer.id());
    assert!(reason.contains("shared graph/clock"));

    let mut value = imported();
    let duplicate = value["composition"]["layers"][0].clone();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let duplicate_document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    assert!(matches!(
        source_variants::discover_document_media_requests(&duplicate_document),
        Err(source_variants::SourceVariantPlanError::ReferenceAnalysis(
            _
        ))
    ));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn unsupported_selector_skips_hash_valid_malformed_stale_asset_and_keeps_sibling() {
    let root = tempfile::tempdir().unwrap();
    let stale = root.path().join("stale.exr");
    let selected = root.path().join("selected.exr");
    fs::write(&stale, b"hash-valid but malformed OpenEXR").unwrap();
    fs::write(&selected, synthetic_exr(64, 64)).unwrap();

    let source_layer_id = LayerId::new(800);
    let dependency = PropertyTarget::layer(source_layer_id, PropType::PositionX);
    let producer = AnimationGraphEntry {
        target: dependency.clone(),
        animator: PropertyAnimator::constant(PropertyValue::Float(123.0)).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let mut selector = AnimationGraphEntry {
        target: PropertyTarget::layer(source_layer_id, PropType::MediaSourceAssetId),
        animator: PropertyAnimator::constant(PropertyValue::String("selected-exr".to_owned()))
            .unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    selector.dependencies.push(dependency);
    let sibling_id = 801_u32;
    let document = document(
        vec![
            image(source_layer_id.value(), "stale-exr"),
            rect(&imported(), u64::from(sibling_id)),
        ],
        vec![producer, selector],
    );
    let builder = TesseractFileBuilder::try_new(document)
        .unwrap()
        .add_asset("stale-exr", &stale, AssetKind::Image)
        .unwrap()
        .add_asset("selected-exr", &selected, AssetKind::Image)
        .unwrap();
    let input = root.path().join("input.tsrct");
    drop(builder.write(&input).unwrap());
    let output = root.path().join("output");

    let checked = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    let written = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();

    assert_eq!(checked, written);
    assert!(written.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("dependent source selection requires a shared graph/clock plan")
    }));
    assert!(
        !output.join("media").exists(),
        "no unsupported selector asset should be published"
    );
    assert_eq!(
        written.artifacts.len(),
        1,
        "omitted media must not be reported"
    );
    assert!(
        written
            .artifacts
            .iter()
            .all(|artifact| output.join(&artifact.path).is_file())
    );
    let native = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(composition) = &native.item(1).unwrap().kind else {
        panic!("root composition")
    };
    assert_eq!(composition.layers.len(), 1, "{:?}", written.diagnostics);
    assert_eq!(composition.layers[0].record.id(), sibling_id);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn fresh_takeover_orders_clip_then_authored_transform_then_slide_and_rejects_video_tail() {
    let mut takeover = image(700, "takeover-exr");
    takeover["name"] = json!("Top takeover");
    takeover["activeRange"] = json!({"start":1000,"duration":1880});
    takeover["placement"] = json!("topHalf");
    takeover["source"] = json!({
        "assetId":"takeover-exr", "fit":"cover",
        "sourceRect":{"x":0.0,"y":0.0,"width":1920.0,"height":540.0}
    });
    takeover["transform"]["position"] = json!([700.0, 320.0]);
    takeover["transform"]["rotation"] = json!(13.0);
    let mut video_takeover = video(701, "takeover-mov");
    video_takeover["placement"] = json!("bottomHalf");
    let sibling = rect(&imported(), 702);
    let doc = document(vec![takeover, video_takeover, sibling], Vec::new());
    let sources = BTreeMap::from([
        (
            "takeover-exr".into(),
            resolved(
                "takeover-exr",
                "media/takeover.exr",
                NativeSourceFormat::OpenExr,
                [1920, 1080],
                0,
                NativeFrameRate::integer(0),
                0.0,
            ),
        ),
        (
            "takeover-mov".into(),
            resolved(
                "takeover-mov",
                "media/takeover.mov",
                NativeSourceFormat::QuickTime,
                [1920, 1080],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
    ]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let timeline = layers(&native);
    assert_eq!(timeline.len(), 3, "{:?}", output.diagnostics);
    let viewport = timeline
        .iter()
        .find(|layer| layer.name.as_ref() == "Media Takeover Viewport")
        .unwrap();
    let slide = timeline
        .iter()
        .find(|layer| layer.name.as_ref() == "Media Takeover Slide")
        .unwrap();
    assert_eq!(viewport.record.parent_id(), slide.record.id());
    assert_eq!(viewport.record.out_point(), Some(3.0));
    assert_eq!(
        property_values(viewport, "ADBE Position"),
        vec![700.0, 320.0, 0.0]
    );
    assert_eq!(property_values(viewport, "ADBE Rotate Z"), vec![13.0]);
    let ItemKind::Composition(clipped) = &native.item(viewport.record.source_id()).unwrap().kind
    else {
        panic!("takeover clip composition")
    };
    assert_eq!((clipped.width, clipped.height), (1920, 540));
    assert_eq!(clipped.layers.len(), 1);
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(701))
            && diagnostic
                .message
                .contains("Video takeover tail is not implemented")
    }));
    assert!(
        timeline
            .iter()
            .any(|layer| layer.name.as_ref() == "Current solid 702")
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn nested_frame_blending_sets_owning_and_ancestor_composition_masters() {
    let mut blended = video(730, "nested-flow-mov");
    blended["activeRange"] = json!({"start":0,"duration":2000});
    blended["sourceRange"] = json!({"start":0,"duration":2000});
    blended["frameBlending"] = json!("opticalFlow");
    let inner = group(731, "Frame Blend Owner", vec![blended]);
    let outer = group(732, "Frame Blend Ancestor", vec![inner]);
    let doc = document(vec![outer], Vec::new());
    let sources = BTreeMap::from([(
        "nested-flow-mov".into(),
        resolved(
            "nested-flow-mov",
            "media/nested-flow.mov",
            NativeSourceFormat::QuickTime,
            [1280, 720],
            4000,
            NativeFrameRate::integer(24),
            0.0,
        ),
    )]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let compositions = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some((item.name.as_str(), composition.as_ref())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(compositions.len(), 3, "{:?}", output.diagnostics);
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("root composition")
    };
    assert_ne!(root.record.flags()[1] & 16, 0, "root ancestor master");
    for expected in ["Frame Blend Ancestor", "Frame Blend Owner"] {
        let composition = compositions
            .iter()
            .find(|(name, _)| name.contains(expected))
            .unwrap_or_else(|| panic!("missing {expected} composition"))
            .1;
        assert_ne!(
            composition.record.flags()[1] & 16,
            0,
            "{expected} must own/propagate the frame-blending master"
        );
    }
    let owner = compositions
        .iter()
        .find(|(name, _)| name.contains("Frame Blend Owner"))
        .unwrap()
        .1;
    assert_eq!(owner.layers.len(), 1);
    assert!(owner.layers[0].record.flags().frame_blending);
    assert!(owner.layers[0].record.flags().frame_blending_mode);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn constant_source_override_fresh_export_preserves_referenced_original_identity() {
    let source_id = LayerId::new(740);
    let mut source = image(source_id.value(), "persisted-exr");
    source["name"] = json!("Referenced constant source");
    let mut dependent = rect(&imported(), 741);
    dependent["name"] = json!("Parented sibling");
    dependent["parent"] = json!(source_id.value());
    let selector = AnimationGraphEntry {
        target: PropertyTarget::layer(source_id, PropType::MediaSourceAssetId),
        animator: PropertyAnimator::constant(PropertyValue::String("override-exr".into())).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let doc = document(vec![source, dependent], vec![selector]);
    let sources = BTreeMap::from([(
        "override-exr".into(),
        resolved(
            "override-exr",
            "media/override.exr",
            NativeSourceFormat::OpenExr,
            [640, 360],
            0,
            NativeFrameRate::integer(0),
            0.0,
        ),
    )]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let source_layer = layers(&native)
        .iter()
        .find(|layer| layer.record.id() == 740)
        .unwrap();
    let dependent_layer = layers(&native)
        .iter()
        .find(|layer| layer.record.id() == 741)
        .unwrap();
    assert_eq!(dependent_layer.record.parent_id(), 740);
    assert_eq!(source_layer.name.as_ref(), "Referenced constant source");
    let media = native
        .item(source_layer.record.source_id())
        .unwrap()
        .media
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(media.authored_path, "media/override.exr");
    assert!(
        !native
            .items
            .iter()
            .filter_map(|item| item.media.as_ref())
            .any(|media| {
                media
                    .as_ref()
                    .is_ok_and(|media| media.authored_path.contains("persisted"))
            })
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn referenced_hold_variants_fresh_export_use_original_wrapper_and_reminted_clocked_children() {
    let source_id = LayerId::new(750);
    let mut guide = rect(&imported(), 751);
    guide["name"] = json!("Variant wrapper mask guide");
    let mut source = video(source_id.value(), "persisted-mov");
    source["name"] = json!("Referenced switched video");
    source.as_object_mut().unwrap().remove("activeRange");
    source["sourceRange"] = json!({"start":500,"duration":2000});
    source["playback"] = fixture_linear_playback(
        json!({"start":1000,"duration":2000}),
        json!({"start":500,"duration":2000}),
    );
    source["transform"] = json!({
        "anchorPoint":[100.0,50.0], "position":[420.0,240.0],
        "scale":[90.0,110.0], "rotation":12.0, "opacity":65.0
    });
    source["masks"] = json!([{"id":9751,"mode":"add","layer":751}]);
    let mut dependent = rect(&imported(), 752);
    dependent["name"] = json!("Wrapper-parented sibling");
    dependent["parent"] = json!(source_id.value());
    let selector = source_selector_entry(
        source_id,
        vec![
            PropertyKeyframe::new(
                KeyframeId::new("held-a"),
                TimeOffset::from_millis(0),
                PropertyValue::String("variant-a-mov".into()),
                PropertyKeyframeEasing::Linear,
            ),
            PropertyKeyframe::new(
                KeyframeId::new("held-b"),
                TimeOffset::from_millis(1000),
                PropertyValue::String("variant-b-mov".into()),
                PropertyKeyframeEasing::Hold,
            ),
        ],
    );
    let doc = document(vec![guide, source, dependent], vec![selector]);
    let sources = BTreeMap::from([
        (
            "variant-a-mov".into(),
            resolved(
                "variant-a-mov",
                "media/variant-a.mov",
                NativeSourceFormat::QuickTime,
                [1280, 720],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
        (
            "variant-b-mov".into(),
            resolved(
                "variant-b-mov",
                "media/variant-b.mov",
                NativeSourceFormat::QuickTime,
                [1280, 720],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        ),
    ]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let wrapper = layers(&native)
        .iter()
        .find(|layer| layer.record.id() == 750)
        .unwrap();
    assert_eq!(wrapper.name.as_ref(), "Referenced switched video");
    assert_eq!(
        property_values(wrapper, "ADBE Position"),
        vec![420.0, 240.0, 0.0]
    );
    assert_eq!(property_values(wrapper, "ADBE Opacity"), vec![0.65]);
    assert_eq!(mask_atom_count(wrapper), 1);
    assert_eq!(
        layers(&native)
            .iter()
            .find(|layer| layer.record.id() == 752)
            .unwrap()
            .record
            .parent_id(),
        750
    );

    let ItemKind::Composition(precomposition) =
        &native.item(wrapper.record.source_id()).unwrap().kind
    else {
        panic!("original source identity must wrap reminted variants")
    };
    assert_eq!(precomposition.layers.len(), 2, "{:?}", output.diagnostics);
    assert!(
        precomposition
            .layers
            .iter()
            .all(|layer| layer.record.id() != 750)
    );
    assert_eq!(
        precomposition.layers[0].record.in_point_fraction(),
        (999, 2000)
    );
    assert_eq!(
        precomposition.layers[0].record.out_point_fraction(),
        (2999, 2000)
    );
    assert_eq!(
        precomposition.layers[1].record.in_point_fraction(),
        (2999, 2000)
    );
    assert_eq!(
        precomposition.layers[1].record.out_point_fraction(),
        (4999, 2000)
    );
    for child in &precomposition.layers {
        assert_eq!(child.record.start_time_fraction(), (-1, 2));
        assert_eq!(child.record.stretch_fraction(), (1, 1));
        assert_eq!(property_values(child, "ADBE Opacity"), vec![1.0]);
        assert_eq!(mask_atom_count(child), 0);
    }
    let child_paths = precomposition
        .layers
        .iter()
        .map(|layer| {
            native
                .item(layer.record.source_id())
                .unwrap()
                .media
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .authored_path
                .as_str()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        child_paths,
        vec!["./media/variant-a.mov", "./media/variant-b.mov"]
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn source_variant_ai_and_segment_semantic_references_are_concretely_rejected() {
    let source_id = LayerId::new(760);
    let source = image(source_id.value(), "persisted-exr");
    let selector = source_selector_entry(
        source_id,
        vec![
            PropertyKeyframe::new(
                KeyframeId::new("a"),
                TimeOffset::from_millis(0),
                PropertyValue::String("variant-a".into()),
                PropertyKeyframeEasing::Linear,
            ),
            PropertyKeyframe::new(
                KeyframeId::new("b"),
                TimeOffset::from_millis(1000),
                PropertyValue::String("variant-b".into()),
                PropertyKeyframeEasing::Hold,
            ),
        ],
    );
    let bounds = BTreeMap::from([(
        source_id,
        source_variants::SourceVariantPrecompositionInput {
            source_dimensions: [640, 360],
            all_time_bounds: source_variants::SourceVariantBounds {
                min: [0.0, 0.0],
                max: [640.0, 360.0],
            },
        },
    )]);

    let mut segment_value = imported();
    segment_value["composition"]["layers"] = json!([source.clone()]);
    segment_value["composition"]["segmentLayerIds"] = json!([source_id.value()]);
    segment_value["composition"]["dynamics"] = json!({"entries":[selector.clone()]});
    let segment_document = EditableFxCompositionDocument::from_json_value(segment_value).unwrap();
    let segment_plan = source_variants::preflight_document(&segment_document, &bounds).unwrap();
    let source_variants::SourceVariantDecision::Unsupported { reason, .. } =
        &segment_plan.decisions[&source_id]
    else {
        panic!("segment source variant must be rejected")
    };
    assert!(reason.contains("segment identity"));

    let ai_edit = json!({
        "type":"AiEdit", "id":761, "name":"AI owner",
        "activeRange":{"start":0,"duration":2000},
        "styleId":"style", "sourceLayerId":source_id.value(),
        "layers":[source]
    });
    let mut ai_value = imported();
    ai_value["composition"]["layers"] = json!([ai_edit]);
    ai_value["composition"]["dynamics"] = json!({"entries":[selector]});
    let ai_document = EditableFxCompositionDocument::from_json_value(ai_value).unwrap();
    let ai_plan = source_variants::preflight_document(&ai_document, &bounds).unwrap();
    let source_variants::SourceVariantDecision::Unsupported { reason, .. } =
        &ai_plan.decisions[&source_id]
    else {
        panic!("AI source variant must be rejected")
    };
    assert!(reason.contains("AI Edit source"));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn explicit_legacy_media_positive_case_exports_resolved_current_values() {
    let mut legacy_transform = transform();
    legacy_transform["anchorPoint"] = json!([16.0, 12.0]);
    legacy_transform["position"] = json!([320.0, 180.0]);
    legacy_transform["scale"] = json!([125.0, 80.0]);
    legacy_transform["rotation"] = json!(-7.0);
    legacy_transform["opacity"] = json!(55.0);
    let legacy = json!({
        "type":"Media", "id":770, "name":"Explicit legacy image", "parent":null,
        "activeRange":{"start":250,"duration":1500}, "transform":legacy_transform,
        "source":{
            "assetId":"legacy-positive-exr", "kind":"image", "fit":"stretch",
            "sourceRect":{"x":10.0,"y":20.0,"width":320.0,"height":180.0}
        }
    });
    let doc = document(vec![legacy], Vec::new());
    let sources = BTreeMap::from([(
        "legacy-positive-exr".into(),
        resolved(
            "legacy-positive-exr",
            "media/legacy-positive.exr",
            NativeSourceFormat::OpenExr,
            [640, 360],
            0,
            NativeFrameRate::integer(0),
            0.0,
        ),
    )]);

    let output = to_aep_with_media(&doc, &sources).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    let layer = &layers(&native)[0];
    let media = native
        .item(layer.record.source_id())
        .unwrap()
        .media
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(media.source_format, *b"oEXR");
    assert_eq!((media.width, media.height), (640, 360));
    assert_eq!(media.authored_path, "media/legacy-positive.exr");
    assert_eq!(layer.record.in_point(), Some(0.25));
    assert_eq!(layer.record.out_point(), Some(1.75));
    assert_eq!(
        property_values(layer, "ADBE Position"),
        vec![320.0, 180.0, 0.0]
    );
    assert_eq!(property_values(layer, "ADBE Rotate Z"), vec![-7.0]);
    assert_eq!(property_values(layer, "ADBE Opacity"), vec![0.55]);
}

#[test]
fn video_quantized_intrinsic_duration_keeps_exact_source_eof() {
    // Original Worlds 32201 has 241 frames at 24 fps but persisted 10041 ms.
    // This synthetic assertion isolates its duration identity and authored keys.
    let mut source = resolved(
        "movie",
        "media/movie.mov",
        NativeSourceFormat::QuickTime,
        [320, 180],
        10_042,
        NativeFrameRate::integer(24),
        0.0,
    );
    source.duration_native_ticks = Some(246_784);
    source.duration_millis_floor = 10_041;
    for intrinsic in [10_041, 10_042] {
        let mut owner = video(32201, "movie");
        owner.as_object_mut().unwrap().remove("activeRange");
        owner["sourceIntrinsicDuration"] = json!(intrinsic);
        owner["sourceRange"] = json!({"start": 0, "duration": intrinsic});
        owner["playback"] = fixture_remapped_playback(
            json!({"start": 1000, "duration": 2000}),
            json!({"keyframes": [
                {"id": "first", "time": 1000, "value": 0, "easing": {"type": "linear"}},
                {"id": "last", "time": 3000, "value": 5958, "easing": {"type": "linear"}}
            ], "before": "inactive", "after": "inactive"}),
        );
        let doc = document(vec![owner], vec![]);
        let spec = media::lower(&doc.composition().layers()[0], &source, doc.dimensions()).unwrap();
        assert_eq!(spec.source.duration_native_ticks, Some(246_784));
        assert_eq!(spec.source.duration_millis, 10_042);
        let output = to_aep_with_media(
            &doc,
            &BTreeMap::from([("movie".to_owned(), source.clone())]),
        )
        .unwrap();
        let native = read_project(&output.bytes).unwrap();
        let media = native
            .items
            .iter()
            .find_map(|item| {
                item.native_media
                    .as_ref()
                    .and_then(|media| media.as_ref().ok())
            })
            .unwrap();
        assert_eq!(media.duration.seconds(), 241.0 / 24.0);
    }
}

fn canonical_video(id: u64, asset: &str) -> Value {
    let mut owner = video(id, asset);
    let active = owner
        .as_object_mut()
        .unwrap()
        .remove("activeRange")
        .unwrap();
    owner["playback"] = fixture_linear_playback(active, owner["sourceRange"].clone());
    owner
}

#[test]
fn shared_media_options_retain_movie_matte_in_root_and_nested_pairs() {
    for (nested, animated) in [(false, false), (true, false), (true, true)] {
        let mut owner = canonical_video(32201, "rgb");
        owner["trackMatte"] = json!({"mode":"alpha", "layer":32202});
        let mut provider = canonical_video(32202, "matte");
        let mut inputs = vec![owner.clone(), provider.clone()];
        if nested {
            owner["parent"] = json!(32280);
            provider["parent"] = json!(32280);
            let mut wrapper = group(32280, "Registered movie pair", vec![owner, provider]);
            let range = wrapper
                .as_object_mut()
                .unwrap()
                .remove("activeRange")
                .unwrap();
            wrapper["playback"] = fixture_linear_playback(range.clone(), range);
            inputs = vec![wrapper];
        }
        let entries = if animated {
            vec![keyed_entry(
                LayerId::new(32201),
                PropType::PositionX,
                [
                    (0, PropertyValue::Float(0.0)),
                    (1_000, PropertyValue::Float(100.0)),
                ],
            )]
        } else {
            vec![]
        };
        let doc = document(inputs, entries);
        let sources = BTreeMap::from([
            (
                "rgb".to_owned(),
                resolved(
                    "rgb",
                    "media/rgb.mov",
                    NativeSourceFormat::QuickTime,
                    [320, 180],
                    4000,
                    NativeFrameRate::integer(24),
                    0.0,
                ),
            ),
            (
                "matte".to_owned(),
                resolved(
                    "matte",
                    "media/matte.mov",
                    NativeSourceFormat::QuickTime,
                    [320, 180],
                    4000,
                    NativeFrameRate::integer(24),
                    0.0,
                ),
            ),
        ]);
        let output = to_aep_with_media(&doc, &sources).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let pair = native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .find(|comp| {
                comp.layers
                    .iter()
                    .any(|layer| layer.name.as_ref() == "video-32201")
            })
            .expect("RGB movie remains editable");
        let rgb = pair
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "video-32201")
            .unwrap();
        let matte = pair
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "video-32202")
            .unwrap();
        assert_eq!(rgb.record.matte_layer_id(), Some(matte.record.id()));
        assert_eq!(rgb.record.track_matte_type(), 1);
        assert_ne!(rgb.record.source_id(), 0);
        assert_ne!(matte.record.source_id(), 0);
        assert!(
            output.omitted_layer_ids.is_empty(),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn retimed_video_intrinsic_quantization_rejects_neighboring_durations() {
    let mut source = resolved(
        "movie",
        "media/movie.mov",
        NativeSourceFormat::QuickTime,
        [320, 180],
        4000,
        NativeFrameRate::integer(24),
        0.0,
    );
    source.duration_native_ticks = Some(98_304);
    // Main admits finite linear selections inside both source domains. Keep
    // this strict intrinsic-duration guard on an authored remap instead.
    let mut remapped = canonical_video(32001, "movie");
    remapped["playback"] = fixture_remapped_playback(
        json!({"start":1000,"duration":2000}),
        json!({"keyframes":[
            {"id":"first","time":1000,"value":500,"easing":{"type":"linear"}},
            {"id":"last","time":3000,"value":1500,"easing":{"type":"linear"}}
        ],"before":"inactive","after":"inactive"}),
    );
    for (intrinsic, accepted) in [(3999, false), (4000, true), (4001, false)] {
        let mut owner = remapped.clone();
        owner["sourceIntrinsicDuration"] = json!(intrinsic);
        let doc = document(vec![owner], vec![]);
        assert_eq!(
            media::lower(&doc.composition().layers()[0], &source, doc.dimensions()).is_ok(),
            accepted
        );
    }
    source.duration_native_ticks = Some(246_784);
    source.duration_millis = 10_042;
    source.duration_millis_floor = 10_041;
    for intrinsic in [10_040, 10_043] {
        let mut owner = remapped.clone();
        owner["sourceIntrinsicDuration"] = json!(intrinsic);
        let doc = document(vec![owner], vec![]);
        assert!(media::lower(&doc.composition().layers()[0], &source, doc.dimensions()).is_err());
    }
}

#[test]
fn ordinary_ceil_full_asset_linear_admission_is_preserved() {
    let mut source = resolved(
        "movie",
        "media/movie.mov",
        NativeSourceFormat::QuickTime,
        [320, 180],
        10_042,
        NativeFrameRate::integer(24),
        0.0,
    );
    source.duration_native_ticks = Some(246_784);
    let mut owner = canonical_video(32001, "movie");
    owner["sourceIntrinsicDuration"] = json!(10_042);
    owner["sourceRange"] = json!({"start":0,"duration":10_042});
    owner["playback"] = fixture_linear_playback(
        json!({"start":0,"duration":10_042}),
        owner["sourceRange"].clone(),
    );
    let doc = document(vec![owner], vec![]);
    let spec = media::lower(&doc.composition().layers()[0], &source, doc.dimensions()).unwrap();
    assert_eq!(spec.source.duration_native_ticks, Some(246_784));
}

#[test]
fn exact_quicktime_source_tick_overflow_omits_only_owner() {
    let ticks = i32::MAX as u64 + 1;
    let millis = (ticks * 1_000).div_ceil(24_576);
    let mut source = resolved(
        "movie",
        "media/movie.mov",
        NativeSourceFormat::QuickTime,
        [320, 180],
        millis,
        NativeFrameRate::integer(24),
        0.0,
    );
    source.duration_native_ticks = Some(ticks);
    let mut owner = canonical_video(32001, "movie");
    owner["sourceIntrinsicDuration"] = json!(millis);
    let doc = document(vec![owner, rect(&imported(), 32002)], vec![]);
    let output = to_aep_with_media(&doc, &BTreeMap::from([("movie".into(), source)])).unwrap();
    assert_eq!(
        output.omitted_layer_ids,
        BTreeSet::from([LayerId::new(32001)])
    );
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1);
}
